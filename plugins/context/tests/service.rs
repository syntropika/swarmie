use futures::{FutureExt, executor::LocalPool, future::LocalBoxFuture};
use std::{cell::Cell, rc::Rc};
use swarmie_commands::{CommandId, Invocation};
use swarmie_context::*;
use swarmie_providers::OperationId;
use tembra::{Code, Context, Host};

struct Source {
    calls: Cell<u32>,
    stops: Cell<u32>,
    fence: WorkFence,
}

impl Backend for Source {
    fn snapshot(&self) -> tembra::Result<Snapshot> {
        Ok(Snapshot {
            fence: Some(self.fence),
            can_compact: true,
            ..Snapshot::default()
        })
    }

    fn compact<'a>(
        &'a self,
        _: &'a Context,
        request: CompactionRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<Receipt>> {
        async move {
            if request.fence != self.fence {
                return Err(tembra::Error::invalid(
                    "fixture.stale_fence",
                    "the captured fence was retired",
                ));
            }
            let operation = request.operation;
            self.calls.set(self.calls.get() + 1);
            Ok(Receipt {
                requested: operation,
                operation,
                admission: Admission::Admitted,
                snapshot: self.snapshot()?,
            })
        }
        .boxed_local()
    }

    fn stop<'a>(
        &'a self,
        _: &'a Context,
        operation: OperationId,
    ) -> LocalBoxFuture<'a, tembra::Result<Receipt>> {
        async move {
            self.stops.set(self.stops.get() + 1);
            Ok(Receipt {
                requested: operation,
                operation,
                admission: Admission::Recognized,
                snapshot: self.snapshot()?,
            })
        }
        .boxed_local()
    }
}

#[test]
fn observation_is_read_only_and_command_and_button_reach_the_same_bound_backend() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let source = Rc::new(Source {
        calls: Cell::new(0),
        stops: Cell::new(0),
        fence: WorkFence::try_from(uuid::Uuid::new_v4()).unwrap(),
    });
    let bound = source.clone();
    let catalog = swarmie_commands::Catalog::default();
    let _registration = catalog
        .register(compact_command(vec!["session".into()]).unwrap())
        .unwrap();
    host.install(vec![
        orb(move |declaration| {
            let address = declaration.address()?;
            if address.principal != "alice" || address.key != ["session"] {
                return Err(tembra::Error::new(
                    Code::Forbidden,
                    "fixture.binding",
                    "conversation access refused",
                ));
            }
            Session::bind(&declaration, bound.clone())
        }),
        swarmie_commands::orb(host.clone(), move |declaration| {
            let address = declaration.address()?;
            if address.principal != "alice" || address.key != ["session"] {
                return Err(tembra::Error::new(
                    Code::Forbidden,
                    "fixture.binding",
                    "conversation access refused",
                ));
            }
            Ok(catalog.clone())
        }),
    ])
    .unwrap();
    let node = host.get(SERVICE, vec!["session".into()], "alice").unwrap();
    pool.run_until(node.ready()).unwrap();
    let projection = node.state::<Snapshot>().unwrap();
    let _observer = projection.subscribe(|_| Ok(())).unwrap();
    assert_eq!(source.calls.get(), 0);
    assert_eq!(source.stops.get(), 0);
    node.release().unwrap();
    assert_eq!(source.stops.get(), 0);

    let operation = OperationId::new();
    let fence = source.fence;
    let command = host
        .get(swarmie_commands::SERVICE, vec!["session".into()], "alice")
        .unwrap();
    let receipt: serde_json::Value = pool
        .run_until(host.root("alice", move |ctx| {
            async move {
                let receipt = command
                    .call(
                        &ctx,
                        "invoke",
                        (Invocation {
                            command: CommandId::new(swarmie_commands::COMPACT)?,
                            operation,
                            arguments: serde_json::to_value(CompactArguments { fence }).unwrap(),
                        },),
                    )
                    .await;
                command.release()?;
                receipt
            }
            .boxed_local()
        }))
        .unwrap();
    let receipt: Receipt = serde_json::from_value(receipt).unwrap();
    assert_eq!(receipt.operation, operation);
    assert_eq!(source.calls.get(), 1);

    let node = host.get(SERVICE, vec!["session".into()], "alice").unwrap();
    let second = OperationId::new();
    let _: Receipt = pool
        .run_until(host.root("alice", move |ctx| {
            async move {
                let receipt = node
                    .call(
                        &ctx,
                        "compact",
                        (CompactionRequest {
                            operation: second,
                            fence,
                        },),
                    )
                    .await;
                node.release()?;
                receipt
            }
            .boxed_local()
        }))
        .unwrap();
    assert_eq!(source.calls.get(), 2);
    assert_eq!(source.stops.get(), 0);
    let forbidden = host.get(SERVICE, vec!["session".into()], "mallory");
    assert!(forbidden.is_err() || pool.run_until(forbidden.unwrap().ready()).is_err());
    pool.run_until(host.close()).unwrap();
}

#[test]
fn compact_requires_a_non_nil_captured_fence_and_strict_command_arguments() {
    assert!(serde_json::from_value::<WorkFence>(serde_json::json!(uuid::Uuid::nil())).is_err());
    assert!(
        Snapshot {
            can_compact: true,
            ..Snapshot::default()
        }
        .validate()
        .is_err()
    );
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let source = Rc::new(Source {
        calls: Cell::new(0),
        stops: Cell::new(0),
        fence: WorkFence::try_from(uuid::Uuid::new_v4()).unwrap(),
    });
    let retained = source.clone();
    host.install(vec![orb(move |declaration| {
        Session::bind(&declaration, source.clone())
    })])
    .unwrap();
    for arguments in [
        serde_json::Value::Null,
        serde_json::json!({}),
        serde_json::json!({"fence":retained.fence,"extra":true}),
        serde_json::json!({"fence":uuid::Uuid::nil()}),
    ] {
        let reference = host.get(SERVICE, vec!["session".into()], "alice").unwrap();
        let result: tembra::Result<serde_json::Value> =
            pool.run_until(host.root("alice", move |context| {
                async move {
                    let result = reference
                        .call(
                            &context,
                            "command",
                            (Invocation {
                                command: CommandId::new(swarmie_commands::COMPACT)?,
                                operation: OperationId::new(),
                                arguments,
                            },),
                        )
                        .await;
                    reference.release()?;
                    result
                }
                .boxed_local()
            }));
        assert!(result.is_err());
        pool.run_until_stalled();
    }
    let reference = host.get(SERVICE, vec!["session".into()], "alice").unwrap();
    let result: tembra::Result<Receipt> = pool.run_until(host.root("alice", move |context| {
        async move {
            let result = reference
                .call(
                    &context,
                    "compact",
                    (CompactionRequest {
                        operation: OperationId::new(),
                        fence: WorkFence::try_from(uuid::Uuid::new_v4())?,
                    },),
                )
                .await;
            reference.release()?;
            result
        }
        .boxed_local()
    }));
    assert_eq!(result.unwrap_err().reason, "fixture.stale_fence");
    assert_eq!(retained.calls.get(), 0);
    pool.run_until(host.close()).unwrap();
}
