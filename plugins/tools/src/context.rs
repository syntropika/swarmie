//! Typed inbound context and explicitly published host-only tool-result metadata.
//!
//! ```
//! use swarmie_tools::{ContextValue, ToolContext};
//! #[derive(serde::Serialize, serde::Deserialize)]
//! struct Session(String);
//! impl ContextValue for Session { const KEY: &'static str = "session"; }
//! let mut context = ToolContext::new();
//! context.insert(Session("example".into()))?;
//! assert_eq!(context.require::<Session>()?.0, "example");
//! # Ok::<(), swarmie_tools::ToolContextError>(())
//! ```

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{OperationId, ToolCall, ToolExecutionError};
use tembra::Context;

#[derive(Clone)]
struct Invocation {
    context: Context,
    operation: OperationId,
    call: ToolCall,
}

/// Context passed to every tool execution.
///
/// Callers insert typed inbound values with [`insert`](Self::insert). Tools read
/// those values with [`get`](Self::get) or [`require`](Self::require), and attach
/// host-only result metadata with [`insert_result`](Self::insert_result). Result
/// callers inspect that metadata through [`result`](Self::result). Neither inbound
/// values nor result metadata are sent to the model.
///
/// Every value is **data**: inserting serializes it, reading deserializes it,
/// and the whole context is itself `Serialize + Deserialize`. Explicit scene
/// serialization can therefore contain sensitive inbound values and must be
/// protected by the host. Result observers receive only [`ToolResultContext`]:
/// values explicitly published through `insert_result`, never inbound slots
/// or runtime authority. Recovery uses the current dispatch's inbound context;
/// code must not treat mutations to inbound slots as durable output. Values with
/// interior sharing (`Arc<Mutex<_>>`, atomics, channels) do not belong here;
/// they belong to the tool instance.
///
/// Tool dispatch clones inbound values once per call.
/// Map-level mutations (inserting, replacing, or removing typed slots) affect
/// only that execution. The dispatch surface returns result metadata without
/// replacing the caller's inbound slots.
///
/// Slots use the value type's declared [`ContextValue::KEY`]. Accessors need
/// no key argument; types must use distinct keys to occupy distinct slots.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct ToolContext {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    inbound: BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    result: BTreeMap<String, serde_json::Value>,
    #[serde(skip)]
    invocation: Option<Invocation>,
}

/// Explicitly published, serializable tool-result metadata. Unlike [`ToolContext`],
/// this contains no inbound values or live runtime authority. Values are
/// keyed by [`ContextValue::KEY`]; applications own their schema versions.
#[derive(Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolResultContext(BTreeMap<String, serde_json::Value>);

impl std::fmt::Debug for ToolResultContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ToolResultContext")
            .field(&self.0.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ToolResultContext {
    /// Read a published value without exposing ambient execution inputs.
    pub fn get<T: ContextValue>(&self) -> Result<Option<T>, ToolContextError> {
        get_slot(&self.0)
    }
}

impl PartialEq for ToolContext {
    fn eq(&self, other: &Self) -> bool {
        self.inbound == other.inbound && self.result == other.result
    }
}

impl Eq for ToolContext {}

/// Serializable context data with a stable slot key. Distinct value types must
/// use distinct keys, and persisted keys must remain stable across schema changes.
pub trait ContextValue: Serialize + DeserializeOwned + 'static {
    /// The slot this value lives under.
    const KEY: &'static str;
}

fn encode<T: ContextValue>(value: &T) -> Result<serde_json::Value, ToolContextError> {
    serde_json::to_value(value).map_err(|error| ToolContextError::Encode {
        key: T::KEY,
        source: error,
    })
}

fn decode<T: ContextValue>(value: &serde_json::Value) -> Result<T, ToolContextError> {
    T::deserialize(value).map_err(|error| ToolContextError::Decode {
        key: T::KEY,
        source: error,
    })
}

/// Replaces a slot after successful encoding, returning the displaced value
/// only if it decodes as `T`.
fn insert_slot<T: ContextValue>(
    map: &mut BTreeMap<String, serde_json::Value>,
    value: T,
) -> Result<Option<T>, ToolContextError> {
    let encoded = encode(&value)?;
    Ok(map
        .insert(T::KEY.to_owned(), encoded)
        .and_then(|previous| decode(&previous).ok()))
}

