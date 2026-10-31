use secrets_scanner::{ScanConfig, Scanner};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    sync::{Arc, OnceLock},
};
use zeroize::Zeroize;

/// Versioned offline profile. Loading never checks a user cache or fetches rules.
pub const CORPUS: &str = "gitleaks-5f6ec56cfa6f8c67648d863582c7ae2f206d5c1c";

/// Bounded memory-only preparation, including decoded logical fields.
pub const MAX_CONTENT_BYTES: usize = 2 * 1024 * 1024;

const MAX_FINDINGS: usize = 128;
const MAX_FIELDS: usize = 32_768;
const RULE_COUNT: usize = 222;

/// Safe rule and numeric location only: no snippets, hashes or input-controlled labels.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct Finding {
    pub rule: String,
    pub field: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Inspection {
    Clean,
    Detected { findings: Vec<Finding> },
    Incomplete,
    Unavailable,
    Unprotected,
}

/// Qualified content profile is chosen by the adapter, never inferred from attachment names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Coverage {
    TextJson,
    Unsupported,
}

#[derive(Clone)]
pub struct Detector {
    scanner: Arc<Scanner>,
}

impl Detector {
    /// A structural or compile failure disables the engine, never just an individual rule.
    pub fn bundled() -> Option<Self> {
        // Compiled public rules contain no input or per-account policy. Reuse them across
        // native owners; each inspection and its private content remain request-local.
        static SCANNER: OnceLock<Option<Arc<Scanner>>> = OnceLock::new();
        SCANNER
            .get_or_init(|| {
                let scanner = Scanner::from_toml(include_str!("../rules/gitleaks.toml")).ok()?;
                if scanner.engine().rules().len() != RULE_COUNT {
                    return None;
                }
                let mut config = ScanConfig::proxy();
                config.max_file_size = MAX_CONTENT_BYTES as u64;
                config.max_findings_per_file = Some(MAX_FINDINGS);
                Some(Arc::new(scanner.with_config(config)))
            })
            .clone()
            .map(|scanner| Self { scanner })
    }

    pub fn inspect(&self, body: &[u8], coverage: Coverage) -> Inspection {
        if coverage != Coverage::TextJson || body.len() > MAX_CONTENT_BYTES {
            return Inspection::Incomplete;
        }
        // Reject duplicate decoded keys: parsers disagree on first/last value semantics.
        if serde_json::from_slice::<UniqueJson>(body).is_err() {
            return Inspection::Incomplete;
        }
        let Ok(value) = serde_json::from_slice::<Value>(body) else {
            return Inspection::Incomplete;
        };
        let value = PrivateJson(value);
        let mut fields = vec![body];
        collect(&value.0, &mut fields);
        if fields.len() > MAX_FIELDS {
            return Inspection::Incomplete;
        }
        let mut findings = BTreeSet::new();
        for (field, content) in fields.into_iter().enumerate() {
            let Ok(output) = self.scanner.scan_proxy(content) else {
                return Inspection::Incomplete;
            };
            if output.findings_truncated {
                return Inspection::Incomplete;
            }
            for found in output.findings {
                findings.insert(Finding {
                    rule: found.rule_id,
                    field,
                    start: found.secret_start_offset,
                    end: found.secret_end_offset,
                });
                if findings.len() > MAX_FINDINGS {
                    return Inspection::Incomplete;
                }
            }
        }
        if findings.is_empty() {
            Inspection::Clean
        } else {
            Inspection::Detected {
                findings: findings.into_iter().collect(),
            }
        }
    }
}

fn collect<'a>(value: &'a Value, fields: &mut Vec<&'a [u8]>) {
    if fields.len() > MAX_FIELDS {
        return;
    }
    match value {
        Value::String(s) => fields.push(s.as_bytes()),
        Value::Array(values) => {
            for v in values {
                collect(v, fields);
                if fields.len() > MAX_FIELDS {
                    break;
                }
            }
        }
        Value::Object(values) => {
            for (key, v) in values {
                fields.push(key.as_bytes());
                collect(v, fields);
                if fields.len() > MAX_FIELDS {
                    break;
                }
            }
        }
        _ => {}
    }
}

// The parser necessarily allocates decoded strings; clear those extra private copies too.
struct PrivateJson(Value);

impl Drop for PrivateJson {
    fn drop(&mut self) {
        fn clear(value: Value) {
            match value {
                Value::String(mut text) => text.zeroize(),
                Value::Array(values) => values.into_iter().for_each(clear),
                Value::Object(values) => {
                    for (mut key, value) in values {
                        key.zeroize();
                        clear(value);
                    }
                }
                _ => {}
            }
        }
        clear(std::mem::take(&mut self.0));
    }
}

// This validation pass retains no values and bounds recursion through serde_json's own limit.
struct UniqueJson;
impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueJson;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("unambiguous JSON")
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_string<E: serde::de::Error>(self, mut value: String) -> Result<UniqueJson, E> {
                value.zeroize();
                Ok(UniqueJson)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<UniqueJson, A::Error> {
                while seq.next_element::<UniqueJson>()?.is_some() {}
                Ok(UniqueJson)
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<UniqueJson, A::Error> {
                let mut keys = PrivateKeys(BTreeSet::<String>::new());
                while let Some(key) = map.next_key::<String>()? {
                    let key = zeroize::Zeroizing::new(key);
                    if keys.0.contains(key.as_str()) || keys.0.len() >= MAX_FIELDS {
                        return Err(serde::de::Error::custom("ambiguous or oversized object"));
                    }
                    keys.0.insert(key.to_string());
                    map.next_value::<UniqueJson>()?;
                }
                Ok(UniqueJson)
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

struct PrivateKeys(BTreeSet<String>);
impl Drop for PrivateKeys {
    fn drop(&mut self) {
        for mut key in std::mem::take(&mut self.0) {
            key.zeroize();
        }
    }
}
