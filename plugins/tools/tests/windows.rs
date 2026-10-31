#![cfg(all(feature = "coding", windows))]

use std::{
    io::{Read as _, Write as _},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::{Command, Stdio},
    sync::mpsc::channel,
    time::{Duration, Instant},
};

use futures::{FutureExt, executor::block_on};
use swarmie_tools::{
    ExitStatus, FilesChanged, Tool, ToolContext,
    coding::{
        Edit, EditArgs, Live, OutputLimit, Replacement, Shell, ShellArgs, Spill,
        fs::write_atomic,
        process::{Ended, KillOnDrop, Launcher, Process, Spawn},
        workspace::{Local, Workspace},
    },
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};

// The test executable is also a child fixture, so process tests need no
// external runtime, console, shell profile or administrator privileges.
#[test]
#[ignore = "subprocess fixture invoked explicitly by the process tests"]
fn process_fixture() {
    let mode = std::env::var("SWARMIE_TEST_PROCESS").unwrap();
    if mode == "io" {
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input).unwrap();
        println!("OUT:{input}");
        eprintln!("ERR:ready");
        return;
    }
    if mode == "leaf" {
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "process_fixture", "--ignored", "--nocapture"])
        .env("SWARMIE_TEST_PROCESS", "leaf")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    println!("CHILD:{}", child.id());
    std::io::stdout().flush().unwrap();
    // The job owns this child even when the leader exits without waiting.
    if mode == "leave" {
        drop(child);
    } else {
        child.wait().unwrap();
    }
}

fn fixture(mode: &str, how: Spawn) -> Process {
    Launcher::default()
        .with_env("SWARMIE_TEST_PROCESS", mode)
        .spawn(
            std::env::current_exe().unwrap(),
            ["--exact", "process_fixture", "--ignored", "--nocapture"],
            how,
        )
        .unwrap()
}

fn descendant(process: &Process) -> OwnedHandle {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let line = process
            .lines()
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        if let Some(pid) = line.text().trim().strip_prefix("CHILD:") {
            let pid = pid.parse().unwrap();
            // SAFETY: request only synchronization access and pin the exact
            // descendant before cancellation; no PID lookup follows termination.
            let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
            assert!(!raw.is_null(), "{}", std::io::Error::last_os_error());
            // SAFETY: successful OpenProcess transfers a unique owned handle.
            return unsafe { OwnedHandle::from_raw_handle(raw) };
        }
    }
}

fn settled(member: &OwnedHandle) {
    // SAFETY: this handle remains owned and identifies the original descendant.
    assert_eq!(
        unsafe { WaitForSingleObject(member.as_raw_handle(), 5000) },
        0
    );
}

#[test]
fn input_and_both_output_streams_work_without_a_console() {
    let output = fixture(
        "io",
        Spawn {
            stdin: Some("hello 世界".as_bytes().to_vec()),
            merge_stderr: false,
        },
    )
    .output(Some(Duration::from_secs(10)))
    .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("OUT:hello 世界")
    );
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("ERR:ready")
    );
    let output = fixture(
        "io",
        Spawn {
            stdin: Some(b"merged".to_vec()),
            merge_stderr: true,
        },
    )
    .output(Some(Duration::from_secs(10)))
    .unwrap();
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("OUT:merged") && text.contains("ERR:ready"));
}

#[test]
fn drop_timeout_and_stop_settle_descendants() {
    for mode in ["drop", "timeout", "stop", "guard"] {
        let mut process = fixture("tree", Spawn::default());
        let child = descendant(&process);
        match mode {
            "timeout" => assert_eq!(
                process
                    .run(Some(Instant::now() + Duration::from_millis(50)), |_| true)
                    .unwrap(),
                Ended::TimedOut
            ),
            "stop" => {
                process.stop(Duration::from_secs(10)).unwrap();
            }
            "guard" => {
                drop(KillOnDrop(process.group()));
                process
                    .run(Some(Instant::now() + Duration::from_secs(5)), |_| true)
                    .unwrap();
            }
            _ => {}
        }
        drop(process);
        settled(&child);
    }
}

#[test]
fn descendants_settle_after_the_leader_exits_and_closes_output() {
    let mut process = fixture("leave", Spawn::default());
    let child = descendant(&process);
    assert!(
        matches!(process.run(Some(Instant::now() + Duration::from_secs(5)), |_| true).unwrap(), Ended::Exited(status) if status.success())
    );
    settled(&child);
    // A retained group still names this exact job after the leader is reaped.
    process.group().kill();
}

#[test]
fn powershell_preserves_quotes_unicode_environment_directory_and_status() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("space and ' quote");
    std::fs::create_dir(&cwd).unwrap();
    let shell = Shell {
        launcher: Launcher::default()
            .in_dir(root.path())
            .with_env("SWARMIE_TEST_VALUE", "héllo 世界"),
        confine: true,
        ..Default::default()
    };
    let mut ctx = ToolContext::new();
    let output = block_on(shell.call(&mut ctx, ShellArgs::new("[Console]::Out.WriteLine($env:SWARMIE_TEST_VALUE); [Console]::Out.WriteLine((Get-Location).Path); [Console]::Error.WriteLine('stderr'); exit 7").in_dir("space and ' quote"))).unwrap();
    assert!(output.contains("héllo 世界"), "{output}");
    assert!(output.contains("space and ' quote"), "{output}");
    assert!(output.contains("stderr"), "{output}");
    assert_eq!(
        ctx.result::<ExitStatus>().unwrap(),
        Some(ExitStatus::Exited(7))
    );
    assert!(shell.description().contains("PowerShell"));
    assert!(block_on(shell.run(ShellArgs::new("echo escaped").in_dir(".."))).is_err());
}

