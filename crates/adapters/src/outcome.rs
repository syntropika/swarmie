//! Translate terminal native facts without treating unfamiliar reasons as success.
use swarmie::TurnOutcome;
use serde_json::Value;

pub(crate) fn acp(reason: Option<&str>) -> TurnOutcome {
    match reason {
        Some("end_turn") => TurnOutcome::Completed,
        Some("cancelled") => TurnOutcome::Cancelled,
        Some("max_tokens" | "max_turn_requests") => TurnOutcome::Limited,
        Some("refusal") => TurnOutcome::Refused,
        _ => TurnOutcome::Unknown,
    }
}

pub(crate) fn codex(turn: &Value, cancelled: bool) -> TurnOutcome {
    match turn["status"].as_str() {
        Some("completed") => TurnOutcome::Completed,
        Some("interrupted") if cancelled => TurnOutcome::Cancelled,
        Some("interrupted") => TurnOutcome::Interrupted,
        Some("failed") => match turn["error"]["codexErrorInfo"].as_str() {
            Some("contextWindowExceeded" | "usageLimitExceeded") => TurnOutcome::Limited,
            _ => TurnOutcome::Failed,
        },
        _ => TurnOutcome::Unknown,
    }
}

pub(crate) fn claude(result: &Value, cancelled: bool) -> TurnOutcome {
    if cancelled {
        return TurnOutcome::Cancelled;
    }
    match result["subtype"].as_str() {
        Some(
            "error_max_turns" | "error_max_budget_usd" | "error_max_structured_output_retries",
        ) => TurnOutcome::Limited,
        _ if result["is_error"] == true => TurnOutcome::Failed,
        Some("success") | None if result["is_error"] == false => TurnOutcome::Completed,
        _ => TurnOutcome::Unknown,
    }
}

pub(crate) fn pi(reason: Option<&str>) -> TurnOutcome {
    match reason {
        Some("stop") => TurnOutcome::Completed,
        Some("length") => TurnOutcome::Limited,
        Some("aborted") => TurnOutcome::Cancelled,
        Some("error") => TurnOutcome::Failed,
        _ => TurnOutcome::Unknown,
    }
}

pub(crate) fn opencode(reason: Option<&str>) -> TurnOutcome {
    match reason {
        Some("stop") => TurnOutcome::Completed,
        Some("length") => TurnOutcome::Limited,
        Some("content-filter") => TurnOutcome::Refused,
        Some("error") => TurnOutcome::Failed,
        _ => TurnOutcome::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_completion_limits_and_refusals_remain_distinct() {
        for (reason, expected) in [
            ("end_turn", TurnOutcome::Completed),
            ("cancelled", TurnOutcome::Cancelled),
            ("max_tokens", TurnOutcome::Limited),
            ("max_turn_requests", TurnOutcome::Limited),
            ("refusal", TurnOutcome::Refused),
            ("future_reason", TurnOutcome::Unknown),
        ] {
            assert_eq!(acp(Some(reason)), expected);
        }
        assert_eq!(acp(None), TurnOutcome::Unknown);
        assert_eq!(pi(Some("length")), TurnOutcome::Limited);
        assert_eq!(pi(Some("aborted")), TurnOutcome::Cancelled);
        assert_eq!(pi(Some("error")), TurnOutcome::Failed);
        assert_eq!(pi(Some("future_reason")), TurnOutcome::Unknown);
        assert_eq!(opencode(Some("length")), TurnOutcome::Limited);
        assert_eq!(opencode(Some("content-filter")), TurnOutcome::Refused);
        assert_eq!(opencode(Some("future_reason")), TurnOutcome::Unknown);
    }

    #[test]
    fn native_errors_and_requested_cancellation_are_not_inferred_from_text() {
        assert_eq!(
            codex(&json!({"status":"interrupted"}), false),
            TurnOutcome::Interrupted
        );
        assert_eq!(
            codex(&json!({"status":"interrupted"}), true),
            TurnOutcome::Cancelled
        );
        assert_eq!(
            codex(&json!({"status":"completed"}), true),
            TurnOutcome::Completed
        );
        assert_eq!(
            codex(
                &json!({"status":"failed","error":{"codexErrorInfo":"contextWindowExceeded"}}),
                false
            ),
            TurnOutcome::Limited
        );
        assert_eq!(
            codex(&json!({"status":"future_status"}), false),
            TurnOutcome::Unknown
        );
        assert_eq!(
            claude(&json!({"subtype":"error_max_turns","is_error":true}), false),
            TurnOutcome::Limited
        );
        assert_eq!(
            claude(
                &json!({"subtype":"error_max_budget_usd","is_error":true}),
                false
            ),
            TurnOutcome::Limited
        );
        assert_eq!(
            claude(
                &json!({"subtype":"error_during_execution","is_error":true}),
                false
            ),
            TurnOutcome::Failed
        );
        assert_eq!(
            claude(&json!({"subtype":"success","is_error":false}), false),
            TurnOutcome::Completed
        );
        assert_eq!(
            claude(&json!({"subtype":"future_reason","is_error":false}), false),
            TurnOutcome::Unknown
        );
    }
}
