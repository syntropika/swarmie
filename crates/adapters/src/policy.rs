use mpai::*;
use serde_json::{Value, json};

pub fn codex_plan(profile: ExecutionProfile) -> Result<ExecutionPolicyPlan> {
    let native = match profile {
        ExecutionProfile::Ask => {
            json!({"approvalPolicy":"on-request","sandbox":"workspace-write","approvalsReviewer":"user"})
        }
        ExecutionProfile::Review => {
            json!({"approvalPolicy":"on-request","sandbox":"workspace-write","approvalsReviewer":"auto_review"})
        }
        ExecutionProfile::Full => {
            json!({"approvalPolicy":"never","sandbox":"danger-full-access","approvalsReviewer":"user"})
        }
        ExecutionProfile::NativeCustom => {
            return Err(Error::Unsupported(
                "select native settings for a custom policy".into(),
            ));
        }
    };
    Ok(ExecutionPolicyPlan {
        profile,
        native,
        application: SettingApplication::NextTurn,
    })
}
pub fn claude_plan(profile: ExecutionProfile) -> Result<ExecutionPolicyPlan> {
    if profile != ExecutionProfile::Ask {
        return Err(Error::Unsupported(match profile {
            ExecutionProfile::Review => "No qualified automatic reviewer is exposed by this adapter",
            ExecutionProfile::Full => "Bypassing prompts does not establish broad access while native sandbox and deny rules are unobserved",
            _ => "select native settings for a custom policy",
        }.into()));
    }
    Ok(ExecutionPolicyPlan {
        profile,
        native: json!({"permissionMode":"default"}),
        application: SettingApplication::NextTurn,
    })
}
pub fn codex_values(config: &Value) -> Value {
    json!({"approvalPolicy":config["approvalPolicy"],"sandbox":config["sandbox"],"approvalsReviewer":config["approvalsReviewer"]})
}
fn codex_profile(native: &Value, cwd: &std::path::Path) -> Option<ExecutionProfile> {
    let sandbox = native["sandbox"]
        .as_str()
        .or_else(|| native["sandbox"]["type"].as_str());
    let object = native["sandbox"].as_object();
    let restricted = matches!(sandbox, Some("workspace-write" | "workspaceWrite"))
        && object.is_none_or(|object| {
            object.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "type"
                        | "writableRoots"
                        | "networkAccess"
                        | "excludeTmpdirEnvVar"
                        | "excludeSlashTmp"
                )
            }) && object.get("writableRoots") == Some(&json!([cwd]))
                && object
                    .get("networkAccess")
                    .is_none_or(|value| value == &json!(false))
                && ["excludeTmpdirEnvVar", "excludeSlashTmp"]
                    .iter()
                    .all(|key| object.get(*key).is_none_or(|value| value == &json!(false)))
        });
    let broad = matches!(sandbox, Some("danger-full-access" | "dangerFullAccess"))
        && object.is_none_or(|object| object.len() == 1);
    if broad && native["approvalPolicy"] == "never" {
        Some(ExecutionProfile::Full)
    } else if restricted && native["approvalPolicy"] == "on-request" {
        match native["approvalsReviewer"].as_str() {
            Some("user") => Some(ExecutionProfile::Ask),
            Some("auto_review") => Some(ExecutionProfile::Review),
            _ => None,
        }
    } else {
        None
    }
}
pub fn codex_observation(
    native: Value,
    evidence: PolicyEvidence,
    version: Option<String>,
    cwd: &std::path::Path,
) -> PolicyObservation {
    PolicyObservation {
        profile:codex_profile(&native, cwd),native,evidence,native_version:version,
        coverage: vec!["Native command, file-change and offered permission approvals".into()],
        limitations: vec!["OS rights and administrative policies remain authoritative".into(), "Child-command sandbox controls do not establish confinement of hosted tools, model transport or external servers".into()],
    }
}
pub fn codex_report(
    config: &Value,
    observed: Option<PolicyObservation>,
    cwd: &std::path::Path,
) -> ExecutionPolicyReport {
    let mut choices =
        unavailable_profile_choices("The native session did not advertise the required control");
    for choice in &mut choices {
        let reviewer = observed
            .as_ref()
            .and_then(|observed| observed.native["approvalsReviewer"].as_str());
        let available = config["approvalPolicy"].is_string()
            && !config["sandbox"].is_null()
            && (choice.profile != ExecutionProfile::Review || reviewer.is_some());
        if available {
            choice.capability = true.into();
            choice.mapping = PolicyMapping::Equivalent;
        }
        choice.description = match choice.profile {
            ExecutionProfile::Ask => "Use native approval checks; ordinary permitted operations can proceed without a prompt",
            ExecutionProfile::Review => "Use the native approval reviewer for eligible requests; unresolved requests remain interactive",
            _ => "Request native no-sandbox, no-approval execution within OS and administrative permissions",
        }.into();
    }
    let planned = codex_values(config);
    let status = match &observed {
        Some(observation) if observation.native != planned => PolicyStatus::Staged,
        _ if planned["approvalPolicy"].is_null() || planned["sandbox"].is_null() => {
            PolicyStatus::Unknown
        }
        Some(observation) if observation.native == planned => match observation.evidence {
            PolicyEvidence::NativeReceipt => PolicyStatus::Effective,
            PolicyEvidence::NativeAcknowledgement => PolicyStatus::Acknowledged,
        },
        _ => PolicyStatus::Unknown,
    };
    ExecutionPolicyReport {
        choices,
        controlled_settings: vec![
            "approvalPolicy".into(),
            "sandbox".into(),
            "approvalsReviewer".into(),
        ],
        planned_profile: codex_profile(&planned, cwd),
        planned,
        observed,
        status,
    }
}
pub fn claude_observation(
    mode: Value,
    evidence: PolicyEvidence,
    version: Option<String>,
) -> PolicyObservation {
    PolicyObservation {
        profile:(mode=="default").then_some(ExecutionProfile::Ask), native:json!({"permissionMode":mode}), evidence,native_version:version,
        coverage:vec!["Native approval callbacks not already resolved by permission modes or rules".into()],
        limitations:vec!["Native sandbox, explicit deny rules and administrator restrictions are not observed by this adapter".into(),"A no-prompts permission mode does not establish unrestricted filesystem or network access".into()],
    }
}
pub fn claude_report(config: &Value, observed: Option<PolicyObservation>) -> ExecutionPolicyReport {
    let mut choices =
        unavailable_profile_choices("No qualified automatic reviewer is exposed by this adapter");
    choices[0].capability = true.into();
    choices[0].mapping = PolicyMapping::Equivalent;
    choices[0].description =
        "Use native approval checks; existing allowed operations can proceed without a prompt"
            .into();
    choices[2].mapping = PolicyMapping::Partial;
    choices[2].capability.reason=Some("Bypassing prompts does not establish broad access while native sandbox and deny rules are unobserved; native permissionMode remains configurable".into());
    let planned = json!({"permissionMode":config["permissionMode"]});
    let status = match &observed {
        Some(observation) if observation.native == planned => match observation.evidence {
            PolicyEvidence::NativeReceipt => PolicyStatus::Effective,
            PolicyEvidence::NativeAcknowledgement => PolicyStatus::Acknowledged,
        },
        _ => PolicyStatus::Unknown,
    };
    ExecutionPolicyReport {
        choices,
        controlled_settings: vec!["permissionMode".into()],
        planned_profile: (planned["permissionMode"] == "default").then_some(ExecutionProfile::Ask),
        planned,
        observed,
        status,
    }
}
