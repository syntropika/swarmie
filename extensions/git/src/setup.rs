//! Explicit project setup commands for extension-managed worktrees.
use crate::{
    GitExtension,
    repository::{git, utf8},
    required,
};
use swarmie::{Error, Result};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

pub fn validate(p: &Value) -> Result<Value> {
    let commands = p["commands"].as_array().ok_or_else(|| {
        Error::Invalid("commands must be an array of program/args objects".into())
    })?;
    if commands.len() > 8 {
        return Err(Error::Invalid(
            "at most eight setup commands are allowed".into(),
        ));
    }
    for command in commands {
        let program = required(command, "program")?;
        if program.len() > 4096 || program.contains('\0') {
            return Err(Error::Invalid("invalid setup executable".into()));
        }
        let args = command["args"]
            .as_array()
            .ok_or_else(|| Error::Invalid("setup args must be an array".into()))?;
        if args.len() > 128
            || args.iter().any(|a| {
                a.as_str()
                    .is_none_or(|s| s.len() > 8192 || s.contains('\0'))
            })
        {
            return Err(Error::Invalid("invalid setup arguments".into()));
        }
    }
    Ok(json!({"commands":commands,"submodules":p["submodules"].as_bool().unwrap_or(false)}))
}
pub async fn run(extension: &GitExtension, root: &Path, p: &Value) -> Result<Value> {
    let tree = extension.tree(root, p).await?;
    let path = tokio::fs::canonicalize(&tree.path).await?;
    if tree.is_main || path.parent() != Some(extension.worktree_root.as_path()) {
        return Err(Error::Invalid(
            "setup is restricted to extension-managed worktrees".into(),
        ));
    }
    let config = extension
        .setups
        .read()
        .await
        .get(&root.to_string_lossy().to_string())
        .cloned()
        .unwrap_or(json!({"commands":[],"submodules":false}));
    let mut receipts = vec![];
    if config["submodules"] == true {
        git(
            &path,
            &["submodule", "update", "--init", "--recursive"],
            false,
        )
        .await?;
        receipts.push(json!({"program":"git","completed":true}));
    }
    for value in config["commands"].as_array().unwrap() {
        let program = required(value, "program")?;
        let args = value["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>();
        let mut command = Command::new(program);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .args(args)
            .current_dir(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let _group = ProcessGroup(child.id());
        let mut out = child.stdout.take().unwrap().take(1024 * 1024 + 1);
        let mut err = child.stderr.take().unwrap().take(1024 * 1024 + 1);
        let result=tokio::time::timeout(Duration::from_secs(120),async {
            let mut stdout=vec![];let mut stderr=vec![];
            let (a,b)=tokio::join!(out.read_to_end(&mut stdout),err.read_to_end(&mut stderr));a?;b?;
            if stdout.len()>1024*1024||stderr.len()>1024*1024{return Err(Error::Invalid("setup output exceeded 1 MiB".into()));}
            let status=child.wait().await?;
            if !status.success(){return Err(Error::Provider(format!("setup {program}: {}",String::from_utf8_lossy(&stderr))));}
            Ok(json!({"program":program,"completed":true,"stdout":utf8(&stdout)?,"stderr":utf8(&stderr)?}))
        }).await.map_err(|_|Error::Timeout(format!("setup {program}")))?;
        if result.is_err() {
            let _ = child.kill().await;
        }
        receipts.push(result?);
    }
    Ok(json!({"completed":true,"receipts":receipts}))
}

struct ProcessGroup(Option<u32>);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0 {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}
