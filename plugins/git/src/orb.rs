//! Optional managed composition. Native work never blocks the local executor.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use futures::{FutureExt, channel::oneshot, future::Either};
use tembra::{Code, Context, Orb, Service};

use crate::{Cancellation, GitService, WireRequest, WireResponse};
#[cfg(feature = "tools")]
use crate::{Request, Response};

pub const SERVICE: &str = "swarmie.git";
pub const INSTANCE_KEY: &str = "main";

/// Independent native calls are bounded; this is admission, not a task scheduler.
const MAX_WORKERS: usize = 8;
const MAX_REPLY_BYTES: usize = 48 * 1024;

struct Environment {
    git: GitService,
    workers: Arc<AtomicUsize>,
}

struct WorkerPermit(Arc<AtomicUsize>);

impl Drop for WorkerPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Expose the independently usable capability as an ordinary compiled Orb.
///
/// The embedding supplies the Git instance, execution authority and journal. Calls
/// use their authenticated context principal. Tembra owns invocation lifetime;
/// cancellation requests native termination and still waits for worker settlement.
pub fn orb(git: GitService) -> Orb {
    let workers = Arc::new(AtomicUsize::new(0));
    let service = Service::new(SERVICE, move |declaration| {
        if declaration.address()?.key != [INSTANCE_KEY] {
            return Err(tembra::Error::new(
                Code::Forbidden,
                "swarmie.git.instance",
                "Git uses the main key",
            ));
        }
        Ok(Environment {
            git: git.clone(),
            workers: workers.clone(),
        })
    })
    .method(
        "request",
        |environment, context, (request,): (WireRequest,)| async move {
            dispatch(&environment, &context, request).await
        },
    );
    #[cfg(feature = "tools")]
    let service = tool_methods(service);
    Orb::new(SERVICE, 1).service(service)
}

async fn dispatch(
    environment: &Environment,
    context: &Context,
    request: WireRequest,
) -> tembra::Result<WireResponse> {
    let principal = context.principal()?;
    let response = worker(environment, context, move |git, cancellation| {
        git.execute(&principal, request, cancellation)
    })
    .await?;
    bounded_response(response)
}

fn reply_size(value: &impl serde::Serialize) -> tembra::Result<usize> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|error| unavailable(error.to_string()))
}

fn oversized(message: impl Into<String>) -> tembra::Error {
    tembra::Error::new(Code::Exhausted, "swarmie.git.reply_limit", message.into())
}

fn compact_receipt(mut receipt: crate::Receipt) -> tembra::Result<WireResponse> {
    use sha2::Digest;
    let reconciliation = receipt
        .result
        .as_ref()
        .and_then(|value| value.get("reconciliation"))
        .and_then(|value| serde_json::from_value::<crate::RecoveryInspection>(value.clone()).ok());
    receipt.result = None;
    if reply_size(&receipt)? <= MAX_REPLY_BYTES - 128 {
        return Ok(WireResponse::Core(Box::new(crate::Response::Receipt {
            receipt: Box::new(receipt),
        })));
    }
    let message = receipt
        .message
        .chars()
        .rev()
        .take(1024)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    let summary = crate::RecoveryEntry {
        operation_digest: format!(
            "{:x}",
            sha2::Sha256::digest(
                serde_json::to_vec(&receipt.operation)
                    .map_err(|error| unavailable(error.to_string()))?
            )
        ),
        operation_id: receipt.operation_id,
        principal: receipt.principal,
        target: receipt.target,
        state: receipt.state,
        message: format!("Full admitted arguments remain in the node journal. {message}"),
        head: receipt.head,
    };
    let response = WireResponse::Managed(crate::ManagedResponse::ReceiptSummary {
        receipt: Box::new(summary),
        reconciliation: reconciliation.map(Box::new),
    });
    if reply_size(&response)? > MAX_REPLY_BYTES {
        return Err(oversized(
            "The captured operation identity exceeds the transport limit. Its journal remains retained; do not replay it.",
        ));
    }
    Ok(response)
}

