use crate::{AccountRecord, ModelSelection, settings::ModelInfo};

/// Intent remains with the account owner until it resolves defaults. A host
/// need not inspect saved defaults or choose an auxiliary model on its behalf.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelChoice {
    Default,
    Auxiliary,
    Exact(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectModel {
    pub account_id: String,
    pub model: ModelChoice,
    /// None uses an applicable configured default. An empty explicit value
    /// clears reasoning; otherwise the exact supported setting is required.
    pub reasoning: Option<String>,
}

pub(super) fn resolve(
    account: &AccountRecord,
    request: &SelectModel,
) -> tembra::Result<ModelSelection> {
    let account = &account.account;
    let primary = account
        .defaults
        .model
        .as_deref()
        .or_else(|| account.models.first().map(|model| model.id.as_str()))
        .ok_or_else(|| super::failure("this account has no available models"))?;
    let id = match &request.model {
        ModelChoice::Default => primary,
        ModelChoice::Auxiliary => account
            .defaults
            .auxiliary_model
            .as_deref()
            .unwrap_or(primary),
        ModelChoice::Exact(id) => id,
    };
    let model = account
        .models
        .iter()
        .find(|model| model.id == id)
        .ok_or_else(|| super::failure("the model is not configured for this account"))?;
    let requested = request.reasoning.as_deref().or_else(|| {
        (id == primary)
            .then_some(account.defaults.effort.as_deref())
            .flatten()
    });
    Ok(ModelSelection {
        account_id: account.id.clone(),
        model_id: id.into(),
        reasoning: reasoning(model, requested)?,
    })
}

pub(super) fn reasoning(
    model: &ModelInfo,
    requested: Option<&str>,
) -> tembra::Result<Option<String>> {
    match requested.filter(|value| !value.is_empty()) {
        None => Ok(None),
        Some(value) => model
            .efforts
            .iter()
            .find(|allowed| allowed.eq_ignore_ascii_case(value))
            .cloned()
            .map(Some)
            .ok_or_else(|| super::failure("the reasoning effort is not supported by this model")),
    }
}
