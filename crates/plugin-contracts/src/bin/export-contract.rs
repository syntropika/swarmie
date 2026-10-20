//! Emits Tembra's actual canonical descriptors for the shared binding generator.

use std::{env, fs, path::PathBuf, process::ExitCode};

use swarmie_plugin_contracts::*;
use tembra::{ContractToken, contracts::Contract};

// A private acceptance-only contract verifies incompatible reload admission. It is excluded
// from the production descriptor set and the public contracts crate's exports.
#[tembra::contract(local, name = "swarmie.dynamic-consumer", version = 2)]
trait IncompatibleDynamicConsumer {
    #[state]
    fn invocations(&self) -> u32;

    #[portable]
    async fn run(&self) -> tembra::Result<String>;
}

fn descriptors() -> tembra::Result<Vec<(&'static str, std::rc::Rc<Contract>)>> {
    Ok(vec![
        ("counter", Counter.descriptor()?),
        ("remote-counter", RemoteCounter.descriptor()?),
        ("dynamic-consumer", DynamicConsumer.descriptor()?),
        ("plugin-source", PluginSourceSchema.descriptor()?),
        ("plugin-administration", PluginAdministration.descriptor()?),
        (
            "remote-plugin-administration",
            RemotePluginAdministration.descriptor()?,
        ),
        ("ui-contributions", UiContributions.descriptor()?),
    ])
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = env::args().skip(1).collect();
    let contracts = descriptors()?;
    match arguments.as_slice() {
        [flag, destination] if flag == "--output-dir" => {
            let destination = PathBuf::from(destination);
            fs::create_dir_all(&destination)?;
            for (name, contract) in contracts {
                let json = tembra::serde_json::to_string_pretty(contract.descriptor())?;
                fs::write(
                    destination.join(format!("{name}.json")),
                    format!("{json}\n"),
                )?;
            }
        }
        [selected] => {
            if selected == "dynamic-consumer-v2" {
                let contract = IncompatibleDynamicConsumer.descriptor()?;
                println!("{}", tembra::serde_json::to_string(contract.descriptor())?);
                return Ok(());
            }
            let (_, contract) = contracts
                .into_iter()
                .find(|(name, _)| name == selected)
                .ok_or_else(|| format!("unknown contract: {selected}"))?;
            println!("{}", tembra::serde_json::to_string(contract.descriptor())?);
        }
        _ => {
            return Err("usage: export-contract <contract-name> | --output-dir <directory>".into());
        }
    }
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
