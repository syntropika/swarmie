use serde::{Deserialize, Serialize};
use swarmie_ai::catalog::Snapshot;

/// Milliseconds encoded as a canonical decimal string. The wire value retains
/// all u64 values without conversion through a floating-point JSON number.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MetadataTimestamp(u64);

impl MetadataTimestamp {
    pub fn milliseconds(self) -> u64 {
        self.0
    }
}

impl From<u64> for MetadataTimestamp {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<MetadataTimestamp> for String {
    fn from(value: MetadataTimestamp) -> Self {
        value.0.to_string()
    }
}

impl TryFrom<String> for MetadataTimestamp {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 20
            || !value.bytes().all(|byte| byte.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            return Err("invalid metadata timestamp");
        }
        value
            .parse()
            .map(Self)
            .map_err(|_| "invalid metadata timestamp")
    }
}

/// Safe timing facts from the same operational metadata snapshot. Preparation
/// time is not a live verification time. The reuse boundary does not assert
/// that every fact is fresh: limit evidence can be admitted under stale grace.
/// No boundary means no finite snapshot expiry, not permanent fact freshness.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataFreshness {
    pub prepared_at_ms: MetadataTimestamp,
    pub valid_until_ms: Option<MetadataTimestamp>,
    pub stale_limit_evidence: u32,
}

impl MetadataFreshness {
    pub fn validate(&self) -> tembra::Result<()> {
        if self
            .valid_until_ms
            .is_some_and(|until| until <= self.prepared_at_ms)
        {
            return Err(crate::invalid("invalid metadata snapshot reuse boundary"));
        }
        Ok(())
    }

    pub fn from_snapshot(snapshot: &Snapshot) -> tembra::Result<Self> {
        Ok(Self {
            prepared_at_ms: snapshot.prepared_at_ms().into(),
            valid_until_ms: snapshot.valid_until_ms().map(Into::into),
            stale_limit_evidence: u32::try_from(snapshot.stale_evidence().len())
                .map_err(|_| crate::invalid("too many stale metadata evidence records"))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_preserves_the_entire_integer_range_as_strings() {
        for value in [0, 1, 9_007_199_254_740_993, u64::MAX] {
            let encoded = serde_json::to_string(&MetadataTimestamp::from(value)).unwrap();
            assert_eq!(encoded, format!("\"{value}\""));
            assert_eq!(
                serde_json::from_str::<MetadataTimestamp>(&encoded)
                    .unwrap()
                    .milliseconds(),
                value
            );
        }
    }

    #[test]
    fn timestamp_rejects_numbers_and_noncanonical_or_out_of_range_strings() {
        for value in [
            "0",
            "1",
            "18446744073709551615",
            "null",
            "true",
            "[]",
            "{}",
            "\"\"",
            "\"00\"",
            "\"01\"",
            "\"-1\"",
            "\"+1\"",
            "\"1.0\"",
            "\"1e3\"",
            "\" 1\"",
            "\"1 \"",
            "\"١\"",
            "\"18446744073709551616\"",
            "\"100000000000000000000\"",
        ] {
            assert!(
                serde_json::from_str::<MetadataTimestamp>(value).is_err(),
                "{value}"
            );
        }
    }
}
