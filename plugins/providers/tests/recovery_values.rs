use std::collections::BTreeMap;
use swarmie_providers::{
    AccountRecord, ProtocolBinding,
    accounts::recovery::{
        CredentialReference, CredentialSlot, LegacyObservation, ProviderOwner, ProviderState,
        ProvisioningPlan, RecoveryAccount, RecoveryKind, RecoveryLimits, RecoveryPhase,
        RecoveryRecord, RecoveryReferences, ReplacementKind, StateReplacement,
    },
    settings::{Account, ModelDefaults, ModelInfo},
};
use uuid::Uuid;

const ID: &str = "account/a:b/é";

fn owner() -> ProviderOwner {
    ProviderOwner::new("principal/é".into(), Uuid::from_u128(1)).unwrap()
}

fn account(label: &str) -> AccountRecord {
    AccountRecord::new(
        Account {
            id: ID.into(), provider_id: "fixture".into(), provider_label: "Fixture".into(),
            label: label.into(), models: vec![ModelInfo { id: "model".into(), label: "Model".into(), efforts: vec![] }],
            available: true, defaults: ModelDefaults::default(),
            authorization_method: Some("oauth-browser".into()), models_truncated: false,
        },
        serde_json::from_value(serde_json::json!({"provider":"fixture","endpoint":"https://provider.example/","extension":{"keep":[1,2,3]}})).unwrap(),
    ).unwrap()
}

fn reference(slot: CredentialSlot, generation: u128) -> CredentialReference {
    CredentialReference::new(owner(), slot, Uuid::from_u128(generation)).unwrap()
}

fn previous() -> CredentialReference {
    reference(
        CredentialSlot::Account {
            account_id: ID.into(),
        },
        31,
    )
}

fn candidate() -> CredentialReference {
    reference(
        CredentialSlot::Account {
            account_id: ID.into(),
        },
        33,
    )
}

fn record() -> RecoveryRecord {
    RecoveryRecord::new(
        Uuid::from_u128(20),
        Uuid::from_u128(10),
        RecoveryKind::Authorize,
        Some(ProtocolBinding::new([4; 32])),
        RecoveryAccount::Captured {
            before: Some(account("Before")),
            candidate: Box::new(account("Candidate")),
        },
        RecoveryReferences::new(
            Some(previous()),
            Some(reference(
                CredentialSlot::Operation {
                    operation: Uuid::from_u128(20),
                },
                32,
            )),
            Some(candidate()),
        ),
    )
    .unwrap()
}

fn state(
    workflow: Option<RecoveryRecord>,
    revision: u128,
    committed: bool,
) -> tembra::Result<ProviderState> {
    ProviderState::new(
        owner(),
        Uuid::from_u128(revision),
        vec![account(if committed { "Candidate" } else { "Before" })],
        BTreeMap::from([(ID.into(), if committed { candidate() } else { previous() })]),
        workflow,
        RecoveryLimits::default(),
    )
}

#[test]
fn protocol_binding_is_exact_fixed_width_private_checkpoint_data() {
    let binding = ProtocolBinding::new([255; 32]);
    let encoded = serde_json::to_value(binding).unwrap();
    assert!(serde_json::from_value::<ProtocolBinding>(encoded.clone()).unwrap() == binding);
    assert!(ProtocolBinding::new([254; 32]) != binding);
    for invalid in [
        serde_json::json!([1, 2]),
        serde_json::json!("00"),
        serde_json::json!({"bytes":[]}),
    ] {
        assert!(serde_json::from_value::<ProtocolBinding>(invalid).is_err());
    }
    let mut invalid = encoded;
    invalid[0] = serde_json::json!(256);
    assert!(serde_json::from_value::<ProtocolBinding>(invalid).is_err());
}

