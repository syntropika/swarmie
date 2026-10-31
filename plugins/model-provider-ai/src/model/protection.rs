use super::AiModel;
use futures::{FutureExt, future::LocalBoxFuture};
use std::rc::Rc;
use swarmie_model_submission_protection::{Coverage, DecisionIntent, ModelAttempt, ModelGuard};
use swarmie_permissions::{ExecutionContexts, HumanDecision};
use swarmie_providers::{ModelResponse, OperationId, SubmissionProtection};
use tembra::{Code, Context, Error};
use zeroize::Zeroizing;

/// Native decision-card owner. An agent tool receives only the safe intent, never this authority.
pub trait ModelDecisions {
    fn request<'a>(
        &'a self,
        execution: &'a swarmie_permissions::ExecutionBinding,
        intent: &'a DecisionIntent,
    ) -> LocalBoxFuture<'a, tembra::Result<HumanDecision>>;
}

/// Explicit standalone/unprotected mode or one node-owned enabled policy for all connections.
#[derive(Clone)]
pub enum ModelProtection {
    Unprotected,
    Guarded {
        guard: ModelGuard,
        executions: ExecutionContexts,
        decisions: Rc<dyn ModelDecisions>,
    },
}

/// Native proof of final content admission for one physical model attempt.
/// Credentials receive this handle; operation IDs alone cannot stand in for it.
pub struct AdmittedAttempt {
    operation: OperationId,
    attempt: uuid::Uuid,
    destination: String,
    provider: String,
    root: u64,
    principal: String,
    execution: Option<swarmie_permissions::ExecutionBinding>,
    policy: Option<(ModelGuard, uuid::Uuid, uuid::Uuid)>,
    prepared: Option<super::PreparedCall>,
    protection: SubmissionProtection,
}

impl Drop for AdmittedAttempt {
    fn drop(&mut self) {
        if let Some(binding) = &self.execution {
            binding.close();
        }
    }
}

impl AdmittedAttempt {
    pub fn protection(&self) -> SubmissionProtection {
        self.protection
    }
    pub fn operation(&self) -> OperationId {
        self.operation
    }
    pub fn attempt(&self) -> uuid::Uuid {
        self.attempt
    }
    pub fn destination(&self) -> &str {
        &self.destination
    }
    pub fn provider(&self) -> &str {
        &self.provider
    }
    pub fn execution(&self) -> Option<&swarmie_permissions::ExecutionBinding> {
        self.execution.as_ref()
    }
    fn check(&self, context: &Context) -> tembra::Result<()> {
        context.check()?;
        if context.root != self.root
            || context.principal()? != self.principal
            || self.execution.as_ref().is_some_and(|e| !e.is_live())
        {
            return Err(Error::new(
                Code::Forbidden,
                "swarmie.models.admission",
                "model attempt owner ended or changed",
            ));
        }
        if let Some((guard, policy, permission)) = &self.policy {
            guard
                .check_version(*policy, *permission)
                .map_err(protection_error)?;
        }
        Ok(())
    }
}

impl ModelProtection {
    pub(super) async fn admit(
        &self,
        context: &Context,
        operation: OperationId,
        prepared: super::PreparedCall,
        provider: &str,
    ) -> tembra::Result<AdmittedAttempt> {
        context.check()?;
        let destination = canonical_destination(prepared.destination())?;
        let phase = ModelAttempt::new(operation.as_uuid());
        let mut admitted = AdmittedAttempt {
            operation,
            attempt: phase.attempt,
            destination: destination.clone(),
            provider: provider.into(),
            root: context.root,
            principal: context.principal()?,
            execution: None,
            policy: None,
            prepared: None,
            protection: SubmissionProtection::Unprotected,
        };
        let Self::Guarded {
            guard,
            executions,
            decisions,
        } = self
        else {
            admitted.prepared = Some(prepared);
            return Ok(admitted);
        };
        let binding = executions.resolve(context)?.scoped();
        admitted.execution = Some(binding.clone());
        let mut submission = guard
            .prepare(
                binding.clone(),
                phase,
                provider.into(),
                destination.clone(),
                prepared.content().to_vec(),
                Coverage::TextJson,
            )
            .map_err(protection_error)?;
        if !matches!(
            submission.intent().inspection,
            swarmie_model_submission_protection::Inspection::Clean
                | swarmie_model_submission_protection::Inspection::Unprotected
        ) {
            let decision = {
                let signal = context.signal();
                let pending = decisions.request(&binding, submission.intent()).fuse();
                let cancelled = signal.cancelled().fuse();
                futures::pin_mut!(pending, cancelled);
                futures::select_biased! {
                    _ = cancelled => { binding.close(); context.check()?; return Err(protection_error(swarmie_model_submission_protection::Error::Revoked)); },
                    result = pending => result?,
                }
            };
            context.check()?;
            submission.allow_once(decision).map_err(protection_error)?;
        }
        admitted.protection = match submission.intent().inspection {
            swarmie_model_submission_protection::Inspection::Clean => SubmissionProtection::Clean,
            swarmie_model_submission_protection::Inspection::Unprotected => {
                SubmissionProtection::Unprotected
            }
            swarmie_model_submission_protection::Inspection::Detected { .. } => {
                SubmissionProtection::HumanOverride
            }
            _ => SubmissionProtection::CannotInspectOverride,
        };
        admitted.policy = Some((
            guard.clone(),
            submission.intent().policy,
            submission.intent().permission,
        ));
        submission
            .dispatch(|route, bytes| {
                if route != destination || bytes != prepared.content() {
                    return Err(Error::new(
                        Code::Conflict,
                        "swarmie.models.protection",
                        "final content or destination changed",
                    ));
                }
                Ok(())
            })
            .map_err(protection_error)??;
        admitted.check(context)?;
        admitted.prepared = Some(prepared);
        Ok(admitted)
    }
}

