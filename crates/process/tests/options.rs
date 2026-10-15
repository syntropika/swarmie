use process::{JsonProcess, ProcessOptions, WireRecording, WireStyle};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, time::Duration};
fn cwd() -> std::path::PathBuf {
    std::env::current_dir().unwrap()
}
const ECHO: &str = "import json,sys,os\nfor line in sys.stdin:\n r=json.loads(line)\n print(json.dumps({'id':r['id'],'result':{'echo':r.get('params',{}),'environment':os.environ.get('SWARMIE_PROCESS_FIXTURE')}}),flush=True)";
#[tokio::test]
async fn spawn_environment_is_real_isolated_and_validated() {
    let env = BTreeMap::from([("SWARMIE_PROCESS_FIXTURE".into(), "child-only-value".into())]);
    let (process, _events) = JsonProcess::spawn_with_env(
        Path::new("python3"),
        &["-c".into(), ECHO.into()],
        &cwd(),
        WireStyle::JsonRpc,
        &env,
    )
    .await
    .unwrap();
    assert_eq!(
        process.request("read", json!({})).await.unwrap()["environment"],
        "child-only-value"
    );
    assert!(std::env::var_os("SWARMIE_PROCESS_FIXTURE").is_none());
    process.close().await.unwrap();
    for env in [
        BTreeMap::from([("bad=name".into(), "value".into())]),
        BTreeMap::from([("NAME".into(), "bad\0value".into())]),
        BTreeMap::from([("".into(), "value".into())]),
    ] {
        assert!(
            JsonProcess::spawn_with_env(
                Path::new("python3"),
                &[],
                &cwd(),
                WireStyle::JsonRpc,
                &env
            )
            .await
            .is_err()
        );
    }
}
#[tokio::test]
async fn recording_redacts_both_directions_and_stops_at_budget_nonfatally() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("wire.jsonl");
    let options = ProcessOptions {
        environment: BTreeMap::from([(
            "SWARMIE_PROCESS_FIXTURE".into(),
            "private-env-secret".into(),
        )]),
        recording: Some(WireRecording {
            path: path.clone(),
            max_bytes: 1800,
        }),
        stall_timeout: None,
    };
    let (process, mut events) = JsonProcess::spawn_with_options(
        Path::new("python3"),
        &["-c".into(), ECHO.into()],
        &cwd(),
        WireStyle::JsonRpc,
        &options,
    )
    .await
    .unwrap();
    let result=process.request("echo",json!({"api_key":"private-key","password":"private-password","headers":{"X-Credential":"private-header"},"text":"echo private-env-secret","usage":{"inputTokens":10}})).await.unwrap();
    assert_eq!(result["echo"]["api_key"], "private-key");
    for _ in 0..8 {
        assert!(
            process
                .request("echo", json!({"text":"large ordinary content".repeat(40)}))
                .await
                .is_ok()
        );
    }
    let warning = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = events.recv().await.unwrap();
            if event["_runtime_diagnostic"]["code"] == "wire_recording_full" {
                break event;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(warning["_runtime_diagnostic"]["level"], "warning");
    process.close().await.unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() <= 1800);
    let text = String::from_utf8(bytes).unwrap();
    for secret in [
        "private-env-secret",
        "private-key",
        "private-password",
        "private-header",
    ] {
        assert!(!text.contains(secret));
    }
    let records = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(records.iter().any(|r| r["direction"] == "send"));
    assert!(records.iter().any(|r| r["direction"] == "receive"));
    assert_eq!(records[0]["frame"]["params"]["usage"]["inputTokens"], 10);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
#[tokio::test]
async fn invalid_recording_is_a_visible_spawn_error_and_does_not_overwrite() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("existing");
    std::fs::write(&path, "keep me").unwrap();
    let options = ProcessOptions {
        recording: Some(WireRecording {
            path: path.clone(),
            max_bytes: 100,
        }),
        ..Default::default()
    };
    assert!(
        JsonProcess::spawn_with_options(
            Path::new("python3"),
            &[],
            &cwd(),
            WireStyle::JsonRpc,
            &options
        )
        .await
        .is_err()
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), "keep me");
}
#[tokio::test]
async fn outstanding_request_stalls_nonfatally_recovers_and_idle_does_not_warn() {
    let options = ProcessOptions {
        stall_timeout: Some(Duration::from_millis(60)),
        ..Default::default()
    };
    let program = "import json,sys,time\nfor line in sys.stdin:\n r=json.loads(line)\n if r['method']=='slow': time.sleep(.2)\n print(json.dumps({'id':r['id'],'result':{'ok':True}}),flush=True)";
    let (process, mut events) = JsonProcess::spawn_with_options(
        Path::new("python3"),
        &["-c".into(), program.into()],
        &cwd(),
        WireStyle::JsonRpc,
        &options,
    )
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(150), events.recv())
            .await
            .is_err()
    );
    let active = process.clone();
    let request = tokio::spawn(async move { active.request("slow", json!({})).await });
    let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event["_runtime_diagnostic"]["code"], "wire_stalled");
    assert_eq!(request.await.unwrap().unwrap()["ok"], true);
    assert_eq!(
        process.request("fast", json!({})).await.unwrap()["ok"],
        true
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(150), events.recv())
            .await
            .is_err()
    );
    process.set_active(true);
    let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event["_runtime_diagnostic"]["code"], "wire_stalled");
    process.set_active(false);
    assert!(
        tokio::time::timeout(Duration::from_millis(150), events.recv())
            .await
            .is_err()
    );
    process.close().await.unwrap();
}