#[test]
fn reference_decoding_cannot_bypass_checked_identity_or_slot_rules() {
    assert!(ProviderOwner::new(String::new(), Uuid::from_u128(1)).is_err());
    assert!(ProviderOwner::new("principal".into(), Uuid::nil()).is_err());
    assert!(
        CredentialReference::new(
            owner(),
            CredentialSlot::Account {
                account_id: String::new()
            },
            Uuid::from_u128(1)
        )
        .is_err()
    );
    assert!(
        CredentialReference::new(
            owner(),
            CredentialSlot::Operation {
                operation: Uuid::nil()
            },
            Uuid::from_u128(1)
        )
        .is_err()
    );
    let valid = serde_json::to_value(previous()).unwrap();
    for field in 0..5 {
        let mut invalid = valid.clone();
        match field {
            0 => invalid["owner"]["principal"] = serde_json::json!(""),
            1 => invalid["owner"]["profile"] = serde_json::json!(Uuid::nil()),
            2 => invalid["generation"] = serde_json::json!(Uuid::nil()),
            3 => invalid["slot"]["account_id"] = serde_json::json!(""),
            _ => invalid["owner"]["authority"] = serde_json::json!(true),
        }
        assert!(serde_json::from_value::<CredentialReference>(invalid).is_err());
    }
}

#[test]
fn identifiers_preserve_utf8_and_tagged_slots_without_normalization() {
    for id in ["a/b:c", "a/b//c", "é", "e\u{301}", " a ", "zero\0byte"] {
        let value = reference(
            CredentialSlot::Account {
                account_id: id.into(),
            },
            2,
        );
        let decoded: CredentialReference =
            serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(decoded == value);
        assert!(
            matches!(decoded.slot(), CredentialSlot::Account { account_id } if account_id.as_bytes() == id.as_bytes())
        );
    }
    assert!(
        reference(
            CredentialSlot::Account {
                account_id: "é".into()
            },
            2
        ) != reference(
            CredentialSlot::Account {
                account_id: "e\u{301}".into()
            },
            2
        )
    );
    assert!(
        reference(
            CredentialSlot::Account {
                account_id: Uuid::from_u128(3).to_string()
            },
            2
        ) != reference(
            CredentialSlot::Operation {
                operation: Uuid::from_u128(3)
            },
            2
        )
    );
}

#[test]
fn state_rejects_foreign_unbound_or_ambiguous_account_references() {
    let foreign = CredentialReference::new(
        ProviderOwner::new("other".into(), Uuid::from_u128(1)).unwrap(),
        CredentialSlot::Account {
            account_id: ID.into(),
        },
        Uuid::from_u128(31),
    )
    .unwrap();
    for (key, value) in [
        (ID.to_string(), foreign),
        ("missing".into(), previous()),
        (
            ID.to_string(),
            reference(
                CredentialSlot::Operation {
                    operation: Uuid::from_u128(20),
                },
                31,
            ),
        ),
    ] {
        assert!(
            ProviderState::new(
                owner(),
                Uuid::from_u128(100),
                vec![account("Before")],
                BTreeMap::from([(key, value)]),
                None,
                RecoveryLimits::default()
            )
            .is_err()
        );
    }
    assert!(
        ProviderState::new(
            owner(),
            Uuid::from_u128(100),
            vec![account("Before"), account("Duplicate")],
            BTreeMap::new(),
            None,
            RecoveryLimits::default()
        )
        .is_err()
    );
    assert!(
        ProviderState::new(
            owner(),
            Uuid::nil(),
            vec![],
            BTreeMap::new(),
            None,
            RecoveryLimits::default()
        )
        .is_err()
    );
}

