#![cfg(all(feature = "coding", any(unix, windows)))]

use futures::{FutureExt, executor::LocalPool};
use serde_json::{Value, json};
use std::{cell::Cell, rc::Rc, time::Duration};
use swarmie_tools::{
    coding::{CodingConfig, CodingTools},
    *,
};
use tembra::{Code, Host, Orb, Reference, Service};

struct Fixture {
    pool: LocalPool,
    host: Host,
    client: Option<Rc<Reference>>,
    allow: Rc<Cell<bool>>,
}

impl Fixture {
    fn new(base: &std::path::Path) -> Self {
        let pool = LocalPool::new();
        let host = Host::new(pool.spawner());
        let allow = Rc::new(Cell::new(true));
        let admitted = allow.clone();
        let policy = ToolPolicy::new(move |ctx, _, _| {
            let admitted = admitted.clone();
            async move {
                Ok(if ctx.principal()? == "alice" && admitted.get() {
                    ToolDecision::Allow
                } else {
                    ToolDecision::Block("not permitted".into())
                })
            }
            .boxed_local()
        });
        let mut coding = CodingTools::new(CodingConfig::local(base).unwrap(), policy).unwrap();
        let catalog = Catalog::new(coding.tools).unwrap();
        coding.orbs.push(orb(host.clone(), catalog));
        let host_for_client = host.clone();
        coding.orbs.push(
            Orb::new("fixture.client", 1).service(
                Service::new("fixture.client", move |d| {
                    CatalogService::bind(
                        &d,
                        host_for_client.clone(),
                        SERVICE,
                        vec![INSTANCE_KEY.into()],
                    )
                })
                .method("execute", |tools, ctx, call: ToolCall| async move {
                    let spec = tools
                        .list(&ctx)
                        .await?
                        .tools
                        .into_iter()
                        .find(|tool| tool.spec.name == call.name)
                        .unwrap()
                        .spec;
                    let prepared = tools.prepare(&ctx, &spec, &call).await?;
                    match prepared.decision {
                        ToolDecision::Block(reason) => Ok(ToolResult::Blocked(reason)),
                        ToolDecision::Allow => {
                            tools
                                .execute(
                                    &ctx,
                                    ToolExecution {
                                        operation: OperationId::new(),
                                        call: &call,
                                        provider: prepared.provider.as_ref(),
                                    },
                                )
                                .await
                        }
                    }
                }),
            ),
        );
        host.install(coding.orbs).unwrap();
        let client = Some(Rc::new(
            host.get("fixture.client", vec!["main".into()], "alice")
                .unwrap(),
        ));
        Self {
            pool,
            host,
            client,
            allow,
        }
    }

    fn call(&mut self, name: &str, arguments: Value) -> tembra::Result<ToolResult> {
        let call = ToolCall {
            id: ToolCallId::new("coding-call").unwrap(),
            name: ToolName::new(name).unwrap(),
            arguments,
        };
        let reference = self.client.as_ref().unwrap().clone();
        self.pool.run_until(self.host.root("alice", move |ctx| {
            async move { reference.call(&ctx, "execute", call).await }.boxed_local()
        }))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.client.take().unwrap().release().unwrap();
        self.pool.run_until(self.host.close()).unwrap();
        assert_eq!(self.host.kernel().diagnostics().resources, 0);
    }
}

fn success(result: ToolResult) -> String {
    match result {
        ToolResult::Success(Value::String(text)) => text,
        other => panic!("expected text success: {other:?}"),
    }
}

#[test]
fn all_eight_tools_execute_through_the_catalog_and_tembra_policy() {
    let root = tempfile::tempdir().unwrap();
    let mut fixture = Fixture::new(root.path());
    success(
        fixture
            .call("write", json!({"path":"src/a.txt","content":"one\ntwo\n"}))
            .unwrap(),
    );
    assert!(
        success(
            fixture
                .call("read", json!({"path":"src/a.txt","offset":2,"limit":1}))
                .unwrap()
        )
        .contains("two")
    );
    success(
        fixture
            .call(
                "edit",
                json!({"path":"src/a.txt","edits":[{"old_text":"two","new_text":"three"}]}),
            )
            .unwrap(),
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("src/a.txt")).unwrap(),
        "one\nthree\n"
    );
    assert!(
        success(fixture.call("search", json!({"pattern":"three"})).unwrap())
            .contains("a.txt:2: three")
    );
    assert!(success(fixture.call("list", json!({"path":"src"})).unwrap()).contains("a.txt"));
    assert!(
        success(
            fixture
                .call("shell", json!({"command":"echo coding-shell-ok"}))
                .unwrap()
        )
        .contains("coding-shell-ok")
    );
    success(
        fixture
            .call("move", json!({"from":"src/a.txt","to":"src/b.txt"}))
            .unwrap(),
    );
    success(fixture.call("delete", json!({"path":"src/b.txt"})).unwrap());
    assert!(!root.path().join("src/a.txt").exists());
    assert!(!root.path().join("src/b.txt").exists());
    fixture.allow.set(false);
    assert!(matches!(
        fixture
            .call("write", json!({"path":"denied.txt","content":"no"}))
            .unwrap(),
        ToolResult::Blocked(_)
    ));
    assert!(!root.path().join("denied.txt").exists());
}

