use process::ManagedProcess;
use std::{path::Path, time::Duration};
use tokio::io::{AsyncBufReadExt, BufReader};

#[cfg(unix)]
#[tokio::test]
async fn private_native_terminal_accepts_input_without_echo_and_stops_its_owned_process() {
    use tokio::io::AsyncWriteExt;
    let (server, mut input, output) = ManagedProcess::spawn_with_terminal(
        Path::new("python3"),
        &["-c".into(), "import os,sys,time; assert os.isatty(0) and os.isatty(1); print('terminal-ready',flush=True); assert sys.stdin.readline().strip()=='private-fixture-code'; print('received',flush=True); time.sleep(60)".into()],
        &std::env::current_dir().unwrap(), &[],
    ).await.unwrap();
    let mut lines = BufReader::new(output).lines();
    let line = tokio::time::timeout(Duration::from_secs(3), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(line.trim(), "terminal-ready");
    input.write_all(b"private-fixture-code\r").await.unwrap();
    input.flush().await.unwrap();
    let line = tokio::time::timeout(Duration::from_secs(3), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(line.trim(), "received");
    server.kill().await.unwrap();
    assert!(server.exited());
    assert!(!server.exit_status().unwrap().success());
}

#[tokio::test]
async fn captured_server_ready_line_preserves_environment_and_supervised_shutdown() {
    let (server, stderr) = ManagedProcess::spawn_with_stderr(
        Path::new("python3"),
        &[
            "-c".into(),
            "import os,sys,time; print('ready '+os.environ['SWARMIE_SERVER_FIXTURE'],file=sys.stderr,flush=True); time.sleep(60)".into(),
        ],
        &std::env::current_dir().unwrap(),
        &[("SWARMIE_SERVER_FIXTURE".into(), "child-only".into())],
    )
    .await
    .unwrap();
    let mut lines = BufReader::new(stderr).lines();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .as_deref(),
        Some("ready child-only")
    );
    assert!(!server.exited());
    assert!(std::env::var_os("SWARMIE_SERVER_FIXTURE").is_none());
    server.kill().await.unwrap();
    assert!(server.exited());
    assert!(
        tokio::time::timeout(Duration::from_secs(3), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .is_none()
    );
}
