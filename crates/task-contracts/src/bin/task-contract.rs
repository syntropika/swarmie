//! Exports canonical task Contract descriptors for reproducible binding generation.

use std::process::ExitCode;

use swarmie_task_contracts::{
    DurableTasks, TaskHandler, TaskLifecycle, TaskLifecycleObserver, TaskStorage, task_lifecycle,
};
use tembra::ContractToken;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let contract = match std::env::args().nth(1).as_deref() {
        Some("task-storage") => TaskStorage.descriptor()?,
        Some("durable-tasks") => DurableTasks.descriptor()?,
        Some("task-handler") => TaskHandler.descriptor()?,
        Some("task-lifecycle-observer") => TaskLifecycleObserver.descriptor()?,
        Some("task-lifecycle") => TaskLifecycle.descriptor()?,
        Some("task-lifecycle-hook") => {
            println!(
                "{}",
                serde_json::to_string(task_lifecycle()?.descriptor().descriptor())?
            );
            return Ok(());
        }
        _ => return Err("expected a task Contract name or task-lifecycle-hook".into()),
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
