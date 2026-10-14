use storage::{NodeConnection, NodeTransport, SqliteSessionStore};
use swarmie::LocalStoreIdentity;
fn route(name: &str, identity: LocalStoreIdentity) -> NodeConnection {
    NodeConnection {
        name: name.into(),
        identity,
        transport: NodeTransport::Http {
            url: "http://127.0.0.1:7480/rpc".into(),
        },
        credential_ref: None,
        revision: 0,
        created_at_ms: 0,
        last_verified_at_ms: 10,
    }
}
#[tokio::test]
async fn local_registration_preserves_selected_remote_and_deduplicates_routes_across_restart() {
    let root = tempfile::tempdir().unwrap();
    let store = SqliteSessionStore::new(root.path()).await.unwrap();
    store
        .register_local_node(store.identity().clone(), 1)
        .await
        .unwrap();
    let remote = LocalStoreIdentity::default();
    store
        .register_node_connection(route("first", remote.clone()), 0)
        .await
        .unwrap();
    store
        .register_node_connection(route("second", remote.clone()), 0)
        .await
        .unwrap();
    store
        .select_node(remote.environment_id.clone())
        .await
        .unwrap();
    store
        .register_local_node(store.identity().clone(), 20)
        .await
        .unwrap();
    let reopened = SqliteSessionStore::new(root.path()).await.unwrap();
    let catalog = reopened.node_catalog().await.unwrap();
    assert_eq!(catalog.nodes.len(), 2);
    assert_eq!(
        catalog.selected_node_id,
        Some(remote.environment_id.clone())
    );
    assert!(catalog.nodes[0].local);
    let remote = catalog.nodes.iter().find(|n| n.identity == remote).unwrap();
    assert_eq!(remote.connections.len(), 2);
    assert!(
        serde_json::to_value(&catalog)
            .unwrap()
            .to_string()
            .find("incarnationId")
            .is_none()
    );
    assert!(
        store
            .register_local_node(LocalStoreIdentity::default(), 21)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn independent_store_handles_preserve_concurrent_enrollment_and_conflicting_updates() {
    let root = tempfile::tempdir().unwrap();
    let first = SqliteSessionStore::new(root.path()).await.unwrap();
    let second = SqliteSessionStore::new(root.path()).await.unwrap();
    let identity = LocalStoreIdentity::default();
    let (a, b) = tokio::join!(
        first.register_node_connection(route("a", identity.clone()), 0),
        second.register_node_connection(route("b", identity.clone()), 0)
    );
    let a = a.unwrap();
    b.unwrap();
    assert_eq!(first.node_catalog().await.unwrap().nodes.len(), 1);
    let mut updated = a.clone();
    updated.transport = NodeTransport::Http {
        url: "http://127.0.0.1:9000/rpc".into(),
    };
    let (one, two) = tokio::join!(
        first.register_node_connection(updated.clone(), a.revision),
        second.register_node_connection(updated, a.revision)
    );
    assert_ne!(one.is_ok(), two.is_ok());
    assert_eq!(first.node_connections().await.unwrap().len(), 2);
}
#[tokio::test]
async fn removal_retains_node_identity_and_stale_verification_cannot_overwrite_reenrollment() {
    let root = tempfile::tempdir().unwrap();
    let store = SqliteSessionStore::new(root.path()).await.unwrap();
    let identity = LocalStoreIdentity::default();
    let original = store
        .register_node_connection(route("node", identity.clone()), 0)
        .await
        .unwrap();
    store
        .select_node(identity.environment_id.clone())
        .await
        .unwrap();
    store
        .remove_node_connection("node".into(), original.revision)
        .await
        .unwrap();
    let catalog = store.node_catalog().await.unwrap();
    assert_eq!(catalog.nodes.len(), 1);
    assert!(!catalog.nodes[0].registered);
    assert_eq!(
        catalog.selected_node_id,
        Some(identity.environment_id.clone())
    );
    assert!(
        store
            .select_node(identity.environment_id.clone())
            .await
            .is_err()
    );
    let new = store
        .register_node_connection(route("node", identity), 0)
        .await
        .unwrap();
    assert!(new.revision > original.revision);
    assert!(
        store
            .register_node_connection(original.clone(), original.revision)
            .await
            .is_err()
    );
    assert!(
        store
            .observe_node_connection("node".into(), original.revision, 30)
            .await
            .is_err()
    );
    assert!(
        store
            .remove_node_connection("node".into(), original.revision)
            .await
            .is_err()
    );
    assert_eq!(
        store.node_connections().await.unwrap()[0].revision,
        new.revision
    );
}
#[tokio::test]
async fn foreign_identity_cannot_replace_a_route_or_the_retained_environment_record() {
    let root = tempfile::tempdir().unwrap();
    let store = SqliteSessionStore::new(root.path()).await.unwrap();
    let identity = LocalStoreIdentity::default();
    let original = store
        .register_node_connection(route("node", identity.clone()), 0)
        .await
        .unwrap();
    assert!(
        store
            .register_node_connection(
                route("node", LocalStoreIdentity::default()),
                original.revision
            )
            .await
            .is_err()
    );
    let changed = LocalStoreIdentity {
        store_id: uuid::Uuid::new_v4().to_string(),
        ..identity.clone()
    };
    assert!(
        store
            .register_node_connection(route("other alias", changed), 0)
            .await
            .is_err()
    );
    let catalog = store.node_catalog().await.unwrap();
    assert_eq!(catalog.nodes.len(), 1);
    assert_eq!(catalog.nodes[0].identity, identity);
    assert_eq!(catalog.nodes[0].connections.len(), 1);
}