#[test]
fn state_decode_preserves_private_fields_and_refuses_corruption_or_wrong_owner() {
    let original = state(Some(record()), 100, false).unwrap();
    let bytes = original.encode(RecoveryLimits::default()).unwrap();
    let decoded = ProviderState::decode(&bytes, &owner(), RecoveryLimits::default()).unwrap();
    assert_eq!(decoded.encode(RecoveryLimits::default()).unwrap(), bytes);
    assert_eq!(
        decoded.accounts()[0].private_configuration()["extension"]["keep"],
        serde_json::json!([1, 2, 3])
    );
    let foreign = ProviderOwner::new("other".into(), owner().profile()).unwrap();
    assert!(ProviderState::decode(&bytes, &foreign, RecoveryLimits::default()).is_err());
    assert!(ProviderState::decode(b"corrupt", &owner(), RecoveryLimits::default()).is_err());
    let mut unknown: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    unknown["fresh"] = serde_json::json!(true);
    assert!(
        ProviderState::decode(
            &serde_json::to_vec(&unknown).unwrap(),
            &owner(),
            RecoveryLimits::default()
        )
        .is_err()
    );
}

#[test]
fn duplicate_credential_keys_are_errors_instead_of_last_value_wins() {
    let bytes = state(None, 100, false)
        .unwrap()
        .encode(RecoveryLimits::default())
        .unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let key = serde_json::to_string(ID).unwrap();
    let value = serde_json::to_string(&previous()).unwrap();
    let original = format!("\"credentials\":{{{key}:{value}}}");
    let duplicate = format!("\"credentials\":{{{key}:{value},{key}:{value}}}");
    assert!(text.contains(&original));
    assert!(
        ProviderState::decode(
            text.replace(&original, &duplicate).as_bytes(),
            &owner(),
            RecoveryLimits::default()
        )
        .is_err()
    );
    let legacy = serde_json::to_string(&vec![account("Legacy")]).unwrap();
    let ambiguous = legacy.replace("\"keep\":[1,2,3]", "\"keep\":[1,2,3],\"keep\":[4]");
    assert_ne!(legacy, ambiguous);
    assert!(
        LegacyObservation::from_bytes(owner(), ambiguous.into_bytes(), RecoveryLimits::default())
            .is_err()
    );
}

#[test]
fn missing_nullable_evidence_is_not_an_explicit_clean_state() {
    let clean = state(None, 100, false).unwrap();
    let mut data = serde_json::to_value(clean).unwrap();
    assert!(data["workflow"].is_null());
    data.as_object_mut().unwrap().remove("workflow");
    assert!(
        ProviderState::decode(
            &serde_json::to_vec(&data).unwrap(),
            &owner(),
            RecoveryLimits::default()
        )
        .is_err()
    );

    let mut active = serde_json::to_value(state(Some(record()), 100, false).unwrap()).unwrap();
    assert!(active["workflow"]["replacement"].is_null());
    active["workflow"]
        .as_object_mut()
        .unwrap()
        .remove("replacement");
    assert!(
        ProviderState::decode(
            &serde_json::to_vec(&active).unwrap(),
            &owner(),
            RecoveryLimits::default()
        )
        .is_err()
    );
}

#[test]
fn records_bind_original_operation_account_protocol_and_owner() {
    let base = serde_json::to_value(state(Some(record()), 100, false).unwrap()).unwrap();
    for field in 0..6 {
        let mut invalid = base.clone();
        match field {
            0 => invalid["workflow"]["operation"] = serde_json::json!(Uuid::nil()),
            1 => invalid["workflow"]["protocol_binding"] = serde_json::Value::Null,
            2 => {
                invalid["workflow"]["references"]["issued"]["slot"]["operation"] =
                    serde_json::json!(Uuid::from_u128(99))
            }
            3 => {
                invalid["workflow"]["references"]["candidate"]["slot"]["account_id"] =
                    serde_json::json!("different")
            }
            4 => {
                invalid["workflow"]["references"]["previous"]["owner"]["principal"] =
                    serde_json::json!("other")
            }
            _ => {
                invalid["workflow"]["account"]["before"]["account"]["id"] =
                    serde_json::json!("different")
            }
        }
        assert!(
            ProviderState::decode(
                &serde_json::to_vec(&invalid).unwrap(),
                &owner(),
                RecoveryLimits::default()
            )
            .is_err()
        );
    }
}