fn protection_error(_: swarmie_model_submission_protection::Error) -> Error {
    Error::new(
        Code::Forbidden,
        "swarmie.models.protection",
        "model submission authority ended or changed",
    )
}

fn canonical_destination(destination: &str) -> tembra::Result<String> {
    let mut url = reqwest::Url::parse(destination)
        .map_err(|_| Error::invalid("swarmie.models.destination", "invalid model destination"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::invalid(
            "swarmie.models.destination",
            "userinfo is not supported",
        ));
    }
    url.set_fragment(None);
    Ok(url.to_string())
}

/// Private transport credential acquired for the final attempt. No serialization or diagnostics.
pub struct ModelCredential {
    value: Zeroizing<String>,
    marker: String,
}

impl ModelCredential {
    pub fn new(value: String, marker: String) -> tembra::Result<Self> {
        let value = Zeroizing::new(value);
        if value.is_empty()
            || value.len() > 65_536
            || value.contains(['\r', '\n', '\0'])
            || marker.is_empty()
            || marker.contains(value.as_str())
        {
            return Err(Error::invalid(
                "swarmie.models.credentials",
                "invalid private credential",
            ));
        }
        Ok(Self { value, marker })
    }

    pub fn mask(&self, text: &str) -> String {
        text.replace(self.value.as_str(), &self.marker)
    }

    pub(super) fn mask_response(&self, output: &mut ModelResponse) {
        // Only provider-controlled content is eligible for masking. Fixed protocol field names,
        // numeric usage and the locally issued inspection label cannot be credential reflections.
        output.message.content = self.mask(&output.message.content);
        if let Some(record) = &mut output.message.provider {
            let original = record.clone();
            self.mask_json(record);
            if *record != original {
                // Altered native signatures or opaque fields cannot authorize a
                // continuation. Retain the sanitized visible result and refuse
                // subsequent preparation instead of replaying changed state.
                *record = serde_json::json!({"unavailable":"credential_reflection"});
            }
        }
        if output.message.tool_calls.iter().any(|call| {
            call.id.as_str().contains(self.value.as_str())
                || call.name.as_str().contains(self.value.as_str())
        }) {
            output.message.tool_calls.clear();
            output
                .message
                .content
                .push_str("\n[Tool calls withheld: a credential was reflected in an identifier.]");
        } else {
            for call in &mut output.message.tool_calls {
                self.mask_json(&mut call.arguments);
            }
        }
    }

    fn preview(&self, text: &str) -> String {
        let value = self.value.as_str();
        let last_complete = text.rfind(value).map_or(0, |i| i + value.len());
        let max_partial = (value.len() - 1).min(text.len() - last_complete);
        let held = (1..=max_partial)
            .rev()
            .find(|&n| value.is_char_boundary(n) && text.ends_with(&value[..n]))
            .unwrap_or(0);
        self.mask(&text[..text.len() - held])
    }

    fn mask_json(&self, value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(text) => *text = self.mask(text),
            serde_json::Value::Array(values) => {
                for v in values {
                    self.mask_json(v);
                }
            }
            serde_json::Value::Object(values) => {
                let prior = std::mem::take(values);
                for (key, mut value) in prior {
                    self.mask_json(&mut value);
                    values.insert(self.mask(&key), value);
                }
            }
            _ => {}
        }
    }
}

pub(super) struct Authentication {
    credential: Option<ModelCredential>,
    _admitted: AdmittedAttempt,
}

