//! Exports the test workflow's canonical method descriptor for real JS artifact generation.

use swarmie_durable_tasks_fixture::DurableWorkflow;
use swarmie_durable_tasks_fixture::gate::TaskGate;
use tembra::ContractToken;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let contract = if std::env::args().nth(1).as_deref() == Some("durable-gate") {
        TaskGate.descriptor()?
    } else {
        DurableWorkflow.descriptor()?
    };
    println!("{}", serde_json::to_string(contract.descriptor())?);
    Ok(())
}
