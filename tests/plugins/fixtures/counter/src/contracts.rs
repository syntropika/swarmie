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

/// Fixture-owned configuration operations, separate from the storage capability.
#[tembra::contract(local, name = "swarmie.acceptance-configuration-control", version = 1)]
pub trait ConfigurationControl {
    #[state]
    fn configuration(&self) -> tembra::serde_json::Value;

    /// Checks the entire candidate before any configuration State mutation.
    #[portable]
    async fn validate(&self, configuration: tembra::serde_json::Value) -> Result<()>;

    /// Revalidates and publishes one configuration State value.
    #[portable]
    async fn apply(&self, configuration: tembra::serde_json::Value) -> Result<()>;
}
