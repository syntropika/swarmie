//! Emits Tembra's actual canonical descriptors for the shared binding generator.

use std::{env, fs, path::PathBuf, process::ExitCode};

use swarmie_plugin_contracts::*;
use tembra::{ContractToken, contracts::Contract};

fn descriptors() -> tembra::Result<Vec<(&'static str, std::rc::Rc<Contract>)>> {
    Ok(vec![
        ("plugin-configuration", PluginConfiguration.descriptor()?),
        ("plugin-source", PluginSourceSchema.descriptor()?),
        ("plugin-administration", PluginAdministration.descriptor()?),
        ("ui-contributions", UiContributions.descriptor()?),
        ("browser-access", BrowserAccess.descriptor()?),
        ("remote-browser-access", RemoteBrowserAccess.descriptor()?),
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
