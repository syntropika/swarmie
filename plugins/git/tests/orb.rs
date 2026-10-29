#![cfg(feature = "tembra")]

use std::{
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::Poll,
    time::Duration,
};

use futures::{FutureExt, executor::LocalPool};
use swarmie_git::{
    Authority, Cancellation, CommandOutput, CommandSpec, DirectoryJournal, Git, GitService, Intent,
    NativeProcess, ProcessExecutor, Request, Response, WireRequest, WireResponse,
};
use tembra::{Code, Host, Reference};

struct PrincipalAuthority;

impl Authority for PrincipalAuthority {
    fn authorize_discovery(&self, principal: &str, _: &Path) -> swarmie_git::Result<()> {
        if principal == "alice" {
            Ok(())
        } else {
            Err(swarmie_git::Error::Denied(
                "principal is not admitted".into(),
            ))
        }
    }

    fn authorize(&self, principal: &str, _: &Intent) -> swarmie_git::Result<()> {
        if principal == "alice" {
            Ok(())
        } else {
            Err(swarmie_git::Error::Denied(
                "principal is not admitted".into(),
            ))
        }
    }
}

fn fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["config", "user.name", "Fixture"],
        vec!["config", "user.email", "fixture@example.invalid"],
        vec![
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "test: initialize fixture",
        ],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(directory.path())
                .status()
                .unwrap()
                .success()
        );
    }
    directory
}

fn capability(process: Arc<dyn ProcessExecutor>, journal: &Path) -> GitService {
    let git = Git::new(
        process,
        Arc::new(DirectoryJournal::open(journal).unwrap()),
        Arc::new(PrincipalAuthority),
    );
    GitService::new(git, journal.join("managed")).unwrap()
}

fn reference(host: &Host, principal: &str) -> Rc<Reference> {
    Rc::new(
        host.get(
            swarmie_git::SERVICE,
            vec![swarmie_git::INSTANCE_KEY.into()],
            principal,
        )
        .unwrap(),
    )
}

fn call<A: 'static, R: 'static>(
    pool: &mut LocalPool,
    host: &Host,
    principal: &str,
    method: &'static str,
    arguments: A,
) -> tembra::Result<R> {
    pool.run_until_stalled();
    let reference = reference(host, principal);
    pool.run_until(host.root(principal, move |context| {
        async move {
            if method == "request" {
                reference.call(&context, method, (arguments,)).await
            } else {
                reference.call(&context, method, arguments).await
            }
        }
        .boxed_local()
    }))
}

fn core_response(response: WireResponse) -> Response {
    match response {
        WireResponse::Core(response) => *response,
        WireResponse::Managed(_) => panic!("expected a core response"),
    }
}

