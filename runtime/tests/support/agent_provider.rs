//! External receipt fixture: its database survives the entire runtime process.

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;
use std::path::{Path, PathBuf};
use swarmie_agent_loop::*;
use swarmie_providers::{ModelProgress, Progress};
use tembra::{Context, Orb, Service};

pub const MODEL: &str = "fixture.recovery-model";
pub const TOOL: &str = "fixture.recovery-counter";

pub fn initialize(path: &Path) {
    Connection::open(path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE calls(kind TEXT, operation TEXT, payload TEXT);
             CREATE TABLE receipts(operation TEXT PRIMARY KEY, payload TEXT, result TEXT);
             CREATE TABLE counter(value INTEGER);
             INSERT INTO counter VALUES(0);",
        )
        .unwrap();
}

pub fn count(path: &Path, kind: &str) -> u64 {
    Connection::open(path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM calls WHERE kind=?1", [kind], |r| {
            r.get(0)
        })
        .unwrap()
}

pub fn effects(path: &Path) -> u64 {
    Connection::open(path)
        .unwrap()
        .query_row("SELECT value FROM counter", [], |r| r.get(0))
        .unwrap()
}

fn log(path: &Path, kind: &str, operation: OperationId, payload: &str) {
    Connection::open(path)
        .unwrap()
        .execute(
            "INSERT INTO calls VALUES(?1,?2,?3)",
            params![kind, operation.to_string(), payload],
        )
        .unwrap();
}

fn response(
    path: &Path,
    ctx: &Context,
    operation: OperationId,
    request: ModelRequest,
    streaming: bool,
) -> ModelResponse {
    assert_eq!(ctx.principal().unwrap(), "alice");
    log(
        path,
        if streaming {
            "model.stream"
        } else {
            "model.complete"
        },
        operation,
        &serde_json::to_string(&request).unwrap(),
    );
    let done = request.tools.is_empty()
        || request
            .context
            .messages
            .iter()
            .any(|m| matches!(m, Message::Tool { .. }));
    ModelResponse {
        message: AssistantMessage {
            content: if done { "Done".into() } else { String::new() },
            tool_calls: if done {
                vec![]
            } else {
                vec![ToolCall {
                    id: ToolCallId::new("call-1").unwrap(),
                    name: ToolName::new("counter.add").unwrap(),
                    arguments: json!({"amount":1}),
                }]
            },
        },
        usage: Usage {
            input_tokens: 2,
            output_tokens: 3,
        },
    }
}

pub fn model(path: PathBuf) -> Orb {
    Orb::new(MODEL, 1).service(
        Service::new(MODEL, move |d| Ok((path.clone(), Progress::bind(&d)?)))
            .method(
                ModelMethod::Complete.as_str(),
                |env, ctx, (operation, request): (OperationId, ModelRequest)| async move {
                    Ok(response(&env.0, &ctx, operation, request, false))
                },
            )
            .method(
                ModelMethod::Stream.as_str(),
                |env, ctx, (operation, request): (OperationId, ModelRequest)| async move {
                    env.1.publish(
                        &ctx,
                        ModelProgress {
                            operation,
                            content: "transient preview".into(),
                        },
                    );
                    let result = response(&env.0, &ctx, operation, request, true);
                    env.1.publish(
                        &ctx,
                        ModelProgress {
                            operation,
                            content: result.message.content.clone(),
                        },
                    );
                    Ok(result)
                },
            )
            .method(
                ModelMethod::Recover.as_str(),
                |env, ctx, (operation, request): (OperationId, ModelRequest)| async move {
                    assert_eq!(ctx.principal()?, "alice");
                    log(
                        &env.0,
                        "model.recover",
                        operation,
                        &serde_json::to_string(&request).unwrap(),
                    );
                    Ok(Recovery::<ModelResponse>::Unknown)
                },
            ),
    )
}

pub fn tool(path: PathBuf, crash: bool, unknown: bool) -> Orb {
    Orb::new(TOOL, 1).service(
        Service::new(TOOL, move |_| Ok(path.clone()))
            .method(
                ToolMethod::Validate.as_str(),
                |_, ctx, (spec, call): (ToolSpec, ToolCall)| async move {
                    assert_eq!(ctx.principal()?, "alice");
                    assert_eq!(spec.name, call.name);
                    assert_eq!(call.arguments, json!({"amount":1}));
                    Ok(ToolDecision::Allow)
                },
            )
            .method(
                ToolMethod::Execute.as_str(),
                move |path, ctx, (operation, call): (OperationId, ToolCall)| async move {
                    assert_eq!(ctx.principal()?, "alice");
                    let mut database = Connection::open(path.as_path()).unwrap();
                    let transaction = database.transaction().unwrap();
                    let payload = serde_json::to_string(&call).unwrap();
                    let result = ToolResult::Success(json!({"value":1}));
                    transaction
                        .execute(
                            "INSERT INTO calls VALUES('tool.execute',?1,?2)",
                            params![operation.to_string(), payload],
                        )
                        .unwrap();
                    // Count every dispatch before receipt insertion; fixture idempotency
                    // must not conceal an accidental repeat by the agent.
                    transaction
                        .execute("UPDATE counter SET value=value+1", [])
                        .unwrap();
                    transaction
                        .execute(
                            "INSERT OR REPLACE INTO receipts VALUES(?1,?2,?3)",
                            params![
                                operation.to_string(),
                                payload,
                                serde_json::to_string(&result).unwrap()
                            ],
                        )
                        .unwrap();
                    transaction.commit().unwrap();
                    if crash {
                        // The external effect is committed, but no reply or runtime
                        // cleanup can confirm it to the pending agent operation.
                        std::process::exit(0);
                    }
                    Ok(result)
                },
            )
            .method(
                ToolMethod::Recover.as_str(),
                move |path, ctx, (operation, call): (OperationId, ToolCall)| async move {
                    assert_eq!(ctx.principal()?, "alice");
                    log(
                        &path,
                        "tool.recover",
                        operation,
                        &serde_json::to_string(&call).unwrap(),
                    );
                    if unknown {
                        return Ok(Recovery::Unknown);
                    }
                    let receipt: Option<(String, String)> = Connection::open(path.as_path())
                        .unwrap()
                        .query_row(
                            "SELECT payload,result FROM receipts WHERE operation=?1",
                            [operation.to_string()],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .optional()
                        .unwrap();
                    Ok(match receipt {
                        Some((payload, result)) => {
                            assert_eq!(payload, serde_json::to_string(&call).unwrap());
                            Recovery::Completed(
                                serde_json::from_str::<ToolResult>(&result).unwrap(),
                            )
                        }
                        None => Recovery::Unknown,
                    })
                },
            ),
    )
}
