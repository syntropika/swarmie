//! Exports canonical task Contract descriptors for reproducible binding generation.

use std::process::ExitCode;

use swarmie_task_contracts::*;
use tembra::ContractToken;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let contract = match std::env::args().nth(1).as_deref() {
        Some("task-storage") => TaskStorage.descriptor()?,
        Some("durable-tasks") => DurableTasks.descriptor()?,
        Some("task-handler") => TaskHandler.descriptor()?,
        Some("task-lifecycle") => TaskLifecycle.descriptor()?,
        Some("task-created-observer") => TaskCreatedObserver.descriptor()?,
        Some("task-created-hook") => {
            println!(
                "{}",
                serde_json::to_string(on_task_created()?.descriptor().descriptor())?
            );
            return Ok(());
        }
        Some("task-phase-started-observer") => TaskPhaseStartedObserver.descriptor()?,
        Some("task-phase-started-hook") => {
            println!(
                "{}",
                serde_json::to_string(on_task_phase_started()?.descriptor().descriptor())?
            );
            return Ok(());
        }
        Some("task-phase-settled-observer") => TaskPhaseSettledObserver.descriptor()?,
        Some("task-phase-settled-hook") => {
            println!(
                "{}",
                serde_json::to_string(on_task_phase_settled()?.descriptor().descriptor())?
            );
            return Ok(());
        }
        Some("task-checkpoint-observer") => TaskCheckpointObserver.descriptor()?,
        Some("task-checkpoint-hook") => {
            println!(
                "{}",
                serde_json::to_string(on_task_checkpoint()?.descriptor().descriptor())?
            );
            return Ok(());
        }
        Some("task-waiting-observer") => TaskWaitingObserver.descriptor()?,
        Some("task-waiting-hook") => {
            println!(
                "{}",
                serde_json::to_string(on_task_waiting()?.descriptor().descriptor())?
            );
            return Ok(());
        }
        Some("task-sleeping-observer") => TaskSleepingObserver.descriptor()?,
        Some("task-sleeping-hook") => {
            println!(
                "{}",
                serde_json::to_string(on_task_sleeping()?.descriptor().descriptor())?
            );
            return Ok(());
        }
        Some("task-recovered-observer") => TaskRecoveredObserver.descriptor()?,
        Some("task-recovered-hook") => {
            println!(
                "{}",
                serde_json::to_string(on_task_recovered()?.descriptor().descriptor())?
            );
            return Ok(());
        }
        Some("task-blocked-observer") => TaskBlockedObserver.descriptor()?,
        Some("task-blocked-hook") => {
            println!(
                "{}",
                serde_json::to_string(on_task_blocked()?.descriptor().descriptor())?
            );
            return Ok(());
        }
        Some("task-parked-observer") => TaskParkedObserver.descriptor()?,
        Some("task-parked-hook") => {
            println!(
                "{}",
                serde_json::to_string(on_task_parked()?.descriptor().descriptor())?
            );
            return Ok(());
        }
        Some("task-retry-observer") => TaskRetryObserver.descriptor()?,
        Some("task-retry-hook") => {
            println!(
                "{}",
                serde_json::to_string(on_task_retry()?.descriptor().descriptor())?
            );
            return Ok(());
        }
        Some("task-cancellation-requested-observer") => {
            TaskCancellationRequestedObserver.descriptor()?
        }
        Some("task-cancellation-requested-hook") => {
            println!(
                "{}",
                serde_json::to_string(on_task_cancellation_requested()?.descriptor().descriptor())?
            );
            return Ok(());
        }
        Some("task-terminal-observer") => TaskTerminalObserver.descriptor()?,
        Some("task-terminal-hook") => {
            println!(
                "{}",
                serde_json::to_string(on_task_terminal()?.descriptor().descriptor())?
            );
            return Ok(());
        }
        _ => return Err("expected a task Contract or lifecycle hook name".into()),
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
