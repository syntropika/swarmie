use swarmie_counter::{ConfigurationControl, Counter, DynamicConsumer, RemoteCounter};
use tembra::{ContractToken, contracts::Contract};

#[test]
fn synthetic_contracts_reparse_through_the_actual_canonical_schema() {
    fn roundtrip(token: impl ContractToken) {
        let descriptor = token.descriptor().unwrap();
        let bytes = tembra::serde_json::to_string(descriptor.descriptor()).unwrap();
        let parsed = Contract::from_json(&bytes).unwrap();
        assert_eq!(descriptor.fingerprint(), parsed.fingerprint());
    }
    roundtrip(Counter);
    roundtrip(RemoteCounter);
    roundtrip(DynamicConsumer);
    roundtrip(ConfigurationControl);
}
