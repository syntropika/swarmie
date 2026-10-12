#![cfg(unix)]
use adapters::{AcpHarness, PiHarness, builtin_builder};
use futures_util::StreamExt;
use swarmie::*;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("execution-policy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn options(&self, harness: &str) -> SpawnOptions {
        let mut options = SpawnOptions::new(harness, &self.0);
        options.config = json!({"executable":"python3","args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/policy_agent.py"),harness]});
        options
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn turn(agent: &Agent, text: &str) -> Value {
    let mut events = agent.events();
    agent.prompt(text).await.unwrap();
    let mut output = String::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            match event.unwrap().kind {
                EventKind::TextDelta { text, .. } => output.push_str(&text),
                EventKind::TurnEnded { .. } => break,
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    serde_json::from_str(&output).unwrap()
}
#[tokio::test]
async fn server_profiles_stage_codex_and_acknowledge_claude_without_client_native_patches() {
    let fixture = Fixture::new();
    for harness in ["codex", "claude"] {
        let runtime = builtin_builder().build();
        let mut options = fixture.options(harness);
        options.config["executionProfile"] = json!("ask");
        let agent = runtime.spawn(options).await.unwrap();
        let before = agent.snapshot().await;
        assert_eq!(
            before.execution_policy.requested,
            Some(ExecutionProfile::Ask)
        );
        assert_eq!(
            before.execution_policy.status,
            if harness == "codex" {
                PolicyStatus::Staged
            } else {
                PolicyStatus::Acknowledged
            }
        );
        assert!(before.execution_policy.native_session.is_some());
        assert!(before.execution_policy.binding_id.is_some());
        assert!(
            agent
                .capabilities()
                .await
                .operation_approvals
                .is_available()
        );
        let controls = agent.configuration().await.unwrap();
        assert!(controls.iter().any(|field| field.id == "executionProfile"));
        let receipt = agent
            .configure(json!({"executionProfile":"ask"}))
            .await
            .unwrap();
        assert_eq!(receipt.values["executionProfile"], "ask");
        assert!(!receipt.execution_policy.dispatch_blocked);
        let native = turn(&agent, "receipt").await;
        assert!(native.get("executionProfile").is_none());
        if harness == "codex" {
            assert_eq!(native["approvalPolicy"], "on-request");
            assert_eq!(native["sandboxPolicy"]["type"], "workspaceWrite");
            assert_eq!(native["sandboxPolicy"]["writableRoots"], json!([fixture.0]));
        } else {
            assert_eq!(native["permissionMode"], "default");
        }
        let after = agent.snapshot().await;
        assert_eq!(after.execution_policy.status, PolicyStatus::Acknowledged);
        assert!(!after.execution_policy.dispatch_blocked);
        assert_eq!(
            after.execution_policy.binding_id,
            before.execution_policy.binding_id
        );
        assert_eq!(
            serde_json::to_value(after.execution_policy.native_session).unwrap(),
            serde_json::to_value(before.execution_policy.native_session).unwrap()
        );
        agent.close().await.unwrap();
    }
}
#[tokio::test]
async fn restrictive_and_external_native_policies_never_become_full_access_and_keep_all_fields() {
    let fixture = Fixture::new();
    for policy in [
        json!({"type":"readOnly","networkAccess":true,"allowedDomains":["example.test"]}),
        json!({"type":"workspaceWrite","networkAccess":true,"writableRoots":["/extra-scope"]}),
        json!({"type":"workspaceWrite","networkAccess":false,"writableRoots":["/extra-scope"]}),
        json!({"type":"dangerFullAccess","customRestrictions":["opaque"]}),
        json!({"type":"externalSandbox","networkAccess":"enabled","customPolicy":{"readRoots":["/policy-scope"]}}),
    ] {
        let runtime = builtin_builder().build();
        let mut options = fixture.options("codex");
        options
            .execution
            .env
            .insert("SWARMIE_POLICY_OBSERVATION".into(), policy.to_string());
        let agent = runtime.spawn(options).await.unwrap();
        let receipt = agent
            .configure(json!({"approvalPolicy":"never"}))
            .await
            .unwrap();
        assert_eq!(
            receipt.execution_policy.observed.as_ref().unwrap().profile,
            None
        );
        assert_eq!(receipt.values["sandbox"], policy);
        let native = turn(&agent, "receipt").await;
        assert_eq!(native["sandboxPolicy"], policy);
        assert_ne!(
            agent
                .snapshot()
                .await
                .execution_policy
                .observed
                .unwrap()
                .profile,
            Some(ExecutionProfile::Full)
        );
        agent.close().await.unwrap();
    }
}
#[tokio::test]
async fn unsupported_equivalents_and_conflicting_native_fields_are_rejected_before_changes() {
    let fixture = Fixture::new();
    let runtime = builtin_builder().build();
    let agent = runtime.spawn(fixture.options("claude")).await.unwrap();
    let before = agent.snapshot().await;
    assert!(matches!(
        agent.configure(json!({"executionProfile":"full"})).await,
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        agent
            .configure(json!({"executionProfile":"ask","permissionMode":"bypassPermissions"}))
            .await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(
        agent.snapshot().await.execution_policy.revision,
        before.execution_policy.revision
    );
    let receipt = agent
        .configure(json!({"permissionMode":"bypassPermissions"}))
        .await
        .unwrap();
    assert_eq!(receipt.execution_policy.requested, None);
    assert_ne!(
        receipt.execution_policy.observed.unwrap().profile,
        Some(ExecutionProfile::Full)
    );
    agent.close().await.unwrap();
    let mut options = fixture.options("pi");
    options.config["executionProfile"] = json!("ask");
    assert!(matches!(
        runtime.spawn(options.clone()).await,
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        PiHarness.compile_execution_profile(&options, ExecutionProfile::Ask),
        Err(Error::Unsupported(_))
    ));
    assert_eq!(
        PiHarness
            .detect()
            .await
            .unwrap()
            .capabilities
            .operation_approvals
            .availability,
        CapabilityAvailability::Unsupported
    );
    assert_eq!(
        AcpHarness::new("acp", "python3", vec![])
            .detect()
            .await
            .unwrap()
            .capabilities
            .operation_approvals
            .availability,
        CapabilityAvailability::Unknown
    );
}
#[tokio::test]
async fn retained_profile_is_recompiled_on_resume_and_keeps_the_host_context() {
    let fixture = Fixture::new();
    let runtime = builtin_builder().build();
    let mut options = fixture.options("codex");
    options.config["executionProfile"] = json!("full");
    let agent = runtime.spawn(options).await.unwrap();
    turn(&agent, "before").await;
    let saved = agent.snapshot().await;
    agent.close().await.unwrap();
    let resumed = runtime.resume(&saved.id).await.unwrap();
    let after = resumed.snapshot().await;
    assert_eq!(
        after.execution_policy.requested,
        Some(ExecutionProfile::Full)
    );
    assert_eq!(
        after.execution_policy.revision,
        saved.execution_policy.revision
    );
    assert_eq!(
        after.working_context.unwrap().id,
        saved.working_context.unwrap().id
    );
    let native = turn(&resumed, "after").await;
    assert_eq!(native["approvalPolicy"], "never");
    assert_eq!(native["sandboxPolicy"]["type"], "dangerFullAccess");
    resumed.close().await.unwrap();
}
