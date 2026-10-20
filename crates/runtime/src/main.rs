use std::{path::PathBuf, process::ExitCode};

use swarmie_plugin_contracts::*;
use swarmie_runtime::{Configuration, start};
use tembra_native::TokioDriver;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("runtime failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = arguments.first() else {
        return Err("usage: swarmie-runtime resolve <application.json> | <serve|composition|inventory|install|activate|update|deactivate|uninstall> <database> <source-directory> [arguments]".into());
    };
    if command == "resolve" {
        if arguments.len() != 2 {
            return Err("usage: swarmie-runtime resolve <application.json>".into());
        }
        let application = serde_json::from_slice(&std::fs::read(&arguments[1])?)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&swarmie_runtime::resolve_application(&application)?)?
        );
        return Ok(());
    }
    let database = arguments.get(1).ok_or("a database path is required")?;
    let source = arguments.get(2).ok_or("a source directory is required")?;
    let application = arguments
        .iter()
        .position(|argument| argument == "--composition")
        .map(|index| -> Result<_, Box<dyn std::error::Error>> {
            if !arguments.iter().any(|argument| argument == "--confirm") {
                return Err("importing trusted executable composition requires --confirm".into());
            }
            let path = arguments
                .get(index + 1)
                .ok_or("a composition file is required")?;
            Ok(serde_json::from_slice(&std::fs::read(path)?)?)
        })
        .transpose()?;
    let driver = TokioDriver::new();
    let scheduler = driver.clone();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(scheduler.run_until(async {
            let runtime = start(
                driver.clone(),
                Configuration {
                    database: PathBuf::from(database),
                    source: PathBuf::from(source),
                    application,
                },
            )
            .await?;
            if let Some(error) = &runtime.recovery_error {
                eprintln!("cached activation failed; administration remains available: {error}");
            }
            for (instance, error) in &runtime.deployment_errors {
                eprintln!("deployment unavailable for {instance}: {error}");
            }
            let result = async {
                let manager = &runtime.manager;
                let id = || {
                    arguments
                        .get(3)
                        .cloned()
                        .ok_or("a plugin identity is required")
                };
                let inventory = match command.as_str() {
                    "serve" => {
                        let address = arguments
                            .get(3)
                            .map(String::as_str)
                            .unwrap_or("127.0.0.1:0")
                            .parse()?;
                        swarmie_runtime::serve(&runtime, driver, address, async {
                            let _ = tokio::signal::ctrl_c().await;
                        })
                        .await?;
                        return Ok(());
                    }
                    "composition" => {
                        println!("{}", serde_json::to_string_pretty(&runtime.effective)?);
                        return Ok(());
                    }
                    "inventory" => manager.inventory()?,
                    "install" | "update" => {
                        let plugin_id = id()?;
                        let version = arguments.get(4).ok_or("a version is required")?.clone();
                        if arguments.get(5).map(String::as_str) != Some("--confirm") {
                            return Err("trusted code installation requires --confirm".into());
                        }
                        if command == "install" {
                            manager
                                .install(InstallRequest {
                                    provider_id: "local".into(),
                                    plugin_id,
                                    version,
                                    confirmed: true,
                                })
                                .await?
                        } else {
                            manager
                                .update(UpdateRequest {
                                    provider_id: "local".into(),
                                    plugin_id,
                                    version,
                                    confirmed: true,
                                })
                                .await?
                        }
                    }
                    "activate" => {
                        manager
                            .activate(ActivateRequest { plugin_id: id()? })
                            .await?
                    }
                    "deactivate" => {
                        manager
                            .deactivate(DeactivateRequest { plugin_id: id()? })
                            .await?
                    }
                    "uninstall" => {
                        manager
                            .uninstall(UninstallRequest { plugin_id: id()? })
                            .await?
                    }
                    _ => return Err("unknown runtime command".into()),
                };
                println!("{}", serde_json::to_string_pretty(&inventory)?);
                Ok::<(), Box<dyn std::error::Error>>(())
            }
            .await;
            let cleanup = runtime.host.close().await;
            result?;
            cleanup?;
            Ok(())
        }))
}
