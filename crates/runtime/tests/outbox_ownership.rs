//! Real JavaScript callers own durable consumer positions across compatible replacement.

use std::{collections::BTreeMap, rc::Rc, time::Duration};

use serde_json::json;
use swarmie_task_contracts::*;
use tembra::{core, native::TokioDriver, prelude::*};
use tembra_executor_js::{BundledJsArtifact, Executor, ExecutorOptions};

async fn caller(
    host: &Host,
    executor: &Executor,
    name: &str,
    generation: u32,
    through: &TaskOutboxEventId,
) -> (core::ExecutableFacet, Rc<core::contracts::Contract>) {
    let token = Rc::new(
        core::contracts::Contract::local(
            format!("{name}.control"),
            1,
            json!({}),
            BTreeMap::from([(
                "invoke".into(),
                core::contracts::Member::Method {
                    args: Some(core::contracts::ValueSchema::Portable(json!({
                        "type":"string", "enum":["register", "read", "ack", "remove"]
                    }))),
                    result: Some(core::contracts::ValueSchema::Portable(
                        json!({"type":"integer", "minimum":1, "maximum":2}),
                    )),
                },
            )]),
        )
        .unwrap(),
    );
    let source = format!(
        r#"
const storage = {storage}, control = {control}, through = {through};
export default {{ name: {name}, uses: [storage], setup(_scope, declare) {{
  declare.serve(control, (_input, self) => {{
    const target = self.get(storage, {{}});
    return {{ methods: {{ async invoke(context, operation) {{
      switch (operation) {{
        case 'register': await target.register_outbox_consumer({{id:'session-history'}}, context); break;
        case 'read': await target.read_outbox({{consumer_id:'session-history', after:null, limit:1}}, context); break;
        case 'ack': await target.acknowledge_outbox({{consumer_id:'session-history', through}}, context); break;
        case 'remove': await target.remove_outbox_consumer({{id:'session-history'}}, context); break;
      }}
      return {generation};
    }} }} }};
  }}, {{lifetime:'host'}});
}} }};
"#,
        storage = serde_json::to_string(TaskStorage.descriptor().unwrap().descriptor()).unwrap(),
        control = serde_json::to_string(token.descriptor()).unwrap(),
        through = serde_json::to_string(through).unwrap(),
        name = json!(name),
    );
    let facet = executor
        .prepare(
            host.core().id(),
            BundledJsArtifact::new(
                format!("{name}-{generation}"),
                source,
                vec!["default".into()],
            )
            .unwrap(),
            "default".into(),
        )
        .await
        .unwrap();
    (facet, token)
}

#[test]
fn javascript_consumer_ownership_survives_reload_and_refuses_other_facets() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            tokio::time::timeout(Duration::from_secs(15), verify(driver.clone()))
                .await
                .expect("JavaScript consumer ownership timed out");
        }));
}

async fn verify(driver: Rc<TokioDriver>) {
    let directory = tempfile::tempdir().unwrap();
    let host = Host::with_driver("outbox-js", "test", driver.clone()).unwrap();
    let configuration: swarmie_task_storage_sqlite::Configuration = serde_json::from_value(json!({
        "database":directory.path().join("tasks.sqlite")
    }))
    .unwrap();
    host.install(
        swarmie_task_storage_sqlite::facet(&configuration).unwrap(),
        core::LoadOptions::default(),
    )
    .unwrap()
    .await
    .unwrap();
    let storage = host.get(TaskStorage, TaskStorageInput {}).await.unwrap();
    storage
        .claim_owner(StorageOwner {
            id: "writer".into(),
        })
        .await
        .unwrap();
    let record = TaskRecord::new(
        TaskSpec {
            id: "task".into(),
            kind: "fixture.outbox-owner".into(),
            definition_version: 1,
            phase: "start".into(),
            input: json!(null),
            checkpoint: json!(null),
        },
        None,
    );
    let receipt = storage
        .commit(TaskTransaction {
            owner: "writer".into(),
            writes: vec![TaskWrite {
                provenance: None,
                expected_revision: None,
                record,
            }],
        })
        .await
        .unwrap();
    let CommitResult::Committed { sequence, .. } = receipt else {
        panic!("fresh commit conflicted")
    };
    let through = TaskOutboxEventId {
        store_id: storage.outbox_status().await.unwrap().store_id,
        sequence,
        ordinal: 0,
    };
    let executor = Executor::new(driver, ExecutorOptions::default()).unwrap();
    let (publisher, token) =
        caller(&host, &executor, "fixture.session-publisher", 1, &through).await;
    host.core()
        .install_executable(publisher.clone(), core::LoadOptions::default())
        .unwrap()
        .await
        .unwrap();
    let reference = host
        .core()
        .get(token, json!({}), core::GetOptions::default())
        .await
        .unwrap();
    for operation in ["register", "read"] {
        assert_eq!(
            reference
                .call_json("invoke", Some(json!(operation)), None)
                .await
                .unwrap(),
            Some(json!(1))
        );
    }
    let before = storage.outbox_status().await.unwrap();
    assert_eq!(before.consumers[0].owner_facet, "fixture.session-publisher");
    assert_eq!(before.consumers[0].delivered.as_ref(), Some(&through));
    assert_eq!(before.consumers[0].acknowledged, None);

    let (intruder, token) = caller(&host, &executor, "fixture.other-observer", 1, &through).await;
    host.core()
        .install_executable(intruder, core::LoadOptions::default())
        .unwrap()
        .await
        .unwrap();
    let other = host
        .core()
        .get(token, json!({}), core::GetOptions::default())
        .await
        .unwrap();
    for operation in ["register", "read", "ack", "remove"] {
        let error = other
            .call_json("invoke", Some(json!(operation)), None)
            .await
            .unwrap_err();
        assert_eq!(error.reason, "task_storage_outbox_owner_mismatch");
        assert_eq!(storage.outbox_status().await.unwrap(), before);
    }
    let (replacement, _) = caller(&host, &executor, "fixture.session-publisher", 2, &through).await;
    host.core()
        .reload(vec![replacement], core::ReloadOptions::default())
        .await
        .unwrap();
    for operation in ["register", "read", "ack"] {
        assert_eq!(
            reference
                .call_json("invoke", Some(json!(operation)), None)
                .await
                .unwrap(),
            Some(json!(2))
        );
    }
    let after = storage.outbox_status().await.unwrap();
    assert_eq!(after.consumers.len(), 1);
    assert_eq!(
        after.consumers[0].owner_facet,
        before.consumers[0].owner_facet
    );
    assert_eq!(after.consumers[0].acknowledged.as_ref(), Some(&through));
    assert_eq!(after.retained_events, 1, "acknowledgement does not compact");
    host.close().await.unwrap();
}
