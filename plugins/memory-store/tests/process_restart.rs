use std::{env, path::PathBuf, process::Command};

use serde_json::json;
use swarmie_memory_store::{
    Configuration, GetRequest, GetResult, MemoryStore, MemoryStoreInput, PutRequest,
};
use tembra::{core, prelude::*};

#[test]
fn committed_data_survives_process_exit_without_resource_cleanup() {
    let directory = tempfile::tempdir().expect("test directory");
    let database = directory.path().join("memory.sqlite");
    for phase in ["write", "read"] {
        let output = Command::new(env::current_exe().expect("test executable"))
            .args(["--exact", "child_process", "--nocapture"])
            .env("SWARMIE_MEMORY_STORE_TEST_PHASE", phase)
            .env("SWARMIE_MEMORY_STORE_TEST_DATABASE", &database)
            .output()
            .expect("child process");
        assert!(
            output.status.success(),
            "{phase}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn child_process() -> Result<()> {
    let Some(phase) = env::var_os("SWARMIE_MEMORY_STORE_TEST_PHASE") else {
        return Ok(());
    };
    let database =
        PathBuf::from(env::var_os("SWARMIE_MEMORY_STORE_TEST_DATABASE").expect("child database"));
    let driver = core::ManualDriver::new();
    let host = Host::with_driver("storage-process", "application", driver.clone())?;
    driver.run_until(async move {
        host.install(
            swarmie_memory_store::facet(&Configuration { database })?,
            core::LoadOptions::default(),
        )?
        .await?;
        let memory = host.get(MemoryStore, MemoryStoreInput {}).await?;
        if phase == "write" {
            memory
                .put(PutRequest {
                    namespace: "durable".into(),
                    key: "document".into(),
                    value: json!({"revision":7,"empty":null}),
                })
                .await?;
            // Exit without Host or connection cleanup, after the durable write returned success.
            std::process::exit(0);
        } else {
            assert_eq!(phase, "read");
            assert_eq!(
                memory
                    .get(GetRequest {
                        namespace: "durable".into(),
                        key: "document".into()
                    })
                    .await?,
                GetResult::Present {
                    value: json!({"revision":7,"empty":null})
                }
            );
        }
        host.close().await
    })
}
