use super::GenaiModel;
use crate::error::{Reason, invalid, upstream};
use futures::StreamExt;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use swarmie_models::{ModelProgress, ModelRequest, ModelResponse, OperationId, Progress};
use tembra::{Code, Context, Error};

pub(super) async fn stream(
    model: &GenaiModel,
    context: &Context,
    operation: OperationId,
    request: &ModelRequest,
    progress: &Progress,
) -> tembra::Result<ModelResponse> {
    use genai::chat::{ChatOptions, ChatResponse, ChatStreamEvent};
    context.check()?;
    let input = model.request(request)?;
    progress.publish(
        context,
        ModelProgress {
            operation,
            content: String::new(),
        },
    );
    let options = ChatOptions::default()
        .with_capture_content(true)
        .with_capture_tool_calls(true)
        .with_capture_usage(true)
        .with_capture_reasoning_content(true);
    context.check()?;
    let mut response = model
        .client
        .exec_chat_stream(model.model.as_str(), input, Some(&options))
        .await
        .map_err(upstream)?;
    context.check()?;
    let mut content = String::new();
    let mut tools = BTreeMap::new();
    let mut tool_bytes = 0usize;
    let interval = Duration::from_millis(model.connection.progress_interval_ms.get());
    let mut last_publish: Option<Instant> = None;
    let mut published_bytes = 0;
    let mut pending = false;
    let mut deadline = Box::pin(tokio::time::sleep(interval));
    // Continue owning transport after logical cancellation. Publishing
    // stops, but the physical future settles at EOF/error/timeout.
    loop {
        let event = if pending {
            let delay = interval.saturating_sub(
                last_publish
                    .expect("pending preview has a publication deadline")
                    .elapsed(),
            );
            if delay.is_zero() {
                progress.publish(
                    context,
                    ModelProgress {
                        operation,
                        content: content.clone(),
                    },
                );
                published_bytes = content.len();
                last_publish = Some(Instant::now());
                deadline
                    .as_mut()
                    .reset(tokio::time::Instant::now() + interval);
                pending = false;
                continue;
            }
            match futures::future::select(response.stream.next(), deadline.as_mut()).await {
                futures::future::Either::Left((event, _)) => event,
                futures::future::Either::Right(_) => {
                    progress.publish(
                        context,
                        ModelProgress {
                            operation,
                            content: content.clone(),
                        },
                    );
                    published_bytes = content.len();
                    last_publish = Some(Instant::now());
                    deadline
                        .as_mut()
                        .reset(tokio::time::Instant::now() + interval);
                    pending = false;
                    continue;
                }
            }
        } else {
            response.stream.next().await
        };
        let Some(event) = event else {
            break;
        };
        match event.map_err(upstream)? {
            ChatStreamEvent::Chunk(chunk) => {
                model.budget(
                    content
                        .len()
                        .saturating_add(chunk.content.len())
                        .saturating_add(tool_bytes),
                    false,
                )?;
                content.push_str(&chunk.content);
                if last_publish.is_none_or(|last| last.elapsed() >= interval) {
                    progress.publish(
                        context,
                        ModelProgress {
                            operation,
                            content: content.clone(),
                        },
                    );
                    published_bytes = content.len();
                    last_publish = Some(Instant::now());
                    deadline
                        .as_mut()
                        .reset(tokio::time::Instant::now() + interval);
                    pending = false;
                } else {
                    pending = true;
                }
            }
            ChatStreamEvent::ToolCallChunk(chunk) => {
                let call = chunk.tool_call;
                let bytes = serde_json::to_vec(&call)
                    .map_err(|_| invalid(Reason::Response, "tool preview cannot be encoded"))?
                    .len();
                let previous = tools.insert(call.call_id, bytes).unwrap_or(0);
                tool_bytes = tool_bytes.saturating_sub(previous).saturating_add(bytes);
                model.budget(content.len().saturating_add(tool_bytes), false)?;
            }
            ChatStreamEvent::ReasoningChunk(_) | ChatStreamEvent::ThoughtSignatureChunk(_) => {
                return Err(invalid(
                    Reason::Response,
                    "reasoning continuation is outside the current text/tool profile",
                ));
            }
            ChatStreamEvent::End(end) => {
                if end.captured_stop_reason.is_none() {
                    return Err(invalid(
                        Reason::Response,
                        "stream ended without a confirmed stop reason",
                    ));
                }
                let output = model.response(
                    ChatResponse {
                        content: end.captured_content.unwrap_or_default(),
                        reasoning_content: end.captured_reasoning_content,
                        model_iden: response.model_iden.clone(),
                        provider_model_iden: response.model_iden,
                        stop_reason: end.captured_stop_reason,
                        usage: end.captured_usage.unwrap_or_default(),
                        captured_raw_body: None,
                        response_id: end.captured_response_id,
                    },
                    request,
                )?;
                context.check()?;
                if output.message.content.len() != published_bytes {
                    progress.publish(
                        context,
                        ModelProgress {
                            operation,
                            content: output.message.content.clone(),
                        },
                    );
                }
                return Ok(output);
            }
            ChatStreamEvent::Start | ChatStreamEvent::Heartbeat => {}
        }
    }
    Err(Error::new(
        Code::Unavailable,
        Reason::Transport.as_str(),
        "model stream ended without a settled response",
    ))
}
