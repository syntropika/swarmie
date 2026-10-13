//! Read-only shell discovery and binary-safe execution through Windows OpenSSH.
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{process::Stdio, time::Duration};
use swarmie::{Error, Result};
use tokio::io::AsyncReadExt;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shell {
    Posix,
    Windows,
}
fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
pub(crate) fn encoded(script: &str) -> String {
    let bytes: Vec<_> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {}",
        STANDARD.encode(bytes)
    )
}
async fn probe(host: &str, script: &str) -> Result<Option<String>> {
    let mut command = super::node_channel::ssh_command(host)?;
    command
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn()?;
    tokio::time::timeout(Duration::from_secs(8), async {
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .ok_or(Error::Closed)?
            .take(257)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() > 256 {
            return Err(Error::Invalid("invalid remote shell response".into()));
        }
        let status = child.wait().await?;
        if status.code() == Some(255) {
            return Err(Error::Provider("SSH access is unavailable".into()));
        }
        Ok(status
            .success()
            .then(|| String::from_utf8_lossy(&bytes).trim().to_owned()))
    })
    .await
    .map_err(|_| Error::Timeout("remote shell discovery".into()))?
}
pub(crate) async fn discover(host: &str) -> Result<Shell> {
    if probe(host, "uname -s")
        .await?
        .is_some_and(|value| matches!(value.as_str(), "Linux" | "Darwin"))
    {
        return Ok(Shell::Posix);
    }
    if probe(host, &encoded("[Console]::Write('swarmie-windows-shell')"))
        .await?
        .as_deref()
        == Some("swarmie-windows-shell")
    {
        return Ok(Shell::Windows);
    }
    Err(Error::Unsupported(
        "remote SSH shell must provide a Linux/macOS shell or Windows PowerShell".into(),
    ))
}
// Windows OpenSSH may use cmd.exe or PowerShell as its default shell. EncodedCommand
// handles both without quoting remote user data in that shell. Process streams preserve bytes.
fn windows_process(resolve: &str, arguments: &[String], framed: bool) -> String {
    let arguments = if framed {
        r#"$source = [Console]::OpenStandardInput()
$header = New-Object System.Collections.Generic.List[byte]
while (($byte = $source.ReadByte()) -ne 10) {
  if ($byte -lt 0 -or $header.Count -ge 131072) { throw 'Invalid SSH bootstrap header' }
  $header.Add([byte]$byte)
}
$arguments = [Text.Encoding]::UTF8.GetString($header.ToArray()) | ConvertFrom-Json
"#
        .to_owned()
    } else {
        format!(
            "$source = [Console]::OpenStandardInput(); $arguments = {}",
            literal(
                &arguments
                    .iter()
                    .map(|v| windows_argument(v))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        )
    };
    encoded(&format!(
        r#"
$ErrorActionPreference = 'Stop'
{resolve}
{arguments}
$info = New-Object System.Diagnostics.ProcessStartInfo
$info.FileName = $binary
$info.Arguments = $arguments
$info.UseShellExecute = $false
$info.RedirectStandardInput = $true
$info.RedirectStandardOutput = $true
$info.RedirectStandardError = $true
$info.CreateNoWindow = $true
$proc = New-Object System.Diagnostics.Process
$proc.StartInfo = $info
[void]$proc.Start()
$inputCopy = $source.CopyToAsync($proc.StandardInput.BaseStream)
$outputCopy = $proc.StandardOutput.BaseStream.CopyToAsync([Console]::OpenStandardOutput())
$errorCopy = $proc.StandardError.BaseStream.CopyToAsync([System.IO.Stream]::Null)
while (-not $proc.WaitForExit(50)) {{
  if ($inputCopy.IsCompleted) {{ $proc.StandardInput.Close() }}
}}
$outputCopy.GetAwaiter().GetResult()
$errorCopy.GetAwaiter().GetResult()
exit $proc.ExitCode
"#
    ))
}
fn windows_argument(value: &str) -> String {
    let mut output = String::from("\"");
    let mut slashes = 0;
    for ch in value.chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        output.extend(std::iter::repeat_n(
            '\\',
            if ch == '"' { slashes * 2 + 1 } else { slashes },
        ));
        output.push(ch);
        slashes = 0;
    }
    output.extend(std::iter::repeat_n('\\', slashes * 2));
    output.push('"');
    output
}
const RESOLVE: &str = r#"
$found = Get-Command swarmie.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
$binary = if ($found) { $found.Source } else { Join-Path $env:USERPROFILE '.local\share\swarmie\runtime\bin\swarmie.exe' }
if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { exit 127 }
"#;
impl Shell {
    pub(crate) fn runtime(self, arguments: &[String]) -> String {
        match self {
            Self::Posix => format!(
                "{}exec \"$SWARMIE\" {}",
                super::node_channel::SSH_RUNTIME,
                arguments
                    .iter()
                    .map(|v| super::node_channel::shell_quote(v))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            Self::Windows => windows_process(RESOLVE, arguments, false),
        }
    }
    pub(crate) fn python(self, script: &str, arguments: &[String]) -> (String, Option<Vec<u8>>) {
        let mut args = vec!["-c".into(), script.into()];
        args.extend_from_slice(arguments);
        match self {
            Self::Posix => (
                format!(
                    "python3 {}",
                    args.iter()
                        .map(|v| super::node_channel::shell_quote(v))
                        .collect::<Vec<_>>()
                        .join(" ")
                ),
                None,
            ),
            Self::Windows => {
                // Send the Python source and arguments through a length-bounded header on stdin.
                // Keep the remote command below cmd.exe's limit and preserve the following archive bytes.
                let arguments = args
                    .iter()
                    .map(|v| windows_argument(v))
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut header = serde_json::to_vec(&arguments).expect("string JSON serialization");
                header.push(b'\n');
                (
                    windows_process(
                        "$found = Get-Command python.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1; $binary = $found.Source",
                        &[],
                        true,
                    ),
                    Some(header),
                )
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[tokio::test]
    async fn native_powershell_preserves_framed_input_and_binary_output() {
        use tokio::io::AsyncWriteExt;
        let payload: Vec<_> = (0u8..=255).cycle().take(16384).collect();
        let (command, prelude) = Shell::Windows.python(
            "import sys;sys.stdout.buffer.write(sys.stdin.buffer.read())",
            &[],
        );
        let mut child = tokio::process::Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-EncodedCommand",
                command.split_whitespace().last().unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let operation = async {
            let write = async {
                input.write_all(&prelude.unwrap()).await.unwrap();
                input.write_all(&payload).await.unwrap();
                input.shutdown().await.unwrap();
            };
            let (_, output) = tokio::join!(write, child.wait_with_output());
            output.unwrap()
        };
        let output = tokio::time::timeout(Duration::from_secs(20), operation)
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, payload);
    }
    #[test]
    fn windows_bootstrap_fits_the_default_cmd_command_limit() {
        let script = include_str!("../../../../scripts/distribution/prepare-ssh.py");
        let (command, header) = Shell::Windows.python(
            script,
            &[
                "install".into(),
                "a".repeat(64),
                "windows-x86_64".into(),
                "0.1.0".into(),
            ],
        );
        assert!(header.unwrap().ends_with(b"\n"));
        assert!(command.len() < 7900, "command has {} bytes", command.len());
    }
    #[test]
    fn windows_commands_keep_paths_and_streams_out_of_the_default_shell() {
        let value = r#"C:\User files\é 'quoted'\folder\"#;
        let command = Shell::Windows.runtime(&[
            "runtime".into(),
            "proxy".into(),
            "--socket".into(),
            value.into(),
        ]);
        assert!(!command.contains(value));
        let bytes = STANDARD
            .decode(command.split_whitespace().last().unwrap())
            .unwrap();
        let script = String::from_utf16(
            &bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(script.contains("BaseStream.CopyToAsync"));
        assert!(script.contains("UseShellExecute = $false"));
        assert!(script.contains("é ''quoted''"));
        assert_eq!(windows_argument(r"C:\tail\"), "\"C:\\tail\\\\\"");
    }
}
