//! Supervised loopback acceptance fixture with read-only remote artifact delivery.

use std::{cell::RefCell, io::BufRead, path::PathBuf};

use futures::{StreamExt, channel::mpsc};
use swarmie_composition::{FacetIdentity, PluginKind as DistributionKind, Release, Selection};
use swarmie_plugin_contracts::*;
use swarmie_runtime::{Application, Configuration, Runtime, resolve_application, start};
use tembra_native::TokioDriver;

type Error = Box<dyn std::error::Error>;

fn application() -> Application {
    let mut application = Application::default();
    application.composition.profile.plugins.push(Selection {
        id: "widget".into(),
        plugin: "multi-host-widget".into(),
        version: "1.0.0".into(),
        provider: Some("local".into()),
        config: serde_json::json!({}),
    });
    application.releases.push(Release {
        plugin: "multi-host-widget".into(),
        version: "1.0.0".into(),
        kind: DistributionKind::Dynamic,
        facets: vec![
            FacetIdentity {
                id: "dynamic-consumer".into(),
                target: FacetTarget::Server,
            },
            FacetIdentity {
                id: "browser-widget".into(),
                target: FacetTarget::Browser,
            },
        ],
    });
    application
}

fn record_intent(runtime: &Runtime, version: Option<&str>, disable: bool) -> Result<(), Error> {
    let mut application: Application = serde_json::from_value(
        runtime
            .manager
            .composition()?
            .ok_or("acceptance Profile is absent")?,
    )?;
    if let Some(version) = version {
        application
            .composition
            .profile
            .plugins
            .iter_mut()
            .find(|selection| selection.id == "widget")
            .ok_or("widget selection is absent")?
            .version = version.into();
        application
            .releases
            .iter_mut()
            .find(|release| release.plugin == "multi-host-widget")
            .ok_or("widget release facts are absent")?
            .version = version.into();
    }
    if disable {
        application
            .composition
            .profile
            .disable
            .push("widget".into());
    }
    resolve_application(&application)?;
    runtime
        .manager
        .save_composition(&serde_json::to_value(application)?)?;
    Ok(())
}

async fn exercise_server(runtime: &Runtime) -> Result<(), Error> {
    let consumer = runtime
        .host
        .get(DynamicConsumer, DynamicConsumerInput {})
        .await?;
    if consumer.run().await? == 0 {
        return Err("server plugin failed to invoke its actual Counter dependency".into());
    }
    runtime
        .manager
        .observe_reference("multi-host-widget", consumer.core().clone())?;
    Ok(())
}

const SESSION_ID: &str = "acceptance-browser";

fn announcement(address: &str, event: &str) {
    println!(
        "{}",
        serde_json::json!({"event":event,"address":address,"session_id":SESSION_ID})
    );
}

/// Only the supervised local runner can submit observations on this process's stdin.
/// The embedding assigns the session; remote RPC exposes no reporting method.
fn report(runtime: &Runtime, address: &str, command: &str) -> Result<(), Error> {
    let mut value: serde_json::Value = serde_json::from_str(command)?;
    if value["action"] != "report" {
        return Err("unknown acceptance supervisor action".into());
    }
    value
        .as_object_mut()
        .ok_or("report must be an object")?
        .remove("action");
    value["session_id"] = serde_json::json!(SESSION_ID);
    let request: ReportBrowserRequest = serde_json::from_value(value)?;
    let observed = request.observed;
    let activation_id = request.activation_id.clone();
    runtime.manager.report_browser(request)?;
    println!(
        "{}",
        serde_json::json!({
            "event":"reported", "address":address, "session_id":SESSION_ID,
            "observed":observed, "activation_id":activation_id
        })
    );
    Ok(())
}

async fn commands(
    runtime: &Runtime,
    address: &str,
    mut input: mpsc::UnboundedReceiver<String>,
) -> Result<(), Error> {
    exercise_server(runtime).await?;
    announcement(address, "ready");
    while let Some(command) = input.next().await {
        match command.as_str() {
            "update" => {
                let permit = runtime.manager.begin_composition_reconciliation()?;
                record_intent(runtime, Some("1.1.0"), false)?;
                runtime
                    .manager
                    .update(UpdateRequest {
                        provider_id: "local".into(),
                        plugin_id: "multi-host-widget".into(),
                        version: "1.1.0".into(),
                        confirmed: true,
                    })
                    .await?;
                exercise_server(runtime).await?;
                drop(permit);
                announcement(address, "updated");
            }
            "deactivate" => {
                let permit = runtime.manager.begin_composition_reconciliation()?;
                record_intent(runtime, None, true)?;
                runtime
                    .manager
                    .deactivate(DeactivateRequest {
                        plugin_id: "multi-host-widget".into(),
                    })
                    .await?;
                drop(permit);
                announcement(address, "deactivated");
            }
            "close" => break,
            _ if command.starts_with('{') => report(runtime, address, &command)?,
            _ => return Err("unknown acceptance supervisor command".into()),
        }
    }
    Ok(())
}

fn main() -> Result<(), Error> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let database = PathBuf::from(args.first().ok_or("a temporary directory is required")?)
        .join("browser.sqlite");
    let source = PathBuf::from(args.get(1).ok_or("a fixture directory is required")?);
    let (sender, receiver) = mpsc::unbounded();
    std::thread::spawn(move || {
        for command in std::io::stdin().lock().lines() {
            match command {
                Ok(command) => {
                    if sender.unbounded_send(command).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    eprintln!("acceptance command input failed: {error}");
                    break;
                }
            }
        }
    });
    let driver = TokioDriver::new();
    let scheduler = driver.clone();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(scheduler.run_until(async {
            let runtime = start(
                driver.clone(),
                Configuration {
                    database,
                    source,
                    application: Some(application()),
                },
            )
            .await?;
            if !runtime.deployment_errors.is_empty() || runtime.recovery_error.is_some() {
                return Err(
                    format!("multi-Host Profile failed: {:?}", runtime.deployment_errors).into(),
                );
            }
            // Reserve a loopback port for this bounded test process. A concurrent bind failure
            // remains an explicit test failure; no listener is exposed beyond loopback.
            let reservation = std::net::TcpListener::bind("127.0.0.1:0")?;
            let address = reservation.local_addr()?;
            drop(reservation);
            let failure = RefCell::new(None);
            let stopped = async {
                if let Err(error) = commands(&runtime, &address.to_string(), receiver).await {
                    *failure.borrow_mut() = Some(error);
                }
            };
            swarmie_runtime::serve(&runtime, driver, address, stopped).await?;
            runtime.host.close().await?;
            if let Some(error) = failure.into_inner() {
                return Err(error);
            }
            Ok(())
        }))
}
