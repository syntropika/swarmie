use crate::catalog::{ModelSpec, ReasoningOption};

/// A descriptive model requirement. It does not represent adapter qualification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelFeature<'a> {
    Tools,
    StructuredOutput,
    Reasoning,
    Temperature,
    Attachments,
    Input(&'a str),
    Output(&'a str),
    ReasoningEffort(&'a str),
    ReasoningBudget(u64),
    DisableReasoning,
    CacheRetention(&'a str),
    Prefill,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelFeatureStatus {
    Unknown,
    Unsupported,
}

#[derive(Debug, thiserror::Error)]
#[error("model requirement {feature} is {status:?}")]
pub struct ModelFeatureError {
    pub feature: String,
    pub status: ModelFeatureStatus,
}

impl ModelSpec {
    /// Reported support: None is unknown, never an implicit true or false.
    /// Check source evidence separately and retain route qualification.
    pub fn supports(&self, feature: ModelFeature<'_>) -> Option<bool> {
        match feature {
            ModelFeature::Tools => self.tools,
            ModelFeature::StructuredOutput => self.structured_output,
            ModelFeature::Reasoning => self.reasoning,
            ModelFeature::Temperature => self.temperature,
            ModelFeature::Attachments => self.attachment,
            ModelFeature::Input(name) => Some(
                self.modalities
                    .as_ref()?
                    .input
                    .as_ref()?
                    .iter()
                    .any(|v| v == name),
            ),
            ModelFeature::Output(name) => Some(
                self.modalities
                    .as_ref()?
                    .output
                    .as_ref()?
                    .iter()
                    .any(|v| v == name),
            ),
            ModelFeature::CacheRetention(name) => Some(
                self.compatibility
                    .as_ref()?
                    .cache_retention
                    .as_ref()?
                    .iter()
                    .any(|v| v == name),
            ),
            ModelFeature::Prefill => self.compatibility.as_ref()?.prefill,
            ModelFeature::ReasoningEffort(name) => {
                if self.reasoning == Some(false) || self.no_reasoning_controls() {
                    return Some(false);
                }
                let mut known = false;
                for option in self.reasoning_options.as_ref()? {
                    if option.kind == "effort"
                        && let Some(values) = &option.values
                    {
                        known = true;
                        if values.iter().any(|value| value == name) {
                            return Some(true);
                        }
                    }
                }
                known.then_some(false)
            }
            ModelFeature::ReasoningBudget(tokens) => {
                if self.reasoning == Some(false) || self.no_reasoning_controls() {
                    return Some(false);
                }
                let budgets: Vec<_> = self
                    .reasoning_options
                    .as_ref()?
                    .iter()
                    .filter(|option| option.kind == "budget_tokens")
                    .collect();
                if budgets.is_empty() {
                    return None;
                }
                let results: Vec<_> = budgets
                    .into_iter()
                    .map(|option| in_budget(option, tokens))
                    .collect();
                if results.contains(&Some(true)) {
                    Some(true)
                } else if results.contains(&None) {
                    None
                } else {
                    Some(false)
                }
            }
            ModelFeature::DisableReasoning => {
                if self.reasoning == Some(false) || self.no_reasoning_controls() {
                    return Some(false);
                }
                let options = self.reasoning_options.as_ref()?;
                options
                    .iter()
                    .any(|option| {
                        option.kind == "toggle"
                            || (option.kind == "effort"
                                && option
                                    .values
                                    .as_ref()
                                    .is_some_and(|values| values.iter().any(|v| v == "none")))
                    })
                    .then_some(true)
            }
        }
    }

    /// Refuse unavailable descriptive requirements. Success is not permission
    /// to dispatch: the exact route still needs a qualified operational snapshot.
    pub fn require(&self, features: &[ModelFeature<'_>]) -> Result<(), ModelFeatureError> {
        for feature in features {
            match self.supports(*feature) {
                Some(true) => {}
                status => {
                    return Err(ModelFeatureError {
                        feature: format!("{feature:?}"),
                        status: if status.is_none() {
                            ModelFeatureStatus::Unknown
                        } else {
                            ModelFeatureStatus::Unsupported
                        },
                    });
                }
            }
        }
        Ok(())
    }

    fn no_reasoning_controls(&self) -> bool {
        self.compatibility
            .as_ref()
            .and_then(|value| value.reasoning_control.as_deref())
            == Some("none")
    }
}

fn in_budget(option: &ReasoningOption, tokens: u64) -> Option<bool> {
    // Negative provider sentinels are retained, but do not establish a token bound.
    let min = option.min.and_then(|value| u64::try_from(value).ok());
    let max = option.max.and_then(|value| u64::try_from(value).ok());
    if min.is_some_and(|min| tokens < min) || max.is_some_and(|max| tokens > max) {
        Some(false)
    } else if min.is_some() && max.is_some() {
        Some(true)
    } else {
        None
    }
}
