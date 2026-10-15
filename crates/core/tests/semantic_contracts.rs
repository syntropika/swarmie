use serde_json::{Value, json};
use std::collections::BTreeMap;
use swarmie::*;

fn legacy_request() -> PermissionRequest {
    serde_json::from_value(json!({
        "id":"question-native", "kind":"question", "title":"Agent needs input", "options":[],
        "metadata":{"questions":[{"id":"platform","prompt":"Choose a platform", "options":[{"id":"linux","label":"Linux", "description":"Local host", "vendor":{"icon":"penguin"}}],"multiple":false,"allowText":false}],"native":{"request_id":"question-native","tool_name":"AskUserQuestion"}}
    })).unwrap()
}

#[test]
fn canonical_questions_and_answers_preserve_legacy_forms_without_ambiguity() {
    let mut request = legacy_request();
    request.normalize_questions().unwrap();
    assert_eq!(
        request.questions[0].options[0].description.as_deref(),
        Some("Local host")
    );
    assert_eq!(
        request.questions[0].options[0].extra["vendor"]["icon"],
        "penguin"
    );
    assert!(!request.questions[0].allow_text);
    assert_eq!(request.metadata["native"]["tool_name"], "AskUserQuestion");
    let mut typed_only = request.clone();
    typed_only.metadata = Value::Null;
    typed_only.normalize_questions().unwrap();
    assert_eq!(
        typed_only.metadata["questions"],
        request.metadata["questions"]
    );
    let mut response: PermissionResponse = serde_json::from_value(
        json!({"id":"question-native","option":"answer","answers":{"platform":"linux"}}),
    )
    .unwrap();
    response.normalize_answers(true).unwrap();
    assert_eq!(response.data["answers"]["platform"], "linux");
    response.data["answers"]["platform"] = json!("other");
    assert!(matches!(
        response.normalize_answers(true),
        Err(Error::Invalid(_))
    ));
    response.data = Value::Null;
    assert!(matches!(
        response.normalize_answers(false),
        Err(Error::Invalid(_))
    ));
    request.questions[0].prompt = "Conflicting prompt".into();
    assert!(matches!(
        request.normalize_questions(),
        Err(Error::Invalid(_))
    ));
    let mut multiple = PermissionResponse {
        id: "multi".into(),
        option: "answer".into(),
        data: Value::Null,
        answers: BTreeMap::from([(
            "ids".into(),
            QuestionAnswer::Choices(vec!["one".into(), "two".into()]),
        )]),
    };
    multiple.normalize_answers(true).unwrap();
    assert_eq!(multiple.data["answers"]["ids"], json!(["one", "two"]));
}

fn journal(kind: &str, data: Value, raw: Value) -> RuntimeEvent {
    serde_json::from_value(json!({"sessionId":"saved", "sequence":42,"turnId":"turn-old", "source":"provider","timestampMs":123,"type":kind,"data":data,"raw":raw})).unwrap()
}

#[test]
fn old_provider_shaped_plan_and_child_journals_remain_readable_and_extensible() {
    let raw = json!({"method":"turn/plan/updated","params":{"plan":[{"step":"Inspect","status":"in_progress","vendorPriority":2}],"explanation":"Read first"}});
    let event = journal("plan.updated", raw["params"].clone(), raw.clone());
    let EventKind::PlanUpdated(plan) = &event.kind else {
        panic!()
    };
    assert_eq!(plan.entries[0].text.as_deref(), Some("Inspect"));
    assert_eq!(plan.entries[0].status.as_deref(), Some("in_progress"));
    assert_eq!(plan.entries[0].extra["vendorPriority"], 2);
    assert_eq!(plan.extra["explanation"], "Read first");
    assert_eq!(serde_json::to_value(&event).unwrap()["raw"], raw);
    let roundtrip: RuntimeEvent =
        serde_json::from_value(serde_json::to_value(&event).unwrap()).unwrap();
    assert!(
        matches!(roundtrip.kind, EventKind::PlanUpdated(p) if p.entries[0].text.as_deref()==Some("Inspect"))
    );
    let native = json!({"id":"child","parentToolId":"tool-parent","model":{"provider":"native"},"role":"researcher","usage":{"input_tokens":10},"vendor":{"progress":3}});
    let child_event = journal("subagent.updated", native.clone(), native.clone());
    let EventKind::SubagentUpdated(child) = &child_event.kind else {
        panic!()
    };
    assert_eq!(child.parent_tool_id.as_deref(), Some("tool-parent"));
    assert_eq!(child.role.as_deref(), Some("researcher"));
    assert!(child.model.is_none());
    assert_eq!(child.extra["nativeFields"]["model"]["provider"], "native");
    assert_eq!(child.extra["nativeUsage"]["input_tokens"], 10);
    assert_eq!(child.extra["vendor"]["progress"], 3);
    let restored: RuntimeEvent =
        serde_json::from_value(serde_json::to_value(child_event).unwrap()).unwrap();
    assert_eq!(restored.raw, Some(native));
}

#[test]
fn legacy_sparse_tool_and_nonobject_plan_records_do_not_break_replay() {
    let event = journal(
        "tool.updated",
        json!({"id":"tool","name":null,"status":"running","input":null,"output":null,"metadata":{"outputDelta":"chunk"}}),
        json!({"delta":"chunk"}),
    );
    let EventKind::ToolUpdated(tool) = event.kind else {
        panic!()
    };
    assert!(tool.details.is_none());
    assert_eq!(tool.metadata["outputDelta"], "chunk");
    for legacy in [
        json!("native markdown"),
        json!([{"content":"first"}]),
        Value::Null,
    ] {
        let event = journal("plan.updated", legacy.clone(), legacy.clone());
        let EventKind::PlanUpdated(plan) = event.kind else {
            panic!()
        };
        if !legacy.is_null() {
            assert_eq!(plan.extra["native"], legacy);
        }
        if legacy.is_string() {
            assert_eq!(plan.markdown.as_deref(), legacy.as_str());
        }
        if legacy.is_array() {
            assert_eq!(plan.entries[0].text.as_deref(), Some("first"));
        }
    }
}

#[test]
fn generated_event_schema_reaches_canonical_nested_types() {
    let schema = serde_json::to_value(schemars::schema_for!(RuntimeEvent)).unwrap();
    let definitions = schema["$defs"].as_object().unwrap();
    for name in [
        "Question",
        "QuestionOption",
        "PlanUpdate",
        "PlanEntry",
        "SubagentInfo",
        "ToolDetails",
        "FileLocation",
        "FileDiff",
        "ToolProgress",
    ] {
        assert!(definitions.contains_key(name), "unreachable schema {name}");
    }
    let answer_schema = serde_json::to_value(schemars::schema_for!(PermissionResponse)).unwrap();
    assert!(answer_schema["$defs"].get("QuestionAnswer").is_some());
    let answers = &answer_schema["$defs"]["QuestionAnswer"]["anyOf"];
    assert!(
        answers
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["type"] == "string")
    );
    assert!(
        answers
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["type"] == "array")
    );
}
