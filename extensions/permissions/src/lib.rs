//! Optional policies for interactive requests surfaced by harness adapters.
//! This extension does not alter native execution modes or provide a sandbox.
use async_trait::async_trait;
use mpai::{
    Error, ExtensionCommand, PermissionRequest, PermissionResponse, Result, RuntimeExtension,
    SessionSnapshot,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    str::FromStr,
    sync::atomic::{AtomicU8, Ordering},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum PolicyMode {
    #[default]
    Ask,
    AutoApprove,
    AcceptEdits,
    Deny,
}
impl FromStr for PolicyMode {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "ask" => Ok(Self::Ask),
            "auto_approve" | "auto-approve" => Ok(Self::AutoApprove),
            "accept_edits" | "accept-edits" => Ok(Self::AcceptEdits),
            "deny" => Ok(Self::Deny),
            _ => Err(Error::Invalid("unknown permission policy mode".into())),
        }
    }
}

/// The selected mode applies to subsequent permission requests in this runtime.
/// Existing pending requests are left for the client to resolve.
#[derive(Default)]
pub struct PermissionPolicyExtension {
    mode: AtomicU8,
}
impl PermissionPolicyExtension {
    pub fn new(mode: PolicyMode) -> Self {
        Self {
            mode: AtomicU8::new(mode as u8),
        }
    }
    pub fn mode(&self) -> PolicyMode {
        match self.mode.load(Ordering::Relaxed) {
            1 => PolicyMode::AutoApprove,
            2 => PolicyMode::AcceptEdits,
            3 => PolicyMode::Deny,
            _ => PolicyMode::Ask,
        }
    }
    pub fn set_mode(&self, mode: PolicyMode) {
        self.mode.store(mode as u8, Ordering::Relaxed);
    }
    pub fn decide(&self, request: &PermissionRequest) -> Option<PermissionResponse> {
        let edit = matches!(
            request.kind.as_str(),
            "file" | "edit" | "file_change" | "file_write" | "patch"
        );
        let operation = edit
            || matches!(
                request.kind.as_str(),
                "command" | "execute" | "read" | "search" | "fetch" | "tool" | "mcp"
            );
        // Unknown kinds include questions, plans, and broad permission grants.
        // The adapter's normalized kind is the authority; titles are never guessed.
        if !operation {
            return None;
        }
        let mode = self.mode();
        let option = match mode {
            PolicyMode::Ask => return None,
            PolicyMode::AcceptEdits if !edit => return None,
            PolicyMode::Deny => request.options.iter().find(|option| {
                matches!(option.kind.as_str(), "deny" | "deny_once") && !option.id.is_empty()
            }),
            PolicyMode::AutoApprove | PolicyMode::AcceptEdits => {
                // A generic approve kind alone does not establish one-shot scope.
                // Unknown opaque IDs and session-wide options remain interactive.
                request.options.iter().find(|option| {
                    (option.kind == "approve_once" && !option.id.is_empty())
                        || (option.kind == "approve"
                            && matches!(
                                option.id.as_str(),
                                "approve"
                                    | "once"
                                    | "allow_once"
                                    | "approve_once"
                                    | "accept"
                                    | "accept_once"
                            ))
                })
            }
        }?;
        if request
            .options
            .iter()
            .filter(|candidate| candidate.id == option.id)
            .count()
            != 1
        {
            return None;
        }
        Some(PermissionResponse {
            answers: Default::default(),
            id: request.id.clone(),
            option: option.id.clone(),
            data: Value::Null,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetPolicy {
    mode: PolicyMode,
}
#[async_trait]
impl RuntimeExtension for PermissionPolicyExtension {
    fn id(&self) -> &str {
        "permissions"
    }
    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            ExtensionCommand {
                name: "policy.get".into(),
                title: "Permission policy".into(),
                mutates: false,
            },
            ExtensionCommand {
                name: "policy.set".into(),
                title: "Change permission policy".into(),
                mutates: true,
            },
        ]
    }
    async fn invoke(&self, command: &str, params: Value) -> Result<Value> {
        match command {
            "policy.get" => Ok(json!({ "mode": self.mode() })),
            "policy.set" => {
                let selection: SetPolicy = serde_json::from_value(params)
                    .map_err(|_| Error::Invalid("provide a valid permission policy mode".into()))?;
                self.set_mode(selection.mode);
                Ok(json!({ "mode": self.mode() }))
            }
            _ => Err(Error::Unsupported(format!("extension command {command}"))),
        }
    }
    async fn on_permission(
        &self,
        _session: &SessionSnapshot,
        request: &PermissionRequest,
    ) -> Result<Option<PermissionResponse>> {
        Ok(self.decide(request))
    }
}