#[test]
fn powershell_timeout_and_dropped_future_stop_the_running_job() {
    let shell = Shell::default();
    let ran =
        block_on(shell.run(ShellArgs::new("Start-Sleep -Seconds 30").timeout_secs(1))).unwrap();
    assert_eq!(ran.status, ExitStatus::TimedOut);
    let (sender, receiver) = channel();
    let shell = Shell {
        live: Some(Live::new(move |line| {
            if let Ok(pid) = line.trim().parse::<u32>() {
                sender.send(pid).unwrap();
            }
        })),
        ..Default::default()
    };
    let mut run = Box::pin(shell.run(ShellArgs::new(
        "[Console]::Out.WriteLine($PID); Start-Sleep -Seconds 30",
    )));
    assert!(run.as_mut().now_or_never().is_none());
    let pid = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
    // SAFETY: capture the live process handle before dropping the future.
    let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    assert!(!raw.is_null());
    // SAFETY: OpenProcess returned a unique live handle.
    let child = unsafe { OwnedHandle::from_raw_handle(raw) };
    drop(run);
    settled(&child);
}

#[test]
fn windows_files_preserve_crlf_replace_existing_content_and_refuse_read_only() {
    let root = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(root.path()).unwrap();
    let path = base.join("space and ' quote 世界.txt");
    write_atomic(&path, b"one\r\ntwo\r\n").unwrap();
    let edit = Edit {
        base: Some(base.clone()),
        confine: true,
        ..Default::default()
    };
    let mut ctx = ToolContext::new();
    block_on(edit.call(
        &mut ctx,
        EditArgs::new(path.to_string_lossy(), [Replacement::new("two", "three")]),
    ))
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"one\r\nthree\r\n");
    assert_eq!(ctx.result::<FilesChanged>().unwrap().unwrap().0.len(), 1);
    write_atomic(&path, b"replacement").unwrap();
    let original = std::fs::metadata(&path).unwrap().permissions();
    let mut permissions = original.clone();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions.clone()).unwrap();
    assert_eq!(
        write_atomic(&path, b"denied").unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
    std::fs::set_permissions(&path, original).unwrap();
    let workspace: &dyn Workspace = &Local;
    assert_eq!(
        workspace
            .place(Some(&base), true, &path.to_string_lossy())
            .unwrap(),
        path
    );
    assert!(
        workspace
            .place(Some(&base), true, "..\\escape.txt")
            .is_err()
    );
    let destination = base.join("destination.txt");
    write_atomic(&destination, b"old").unwrap();
    Local.rename(&path, &destination).unwrap();
    assert_eq!(std::fs::read(destination).unwrap(), b"replacement");
}

#[test]
fn powershell_limits_output_and_redacts_live_returned_and_spilled_text() {
    let root = tempfile::tempdir().unwrap();
    let (sender, receiver) = channel();
    let shell = Shell {
        cut: Some(OutputLimit {
            lines: 6,
            bytes: 1024,
        }),
        spill: Some(Spill(root.path().to_owned())),
        live: Some(Live::new(move |text| {
            sender.send(text.to_owned()).unwrap();
        })),
        ..Default::default()
    };
    let ran = block_on(shell.run(ShellArgs::new("Write-Output 'OPENAI_API_KEY=sk-test-probe-1234567890abcdef'; 1..100 | ForEach-Object { Write-Output ('line ' + $_) }"))).unwrap();
    assert_eq!(ran.status, ExitStatus::Exited(0));
    assert!(ran.output.contains("line 100"));
    assert!(ran.output.len() < 2048);
    assert!(!ran.output.contains("sk-test-probe"));
    let live: String = receiver.try_iter().collect();
    assert!(live.contains("[REDACTED]"));
    assert!(!live.contains("sk-test-probe"));
    let files: Vec<_> = std::fs::read_dir(root.path()).unwrap().collect();
    assert_eq!(files.len(), 1);
    let spill = std::fs::read_to_string(files[0].as_ref().unwrap().path()).unwrap();
    assert!(
        spill.lines().any(|line| line == "line 1") && spill.lines().any(|line| line == "line 100")
    );
    assert!(!spill.contains("sk-test-probe"));
}

#[test]
fn replacement_preserves_an_explicit_protected_windows_dacl() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("access ' rules.txt");
    std::fs::write(&path, "original").unwrap();
    let quoted = format!("'{}'", path.to_string_lossy().replace('\'', "''"));
    let powershell = |script: String| {
        let output = Command::new("powershell.exe")
            // Windows PowerShell must construct its own module path instead of
            // inheriting the incompatible PowerShell Core path from the CI shell.
            .env_remove("PSModulePath")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &script,
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    let before = powershell(format!(
        "$ErrorActionPreference='Stop'; $acl=Get-Acl -LiteralPath {quoted}; $acl.SetAccessRuleProtection($true,$false); $sid=[System.Security.Principal.WindowsIdentity]::GetCurrent().User; $rule=[System.Security.AccessControl.FileSystemAccessRule]::new($sid,'FullControl','Allow'); $acl.AddAccessRule($rule); Set-Acl -LiteralPath {quoted} -AclObject $acl; (Get-Acl -LiteralPath {quoted}).Sddl"
    ));
    assert!(!before.is_empty());
    write_atomic(&path, b"replacement").unwrap();
    let after = powershell(format!(
        "$ErrorActionPreference='Stop'; (Get-Acl -LiteralPath {quoted}).Sddl"
    ));
    assert_eq!(before, after);
    assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
}
