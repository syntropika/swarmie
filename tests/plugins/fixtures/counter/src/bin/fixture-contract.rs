//! Canonical descriptor exports for synthetic acceptance contracts.

use std::{env, process::ExitCode};

use swarmie_counter::{ConfigurationControl, Counter, DynamicConsumer, RemoteCounter};
use tembra::ContractToken;

// A private acceptance-only contract verifies incompatible reload admission. It is excluded
// from the production descriptor set and the public contracts crate's exports.
#[tembra::contract(local, name = "swarmie.dynamic-consumer", version = 2)]
trait IncompatibleDynamicConsumer {
    #[state]
    fn invocations(&self) -> u32;

    #[portable]
    async fn run(&self) -> tembra::Result<String>;
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let selected = env::args()
        .nth(1)
        .ok_or("usage: fixture-contract <contract-name>")?;
    let contract = match selected.as_str() {
        "counter" => Counter.descriptor()?,
        "remote-counter" => RemoteCounter.descriptor()?,
        "dynamic-consumer" => DynamicConsumer.descriptor()?,
        "dynamic-consumer-v2" => IncompatibleDynamicConsumer.descriptor()?,
        "configuration-control" => ConfigurationControl.descriptor()?,
        _ => return Err(format!("unknown acceptance contract: {selected}").into()),
    };
    println!("{}", tembra::serde_json::to_string(contract.descriptor())?);
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