impl Authentication {
    pub(super) fn protection(&self) -> SubmissionProtection {
        self._admitted.protection()
    }
    fn take_prepared(&mut self, context: &Context) -> tembra::Result<super::PreparedCall> {
        self._admitted.check(context)?;
        self._admitted
            .prepared
            .take()
            .ok_or_else(|| protection_error(swarmie_model_submission_protection::Error::Revoked))
    }
    pub(super) fn dispatch(
        &mut self,
        context: &Context,
        stop: Option<swarmie_ai::StopHandle>,
    ) -> tembra::Result<swarmie_ai::Execution> {
        let prepared = self.take_prepared(context)?;
        let admitted = if let Some(credential) = &self.credential {
            prepared
                .admitted
                .authenticate(
                    swarmie_ai::Credentials::new(
                        prepared.description.identity,
                        swarmie_ai::AuthProfile::ApiKey,
                        Some(credential.value.clone()),
                        None,
                    )
                    .map_err(crate::error::upstream)?,
                )
                .map_err(crate::error::upstream)?
        } else {
            prepared.admitted
        };
        self._admitted.check(context)?;
        if self._admitted.execution.is_none() && self._admitted.policy.is_none() {
            let runtime = tokio::runtime::Handle::try_current().map_err(|_| {
                crate::error::invalid(
                    crate::error::Reason::Runtime,
                    "model runtime is unavailable",
                )
            })?;
            return admitted
                .dispatch(&runtime, move |_| {
                    if stop.as_ref().is_none_or(|stop| !stop.is_cancelled()) {
                        Ok(())
                    } else {
                        Err(swarmie_ai::Error {
                            failure: swarmie_ai::Failure::Stopped,
                            effect: swarmie_ai::Effect::NotDispatched,
                        })
                    }
                })
                .map_err(crate::error::upstream);
        }
        let context = context.clone();
        let execution = self._admitted.execution.clone();
        let policy = self._admitted.policy.clone();
        admitted
            .dispatch_local(&super::Executor, move |_| {
                let valid = stop.as_ref().is_none_or(|stop| !stop.is_cancelled())
                    && context.check().is_ok()
                    && execution.as_ref().is_none_or(|binding| binding.is_live())
                    && policy
                        .as_ref()
                        .is_none_or(|(guard, generation, permission)| {
                            guard.check_version(*generation, *permission).is_ok()
                        });
                if valid {
                    Ok(())
                } else {
                    Err(swarmie_ai::Error {
                        failure: swarmie_ai::Failure::Admission,
                        effect: swarmie_ai::Effect::NotDispatched,
                    })
                }
            })
            .map_err(crate::error::upstream)
    }
    pub(super) fn mask_response(&self, output: &mut ModelResponse) {
        if let Some(credential) = &self.credential {
            credential.mask_response(output);
        }
    }
    pub(super) fn preview(&self, content: &str) -> String {
        self.credential
            .as_ref()
            .map_or_else(|| content.to_owned(), |c| c.preview(content))
    }
}

impl AiModel {
    pub(super) async fn authentication(
        &self,
        context: &Context,
        admitted: AdmittedAttempt,
    ) -> tembra::Result<Authentication> {
        admitted.check(context)?;
        let credential = if let Some(key) = &self.connection.credential_key {
            let signal = context.signal();
            let pending = self.credentials.acquire(context, &admitted, key).fuse();
            let cancelled = signal.cancelled().fuse();
            futures::pin_mut!(pending, cancelled);
            Some(futures::select_biased! {
                _=cancelled=>{context.check()?;return Err(protection_error(swarmie_model_submission_protection::Error::Revoked));},
                result=pending=>result?,
            })
        } else {
            None
        };
        admitted.check(context)?;
        Ok(Authentication {
            credential,
            _admitted: admitted,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reflected_credential_retires_native_continuation_without_retaining_the_secret() {
        let credential = ModelCredential::new("private-api-key".into(), "[secret]".into()).unwrap();
        let mut response = ModelResponse {
            protection: SubmissionProtection::Clean,
            message: swarmie_providers::AssistantMessage {
                provider: Some(
                    serde_json::json!({"choice":[{"type":"reasoning","native":{"item":{"signature":"private-api-key"}}}]}),
                ),
                content: "Visible answer".into(),
                tool_calls: vec![],
            },
            usage: swarmie_providers::Usage {
                input_tokens: 10,
                output_tokens: 5,
            },
        };
        credential.mask_response(&mut response);
        assert_eq!(response.message.content, "Visible answer");
        assert_eq!(response.usage.input_tokens, 10);
        assert_eq!(
            response.message.provider,
            Some(serde_json::json!({"unavailable":"credential_reflection"}))
        );
        assert!(
            !serde_json::to_string(&response)
                .unwrap()
                .contains("private-api-key")
        );
    }

    #[test]
    fn fixed_protocol_fields_and_protection_labels_are_not_provider_reflections() {
        for value in ["clean", "message", "usage"] {
            let credential = ModelCredential::new(value.into(), "[secret]".into()).unwrap();
            let mut response = ModelResponse {
                protection: SubmissionProtection::Clean,
                message: swarmie_providers::AssistantMessage {
                    provider: None,
                    content: value.into(),
                    tool_calls: vec![],
                },
                usage: swarmie_providers::Usage::default(),
            };
            credential.mask_response(&mut response);
            assert_eq!(response.protection, SubmissionProtection::Clean);
            assert_eq!(response.message.content, "[secret]");
        }
    }
}
