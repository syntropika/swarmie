//! Synthetic Counter fixture for native, dynamic and remote composition acceptance.

mod contracts;

pub use contracts::*;
use tembra::{core, prelude::*};

pub use contracts::CounterInput as Input;

/// Registry exercised by synthetic capability contribution acceptance.
pub const CAPABILITY_REGISTRY: &str = "swarmie.capabilities";

/// Logical identity of the compiled plugin package.
pub const PLUGIN_ID: &str = "swarmie.counter";

/// Executable identity, independent of the logical plugin package.
pub const FACET_ID: &str = "swarmie.counter.runtime";

/// Creates the compiled Counter facet with an initial value of zero.
///
/// The local object lives until Host shutdown and contributes a checked increment operation
/// to the capability registry. Remote consumers forward to that same object and State.
///
/// # Errors
/// Returns a Tembra declaration error if registry or facet metadata is invalid.
pub fn facet() -> Result<NativeFacet> {
    facet_with_initial(0)
}

/// Creates a Counter facet with an explicitly selected initial value.
///
/// The seed is applied only when the local singleton is created. It is not durable storage.
///
/// # Errors
/// Returns a Tembra declaration error if registry or facet metadata is invalid.
pub fn facet_with_initial(initial: u32) -> Result<NativeFacet> {
    let registry = core::Registry::new(CAPABILITY_REGISTRY, core::ConflictPolicy::Error)?;
    NativeFacet::new(FACET_ID, vec![], move |_, setup| {
        let registry = registry.clone();
        let count_definition = setup.state("count")?;
        setup.serve_with_options(
            Counter,
            move |_, _, declaration| {
                declaration.core_mut().register_method(
                    &registry,
                    PLUGIN_ID,
                    tembra::serde_json::json!({
                        "plugin_id": PLUGIN_ID,
                        "contract": "swarmie.counter",
                        "operation": "increment",
                    }),
                    "increment",
                )?;
                Ok(CounterService {
                    count: declaration.state(count_definition.value(initial)?)?,
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )?;
        setup.serve(RemoteCounter, |_, _, declaration| {
            Ok(RemoteCounterService {
                counter: declaration.get(Counter, CounterInput {})?,
            })
        })
    })
}

struct CounterService {
    count: Cell<u32>,
}

impl CounterProvider for CounterService {
    fn count(&self) -> Readable<u32> {
        self.count.readable()
    }

    async fn increment(&self, by: u32, context: Context) -> Result<u32> {
        self.count.change(&context, |value| {
            let candidate = value.checked_add(by).ok_or_else(|| {
                RuntimeError::invalid("counter_overflow", "counter reached its maximum value")
            })?;
            *value = candidate;
            Ok(())
        })?;
        self.count.value()
    }

    async fn read(&self, _context: Context) -> Result<u32> {
        self.count.value()
    }
}

struct RemoteCounterService {
    counter: CounterRef,
}

impl RemoteCounterProvider for RemoteCounterService {
    fn count(&self) -> Readable<u32> {
        self.counter.count()
    }

    async fn increment(&self, by: u32, context: Context) -> Result<u32> {
        self.counter.with_context(&context).increment(by).await
    }

    async fn read(&self, context: Context) -> Result<u32> {
        self.counter.with_context(&context).read().await
    }
}
