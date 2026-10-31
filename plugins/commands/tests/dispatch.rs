use futures::{FutureExt, executor::LocalPool};
use serde_json::{Value, json};
use std::{cell::Cell, rc::Rc};
use swarmie_commands::*;
use swarmie_providers::OperationId;
use tembra::{Code, Host, Orb, Service};

fn compact(alias: &str) -> Command {
    Command {
        id: CommandId::new(COMPACT).unwrap(),
        label: "Compact context".into(),
        description: "Compact the selected conversation".into(),
        aliases: vec![alias.into()],
        target: Target {
            service: "fixture.context".into(),
            key: vec!["conversation".into()],
            method: "compact".into(),
        },
    }
}

fn invoke(
    pool: &mut LocalPool,
    host: &Host,
    principal: &str,
    invocation: Invocation,
) -> tembra::Result<Value> {
    let reference = host.get(SERVICE, vec!["main".into()], principal)?;
    pool.run_until(host.root(principal, move |ctx| {
        async move {
            let outcome = reference.call(&ctx, "invoke", (invocation,)).await;
            reference.release()?;
            outcome
        }
        .boxed_local()
    }))
}

#[test]
fn command_identity_is_validated_and_alias_is_not_an_invocation_identity() {
    for invalid in [
        "",
        "/compact",
        "Swarmie.context.compact",
        "swarmie..compact",
        "swarmie.context. compact",
    ] {
        assert!(CommandId::new(invalid).is_err());
        assert!(serde_json::from_value::<CommandId>(json!(invalid)).is_err());
    }
    assert_eq!(CommandId::new(COMPACT).unwrap().as_str(), COMPACT);
    let mut definition = compact("/compact");
    definition.aliases.push("/compact".into());
    assert!(definition.validate().is_err());
}

#[test]
fn registration_withdrawal_and_alias_changes_keep_semantic_identity() {
    let catalog = Catalog::default();
    let old = catalog.register(compact("/compact")).unwrap();
    assert_eq!(
        catalog.register(compact("/compact")).err().unwrap().code,
        Code::Conflict
    );
    let mut collision = compact("/compact");
    collision.id = CommandId::new("fixture.other").unwrap();
    assert_eq!(
        catalog.register(collision).err().unwrap().code,
        Code::Conflict
    );
    old.withdraw();
    let current = catalog.register(compact("/shorten")).unwrap();
    drop(old);
    let command = catalog.get(&CommandId::new(COMPACT).unwrap()).unwrap();
    assert_eq!(command.aliases, ["/shorten"]);
    drop(current);
    assert!(catalog.list().is_empty());
}

#[test]
fn actual_tembra_handler_receives_exact_identity_and_principal_without_chat_dispatch() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let catalog = Catalog::default();
    let registration = catalog.register(compact("/compact")).unwrap();
    let dispatched = Rc::new(Cell::new(0));
    let calls = dispatched.clone();
    host.install(vec![
        Orb::new("fixture.context", 1).service(Service::new("fixture.context", |d| {
            assert_eq!(d.address()?.principal, "alice");
            assert_eq!(d.address()?.key, ["conversation"]);
            Ok(())
        }).method("compact", move |_, ctx, (input,): (Invocation,)| {
            let calls = calls.clone();
            async move {
                calls.set(calls.get() + 1);
                Ok(json!({"principal": ctx.principal()?, "operation": input.operation, "command": input.command, "arguments": input.arguments}))
            }
        })),
        orb(host.clone(), move |declaration| {
            let principal=declaration.address()?.principal;
            if principal == "alice" { Ok(catalog.clone()) }
            else { Err(tembra::Error::new(Code::Forbidden, "fixture.principal", "unbound principal")) }
        }),
    ]).unwrap();
    let operation = OperationId::new();
    let input = Invocation {
        command: CommandId::new(COMPACT).unwrap(),
        operation,
        arguments: json!({"conversation":"selected"}),
    };
    let output = invoke(&mut pool, &host, "alice", input.clone()).unwrap();
    assert_eq!(output["principal"], "alice");
    assert_eq!(output["operation"], json!(operation));
    assert_eq!(output["command"], COMPACT);
    assert_eq!(output["arguments"], input.arguments);
    assert_eq!(dispatched.get(), 1);
    assert_eq!(
        invoke(&mut pool, &host, "mallory", input.clone())
            .unwrap_err()
            .code,
        Code::Forbidden
    );
    assert_eq!(dispatched.get(), 1);
    drop(registration);
    assert_eq!(
        invoke(&mut pool, &host, "alice", input).unwrap_err().code,
        Code::Unavailable
    );
    assert_eq!(dispatched.get(), 1);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn unknown_handler_outcome_is_returned_without_retries() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let catalog = Catalog::default();
    let _registration = catalog.register(compact("/compact")).unwrap();
    let calls = Rc::new(Cell::new(0));
    let dispatched = calls.clone();
    host.install(vec![
        Orb::new("fixture.context", 1).service(Service::new("fixture.context", |_| Ok(())).method(
            "compact",
            move |_, _, (_,): (Invocation,)| {
                let calls = calls.clone();
                async move {
                    calls.set(calls.get() + 1);
                    Err::<Value, _>(tembra::Error::new(
                        Code::Unavailable,
                        "fixture.lost",
                        "outcome unknown",
                    ))
                }
            },
        )),
        orb(host.clone(), move |_| Ok(catalog.clone())),
    ])
    .unwrap();
    let outcome = invoke(
        &mut pool,
        &host,
        "alice",
        Invocation {
            command: CommandId::new(COMPACT).unwrap(),
            operation: OperationId::new(),
            arguments: json!({}),
        },
    );
    assert_eq!(outcome.unwrap_err().reason, "fixture.lost");
    assert_eq!(dispatched.get(), 1);
    pool.run_until(host.close()).unwrap();
}
