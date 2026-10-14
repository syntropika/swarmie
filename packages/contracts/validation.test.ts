// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
import { describe, expect, expectTypeOf, it } from "bun:test";

import type {
  AgentTask,
  ContractDefinitions,
  ModelDescriptor,
  NativeHistorySession,
  NativeMessageNode,
  RuntimeResults,
} from "./index.js";
import { decodeContract, decodeRuntimeResult } from "./validation.js";

const history: NativeHistorySession = {
  native: { provider: "cursor", id: "conversation-1" },
  cwd: "/workspace/project",
  path: "/private/history/conversation-1.json",
  title: "Review the parser",
  updatedAtMs: 42,
  metadata: { source: "sdk" },
};
const node: NativeMessageNode = {
  id: "message-1",
  parentId: null,
  role: "assistant",
  text: "Review complete.",
  active: true,
  metadata: null,
};
const task: AgentTask = {
  id: "task-1",
  description: "Review parser tests",
  status: "running",
  kind: "local_agent",
  summary: null,
};

function invokeUnknownMethod(method: string, value: unknown) {
  // Exercise untyped callers at the public wire boundary without weakening its TypeScript contract.
  Reflect.apply(decodeRuntimeResult, undefined, [method, value]);
}
function invokeUnknownDefinition(name: string, value: unknown) {
  Reflect.apply(decodeContract, undefined, [name, value]);
}

describe("public runtime response validation", () => {
  it("rejects unknown and inherited method/definition names before selecting a schema", () => {
    for (const name of [
      "missing/method",
      "toString",
      "constructor",
      "__proto__",
      "account/profile/update",
    ]) {
      expect(() => {
        invokeUnknownMethod(name, null);
      }).toThrow("Unknown runtime method.");
    }
    for (const name of ["MissingDefinition", "toString", "constructor", "__proto__"]) {
      expect(() => {
        invokeUnknownDefinition(name, {});
      }).toThrow("Unknown contract definition.");
    }
  });
  it("validates nested history and tree fields from their method-selected contracts", () => {
    const entries = [history];
    expect(decodeRuntimeResult("runtime/history", entries)).toBe(entries);
    expect(decodeRuntimeResult("session/tree", [node])).toEqual([node]);
    expect(() =>
      decodeRuntimeResult("runtime/history", [{ ...history, native: { id: "missing-provider" } }]),
    ).toThrow(TypeError);
    expect(() => decodeRuntimeResult("runtime/history", [{ ...history, updatedAtMs: -1 }])).toThrow(
      TypeError,
    );
    expect(() =>
      decodeRuntimeResult("runtime/history", [{ ...history, updatedAtMs: "42" }]),
    ).toThrow(TypeError);
    expect(() => decodeRuntimeResult("session/tree", [{ ...node, active: "true" }])).toThrow(
      TypeError,
    );
    expect(() => decodeRuntimeResult("session/tree", [{ ...node, parentId: 17 }])).toThrow(
      TypeError,
    );
  });
  it("rejects unrelated schemas and invalid native task states while keeping nullable fields", () => {
    expect(decodeRuntimeResult("session/tasks", [task])).toEqual([task]);
    expect(decodeContract("AgentTask", task)).toBe(task);
    expect(() => decodeRuntimeResult("session/tasks", [history])).toThrow(TypeError);
    expect(() => decodeContract("AgentTask", [task])).toThrow(TypeError);
    expect(() => decodeContract("AgentTask", { ...task, status: "cancelled" })).toThrow(TypeError);
    expect(() =>
      decodeContract("AgentTask", { id: task.id, description: task.description }),
    ).toThrow(TypeError);
    expect(decodeContract("AgentTask", { ...task, status: "stopped", kind: null })).toHaveProperty(
      "status",
      "stopped",
    );
  });
  it("distinguishes null success receipts from missing or falsy malformed responses", () => {
    expect(decodeRuntimeResult("session/cancel", null)).toBeNull();
    for (const value of [undefined, false, 0, "", {}]) {
      expect(() => decodeRuntimeResult("session/cancel", value)).toThrow(TypeError);
    }
    expect(
      decodeRuntimeResult("runtime/agentToolsFlow", {
        id: "flow-1",
        operation: "install",
        status: "completed",
      }),
    ).toHaveProperty("status", "completed");
    expect(() =>
      decodeRuntimeResult("runtime/agentToolsFlow", { id: "flow-1", status: "authenticated" }),
    ).toThrow(TypeError);
    expect(() =>
      decodeRuntimeResult("runtime/agentToolsFlow", {
        id: "flow-1",
        status: "pending",
        inputFields: [{ id: "code", label: "Code", secret: "yes" }],
      }),
    ).toThrow(TypeError);
  });
  it("does not coerce, mutate, or clone valid values and recovers after cached validation fails", () => {
    const model = Object.freeze({
      id: "model-1",
      label: "Model",
      metadata: Object.freeze({ futureField: true }),
    });
    const models = Object.freeze([model]);
    expect(Object.is(decodeRuntimeResult("agent/models", models), models)).toBe(true);
    expect(decodeRuntimeResult("agent/models", models).at(0)).toBe(model);
    expect(() => decodeRuntimeResult("agent/models", [{ id: 1, label: "Model" }])).toThrow(
      "Invalid wire response for method:agent/models.",
    );
    expect(Object.is(decodeRuntimeResult("agent/models", models), models)).toBe(true);
    expect(decodeContract("ModelDescriptor", model)).toBe(model);
    expect(() => decodeContract("ModelDescriptor", null)).toThrow(
      "Invalid wire response for definition:ModelDescriptor.",
    );
    expect(decodeContract("ModelDescriptor", model)).toBe(model);
  });
  it("infers generated output types from the selected public method and definition", () => {
    const tasks = decodeRuntimeResult("session/tasks", [task]);
    const models = decodeRuntimeResult("agent/models", []);
    const decodedHistory = decodeContract("NativeHistorySession", history);
    expectTypeOf(tasks).toEqualTypeOf<AgentTask[]>();
    expectTypeOf(models).toEqualTypeOf<ModelDescriptor[]>();
    expectTypeOf(decodedHistory).toEqualTypeOf<NativeHistorySession>();
    expectTypeOf<Parameters<typeof decodeRuntimeResult>[0]>().toEqualTypeOf<keyof RuntimeResults>();
    expectTypeOf<Parameters<typeof decodeContract>[0]>().toEqualTypeOf<keyof ContractDefinitions>();
  });
});
