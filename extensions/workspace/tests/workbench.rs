use protocol::RuntimeService;
use serde_json::{Value, json};
use std::time::Duration;
use swarmie::{Runtime, RuntimeExtension};
use swarmie_workspace::WorkspaceExtension;
async fn invoke(service: &RuntimeService, command: &str, args: Value) -> swarmie::Result<Value> {
    service
        .invoke(
            "extension/invoke",
            json!({"extensionId":"workspace","command":command,"arguments":args}),
        )
        .await
}
#[tokio::test]
async fn public_extension_supports_non_git_files_and_confines_paths() {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("hello.txt"), "before\n").unwrap();
    let service = RuntimeService::new(
        Runtime::builder()
            .extension(WorkspaceExtension::new())
            .build(),
    );
    let extensions = service.invoke("extension/list", json!({})).await.unwrap();
    assert!(
        extensions[0]["views"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["kind"] == "files")
    );
    let project = invoke(&service, "project.open", json!({"path":folder.path()}))
        .await
        .unwrap();
    let id = &project["id"];
    let files = invoke(&service, "files.list", json!({"projectId":id}))
        .await
        .unwrap();
    assert_eq!(files["entries"][0]["path"], "hello.txt");
    assert_eq!(
        invoke(
            &service,
            "files.read",
            json!({"projectId":id,"path":"hello.txt"})
        )
        .await
        .unwrap()["content"],
        "before\n"
    );
    invoke(
        &service,
        "files.write",
        json!({"projectId":id,"path":"new.txt","content":"after\n"}),
    )
    .await
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(folder.path().join("new.txt")).unwrap(),
        "after\n"
    );
    assert!(
        invoke(
            &service,
            "files.read",
            json!({"projectId":id,"path":"../outside"})
        )
        .await
        .is_err()
    );
    assert!(
        invoke(
            &service,
            "files.write",
            json!({"projectId":id,"path":"/tmp/outside","content":"bad"})
        )
        .await
        .is_err()
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc/passwd", folder.path().join("link")).unwrap();
        assert!(
            invoke(
                &service,
                "files.read",
                json!({"projectId":id,"path":"link"})
            )
            .await
            .is_err()
        );
        assert!(
            invoke(
                &service,
                "files.write",
                json!({"projectId":id,"path":"link","content":"bad"})
            )
            .await
            .is_err()
        );
    }
}
#[tokio::test]
async fn real_pty_accepts_input_resize_and_reports_exit() {
    let folder = tempfile::tempdir().unwrap();
    let extension = WorkspaceExtension::new();
    let project = extension
        .invoke("project.open", json!({"path":folder.path()}))
        .await
        .unwrap();
    let terminal = extension
        .invoke(
            "terminal.start",
            json!({"projectId":project["id"],"program":"/bin/sh"}),
        )
        .await
        .unwrap();
    let id = &terminal["terminalId"];
    extension
        .invoke(
            "terminal.resize",
            json!({"terminalId":id,"cols":110,"rows":35}),
        )
        .await
        .unwrap();
    extension.invoke("terminal.write",json!({"terminalId":id,"data":"printf 'PTY_OK\\n'; test -t 0 && printf 'INTERACTIVE_OK\\n'; exit 7\n"})).await.unwrap();
    let mut text = String::new();
    let mut next = 0;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let result = extension
                .invoke("terminal.read", json!({"terminalId":id,"after":next}))
                .await
                .unwrap();
            text.push_str(result["text"].as_str().unwrap());
            next = result["next"].as_u64().unwrap();
            if result["exited"] == true && result["exitCode"].as_u64() == Some(7) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(text.contains("PTY_OK") && text.contains("INTERACTIVE_OK"));
    extension
        .invoke("terminal.close", json!({"terminalId":id}))
        .await
        .unwrap();
    assert!(
        extension
            .invoke("terminal.read", json!({"terminalId":id}))
            .await
            .is_err()
    );
}
#[tokio::test]
async fn discovered_project_action_runs_real_test_command() {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join("package.json"),
        r#"{"scripts":{"test":"node -e \"console.log('PROJECT_TEST_OK')\""}}"#,
    )
    .unwrap();
    let extension = WorkspaceExtension::new();
    let project = extension
        .invoke("project.open", json!({"path":folder.path()}))
        .await
        .unwrap();
    let actions = extension
        .invoke("actions.list", json!({"projectId":project["id"]}))
        .await
        .unwrap();
    assert_eq!(actions["actions"][0]["id"], "script.test");
    let terminal = extension
        .invoke(
            "actions.run",
            json!({"projectId":project["id"],"actionId":"script.test"}),
        )
        .await
        .unwrap();
    let mut text = String::new();
    let mut next = 0;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let result = extension
                .invoke(
                    "terminal.read",
                    json!({"terminalId":terminal["terminalId"],"after":next}),
                )
                .await
                .unwrap();
            text.push_str(result["text"].as_str().unwrap());
            next = result["next"].as_u64().unwrap();
            if result["exited"] == true {
                assert_eq!(result["exitCode"], 0);
                break;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .unwrap();
    assert!(text.contains("PROJECT_TEST_OK"));
    extension
        .invoke(
            "terminal.close",
            json!({"terminalId":terminal["terminalId"]}),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn terminal_drains_large_final_output_before_exited() {
    let folder = tempfile::tempdir().unwrap();
    let extension = WorkspaceExtension::new();
    let project = extension
        .invoke("project.open", json!({"path":folder.path()}))
        .await
        .unwrap();
    let terminal=extension.invoke("terminal.start",json!({"projectId":project["id"],"program":"/bin/sh","args":["-c","head -c 262144 /dev/zero | tr '\\000' X; printf 'FINAL_MARKER'" ]})).await.unwrap();
    let mut next = 0;
    let mut text = String::new();
    let mut chunks = 0;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let result = extension
                .invoke(
                    "terminal.read",
                    json!({"terminalId":terminal["terminalId"],"after":next}),
                )
                .await
                .unwrap();
            let chunk = result["text"].as_str().unwrap();
            assert!(chunk.len() <= 65536);
            text.push_str(chunk);
            if !chunk.is_empty() {
                chunks += 1;
            }
            next = result["next"].as_u64().unwrap();
            if result["exited"] == true {
                assert_eq!(result["exitCode"], 0);
                assert_eq!(result["next"], result["availableEnd"]);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(text.len(), 262144 + "FINAL_MARKER".len());
    assert!(text.ends_with("FINAL_MARKER"));
    assert!(chunks >= 4);
    extension
        .invoke(
            "terminal.close",
            json!({"terminalId":terminal["terminalId"]}),
        )
        .await
        .unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn closing_pty_kills_separate_background_job_groups() {
    let folder = tempfile::tempdir().unwrap();
    let extension = WorkspaceExtension::new();
    let project = extension
        .invoke("project.open", json!({"path":folder.path()}))
        .await
        .unwrap();
    let terminal=extension.invoke("terminal.start",json!({"projectId":project["id"],"program":"/bin/sh","args":["-c","set -m; sleep 60 & echo BACKGROUND_PID=$!; wait"]})).await.unwrap();
    let mut text = String::new();
    let mut after = 0;
    let pid = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let result = extension
                .invoke(
                    "terminal.read",
                    json!({"terminalId":terminal["terminalId"],"after":after}),
                )
                .await
                .unwrap();
            text.push_str(result["text"].as_str().unwrap());
            after = result["next"].as_u64().unwrap();
            if let Some(line) = text
                .lines()
                .find_map(|line| line.strip_prefix("BACKGROUND_PID="))
                && let Ok(pid) = line.trim().parse::<i32>()
            {
                break pid;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    extension
        .invoke(
            "terminal.close",
            json!({"terminalId":terminal["terminalId"]}),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = std::fs::read_to_string(format!("/proc/{pid}/stat"));
            if status.is_err() || status.unwrap().split_whitespace().nth(2) == Some("Z") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn projectless_chat_directory_registers_file_access_and_validates_custom_paths() {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("note.txt"), "chat context").unwrap();
    let service = RuntimeService::new(
        Runtime::builder()
            .extension(WorkspaceExtension::new())
            .build(),
    );
    let directory = invoke(
        &service,
        "conversation.directory",
        json!({"path":folder.path()}),
    )
    .await
    .unwrap();
    assert_eq!(
        directory["path"],
        json!(folder.path().canonicalize().unwrap())
    );
    assert_eq!(
        invoke(
            &service,
            "files.read",
            json!({"projectId":directory["id"],"path":"note.txt"})
        )
        .await
        .unwrap()["content"],
        "chat context"
    );
    for path in [
        std::path::PathBuf::from("relative"),
        folder.path().join("missing"),
        folder.path().join("note.txt"),
    ] {
        assert!(
            invoke(&service, "conversation.directory", json!({"path":path}))
                .await
                .is_err()
        );
    }
    assert!(!folder.path().join("missing").exists());
}

#[tokio::test]
async fn contextual_file_commands_use_retained_directory_and_block_replacements() {
    let parent = tempfile::tempdir().unwrap();
    let alternate = tempfile::tempdir().unwrap();
    let folder = parent.path().join("folder");
    std::fs::create_dir(&folder).unwrap();
    let runtime = Runtime::builder()
        .harness(swarmie::mock::MockHarness::new(Default::default()))
        .extension(WorkspaceExtension::new())
        .build();
    let saved = runtime
        .spawn(swarmie::SpawnOptions::new("mock", &folder))
        .await
        .unwrap()
        .snapshot()
        .await;
    let project = runtime
        .invoke_extension(
            "workspace",
            "project.open",
            json!({"path":folder,"sessionId":saved.id}),
        )
        .await
        .unwrap();
    runtime.invoke_extension("workspace", "files.write", json!({"projectId":project["id"],"sessionId":saved.id,"path":"owned.txt","content":"original"})).await.unwrap();
    runtime
        .invoke_extension(
            "workspace",
            "project.open",
            json!({"path":alternate.path()}),
        )
        .await
        .unwrap();
    assert!(runtime.invoke_extension("workspace", "files.write", json!({"projectId":alternate.path(),"sessionId":saved.id,"path":"wrong.txt","content":"blocked"})).await.is_err());
    std::fs::rename(&folder, parent.path().join("original")).unwrap();
    std::fs::create_dir(&folder).unwrap();
    assert!(runtime.invoke_extension("workspace", "files.write", json!({"projectId":project["id"],"sessionId":saved.id,"path":"replacement.txt","content":"blocked"})).await.is_err());
    assert!(!folder.join("replacement.txt").exists());
    assert!(!alternate.path().join("wrong.txt").exists());
    runtime.transcript(&saved.id, 0, 100).await.unwrap();
    runtime.shutdown().await.unwrap();
}
