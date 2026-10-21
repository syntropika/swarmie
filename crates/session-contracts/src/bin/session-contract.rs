//! Exports the canonical descriptor consumed by existing Tembra TypeScript code generation.
use swarmie_session_contracts::SessionStore;
use tembra::ContractToken;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        serde_json::to_string(SessionStore.descriptor()?.descriptor())?
    );
    Ok(())
}
