use rmcp::{ServiceExt, transport::stdio};
use swarmie_fixture_mcp_server::Counter;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let mut counter = Counter::default();
    if let [mode, path] = arguments.as_slice() {
        if mode == "--gate-effect" {
            counter = counter.with_effect_gate(path.into());
        } else {
            std::fs::write(path, std::process::id().to_string())?;
            if mode == "--stall-before-handshake" {
                return std::future::pending().await;
            }
            if mode != "--pid-file" {
                return Err("unknown synthetic mode".into());
            }
        }
    } else if !arguments.is_empty() {
        return Err("invalid synthetic arguments".into());
    }
    counter.serve(stdio()).await?.waiting().await?;
    Ok(())
}
