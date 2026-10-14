//! Shared native SDK engine, generated boundary types, and bounded transport ownership.

mod events;
mod handles;
mod transport;

pub use handles::execute;

/// Types and dispatch signatures generated from the versioned WIT interface.
#[allow(async_fn_in_trait)]
pub mod boundary {
    include!(concat!(env!("OUT_DIR"), "/boundary.rs"));
}

use boundary::{ClientError, ErrorKind};
use serde_json::Value;

impl ClientError {
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            code: None,
            data_json: None,
        }
    }

    /// Return a coarse internal failure without exposing credentials or request data.
    pub fn internal() -> Self {
        Self::new(ErrorKind::Internal, "Native client internal failure")
    }

    pub(crate) fn protocol(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Protocol, message)
    }
}

/// Validate an RPC envelope without conflating a null result with a missing result.
pub fn decode(value: Value, id: &str) -> Result<Value, ClientError> {
    let object = value
        .as_object()
        .ok_or_else(|| ClientError::protocol("Invalid JSON-RPC response"))?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || object.get("id").and_then(Value::as_str) != Some(id)
    {
        return Err(ClientError::protocol("JSON-RPC response identity mismatch"));
    }
    if let Some(error) = object.get("error") {
        if object.contains_key("result") {
            return Err(ClientError::protocol("Ambiguous JSON-RPC response"));
        }
        let code = error
            .get("code")
            .and_then(Value::as_i64)
            .ok_or_else(|| ClientError::protocol("Malformed JSON-RPC error"))?;
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .ok_or_else(|| ClientError::protocol("Malformed JSON-RPC error"))?;
        return Err(ClientError {
            kind: ErrorKind::Rpc,
            message: message.into(),
            code: Some(code),
            data_json: error.get("data").map(Value::to_string),
        });
    }
    object
        .get("result")
        .cloned()
        .ok_or_else(|| ClientError::protocol("JSON-RPC response has no result"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn envelopes_preserve_null_and_errors_but_reject_ambiguity() {
        assert_eq!(
            decode(json!({"jsonrpc":"2.0","id":"a","result":null}), "a").unwrap(),
            Value::Null
        );
        let error = decode(json!({"jsonrpc":"2.0","id":"a","error":{"code":-32602,"message":"invalid","data":{"field":"cwd"}}}), "a").unwrap_err();
        assert_eq!(error.code, Some(-32602));
        assert_eq!(error.data_json.as_deref(), Some("{\"field\":\"cwd\"}"));
        for value in [
            json!({"jsonrpc":"2.0","id":"b","result":null}),
            json!({"jsonrpc":"2.0","id":"a"}),
            json!({"jsonrpc":"2.0","id":"a","result":null,"error":{}}),
        ] {
            assert!(decode(value, "a").is_err());
        }
    }
}