#[test]
fn ordinary_host_calls_use_authenticated_principal() {
    let repository = fixture();
    let journal = tempfile::tempdir().unwrap();
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    host.install(vec![swarmie_git::orb(capability(
        Arc::new(NativeProcess),
        journal.path(),
    ))])
    .unwrap();
    let request = WireRequest::Core(Request::Discover {
        path: repository.path().to_str().unwrap().into(),
    });
    let found: WireResponse = call(&mut pool, &host, "alice", "request", request.clone()).unwrap();
    assert!(matches!(
        core_response(found),
        Response::Repository {
            repository: Some(_)
        }
    ));
    let refused =
        call::<_, WireResponse>(&mut pool, &host, "mallory", "request", request).unwrap_err();
    assert_eq!(refused.code, Code::Forbidden);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn response_limit_counts_serialized_bytes() {
    let repository = fixture();
    // JSON escapes these bytes, so a bounded native preview can still exceed RPC.
    std::fs::write(repository.path().join("escaped.txt"), vec![1; 16 * 1024]).unwrap();
    let journal = tempfile::tempdir().unwrap();
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    host.install(vec![swarmie_git::orb(capability(
        Arc::new(NativeProcess),
        journal.path(),
    ))])
    .unwrap();
    let found: WireResponse = call(
        &mut pool,
        &host,
        "alice",
        "request",
        WireRequest::Core(Request::Discover {
            path: repository.path().to_str().unwrap().into(),
        }),
    )
    .unwrap();
    let Response::Repository {
        repository: Some(target),
    } = core_response(found)
    else {
        panic!("missing fixture")
    };
    let result = call::<_, WireResponse>(
        &mut pool,
        &host,
        "alice",
        "request",
        WireRequest::Core(Request::File {
            target,
            path: "escaped.txt".into(),
            revision: None,
        }),
    )
    .unwrap_err();
    assert_eq!(result.code, Code::Exhausted);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn paginated_inventories_fit_the_real_orb_reply_limit() {
    let repository = fixture();
    for index in 0..1000 {
        std::fs::write(
            repository
                .path()
                .join(format!("{index:04}-{}.txt", "filename".repeat(8))),
            "",
        )
        .unwrap();
    }
    let journal = tempfile::tempdir().unwrap();
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    host.install(vec![swarmie_git::orb(capability(
        Arc::new(NativeProcess),
        journal.path(),
    ))])
    .unwrap();
    let found: WireResponse = call(
        &mut pool,
        &host,
        "alice",
        "request",
        WireRequest::Core(Request::Discover {
            path: repository.path().to_str().unwrap().into(),
        }),
    )
    .unwrap();
    let Response::Repository {
        repository: Some(target),
    } = core_response(found)
    else {
        panic!("fixture repository")
    };
    for status in [false, true] {
        let mut page = None;
        let mut count = 0;
        let mut pages = 0;
        loop {
            let request = if status {
                Request::Status {
                    target: target.clone(),
                    page,
                }
            } else {
                Request::Files {
                    target: target.clone(),
                    page,
                }
            };
            let response: WireResponse = call(
                &mut pool,
                &host,
                "alice",
                "request",
                WireRequest::Core(request),
            )
            .unwrap();
            assert!(serde_json::to_vec(&response).unwrap().len() < 48 * 1024);
            let next = match core_response(response) {
                Response::Files { paths, next_page } => {
                    count += paths.len();
                    next_page
                }
                Response::Status { status, next_page } => {
                    count += status.entries.len();
                    next_page
                }
                _ => panic!("inventory response"),
            };
            pages += 1;
            page = next;
            if page.is_none() {
                break;
            }
        }
        assert!(pages > 1);
        assert_eq!(count, 1000);
    }
    pool.run_until(host.close()).unwrap();
}

struct SettlingProcess {
    started: Arc<AtomicBool>,
    settled: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}

impl ProcessExecutor for SettlingProcess {
    fn run(
        &self,
        _: &CommandSpec,
        cancellation: &Cancellation,
    ) -> swarmie_git::Result<CommandOutput> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        self.started.store(true, Ordering::Release);
        while !cancellation.is_cancelled() {
            std::thread::sleep(Duration::from_millis(1));
        }
        // Simulate termination acknowledgement arriving after cancellation.
        std::thread::sleep(Duration::from_millis(20));
        self.settled.store(true, Ordering::Release);
        Err(swarmie_git::Error::Cancelled)
    }
}

#[test]
fn host_closure_waits_for_cancelled_native_work_and_executor_stays_responsive() {
    let journal = tempfile::tempdir().unwrap();
    let started = Arc::new(AtomicBool::new(false));
    let settled = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let process = Arc::new(SettlingProcess {
        started: started.clone(),
        settled: settled.clone(),
        calls: calls.clone(),
    });
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    host.install(vec![swarmie_git::orb(capability(process, journal.path()))])
        .unwrap();
    let reference = reference(&host, "alice");
    let observed = settled.clone();
    let result: tembra::Result<WireResponse> = pool.run_until(host.root("alice", move |context| {
        async move {
            let cancel = async {
                futures::future::poll_fn(|task| {
                    if started.load(Ordering::Acquire) {
                        Poll::Ready(())
                    } else {
                        task.waker().wake_by_ref();
                        Poll::Pending
                    }
                })
                .await;
                context.cancel().unwrap();
            };
            let request = reference.call(
                &context,
                "request",
                (WireRequest::Core(Request::Discover {
                    path: PathBuf::from(".").to_string_lossy().into_owned(),
                }),),
            );
            let (result, ()) = futures::join!(request, cancel);
            result
        }
        .boxed_local()
    }));
    assert_eq!(result.unwrap_err().code, Code::Cancelled);
    pool.run_until(host.close()).unwrap();
    assert!(observed.load(Ordering::Acquire));
    assert_eq!(calls.load(Ordering::Acquire), 1);
}

#[cfg(feature = "tools")]
#[test]
fn tools_keep_host_operation_identity_and_authorize_receipt_recovery() {
    use swarmie_git::{Operation, Outcome};
    use swarmie_tools::{OperationId, Recovery, ToolCall, ToolCallId, ToolName, ToolResult};

    let repository = fixture();
    let journal = tempfile::tempdir().unwrap();
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    host.install(vec![swarmie_git::orb(capability(
        Arc::new(NativeProcess),
        journal.path(),
    ))])
    .unwrap();
    let found: WireResponse = call(
        &mut pool,
        &host,
        "alice",
        "request",
        WireRequest::Core(Request::Discover {
            path: repository.path().to_str().unwrap().into(),
        }),
    )
    .unwrap();
    let Response::Repository {
        repository: Some(target),
    } = core_response(found)
    else {
        panic!("missing fixture")
    };
    let operation_id = OperationId::new();
    let args = ToolCall {
        id: ToolCallId::new("call-1").unwrap(),
        name: ToolName::new("git_mutate").unwrap(),
        arguments: serde_json::json!({"target": target, "operation": Operation::CreateBranch { name: "test/managed".into() }}),
    };
    let result: ToolResult = call(
        &mut pool,
        &host,
        "alice",
        "execute",
        (operation_id, args.clone()),
    )
    .unwrap();
    let ToolResult::Success(value) = result else {
        panic!("mutation failed")
    };
    let Response::Receipt { receipt } = serde_json::from_value(value).unwrap() else {
        panic!("missing receipt")
    };
    assert_eq!(receipt.operation_id, operation_id.to_string());
    assert_eq!(receipt.principal, "alice");
    assert_eq!(receipt.state, Outcome::Completed);
    let recovered: Recovery<ToolResult> = call(
        &mut pool,
        &host,
        "alice",
        "recover",
        (operation_id, args.clone()),
    )
    .unwrap();
    assert!(matches!(
        recovered,
        Recovery::Completed(ToolResult::Success(_))
    ));
    let denied = call::<_, Recovery<ToolResult>>(
        &mut pool,
        &host,
        "mallory",
        "recover",
        (operation_id, args.clone()),
    )
    .unwrap_err();
    assert_eq!(denied.code, Code::Forbidden);
    let mut substituted = args;
    substituted.arguments["operation"] =
        serde_json::json!({"kind": "create_branch", "name": "test/substitution"});
    let refused = call::<_, Recovery<ToolResult>>(
        &mut pool,
        &host,
        "alice",
        "recover",
        (operation_id, substituted),
    );
    assert!(refused.is_err());
    pool.run_until(host.close()).unwrap();
}

#[cfg(feature = "tools")]
#[test]
fn pending_tool_receipt_stays_unknown_and_inspect_cannot_dispatch_a_mutation() {
    use swarmie_git::{Journal, Operation, PendingOperation};
    use swarmie_tools::{OperationId, Recovery, ToolCall, ToolCallId, ToolName, ToolResult};

    let repository = fixture();
    let journal = tempfile::tempdir().unwrap();
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    host.install(vec![swarmie_git::orb(capability(
        Arc::new(NativeProcess),
        journal.path(),
    ))])
    .unwrap();
    let found: WireResponse = call(
        &mut pool,
        &host,
        "alice",
        "request",
        WireRequest::Core(Request::Discover {
            path: repository.path().to_str().unwrap().into(),
        }),
    )
    .unwrap();
    let Response::Repository {
        repository: Some(target),
    } = core_response(found)
    else {
        panic!("missing fixture")
    };
    let operation_id = OperationId::new();
    let operation = Operation::CreateBranch {
        name: "test/uncertain".into(),
    };
    DirectoryJournal::open(journal.path())
        .unwrap()
        .admit(&PendingOperation {
            operation_id: operation_id.to_string(),
            principal: "alice".into(),
            target: target.clone(),
            operation: operation.clone(),
        })
        .unwrap();
    let args = ToolCall {
        id: ToolCallId::new("pending").unwrap(),
        name: ToolName::new("git_mutate").unwrap(),
        arguments: serde_json::json!({"target": target, "operation": operation}),
    };
    let recovered: Recovery<ToolResult> =
        call(&mut pool, &host, "alice", "recover", (operation_id, args)).unwrap();
    assert!(matches!(recovered, Recovery::Unknown));
    let injected = ToolCall {
        id: ToolCallId::new("inspect-mutation").unwrap(),
        name: ToolName::new("git_inspect").unwrap(),
        arguments: serde_json::to_value(Request::Mutate {
            operation_id: "model-chosen".into(),
            target,
            operation,
        })
        .unwrap(),
    };
    let refused: ToolResult = call(
        &mut pool,
        &host,
        "alice",
        "execute",
        (OperationId::new(), injected),
    )
    .unwrap();
    assert!(matches!(refused, ToolResult::Blocked(_)));
    let branch = Command::new("git")
        .args(["show-ref", "--verify", "refs/heads/test/uncertain"])
        .current_dir(repository.path())
        .output()
        .unwrap();
    assert!(!branch.status.success());
    pool.run_until(host.close()).unwrap();
}
