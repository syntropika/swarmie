//! Checked private persistence values for account workflow recovery.
//! These values grant no custody, initialization or inference authority. They do
//! not install a native recovery implementation.

mod decode;
mod observation;
mod record;
mod reference;
mod state;

pub use observation::{
    CredentialRead, LegacyObservation, LegacyPredecessor, ProvisioningPlan, StateLoad,
};
pub use record::{
    RecoveryAccount, RecoveryKind, RecoveryPhase, RecoveryRecord, RecoveryReferences,
    ReplacementKind, StateReplacement,
};
pub use reference::{CredentialReference, CredentialSlot, ProtocolBinding, ProviderOwner};
pub use state::{ProviderState, RecoveryLimits};

use serde::Serialize;
use uuid::Uuid;

fn non_nil(value: Uuid) -> tembra::Result<()> {
    if value.is_nil() {
        return Err(crate::invalid(
            "provider recovery requires a non-nil identity",
        ));
    }
    Ok(())
}

fn encoded<T: Serialize>(value: &T, maximum: usize) -> tembra::Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| crate::invalid("provider recovery data could not be encoded"))?;
    if bytes.len() > maximum {
        return Err(crate::invalid(
            "provider recovery data exceeds its storage bound",
        ));
    }
    Ok(bytes)
}
