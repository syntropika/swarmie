use crate::*;
use futures_util::StreamExt;
use serde_json::Value;

pub(crate) async fn collect(
    agent: &Agent,
    events: &mut EventStream,
    prompt: Prompt,
    schema: bool,
) -> Result<String> {
    agent.prompt(prompt).await?;
    let mut text = String::new();
    let mut message = None;
    let mut final_text = None;
    let mut structured = None;
    let mut last_error = None;
    while let Some(event) = events.next().await {
        match event?.kind {
            EventKind::TextDelta {
                text: fragment,
                item_id,
            } => {
                if schema && message != item_id {
                    text.clear();
                }
                message = item_id;
                text.push_str(&fragment);
            }
            EventKind::AssistantMessage(value) => {
                if let Some(value) = message_text(&value) {
                    final_text = Some(value);
                }
            }
            EventKind::StructuredOutput(value) => structured = Some(value),
            EventKind::ToolStarted(_) | EventKind::ToolUpdated(_) | EventKind::ToolCompleted(_) => {
                let _ = agent.cancel().await;
                return Err(Error::Provider(
                    "generate requires text-only execution; the harness attempted a tool".into(),
                ));
            }
            EventKind::PermissionRequested(request) => {
                let option = request
                    .options
                    .iter()
                    .find(|option| {
                        matches!(
                            option.kind.as_str(),
                            "deny" | "deny_once" | "deny_session" | "cancel"
                        ) || option.id == "cancel"
                    })
                    .ok_or_else(|| {
                        Error::Unsupported(
                            "generate received a request that cannot be declined".into(),
                        )
                    })?;
                agent
                    .respond_permission(PermissionResponse {
                        answers: Default::default(),
                        id: request.id,
                        option: option.id.clone(),
                        data: Value::Null,
                    })
                    .await?;
            }
            EventKind::TurnEnded { outcome, .. } => {
                if outcome != TurnOutcome::Completed {
                    let detail = last_error
                        .map(|message| format!(": {message}"))
                        .unwrap_or_default();
                    return Err(Error::Provider(format!(
                        "generate turn ended: {outcome}{detail}"
                    )));
                }
                let reply = final_text.unwrap_or(text);
                if schema {
                    let value = structured
                        .or_else(|| serde_json::from_str(&reply).ok())
                        .ok_or_else(|| {
                            Error::Provider("generate expected structured JSON output".into())
                        })?;
                    return Ok(serde_json::to_string(&value)?);
                }
                return Ok(reply);
            }
            // Native retry diagnostics can precede a successful terminal receipt.
            EventKind::Error { message } => last_error = Some(message),
            EventKind::SessionEnded => return Err(Error::Closed),
            _ => (),
        }
    }
    Err(Error::Closed)
}
fn message_text(value: &Value) -> Option<String> {
    if let Some(text) = value
        .as_str()
        .or_else(|| value["text"].as_str())
        .or_else(|| value["content"].as_str())
    {
        return Some(text.into());
    }
    let blocks = value["content"].as_array()?;
    let text = blocks
        .iter()
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("");
    if text.is_empty() { None } else { Some(text) }
}