/// `Ok(None)` when the slot is empty, `Err(Decode)` when it holds something
/// that is not a `T`: absence and a shape mismatch are different facts.
fn get_slot<T: ContextValue>(
    map: &BTreeMap<String, serde_json::Value>,
) -> Result<Option<T>, ToolContextError> {
    map.get(T::KEY).map(decode).transpose()
}

impl ToolContext {
    /// Create an empty context.
    pub const fn new() -> Self {
        Self {
            inbound: BTreeMap::new(),
            result: BTreeMap::new(),
            invocation: None,
        }
    }

    /// The physical invocation's authority and cancellation context.
    /// Direct author calls have no invocation and return a context error.
    pub fn execution(&self) -> Result<&Context, ToolContextError> {
        Ok(&self
            .invocation
            .as_ref()
            .ok_or(ToolContextError::Missing("execution"))?
            .context)
    }

    /// Stable operation identity supplied by the execution owner.
    pub fn operation(&self) -> Result<OperationId, ToolContextError> {
        Ok(self
            .invocation
            .as_ref()
            .ok_or(ToolContextError::Missing("execution"))?
            .operation)
    }

    /// The exact confirmed call being executed.
    pub fn call(&self) -> Result<&ToolCall, ToolContextError> {
        Ok(&self
            .invocation
            .as_ref()
            .ok_or(ToolContextError::Missing("execution"))?
            .call)
    }

    pub(crate) fn enter(&mut self, context: Context, operation: OperationId, call: ToolCall) {
        self.invocation = Some(Invocation {
            context,
            operation,
            call,
        });
    }

    /// Insert an inbound typed value, returning the displaced value if it decodes
    /// as `T`. A differently shaped displaced value is replaced successfully and
    /// returns `None`; decoding the previous value cannot undo a successful write.
    ///
    /// Returns an encoding error if serialization fails, leaving the slot unchanged.
    pub fn insert<T: ContextValue>(&mut self, value: T) -> Result<Option<T>, ToolContextError> {
        insert_slot(&mut self.inbound, value)
    }

    /// Read an inbound typed value: `Ok(None)` when absent, a decode error when
    /// the stored value does not match `T`.
    pub fn get<T: ContextValue>(&self) -> Result<Option<T>, ToolContextError> {
        get_slot(&self.inbound)
    }

    /// Require an inbound typed value.
    pub fn require<T: ContextValue>(&self) -> Result<T, ToolContextError> {
        get_slot(&self.inbound)?.ok_or(ToolContextError::Missing(T::KEY))
    }

    /// Remove an inbound typed value. The slot is removed even if decoding its
    /// former value fails.
    pub fn remove<T: ContextValue>(&mut self) -> Result<Option<T>, ToolContextError> {
        self.inbound
            .remove(T::KEY)
            .map(|value| decode(&value))
            .transpose()
    }

    /// Attach host-only metadata to this execution's result.
    pub fn insert_result<T: ContextValue>(
        &mut self,
        value: T,
    ) -> Result<Option<T>, ToolContextError> {
        insert_slot(&mut self.result, value)
    }

    /// Read host-only result metadata.
    pub fn result<T: ContextValue>(&self) -> Result<Option<T>, ToolContextError> {
        get_slot(&self.result)
    }

    /// Whether this context contains the inbound type `T`.
    pub fn contains<T: ContextValue>(&self) -> bool {
        self.inbound.contains_key(T::KEY)
    }

    /// Whether both maps are empty.
    pub fn is_empty(&self) -> bool {
        self.inbound.is_empty() && self.result.is_empty()
    }

    /// Build the snapshot one dispatch runs against: the same inbound values
    /// and no result metadata. Runtimes call this per tool call so map-level
    /// mutations inside the call cannot leak into the caller's context.
    pub fn for_dispatch(&self) -> Self {
        Self {
            inbound: self.inbound.clone(),
            result: BTreeMap::new(),
            invocation: self.invocation.clone(),
        }
    }

    /// Publish the result metadata a dispatch produced (see
    /// [`Self::for_dispatch`]) while keeping the caller's inbound values.
    pub fn accept_dispatch_result(&mut self, dispatched: Self) {
        self.result = dispatched.result;
    }

    /// Snapshot only the values explicitly published with `insert_result`.
    pub fn result_context(&self) -> ToolResultContext {
        ToolResultContext(self.result.clone())
    }

    /// The values explicitly published with `insert_result`, taken.
    pub fn into_result_context(self) -> ToolResultContext {
        ToolResultContext(self.result)
    }

