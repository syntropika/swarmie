use serde_json::json;
use swarmie_model_submission_protection::*;
use swarmie_permissions::{
    Authority, CorePlugin, ExecutionBinding, ExecutionIdentity, HumanSession, PluginGrant,
    ShellIdentity,
};
use uuid::Uuid;

fn owner(authority: &Authority) -> (HumanSession, ExecutionBinding) {
    let shell = authority.authenticate_shell(ShellIdentity {
        cloud: None,
        account: None,
        installation: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    });
    let execution = authority
        .execution(
            &shell,
            ExecutionIdentity {
                account: None,
                execution: Uuid::new_v4(),
                generation: Uuid::new_v4(),
                project: None,
                agent: None,
                conversation: None,
                main_agent: true,
            },
        )
        .unwrap();
    (shell, execution)
}

fn guard(detector: Option<Detector>) -> ModelGuard {
    ModelGuard::new(
        "core.detector".into(),
        PluginGrant::bundled(CorePlugin::ModelProtection, "core.detector".into()),
        detector,
    )
}

fn token() -> String {
    format!("ghp_{}", "aB3dE5gH7jK9mN1pQ3sT5vW7yZ9bC1fG3hJ5")
}

#[test]
fn pinned_corpus_catches_nested_escaped_text_and_ignores_suppression_comments() {
    let detector = Detector::bundled().expect("all 222 pinned rules compile");
    let value = json!({"tools": [{"description": format!("token={} # gitleaks:allow", token())}]});
    let body = serde_json::to_vec(&value).unwrap();
    let inspection = detector.inspect(&body, Coverage::TextJson);
    assert!(matches!(inspection, Inspection::Detected { .. }));
    assert!(
        !serde_json::to_string(&inspection)
            .unwrap()
            .contains(&token())
    );
    let escaped = format!(
        r#"{{"message":"\u0067hp_{}"}}"#,
        "aB3dE5gH7jK9mN1pQ3sT5vW7yZ9bC1fG3hJ5"
    );
    assert!(matches!(
        detector.inspect(escaped.as_bytes(), Coverage::TextJson),
        Inspection::Detected { .. }
    ));
    assert_eq!(
        detector.inspect(br#"{"message":"hello"}"#, Coverage::Unsupported),
        Inspection::Incomplete
    );
    assert_eq!(
        detector.inspect(b"broken json", Coverage::TextJson),
        Inspection::Incomplete
    );
}

#[test]
fn exact_owner_override_dispatches_original_bytes_once() {
    let authority = Authority::new();
    let (shell, execution) = owner(&authority);
    let (other, _) = owner(&authority);
    let guard = guard(Detector::bundled());
    let body = serde_json::to_vec(&json!({"content": token()})).unwrap();
    let mut prepared = guard
        .prepare(
            execution,
            ModelAttempt::new(Uuid::new_v4()),
            "model".into(),
            "https://model.example/v1".into(),
            body.clone(),
            Coverage::TextJson,
        )
        .unwrap();
    let foreign = other.decide(&("model-send/v1", prepared.intent())).unwrap();
    assert_eq!(prepared.allow_once(foreign), Err(Error::Revoked));
    let decision = shell.decide(&("model-send/v1", prepared.intent())).unwrap();
    prepared.allow_once(decision).unwrap();
    let sent = prepared
        .dispatch(|destination, content| {
            assert_eq!(destination, "https://model.example/v1");
            content.to_vec()
        })
        .unwrap();
    assert_eq!(sent, body);
}

#[test]
fn engine_absence_blocks_and_explicit_cannot_inspect_override_works() {
    let authority = Authority::new();
    let (shell, execution) = owner(&authority);
    let guard = guard(None);
    let prepare = || {
        guard
            .prepare(
                execution.clone(),
                ModelAttempt::new(Uuid::new_v4()),
                "model".into(),
                "https://model.example".into(),
                b"{}".to_vec(),
                Coverage::TextJson,
            )
            .unwrap()
    };
    let blocked = prepare();
    assert_eq!(blocked.intent().inspection, Inspection::Unavailable);
    assert_eq!(blocked.dispatch(|_, _| ()), Err(Error::DecisionRequired));
    let mut pending = prepare();
    pending
        .allow_once(shell.decide(&("model-send/v1", pending.intent())).unwrap())
        .unwrap();
    assert!(pending.dispatch(|_, _| ()).is_ok());
}

#[test]
fn policy_changes_and_owner_end_invalidate_existing_preparations() {
    let authority = Authority::new();
    let (shell, execution) = owner(&authority);
    let guard = guard(Detector::bundled());
    let prepare = || {
        guard
            .prepare(
                execution.clone(),
                ModelAttempt::new(Uuid::new_v4()),
                "model".into(),
                "https://model.example".into(),
                b"{}".to_vec(),
                Coverage::TextJson,
            )
            .unwrap()
    };
    let clean = prepare();
    let decision = shell
        .decide(&("model-protection-policy/v1", guard.generation(), false))
        .unwrap();
    guard.configure(&shell, decision, false).unwrap();
    assert_eq!(clean.dispatch(|_, _| ()), Err(Error::Revoked));
    let unprotected = prepare();
    assert_eq!(unprotected.intent().inspection, Inspection::Unprotected);
    shell.close();
    assert_eq!(unprotected.dispatch(|_, _| ()), Err(Error::Revoked));
}

#[test]
fn duplicate_decoded_json_keys_are_incomplete_even_if_the_last_value_is_safe() {
    let detector = Detector::bundled().unwrap();
    let body = br#"{"a":{"text":"ignored"},"a":{"text":"safe"}}"#;
    assert_eq!(
        detector.inspect(body, Coverage::TextJson),
        Inspection::Incomplete
    );
    assert_eq!(
        detector.inspect(
            br#"{"text":"ignored","\u0074ext":"safe"}"#,
            Coverage::TextJson
        ),
        Inspection::Incomplete
    );
}

#[test]
fn owned_worker_results_require_the_original_guard_and_current_policy() {
    let authority = Authority::new();
    let (shell, execution) = owner(&authority);
    let original = guard(Detector::bundled());
    let foreign = guard(Detector::bundled());
    let scan = || original.scan(b"{}".to_vec(), Coverage::TextJson).unwrap();
    let first = scan();
    let inspected = std::thread::spawn(move || first.run()).join().unwrap();
    assert!(matches!(
        foreign.prepare_inspected(
            execution.clone(),
            ModelAttempt::new(Uuid::new_v4()),
            "model".into(),
            "https://model.example".into(),
            inspected,
        ),
        Err(Error::Revoked)
    ));
    let valid = original
        .prepare_inspected(
            execution.clone(),
            ModelAttempt::new(Uuid::new_v4()),
            "model".into(),
            "https://model.example".into(),
            scan().run(),
        )
        .unwrap();
    assert_eq!(valid.intent().inspection, Inspection::Clean);
    assert_eq!(valid.dispatch(|_, body| body.to_vec()), Ok(b"{}".to_vec()));
    let pending = scan();
    let decision = shell
        .decide(&("model-protection-policy/v1", original.generation(), false))
        .unwrap();
    original.configure(&shell, decision, false).unwrap();
    let stale = std::thread::spawn(move || pending.run()).join().unwrap();
    assert!(matches!(
        original.prepare_inspected(
            execution,
            ModelAttempt::new(Uuid::new_v4()),
            "model".into(),
            "https://model.example".into(),
            stale,
        ),
        Err(Error::Revoked)
    ));
}
