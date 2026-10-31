//! Fixed public wire fixture shared by independent client and hosted implementations.
use swarmie_cloud::device_linking::*;

#[test]
fn fixed_domain_signatures_and_canonical_bytes_match_the_public_contract() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/link.json")).unwrap();
    let controller: DeviceRegistration =
        serde_json::from_value(fixture["controller"].clone()).unwrap();
    let target: DeviceRegistration = serde_json::from_value(fixture["target"].clone()).unwrap();
    let link: SignedLink = serde_json::from_value(fixture["link"].clone()).unwrap();
    link.verify(&controller, &target).unwrap();
    assert_eq!(
        link.intent.signing_bytes().unwrap(),
        fixture["signing_bytes"].as_str().unwrap().as_bytes()
    );
    let expected: [u8; 32] = serde_json::from_value(fixture["intent_digest"].clone()).unwrap();
    assert_eq!(digest(&link.intent).unwrap(), expected);
    let mut changed = link.clone();
    changed.intent.vault.as_mut().unwrap().recipient_key[0] ^= 1;
    assert!(changed.verify(&controller, &target).is_err());
    changed = link.clone();
    changed.intent.node = "another-node".into();
    assert!(changed.verify(&controller, &target).is_err());
}
