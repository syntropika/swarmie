#![cfg(unix)]
use adapters::{ClaudeHarness, CodexHarness, PiHarness};
use swarmie::*;
use serde_json::{Value, json};
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("swarmie-account-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn query(&self, provider: &str) -> AccountQuery {
        let mut query = AccountQuery::new(provider, &self.0);
        let peer = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/account_quotas.py")
            .canonicalize()
            .unwrap();
        query.config = json!({"executable":"python3","args":[peer]});
        query.execution.config_home = Some(self.0.join(provider));
        query
            .execution
            .env
            .insert("SWARMIE_QUOTA_PROVIDER".into(), provider.into());
        query.execution.env.insert(
            "SWARMIE_QUOTA_LEDGER".into(),
            self.0
                .join(format!("{provider}.jsonl"))
                .to_string_lossy()
                .into(),
        );
        query
    }
    fn ledger(&self, provider: &str) -> Vec<Value> {
        std::fs::read_to_string(self.0.join(format!("{provider}.jsonl")))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[tokio::test]
async fn account_queries_use_native_controls_without_sessions_turns_or_events() {
    let fixture = Fixture::new();
    let runtime = Runtime::builder()
        .harness(CodexHarness)
        .harness(ClaudeHarness)
        .build();
    let mut events = runtime.events();
    for (provider, used) in [("codex", 25.0), ("claude", 40.0)] {
        let query = fixture.query(provider);
        assert!(
            !serde_json::to_value(&query).unwrap()["execution"]
                .as_object()
                .unwrap()
                .contains_key("env")
        );
        let quotas = runtime.account_quotas(query).await.unwrap();
        assert_eq!(quotas["windows"][0]["usedPercent"], used);
        let ledger = fixture.ledger(provider);
        let methods: Vec<_> = ledger[1..]
            .iter()
            .map(|frame| frame["method"].as_str().unwrap())
            .collect();
        assert_eq!(
            methods,
            if provider == "codex" {
                vec!["initialize", "initialized", "account/rateLimits/read"]
            } else {
                vec!["initialize", "get_usage"]
            }
        );
        assert_eq!(ledger[0]["home"], json!(fixture.0.join(provider)));
        if provider == "claude" {
            let args = ledger[0]["argv"].as_array().unwrap();
            assert!(args.contains(&json!("--no-session-persistence")));
            assert!(args.contains(&json!("--bare")));
            assert!(!args.contains(&json!("--session-id")));
        }
    }
    assert!(runtime.sessions().await.unwrap().is_empty());
    assert!(matches!(
        events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}
#[tokio::test]
async fn aggregate_retains_order_and_unsupported_and_failed_accounts() {
    let fixture = Fixture::new();
    let runtime = Runtime::builder()
        .harness(CodexHarness)
        .harness(ClaudeHarness)
        .harness(PiHarness)
        .build();
    let mut empty = fixture.query("claude");
    empty
        .execution
        .env
        .insert("SWARMIE_QUOTA_EMPTY".into(), "1".into());
    let mut failed = fixture.query("codex");
    failed
        .execution
        .env
        .insert("SWARMIE_QUOTA_FAIL".into(), "1".into());
    let results = runtime
        .account_quotas_all(vec![
            fixture.query("codex"),
            empty,
            AccountQuery::new("pi", &fixture.0),
            failed,
            AccountQuery::new("unregistered", &fixture.0),
        ])
        .await
        .unwrap();
    assert!(
        matches!(&results[0], AccountQuotaResult::Available { harness, .. } if harness == "codex")
    );
    assert!(
        matches!(&results[1], AccountQuotaResult::Unsupported { harness, .. } if harness == "claude")
    );
    assert!(
        matches!(&results[2], AccountQuotaResult::Unsupported { harness, .. } if harness == "pi")
    );
    assert!(
        matches!(&results[3], AccountQuotaResult::Error { harness, message } if harness == "codex" && message.contains("account unavailable"))
    );
    assert!(
        matches!(&results[4], AccountQuotaResult::Error { harness, .. } if harness == "unregistered")
    );
    assert!(runtime.account_quotas_all(vec![]).await.is_err());
    assert!(runtime.sessions().await.unwrap().is_empty());
}
