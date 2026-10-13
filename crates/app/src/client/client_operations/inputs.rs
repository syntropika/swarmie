//! Authored input preparation and resource recovery on the selected runtime.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    session_id: String,
    text: String,
    #[serde(default)]
    attachments: Vec<Value>,
    #[serde(default)]
    context_items: Vec<Value>,
    #[serde(default = "prompt")]
    delivery: String,
    #[serde(default)]
    idempotency_key: Option<String>,
}
fn prompt() -> String {
    "prompt".into()
}
pub(crate) async fn prepare(
    nodes: &NodeGateway,
    node: &str,
    target: RuntimeIdentity,
    params: &Value,
) -> Outcome {
    let mut call = Calls {
        nodes,
        node,
        target,
        effects: false,
    };
    prepare_with(&mut call, params).await.map_err(|mut error| {
        if call.effects {
            error_data(&mut error)["effectKnowledge"] = "unknown".into();
        }
        error
    })
}
async fn prepare_with(call: &mut Calls<'_>, params: &Value) -> Outcome {
    let input: Input =
        serde_json::from_value(params.clone()).map_err(|e| invalid(&e.to_string()))?;
    if input.session_id.is_empty()
        || !matches!(input.delivery.as_str(), "prompt" | "steer" | "queue")
    {
        return Err(invalid("invalid input destination or delivery mode"));
    }
    if input.delivery == "prompt" && input.idempotency_key.as_ref().is_none_or(|s| s.is_empty()) {
        return Err(invalid("idempotencyKey is required"));
    }
    if input.text.trim().is_empty() && input.attachments.is_empty() {
        return Err(invalid("write a message or attach a file"));
    }
    if serde_json::to_vec(params)
        .map_err(|e| invalid(&e.to_string()))?
        .len()
        > 12 * 1024 * 1024
    {
        return Err(invalid("keep the complete message under 12 MB"));
    }
    let snapshot = call
        .call("session/get", json!({"sessionId":input.session_id}), false)
        .await?;
    let caps = &snapshot["capabilities"];
    if input.delivery == "prompt" && snapshot["state"] != "idle" {
        return Err(invalid(
            "choose Queue follow-up or Steer while the agent is working",
        ));
    }
    if input.delivery == "steer" && caps["steering"]["availability"] != "available" {
        return Err(invalid(
            "this harness does not support steering; queue a follow-up instead",
        ));
    }
    let mut contexts = Vec::new();
    let mut skills = Vec::new();
    for item in &input.context_items {
        if item.get("skill").is_some_and(|s| !s.is_null()) {
            if caps["nativeSkillInputs"]["availability"] != "available" {
                return Err(invalid(
                    "this harness does not accept selected native skill paths",
                ));
            }
            text(&item["skill"], "name")?;
            text(&item["skill"], "path")?;
            skills.push(json!({"type":"skill","data":item["skill"]}));
        } else {
            contexts.push(format!(
                "Context ({}): {}\n{}",
                text(item, "kind")?,
                text(item, "label")?,
                item["text"]
                    .as_str()
                    .ok_or_else(|| invalid("context text is required"))?
            ));
        }
    }
    let context = contexts.join("\n\n");
    if context.len() > 1024 * 1024 {
        return Err(invalid("keep selected context under 1 MB"));
    }
    // Validate every attachment before opening a workspace or writing assets.
    for attachment in &input.attachments {
        text(attachment, "id")?;
        let name = text(attachment, "name")?;
        let data = attachment["data"]
            .as_str()
            .ok_or_else(|| invalid("attachment data is required"))?;
        if attachment["image"] == true {
            if caps["images"]["availability"] != "available" {
                return Err(invalid("this harness does not accept images"));
            }
            text(attachment, "mime")?;
            STANDARD
                .decode(data)
                .map_err(|_| invalid("image data must be base64"))?;
        } else if attachment["binary"] == true {
            if name == "."
                || name == ".."
                || name.contains(['/', '\\'])
                || name.chars().any(char::is_control)
            {
                return Err(invalid("asset name must be one file name"));
            }
            if attachment["assetId"].is_null()
                && STANDARD
                    .decode(data)
                    .map_err(|_| invalid("asset data must be base64"))?
                    .len()
                    > 8 * 1024 * 1024
            {
                return Err(invalid("asset exceeds 8 MiB"));
            }
        }
    }
    let binary = input
        .attachments
        .iter()
        .any(|a| a["binary"] == true && a["image"] != true);
    let folder = if binary {
        if !call
            .extensions()
            .await?
            .iter()
            .any(|e| e["id"] == "workspace")
        {
            return Err(invalid(
                "enable the workspace extension to attach binary files",
            ));
        }
        Some(
            call.extension(
                "workspace",
                "project.open",
                json!({"path":snapshot["options"]["cwd"],"sessionId":input.session_id}),
                false,
            )
            .await?,
        )
    } else {
        None
    };
    let mut content = vec![
        json!({"type":"text","data":if context.is_empty() { input.text.clone() } else { format!("{}\n\n{context}",input.text) }}),
    ];
    let mut attachments = Vec::new();
    for mut attachment in input.attachments {
        let name = text(&attachment, "name")?.to_owned();
        if attachment["image"] == true {
            content.push(json!({"type":"image","data":{"data":attachment["data"],"mime_type":attachment["mime"]}}));
        } else if attachment["binary"] == true {
            let asset = if attachment["assetId"].is_string() {
                // Resolve the retained asset on the owning node; client-supplied paths are not evidence.
                call.extension(
                    "workspace",
                    "assets.read",
                    json!({"assetId":attachment["assetId"],"sessionId":input.session_id}),
                    false,
                )
                .await?
            } else {
                call.extension("workspace", "assets.write", json!({"projectId":folder.as_ref().unwrap()["id"],"sessionId":input.session_id,"name":name,"data":attachment["data"]}), true).await?
            };
            let path = text(&asset, "path")?;
            content.push(json!({"type":"text","data":format!("Attached file available on disk: {path}\nName: {name}. Use the harness's native file tools if they support this format.")}));
            attachment["path"] = asset["path"].clone();
            attachment["assetId"] = asset["assetId"].clone();
        } else {
            content.push(json!({"type":"text","data":format!("Attached file: {name}\n{}",attachment["data"].as_str().unwrap())}));
        }
        attachment["data"] = "".into();
        attachments.push(attachment);
    }
    content.extend(skills);
    if serde_json::to_vec(&content).unwrap().len() > 12 * 1024 * 1024 {
        return Err(invalid("keep the complete message under 12 MB"));
    }
    Ok(
        json!({"text":input.text,"attachments":attachments,"contextItems":input.context_items,"content":content,"afterSequence":snapshot["sequence"]}),
    )
}
pub(super) async fn deliver(
    call: &mut Calls<'_>,
    submissions: &ClientSubmissions,
    request: &Value,
) -> Outcome {
    let delivery = request["params"]["delivery"].as_str().unwrap_or("prompt");
    if delivery == "prompt" {
        let response = submissions
            .input(call.node, request.clone())
            .await
            .map_err(|e| {
                if matches!(e, swarmie::Error::Invalid(_)) {
                    invalid(&e.to_string())
                } else {
                    failure(&e.to_string())
                }
            })?;
        if let Some(error) = response.get("error") {
            return Err(error.clone());
        }
        let mut output = response["result"].clone();
        if output["receipt"]["delivery"] == "accepted"
            && output["receipt"]["recoveryRequired"] != true
        {
            call.effects = true;
            title(call, text(&request["params"], "sessionId")?, &mut output).await;
        }
        return Ok(output);
    }
    let mut input = prepare_with(call, &request["params"]).await?;
    input["id"] = uuid::Uuid::new_v4().to_string().into();
    let session = text(&request["params"], "sessionId")?;
    submissions
        .retain_input(&call.target, session, input.clone())
        .await
        .map_err(|e| failure(&e.to_string()))?;
    let result = call
        .call(
            match delivery {
                "queue" => "session/queue",
                "steer" => "session/steer",
                _ => return Err(invalid("invalid delivery mode")),
            },
            json!({"sessionId":session,"content":input["content"]}),
            true,
        )
        .await?;
    if delivery == "queue" {
        input["queueId"] = result["id"].clone();
        submissions
            .retain_input(&call.target, session, input.clone())
            .await
            .map_err(|e| failure(&e.to_string()))?;
    }
    let mut output = json!({"input":input,"result":result});
    if delivery == "queue" {
        output["queue"] = call
            .call("session/queue/list", json!({"sessionId":session}), false)
            .await?;
    }
    title(call, session, &mut output).await;
    Ok(output)
}

async fn title(call: &mut Calls<'_>, session: &str, output: &mut Value) {
    let observed = call
        .call("session/get", json!({"sessionId":session}), false)
        .await;
    let result = match observed {
        Ok(snapshot) => {
            if let Some(title) = snapshot["metadata"]["title"].as_str() {
                Ok(title.to_owned())
            } else {
                let text = output["input"]["text"]
                    .as_str()
                    .unwrap_or("")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let title: String = if text.is_empty() {
                    "Attached files".into()
                } else {
                    text.chars().take(64).collect()
                };
                call.call(
                    "session/metadata",
                    json!({"sessionId":session,"title":title}),
                    true,
                )
                .await
                .map(|_| title)
            }
        }
        Err(error) => Err(error),
    };
    match result {
        Ok(title) => output["title"] = title.into(),
        Err(error) => output["metadataError"] = error,
    }
}
