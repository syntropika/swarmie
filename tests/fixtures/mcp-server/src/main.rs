use rmcp::{ServiceExt, transport::stdio};
use swarmie_fixture_mcp_server::Counter;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if let [mode, path] = arguments.as_slice() {
        std::fs::write(path, std::process::id().to_string())?;
        if mode == "--stall-before-handshake" {
            return std::future::pending().await;
        }
        if mode != "--pid-file" {
            return Err("unknown synthetic mode".into());
        }
    } else if !arguments.is_empty() {
        return Err("invalid synthetic arguments".into());
    }
    Counter::default().serve(stdio()).await?.waiting().await?;
    Ok(())
}
