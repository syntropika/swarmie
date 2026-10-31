use serde::{
    Deserialize,
    de::{self, DeserializeOwned},
};
use std::collections::BTreeSet;

/// Reject ambiguous object keys before ordinary typed decoding can discard any
/// account extension or credential binding. Bounds are checked by the caller.
pub(super) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> tembra::Result<T> {
    serde_json::from_slice::<UniqueDocument>(bytes)
        .map_err(|_| crate::invalid("provider recovery data is invalid or ambiguous"))?;
    serde_json::from_slice(bytes)
        .map_err(|_| crate::invalid("provider recovery data could not be decoded"))
}

struct UniqueDocument;

impl<'de> Deserialize<'de> for UniqueDocument {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Unique;

        impl<'de> de::Visitor<'de> for Unique {
            type Value = UniqueDocument;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("JSON with unique object keys")
            }

            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(UniqueDocument)
            }

            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Ok(UniqueDocument)
            }

            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Ok(UniqueDocument)
            }

            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(UniqueDocument)
            }

            fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(UniqueDocument)
            }

            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueDocument)
            }

            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut values: A,
            ) -> Result<Self::Value, A::Error> {
                while values.next_element::<UniqueDocument>()?.is_some() {}
                Ok(UniqueDocument)
            }

            fn visit_map<A: de::MapAccess<'de>>(
                self,
                mut values: A,
            ) -> Result<Self::Value, A::Error> {
                let mut keys = BTreeSet::new();
                while let Some(key) = values.next_key::<String>()? {
                    if !keys.insert(key) {
                        return Err(de::Error::custom("duplicate provider data key"));
                    }
                    values.next_value::<UniqueDocument>()?;
                }
                Ok(UniqueDocument)
            }
        }

        deserializer.deserialize_any(Unique)
    }
}
