//! Emits the canonical owned Contract descriptor for Tembra binding generation.

use std::process::ExitCode;

use swarmie_memory_store::MemoryStore;
use tembra::ContractToken;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let contract = MemoryStore.descriptor()?;
    println!("{}", serde_json::to_string(contract.descriptor())?);
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
