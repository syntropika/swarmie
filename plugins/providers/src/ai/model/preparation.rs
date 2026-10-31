use super::AiModel;
use crate::ai::conversion;
use crate::ai::{
    Provider,
    error::{Reason, invalid, upstream},
};
use crate::{
    Component, ComponentKind, CountPrecision, Measurement, ModelRequest, OperationId,
    OperationalModel, OutputReservation, PreparedDescription, PreparedModelRequest, Progress,
};
use futures::FutureExt;
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use tembra::Context;

impl AiModel {
    pub(super) async fn final_request(
        &self,
        request: &ModelRequest,
        streaming: bool,
        operation: OperationId,
    ) -> tembra::Result<super::PreparedCall> {
        let metadata = self.metadata.as_ref().cloned().ok_or_else(|| {
            tembra::Error::new(
                tembra::Code::Unavailable,
                "swarmie.models.metadata",
                "qualified operational metadata is required before model preparation",
            )
        })?;
        let mut input = conversion::request(request)?;
        input.model = Some(self.model.as_str().into());
        input.max_tokens = Some(
            metadata
                .output()
                .known()
                .ok_or_else(|| invalid(Reason::Configuration, "output limit is unavailable"))?
                .tokens
                .get(),
        );
        input.options.reasoning = self.reasoning;
        let model = self
            .route
            .as_ref()
            .ok_or_else(|| invalid(Reason::Configuration, "prepared model route is unavailable"))?;
        let prepared = model
            .prepare(
                operation.as_uuid(),
                input,
                if streaming {
                    swarmie_ai::WireMode::Stream
                } else {
                    swarmie_ai::WireMode::Complete
                },
            )
            .map_err(upstream)?;
        let description = prepared.description().clone();
        let mut content = Vec::new();
        let admitted = prepared
            .admit(|_, body, _, _| {
                content = body.to_vec();
                Ok(())
            })
            .map_err(upstream)?;
        self.budget(content.len(), true)?;
        Ok(super::PreparedCall {
            admitted,
            description,
            content,
        })
    }

    pub(super) async fn prepare_native<'a>(
        &'a self,
        context: &'a Context,
        operation: OperationId,
        request: ModelRequest,
        progress: Option<Progress>,
    ) -> tembra::Result<PreparedModelRequest<'a>> {
        context.check()?;
        let metadata = self.metadata.as_ref().cloned().ok_or_else(|| {
            tembra::Error::new(
                tembra::Code::Unavailable,
                "swarmie.models.metadata",
                "qualified operational metadata is required before context preparation",
            )
        })?;
        let streaming = progress.is_some();
        if (!request.tools.is_empty()
            && !matches!(
                metadata.tools(),
                swarmie_ai::catalog::Capability::Supported { .. }
            ))
            || (streaming
                && !matches!(
                    metadata.streaming(),
                    swarmie_ai::catalog::Capability::Supported { .. }
                ))
        {
            return Err(tembra::Error::new(
                tembra::Code::Unavailable,
                "swarmie.models.capability",
                "required model request capability is not qualified",
            ));
        }
        let prepared = self.final_request(&request, streaming, operation).await?;
        let output_reserve = metadata
            .output()
            .known()
            .ok_or_else(|| invalid(Reason::Configuration, "output reserve is unavailable"))?
            .tokens
            .get();
        check_sent_cap(self.connection.provider, prepared.content(), output_reserve)?;
        context.check()?;
        let measurement = measure(prepared.content())?;
        let mut digest = Sha256::new();
        digest.update(metadata.generation().as_bytes());
        digest.update([u8::from(streaming)]);
        digest.update(prepared.destination().as_bytes());
        digest.update([0]);
        digest.update(prepared.content());
        let description = PreparedDescription {
            operation,
            model: OperationalModel::from_snapshot(&metadata)?,
            fingerprint: digest.finalize().into(),
            streaming,
            output_reserve,
            output_reservation: OutputReservation::SentCap,
            measurement,
        };
        PreparedModelRequest::new(description, move || {
            async move {
                let result = match progress {
                    Some(progress) => {
                        self.stream_prepared(context, operation, &request, &progress, prepared)
                            .await
                    }
                    None => {
                        self.complete_prepared(context, operation, &request, prepared)
                            .await
                    }
                };
                // The exact metadata generation remains retained with the private body
                // throughout admission, credential resolution and physical settlement.
                drop(metadata);
                result
            }
            .boxed_local()
        })
    }
}