fn bounded_response(response: WireResponse) -> tembra::Result<WireResponse> {
    if reply_size(&response)? <= MAX_REPLY_BYTES {
        return Ok(response);
    }
    let receipt = match response {
        WireResponse::Core(response) => match *response {
            crate::Response::Receipt { receipt } => Some(*receipt),
            _ => None,
        },
        WireResponse::Managed(crate::ManagedResponse::Worktree { receipt, .. })
        | WireResponse::Managed(crate::ManagedResponse::Archive { receipt, .. }) => Some(*receipt),
        _ => None,
    }.ok_or_else(|| oversized(
        "Git result exceeds the transport limit; choose a narrower query. Pagination is unavailable for this result.",
    ))?;
    compact_receipt(receipt)
}

async fn worker<T: Send + 'static>(
    environment: &Environment,
    context: &Context,
    action: impl FnOnce(&GitService, &Cancellation) -> crate::Result<T> + Send + 'static,
) -> tembra::Result<T> {
    context.check()?;
    let signal = context.signal();
    signal.check()?;
    environment
        .workers
        .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < MAX_WORKERS).then_some(count + 1)
        })
        .map_err(|_| {
            tembra::Error::new(
                Code::Exhausted,
                "swarmie.git.workers",
                "Git has reached its concurrent execution limit",
            )
        })?;
    let permit = WorkerPermit(environment.workers.clone());
    let git = environment.git.clone();
    let cancellation = Cancellation::default();
    let native_cancellation = cancellation.clone();
    let (send, receive) = oneshot::channel();
    let thread = std::thread::Builder::new()
        .name("swarmie-git".into())
        .spawn(move || {
            let result = action(&git, &native_cancellation);
            drop(permit);
            let _ = send.send(result);
        })
        .map_err(|error| unavailable(error.to_string()))?;

    // A cancelled logical wait never detaches its native work. In particular,
    // confirmed mutation receipts remain owned and recoverable after cancellation.
    let result = match futures::future::select(receive, signal.cancelled().boxed_local()).await {
        Either::Left((result, _)) => result,
        Either::Right(((), receive)) => {
            cancellation.cancel();
            receive.await
        }
    };
    thread
        .join()
        .map_err(|_| unavailable("Git worker panicked; operation outcome requires recovery"))?;
    result
        .map_err(|_| unavailable("Git worker stopped; operation outcome requires recovery"))?
        .map_err(map_error)
}

fn unavailable(message: impl Into<String>) -> tembra::Error {
    tembra::Error::new(Code::Unavailable, "swarmie.git.execution", message.into())
}

fn map_error(error: crate::Error) -> tembra::Error {
    let code = match &error {
        crate::Error::Denied(_) => Code::Forbidden,
        crate::Error::Invalid(_) => Code::Invalid,
        crate::Error::Stale => Code::Conflict,
        crate::Error::Cancelled => Code::Cancelled,
        crate::Error::OutputLimit => Code::Exhausted,
        _ => Code::Unavailable,
    };
    tembra::Error::new(code, "swarmie.git.execution", error.to_string())
}

#[cfg(feature = "tools")]
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolMutation {
    target: crate::RepositoryTarget,
    operation: crate::Operation,
}

#[cfg(feature = "tools")]
fn tool_request(call: &swarmie_tools::ToolCall, operation_id: &str) -> tembra::Result<Request> {
    match call.name.as_str() {
        "git_inspect" => {
            let request: Request = serde_json::from_value(call.arguments.clone())
                .map_err(|error| map_error(crate::Error::Invalid(error.to_string())))?;
            if matches!(
                request,
                Request::Mutate { .. } | Request::Recover { .. } | Request::Reconcile { .. }
            ) {
                return Err(map_error(crate::Error::Denied(
                    "git_inspect accepts repository reads only".into(),
                )));
            }
            Ok(request)
        }
        "git_mutate" => {
            let mutation: ToolMutation = serde_json::from_value(call.arguments.clone())
                .map_err(|error| map_error(crate::Error::Invalid(error.to_string())))?;
            Ok(Request::Mutate {
                operation_id: operation_id.into(),
                target: mutation.target,
                operation: mutation.operation,
            })
        }
        _ => Err(map_error(crate::Error::Denied("unknown Git tool".into()))),
    }
}

