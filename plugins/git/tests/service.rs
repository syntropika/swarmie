use std::{path::Path, process::Command, sync::Arc};

use swarmie_git::{
    Authority, Cancellation, DirectoryJournal, Git, GitService, GitSettings, Intent,
    ManagedRequest, ManagedResponse, NativeProcess, Request, Response, Result, SetupScript,
    WireRequest, WireResponse, WorktreeRequest,
};

struct Grant;
impl Authority for Grant {
    fn authorize(&self, principal: &str, _: &Intent) -> Result<()> {
        if principal == "owner" {
            Ok(())
        } else {
            Err(swarmie_git::Error::Denied("not granted".into()))
        }
    }
    fn authorize_discovery(&self, principal: &str, _: &Path) -> Result<()> {
        self.authorize(
            principal,
            &Intent {
                target: unreachable_target(),
                operation: swarmie_git::IntentOperation::Discover,
            },
        )
    }
}
fn unreachable_target() -> swarmie_git::RepositoryTarget {
    swarmie_git::RepositoryTarget {
        root: None,
        git_dir: String::new(),
        common_dir: String::new(),
        bare: false,
        head: None,
        branch: None,
        fingerprint: String::new(),
        index_fingerprint: String::new(),
        observation_error: None,
    }
}
fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn setup() -> (tempfile::TempDir, GitService, swarmie_git::RepositoryTarget) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "test@example.invalid"]);
    git(&root, &["config", "user.name", "Test"]);
    std::fs::write(root.join("file"), "baseline\n").unwrap();
    git(&root, &["add", "file"]);
    git(&root, &["commit", "-qm", "test: baseline"]);
    let core = Git::new(
        Arc::new(NativeProcess),
        Arc::new(DirectoryJournal::open(directory.path().join("journal")).unwrap()),
        Arc::new(Grant),
    );
    let target = discover(&core, &root);
    let service = GitService::new(core, directory.path().join("state")).unwrap();
    (directory, service, target)
}
fn request(service: &GitService, value: ManagedRequest) -> WireResponse {
    service
        .execute(
            "owner",
            WireRequest::Managed(value),
            &Cancellation::default(),
        )
        .unwrap()
}

