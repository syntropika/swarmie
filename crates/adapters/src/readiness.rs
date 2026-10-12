use super::*;
use std::time::Duration;
use tokio::process::Command;

async fn output(exe: &std::path::Path, args: &[&str]) -> Option<std::process::Output> {
    let mut command = Command::new(exe);
    command.args(args).kill_on_drop(true);
    tokio::time::timeout(Duration::from_secs(8), command.output())
        .await
        .ok()?
        .ok()
}
pub async fn probe(d: Detection, auth_args: Option<&[&str]>) -> Result<HarnessProbe> {
    let mut probe = HarnessProbe {
        harness: d.harness.clone(),
        available: d.available,
        executable: d.executable.clone(),
        version: None,
        auth_state: "unknown".into(),
        message: None,
        capabilities: d.capabilities,
    };
    let Some(exe) = d.executable else {
        probe.message = Some(format!(
            "Install {} and add it to PATH, or set SWARMIE_{}_EXECUTABLE.",
            d.harness,
            d.harness.to_uppercase()
        ));
        return Ok(probe);
    };
    if let Some(o) = output(&exe, &["--version"]).await {
        let version = String::from_utf8_lossy(&o.stdout).trim().to_owned();
        if o.status.success() && !version.is_empty() {
            probe.version = Some(version.lines().next().unwrap_or_default().into());
        }
    }
    if let Some(args) = auth_args {
        if let Some(o) = output(&exe, args).await {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let report: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);
            let stderr = String::from_utf8_lossy(&o.stderr);
            let diagnostic = format!("{stdout} {stderr}").to_lowercase();
            let missing = report["loggedIn"] == false
                || report["authenticated"] == false
                || report["valid"] == false
                || diagnostic.contains("not logged in")
                || diagnostic.contains("not authenticated");
            probe.auth_state = if missing {
                "unauthenticated"
            } else if o.status.success() {
                "authenticated"
            } else {
                "unknown"
            }
            .into();
            if probe.auth_state == "unauthenticated" {
                probe.message = Some(format!(
                    "Run {} in a terminal to sign in or configure its supported API endpoint.",
                    d.harness
                ));
            }
            if probe.auth_state == "unknown" {
                probe.message = Some("This installed version could not report authentication status; check it in a terminal or open a session for its native diagnostic.".into());
            }
        } else {
            probe.message = Some("Authentication check timed out or is unavailable; opening a session will report the provider error.".into());
        }
    } else {
        probe.message = Some("Authentication is provider-specific; an executable check cannot establish account readiness.".into());
    }
    Ok(probe)
}