/// Read only cap fields from the final bytes. Serde skips the private payload
/// without copying its messages or tools into another JSON tree.
fn check_sent_cap(provider: Provider, bytes: &[u8], expected: u64) -> tembra::Result<()> {
    #[derive(Default, serde::Deserialize)]
    #[serde(default)]
    struct Options {
        num_predict: Option<u64>,
    }
    #[derive(Default, serde::Deserialize)]
    #[serde(default, rename_all = "camelCase")]
    struct Generation {
        max_output_tokens: Option<u64>,
    }
    #[derive(serde::Deserialize)]
    struct Caps {
        max_tokens: Option<u64>,
        max_completion_tokens: Option<u64>,
        max_output_tokens: Option<u64>,
        #[serde(default)]
        options: Options,
        #[serde(default, rename = "generationConfig")]
        generation: Generation,
    }
    let unavailable = || {
        tembra::Error::new(
            tembra::Code::Unavailable,
            "swarmie.models.output_reservation",
            "the final provider request does not encode the reserved output cap",
        )
    };
    let cap: Caps = serde_json::from_slice(bytes).map_err(|_| unavailable())?;
    let actual = match provider {
        Provider::OpenAi => match (cap.max_tokens, cap.max_completion_tokens) {
            (Some(value), None) | (None, Some(value)) => Some(value),
            _ => None,
        },
        Provider::DeepSeek | Provider::Kimi if cap.max_completion_tokens.is_none() => {
            cap.max_tokens
        }
        Provider::DeepSeek | Provider::Kimi => None,
        Provider::OpenAiResponses => cap.max_output_tokens,
        Provider::Anthropic => cap.max_tokens,
        Provider::Gemini => cap.generation.max_output_tokens,
        Provider::Ollama => cap.options.num_predict,
    };
    if expected == 0 || actual != Some(expected) {
        return Err(unavailable());
    }
    Ok(())
}

