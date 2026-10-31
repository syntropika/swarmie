use std::collections::BTreeMap;
use std::fmt;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::codecs::completion::{CompletionRequest, ReplayTarget};
use crate::codecs::json_utils::remove_preserving_order;
use crate::codecs::message::Api;

pub const SHARED: &str = "*";

pub trait ProviderExtension {
    const PROVIDER: &'static str;
    type Options: ExtensionOptions;
    type Extras: ReplyExtras;
}

pub trait ExtensionOptions:
    Serialize + Clone + fmt::Debug + Send + Sync + UnwindSafe + RefUnwindSafe + 'static
{
    type Ext: ProviderExtension<Options = Self>;

    fn unsupported(
        &self,
        target: &dyn ReplayTarget,
        request: &CompletionRequest,
    ) -> Vec<(&'static str, String)> {
        let _ = (target, request);
        Vec::new()
    }
}

pub trait ReplyExtras: Sized {
    fn from_reply(api: &Api, raw: &Value) -> Result<Self, serde_json::Error>;
}

pub(crate) fn reply_field<T: serde::de::DeserializeOwned>(
    raw: &Value,
    pointer: &str,
) -> Result<Option<T>, serde_json::Error> {
    match raw.pointer(pointer) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) if items.is_empty() => Ok(None),
        Some(value) => T::deserialize(value).map(Some),
    }
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum OptionsError {
    #[error("{provider} options do not serialize: {source}")]
    Serialize {
        provider: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("{provider} options must serialize to an object of sections, each an object")]
    NotSections { provider: &'static str },
}

trait Refusals: fmt::Debug + Send + Sync + UnwindSafe + RefUnwindSafe {
    fn refusals(
        &self,
        target: &dyn ReplayTarget,
        request: &CompletionRequest,
    ) -> Vec<(&'static str, String)>;
}

impl<T: ExtensionOptions> Refusals for T {
    fn refusals(
        &self,
        target: &dyn ReplayTarget,
        request: &CompletionRequest,
    ) -> Vec<(&'static str, String)> {
        self.unsupported(target, request)
    }
}

#[derive(Clone)]
enum Entry {
    Sections {
        sections: Map<String, Value>,
        typed: Option<Arc<dyn Refusals>>,
    },
    Failed(Failure),
}

#[derive(Clone)]
struct Failure(Arc<OptionsError>);

impl UnwindSafe for Failure {}
impl RefUnwindSafe for Failure {}

impl Entry {
    fn sections(&self) -> Option<&Map<String, Value>> {
        match self {
            Self::Sections { sections, .. } => Some(sections),
            Self::Failed(_) => None,
        }
    }
}

fn sections_of<P: ProviderExtension>(
    options: &P::Options,
) -> Result<Map<String, Value>, OptionsError> {
    let provider = P::PROVIDER;
    let value = serde_json::to_value(options)
        .map_err(|source| OptionsError::Serialize { provider, source })?;
    let Value::Object(sections) = value else {
        return Err(OptionsError::NotSections { provider });
    };
    non_empty_sections(sections).ok_or(OptionsError::NotSections { provider })
}

#[derive(Clone, Default)]
pub struct ProviderOptions(BTreeMap<String, Entry>);

impl ProviderOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert<P: ProviderExtension>(
        &mut self,
        options: &P::Options,
    ) -> Result<&mut Self, OptionsError> {
        let sections = sections_of::<P>(options)?;
        self.put(P::PROVIDER, sections, || Arc::new(options.clone()));
        Ok(self)
    }

    pub fn set<O: ExtensionOptions>(mut self, options: O) -> Self {
        let provider = <O::Ext as ProviderExtension>::PROVIDER;
        match sections_of::<O::Ext>(&options) {
            Ok(sections) => self.put(provider, sections, || Arc::new(options)),
            Err(error) => {
                self.0
                    .insert(provider.to_owned(), Entry::Failed(Failure(Arc::new(error))));
            }
        }
        self
    }

    fn put(
        &mut self,
        provider: &str,
        sections: Map<String, Value>,
        typed: impl FnOnce() -> Arc<dyn Refusals>,
    ) {
        if sections.is_empty() {
            self.0.remove(provider);
        } else {
            self.0.insert(
                provider.to_owned(),
                Entry::Sections {
                    sections,
                    typed: Some(typed()),
                },
            );
        }
    }

    pub fn with<P: ProviderExtension>(
        mut self,
        options: &P::Options,
    ) -> Result<Self, OptionsError> {
        self.insert::<P>(options)?;
        Ok(self)
    }

    pub fn get<P: ProviderExtension>(&self) -> Option<&Map<String, Value>> {
        self.0.get(P::PROVIDER).and_then(Entry::sections)
    }

    pub fn remove<P: ProviderExtension>(&mut self) {
        self.0.remove(P::PROVIDER);
    }

    pub fn contains<P: ProviderExtension>(&self) -> bool {
        self.0.contains_key(P::PROVIDER)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn overlay(mut self, over: &ProviderOptions) -> ProviderOptions {
        for (provider, entry) in &over.0 {
            self.0.insert(provider.clone(), entry.clone());
        }
        self
    }

    pub(crate) fn sections(&self, provider: &str) -> Option<&Map<String, Value>> {
        self.0.get(provider).and_then(Entry::sections)
    }

    pub(crate) fn failure(&self) -> Option<&Arc<OptionsError>> {
        self.0.values().find_map(|entry| match entry {
            Entry::Failed(Failure(error)) => Some(error),
            Entry::Sections { .. } => None,
        })
    }

    pub(crate) fn refusals(
        &self,
        provider: &str,
        target: &dyn ReplayTarget,
        request: &CompletionRequest,
    ) -> Vec<(&'static str, String)> {
        match self.0.get(provider) {
            Some(Entry::Sections {
                typed: Some(typed), ..
            }) => typed.refusals(target, request),
            _ => Vec::new(),
        }
    }

    pub(crate) fn remove_field(&mut self, provider: &str, sections: &[&str], field: &str) {
        if let Some(Entry::Sections {
            sections: entry, ..
        }) = self.0.get_mut(provider)
        {
            for section in sections {
                if let Some(Value::Object(fields)) = entry.get_mut(*section) {
                    remove_preserving_order(fields, field);
                }
            }
        }
    }
}

fn non_empty_sections(sections: Map<String, Value>) -> Option<Map<String, Value>> {
    let mut kept = Map::new();
    for (name, section) in sections {
        match section {
            Value::Object(fields) if fields.is_empty() => {}
            Value::Object(fields) => {
                kept.insert(name, Value::Object(fields));
            }
            _ => return None,
        }
    }
    Some(kept)
}

impl fmt::Debug for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sections { sections, .. } => sections.fmt(f),
            Self::Failed(Failure(error)) => {
                f.debug_tuple("Failed").field(&error.to_string()).finish()
            }
        }
    }
}

impl fmt::Debug for ProviderOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(&self.0).finish()
    }
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Sections { sections: a, .. }, Self::Sections { sections: b, .. }) => a == b,
            (Self::Failed(Failure(a)), Self::Failed(Failure(b))) => a.to_string() == b.to_string(),
            _ => false,
        }
    }
}

impl PartialEq for ProviderOptions {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Serialize for ProviderOptions {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(error) = self.failure() {
            return Err(serde::ser::Error::custom(error));
        }
        serializer.collect_map(
            self.0
                .iter()
                .filter_map(|(provider, entry)| Some((provider, entry.sections()?))),
        )
    }
}

impl<'de> Deserialize<'de> for ProviderOptions {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entries = BTreeMap::<String, Map<String, Value>>::deserialize(deserializer)?;
        let mut options = BTreeMap::new();
        for (provider, sections) in entries {
            let sections = non_empty_sections(sections).ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "{provider} options must be an object of sections, each an object"
                ))
            })?;
            if !sections.is_empty() {
                options.insert(
                    provider,
                    Entry::Sections {
                        sections,
                        typed: None,
                    },
                );
            }
        }
        Ok(Self(options))
    }
}
