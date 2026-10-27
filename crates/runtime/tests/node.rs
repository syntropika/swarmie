use futures::{FutureExt, executor::LocalPool};
use swarmie_runtime::{
    NODE_KEY, NODE_PROTOCOL_VERSION, NODE_SERVICE, NodeId, NodeIdentity, NodeMethod, node_orb,
};
use tembra::Host;

#[test]
fn node_identity_uses_the_configured_id_and_each_invocations_principal() {
    let mut executor = LocalPool::new();
    let host = Host::new(executor.spawner());
    executor.run_until(async {
        host.install(vec![node_orb(NodeId::new("stable-node").unwrap())])
            .unwrap();
        for principal in ["alice", "bob"] {
            let reference = host
                .get(NODE_SERVICE, vec![NODE_KEY.into()], principal)
                .unwrap();
            let identity: NodeIdentity = host
                .root(principal, move |context| {
                    async move {
                        reference
                            .call(&context, NodeMethod::Identify.as_str(), ())
                            .await
                    }
                    .boxed_local()
                })
                .await
                .unwrap();
            assert_eq!(identity.node_id.as_str(), "stable-node");
            assert_eq!(identity.principal, principal);
            assert_eq!(identity.protocol_version, NODE_PROTOCOL_VERSION);
        }
        host.close().await.unwrap();
        assert_eq!(host.kernel().diagnostics().calls, 0);
        assert_eq!(host.kernel().diagnostics().resources, 0);
    });
}

#[test]
fn malformed_node_identifiers_are_refused_at_constructor_and_decode_boundaries() {
    for invalid in [String::new(), "two nodes".into(), "x".repeat(129)] {
        assert!(NodeId::new(invalid.clone()).is_err());
        assert!(serde_json::from_value::<NodeId>(serde_json::json!(invalid)).is_err());
    }
    let identity = NodeIdentity {
        node_id: NodeId::new("node-a").unwrap(),
        principal: "alice".into(),
        protocol_version: NODE_PROTOCOL_VERSION,
    };
    assert_eq!(
        serde_json::to_value(identity).unwrap(),
        serde_json::json!({"nodeId":"node-a","principal":"alice","protocolVersion":1})
    );
}