/// This is explicitly an estimate of model tokens, not provider usage or an exact
/// tokenizer. Attribution uses disjoint final-JSON byte shares. It never exposes
/// payload strings, counts tool definitions twice or changes the prepared bytes.
fn measure(bytes: &[u8]) -> tembra::Result<Measurement> {
    let fields: BTreeMap<&str, &RawValue> = serde_json::from_slice(bytes).map_err(|_| {
        invalid(
            Reason::Request,
            "final provider payload is not an inspectable object",
        )
    })?;
    let mut sizes = BTreeMap::<ComponentKind, u64>::new();
    for (key, value) in fields {
        let kind = match key {
            "system" | "instructions" | "systemInstruction" | "system_instruction" => {
                Some(ComponentKind::Instructions)
            }
            "tools" | "functions" => Some(ComponentKind::Tools),
            "messages" | "input" => {
                if let Ok(records) = serde_json::from_str::<Vec<&RawValue>>(value.get()) {
                    for record in records {
                        let role = serde_json::from_str::<BTreeMap<&str, &RawValue>>(record.get())
                            .ok()
                            .and_then(|parts| {
                                parts
                                    .get("role")
                                    .and_then(|role| serde_json::from_str::<&str>(role.get()).ok())
                            });
                        let kind = if matches!(role, Some("system" | "developer")) {
                            ComponentKind::Instructions
                        } else {
                            ComponentKind::Conversation
                        };
                        *sizes.entry(kind).or_default() += record.get().len() as u64;
                    }
                    None
                } else {
                    Some(ComponentKind::Conversation)
                }
            }
            "contents" => Some(ComponentKind::Conversation),
            _ => None,
        };
        if let Some(kind) = kind {
            *sizes.entry(kind).or_default() += value.get().len() as u64;
        }
    }
    let total = bytes.len() as u64;
    let classified = sizes
        .values()
        .try_fold(0_u64, |sum, n| sum.checked_add(*n))
        .ok_or_else(|| invalid(Reason::Request, "final request attribution overflow"))?;
    let other = total
        .checked_sub(classified)
        .ok_or_else(|| invalid(Reason::Request, "final request attribution overlaps"))?;
    sizes.insert(ComponentKind::Other, other);
    let tokens = total.div_ceil(4);
    let mut assigned = 0_u64;
    let mut components = Vec::new();
    for (kind, bytes) in sizes
        .iter()
        .filter(|(kind, _)| **kind != ComponentKind::Other)
    {
        let share = ((u128::from(tokens) * u128::from(*bytes)) / u128::from(total.max(1))) as u64;
        assigned += share;
        components.push(Component {
            kind: *kind,
            tokens: share,
            bytes: *bytes,
        });
    }
    components.push(Component {
        kind: ComponentKind::Other,
        tokens: tokens - assigned,
        bytes: other,
    });
    let result = Measurement {
        input_tokens: tokens,
        final_payload_bytes: total,
        precision: CountPrecision::Estimated,
        method: "final-json-utf8-bytes/4; disjoint-byte-share attribution".into(),
        components,
    };
    result.validate()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sent_cap_requires_the_exact_final_protocol_field_on_each_route() {
        for (provider, body) in [
            (Provider::OpenAi, serde_json::json!({"max_tokens":100})),
            (
                Provider::OpenAi,
                serde_json::json!({"max_completion_tokens":100}),
            ),
            (Provider::DeepSeek, serde_json::json!({"max_tokens":100})),
            (
                Provider::OpenAiResponses,
                serde_json::json!({"max_output_tokens":100}),
            ),
            (Provider::Anthropic, serde_json::json!({"max_tokens":100})),
            (
                Provider::Gemini,
                serde_json::json!({"generationConfig":{"maxOutputTokens":100}}),
            ),
            (
                Provider::Ollama,
                serde_json::json!({"options":{"num_predict":100}}),
            ),
        ] {
            let encoded = serde_json::to_vec(&body).unwrap();
            check_sent_cap(provider, &encoded, 100).unwrap();
            let error = check_sent_cap(provider, &encoded, 99).unwrap_err();
            assert_eq!(error.code, tembra::Code::Unavailable);
            assert_eq!(error.reason, "swarmie.models.output_reservation");
            assert!(check_sent_cap(provider, b"{}", 100).is_err());
            assert!(check_sent_cap(provider, &encoded, 0).is_err());
        }
    }

    #[test]
    fn missing_ambiguous_or_unbounded_caps_do_not_authorize_a_reservation() {
        for body in [
            br#"{"max_tokens":100,"max_completion_tokens":100}"#.as_slice(),
            br#"{"max_tokens":null}"#,
            br#"{"max_tokens":"100"}"#,
            br#"{"max_tokens":100.0}"#,
            br#"{"max_tokens":-1}"#,
            br#"{"max_tokens":18446744073709551616}"#,
        ] {
            assert!(check_sent_cap(Provider::OpenAi, body, 100).is_err());
        }
        assert!(
            check_sent_cap(Provider::Ollama, br#"{"options":{"num_predict":-1}}"#, 100).is_err()
        );
        assert!(
            check_sent_cap(Provider::DeepSeek, br#"{"max_completion_tokens":100}"#, 100).is_err()
        );
        assert!(check_sent_cap(Provider::OpenAiResponses, br#"{"max_tokens":100}"#, 100).is_err());
        assert!(check_sent_cap(Provider::Gemini, br#"{"maxOutputTokens":100}"#, 100).is_err());
    }

    #[test]
    fn attribution_covers_actual_provider_additions_without_double_counting_tools() {
        let bytes = br#"{"model":"fixed","messages":[{"role":"system","content":"instruction"},{"role":"user","content":"message"}],"tools":[{"function":{"name":"mcp.tool","description":"body","parameters":{}}}],"provider_addition":"retained"}"#;
        let result = measure(bytes).unwrap();
        result.validate().unwrap();
        assert_eq!(result.precision, CountPrecision::Estimated);
        assert_eq!(result.final_payload_bytes, bytes.len() as u64);
        for kind in [
            ComponentKind::Instructions,
            ComponentKind::Conversation,
            ComponentKind::Tools,
            ComponentKind::Other,
        ] {
            assert_eq!(
                result.components.iter().filter(|v| v.kind == kind).count(),
                1
            );
        }
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("provider_addition")
        );
    }
}
