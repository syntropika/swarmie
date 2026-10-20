use std::{path::PathBuf, process::ExitCode};

use swarmie_plugin_contracts::*;
use swarmie_plugin_manager::PluginManager;
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
        return Err("usage: swarmie-runtime <serve|inventory|install|activate|update|deactivate|uninstall> <database> <source-directory> [arguments]".into());
    };
    let database = arguments.get(1).ok_or("a database path is required")?;
    let source = arguments.get(2).ok_or("a source directory is required")?;
    let proof = match std::env::var("SWARMIE_ADMIN_PROOF") {
        Ok(value) => AdminProof::new(value),
        Err(std::env::VarError::NotPresent) => PluginManager::issue_proof(),
        Err(error) => return Err(error.into()),
    };
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
                    proof: proof.clone(),
                },
            )
            .await?;
            if let Some(error) = &runtime.recovery_error {
                eprintln!("cached activation failed; administration remains available: {error}");
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
                    "inventory" => manager.inventory(&proof)?,
                    "install" | "update" => {
                        let plugin_id = id()?;
                        let version = arguments.get(4).ok_or("a version is required")?.clone();
                        if arguments.get(5).map(String::as_str) != Some("--confirm") {
                            return Err("trusted code installation requires --confirm".into());
                        }
                        if command == "install" {
                            manager
                                .install(InstallRequest {
                                    proof: proof.clone(),
                                    provider_id: "local".into(),
                                    plugin_id,
                                    version,
                                    confirmed: true,
                                })
                                .await?
                        } else {
                            manager
                                .update(UpdateRequest {
                                    proof: proof.clone(),
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
                            .activate(ActivateRequest {
                                proof: proof.clone(),
                                plugin_id: id()?,
                            })
                            .await?
                    }
                    "deactivate" => {
                        manager
                            .deactivate(DeactivateRequest {
                                proof: proof.clone(),
                                plugin_id: id()?,
                            })
                            .await?
                    }
                    "uninstall" => {
                        manager
                            .uninstall(UninstallRequest {
                                proof: proof.clone(),
                                plugin_id: id()?,
                            })
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
