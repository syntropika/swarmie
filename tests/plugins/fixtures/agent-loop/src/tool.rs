//! A real Registry tool whose terminal enforces permission independently of hooks.

use std::{cell::RefCell, fs::OpenOptions, io::Write, path::PathBuf, rc::Rc};

use swarmie_agent_contracts::{
    ReplayPolicy, ToolCall, ToolDefinition, ToolResult, tool_definitions,
};
use tembra::{core, prelude::*};

#[tembra::contract(local, name = "swarmie.acceptance-agent-tool", version = 1)]
trait FixtureTool {
    #[portable]
    async fn execute(&self, call: ToolCall) -> Result<ToolResult>;
}

/// Observes the final invocation crossing the effective execution boundary.
#[derive(Clone, Default)]
pub struct ToolProbe {
    pub calls: Rc<RefCell<Vec<ToolCall>>>,
    pub physical_gate: Rc<RefCell<Option<crate::gate::AgentGateRef>>>,
    pub physically_settled: Rc<std::cell::Cell<bool>>,
}

/// Fixture controls for deterministic failures and real-process crash windows.
#[derive(Clone)]
pub struct ToolConfiguration {
    pub replay: ReplayPolicy,
    pub deny: bool,
    pub fail: bool,
    pub raw_error: bool,
    pub wait_for_cancellation: bool,
    pub effect_log: Option<PathBuf>,
    pub hang_after_effect: bool,
}

impl Default for ToolConfiguration {
    fn default() -> Self {
        Self {
            replay: ReplayPolicy::Safe,
            deny: false,
            fail: false,
            raw_error: false,
            wait_for_cancellation: false,
            effect_log: None,
            hang_after_effect: false,
        }
    }
}

/// Registers a functional numeric tool using the public tool Contract schema.
pub fn facet(configuration: ToolConfiguration, probe: ToolProbe) -> Result<NativeFacet> {
    NativeFacet::new("fixture.agent-tool", vec![], move |_, setup| {
        let configuration = configuration.clone();
        let probe = probe.clone();
        setup.serve_with_options(
            FixtureTool,
            move |_, _, declaration| {
                declaration.core_mut().register_method(
                    &tool_definitions()?,
                    "fixture.double",
                    serde_json::to_value(ToolDefinition {
                        name: "fixture.double".into(),
                        description: "Doubles an integer after terminal permission validation."
                            .into(),
                        arguments_schema: serde_json::json!({
                            "type":"object",
                            "properties":{"value":{"type":"integer"}},
                            "required":["value"],
                            "additionalProperties":false
                        }),
                        replay: configuration.replay,
                    })
                    .map_err(|error| {
                        RuntimeError::invalid("fixture_tool_metadata", error.to_string())
                    })?,
                    "execute",
                )?;
                Ok(Tool {
                    configuration: configuration.clone(),
                    probe: probe.clone(),
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

struct Tool {
    configuration: ToolConfiguration,
    probe: ToolProbe,
}

impl FixtureToolProvider for Tool {
    async fn execute(&self, call: ToolCall, context: Context) -> Result<ToolResult> {
        if self.configuration.deny {
            return Ok(ToolResult {
                operation_id: call.operation_id,
                tool_call_id: call.tool_call_id,
                output: serde_json::json!({"error":"the effective tool terminal denied execution"}),
                is_error: true,
            });
        }
        let value = call.arguments["value"].as_i64().ok_or_else(|| {
            RuntimeError::invalid("fixture_tool_arguments", "an integer value is required")
        })?;
        self.probe.calls.borrow_mut().push(call.clone());
        if let Some(path) = &self.configuration.effect_log {
            let mut file = OpenOptions::new()
                .append(true)
                .create(true)
                .open(path)
                .map_err(|error| RuntimeError::invalid("fixture_effect", error.to_string()))?;
            writeln!(file, "{}", call.operation_id)
                .and_then(|()| file.sync_all())
                .map_err(|error| RuntimeError::invalid("fixture_effect", error.to_string()))?;
        }
        if self.configuration.wait_for_cancellation {
            let error = context.cancellation().cancelled().await;
            let gate = self.probe.physical_gate.borrow().clone();
            if let Some(gate) = gate {
                gate.await_gate().await?;
            }
            self.probe.physically_settled.set(true);
            return Err(error);
        }
        if self.configuration.hang_after_effect {
            futures::future::pending::<()>().await;
        }
        if self.configuration.raw_error {
            return Err(RuntimeError::invalid(
                "fixture_tool_failure",
                "the tool failed after admission",
            ));
        }
        Ok(ToolResult {
            operation_id: call.operation_id,
            tool_call_id: call.tool_call_id,
            output: if self.configuration.fail {
                serde_json::json!({"error":"expected tool failure"})
            } else {
                serde_json::json!({"value":value * 2})
            },
            is_error: self.configuration.fail,
        })
    }
}
