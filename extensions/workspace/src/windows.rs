//! Operating-system window metadata, capture and accessibility context.
use crate::{WorkspaceExtension, assets, command, required};
use serde_json::{Value, json};
use std::path::PathBuf;
use swarmie::{Error, Result};
fn program(extension: &WorkspaceExtension, name: &str) -> PathBuf {
    extension
        .integrations
        .get(name)
        .cloned()
        .unwrap_or_else(|| name.into())
}
fn window(p: &Value) -> Result<&str> {
    let id = required(p, "windowId")?;
    let valid = if let Some(hex) = id.strip_prefix("0x") {
        !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit())
    } else {
        !id.is_empty() && id.chars().all(|c| c.is_ascii_digit())
    };
    if !valid {
        return Err(Error::Invalid(
            "windowId must be a native numeric window identifier".into(),
        ));
    }
    Ok(id)
}
const MAC_WINDOWS: &str = r#"import Foundation
import CoreGraphics
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String:Any]] ?? []
let result = windows.filter { ($0[kCGWindowLayer as String] as? Int ?? 0) == 0 }.map { w -> [String:Any] in ["id":String(w[kCGWindowNumber as String] as? Int ?? 0),"title":w[kCGWindowName as String] as? String ?? "","application":w[kCGWindowOwnerName as String] as? String ?? "","pid":w[kCGWindowOwnerPID as String] as? Int ?? 0,"bounds":w[kCGWindowBounds as String] as? [String:Any] ?? [:]] }
print(String(data:try JSONSerialization.data(withJSONObject:result),encoding:.utf8)!)"#;
const MAC_CONTEXT: &str = r#"tell application "System Events"
set frontProcess to first application process whose unix id is SELECTED_PID
set processName to name of frontProcess
set windowNames to name of every window of frontProcess
set focusedElement to value of attribute "AXFocusedUIElement" of frontProcess
try
set elementDescription to description of focusedElement
on error
set elementDescription to ""
end try
try
set elementValue to value of focusedElement
on error
set elementValue to ""
end try
return {processName, windowNames, elementDescription, elementValue} as text
end tell"#;
pub async fn invoke(extension: &WorkspaceExtension, name: &str, p: &Value) -> Result<Value> {
    extension.ensure_storage().await?;
    match name {
        "platform.capabilities" => {
            let names = if cfg!(target_os = "macos") {
                vec![
                    "xcrun",
                    "screencapture",
                    "osascript",
                    "swift",
                    "adb",
                    "emulator",
                    "gh",
                ]
            } else if cfg!(windows) {
                vec!["powershell", "adb", "emulator", "gh"]
            } else {
                vec![
                    "wmctrl", "xprop", "import", "gdbus", "python3", "adb", "emulator", "gh",
                ]
            };
            let integrations = names
                .into_iter()
                .map(|name| {
                    let path = program(extension, name);
                    json!({"id":name,"available":command::available(&path.to_string_lossy())})
                })
                .collect::<Vec<_>>();
            Ok(
                json!({"os":std::env::consts::OS,"integrations":integrations,"windowBackend":if cfg!(target_os="macos"){"quartz"}else if cfg!(windows){"win32"}else{"x11"},"accessibilityRequiresPermission":cfg!(target_os="macos"),"message":"Window capture needs a graphical session and operating-system permission. Linux window selection uses X11; Wayland-only sessions are unsupported."}),
            )
        }
        "windows.list" => {
            let windows = if cfg!(target_os = "macos") {
                mac_windows(extension).await?
            } else if cfg!(windows) {
                serde_json::from_str(&command::run(&program(extension,"powershell"),&extension.storage_root,&command::args(&["-NoProfile","-Command","@(Get-Process | Where-Object {$_.MainWindowHandle -ne 0} | ForEach-Object {[PSCustomObject]@{id=$_.MainWindowHandle.ToString();title=$_.MainWindowTitle;application=$_.ProcessName;pid=$_.Id}}) | ConvertTo-Json -Compress"])).await?)?
            } else {
                let text = command::run(
                    &program(extension, "wmctrl"),
                    &extension.storage_root,
                    &command::args(&["-lpG"]),
                )
                .await?;
                json!(text.lines().filter_map(|line|{let fields=line.split_whitespace().collect::<Vec<_>>();if fields.len()<8{return None;}Some(json!({"id":fields[0],"pid":fields[2].parse::<u64>().ok(),"bounds":{"x":fields[3].parse::<i64>().ok(),"y":fields[4].parse::<i64>().ok(),"width":fields[5].parse::<u64>().ok(),"height":fields[6].parse::<u64>().ok()},"title":fields[8..].join(" ")}))}).collect::<Vec<_>>())
            };
            Ok(json!({"windows":windows,"os":std::env::consts::OS}))
        }
        "windows.capture" => {
            let id = window(p)?;
            let path = assets::output_path(extension, "window.png").await?;
            if cfg!(target_os = "macos") {
                command::run(
                    &program(extension, "screencapture"),
                    &extension.storage_root,
                    &[
                        "-x".into(),
                        "-l".into(),
                        id.into(),
                        path.to_string_lossy().into(),
                    ],
                )
                .await?;
            } else if cfg!(windows) {
                let escaped = path.to_string_lossy().replace('\'', "''");
                let script = format!(
                    r#"Add-Type -AssemblyName System.Drawing; Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public class WinCapture {{ [StructLayout(LayoutKind.Sequential)] public struct Rect {{ public int L,T,R,B; }} [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r); }}'; $r=New-Object WinCapture+Rect; if(-not [WinCapture]::GetWindowRect([IntPtr]{id},[ref]$r)){{throw 'Window unavailable'}}; $b=New-Object Drawing.Bitmap ($r.R-$r.L),($r.B-$r.T); $g=[Drawing.Graphics]::FromImage($b); $g.CopyFromScreen($r.L,$r.T,0,0,$b.Size); $b.Save('{escaped}',[Drawing.Imaging.ImageFormat]::Png); $g.Dispose(); $b.Dispose()"#
                );
                command::run(
                    &program(extension, "powershell"),
                    &extension.storage_root,
                    &["-NoProfile".into(), "-Command".into(), script],
                )
                .await?;
            } else {
                command::run(
                    &program(extension, "import"),
                    &extension.storage_root,
                    &["-window".into(), id.into(), path.to_string_lossy().into()],
                )
                .await?;
            }
            assets::register(extension, path).await
        }
        "windows.context" => {
            let id = window(p)?;
            let native = if cfg!(target_os = "macos") {
                let windows = mac_windows(extension).await?;
                let pid = windows
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|v| v["id"] == id)
                    .and_then(|v| v["pid"].as_u64())
                    .ok_or_else(|| Error::Invalid("window is no longer available".into()))?;
                command::run(
                    &program(extension, "osascript"),
                    &extension.storage_root,
                    &[
                        "-e".into(),
                        MAC_CONTEXT.replace("SELECTED_PID", &pid.to_string()),
                    ],
                )
                .await?
            } else if cfg!(windows) {
                let script = format!(
                    "Add-Type -AssemblyName UIAutomationClient; $element=[System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]{id}); if($null -eq $element){{throw 'Window unavailable'}}; $children=$element.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition); @($children | Select-Object -First 256 | ForEach-Object {{[PSCustomObject]@{{name=$_.Current.Name;type=$_.Current.ControlType.ProgrammaticName;enabled=$_.Current.IsEnabled}}}}) | ConvertTo-Json -Compress"
                );
                command::run(
                    &program(extension, "powershell"),
                    &extension.storage_root,
                    &["-NoProfile".into(), "-Command".into(), script],
                )
                .await?
            } else {
                let properties = command::run(
                    &program(extension, "xprop"),
                    &extension.storage_root,
                    &command::args(&["-id", id, "_NET_WM_NAME", "WM_CLASS", "_NET_WM_PID"]),
                )
                .await?;
                let pid = properties
                    .lines()
                    .find(|s| s.starts_with("_NET_WM_PID"))
                    .and_then(|line| {
                        line.rsplit_once('=')
                            .and_then(|(_, value)| value.trim().parse::<u64>().ok())
                    })
                    .ok_or_else(|| Error::Provider("X11 window omitted its process id".into()))?;
                let accessibility = command::run(
                    &program(extension, "python3"),
                    &extension.storage_root,
                    &["-c".into(), LINUX_CONTEXT.into(), pid.to_string()],
                )
                .await;
                return Ok(
                    json!({"windowId":id,"native":properties,"accessibility":match &accessibility{Ok(value)=>Some(value.as_str()),Err(_)=>None},"accessibilityError":accessibility.err().map(|e|e.to_string()),"source":"x11/at-spi"}),
                );
            };
            Ok(
                json!({"windowId":id,"native":native,"source":if cfg!(target_os="macos"){"macos-accessibility-selected-application"}else{"windows-ui-automation"}}),
            )
        }
        _ => Err(Error::Unsupported(format!("platform command {name}"))),
    }
}
const LINUX_CONTEXT: &str = r#"import json,sys
try:
 import pyatspi
except ImportError:
 raise SystemExit('Install python3-pyatspi for Linux accessibility context')
items=[]
def walk(node,depth=0):
 if depth>6 or len(items)>=256:return
 try:
  if node.name:items.append({'name':node.name,'role':node.getRoleName()})
  for child in node:walk(child,depth+1)
 except Exception:pass
for application in pyatspi.Registry.getDesktop(0):
 try:
  if application.get_process_id()==int(sys.argv[1]):walk(application)
 except Exception:pass
print(json.dumps(items))"#;

async fn mac_windows(extension: &WorkspaceExtension) -> Result<Value> {
    Ok(serde_json::from_str(
        &command::run(
            &program(extension, "swift"),
            &extension.storage_root,
            &["-e".into(), MAC_WINDOWS.into()],
        )
        .await?,
    )?)
}
