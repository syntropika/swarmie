use crate::{
    Capability, Error, NativeSessionRef, Result, SettingApplication, SettingChoice,
    SettingDescriptor, SettingType,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const EXECUTION_PROFILE_SETTING: &str = "executionProfile";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProfile {
    Ask,
    Review,
    Full,
    NativeCustom,
}
impl ExecutionProfile {
    pub fn id(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Review => "review",
            Self::Full => "full",
            Self::NativeCustom => "native_custom",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PolicyMapping {
    Equivalent,
    Partial,
    Unsupported,
    Unknown,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PolicyStatus {
    Staged,
    Acknowledged,
    Effective,
    Rejected,
    Partial,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PolicyEvidence {
    NativeReceipt,
    NativeAcknowledgement,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionProfileChoice {
    pub profile: ExecutionProfile,
    pub label: String,
    pub description: String,
    pub capability: Capability,
    pub mapping: PolicyMapping,
    pub application: SettingApplication,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PolicyObservation {
    pub profile: Option<ExecutionProfile>,
    pub native: Value,
    pub evidence: PolicyEvidence,
    pub coverage: Vec<String>,
    pub limitations: Vec<String>,
    pub native_version: Option<String>,
}

/// A requested selection and its native observation have independent lifetimes.
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPolicy {
    pub revision: u64,
    pub requested: Option<ExecutionProfile>,
    pub status: PolicyStatus,
    pub planned: Value,
    pub planned_profile: Option<ExecutionProfile>,
    pub observed: Option<PolicyObservation>,
    pub choices: Vec<ExecutionProfileChoice>,
    pub controlled_settings: Vec<String>,
    pub dispatch_blocked: bool,
    pub reason: Option<String>,
    pub binding_id: Option<String>,
    pub native_session: Option<NativeSessionRef>,
}

/// The adapter reports only native facts; retained user intent belongs to the host.
#[derive(Clone, Debug)]
pub struct ExecutionPolicyReport {
    pub choices: Vec<ExecutionProfileChoice>,
    pub controlled_settings: Vec<String>,
    pub planned: Value,
    pub planned_profile: Option<ExecutionProfile>,
    pub observed: Option<PolicyObservation>,
    pub status: PolicyStatus,
}
impl Default for ExecutionPolicyReport {
    fn default() -> Self {
        Self {
            choices: unavailable_profile_choices(
                "No common execution-policy translation is implemented for this route",
            ),
            controlled_settings: vec![],
            planned: Value::Null,
            planned_profile: None,
            observed: None,
            status: PolicyStatus::Unknown,
        }
    }
}
#[derive(Clone, Debug)]
pub struct ExecutionPolicyPlan {
    pub profile: ExecutionProfile,
    pub native: Value,
    pub application: SettingApplication,
}

pub fn requested_execution_profile(config: &Value) -> Result<Option<ExecutionProfile>> {
    config
        .get(EXECUTION_PROFILE_SETTING)
        .map(|value| {
            serde_json::from_value(value.clone())
                .map_err(|_| Error::Invalid("invalid executionProfile".into()))
        })
        .transpose()
}

pub fn unavailable_profile_choices(reason: &str) -> Vec<ExecutionProfileChoice> {
    [
        (
            ExecutionProfile::Ask,
            "Ask for approval",
            "Use supported native approval checks",
        ),
        (
            ExecutionProfile::Review,
            "Approve for me",
            "Review supported operation requests automatically",
        ),
        (
            ExecutionProfile::Full,
            "Full access",
            "Use the broad native execution route within system permissions",
        ),
    ]
    .into_iter()
    .map(|(profile, label, description)| ExecutionProfileChoice {
        profile,
        label: label.into(),
        description: description.into(),
        capability: Capability {
            reason: Some(reason.into()),
            ..Capability::default()
        },
        mapping: PolicyMapping::Unsupported,
        application: SettingApplication::NextTurn,
    })
    .collect()
}
impl ExecutionPolicyReport {
    pub fn require(&self, profile: ExecutionProfile) -> Result<()> {
        let choice = self
            .choices
            .iter()
            .find(|choice| choice.profile == profile)
            .ok_or_else(|| Error::Unsupported("execution profile is not advertised".into()))?;
        choice.capability.require(profile.id())?;
        if choice.mapping != PolicyMapping::Equivalent {
            return Err(Error::Unsupported(format!(
                "{} has no equivalent native translation",
                profile.id()
            )));
        }
        Ok(())
    }
}
impl ExecutionPolicy {
    pub fn observe(&mut self, report: ExecutionPolicyReport) {
        self.choices = report.choices;
        self.controlled_settings = report.controlled_settings;
        let intent_mismatch = self.requested.is_some()
            && matches!(
                report.status,
                PolicyStatus::Effective | PolicyStatus::Acknowledged
            )
            && report
                .observed
                .as_ref()
                .is_some_and(|observation| observation.profile != self.requested);
        if intent_mismatch && !self.dispatch_blocked {
            self.dispatch_blocked = true;
            self.status = PolicyStatus::Unknown;
            self.reason = Some(
                "The observed native policy no longer matches the requested execution profile"
                    .into(),
            );
        }
        if !self.dispatch_blocked {
            self.status = report.status;
            self.planned = report.planned;
            self.planned_profile = report.planned_profile;
            self.observed = report.observed;
            self.reason = None;
        } else if report.observed.is_some() {
            // Readback must not silently release an unresolved native mutation.
            self.observed = report.observed;
        }
    }
    pub fn descriptor(&self) -> SettingDescriptor {
        let selected = self.requested.or_else(|| {
            if self.status == PolicyStatus::Staged {
                self.planned_profile
            } else {
                self.observed
                    .as_ref()
                    .and_then(|observation| observation.profile)
            }
        });
        SettingDescriptor {
            id: EXECUTION_PROFILE_SETTING.into(),
            label: "Execution permissions".into(),
            kind: SettingType::Select,
            value: selected.map(|profile| json!(profile)),
            choices: self
                .choices
                .iter()
                .map(|choice| SettingChoice {
                    id: choice.profile.id().into(),
                    label: choice.label.clone(),
                    description: Some(choice.description.clone()),
                    metadata: std::collections::BTreeMap::from([
                        ("capability".into(), json!(choice.capability)),
                        ("mapping".into(), json!(choice.mapping)),
                    ]),
                })
                .collect(),
            application: SettingApplication::NextTurn,
            description: Some(
                "The selected future configuration is separate from native observations".into(),
            ),
            metadata: std::collections::BTreeMap::from([("policy".into(), json!(self))]),
        }
    }
}
