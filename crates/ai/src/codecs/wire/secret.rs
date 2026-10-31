#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct Secret(String);

const REDACTED: &str = "[redacted]";

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<S: Into<String>> From<S> for Secret {
    fn from(value: S) -> Self {
        Self(value.into())
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(REDACTED)
    }
}

impl serde::Serialize for Secret {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(REDACTED)
    }
}

impl<'de> serde::Deserialize<'de> for Secret {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut value = <String as serde::Deserialize>::deserialize(deserializer)?;
        if value == REDACTED {
            value.clear();
        }
        Ok(Self(value))
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::Secret;
    #[test]
    fn a_secret_never_renders_or_serializes_its_value() {
        let secret = Secret::from("sk-live-do-not-print");
        assert_eq!(format!("{secret:?}"), "[redacted]");
        assert_eq!(
            serde_json::to_string(&secret).expect("a string serializes"),
            "\"[redacted]\""
        );
        assert_eq!(secret.expose(), "sk-live-do-not-print");
    }

    #[test]
    fn secrets_compare_by_value() {
        assert_eq!(Secret::from("k"), Secret::from("k"));
        assert_ne!(Secret::from("k"), Secret::from("j"));
    }

    #[test]
    fn a_secret_deserializes_from_a_bare_string() {
        let secret: Secret = serde_json::from_str("\"k\"").expect("a string deserializes");
        assert_eq!(secret.expose(), "k");
    }

    #[test]
    fn the_redaction_sentinel_reloads_as_no_credential() {
        let stored = serde_json::to_string(&Secret::from("sk-live-do-not-print"))
            .expect("a secret serializes");
        let reloaded: Secret = serde_json::from_str(&stored).expect("the sentinel deserializes");
        assert!(reloaded.is_empty());
        assert_eq!(reloaded, Secret::default());
    }
}
