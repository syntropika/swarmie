//! Real-Host diagnostics preserve exact semantics and refuse overwriting retained evidence.

#[path = "../examples/diagnostic/mod.rs"]
mod diagnostic;

use std::{ffi::OsString, path::Path};

use serde_json::Value;

fn arguments(output: &Path, observer: &str) -> Vec<OsString> {
    vec![
        "--output".into(),
        output.into(),
        "--observer".into(),
        observer.into(),
    ]
}

fn summary(output: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(output.join("summary.json")).unwrap()).unwrap()
}

#[test]
fn both_observers_validate_real_durable_tool_turns() {
    let root = tempfile::tempdir().unwrap();
    for observer in ["wait", "store"] {
        let output = root.path().join(observer);
        diagnostic::run(arguments(&output, observer)).unwrap();
        let result = summary(&output);
        assert_eq!(result["valid"], true, "{result:#}");
        let run = &result["runs"][0];
        assert_eq!(run["invariants"]["exact_transcript"], true);
        assert_eq!(run["invariants"]["tool_invocations"], 1);
        assert_eq!(run["invariants"]["publisher_lag_after"], 0);
        assert!(run["window"]["wall_ns"].as_u64().unwrap() > 0);
        if cfg!(target_os = "linux") {
            assert!(run["window"]["thread_cpu_ns"].as_u64().unwrap() > 0);
        }
        let retained = std::fs::read(output.join("summary.json")).unwrap();
        assert!(diagnostic::run(arguments(&output, observer)).is_err());
        assert_eq!(
            std::fs::read(output.join("summary.json")).unwrap(),
            retained
        );
    }
}

#[test]
fn pending_provider_retains_invalid_result_and_returns_failure() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("censored");
    let mut arguments = arguments(&output, "wait");
    arguments.extend(["--diagnostic-timeout-secs".into(), "1".into()]);
    // This provider cannot complete regardless of host speed. The public executable's
    // ordinary turn remains unchanged; only this integration fixture selects Hang.
    assert!(
        diagnostic::run_replies(
            arguments,
            vec![swarmie_agent_loop_fixture::model::Reply::Hang]
        )
        .is_err()
    );
    let result = summary(&output);
    assert_eq!(result["valid"], false);
    assert_eq!(result["runs"][0]["error"]["reason"], "diagnostic_timeout");
    assert_eq!(result["diagnostic_not_ci_acceptance"], true);
    assert!(output.join("run-0001/result.json").is_file());
}
