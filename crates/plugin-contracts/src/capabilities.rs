use crate::UiRegistryMetadata;

/// Compiled Counter capability shared by native and same-host JavaScript consumers.
#[tembra::contract(local, name = "swarmie.counter", version = 1)]
pub trait Counter {
    #[state]
    fn count(&self) -> u32;

    /// Adds `by` to the live count and publishes the result.
    ///
    /// # Guarantees
    /// Overflow refuses the operation before mutation. Adding zero succeeds unchanged.
    /// Observer failures can be reported after publication and do not undo the count.
    ///
    /// # Errors
    /// Returns `counter_overflow` when the mathematical sum exceeds `u32::MAX`, or a
    /// Tembra lifecycle error when this capability is unavailable.
    #[portable]
    async fn increment(&self, by: u32) -> Result<u32>;

    /// Reads the current live count without mutating or persisting it.
    #[portable]
    async fn read(&self) -> Result<u32>;
}

/// A remote facade over the same Counter object and State.
#[tembra::contract(
    remote,
    name = "swarmie.remote-counter",
    version = 1,
    host = "swarmie-runtime"
)]
pub trait RemoteCounter {
    #[state]
    fn count(&self) -> u32;

    async fn increment(&self, by: u32) -> Result<u32>;

    async fn read(&self) -> Result<u32>;
}

/// Minimal dynamic consumer used to qualify direct plugin-to-plugin calls.
#[tembra::contract(local, name = "swarmie.dynamic-consumer", version = 1)]
pub trait DynamicConsumer {
    #[state]
    fn invocations(&self) -> u32;

    #[portable]
    async fn run(&self) -> Result<u32>;
}

/// Browser-facet contribution metadata tied to the provider object's lifecycle.
#[tembra::contract(local, name = "swarmie.ui-contributions", version = 1)]
pub trait UiContributions {
    #[state]
    fn contributions(&self) -> Vec<UiRegistryMetadata>;
}