#[test]
fn replacement_phase_requires_exact_predecessor_target_and_account_image() {
    let plan = StateReplacement::new(
        ReplacementKind::Publish,
        Uuid::from_u128(100),
        Uuid::from_u128(101),
    )
    .unwrap();
    assert!(
        StateReplacement::new(ReplacementKind::Publish, Uuid::nil(), Uuid::from_u128(101)).is_err()
    );
    assert!(
        StateReplacement::new(
            ReplacementKind::Publish,
            Uuid::from_u128(100),
            Uuid::from_u128(100)
        )
        .is_err()
    );
    assert!(
        record()
            .with_phase(RecoveryPhase::AccountPublication, None)
            .is_err()
    );
    let publishing = record()
        .with_phase(RecoveryPhase::AccountPublication, Some(plan))
        .unwrap();
    assert!(state(Some(publishing.clone()), 100, false).is_ok());
    assert!(state(Some(publishing), 999, false).is_err());
    let committed = record()
        .with_phase(RecoveryPhase::Committed, Some(plan))
        .unwrap();
    assert!(state(Some(committed.clone()), 101, true).is_ok());
    assert!(state(Some(committed.clone()), 101, false).is_err());
    assert!(state(Some(committed), 100, true).is_err());
}

#[test]
fn clearing_keeps_its_target_and_active_data_without_claiming_native_readiness() {
    let clear = StateReplacement::new(
        ReplacementKind::Clear,
        Uuid::from_u128(110),
        Uuid::from_u128(111),
    )
    .unwrap();
    let retiring = record()
        .with_phase(RecoveryPhase::Retirement, Some(clear))
        .unwrap();
    let bytes = state(Some(retiring), 110, true)
        .unwrap()
        .encode(RecoveryLimits::default())
        .unwrap();
    let decoded = ProviderState::decode(&bytes, &owner(), RecoveryLimits::default()).unwrap();
    let retained = decoded.workflow().unwrap();
    assert!(retained.phase() == RecoveryPhase::Retirement);
    assert!(retained.replacement().unwrap() == clear);
    assert_eq!(retained.operation(), Uuid::from_u128(20));
    assert!(
        state(
            Some(
                record()
                    .with_phase(RecoveryPhase::Retirement, Some(clear))
                    .unwrap()
            ),
            111,
            true
        )
        .is_err()
    );
}

#[test]
fn configured_bounds_apply_to_decode_encode_references_and_active_records() {
    let bytes = state(Some(record()), 100, false)
        .unwrap()
        .encode(RecoveryLimits::default())
        .unwrap();
    let small = RecoveryLimits::new(1, 2, bytes.len() - 1).unwrap();
    assert!(ProviderState::decode(&bytes, &owner(), small).is_err());
    let ref_length = serde_json::to_vec(&previous()).unwrap().len();
    assert!(previous().check_size(ref_length).is_ok());
    assert!(previous().check_size(ref_length - 1).is_err());
    assert!(
        state(None, 100, false)
            .unwrap()
            .encode(RecoveryLimits::new(ref_length - 1, 1000, 10000).unwrap())
            .is_err()
    );
    let workflow_length = serde_json::to_vec(&record()).unwrap().len();
    let maximum_reference = [
        record().references().previous(),
        record().references().issued(),
        record().references().candidate(),
    ]
    .into_iter()
    .flatten()
    .map(|value| serde_json::to_vec(value).unwrap().len())
    .max()
    .unwrap();
    let exact = RecoveryLimits::new(maximum_reference, workflow_length, bytes.len()).unwrap();
    let complete = state(Some(record()), 100, false).unwrap();
    assert_eq!(complete.encode(exact).unwrap(), bytes);
    assert!(
        complete
            .encode(
                RecoveryLimits::new(maximum_reference, workflow_length - 1, bytes.len()).unwrap()
            )
            .is_err()
    );
    assert!(
        complete
            .encode(
                RecoveryLimits::new(maximum_reference, workflow_length, bytes.len() - 1).unwrap()
            )
            .is_err()
    );
    assert!(
        state(Some(record()), 100, false)
            .unwrap()
            .encode(RecoveryLimits::new(ref_length, ref_length, 10000).unwrap())
            .is_err()
    );
    assert!(RecoveryLimits::new(0, 1, 2).is_err());
    assert!(RecoveryLimits::new(2, 1, 3).is_err());
    assert!(RecoveryLimits::new(1, 3, 2).is_err());
}

