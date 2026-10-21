//! Exports provider-neutral schemas for reproducible Tembra binding generation.
use std::process::ExitCode;
use swarmie_model_contracts::{ModelGenerationSchema, ModelProviderSchema};
use tembra::ContractToken;

fn export() -> Result<(), Box<dyn std::error::Error>> {
    let contract = match std::env::args().nth(1).as_deref() {
        Some("provider") => ModelProviderSchema.descriptor()?,
        Some("generation") => ModelGenerationSchema.descriptor()?,
        _ => return Err("expected provider or generation".into()),
    };
    println!("{}", serde_json::to_string(contract.descriptor())?);
    Ok(())
}
fn main() -> ExitCode {
    match export() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
