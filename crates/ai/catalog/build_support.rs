//! Pure validation shared by the build script and its qualification tests.

use serde::Serialize;

#[path = "model_schema.rs"]
mod model_schema;
use model_schema::{Provider, providers};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const SOURCE_URL: &str = "https://models.dev/api.json";
pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_AGE_MS: u64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Serialize)]
struct Snapshot {
    schema_version: u32,
    source_url: &'static str,
    upstream_sha256: String,
    content_sha256: String,
    fetched_at_ms: u64,
    expires_at_ms: u64,
    providers: BTreeMap<String, Provider>,
}

/// Retain exact model facts, with no inference of route qualification.
/// Only validated bytes are returned, so callers cannot publish partial input.
pub fn snapshot(raw: &[u8], fetched_at_ms: u64) -> Result<Vec<u8>, String> {
    if raw.len() as u64 > MAX_BYTES {
        return Err("catalog exceeds the download limit".into());
    }
    let providers = providers(raw)?;
    if providers.is_empty() {
        return Err("catalog must contain provider records".into());
    }
    let normalized = model_schema::canonical_bytes(&providers)?;
    let expires_at_ms = fetched_at_ms
        .checked_add(MAX_AGE_MS)
        .ok_or("invalid catalog timestamp")?;
    model_schema::canonical_bytes(&Snapshot {
        schema_version: 1,
        source_url: SOURCE_URL,
        upstream_sha256: format!("{:x}", Sha256::digest(raw)),
        content_sha256: format!("{:x}", Sha256::digest(&normalized)),
        fetched_at_ms,
        expires_at_ms,
        providers,
    })
    .map_err(|e| e.to_string())
}
