use process::{JsonProcess, WireStyle};
use serde_json::json;
use std::{path::Path, time::Duration};
#[tokio::test]
async fn clean_exit_drains_final_response_and_preceding_records() {
    let program = "import json,sys\nr=json.loads(sys.stdin.readline())\nfor i in range(800): print(json.dumps({'method':'chunk','params':{'n':i}}))\nprint(json.dumps({'id':r['id'],'result':{'done':True}}),flush=True)";
    for _ in 0..30 {
        let (process, mut events) = JsonProcess::spawn(
            Path::new("python3"),
            &["-c".into(), program.into()],
            &std::env::current_dir().unwrap(),
            WireStyle::JsonRpc,
        )
        .await
        .unwrap();
        let result = process.request("prompt", json!({})).await.unwrap();
        assert_eq!(result["done"], true);
        let mut count = 0;
        tokio::time::timeout(Duration::from_secs(3), async {
            while let Some(frame) = events.recv().await {
                if frame["method"] == "chunk" {
                    count += 1;
                }
                if frame.get("_runtime_exit").is_some() {
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(count, 800);
    }
}
#[tokio::test]
async fn dropping_process_handle_terminates_an_unresponsive_child() {
    let (process, mut events) = JsonProcess::spawn(
        Path::new("python3"),
        &["-c".into(), "import time;time.sleep(60)".into()],
        &std::env::current_dir().unwrap(),
        WireStyle::JsonRpc,
    )
    .await
    .unwrap();
    drop(process);
    let frame = tokio::time::timeout(Duration::from_secs(3), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(frame.get("_runtime_exit").is_some());
}
