use swarmie_plugin_contracts::*;
use tembra::{ContractToken, contracts::Contract};

fn roundtrip(token: impl ContractToken) {
    let descriptor = token.descriptor().expect("portable contract descriptor");
    let serialized = tembra::serde_json::to_string(descriptor.descriptor()).unwrap();
    let reparsed = Contract::from_json(&serialized).expect("canonical descriptor roundtrip");
    assert_eq!(descriptor.fingerprint(), reparsed.fingerprint());
}

#[test]
fn all_authored_contracts_construct_and_reparse() {
    roundtrip(Counter);
    roundtrip(PluginConfiguration);
    roundtrip(RemoteCounter);
    roundtrip(DynamicConsumer);
    roundtrip(PluginSourceSchema);
    roundtrip(PluginAdministration);
    roundtrip(RemotePluginAdministration);
    roundtrip(UiContributions);
    roundtrip(BrowserAccess);
    roundtrip(RemoteBrowserAccess);
}

#[test]
fn administrative_proof_debug_never_exposes_capability_material() {
    let proof = AdminProof::new("private-supervisor-material".into());
    assert_eq!(format!("{proof:?}"), "AdminProof([redacted])");
    assert_eq!(proof.as_str(), "private-supervisor-material");
    assert_eq!(
        tembra::serde_json::to_value(&proof).unwrap(),
        "private-supervisor-material"
    );
}

#[test]
fn unknown_fields_and_source_operation_variants_are_refused() {
    assert!(
        tembra::serde_json::from_value::<SearchRequest>(
            tembra::serde_json::json!({"query":"counter", "unexpected":true}),
        )
        .is_err()
    );
    assert!(
        tembra::serde_json::from_value::<SourceRegistryMetadata>(
            tembra::serde_json::json!({"provider_id":"local", "operation":"execute"}),
        )
        .is_err()
    );
}

#[test]
fn administrative_descriptors_expose_only_proof_bearing_methods() {
    use tembra::contracts::{Member, ValueSchema};

    for contract in [
        PluginAdministration.descriptor().unwrap(),
        RemotePluginAdministration.descriptor().unwrap(),
    ] {
        for variant in contract.descriptor().variants.values() {
            assert!(!variant.is_empty());
            for member in variant.values() {
                let Member::Method {
                    args: Some(ValueSchema::Portable(schema)),
                    ..
                } = member
                else {
                    panic!("administrative acquisition must not hydrate unauthenticated state");
                };
                assert!(
                    schema["required"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|field| field == "proof")
                );
                assert_eq!(schema["properties"]["proof"]["type"], "string");
            }
        }
    }
}

#[test]
fn browser_package_reads_have_bounded_wire_schemas_and_metadata_excludes_file_bytes() {
    use tembra::contracts::{Member, ValueSchema};

    for contract in [
        PluginAdministration.descriptor().unwrap(),
        RemotePluginAdministration.descriptor().unwrap(),
    ] {
        for members in contract.descriptor().variants.values() {
            let Member::Method {
                args: Some(ValueSchema::Portable(request)),
                result: Some(ValueSchema::Portable(response)),
            } = members.get("read_package_file").unwrap()
            else {
                panic!("package reads must use checked portable schemas")
            };
            assert_eq!(request["properties"]["length"]["minimum"], 1);
            assert_eq!(request["properties"]["length"]["maximum"], 32768);
            assert_eq!(response["properties"]["bytes"]["maxItems"], 32768);
            assert_eq!(response["properties"]["total_length"]["maximum"], 8388608);
            let Member::Method {
                result: Some(ValueSchema::Portable(metadata)),
                ..
            } = members.get("active_package").unwrap()
            else {
                panic!("package metadata must be portable")
            };
            let fields = metadata["properties"].as_object().unwrap();
            assert!(
                fields.contains_key("manifest")
                    && fields.contains_key("provenance")
                    && fields.contains_key("activation_id")
            );
            assert!(!fields.contains_key("files") && !fields.contains_key("package"));
        }
    }
}

#[test]
fn browser_access_descriptors_have_only_session_scoped_grants_and_no_mutations() {
    use tembra::contracts::{Member, ValueSchema};

    for contract in [
        BrowserAccess.descriptor().unwrap(),
        RemoteBrowserAccess.descriptor().unwrap(),
    ] {
        for members in contract.descriptor().variants.values() {
            assert_eq!(
                members
                    .keys()
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>(),
                std::collections::BTreeSet::from([
                    "inventory".into(),
                    "active_package".into(),
                    "read_package_file".into(),
                    "report_browser".into()
                ])
            );
            for member in members.values() {
                let Member::Method {
                    args: Some(ValueSchema::Portable(args)),
                    ..
                } = member
                else {
                    panic!("browser access must not hydrate state")
                };
                let required = args["required"].as_array().unwrap();
                assert!(required.iter().any(|field| field == "grant"));
                assert!(required.iter().any(|field| field == "session_id"));
                assert!(args["properties"].get("proof").is_none());
            }
        }
    }
    let grant = BrowserGrant::new("private-browser-material".into());
    assert_eq!(format!("{grant:?}"), "BrowserGrant([redacted])");
}
