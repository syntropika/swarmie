import { expect, test } from "bun:test";

import { hookFromDescriptor, validateValue } from "@tembra/sdk";

import hookDescriptors from "../../../../target/generated/contracts/agent-hooks.json";
import { AgentLoop } from "../../../../target/generated/contracts/agent-loop";
import type { AgentLoopLocalStartArgs } from "../../../../target/generated/contracts/agent-loop";
import { ToolHandler } from "../../../../target/generated/contracts/agent-tool";

const start: AgentLoopLocalStartArgs = {
  turn_id: "turn",
  session_id: "session",
  input_event_id: "input-event",
  model: { connection: "test", model: "fake" },
  limits: {
    max_steps: 3,
    max_tool_calls: 4,
    max_duration_ms: 1000,
    max_output_tokens: 100,
  },
};

function argumentSchema(contract: typeof AgentLoop | typeof ToolHandler, name: string): unknown {
  const local = contract.descriptor.variants[""];
  if (local === undefined) {
    throw new TypeError("Expected local contract members");
  }
  const method = local[name];
  if (method === undefined || method.kind !== "method" || method.args === undefined) {
    throw new TypeError("Expected a typed method argument");
  }
  return method.args;
}

test("generated agent start preserves the neutral model identity and strict public shape", () => {
  const schema = argumentSchema(AgentLoop, "start");
  expect(validateValue(schema, start)).toBe(true);
  expect(validateValue(schema, { ...start, model: "provider-specific-model" })).toBe(false);
  expect(validateValue(schema, { ...start, private_history: [] })).toBe(false);
  expect(
    validateValue(schema, { ...start, limits: { ...start.limits, max_duration_ms: 1.5 } }),
  ).toBe(false);
});

test("generated tool execution requires stable identity without accepting an authorization grant", () => {
  const schema = argumentSchema(ToolHandler, "execute");
  const call = {
    turn: { turn_id: "turn", session_id: "session", task_id: "task", step: 0 },
    operation_id: "operation",
    tool_call_id: "model-call",
    tool: "echo",
    arguments: { text: "hello" },
  };
  expect(validateValue(schema, call)).toBe(true);
  expect(validateValue(schema, { ...call, authorized: true })).toBe(false);
  expect(validateValue(schema, { ...call, operation_id: null })).toBe(false);
});

test("every Rust hook descriptor materializes through the real TypeScript hook API", () => {
  expect(hookDescriptors).toHaveLength(11);
  for (const descriptor of hookDescriptors) {
    const hook = hookFromDescriptor(descriptor);
    expect(hook.descriptor.fingerprint).toBe(descriptor.fingerprint);
    expect(hook.descriptor.version).toBe(1);
    expect(() => hookFromDescriptor({ ...descriptor, version: 2 })).toThrow();
  }
});

test("context hook schema accepts model messages and rejects malformed transformations", () => {
  const descriptor = hookDescriptors.find(
    (candidate) => candidate.name === "swarmie.agent.prepare-context",
  );
  if (descriptor === undefined) {
    throw new TypeError("Missing context hook");
  }
  const hook = hookFromDescriptor(descriptor);
  const context = {
    turn: { turn_id: "turn", session_id: "session", task_id: "task", step: 0 },
    messages: [{ role: "user", content: [{ type: "text", text: "hello" }] }],
  };
  expect(validateValue(hook.descriptor.input, context)).toBe(true);
  expect(validateValue(hook.descriptor.input, { ...context, messages: "history" })).toBe(false);
  expect(
    validateValue(hook.descriptor.input, {
      ...context,
      messages: [{ role: "invalid", content: [] }],
    }),
  ).toBe(false);
});
