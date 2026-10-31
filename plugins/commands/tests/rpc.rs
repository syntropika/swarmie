use futures::executor::LocalPool;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use swarmie_commands::*;
use swarmie_providers::OperationId;
use tembra::{
    Host, Orb, Service,
    rpc::{Options, serve},
};

#[test]
fn discovery_and_invocation_cross_the_actual_tembra_codec_with_fixed_authority() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let catalog = Catalog::default();
    let _registration = catalog
        .register(Command {
            id: CommandId::new(COMPACT).unwrap(),
            label: "Compact context".into(),
            description: "Compact the selected conversation".into(),
            aliases: vec!["/compact".into()],
            target: Target {
                service: "fixture.context".into(),
                key: vec!["session".into()],
                method: "compact".into(),
            },
        })
        .unwrap();
    let calls = Rc::new(Cell::new(0));
    let dispatched = calls.clone();
    host.install(vec![
        Orb::new("fixture.context", 1).service(Service::new("fixture.context", |_| Ok(())).method(
            "compact",
            move |_, context, (input,): (Invocation,)| {
                let calls = calls.clone();
                async move {
                    calls.set(calls.get() + 1);
                    Ok(json!({"operation":input.operation,"principal":context.principal()?}))
                }
            },
        )),
        orb(host.clone(), move |declaration| {
            let address = declaration.address()?;
            let principal = address.principal;
            let key = address.key;
            assert_eq!(principal, "alice");
            assert_eq!(key, ["session"]);
            Ok(catalog.clone())
        }),
    ])
    .unwrap();
    let frames = Rc::new(RefCell::new(Vec::<Value>::new()));
    let received = frames.clone();
    let connection = serve(
        host.clone(),
        Options::new("alice".into(), |service, key| {
            service == SERVICE && key.is_some_and(|key| key == ["session"])
        }),
        move |bytes| {
            received
                .borrow_mut()
                .push(serde_json::from_slice(&bytes).unwrap());
            Ok(())
        },
    );
    let request = |frame: Value| {
        connection
            .receive(&serde_json::to_vec(&frame).unwrap())
            .unwrap()
    };
    let response = |id: u64| {
        frames
            .borrow()
            .iter()
            .find(|frame| frame["id"] == id)
            .unwrap()
            .clone()
    };
    request(json!({"id":1,"op":"get","payload":{"service":SERVICE,"key":["session"]}}));
    pool.run_until_stalled();
    let reference = response(1)["ok"]["reference"].as_u64().unwrap();
    request(
        json!({"id":2,"op":"call","payload":{"reference":reference,"method":"list","arguments":[]}}),
    );
    pool.run_until_stalled();
    assert_eq!(response(2)["ok"][0]["id"], COMPACT);
    assert_eq!(response(2)["ok"][0]["aliases"], json!(["/compact"]));
    assert_eq!(dispatched.get(), 0);
    let operation = OperationId::new();
    request(
        json!({"id":3,"op":"call","payload":{"reference":reference,"method":"invoke","principal":"host","arguments":[{"command":COMPACT,"operation":operation,"arguments":null}]}}),
    );
    pool.run_until_stalled();
    assert!(
        response(3).get("ok").is_some(),
        "unexpected command response: {:?}",
        response(3)
    );
    assert_eq!(response(3)["ok"]["operation"], json!(operation));
    assert_eq!(response(3)["ok"]["principal"], "alice");
    assert_eq!(dispatched.get(), 1);
    request(
        json!({"id":4,"op":"call","payload":{"reference":reference,"method":"invoke","arguments":[{"command":"/compact","operation":operation,"arguments":null}]}}),
    );
    pool.run_until_stalled();
    assert!(response(4).get("error").is_some());
    assert_eq!(dispatched.get(), 1);
    connection.close();
    pool.run_until(host.close()).unwrap();
    assert_eq!(host.kernel().diagnostics().resources, 0);
}
