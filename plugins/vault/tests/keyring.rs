#![cfg(all(feature = "native-keyring", feature = "cloud-client"))]

use swarmie_cloud::account::{KeyringSessions, SessionStorage, StoredSession};
use swarmie_vault::{keyring::*, *};
use uuid::Uuid;

#[test]
#[ignore = "requires a qualified unlocked native credential service"]
fn device_bundle_roundtrip_is_atomic_and_account_logout_preserves_enrollment() {
    let identity = VaultIdentity {
        account: Uuid::new_v4(),
        vault: Uuid::new_v4(),
        epoch: Uuid::new_v4(),
    };
    let device = Uuid::new_v4();
    let keys = DeviceKeys::generate();
    let public = keys.signing_public();
    let bundle = TrustBundle {
        keys,
        pins: TrustPins {
            identity,
            root: public.clone(),
            chain: vec![],
            approver: public.clone(),
            envelope_digest: vec![1; 32],
            statement_digest: vec![3; 32],
            commitment: vec![2; 32],
        },
    };
    let installation = Uuid::new_v4();
    let session=StoredSession::decode(&serde_json::to_vec(&serde_json::json!({"version":1,"account":identity.account,"installation":installation,"family":Uuid::new_v4(),"endpoints":{"api":"https://cloud.example/","authentication":"https://cloud.example/"},"access_token":"synthetic-access-fixture","refresh_token":"synthetic-refresh-fixture","expires_at":1,"renewal_unknown":false})).unwrap()).unwrap();
    OsKeyring.save_bundle(identity, device, &bundle).unwrap();
    OsKeyring
        .save_pending_device(identity, device, &bundle.keys)
        .unwrap();
    assert_eq!(
        OsKeyring
            .load_pending_device(identity, device)
            .unwrap()
            .signing_public(),
        public
    );
    OsKeyring.forget_pending_device(identity, device).unwrap();
    assert!(OsKeyring.load_pending_device(identity, device).is_err());
    KeyringSessions.save(&session).unwrap();
    KeyringSessions
        .load(identity.account, installation)
        .unwrap()
        .write_private(|bytes| {
            assert!(
                std::str::from_utf8(bytes)
                    .unwrap()
                    .contains("synthetic-access-fixture")
            );
            Ok(())
        })
        .unwrap();
    KeyringSessions
        .forget(identity.account, installation)
        .unwrap();
    assert!(
        KeyringSessions
            .load(identity.account, installation)
            .is_err()
    );
    let restored = OsKeyring.load_bundle(identity, device).unwrap();
    assert_eq!(restored.pins, bundle.pins);
    assert_eq!(restored.keys.signing_public(), public);
    assert_eq!(
        restored.keys.encryption_public().unwrap(),
        bundle.keys.encryption_public().unwrap()
    );
    OsKeyring.forget_bundle(identity, device).unwrap();
    assert!(OsKeyring.load_bundle(identity, device).is_err());
}
