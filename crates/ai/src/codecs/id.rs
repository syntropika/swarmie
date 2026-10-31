const ALPHABET: &[u8; 64] = b"_-0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

const DEFAULT_LEN: usize = 21;

pub fn generate() -> String {
    generate_with_len(DEFAULT_LEN)
}

macro_rules! counter_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(std::num::NonZeroU64);

        impl $name {
            fn counter() -> &'static std::sync::atomic::AtomicU64 {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
                &NEXT
            }

            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                // Zero is reserved even after counter wraparound.
                loop {
                    if let Some(raw) = std::num::NonZeroU64::new(
                        Self::counter().fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                    ) {
                        return Self(raw);
                    }
                }
            }

            pub const fn from_raw(raw: u64) -> Option<Self> {
                match std::num::NonZeroU64::new(raw) {
                    Some(raw) => Some(Self(raw)),
                    None => None,
                }
            }

            pub const fn to_raw(self) -> u64 {
                self.0.get()
            }
        }

        impl From<std::num::NonZeroU64> for $name {
            fn from(raw: std::num::NonZeroU64) -> Self {
                Self(raw)
            }
        }

        impl From<$name> for std::num::NonZeroU64 {
            fn from(id: $name) -> Self {
                id.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Display::fmt(&self.0, f)
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl std::str::FromStr for $name {
            type Err = ParseIdError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.parse::<std::num::NonZeroU64>()
                    .map(Self)
                    .map_err(|_| ParseIdError(s.to_owned()))
            }
        }

        const _: () = assert!(
            std::mem::size_of::<Option<$name>>() == std::mem::size_of::<u64>()
        );
    };
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid id: expected a non-zero integer, got {0:?}")]
pub struct ParseIdError(String);

counter_id! {
    RunId
}

pub(crate) fn generate_with_len(len: usize) -> String {
    std::iter::repeat_with(|| {
        let idx = fastrand::usize(..ALPHABET.len());
        ALPHABET.get(idx).copied().unwrap_or(b'_') as char
    })
    .take(len)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_ids_are_unique_increasing_and_round_trip() {
        let a = RunId::new();
        let b = RunId::new();
        assert_ne!(a, b);
        assert!(b > a);
        assert_eq!(RunId::from_raw(a.to_raw()), Some(a));
        assert_eq!(RunId::from_raw(0), None);
        let id = RunId::from_raw(42).expect("non-zero");
        assert_eq!(id.to_string(), "42");
        assert_eq!("42".parse::<RunId>(), Ok(id));
        assert!("0".parse::<RunId>().is_err());
        assert_eq!(format!("{id:?}"), "RunId(42)");
        assert_eq!(serde_json::to_string(&id).expect("serialize"), "42");
        assert_eq!(
            serde_json::from_str::<RunId>("42").expect("deserialize"),
            id
        );
    }
}
