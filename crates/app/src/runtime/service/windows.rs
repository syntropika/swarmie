//! Per-user Task Scheduler adapter. Scripts are fixed; paths and actions are process data.
use serde_json::Value;
use std::{path::Path, time::Duration};
use swarmie::{Error, Result};

fn argument(value: &str) -> Result<String> {
    if value.chars().any(char::is_control) || value.contains('%') {
        return Err(Error::Invalid(
            "Windows startup arguments cannot contain control characters or environment expansion"
                .into(),
        ));
    }
    // Quote for CommandLineToArgvW; Task Scheduler launches the executable directly.
    let mut result = String::from("\"");
    let mut slashes = 0;
    for ch in value.chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        result.extend(std::iter::repeat_n(
            '\\',
            if ch == '"' { slashes * 2 + 1 } else { slashes },
        ));
        result.push(ch);
        slashes = 0;
    }
    result.extend(std::iter::repeat_n('\\', slashes * 2));
    result.push('"');
    Ok(result)
}
pub(super) fn render(executable: &Path, arguments: &[String], token: &Path) -> Result<String> {
    let command = executable.to_string_lossy();
    argument(&command)?;
    let mut args = arguments.to_vec();
    args.extend(["--token-file".into(), token.to_string_lossy().into_owned()]);
    let args = args
        .iter()
        .map(|value| argument(value))
        .collect::<Result<Vec<_>>>()?
        .join(" ");
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
<RegistrationInfo><Description>Swarmie user runtime</Description></RegistrationInfo>
<Triggers><LogonTrigger><Enabled>true</Enabled></LogonTrigger></Triggers>
<Principals><Principal id="RuntimeUser"><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
<Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><AllowHardTerminate>false</AllowHardTerminate><StartWhenAvailable>true</StartWhenAvailable><ExecutionTimeLimit>PT0S</ExecutionTimeLimit><RestartOnFailure><Interval>PT1M</Interval><Count>999</Count></RestartOnFailure></Settings>
<Actions Context="RuntimeUser"><Exec><Command>{}</Command><Arguments>{}</Arguments></Exec></Actions>
</Task>
"#,
        super::xml(&command),
        super::xml(&args)
    ))
}
const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
function User-Sid([string]$value) {
  try {
    if ($value.StartsWith('S-1-')) { return (New-Object Security.Principal.SecurityIdentifier($value)).Value }
    return (New-Object Security.Principal.NTAccount($value)).Translate([Security.Principal.SecurityIdentifier]).Value
  } catch { return '' }
}
$action = $env:SWARMIE_STARTUP_ACTION
$path = $env:SWARMIE_STARTUP_XML
$hash = [System.Security.Cryptography.SHA256]::Create()
$suffix = [BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes([IO.Path]::GetFullPath($path).ToLowerInvariant()))).Replace('-', '').Substring(0,16)
$name = 'Swarmie Runtime ' + $sid + ' ' + $suffix
$task = Get-ScheduledTask -TaskName $name -TaskPath '\' -ErrorAction SilentlyContinue
$owned = $true
if (Test-Path -LiteralPath $path) {
  [xml]$wanted = [IO.File]::ReadAllText($path, [Text.Encoding]::UTF8)
  if ($task) {
    [xml]$actual = Export-ScheduledTask -TaskName $name -TaskPath '\'
    $owned = @($actual.Task.Actions.ChildNodes).Count -eq 1 -and
      $actual.Task.Actions.Exec.Command -eq $wanted.Task.Actions.Exec.Command -and
      $actual.Task.Actions.Exec.Arguments -eq $wanted.Task.Actions.Exec.Arguments -and
      (User-Sid $actual.Task.Principals.Principal.UserId) -eq $sid -and
      $actual.Task.Principals.Principal.LogonType -eq 'InteractiveToken' -and
      ([string]$actual.Task.Principals.Principal.RunLevel -in @('', 'LeastPrivilege')) -and
      @($actual.Task.Triggers.ChildNodes).Count -eq 1 -and
      (User-Sid $actual.Task.Triggers.LogonTrigger.UserId) -eq $sid
  }
} elseif ($task) { $owned = $false }
if ($action -ne 'inspect' -and -not $owned) { throw 'The scheduled task is not owned by this configuration.' }
switch ($action) {
  'enable' {
    if (-not $task) {
      if (-not $wanted) { throw 'The task definition is missing.' }
      $principal = $wanted.Task.Principals.Principal
      $user = $wanted.CreateElement('UserId', $wanted.DocumentElement.NamespaceURI)
      $user.InnerText = $sid
      [void]$principal.PrependChild($user)
      $triggerUser = $wanted.CreateElement('UserId', $wanted.DocumentElement.NamespaceURI)
      $triggerUser.InnerText = $sid
      [void]$wanted.Task.Triggers.LogonTrigger.AppendChild($triggerUser)
      # The scheduler receives a UTF-16 managed string, not the UTF-8 file bytes.
      $wanted.FirstChild.Encoding = 'UTF-16'
      Register-ScheduledTask -TaskName $name -TaskPath '\' -Xml $wanted.OuterXml -ErrorAction Stop | Out-Null
    }
    Enable-ScheduledTask -TaskName $name -TaskPath '\' -ErrorAction Stop | Out-Null
  }
  'disable' { if ($task) { Disable-ScheduledTask -TaskName $name -TaskPath '\' -ErrorAction Stop | Out-Null } }
  'start' { Start-ScheduledTask -TaskName $name -TaskPath '\' -ErrorAction Stop | Out-Null }
  'stop' { Stop-ScheduledTask -TaskName $name -TaskPath '\' -ErrorAction Stop }
  'uninstall' { if ($task) { Unregister-ScheduledTask -TaskName $name -TaskPath '\' -Confirm:$false -ErrorAction Stop } }
  'inspect' { }
  default { throw 'Unknown startup operation.' }
}
$task = Get-ScheduledTask -TaskName $name -TaskPath '\' -ErrorAction SilentlyContinue
[ordered]@{
  available = $owned; installed = [bool]$task; enabled = [bool]($task -and $task.Settings.Enabled)
  running = [bool]($task -and $task.State -eq 'Running'); trigger = 'login'; bootReady = $false
  canEnableBoot = $false
  reason = $(if ($owned) { '' } else { 'The scheduled task belongs to another installation or was modified; preserve it and configure startup manually.' })
  notice = 'Starts when this Windows user signs in. Startup without sign-in requires an administrator-managed task or service.'
} | ConvertTo-Json -Compress
"#;
pub(super) async fn operation(action: &str, path: &Path) -> Result<Value> {
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        tokio::process::Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                SCRIPT,
            ])
            .env("SWARMIE_STARTUP_ACTION", action)
            .env("SWARMIE_STARTUP_XML", path)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| Error::Timeout("Windows startup task".into()))??;
    if !output.status.success() || output.stdout.len() > 8192 {
        return Err(Error::Provider("Windows startup task could not be verified or changed; check the task permissions and configuration".into()));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|_| Error::Provider("invalid Windows startup observation".into()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_runs_the_runtime_without_shell_expansion_or_administrator_privileges() {
        let task = render(
            Path::new(r"C:\User files\Swarmie\swarmie.exe"),
            &[
                "serve".into(),
                "--state-dir".into(),
                r"C:\User files\state\".into(),
            ],
            Path::new(r"C:\User files\service.token"),
        )
        .unwrap();
        assert!(task.contains("<RunLevel>LeastPrivilege</RunLevel>"));
        assert!(task.contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(task.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
        assert!(task.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(task.contains("<RestartOnFailure>"));
        assert!(task.contains("--token-file"));
        assert!(!task.contains("password"));
        assert_eq!(argument(r"C:\trailing\").unwrap(), "\"C:\\trailing\\\\\"");
        assert!(argument("%UNTRUSTED%").is_err());
    }
}
