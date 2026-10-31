//! Native private-entry contributions. Raw values never cross an Orb method or serialized State.
use crate::*;
use std::{collections::BTreeSet, rc::Rc};

/// A compiled sink with AcceptAndStore may observe only a newly entered value. It receives no
/// vault key, existing-secret getter, decision issuer or serialized credential. Its implementation
/// must settle synchronously before returning; external side effects are not rolled back.
pub trait PrivateEntrySink {
    fn accept(&self, intent: &MutationIntent, value: &SecretValue) -> Result<()>;
}

/// Opaque native contribution. The embedding pins provider identity and the actual accepted grant;
/// neither can be supplied by an agent's request or asserted by an extension manifest string.
#[derive(Clone)]
pub struct PrivateEntryHook {
    provider: String,
    grant: PluginGrant,
    sink: Rc<dyn PrivateEntrySink>,
}
impl PrivateEntryHook {
    pub fn new(
        provider: String,
        grant: PluginGrant,
        sink: Rc<dyn PrivateEntrySink>,
    ) -> Result<Self> {
        grant.check(&provider, Permission::AcceptAndStore)?;
        Ok(Self {
            provider,
            grant,
            sink,
        })
    }
}

/// A read of the embedding's active native contributions, not another plugin registry. For a
/// Plugin Manager composition, read `active_contributions::<PrivateEntryHook>()` from its index.
#[derive(Clone)]
pub struct PrivateEntries(Rc<dyn Fn() -> Vec<PrivateEntryHook>>);
impl Default for PrivateEntries {
    fn default() -> Self {
        Self(Rc::new(Vec::new))
    }
}
impl PrivateEntries {
    pub fn new(active: impl Fn() -> Vec<PrivateEntryHook> + 'static) -> Self {
        Self(Rc::new(active))
    }
    /// Called only by the origin-authenticated human entry owner, before the core save dispatch.
    /// Validate the entire contribution set before releasing a value to any recipient.
    pub fn accept(&self, intent: &MutationIntent, value: &SecretValue) -> Result<()> {
        let hooks = (self.0)();
        let mut seen = BTreeSet::new();
        for hook in &hooks {
            hook.grant
                .check(&hook.provider, Permission::AcceptAndStore)?;
            if !seen.insert(&hook.provider) {
                return Err(Error::Invalid);
            }
        }
        for hook in hooks {
            hook.grant
                .check(&hook.provider, Permission::AcceptAndStore)?;
            hook.sink.accept(intent, value)?;
        }
        Ok(())
    }
}
