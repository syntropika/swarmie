//! Exports canonical task Contract descriptors for reproducible binding generation.

use std::process::ExitCode;

use swarmie_task_contracts::{DurableTasks, TaskHandler, TaskStorage};
use tembra::ContractToken;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let contract = match std::env::args().nth(1).as_deref() {
        Some("task-storage") => TaskStorage.descriptor()?,
        Some("durable-tasks") => DurableTasks.descriptor()?,
        Some("task-handler") => TaskHandler.descriptor()?,
        _ => return Err("expected task-storage, durable-tasks or task-handler".into()),
    };
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
