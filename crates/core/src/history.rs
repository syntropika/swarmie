use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NativeHistoryQuery {
    pub harness: String,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub config_home: Option<PathBuf>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}
fn default_limit() -> usize {
    100
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NativeHistorySession {
    pub native: NativeSessionRef,
    pub cwd: PathBuf,
    pub title: String,
    pub path: PathBuf,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub metadata: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NativeMessageNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub role: String,
    pub text: String,
    pub active: bool,
    #[serde(default)]
    pub metadata: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NativeHistoryRecord {
    pub turn_id: Option<String>,
    pub kind: EventKind,
    pub raw: Option<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NativeHistoryDocument {
    pub session: NativeHistorySession,
    pub options: SpawnOptions,
    pub capabilities: Capabilities,
    pub records: Vec<NativeHistoryRecord>,
    pub tree: Vec<NativeMessageNode>,
}