#[test]
fn malformed_calls_and_paths_outside_the_base_never_change_files() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("project");
    std::fs::create_dir(&base).unwrap();
    let mut fixture = Fixture::new(&base);
    assert!(matches!(
        fixture
            .call("write", json!({"path":"a.txt","content":4}))
            .unwrap(),
        ToolResult::Blocked(_)
    ));
    assert!(matches!(
        fixture
            .call("write", json!({"path":"../outside.txt","content":"no"}))
            .unwrap(),
        ToolResult::Failed(_)
    ));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.path(), base.join("outside")).unwrap();
        assert!(matches!(
            fixture
                .call("write", json!({"path":"outside/escape.txt","content":"no"}))
                .unwrap(),
            ToolResult::Failed(_)
        ));
    }
    #[cfg(windows)]
    {
        let junction = base.join("outside");
        let created = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(root.path())
            .output()
            .unwrap();
        assert!(created.status.success(), "{created:?}");
        assert!(matches!(
            fixture
                .call("write", json!({"path":"outside/escape.txt","content":"no"}))
                .unwrap(),
            ToolResult::Failed(_)
        ));
        std::fs::remove_dir(junction).unwrap();
    }
    assert!(!root.path().join("outside.txt").exists());
    assert!(!root.path().join("escape.txt").exists());
    assert!(!base.join("a.txt").exists());
}

#[test]
fn policy_is_rechecked_after_preparation_and_before_receipt_access() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let allowed = Rc::new(Cell::new(true));
    let check = allowed.clone();
    let root = tempfile::tempdir().unwrap();
    let write = coding::Write {
        base: Some(root.path().to_owned()),
        confine: true,
        ..Default::default()
    };
    let handler = ToolHandler::new(write)
        .unwrap()
        .with_policy(ToolPolicy::new(move |_, _, _| {
            let allowed = check.get();
            async move {
                Ok(if allowed {
                    ToolDecision::Allow
                } else {
                    ToolDecision::Block("revoked".into())
                })
            }
            .boxed_local()
        }));
    pool.run_until(host.root("alice", move |ctx| {
        async move {
            let call = ToolCall {
                id: ToolCallId::new("write").unwrap(),
                name: ToolName::new("write").unwrap(),
                arguments: json!({"path":"no.txt","content":"no"}),
            };
            assert_eq!(
                handler.prepare(&ctx, handler.spec(), &call).await?.decision,
                ToolDecision::Allow
            );
            allowed.set(false);
            let execution = ToolExecution::new(OperationId::new(), &call);
            assert_eq!(
                handler.execute(&ctx, execution).await?,
                ToolResult::Blocked("revoked".into())
            );
            assert_eq!(
                handler.recover(&ctx, execution).await.unwrap_err().code,
                Code::Forbidden
            );
            assert!(!root.path().join("no.txt").exists());
            Ok(())
        }
        .boxed_local()
    }))
    .unwrap();
    pool.run_until(host.close()).unwrap();
}

#[test]
fn cancellation_stops_the_shell_and_waits_for_physical_settlement() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("late.txt");
    let shell = coding::Shell {
        launcher: coding::process::Launcher::default().in_dir(dir.path()),
        ..Default::default()
    };
    let handler = ToolHandler::new(shell).unwrap();
    let command = if cfg!(windows) {
        "Start-Sleep -Seconds 2; Set-Content late.txt late"
    } else {
        "sleep 2; printf late > late.txt"
    };
    pool.run_until(host.root("alice", move |ctx| {
        async move {
            let call = ToolCall {
                id: ToolCallId::new("shell").unwrap(),
                name: ToolName::new("shell").unwrap(),
                arguments: json!({"command":command}),
            };
            let execution = ToolExecution::new(OperationId::new(), &call);
            let cancel = async {
                futures_timer::Delay::new(Duration::from_millis(100)).await;
                ctx.cancel().unwrap();
            };
            let (result, ()) = futures::join!(handler.execute(&ctx, execution), cancel);
            assert!(result.is_err());
            assert!(!marker.exists());
            Ok(())
        }
        .boxed_local()
    }))
    .unwrap();
    pool.run_until(host.close()).unwrap();
}
