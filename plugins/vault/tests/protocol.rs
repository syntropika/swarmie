use hpke::{
    Deserializable, Kem, OpModeR, aead::ChaCha20Poly1305, kdf::HkdfSha256, kem::X25519HkdfSha256,
};
use serde_json::Value;
use swarmie_vault::{RecordCommand, digest, verify_signature};

#[test]
fn public_command_encoding_matches_an_independent_signature_fixture() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/vault-contract/record-command-v1.json"
    ))
    .unwrap();
    let command: RecordCommand = serde_json::from_value(fixture["command"].clone()).unwrap();
    assert_eq!(
        command.signing_bytes().unwrap(),
        fixture["signing_utf8"].as_str().unwrap().as_bytes()
    );
    let public: Vec<u8> = serde_json::from_value(fixture["public_key"].clone()).unwrap();
    verify_signature(
        &public,
        &command.signing_bytes().unwrap(),
        &command.signature,
    )
    .unwrap();
    assert_eq!(
        digest(&command.signing_bytes().unwrap()),
        serde_json::from_value::<Vec<u8>>(fixture["digest"].clone()).unwrap()
    );
}

#[test]
fn selected_hpke_suite_decrypts_the_published_base_mode_known_answer() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/vault-contract/hpke-base-v1.json"
    ))
    .unwrap();
    let bytes = |v: &Value| hex::decode(v.as_str().unwrap()).unwrap();
    type K = X25519HkdfSha256;
    let private = <K as Kem>::PrivateKey::from_bytes(&bytes(&fixture["skRm"])).unwrap();
    let enc = <K as Kem>::EncappedKey::from_bytes(&bytes(&fixture["enc"])).unwrap();
    let mut receiver = hpke::setup_receiver::<ChaCha20Poly1305, HkdfSha256, K>(
        &OpModeR::Base,
        &private,
        &enc,
        &bytes(&fixture["info"]),
    )
    .unwrap();
    let v = &fixture["encryption"];
    assert_eq!(
        receiver.open(&bytes(&v["ct"]), &bytes(&v["aad"])).unwrap(),
        bytes(&v["pt"])
    );
}