#[cfg(feature = "tools")]
fn tool_methods(service: Service<Environment>) -> Service<Environment> {
    use swarmie_tools::{OperationId, Recovery, ToolCall, ToolDecision, ToolResult, ToolSpec};

    service
        .method(
            "validate",
            |environment, context, (spec, call): (ToolSpec, ToolCall)| async move {
                if spec.name != call.name {
                    return Ok(ToolDecision::Block("Git tool specification differs".into()));
                }
                let request = match tool_request(&call, "validation") {
                    Ok(request) => request,
                    Err(error) => return Ok(ToolDecision::Block(error.to_string())),
                };
                let principal = context.principal()?;
                match worker(&environment, &context, move |git, _| {
                    git.git().validate(&principal, &request)
                })
                .await
                {
                    Ok(()) => Ok(ToolDecision::Allow),
                    Err(error) if error.code == Code::Forbidden => {
                        Ok(ToolDecision::Block(error.to_string()))
                    }
                    Err(error) => Err(error),
                }
            },
        )
        .method(
            "execute",
            |environment, context, (operation, call): (OperationId, ToolCall)| async move {
                let request = match tool_request(&call, &operation.to_string()) {
                    Ok(request) => request,
                    Err(error) => return Ok(ToolResult::Blocked(error.to_string())),
                };
                let principal = context.principal()?;
                match worker(&environment, &context, move |git, cancellation| {
                    Ok(git.git().execute(&principal, request, cancellation))
                })
                .await?
                {
                    Ok(response) => tool_result(response),
                    Err(error @ crate::Error::Denied(_)) => {
                        Ok(ToolResult::Blocked(error.to_string()))
                    }
                    Err(error @ crate::Error::Command { .. }) => {
                        Ok(ToolResult::Failed(error.to_string()))
                    }
                    Err(error) => Err(map_error(error)),
                }
            },
        )
        .method(
            "recover",
            |environment, context, (operation, call): (OperationId, ToolCall)| async move {
                let request = tool_request(&call, &operation.to_string())?;
                let Request::Mutate {
                    operation_id,
                    target,
                    operation,
                } = request
                else {
                    // Reads have no retained receipt. Lack of one does not establish
                    // safe replay for another provider or a changed execution target.
                    let principal = context.principal()?;
                    worker(&environment, &context, move |git, _| {
                        git.git().validate(&principal, &request)
                    })
                    .await?;
                    return Ok(Recovery::<ToolResult>::Unknown);
                };
                let principal = context.principal()?;
                let response = worker(&environment, &context, move |git, cancellation| {
                    git.git().recover_matching(
                        &principal,
                        &operation_id,
                        &target,
                        &operation,
                        cancellation,
                    )
                    .map(|receipt| Response::Receipt { receipt: Box::new(receipt) })
                })
                .await?;
                if matches!(&response, Response::Receipt { receipt } if matches!(receipt.state, crate::Outcome::Unknown | crate::Outcome::InProgress))
                {
                    return Ok(Recovery::Unknown);
                }
                Ok(Recovery::Completed(tool_result(response)?))
            },
        )
}

#[cfg(feature = "tools")]
fn tool_result(response: Response) -> tembra::Result<swarmie_tools::ToolResult> {
    use swarmie_tools::ToolResult;
    let response = bounded_response(WireResponse::Core(Box::new(response)))?;
    let outcome = match &response {
        WireResponse::Core(response) => match response.as_ref() {
            Response::Receipt { receipt } => Some((receipt.state, receipt.message.as_str())),
            _ => None,
        },
        WireResponse::Managed(crate::ManagedResponse::ReceiptSummary { receipt, .. }) => {
            Some((receipt.state, receipt.message.as_str()))
        }
        WireResponse::Managed(_) => None,
    };
    if let Some((state, message)) = outcome {
        match state {
            crate::Outcome::Failed => return Ok(ToolResult::Failed(message.into())),
            crate::Outcome::Unknown | crate::Outcome::InProgress => {
                return Err(unavailable(
                    "Git outcome is unknown; inspect the retained operation before retrying",
                ));
            }
            crate::Outcome::Completed => {}
        }
    }
    serde_json::to_value(response)
        .map(ToolResult::Success)
        .map_err(|error| unavailable(error.to_string()))
}

