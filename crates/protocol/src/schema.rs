use crate::{METHODS, PROTOCOL_VERSION};
use schemars::{JsonSchema, SchemaGenerator, generate::SchemaSettings};
use serde_json::{Map, Value, json};
use swarmie::*;

/// Generated public type definitions with the canonical named-parameter RPC catalog.
pub fn protocol_schema() -> Value {
    let mut input = SchemaSettings::draft2020_12()
        .for_deserialize()
        .into_generator();
    let mut output = SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator();
    let spawn = input_schema::<SpawnOptions>(&mut input);
    let execution = input_schema::<SessionExecutionOptions>(&mut input);
    let mcp_query = input_schema::<McpQuery>(&mut input);
    let mcp_mutation = input_schema::<McpMutation>(&mut input);
    let mcp_catalog = schema::<McpCatalog>(&mut output);
    let mcp_receipt = schema::<McpReceipt>(&mut output);
    let permission = input_schema::<PermissionResponse>(&mut input);
    let history_query = input_schema::<NativeHistoryQuery>(&mut input);
    let history_sessions = schema::<Vec<NativeHistorySession>>(&mut output);
    let history_tree = schema::<Vec<NativeMessageNode>>(&mut output);
    let history_document = schema::<NativeHistoryDocument>(&mut output);
    let account_query = input_schema::<AccountQuery>(&mut input);
    let account_results = schema::<Vec<AccountQuotaResult>>(&mut output);
    let metadata = input_schema::<SessionMetadataUpdate>(&mut input);
    let content = input_schema::<Vec<PromptContent>>(&mut input);
    let rollback_scope = input_schema::<RollbackScope>(&mut input);
    let snapshots = schema::<Vec<SessionSnapshot>>(&mut output);
    let snapshot = schema::<SessionSnapshot>(&mut output);
    let events = schema::<Vec<RuntimeEvent>>(&mut output);
    let event = schema::<RuntimeEvent>(&mut output);
    let discovery = schema::<Vec<Detection>>(&mut output);
    let probe = schema::<HarnessProbe>(&mut output);
    let extensions = schema::<Vec<ExtensionInfo>>(&mut output);
    let queued = schema::<QueuedPrompt>(&mut output);
    let queue = schema::<PromptQueue>(&mut output);
    let models = schema::<Vec<ModelDescriptor>>(&mut output);
    let tasks = schema::<Vec<AgentTask>>(&mut output);
    let configuration = schema::<Vec<SettingDescriptor>>(&mut output);
    let receipt = schema::<ConfigurationReceipt>(&mut output);
    let submission = schema::<SubmissionReceipt>(&mut output);
    let optional_submission = schema::<Option<SubmissionReceipt>>(&mut output);
    let preview = schema::<ConfigurationPreview>(&mut output);
    let directory_ref = input_schema::<DirectoryRef>(&mut input);
    let directory_input = input_schema::<DirectoryRegistration>(&mut input);
    let project_input = input_schema::<ProjectCreate>(&mut input);
    let project_update = input_schema::<ProjectUpdate>(&mut input);
    let default_update = input_schema::<DefaultDirectoryUpdate>(&mut input);
    let directory_record = schema::<DirectoryResource>(&mut output);
    let project_record = schema::<ProjectRecord>(&mut output);
    let project_list = schema::<Vec<ProjectRecord>>(&mut output);
    let default_record = schema::<Option<DefaultDirectory>>(&mut output);
    let selected_default = schema::<DefaultDirectory>(&mut output);
    let context_status = schema::<ContextStatus>(&mut output);
    let runtime_identity = schema::<RuntimeIdentity>(&mut output);
    let request_target = input_schema::<RuntimeIdentity>(&mut input);
    let mut methods = Map::new();
    for &method in METHODS {
        let session = || object(&["sessionId"], [("sessionId", json!({"type":"string"}))]);
        let prompt = || {
            let mut value = object(
                &[],
                [
                    ("text", json!({"type":"string"})),
                    ("content", content.clone()),
                ],
            );
            value["anyOf"] = json!([{"required":["text"]},{"required":["content"]}]);
            value
        };
        let unit = json!({"type":"null"});
        let (params, result) = match method {
            "directory/get" => (directory_ref.clone(), directory_record.clone()),
            "directory/register" => (directory_input.clone(), directory_record.clone()),
            "project/create" => (project_input.clone(), project_record.clone()),
            "project/update" => (project_update.clone(), project_record.clone()),
            "project/list" => (object(&[], []), project_list.clone()),
            "project/get" => (
                object(&["projectId"], [("projectId", json!({"type":"string"}))]),
                project_record.clone(),
            ),
            "settings/directory" => (object(&[], []), default_record.clone()),
            "settings/setDirectory" => (default_update.clone(), selected_default.clone()),
            "session/context" => (session(), context_status.clone()),
            "runtime/initialize" => (
                object(&[], []),
                object(
                    &[
                        "name",
                        "version",
                        "protocolVersion",
                        "methods",
                        "eventHistoryLimit",
                        "transcriptDurability",
                        "runtimeIdentity",
                    ],
                    [
                        ("name", json!({"type":"string"})),
                        ("version", json!({"type":"string"})),
                        ("protocolVersion", json!({"type":"string"})),
                        ("methods", json!({"type":"array","items":{"type":"string"}})),
                        ("eventHistoryLimit", json!({"type":"integer"})),
                        ("transcriptDurability", json!({"type":["string","null"]})),
                        ("runtimeIdentity", runtime_identity.clone()),
                    ],
                ),
            ),
            "runtime/shutdown" => (object(&[], []), unit.clone()),
            "runtime/schema" => (object(&[], []), json!({"type":"object"})),
            "mcp/reopen" => (session(), snapshot.clone()),
            "mcp/list" => (mcp_query.clone(), mcp_catalog.clone()),
            "mcp/mutate" => (mcp_mutation.clone(), mcp_receipt.clone()),
            "runtime/discover" => (object(&[], []), discovery.clone()),
            "runtime/agentToolsSetup" | "runtime/agentToolsDisconnect" => (
                object(&["harness"], [("harness", json!({"type":"string"}))]),
                agent_tool_descriptor_schema(),
            ),
            "runtime/agentToolsInstall" => (
                object(&["harness"], [("harness", json!({"type":"string"}))]),
                agent_tool_flow_schema(),
            ),
            "runtime/agentToolsConnect" => (
                object(
                    &["harness", "provider", "method", "storage", "values"],
                    [
                        ("harness", json!({"type":"string"})),
                        ("provider", json!({"type":"string"})),
                        ("method", json!({"type":"string"})),
                        ("storage", json!({"enum":["secure","session"]})),
                        (
                            "values",
                            json!({"type":"object","additionalProperties":{"type":"string","maxLength":16384}}),
                        ),
                    ],
                ),
                agent_tool_flow_schema(),
            ),
            "runtime/agentToolsFlow" | "runtime/agentToolsCancel" => (
                object(&["id"], [("id", json!({"type":"string"}))]),
                agent_tool_flow_schema(),
            ),
            "runtime/agentToolsInput" => (
                object(
                    &["id", "values"],
                    [
                        ("id", json!({"type":"string"})),
                        (
                            "values",
                            json!({"type":"object","additionalProperties":{"type":"string","maxLength":16384}}),
                        ),
                    ],
                ),
                agent_tool_flow_schema(),
            ),
            "runtime/configuration" => (spawn.clone(), preview.clone()),
            "runtime/probe" => (
                object(&["harness"], [("harness", json!({"type":"string"}))]),
                probe.clone(),
            ),
            "runtime/history" => (history_query.clone(), history_sessions.clone()),
            "runtime/history/read" | "session/import" => (
                json!({"allOf":[history_query.clone(),object(&["nativeId"],[("nativeId",json!({"type":"string"}))])]}),
                if method == "session/import" {
                    snapshot.clone()
                } else {
                    history_document.clone()
                },
            ),
            "runtime/quotas" => (account_query.clone(), json!(true)),
            "runtime/quotas/all" => (
                object(
                    &["queries"],
                    [(
                        "queries",
                        json!({"type":"array","minItems":1,"maxItems":32,"items":account_query.clone()}),
                    )],
                ),
                account_results.clone(),
            ),
            "runtime/generate" => (
                json!({"allOf":[spawn,prompt(),object(&[],[("timeoutMs",json!({"type":"integer","minimum":1,"maximum":600000}))])]}),
                json!({"type":"string"}),
            ),
            "extension/list" => (object(&[], []), extensions.clone()),
            "extension/invoke" => (
                object(
                    &["extensionId", "command"],
                    [
                        ("extensionId", json!({"type":"string"})),
                        ("command", json!({"type":"string"})),
                        ("arguments", json!(true)),
                    ],
                ),
                json!(true),
            ),
            "agent/spawn" | "session/create" => (spawn.clone(), snapshot.clone()),
            "session/tree" => (session(), history_tree.clone()),
            "session/list" => (object(&[], []), snapshots.clone()),
            "session/resume" => (
                json!({"allOf":[session(),object(&[],[("execution",execution.clone())])]}),
                snapshot.clone(),
            ),
            "session/fork" => (
                json!({"allOf":[session(),object(&[],[("messageId",json!({"type":"string","minLength":1,"maxLength":4096})),("execution",execution.clone()),("position",json!({"type":"string","enum":["after_assistant","before_user"]}))])]}),
                snapshot.clone(),
            ),
            "session/get" => (session(), snapshot.clone()),
            "session/metadata" => (json!({"allOf":[session(),metadata]}), snapshot.clone()),
            "session/events" | "session/transcript" => (
                json!({"allOf":[session(),object(&[],[("afterSequence",json!({"type":"integer","minimum":0})),("limit",json!({"type":"integer","minimum":1,"maximum":4096}))])]}),
                events.clone(),
            ),
            "session/queue/list" => (session(), queue.clone()),
            "session/queue" => (json!({"allOf":[session(),prompt()]}), queued.clone()),
            "session/queue/update" => (
                json!({"allOf":[session(),prompt(),object(&["id"],[("id",json!({"type":"string"}))])]}),
                unit.clone(),
            ),
            "session/queue/remove" => (
                json!({"allOf":[session(),object(&["id"],[("id",json!({"type":"string"}))])]}),
                unit.clone(),
            ),
            "session/queue/pause" => (
                json!({"allOf":[session(),object(&["paused"],[("paused",json!({"type":"boolean"}))])]}),
                unit.clone(),
            ),
            "session/prompt" => (
                json!({"allOf":[session(),prompt(),object(&["idempotencyKey"],[("idempotencyKey",json!({"type":"string","minLength":1,"maxLength":128}))])]}),
                submission.clone(),
            ),
            "session/submission" | "session/submission/reconcile" => (
                json!({"allOf":[session(),object(&["idempotencyKey"],[("idempotencyKey",json!({"type":"string","minLength":1,"maxLength":128}))])]}),
                optional_submission.clone(),
            ),
            "session/steer" => (
                json!({"allOf":[session(),prompt()]}),
                object(&["accepted"], [("accepted", json!({"type":"boolean"}))]),
            ),
            "session/cancel" => (
                json!({"allOf":[session(),object(&[],[("turnId",json!({"type":"string"}))])]}),
                unit.clone(),
            ),
            "permission/respond" => (json!({"allOf":[session(),permission]}), unit.clone()),
            "agent/configure" => (
                json!({"allOf":[session(),object(&["config"],[("config",json!({"type":"object"}))])]}),
                receipt.clone(),
            ),
            "agent/models" => (session(), models.clone()),
            "agent/configuration" => (session(), configuration.clone()),
            "agent/commands" | "agent/quotas" => (session(), json!(true)),
            "session/tasks" => (session(), tasks.clone()),
            "session/task/stop" => (
                json!({"allOf":[session(),object(&["taskId"],[("taskId",json!({"type":"string","minLength":1,"maxLength":4096}))])]}),
                unit.clone(),
            ),
            "session/rollback" => (
                json!({"allOf":[session(),object(&["turns"],[("turns",json!({"type":"integer","minimum":1})),("scope",rollback_scope.clone()),("expectedSequence",json!({"type":"integer","minimum":0}))])]}),
                unit.clone(),
            ),
            "session/compact" | "session/close" | "agent/kill" => (session(), unit),
            _ => panic!("RPC method has no schema: {method}"),
        };
        methods.insert(method.into(), json!({"params":params,"result":result}));
    }
    let mut definitions = output.take_definitions(true);
    for (name, mut value) in input.take_definitions(true) {
        prefix_input_refs(&mut value);
        definitions.insert(format!("Input{name}"), value);
    }
    json!({"$schema":"https://json-schema.org/draft/2020-12/schema","title":"swarmie runtime protocol","$defs":definitions,"x-protocolVersion":PROTOCOL_VERSION,"x-methods":methods,"x-event":event,"x-requestTarget":request_target})
}
fn agent_tool_field_schema() -> Value {
    object(
        &["id", "label"],
        [
            ("id", json!({"type":"string"})),
            ("label", json!({"type":"string"})),
            ("secret", json!({"type":"boolean"})),
            ("placeholder", json!({"type":"string"})),
        ],
    )
}
fn agent_tool_flow_schema() -> Value {
    object(
        &["id", "status"],
        [
            ("id", json!({"type":"string"})),
            (
                "status",
                json!({"enum":["pending","connected","completed","failed","cancelled"]}),
            ),
            ("operation", json!({"enum":["install","connect"]})),
            ("message", json!({"type":"string"})),
            ("authorizationUrl", json!({"type":"string"})),
            ("userCode", json!({"type":"string"})),
            ("expiresAt", json!({"type":"string"})),
            (
                "inputFields",
                json!({"type":"array","items":agent_tool_field_schema()}),
            ),
        ],
    )
}
fn agent_tool_descriptor_schema() -> Value {
    let method = object(
        &["id", "label", "kind"],
        [
            ("id", json!({"type":"string"})),
            ("label", json!({"type":"string"})),
            (
                "kind",
                json!({"enum":["api_key","browser","device","existing"]}),
            ),
            ("help", json!({"type":"string"})),
            (
                "fields",
                json!({"type":"array","items":agent_tool_field_schema()}),
            ),
        ],
    );
    let provider = object(
        &["id", "label", "methods"],
        [
            ("id", json!({"type":"string"})),
            ("label", json!({"type":"string"})),
            ("methods", json!({"type":"array","items":method})),
        ],
    );
    object(
        &["harness", "installed", "authState", "storage", "providers"],
        [
            ("harness", json!({"type":"string"})),
            ("installed", json!({"type":"boolean"})),
            ("manual", json!({"type":"boolean"})),
            (
                "authState",
                json!({"enum":["authenticated","unauthenticated","unknown"]}),
            ),
            (
                "storage",
                object(
                    &["persistent", "description"],
                    [
                        ("persistent", json!({"type":"boolean"})),
                        ("description", json!({"type":"string"})),
                    ],
                ),
            ),
            ("providers", json!({"type":"array","items":provider})),
            ("message", json!({"type":"string"})),
            ("flow", agent_tool_flow_schema()),
            (
                "connection",
                object(
                    &["provider", "method", "storage", "verified"],
                    [
                        ("provider", json!({"type":"string"})),
                        ("method", json!({"type":"string"})),
                        ("storage", json!({"enum":["native","secure","session"]})),
                        ("verified", json!({"type":"boolean"})),
                    ],
                ),
            ),
            (
                "installation",
                object(
                    &["supported"],
                    [
                        ("supported", json!({"type":"boolean"})),
                        ("message", json!({"type":"string"})),
                    ],
                ),
            ),
        ],
    )
}
fn schema<T: JsonSchema>(generator: &mut SchemaGenerator) -> Value {
    generator.subschema_for::<T>().to_value()
}
fn input_schema<T: JsonSchema>(generator: &mut SchemaGenerator) -> Value {
    let mut value = schema::<T>(generator);
    prefix_input_refs(&mut value);
    value
}
fn prefix_input_refs(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if let Some(Value::String(reference)) = object.get_mut("$ref")
                && let Some(name) = reference.strip_prefix("#/$defs/")
            {
                *reference = format!("#/$defs/Input{name}");
            }
            for child in object.values_mut() {
                prefix_input_refs(child);
            }
        }
        Value::Array(array) => {
            for child in array {
                prefix_input_refs(child);
            }
        }
        _ => (),
    }
}
fn object<const N: usize>(required: &[&str], properties: [(&str, Value); N]) -> Value {
    json!({"type":"object","required":required,"properties":properties.into_iter().map(|(key,value)|(key.into(),value)).collect::<Map<String,Value>>()})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_schema_covers_every_method_and_has_resolvable_separate_input_output_refs() {
        let schema = protocol_schema();
        assert_eq!(
            schema["x-methods"].as_object().unwrap().len(),
            METHODS.len()
        );
        for &method in METHODS {
            assert!(schema["x-methods"][method]["params"].is_object());
        }
        fn inspect(value: &Value, root: &Value) {
            match value {
                Value::Object(object) => {
                    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                        assert!(
                            root.pointer(reference.strip_prefix('#').unwrap()).is_some(),
                            "unresolved {reference}"
                        );
                    }
                    for value in object.values() {
                        inspect(value, root);
                    }
                }
                Value::Array(array) => {
                    for value in array {
                        inspect(value, root);
                    }
                }
                _ => (),
            }
        }
        inspect(&schema, &schema);
        assert!(schema["$defs"]["InputSessionExecutionOptions"]["properties"]["env"].is_object());
        assert!(
            schema["$defs"]["SessionExecutionOptions"]["properties"]
                .get("env")
                .is_none()
        );
        assert!(schema["$defs"]["SessionSnapshot"]["properties"]["options"].is_object());
    }
}
