// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
//! Opt-in native qualification touches only a newly generated fixture entry.
use swarmie_accounts::vault::{PlatformVault, Vault};
#[tokio::test]
#[ignore = "requires an unlocked platform credential store"]
async fn isolated_platform_vault_round_trip() {
    let vault = PlatformVault::new(uuid::Uuid::new_v4());
    assert!(vault.read().await.unwrap().is_none());
    vault
        .write("isolated-native-vault-fixture".into())
        .await
        .unwrap();
    let read = vault.read().await;
    let deleted = vault.delete().await;
    assert_eq!(
        read.unwrap().as_deref(),
        Some("isolated-native-vault-fixture")
    );
    deleted.unwrap();
    assert!(vault.read().await.unwrap().is_none());
}
