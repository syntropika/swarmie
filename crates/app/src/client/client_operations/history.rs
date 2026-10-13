//! Conversation changes recover resources before attempting one mutation.
use super::*;

pub(super) async fn change(
    call: &mut Calls<'_>,
    submissions: &ClientSubmissions,
    request: &Value,
) -> Outcome {
    let params = &request["params"];
    let session = text(params, "sessionId")?;
    let action = text(params, "action")?;
    if !matches!(action, "fork" | "branch" | "edit" | "rollback") {
        return Err(invalid("invalid history action"));
    }
    let projection = restore(call, submissions, request, session).await?;
    if projection["unresolved"] == true {
        return Err(invalid(
            "resolve the original input before changing history",
        ));
    }
    let mut draft = Value::Null;
    let mut mutation = json!({"sessionId":session});
    let method = if matches!(action, "fork" | "branch") {
        if let Some(message) = params.get("messageId") {
            mutation["messageId"] = message.clone();
        }
        mutation["position"] = if action == "branch" {
            "before_user".into()
        } else {
            params
                .get("position")
                .cloned()
                .unwrap_or("after_assistant".into())
        };
        if action == "branch" {
            let message = text(params, "messageId")?;
            let parts = message_parts(call, &projection, message).await?;
            draft = recover(call, &projection, &parts, message).await?;
        }
        "session/fork"
    } else {
        let scope = params["scope"].as_str().unwrap_or("conversation_only");
        if !matches!(scope, "conversation_only" | "conversation_and_files") {
            return Err(invalid("invalid rollback scope"));
        }
        mutation["scope"] = scope.into();
        if action == "edit" {
            let message = text(params, "messageId")?;
            let groups = active_turns(&projection["events"])?;
            let index = groups
                .iter()
                .position(|group| {
                    group
                        .iter()
                        .any(|event| format!("user-{}", event["sequence"]) == message)
                })
                .ok_or_else(|| invalid("choose a recorded user message to edit"))?;
            let event = groups[index]
                .iter()
                .find(|event| format!("user-{}", event["sequence"]) == message)
                .unwrap();
            draft = recover(call, &projection, &event["data"]["content"], message).await?;
            mutation["turns"] = json!(groups.len() - index);
        } else {
            mutation["turns"] = params
                .get("turns")
                .cloned()
                .ok_or_else(|| invalid("turns is required"))?;
        }
        mutation["expectedSequence"] = projection["snapshot"]["sequence"].clone();
        "session/rollback"
    };
    // Refuse changes based on a projection replaced while resources were being recovered.
    let observed = call
        .call("session/get", json!({"sessionId":session}), false)
        .await?;
    if observed["sequence"] != projection["snapshot"]["sequence"] {
        return Err(invalid(
            "history changed; refresh the conversation before editing",
        ));
    }
    let changed = call.call(method, mutation, true).await?;
    let destination = if method == "session/fork" {
        text(&changed, "id")?
    } else {
        session
    };
    let mut restored = match restore(call, submissions, request, destination).await {
        Ok(value) => value,
        Err(mut error) => {
            error_data(&mut error)["sessionId"] = destination.into();
            if method == "session/fork" {
                error_data(&mut error)["createdSession"] = changed.clone();
            }
            error_data(&mut error)["draft"] = draft;
            return Err(error);
        }
    };
    if method == "session/fork" {
        let inherited = async {
            let inputs = inherited_inputs(&projection, &restored)?;
            for input in &inputs {
                submissions
                    .retain_input(&call.target, destination, input.clone())
                    .await
                    .map_err(|e| failure(&e.to_string()))?;
            }
            Ok::<_, Value>(inputs)
        }
        .await;
        match inherited {
            Ok(inputs) => restored["inputs"] = json!(inputs),
            Err(mut error) => {
                error_data(&mut error)["sessionId"] = destination.into();
                error_data(&mut error)["createdSession"] = changed;
                error_data(&mut error)["draft"] = draft;
                return Err(error);
            }
        }
    }
    Ok(json!({"projection":restored,"draft":draft}))
}
// Copy authored metadata only for active messages present in the child's canonical history.
fn inherited_inputs(source: &Value, child: &Value) -> std::result::Result<Vec<Value>, Value> {
    let source_events: Vec<Value> = active_turns(&source["events"])?
        .into_iter()
        .flatten()
        .collect();
    let child_events = active_turns(&child["events"])?;
    let mut candidates: Vec<(Option<Value>, Value)> = source["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["receipt"]["delivery"] == "accepted" && entry["input"].is_object())
        .map(|entry| {
            let mut input = entry["input"].clone();
            input["id"] = format!(
                "{}:{}",
                source["snapshot"]["id"].as_str().unwrap_or(""),
                entry["key"].as_str().unwrap_or("")
            )
            .into();
            (Some(entry["receipt"]["turnId"].clone()), input)
        })
        .chain(
            source["inputs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|input| input["id"].is_string())
                .map(|input| (None, input.clone())),
        )
        .collect();
    let mut authored = Vec::new();
    for event in &source_events {
        let index = candidates.iter().position(|(turn, input)| {
            input["content"] == event["data"]["content"]
                && match turn {
                    Some(turn) => !turn.is_null() && *turn == event["turnId"],
                    None => input["afterSequence"]
                        .as_u64()
                        .is_some_and(|s| s < event["sequence"].as_u64().unwrap_or(0)),
                }
        });
        authored.push(index.map(|i| candidates.remove(i).1));
    }
    let mut cursor = 0;
    let mut inherited = Vec::new();
    for event in child_events.into_iter().flatten() {
        let matches = |source: &Value| source["data"]["content"] == event["data"]["content"];
        let remaining = &source_events[cursor..];
        let index = remaining
            .iter()
            .position(|source| {
                !event["turnId"].is_null() && source["turnId"] == event["turnId"] && matches(source)
            })
            .or_else(|| remaining.iter().position(matches));
        if let Some(offset) = index {
            let index = cursor + offset;
            cursor = index + 1;
            if let Some(mut input) = authored[index].clone() {
                input["afterSequence"] =
                    json!(event["sequence"].as_u64().unwrap_or(1).saturating_sub(1));
                input.as_object_mut().unwrap().remove("queueId");
                inherited.push(input);
            }
        }
    }
    Ok(inherited)
}
async fn restore(
    call: &Calls<'_>,
    submissions: &ClientSubmissions,
    request: &Value,
    session: &str,
) -> Outcome {
    let mut query = request.clone();
    query["params"] = json!({"sessionId":session});
    let response = crate::client_projection::restore(call.nodes, submissions, call.node, query)
        .await
        .map_err(|e| failure(&e.to_string()))?;
    let mut result = response["result"].clone();
    result["activation"] = json!({"status":if matches!(result["snapshot"]["state"].as_str(),Some("closed"|"failed")) { "inactive" } else { "attached" }});
    result["queue"] = result["snapshot"]["queue"].clone();
    Ok(result)
}
fn active_turns(events: &Value) -> std::result::Result<Vec<Vec<Value>>, Value> {
    let mut groups: Vec<Vec<Value>> = Vec::new();
    for event in events
        .as_array()
        .ok_or_else(|| failure("invalid history projection"))?
    {
        if event["type"] == "conversation.rolled_back" {
            let count = event["data"]["turns"]
                .as_u64()
                .ok_or_else(|| failure("invalid rollback event"))? as usize;
            groups.truncate(groups.len().saturating_sub(count));
        } else if event["type"] == "message.user" {
            let turn = event["turnId"].as_str();
            if let Some(group) = groups
                .iter_mut()
                .find(|g| turn.is_some() && g[0]["turnId"].as_str() == turn)
            {
                group.push(event.clone());
            } else {
                groups.push(vec![event.clone()]);
            }
        }
    }
    Ok(groups)
}
async fn message_parts(call: &mut Calls<'_>, projection: &Value, message: &str) -> Outcome {
    if let Some(event) = active_turns(&projection["events"])?
        .into_iter()
        .flatten()
        .find(|event| format!("user-{}", event["sequence"]) == message)
    {
        return Ok(event["data"]["content"].clone());
    }
    let tree = call
        .call(
            "session/tree",
            json!({"sessionId":projection["snapshot"]["id"]}),
            false,
        )
        .await?;
    let node = tree
        .as_array()
        .and_then(|nodes| {
            nodes
                .iter()
                .find(|node| node["id"] == message && node["role"] == "user")
        })
        .ok_or_else(|| invalid("choose an available native user message"))?;
    Ok(node["metadata"]["prompt"]["content"]
        .as_array()
        .map(|parts| json!(parts))
        .unwrap_or_else(|| json!([{ "type":"text", "data":node["text"] }])))
}
async fn recover(
    call: &mut Calls<'_>,
    projection: &Value,
    content: &Value,
    message: &str,
) -> Outcome {
    let parts = content
        .as_array()
        .ok_or_else(|| invalid("native message content is unavailable"))?;
    if parts
        .iter()
        .any(|part| !matches!(part["type"].as_str(), Some("text" | "image" | "skill")))
    {
        return Err(invalid("reattach the unavailable resource before editing"));
    }
    let candidates: Vec<Value> = projection["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["receipt"]["delivery"] == "accepted")
        .filter_map(|entry| {
            let input = &entry["input"];
            if input["content"] == *content {
                Some(input.clone())
            } else {
                None
            }
        })
        .chain(
            projection["inputs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|input| input["content"] == *content)
                .cloned(),
        )
        .collect();
    let mut unique = Vec::new();
    for candidate in candidates {
        if !unique.contains(&candidate) {
            unique.push(candidate);
        }
    }
    let mut candidates = unique;
    if candidates.len() > 1 {
        // A turn anchor disambiguates repeated provider inputs with different authored context.
        let turn = projection["events"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|e| format!("user-{}", e["sequence"]) == message)
            .map(|e| &e["turnId"]);
        if let Some(turn) = turn {
            candidates = projection["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|e| &e["receipt"]["turnId"] == turn && e["input"]["content"] == *content)
                .map(|e| e["input"].clone())
                .collect();
            if candidates.is_empty() {
                let sequence = projection["events"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|e| format!("user-{}", e["sequence"]) == message)
                    .and_then(|e| e["sequence"].as_u64());
                candidates = projection["inputs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|input| {
                        input["content"] == *content
                            && sequence.is_some_and(|sequence| {
                                input["afterSequence"].as_u64() == sequence.checked_sub(1)
                            })
                    })
                    .cloned()
                    .collect();
            }
        }
        if candidates.len() != 1 {
            return Err(invalid(
                "several original prompts match this native entry; edit the recorded chat message",
            ));
        }
    }
    if let Some(input) = candidates.first() {
        let mut attachments = input["attachments"].as_array().cloned().unwrap_or_default();
        for (index, attachment) in attachments.iter_mut().enumerate() {
            let part = parts
                .get(index + 1)
                .ok_or_else(|| invalid("saved attachment content is unavailable"))?;
            if attachment["image"] == true {
                attachment["data"] = part["data"]["data"].clone();
            } else if attachment["binary"] == true {
                let asset = call.extension("workspace","assets.read",json!({"assetId":attachment["assetId"],"sessionId":projection["snapshot"]["id"]}),false).await?;
                attachment["data"] = asset["data"].clone();
            } else {
                let prefix = format!("Attached file: {}\n", text(attachment, "name")?);
                attachment["data"] = part["data"]
                    .as_str()
                    .and_then(|s| s.strip_prefix(&prefix))
                    .ok_or_else(|| invalid("saved text attachment is unavailable"))?
                    .into();
            }
        }
        return Ok(
            json!({"draft":input["text"],"attachments":attachments,"contextItems":input["contextItems"]}),
        );
    }
    let text = parts
        .iter()
        .filter(|p| p["type"] == "text")
        .map(|p| p["data"].as_str().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let attachments = parts.iter().filter(|p| p["type"] == "image").map(|p| json!({"id":uuid::Uuid::new_v4().to_string(),"name":"Restored image","mime":p["data"]["mime_type"],"data":p["data"]["data"],"image":true})).collect::<Vec<_>>();
    let contexts = parts.iter().filter(|p| p["type"] == "skill").map(|p| json!({"id":uuid::Uuid::new_v4().to_string(),"kind":"resource","label":format!("Skill: {}",p["data"]["name"].as_str().unwrap_or("")),"text":format!("Native skill selected from {}",p["data"]["path"].as_str().unwrap_or("")),"skill":p["data"]})).collect::<Vec<_>>();
    Ok(json!({"draft":text,"attachments":attachments,"contextItems":contexts}))
}
pub(super) async fn queue(
    call: &mut Calls<'_>,
    submissions: &ClientSubmissions,
    params: &Value,
) -> Outcome {
    let session = text(params, "sessionId")?;
    let action = text(params, "action")?;
    let mut edited_input = None;
    let mut queue = call
        .call("session/queue/list", json!({"sessionId":session}), false)
        .await?;
    match action {
        "list" => return Ok(queue),
        "pause" => {
            call.call(
                "session/queue/pause",
                json!({"sessionId":session,"paused":params["paused"]}),
                true,
            )
            .await?;
        }
        "remove" | "update" => {
            let id = text(params, "id")?;
            let item = queue["items"]
                .as_array()
                .and_then(|items| items.iter().find(|item| item["id"] == id))
                .ok_or_else(|| invalid("queued input is no longer available"))?;
            let retained = submissions
                .recover(
                    call.node,
                    json!({"id":1,"expectedRuntime":call.target,"params":{"sessionId":session}}),
                )
                .await
                .map_err(|e| failure(&e.to_string()))?;
            let mut input = retained["result"]["inputs"]
                .as_array()
                .and_then(|inputs| inputs.iter().find(|input| input["queueId"] == id))
                .cloned();
            let mut args = json!({"sessionId":session,"id":id});
            if action == "update" {
                let replacement = params["text"]
                    .as_str()
                    .ok_or_else(|| invalid("queue text is required"))?;
                let mut content = item["prompt"]["content"]
                    .as_array()
                    .cloned()
                    .ok_or_else(|| failure("invalid queued prompt"))?;
                // Replace only the authored text; keep images, skills and other resources.
                if content.first().is_some_and(|p| p["type"] == "text") {
                    content[0]["data"] = replacement.into();
                } else {
                    content.insert(0, json!({"type":"text","data":replacement}));
                }
                args["content"] = content.into();
            }
            call.call(
                if action == "update" {
                    "session/queue/update"
                } else {
                    "session/queue/remove"
                },
                args.clone(),
                true,
            )
            .await?;
            if action == "update"
                && let Some(input) = input.as_mut()
            {
                input["text"] = params["text"].clone();
                input["content"] = args["content"].clone();
                let skills = input["contextItems"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter(|item| !item["skill"].is_null())
                            .cloned()
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                input["contextItems"] = skills.into();
                submissions
                    .retain_input(&call.target, session, input.clone())
                    .await
                    .map_err(|e| failure(&e.to_string()))?;
                edited_input = Some(input.clone());
            }
        }
        _ => return Err(invalid("invalid queue action")),
    }
    queue = call
        .call("session/queue/list", json!({"sessionId":session}), false)
        .await?;
    if let Some(input) = edited_input {
        queue["input"] = input;
    }
    Ok(queue)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inheritance_filters_rolled_back_inputs_and_remaps_native_sequences() {
        let content = json!([{"type":"text","data":"same provider input"}]);
        let source = json!({"snapshot":{"id":"source"},"events":[
            {"type":"message.user","sequence":2,"turnId":"removed","data":{"content":content}},
            {"type":"conversation.rolled_back","sequence":4,"data":{"turns":1}},
            {"type":"message.user","sequence":5,"turnId":"kept","data":{"content":content}}],
            "entries":[
                {"key":"removed","receipt":{"delivery":"accepted","turnId":"removed"},"input":{"text":"removed original","content":content,"afterSequence":1}},
                {"key":"kept","receipt":{"delivery":"accepted","turnId":"kept"},"input":{"text":"kept original","content":content,"afterSequence":4}}],"inputs":[]});
        let empty = json!({"events":[]});
        assert!(inherited_inputs(&source, &empty).unwrap().is_empty());
        let child = json!({"events":[{"type":"message.user","sequence":1,"turnId":"rewritten","data":{"content":content}}]});
        let inputs = inherited_inputs(&source, &child).unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0]["text"], "kept original");
        assert_eq!(inputs[0]["afterSequence"], 0);
        assert_eq!(inputs[0]["id"], "source:kept");
    }
}