#[test]
fn preferences_survive_reopen_and_cannot_configure_another_system() {
    let (directory, service, target) = setup();
    let settings = GitSettings {
        branch_prefix: "feat/".into(),
        ..GitSettings::default()
    };
    request(
        &service,
        ManagedRequest::SettingsSet {
            operation_id: "save".into(),
            target: target.clone(),
            project_id: Some("project".into()),
            settings,
        },
    );
    let reopened = GitService::new(service.git().clone(), directory.path().join("state")).unwrap();
    let response = request(
        &reopened,
        ManagedRequest::SettingsGet {
            target: target.clone(),
            project_id: Some("project".into()),
        },
    );
    let WireResponse::Managed(ManagedResponse::Settings {
        settings,
        execution_platform,
    }) = response
    else {
        panic!("wrong response")
    };
    assert_eq!(settings.branch_prefix, "feat/");
    assert_eq!(execution_platform, std::env::consts::OS);
    let mut invalid = settings;
    invalid.setup_by_platform.insert(
        "another-system".into(),
        SetupScript {
            enabled: true,
            shell: "sh".into(),
            arguments: vec!["-c".into()],
            script: "true".into(),
            working_directory: ".".into(),
        },
    );
    let rejected = reopened
        .execute(
            "owner",
            WireRequest::Managed(ManagedRequest::SettingsSet {
                operation_id: "invalid".into(),
                target,
                project_id: Some("project".into()),
                settings: invalid,
            }),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(
        matches!(rejected, WireResponse::Core(response) if matches!(&*response, Response::Receipt { receipt } if receipt.state == swarmie_git::Outcome::Failed))
    );
}

#[test]
fn detached_worktree_runs_saved_setup_once_and_retains_failed_preparation() {
    let (directory, service, target) = setup();
    let mut settings = GitSettings::default();
    settings.set_setup(
        std::env::consts::OS.into(),
        SetupScript {
            enabled: true,
            shell: "sh".into(),
            arguments: vec!["-c".into()],
            script: "printf marker >> setup-marker; exit 7".into(),
            working_directory: ".".into(),
        },
    );
    request(
        &service,
        ManagedRequest::SettingsSet {
            operation_id: "save".into(),
            target: target.clone(),
            project_id: None,
            settings,
        },
    );
    let managed = directory.path().join("managed");
    std::fs::create_dir(&managed).unwrap();
    let destination = managed.join("new");
    let create = ManagedRequest::WorktreeCreate {
        operation_id: "create".into(),
        target: target.clone(),
        project_id: None,
        request: WorktreeRequest {
            managed_root: managed,
            destination: destination.clone(),
            commit: target.head.clone().unwrap(),
            include_paths: Vec::new(),
        },
    };
    for _ in 0..2 {
        let response = request(&service, create.clone());
        let WireResponse::Managed(ManagedResponse::Worktree { worktree, receipt }) = response
        else {
            panic!("wrong response")
        };
        assert!(worktree.target.branch.is_none());
        assert!(worktree.preparation_error.unwrap().contains('7'));
        assert_eq!(receipt.operation_id, "create");
    }
    assert_eq!(
        std::fs::read_to_string(destination.join("setup-marker")).unwrap(),
        "marker"
    );
    let recovered = service
        .execute(
            "owner",
            WireRequest::Core(Request::Recover {
                operation_id: "create".into(),
            }),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(
        matches!(recovered, WireResponse::Core(response) if matches!(*response, Response::Receipt { .. }))
    );
    assert!(
        service
            .execute(
                "stranger",
                WireRequest::Core(Request::Recover {
                    operation_id: "create".into()
                }),
                &Cancellation::default()
            )
            .is_err()
    );
}

#[test]
fn archive_round_trip_is_separate_from_cleanup_permission() {
    let (directory, service, initial) = setup();
    let root = Path::new(initial.root.as_ref().unwrap());
    std::fs::write(root.join("file"), "staged\n").unwrap();
    git(root, &["add", "file"]);
    std::fs::write(root.join("file"), "unstaged\n").unwrap();
    std::fs::write(root.join("local"), "untracked\n").unwrap();
    let target = discover(service.git(), root);
    request(
        &service,
        ManagedRequest::ArchiveCreate {
            operation_id: "archive".into(),
            target: target.clone(),
            archive_id: "snapshot".into(),
        },
    );
    let managed = directory.path().join("managed");
    std::fs::create_dir(&managed).unwrap();
    let destination = managed.join("restore");
    let response = request(
        &service,
        ManagedRequest::ArchiveRestore {
            operation_id: "restore".into(),
            target: target.clone(),
            archive_id: "snapshot".into(),
            managed_root: managed,
            destination: destination.clone(),
        },
    );
    assert!(
        matches!(response, WireResponse::Managed(ManagedResponse::Archive { archive, .. }) if archive.restore_verified)
    );
    assert_eq!(
        std::fs::read_to_string(destination.join("file")).unwrap(),
        "unstaged\n"
    );
    assert_eq!(
        std::fs::read_to_string(destination.join("local")).unwrap(),
        "untracked\n"
    );
    let check = request(
        &service,
        ManagedRequest::CleanupCheck {
            target,
            archive_id: "snapshot".into(),
        },
    );
    assert!(
        matches!(check, WireResponse::Managed(ManagedResponse::Cleanup { eligibility }) if !eligibility.eligible)
    );
    assert!(root.exists());
}

#[test]
fn forged_settings_identity_and_abandoned_temporary_files_do_not_bypass_binding() {
    let (directory, service, target) = setup();
    request(
        &service,
        ManagedRequest::SettingsSet {
            operation_id: "first-save".into(),
            target: target.clone(),
            project_id: None,
            settings: GitSettings::default(),
        },
    );
    let saved = std::fs::read_dir(directory.path().join("state/settings"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(saved.with_extension("pending"), b"interrupted save").unwrap();
    request(
        &service,
        ManagedRequest::SettingsSet {
            operation_id: "second-save".into(),
            target: target.clone(),
            project_id: None,
            settings: GitSettings {
                branch_prefix: "task/".into(),
                ..GitSettings::default()
            },
        },
    );
    let mut forged = target;
    forged.common_dir = directory
        .path()
        .join("another-repository/.git")
        .to_str()
        .unwrap()
        .into();
    assert!(
        service
            .execute(
                "owner",
                WireRequest::Managed(ManagedRequest::SettingsGet {
                    target: forged,
                    project_id: None
                }),
                &Cancellation::default()
            )
            .is_err()
    );
}

#[test]
fn destination_validation_failure_has_a_recoverable_failed_receipt() {
    let (directory, service, target) = setup();
    let managed = directory.path().join("managed");
    std::fs::create_dir(&managed).unwrap();
    let response = request(
        &service,
        ManagedRequest::WorktreeCreate {
            operation_id: "invalid-directory".into(),
            target: target.clone(),
            project_id: None,
            request: WorktreeRequest {
                managed_root: managed.clone(),
                destination: managed.join("missing-parent/new"),
                commit: target.head.unwrap(),
                include_paths: Vec::new(),
            },
        },
    );
    assert!(
        matches!(response, WireResponse::Core(response) if matches!(&*response, Response::Receipt {receipt} if receipt.state == swarmie_git::Outcome::Failed))
    );
    let recovered = service
        .execute(
            "owner",
            WireRequest::Core(Request::Recover {
                operation_id: "invalid-directory".into(),
            }),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(
        matches!(recovered, WireResponse::Core(response) if matches!(&*response, Response::Receipt {receipt} if receipt.state == swarmie_git::Outcome::Failed))
    );
    assert!(!managed.join("missing-parent").exists());
}

#[test]
fn oversized_identity_keeps_settings_readable_and_never_authorizes_mutations() {
    use std::{io::Write, process::Stdio};
    let (directory, service, original) = setup();
    let root = directory.path().join("repo");
    let output = Command::new("git")
        .args(["rev-parse", "HEAD:file"])
        .current_dir(&root)
        .output()
        .unwrap();
    let oid = String::from_utf8(output.stdout).unwrap().trim().to_owned();
    let mut child = Command::new("git")
        .args(["update-index", "--index-info"])
        .current_dir(&root)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut inventory = String::new();
    for index in 0..40_000 {
        inventory.push_str(&format!(
            "100644 {oid}\t{index:05}-{}.txt\n",
            "filename".repeat(10)
        ));
    }
    assert!(inventory.len() > 4 * 1024 * 1024);
    input.write_all(inventory.as_bytes()).unwrap();
    drop(input);
    assert!(child.wait().unwrap().success());
    let target = discover(service.git(), &root);
    assert_eq!(
        target.observation_error,
        Some(swarmie_git::ObservationError::OutputLimit)
    );
    assert!(target.fingerprint.is_empty());
    assert!(matches!(
        request(
            &service,
            ManagedRequest::SettingsGet {
                target: target.clone(),
                project_id: None
            }
        ),
        WireResponse::Managed(ManagedResponse::Settings { .. })
    ));
    assert!(matches!(
        service.git().execute(
            "owner",
            Request::Status {
                target: target.clone(),
                page: None
            },
            &Cancellation::default()
        ),
        Err(swarmie_git::Error::OutputLimit)
    ));
    assert_refused(&service, target.clone(), "limited-observation");
    let mut forged = target.clone();
    forged.observation_error = None;
    forged.fingerprint = "forged".into();
    forged.index_fingerprint = "forged".into();
    assert_refused(&service, forged, "forged-observation");
    let response = service
        .git()
        .execute(
            "owner",
            Request::Mutate {
                operation_id: "oversized-refusal".into(),
                target,
                operation: swarmie_git::Operation::CreateBranch {
                    name: "must-not-exist".into(),
                },
            },
            &Cancellation::default(),
        )
        .unwrap();
    assert!(
        matches!(response, Response::Receipt { receipt } if receipt.state == swarmie_git::Outcome::Failed)
    );
    assert!(!root.join(".git/refs/heads/must-not-exist").exists());
    assert!(!root.join(".git/index.lock").exists());
    assert_refused(&service, original, "outdated-observation");
}

fn discover(core: &Git, root: &Path) -> swarmie_git::RepositoryTarget {
    match core
        .execute(
            "owner",
            Request::Discover {
                path: root.to_string_lossy().into(),
            },
            &Cancellation::default(),
        )
        .unwrap()
    {
        Response::Repository {
            repository: Some(target),
        } => target,
        response => panic!("expected repository: {response:?}"),
    }
}

fn assert_refused(service: &GitService, target: swarmie_git::RepositoryTarget, id: &str) {
    let response = service
        .git()
        .execute(
            "owner",
            Request::Mutate {
                operation_id: id.into(),
                target,
                operation: swarmie_git::Operation::CreateBranch {
                    name: "must-not-exist".into(),
                },
            },
            &Cancellation::default(),
        )
        .unwrap();
    assert!(
        matches!(response, Response::Receipt { receipt } if receipt.state == swarmie_git::Outcome::Failed)
    );
}

#[test]
fn conversation_retention_defers_archiving_until_execution_settles_and_survives_reopen() {
    use swarmie_git::{ArchiveState, ConversationHoldUpdate};
    let (directory, service, target) = setup();
    let update = |pinned, archived, active| ConversationHoldUpdate {
        conversation_id: "chat".into(),
        pinned,
        archived,
        active,
    };
    let hold = service
        .set_conversation_hold(
            "owner",
            &target,
            update(true, false, true),
            &Cancellation::default(),
        )
        .unwrap();
    assert!(hold.pinned && hold.active);
    assert_eq!(hold.archive_state, None);
    let hold = service
        .set_conversation_hold(
            "owner",
            &target,
            update(true, true, true),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(hold.archive_state, Some(ArchiveState::Deferred));
    assert!(hold.warning.is_some());
    let hold = service
        .set_conversation_hold(
            "owner",
            &target,
            update(true, true, false),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(hold.archive_state, Some(ArchiveState::Completed));
    assert!(hold.warning.is_none());
    let archive_id = hold.archive_id.clone();
    let reopened = GitService::new(service.git().clone(), directory.path().join("state")).unwrap();
    let hold = reopened
        .set_conversation_hold(
            "owner",
            &target,
            update(false, true, false),
            &Cancellation::default(),
        )
        .unwrap();
    assert_eq!(hold.archive_id, archive_id);
    assert_eq!(hold.generation, 1);
    let response = request(
        &reopened,
        ManagedRequest::ArchiveList {
            target: target.clone(),
            page: None,
        },
    );
    assert!(
        matches!(response,WireResponse::Managed(ManagedResponse::Archives{archives,unavailable,..}) if archives.len()==1 && unavailable.is_empty())
    );
    assert!(
        reopened
            .set_conversation_hold(
                "other",
                &target,
                update(false, false, false),
                &Cancellation::default()
            )
            .is_err()
    );
}

#[test]
fn interrupted_conversation_capture_is_retained_without_automatic_replay() {
    use swarmie_git::{ArchiveState, ConversationHoldUpdate};
    let (directory, service, target) = setup();
    let update = || ConversationHoldUpdate {
        conversation_id: "chat".into(),
        pinned: false,
        archived: true,
        active: false,
    };
    service
        .set_conversation_hold("owner", &target, update(), &Cancellation::default())
        .unwrap();
    let entry = std::fs::read_dir(directory.path().join("state/conversation-holds"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut hold: swarmie_git::ConversationHold =
        serde_json::from_slice(&std::fs::read(&entry).unwrap()).unwrap();
    hold.archive_state = Some(ArchiveState::Pending);
    std::fs::write(&entry, serde_json::to_vec(&hold).unwrap()).unwrap();
    let reopened = GitService::new(service.git().clone(), directory.path().join("state")).unwrap();
    let current = reopened
        .set_conversation_hold("owner", &target, update(), &Cancellation::default())
        .unwrap();
    assert_eq!(current.archive_state, Some(ArchiveState::Unknown));
    assert_eq!(current.generation, 1);
    assert!(current.warning.unwrap().contains("not replayed"));
}

#[test]
fn capability_probe_reports_the_complete_interface_without_granting_a_repository() {
    let (_directory, service, _) = setup();
    let response = request(&service, ManagedRequest::CapabilityInfo);
    assert!(
        matches!(response,WireResponse::Managed(ManagedResponse::CapabilityInfo{api_version:1,execution_available:true,operations}) if operations.contains(&"capability_info".into()) && operations.contains(&"operations".into()))
    );
    assert!(
        service
            .execute(
                "",
                WireRequest::Managed(ManagedRequest::CapabilityInfo),
                &Cancellation::default()
            )
            .is_err()
    );
}

#[test]
fn an_unborn_archive_is_rejected_before_private_snapshot_effects() {
    let (directory, service, _) = setup();
    let root = directory.path().join("unborn");
    std::fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    let target = discover(service.git(), &root);
    let response = request(
        &service,
        ManagedRequest::ArchiveCreate {
            operation_id: "unborn".into(),
            target,
            archive_id: "unborn".into(),
        },
    );
    assert!(
        matches!(response,WireResponse::Core(value) if matches!(&*value,Response::Receipt{receipt} if receipt.state==swarmie_git::Outcome::Failed))
    );
    assert_eq!(
        std::fs::read_dir(directory.path().join("state/archives"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn archive_catalog_paginates_both_valid_and_incomplete_entries_under_the_frame_budget() {
    let (directory, service, target) = setup();
    let archives = directory.path().join("state/archives");
    for index in 0..350 {
        std::fs::create_dir(archives.join(format!("{index:064x}"))).unwrap();
    }
    let mut page = None;
    let mut count = 0;
    let mut pages = 0;
    loop {
        let response = request(
            &service,
            ManagedRequest::ArchiveList {
                target: target.clone(),
                page,
            },
        );
        assert!(serde_json::to_vec(&response).unwrap().len() < 32 * 1024);
        match response {
            WireResponse::Managed(ManagedResponse::Archives {
                archives,
                unavailable,
                next_page,
            }) => {
                assert!(archives.is_empty());
                count += unavailable.len();
                pages += 1;
                if next_page.is_none() {
                    break;
                }
                page = next_page;
            }
            other => panic!("unexpected catalog {other:?}"),
        }
    }
    assert_eq!(count, 350);
    assert!(pages > 1);
}
