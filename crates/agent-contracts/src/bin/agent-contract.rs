//! Exports public Contract and native hook descriptors for TypeScript fixture generation.

use std::process::ExitCode;

use swarmie_agent_contracts::{AgentLoop, AgentObservations, ToolHandler, ToolPolicy};
use tembra::ContractToken;

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let contract = match std::env::args().nth(1).as_deref() {
        Some("agent-loop") => AgentLoop.descriptor()?,
        Some("agent-observations") => AgentObservations.descriptor()?,
        Some("agent-tool") => ToolHandler.descriptor()?,
        Some("agent-tool-policy") => ToolPolicy.descriptor()?,
        Some("hooks") => {
            use swarmie_agent_contracts::*;
            let hooks = [
                on_turn_start()?.descriptor().descriptor().clone(),
                select_context()?.descriptor().descriptor().clone(),
                prepare_context()?.descriptor().descriptor().clone(),
                before_model_call()?.descriptor().descriptor().clone(),
                after_model_call()?.descriptor().descriptor().clone(),
                on_model_event()?.descriptor().descriptor().clone(),
                before_tool_call()?.descriptor().descriptor().clone(),
                authorize_tool_call()?.descriptor().descriptor().clone(),
                after_tool_call()?.descriptor().descriptor().clone(),
                on_step_end()?.descriptor().descriptor().clone(),
                on_turn_end()?.descriptor().descriptor().clone(),
                wrap_model_call()?.descriptor().descriptor().clone(),
            ];
            println!("{}", serde_json::to_string(&hooks)?);
            return Ok(());
        }
        _ => return Err("expected agent-loop, agent-tool, agent-tool-policy or hooks".into()),
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
