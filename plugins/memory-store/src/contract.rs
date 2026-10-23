use tembra::prelude::*;

/// Selects one exact namespace and key. Both are case-sensitive UTF-8 strings.
///
/// Namespaces contain 1..=128 UTF-8 bytes; keys contain 1..=512 bytes. Neither may contain
/// Unicode control characters or begin/end with Unicode whitespace. No normalization occurs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct GetRequest {
    pub namespace: String,
    pub key: String,
}

/// A write to an exact namespace and key, using the same validation as [`GetRequest`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct PutRequest {
    pub namespace: String,
    pub key: String,
    pub value: serde_json::Value,
}

/// Selects an exact entry to delete, using the same validation as [`GetRequest`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct DeleteRequest {
    pub namespace: String,
    pub key: String,
}

/// Selects a namespace to list, using the namespace validation of [`GetRequest`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ListRequest {
    pub namespace: String,
}

/// Distinguishes absence from all JSON values, including an explicitly stored `null`.
/// The portable representation is `null` for Missing and `{ "value": ... }` for Present.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", untagged, deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub enum GetResult {
    Missing,
    Present { value: serde_json::Value },
}

/// Local application storage, available to trusted plugins through ordinary Tembra References.
///
/// Each method executes a synchronous SQLite operation during its poll; there is no detached
/// write task. Successful writes use SQLite's NORMAL synchronous WAL mode. A power loss or
/// system crash may roll back recent commits; application-crash recovery remains supported.
/// Storage errors are explicit and must not be interpreted as absence or as proof of rollback
/// after an I/O fault.
/// Host shutdown closes the Resource; retained References cannot revive it. Closing or retiring
/// this compiled plugin never deletes its database. Replacing native declarations needs a new Host.
#[tembra::contract(local, name = "swarmie.memory-store", version = 1)]
pub trait MemoryStore {
    /// Reads an entry. Only a missing row returns Missing; malformed stored JSON is an error.
    #[portable]
    async fn get(&self, request: GetRequest) -> Result<GetResult>;

    /// Atomically inserts or replaces one JSON value. Validation failure performs no SQL write.
    #[portable]
    async fn put(&self, request: PutRequest) -> Result<()>;

    /// Deletes an entry, returning whether it existed. Repeated deletion returns false.
    #[portable]
    async fn delete(&self, request: DeleteRequest) -> Result<bool>;

    /// Returns keys in the exact namespace, sorted by SQLite BINARY (UTF-8 byte) order.
    #[portable]
    async fn list(&self, request: ListRequest) -> Result<Vec<String>>;
}

pub(crate) fn validate(namespace: &str, key: Option<&str>) -> Result<()> {
    validate_part(namespace, 128, "namespace")?;
    if let Some(key) = key {
        validate_part(key, 512, "key")?;
    }
    Ok(())
}

fn validate_part(value: &str, maximum: usize, field: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > maximum
        || value.chars().any(char::is_control)
        || value.starts_with(char::is_whitespace)
        || value.ends_with(char::is_whitespace)
    {
        return Err(RuntimeError::invalid(
            format!("memory_store_invalid_{field}"),
            format!(
                "{field} must contain 1..={maximum} UTF-8 bytes, without control characters or surrounding whitespace"
            ),
        ));
    }
    Ok(())
}