/// Metadata for the embedding's Tools catalog. Registration never grants authority.
///
/// Tool mutation IDs come from the pending Tools execution, never model arguments.
#[cfg(feature = "tools")]
pub fn tool_definitions() -> tembra::Result<Vec<swarmie_tools::ToolDefinition>> {
    use serde_json::json;
    use swarmie_tools::{ToolDefinition, ToolName, ToolProvider, ToolSpec};

    let provider = ToolProvider::new(SERVICE, vec![INSTANCE_KEY.into()])?;
    let target = json!({
        "type": "object",
        "required": ["root", "git_dir", "common_dir", "bare", "head", "branch", "fingerprint", "index_fingerprint", "observation_error"],
        "properties": {
            "root": {"type": ["string", "null"]}, "git_dir": {"type": "string"},
            "common_dir": {"type": "string"}, "bare": {"type": "boolean"},
            "head": {"type": ["string", "null"]}, "branch": {"type": ["string", "null"]},
            "fingerprint": {"type": "string"}, "index_fingerprint": {"type": "string"}, "observation_error": {"enum": [null, "output_limit"]}
        }, "additionalProperties": false
    });
    let mut reads = vec![json!({
        "type": "object", "required": ["kind", "path"],
        "properties": {"kind": {"enum": ["discover", "identify"]}, "path": {"type": "string"}},
        "additionalProperties": false
    })];
    for kind in ["status", "remotes", "files"] {
        let mut properties = json!({"kind": {"const": kind}, "target": target});
        if kind != "remotes" {
            properties["page"] = json!({
                "type": "object", "required": ["offset", "digest"],
                "properties": {
                    "offset": {"type": "integer", "minimum": 1},
                    "digest": {"type": "string", "minLength": 1}
                }, "additionalProperties": false
            });
        }
        reads.push(json!({"type": "object", "required": ["kind", "target"],
            "properties": properties, "additionalProperties": false}));
    }
    reads.push(json!({"type": "object", "required": ["kind", "target", "limit"],
        "properties": {"kind": {"const": "history"}, "target": target, "limit": {"type": "integer", "minimum": 1}},
        "additionalProperties": false}));
    reads.push(json!({"type": "object", "required": ["kind", "target", "path", "staged", "mode"],
        "properties": {"kind": {"const": "diff"}, "target": target, "path": {"type": ["string", "null"]},
            "staged": {"type": "boolean"}, "mode": {"enum": ["unified", "split"]}}, "additionalProperties": false}));
    reads.push(
        json!({"type": "object", "required": ["kind", "target", "path", "revision"],
        "properties": {"kind": {"const": "file"}, "target": target, "path": {"type": "string"},
            "revision": {"type": ["string", "null"]}}, "additionalProperties": false}),
    );
    let mut mutations = Vec::new();
    for kind in ["stage", "unstage"] {
        mutations.push(json!({"type": "object", "required": ["kind", "paths"],
            "properties": {"kind": {"const": kind}, "paths": {"type": "array", "items": {"type": "string"}}},
            "additionalProperties": false}));
    }
    mutations.push(json!({"type": "object", "required": ["kind", "message", "reviewed_index", "run_hooks"],
        "properties": {"kind": {"const": "commit"}, "message": {"type": "string"}, "reviewed_index": {"type": "string"}, "run_hooks": {"type": "boolean", "description": "Whether to run native commit hooks. Choose explicitly; false disables hooks for this invocation only."}},
        "additionalProperties": false}));
    mutations.push(json!({"type": "object", "required": ["kind", "name"],
        "properties": {"kind": {"const": "create_branch"}, "name": {"type": "string"}}, "additionalProperties": false}));
    mutations.push(json!({"type": "object", "required": ["kind", "remote"],
        "properties": {"kind": {"const": "fetch"}, "remote": {"type": "string"}}, "additionalProperties": false}));
    mutations.push(json!({"type": "object", "required": ["kind", "remote", "source_oid", "destination_ref"],
        "properties": {"kind": {"const": "push"}, "remote": {"type": "string"}, "source_oid": {"type": "string"},
            "destination_ref": {"type": "string"}}, "additionalProperties": false}));
    [("git_inspect", "Inspect the captured repository without modifying Git state.", json!({"type":"object", "oneOf": reads})),
        ("git_mutate", "Apply one reviewed Git operation to its captured checkout.", json!({"type": "object",
            "required": ["target", "operation"], "properties": {"target": target, "operation": {"oneOf": mutations}},
            "additionalProperties": false}))]
        .into_iter()
        .map(|(name, description, input_schema)| {
            ToolDefinition::new(ToolSpec {
                name: ToolName::new(name)?, description: description.into(), input_schema,
            }, provider.clone())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_success_preserves_confirmed_receipt_identity_and_intent() {
        let target = crate::RepositoryTarget {
            root: Some("repository".into()),
            git_dir: "repository/.git".into(),
            common_dir: "repository/.git".into(),
            bare: false,
            head: Some("a".repeat(40)),
            branch: None,
            fingerprint: "observed".into(),
            index_fingerprint: "reviewed".into(),
            observation_error: None,
        };
        let operation = crate::Operation::Stage {
            paths: vec!["src/main.rs".into()],
        };
        let receipt = crate::Receipt {
            result: Some(serde_json::json!({"output": "x".repeat(MAX_REPLY_BYTES)})),
            operation: operation.clone(),
            operation_id: "confirmed-operation".into(),
            principal: "alice".into(),
            target,
            state: crate::Outcome::Completed,
            message: "confirmed".into(),
            head: Some("a".repeat(40)),
        };
        let response = bounded_response(WireResponse::Core(Box::new(crate::Response::Receipt {
            receipt: Box::new(receipt),
        })))
        .unwrap();
        assert!(reply_size(&response).unwrap() <= MAX_REPLY_BYTES);
        let WireResponse::Core(response) = response else {
            panic!("receipt must stay core")
        };
        let crate::Response::Receipt { receipt } = *response else {
            panic!("known outcome must stay inspectable")
        };
        assert_eq!(receipt.operation_id, "confirmed-operation");
        assert_eq!(receipt.principal, "alice");
        assert_eq!(receipt.state, crate::Outcome::Completed);
        assert!(receipt.result.is_none());
        assert_eq!(
            serde_json::to_value(receipt.operation).unwrap(),
            serde_json::to_value(operation).unwrap()
        );
    }

    #[test]
    fn oversized_admitted_arguments_have_a_truthful_bounded_recovery_projection() {
        let target = crate::RepositoryTarget {
            root: Some("repo".into()),
            git_dir: "repo/.git".into(),
            common_dir: "repo/.git".into(),
            bare: false,
            head: None,
            branch: Some("main".into()),
            fingerprint: "seen".into(),
            index_fingerprint: "index".into(),
            observation_error: None,
        };
        let receipt = crate::Receipt {
            operation_id: "large-admission".into(),
            principal: "owner".into(),
            target,
            operation: crate::Operation::Stage {
                paths: (0..2000)
                    .map(|index| format!("{index}-{}", "path".repeat(20)))
                    .collect(),
            },
            state: crate::Outcome::Unknown,
            message: "Read-only inspection did not prove the previous effect.".into(),
            head: None,
            result: None,
        };
        let original = serde_json::to_vec(&receipt.operation).unwrap();
        assert!(original.len() > MAX_REPLY_BYTES);
        let response = bounded_response(WireResponse::Core(Box::new(crate::Response::Receipt {
            receipt: Box::new(receipt),
        })))
        .unwrap();
        let bytes = serde_json::to_vec(&response).unwrap();
        assert!(bytes.len() < MAX_REPLY_BYTES);
        let decoded: WireResponse = serde_json::from_slice(&bytes).unwrap();
        match decoded {
            WireResponse::Managed(crate::ManagedResponse::ReceiptSummary { receipt, .. }) => {
                assert_eq!(receipt.operation_id, "large-admission");
                assert_eq!(receipt.state, crate::Outcome::Unknown);
                assert!(!receipt.operation_digest.is_empty());
                assert!(receipt.message.contains("did not prove"));
            }
            other => panic!("expected a bounded summary {other:?}"),
        }
        assert!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["receipt"]
                .get("operation")
                .is_none()
        );
    }
}
