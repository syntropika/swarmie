//! Public provider administration values. Credentials never enter account projections.
use futures::future::LocalBoxFuture;
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use tembra::{Orb, Service};

pub const SETTINGS_SERVICE: &str = "swarmie.providers.settings";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub efforts: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelDefaults {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub auxiliary_model: Option<String>,
}

impl ModelDefaults {
    pub fn validate(&self, models: &[ModelInfo]) -> tembra::Result<()> {
        let selected = match &self.model {
            Some(id) => models.iter().find(|model| model.id == *id),
            None => models.first(),
        };
        if self.model.is_some() && selected.is_none() {
            return Err(crate::invalid("the default model is not available"));
        }
        if let Some(effort) = &self.effort
            && !selected.is_some_and(|model| model.efforts.contains(effort))
        {
            return Err(crate::invalid("the default reasoning is not supported"));
        }
        if let Some(id) = &self.auxiliary_model
            && !models.iter().any(|model| model.id == *id)
        {
            return Err(crate::invalid("the auxiliary model is not available"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetModelDefaults {
    pub account_id: String,
    pub defaults: ModelDefaults,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Account {
    pub id: String,
    pub provider_id: String,
    pub provider_label: String,
    pub label: String,
    pub models: Vec<ModelInfo>,
    pub available: bool,
    /// Existing saved accounts start with automatic model selection.
    #[serde(default)]
    pub defaults: ModelDefaults,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_method: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub models_truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigurationKind {
    ApiKey,
    Local,
    Oauth,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationMethod {
    pub id: String,
    pub label: String,
    pub kind: ConfigurationKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub id: String,
    pub label: String,
    pub publisher: String,
    pub methods: Vec<ConfigurationMethod>,
}

/// Product provider choices shared by the native composition and presentation fixtures.
pub fn provider_catalog() -> Vec<Choice> {
    serde_json::from_str(include_str!("../catalog.json"))
        .expect("the bundled provider catalog is checked by tests")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderCatalog {
    pub choices: Vec<Choice>,
    pub accounts: Vec<Account>,
    pub next_cursor: Option<String>,
}

// Deliberately no Debug: this input may contain a user-supplied secret.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigureProvider {
    pub provider_id: String,
    pub method: String,
    pub credential: Option<String>,
    pub url: Option<String>,
}

/// The embedding owns authentication, credential custody and provider I/O.
pub trait ProviderSettings {
    fn list(
        &self,
        principal: String,
        cursor: Option<String>,
    ) -> LocalBoxFuture<'_, tembra::Result<ProviderCatalog>>;
    fn configure(
        &self,
        principal: String,
        input: ConfigureProvider,
    ) -> LocalBoxFuture<'_, tembra::Result<Account>>;
    fn set_defaults(
        &self,
        principal: String,
        input: SetModelDefaults,
    ) -> LocalBoxFuture<'_, tembra::Result<ModelDefaults>>;
}

pub fn settings_orb(source: Rc<dyn ProviderSettings>) -> Orb {
    Orb::new(SETTINGS_SERVICE, 1).service(
        Service::new(SETTINGS_SERVICE, move |declaration| {
            if declaration.address()?.key != ["main"] {
                return Err(tembra::Error::invalid(
                    "providers.key",
                    "provider settings uses the main key",
                ));
            }
            Ok(source.clone())
        })
        .method(
            "list",
            |source, context, (cursor,): (Option<String>,)| async move {
                context.check()?;
                source.list(context.principal()?, cursor).await
            },
        )
        .method(
            "set_defaults",
            |source, context, (input,): (SetModelDefaults,)| async move {
                context.check()?;
                source.set_defaults(context.principal()?, input).await
            },
        )
        .method(
            "configure",
            |source, context, (input,): (ConfigureProvider,)| async move {
                context.check()?;
                let account = source.configure(context.principal()?, input).await?;
                context.check()?;
                Ok(account)
            },
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_provider_catalog_has_the_reviewed_choices_and_unique_methods() {
        let catalog = provider_catalog();
        assert_eq!(
            catalog
                .iter()
                .map(|choice| choice.label.as_str())
                .collect::<Vec<_>>(),
            ["ChatGPT", "Claude", "Kimi", "Local"]
        );
        let mut ids = std::collections::BTreeSet::new();
        for choice in catalog {
            assert!(ids.insert(choice.id));
            let mut methods = std::collections::BTreeSet::new();
            for method in choice.methods {
                assert!(methods.insert(method.id));
            }
        }
    }
}