    /// Restore saved output while preserving this dispatch's inbound values.
    pub fn with_result_context(mut self, result: ToolResultContext) -> Self {
        self.result = result.0;
        self
    }

    /// Drop the previous dispatch's result metadata before starting another.
    pub fn clear_dispatch_result(&mut self) {
        self.result.clear();
    }
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("inbound_types", &self.inbound.keys().collect::<Vec<_>>())
            .field("result_types", &self.result.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// A [`ToolContext`] slot could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum ToolContextError {
    /// A required typed value was absent.
    #[error("required tool context value `{0}` was not found")]
    Missing(&'static str),
    /// A value could not be represented as JSON.
    #[error("tool context value `{key}` could not be encoded: {source}")]
    Encode {
        /// The slot's type name.
        key: &'static str,
        /// The serializer's failure.
        #[source]
        source: serde_json::Error,
    },
    /// A stored value could not be decoded as the requested type.
    #[error("tool context value `{key}` could not be decoded: {source}")]
    Decode {
        /// The slot's type name.
        key: &'static str,
        /// The deserializer's failure.
        #[source]
        source: serde_json::Error,
    },
}

impl From<ToolContextError> for ToolExecutionError {
    fn from(error: ToolContextError) -> Self {
        ToolExecutionError::from_error(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Counter(u32);
    impl ContextValue for Counter {
        const KEY: &'static str = "test.counter";
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct RequestId(String);
    impl ContextValue for RequestId {
        const KEY: &'static str = "test.request_id";
    }

    // Two shapes that deliberately share a key: the fixture for "the slot holds
    // something that is not a `T`".
    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct A(u32);
    impl ContextValue for A {
        const KEY: &'static str = "k";
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct B {
        name: String,
    }
    impl ContextValue for B {
        const KEY: &'static str = "k";
    }

    #[test]
    fn context_separates_inbound_and_result_values() {
        let mut context = ToolContext::new();
        context.insert(Counter(42)).unwrap();
        context
            .insert_result(RequestId("request-1".to_string()))
            .unwrap();
        assert_eq!(context.get::<Counter>().unwrap(), Some(Counter(42)));
        assert_eq!(
            context.result::<RequestId>().unwrap(),
            Some(RequestId("request-1".to_string()))
        );

        let next = context.for_dispatch();
        assert_eq!(next.get::<Counter>().unwrap(), Some(Counter(42)));
        assert_eq!(next.result::<RequestId>().unwrap(), None);
    }

    #[test]
    fn context_round_trips_through_serde() {
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct Session {
            id: String,
        }
        impl ContextValue for Session {
            const KEY: &'static str = "test.session";
        }
        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct Seq(u64);
        impl ContextValue for Seq {
            const KEY: &'static str = "test.seq";
        }
        let mut context = ToolContext::new();
        context
            .insert(Session {
                id: "abc".to_string(),
            })
            .unwrap();
        context.insert_result(Seq(7)).unwrap();

        let json = serde_json::to_string(&context).unwrap();
        let back: ToolContext = serde_json::from_str(&json).unwrap();
        assert_eq!(back, context);
        assert_eq!(
            back.get::<Session>().unwrap(),
            Some(Session {
                id: "abc".to_string()
            })
        );
        assert_eq!(back.result::<Seq>().unwrap(), Some(Seq(7)));

        let empty: ToolContext = serde_json::from_str("{}").unwrap();
        assert!(empty.is_empty());
        assert_eq!(serde_json::to_string(&ToolContext::new()).unwrap(), "{}");
    }

    #[test]
    fn decode_failure_keeps_the_serde_error_as_its_source() {
        let mut context = ToolContext::new();
        context
            .inbound
            .insert(Counter::KEY.to_string(), serde_json::json!("not a number"));
        let error = context.require::<Counter>().unwrap_err();
        let source = std::error::Error::source(&error)
            .and_then(|source| source.downcast_ref::<serde_json::Error>())
            .expect("decode failure should expose the serde error");
        assert!(source.is_data());
    }

    #[test]
    fn insert_replaces_an_undecodable_displaced_value_and_returns_none() {
        let mut context = ToolContext::new();
        context.insert(A(1)).unwrap();
        assert_eq!(
            context
                .insert(B {
                    name: "b".to_string()
                })
                .unwrap(),
            None
        );
        assert_eq!(
            context.get::<B>().unwrap(),
            Some(B {
                name: "b".to_string()
            })
        );
    }
}