#[test]
fn legacy_observation_preserves_exact_bytes_and_cannot_become_fresh_evidence() {
    let bytes = serde_json::to_vec(&vec![account("Legacy")]).unwrap();
    let observed =
        LegacyObservation::from_bytes(owner(), bytes.clone(), RecoveryLimits::default()).unwrap();
    assert_eq!(observed.bytes(), bytes);
    assert_eq!(observed.records()[0].account.id, ID);
    assert!(observed.predecessor().matches_bytes(&owner(), &bytes));
    let mut changed = bytes.clone();
    changed.push(b' ');
    assert!(!observed.predecessor().matches_bytes(&owner(), &changed));
    assert!(!observed.predecessor().matches_bytes(
        &ProviderOwner::new("other".into(), owner().profile()).unwrap(),
        &bytes
    ));
    assert!(
        LegacyObservation::from_bytes(owner(), b"[]".to_vec(), RecoveryLimits::default()).is_ok()
    );
    assert!(
        LegacyObservation::from_bytes(owner(), b"corrupt".to_vec(), RecoveryLimits::default())
            .is_err()
    );
    assert!(
        LegacyObservation::from_bytes(
            owner(),
            serde_json::to_vec(&vec![account("A"), account("B")]).unwrap(),
            RecoveryLimits::default()
        )
        .is_err()
    );
    assert!(ProvisioningPlan::new(owner(), Uuid::nil()).is_err());
}

#[test]
fn issued_ready_requires_authorization_escrow_binding_and_the_prepublication_image() {
    assert!(
        record()
            .with_phase(RecoveryPhase::IssuedReady, None)
            .is_err()
    );
    let limits = RecoveryLimits::default();
    let operation = Uuid::from_u128(20);
    let record = RecoveryRecord::new(
        operation,
        Uuid::from_u128(10),
        RecoveryKind::Authorize,
        Some(ProtocolBinding::new([4; 32])),
        RecoveryAccount::Unassociated {
            provider: "fixture".into(),
            returning: Some(account("Before")),
        },
        RecoveryReferences::new(
            Some(previous()),
            Some(reference(CredentialSlot::Operation { operation }, 32)),
            None,
        ),
    )
    .unwrap()
    .with_phase(RecoveryPhase::IssuedReady, None)
    .unwrap();
    let state = ProviderState::new(
        owner(),
        Uuid::from_u128(11),
        vec![account("Before")],
        BTreeMap::from([(ID.into(), previous())]),
        Some(record.clone()),
        limits,
    )
    .unwrap();
    let bytes = state.encode(limits).unwrap();
    assert!(ProviderState::decode(&bytes, &owner(), limits).is_ok());
    assert!(
        ProviderState::new(
            owner(),
            Uuid::from_u128(11),
            vec![account("Other")],
            BTreeMap::from([(ID.into(), previous())]),
            Some(record),
            limits
        )
        .is_err()
    );
    for (field, value) in [
        ("kind", serde_json::json!("configure")),
        ("protocol_binding", serde_json::Value::Null),
    ] {
        let mut raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        raw["workflow"][field] = value;
        assert!(
            ProviderState::decode(&serde_json::to_vec(&raw).unwrap(), &owner(), limits).is_err()
        );
    }
    let mut raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    raw["workflow"]["references"]["issued"] = serde_json::Value::Null;
    assert!(ProviderState::decode(&serde_json::to_vec(&raw).unwrap(), &owner(), limits).is_err());
}
