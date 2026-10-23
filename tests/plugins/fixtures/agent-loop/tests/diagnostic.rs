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
fn joint_same_owner_turns_preserve_history_and_effect_boundaries_for_both_observers() {
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
        assert_eq!(
            run["invariants"]["source_facts_visible_before_publication"],
            6
        );
        assert_eq!(run["invariants"]["terminal"]["attempts"], 4);
        assert_eq!(run["invariants"]["terminal"]["revision"], 6);
        assert_eq!(run["invariants"]["model_invocations"], 2);
        assert_eq!(run["invariants"]["policy_invocations"], 1);
        assert_eq!(run["invariants"]["terminal_decision_visible"], true);
        assert_eq!(
            run["invariants"]["semantic_kinds"],
            serde_json::json!([
                "swarmie.message",
                "swarmie.agent.turn-started",
                "swarmie.agent.model-request",
                "swarmie.agent.model-actual",
                "swarmie.agent.model-started",
                "swarmie.agent.model-provider-confirmed",
                "swarmie.agent.model-confirmed",
                "swarmie.message",
                "swarmie.agent.tool-intent",
                "swarmie.agent.tool-started",
                "swarmie.agent.tool-confirmed",
                "swarmie.message",
                "swarmie.agent.step-ended",
                "swarmie.agent.model-request",
                "swarmie.agent.model-actual",
                "swarmie.agent.model-started",
                "swarmie.agent.model-provider-confirmed",
                "swarmie.agent.model-confirmed",
                "swarmie.message",
                "swarmie.agent.step-ended",
                "swarmie.agent.turn-ended",
                "swarmie.agent.task-settled"
            ])
        );
        let persistence = &run["persistence"];
        assert_eq!(persistence["composition"], "unified_same_owner");
        assert_eq!(persistence["database_files"], 1);
        assert_eq!(persistence["journal_mode"], "wal");
        assert_eq!(persistence["synchronous"], 1, "the live owner uses NORMAL");
        assert_eq!(persistence["scheduler_commit"], "task_session");
        assert_eq!(persistence["joint_commit_records"], 6);
        assert_eq!(persistence["joint_explicit_intent_records"], 3);
        let mutations = persistence["joint_mutations"].as_array().unwrap();
        assert_eq!(mutations.len(), 6);
        for (mutation, revision) in mutations.iter().zip(1..=6) {
            assert_eq!(mutation["revision"], revision);
        }
        assert_eq!(persistence["logical_session_batches"], 26);
        assert_eq!(persistence["history_events"], 28);
        let intents = persistence["joint_intents"].as_array().unwrap();
        assert_eq!(intents.len(), 3);
        for (intent, (phase, revision)) in
            intents
                .iter()
                .zip([("model", 3), ("tool", 4), ("model", 5)])
        {
            assert_eq!(intent["phase"], phase);
            assert_eq!(intent["revision"], revision);
            assert_eq!(intent["checkpoint_reference"], intent["batch_id"]);
            assert_eq!(intent["first_sequence"], intent["last_sequence"]);
            assert_eq!(
                intent["append_commit_sequence"].as_u64().unwrap() + 1,
                intent["head"]["commit_sequence"].as_u64().unwrap()
            );
        }
        assert!(output.join("run-0001/runtime.sqlite").is_file());
        for separate in ["tasks.sqlite", "sessions.sqlite", "bindings.sqlite"] {
            assert!(!output.join("run-0001").join(separate).exists());
        }

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

fn load_arguments(output: &Path, scheduled: bool) -> Vec<OsString> {
    let mut args = vec![
        "--output".into(),
        output.into(),
        "--turns".into(),
        "4".into(),
        "--concurrency".into(),
        if scheduled { "1".into() } else { "4".into() },
        "--max-parallel".into(),
        "2".into(),
    ];
    if scheduled {
        args.extend(["--arrival-interval-ms".into(), "1".into()]);
    }
    args
}

#[test]
fn shared_host_load_validates_each_turn_under_interleaving_and_scheduled_arrivals() {
    let root = tempfile::tempdir().unwrap();
    for scheduled in [false, true] {
        let output = root
            .path()
            .join(if scheduled { "scheduled" } else { "closed" });
        diagnostic::run(load_arguments(&output, scheduled)).unwrap();
        let result = summary(&output);
        assert_eq!(result["valid"], true, "{result:#}");
        assert_eq!(result["scope"]["hosts"], 1);
        assert_eq!(result["configuration"]["scheduler_max_parallel"], 2);
        let mut tasks = std::collections::BTreeSet::new();
        let mut expected_revisions = 0_u64;
        for turn in result["warmup"]
            .as_array()
            .unwrap()
            .iter()
            .chain(result["turns"].as_array().unwrap())
        {
            if turn["state"] == "load_shed" {
                continue;
            }
            assert!(tasks.insert(turn["task_id"].as_str().unwrap()));
            assert_eq!(turn["invariants"]["model_invocations"], 2);
            assert_eq!(turn["invariants"]["policy_invocations"], 1);
            assert_eq!(turn["invariants"]["tool_result_consumed_by_model"], true);
            assert_eq!(turn["invariants"]["distinct_ordered_source_facts"], true);
            let terminal = &turn["invariants"]["terminal"];
            let revision = terminal["revision"].as_u64().unwrap();
            let split = turn["invariants"]["split_continuations"].as_u64().unwrap();
            assert_eq!(terminal["attempts"], 4);
            assert!(split <= 3);
            assert_eq!(revision, 6 + split);
            assert_eq!(turn["invariants"]["published_facts"], revision);
            assert_eq!(
                turn["invariants"]["semantic_kinds"]
                    .as_array()
                    .unwrap()
                    .len(),
                22
            );
            let revisions = result["persistence"]["joint_mutations"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|mutation| mutation["task_id"] == terminal["id"])
                .map(|mutation| mutation["revision"].as_u64().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(revisions, (1..=revision).collect::<Vec<_>>());
            expected_revisions += revision;
        }
        assert_eq!(
            result["source_facts_visible_before_publication"],
            expected_revisions
        );
        assert_eq!(
            result["persistence"]["joint_commit_records"],
            expected_revisions
        );
        assert_eq!(
            result["persistence"]["joint_explicit_intent_records"],
            (tasks.len() * 3) as u64
        );
        assert_eq!(result["statistics"]["planned"], 4);
        assert_eq!(result["statistics"]["offered"], 4);
        assert_eq!(
            result["statistics"]["completed"].as_u64().unwrap()
                + result["statistics"]["load_shed"].as_u64().unwrap(),
            4
        );
        assert_eq!(result["publication"]["lag_after"], 0);
        assert_eq!(result["lifecycle"]["premature_generation_drops"], 0);
        assert!(result["phases"]["publication_drain"].is_object());
        assert!(result["phases"]["validation"].is_object());
        assert_eq!(
            result["statistics"]["offered_to_observed_ns"]["p99_has_at_least_100_samples"],
            false
        );
    }
}

#[test]
fn pending_shared_provider_retains_censored_turn_and_closes_the_real_host() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("load-censored");
    let mut args = load_arguments(&output, false);
    args.extend(["--diagnostic-timeout-secs".into(), "1".into()]);
    assert!(
        diagnostic::run_replies(args, vec![swarmie_agent_loop_fixture::model::Reply::Hang])
            .is_err()
    );
    let result = summary(&output);
    assert_eq!(result["valid"], false);
    assert_eq!(result["error"]["phase"], "warmup");
    assert_eq!(result["error"]["reason"], "diagnostic_timeout");
    assert_eq!(result["warmup"][0]["state"], "censored");
    assert_eq!(result["turns"][0]["state"], "not_offered");
    assert!(result["phases"]["close"].is_object());
}
