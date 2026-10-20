use swarmie_plugin_contracts::*;
use tembra::{
    ContractToken,
    contracts::{Contract, ContractKind, Member, ValueSchema},
};

fn roundtrip(token: impl ContractToken) {
    let descriptor = token.descriptor().expect("portable contract descriptor");
    let serialized = tembra::serde_json::to_string(descriptor.descriptor()).unwrap();
    let reparsed = Contract::from_json(&serialized).expect("canonical descriptor roundtrip");
    assert_eq!(descriptor.fingerprint(), reparsed.fingerprint());
}

#[test]
fn all_authored_contracts_construct_and_reparse() {
    roundtrip(PluginConfiguration);
    roundtrip(PluginConfigurationControlSchema);
    roundtrip(PluginSourceSchema);
    roundtrip(PluginAdministration);
    roundtrip(UiContributions);
    roundtrip(BrowserAccess);
    roundtrip(RemoteBrowserAccess);
}

#[test]
fn unknown_fields_and_source_operation_variants_are_refused() {
    assert!(
        tembra::serde_json::from_value::<SearchRequest>(
            tembra::serde_json::json!({"query":"counter","unexpected":true})
        )
        .is_err()
    );
    assert!(
        tembra::serde_json::from_value::<SourceRegistryMetadata>(
            tembra::serde_json::json!({"provider_id":"local","operation":"execute"})
        )
        .is_err()
    );
}

#[test]
fn administration_is_local_and_inventory_has_no_request_payload() {
    let contract = PluginAdministration.descriptor().unwrap();
    assert_eq!(contract.kind(), ContractKind::Local);
    for members in contract.descriptor().variants.values() {
        assert!(matches!(
            members.get("inventory"),
            Some(Member::Method { args: None, .. })
        ));
        assert!(members.contains_key("install"));
        assert!(members.contains_key("report_browser"));
        assert!(
            members
                .values()
                .all(|member| matches!(member, Member::Method { .. }))
        );
    }
}

#[test]
fn remote_browser_delivery_exposes_only_approved_package_reads() {
    let contract = RemoteBrowserAccess.descriptor().unwrap();
    assert_eq!(contract.kind(), ContractKind::Remote);
    for members in contract.descriptor().variants.values() {
        assert_eq!(
            members
                .keys()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from(["inventory", "active_package", "read_package_file"])
        );
        let Member::Method {
            args: Some(ValueSchema::Portable(request)),
            result: Some(ValueSchema::Portable(response)),
        } = members.get("read_package_file").unwrap()
        else {
            panic!("bounded package schema")
        };
        assert_eq!(request["properties"]["length"]["minimum"], 1);
        assert_eq!(request["properties"]["length"]["maximum"], 32768);
        assert_eq!(response["properties"]["bytes"]["maxItems"], 32768);
        assert_eq!(response["properties"]["total_length"]["maximum"], 8388608);
        let fields = request["properties"].as_object().unwrap();
        assert_eq!(
            fields
                .keys()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from([
                "plugin_id",
                "activation_id",
                "path",
                "offset",
                "length"
            ])
        );
        let Member::Method {
            result: Some(ValueSchema::Portable(metadata)),
            ..
        } = members.get("active_package").unwrap()
        else {
            panic!("portable approved metadata")
        };
        let fields = metadata["properties"].as_object().unwrap();
        assert!(fields.contains_key("manifest") && fields.contains_key("activation_id"));
        assert!(!fields.contains_key("files") && !fields.contains_key("package"));
    }
}
