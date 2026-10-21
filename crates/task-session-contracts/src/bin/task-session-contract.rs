use std::process::ExitCode;

use swarmie_task_session_contracts::{TaskSessionBindings, TaskSessionPublisher};
use tembra::ContractToken;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let contract = match std::env::args().nth(1).as_deref() {
        Some("task-session-bindings") => TaskSessionBindings.descriptor()?,
        Some("task-session-publisher") => TaskSessionPublisher.descriptor()?,
        _ => return Err("expected task-session-bindings or task-session-publisher".into()),
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
