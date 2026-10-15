import assert from "node:assert/strict";
import { test } from "node:test";
import { setImmediate as immediate } from "node:timers/promises";

import { diagnosticRedactor, diagnosticWriter } from "../diagnostics.mjs";
import { AgentSession } from "../session.mjs";

function fixture() {
  const emitted = [];
  const calls = [];
  let captured;

  let stream;
  const endDeferred = Promise.withResolvers();
  const end = endDeferred.promise;
  const finish = endDeferred.resolve;
  const sdk = {
    async *[Symbol.asyncIterator]() {
      yield* await end;
    },
    initializationResult: async () => ({ models: [{ value: "test-model" }] }),
    interrupt: async () => ({ still_queued: [] }),
    setModel: async (value) => calls.push(["model", value]),
    setPermissionMode: async (value) => calls.push(["mode", value]),
    applyFlagSettings: async (value) => calls.push(["settings", value]),
    rewindFiles: async (id, options) => {
      calls.push(["rewind", id, options]);
      return { canRewind: true };
    },
    usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET: async () => ({ rate_limits: null }),
    setMcpServers: async (value) => ({ added: Object.keys(value), errors: {} }),
    stopTask: async (id) => calls.push(["stop", id]),
    close: () => {
      calls.push(["close"]);
      finish([]);
    },
  };
  const session = new AgentSession(
    (input) => {
      captured = input.options;
      stream = input.prompt;
      return sdk;
    },
    (value) => emitted.push(value),
  );
  return { session, emitted, calls, options: () => captured, input: () => stream };
}
const start = (sample) =>
  sample.session.call("initialize", {
    cliExecutable: "claude",
    options: { cwd: "/workspace", persistSession: false },
  });

test("uses the SDK query options and streaming queue for prompts and steering", async () => {
  const f = fixture();
  const awaitedResult1 = await start(f);
  assert.equal(awaitedResult1.models[0].value, "test-model");
  assert.equal(f.options().pathToClaudeCodeExecutable, "claude");
  assert.equal(f.options().persistSession, false);
  const original = { type: "user", uuid: "first", message: { role: "user", content: "work" } };
  const steer = {
    type: "user",
    uuid: "second",
    priority: "now",
    message: { role: "user", content: "change direction" },
  };
  await f.session.call("prompt", original);
  await f.session.call("prompt", steer);
  const awaitedResult2 = await f.input().next();
  assert.deepEqual(awaitedResult2.value, original);
  const awaitedResult3 = await f.input().next();
  assert.deepEqual(awaitedResult3.value, steer);
  f.session.close();
  const awaitedResult4 = await f.input().next();
  assert.equal(awaitedResult4.done, true);
});

test("routes supported controls through public SDK methods", async () => {
  const f = fixture();
  await start(f);
  await f.session.call("set_model", { model: "opus" });
  await f.session.call("set_permission_mode", { mode: "plan" });
  await f.session.call("apply_flag_settings", { settings: { effortLevel: "high" } });
  assert.deepEqual(
    await f.session.call("rewind_files", { user_message_id: "user-cut", dry_run: true }),
    { canRewind: true },
  );
  await f.session.observeTasks({ type: "system", subtype: "init", session_id: "native-session" });
  await f.session.observeTasks({
    type: "system",
    subtype: "task_started",
    session_id: "native-session",
    task_id: "task-one",
    description: "Observed task",
  });
  await f.session.call("stop_task", { taskId: "task-one" });
  assert.deepEqual(f.calls, [
    ["model", "opus"],
    ["mode", "plan"],
    ["settings", { effortLevel: "high" }],
    ["rewind", "user-cut", { dryRun: true }],
    ["stop", "task-one"],
  ]);
  assert.deepEqual(await f.session.call("interrupt", {}), { still_queued: [] });
  assert.deepEqual(await f.session.call("get_usage", {}), { rate_limits: null });
  f.session.close();
});

test("permission callback waits for a correlated host response", async () => {
  const f = fixture();
  await start(f);
  const signal = new AbortController();
  const pending = f
    .options()
    .canUseTool(
      "AskUserQuestion",
      { questions: [] },
      { signal: signal.signal, toolUseID: "tool-one" },
    );
  const [request] = f.emitted;
  assert.equal(request.type, "sdk_permission");
  assert.equal(request.request.tool_use_id, "tool-one");
  const response = { behavior: "allow", updatedInput: { answers: { Q: "Yes" } } };
  await f.session.call("permission", { id: request.request_id, response });
  assert.deepEqual(await pending, response);
  await assert.rejects(
    f.session.call("permission", { id: request.request_id, response }),
    /expired/u,
  );
  f.session.close();
});

test("SDK abort expires permission callbacks without leaving pending promises", async () => {
  const f = fixture();
  await start(f);
  const signal = new AbortController();
  const pending = f.options().canUseTool("Bash", {}, { signal: signal.signal });
  signal.abort();
  const awaitedResult5 = await pending;
  assert.equal(awaitedResult5.behavior, "deny");
  assert.equal(f.session.permissions.size, 0);
  assert.equal(f.emitted[1].type, "sdk_permission_cancel");
  f.session.close();
});

test("closing denies pending permissions and disposes the SDK exactly once", async () => {
  const f = fixture();
  await start(f);
  const signal = new AbortController();
  const pending = f.options().canUseTool("Bash", {}, { signal: signal.signal });
  f.session.close();
  f.session.close();
  const awaitedResult6 = await pending;
  assert.equal(awaitedResult6.behavior, "deny");
  assert.deepEqual(f.calls, [["close"]]);
  await assert.rejects(f.session.call("prompt", {}), /closed/u);
});

test("close RPC waits for the official SDK graceful disposal before acknowledging", async () => {
  const f = fixture();
  await start(f);

  let finished = false;
  const gateDeferred = Promise.withResolvers();
  const gate = gateDeferred.promise;
  const finish = gateDeferred.resolve;
  f.session.query[Symbol.asyncDispose] = async () => {
    await gate;
    finished = true;
  };
  const closing = f.session.call("close", {});
  await Promise.resolve();
  assert.equal(finished, false);
  finish([]);
  assert.deepEqual(await closing, {});
  assert.equal(finished, true);
});

test("hooks forward observability data and return a neutral SDK result", async () => {
  const f = fixture();
  await start(f);
  const event = {
    hook_event_name: "PreToolUse",
    tool_name: "Read",
    tool_input: { file_path: "file.ts" },
  };
  assert.deepEqual(await f.options().hooks.PreToolUse[0].hooks[0](event), {});
  assert.deepEqual(f.emitted[0], { type: "sdk_hook", hook: "PreToolUse", data: event });
  f.session.close();
});

test("initialization failure closes SDK resources and duplicate initialization is rejected", async () => {
  let closed = false;
  const sdk = {
    async *[Symbol.asyncIterator]() {
      yield* [];
    },
    initializationResult: async () => {
      throw new Error("auth refused");
    },
    close() {
      closed = true;
    },
  };
  const session = new AgentSession(
    () => sdk,
    () => undefined,
  );
  await assert.rejects(
    session.initialize({ cliExecutable: "claude", options: {} }),
    /auth refused/u,
  );
  assert.equal(closed, true);
  const f = fixture();
  await start(f);
  await assert.rejects(start(f), /already initialized/u);
  f.session.close();
});

test("SDK failure diagnostics redact known credentials while retaining actionable context", async () => {
  const events = [];

  const readyDeferred = Promise.withResolvers();
  const ready = readyDeferred.promise;
  const release = readyDeferred.resolve;
  const sdk = {
    async *[Symbol.asyncIterator]() {
      yield* await ready;
      throw new Error("Authentication rejected secret-fixture-key");
    },
    initializationResult: async () => ({}),
    close() {
      return undefined;
    },
  };
  const redact = diagnosticRedactor({ ANTHROPIC_API_KEY: "secret-fixture-key", PATH: "/usr/bin" });
  const session = new AgentSession(
    () => sdk,
    (value) => events.push(value),
    redact,
  );
  await session.initialize({ cliExecutable: "claude", options: {} });
  release([]);
  await session.pump;
  assert.deepEqual(events, [{ _runtime_error: "Authentication rejected [redacted]" }]);
  assert.equal(redact("/usr/bin secret-fixture-key"), "/usr/bin [redacted]");
});

test("stderr credentials split across chunks are redacted before reaching runtime diagnostics", () => {
  const lines = [];
  const writer = diagnosticWriter(
    (text) => lines.push(text),
    diagnosticRedactor({ ANTHROPIC_API_KEY: "split-secret-key" }),
  );
  writer.write("Failure: split-");
  writer.write("secret-key\nsecond line");
  assert.deepEqual(lines, ["Failure: [redacted]\n"]);
  writer.flush();
  assert.deepEqual(lines, ["Failure: [redacted]\n", "second line"]);
});

test("native SDK error results redact operational text before runtime persistence", async () => {
  const events = [];

  const readyDeferred = Promise.withResolvers();
  const ready = readyDeferred.promise;
  const release = readyDeferred.resolve;
  const sdk = {
    async *[Symbol.asyncIterator]() {
      await ready;
      yield {
        type: "result",
        is_error: true,
        result: "Rejected native-secret",
        errors: ["Authorization native-secret"],
      };
    },
    initializationResult: async () => ({}),
    close() {
      return undefined;
    },
  };
  const session = new AgentSession(
    () => sdk,
    (value) => events.push(value),
    diagnosticRedactor({ ANTHROPIC_API_KEY: "native-secret" }),
  );
  await session.initialize({ cliExecutable: "claude", options: {} });
  release([]);
  await session.pump;
  assert.deepEqual(events[0], {
    type: "result",
    is_error: true,
    result: "Rejected [redacted]",
    errors: ["Authorization [redacted]"],
  });
});

const taskFrame = (subtype, fields = {}) => ({
  type: "system",
  subtype,
  session_id: "native-session",
  task_id: "task-one",
  run_id: "run-001",
  ...fields,
});
const taskStart = async (f) => {
  await start(f);
  await f.session.observeTasks({ type: "system", subtype: "init", session_id: "native-session" });
  await f.session.observeTasks(
    taskFrame("task_started", {
      description: "Inspect dependencies",
      task_type: "local_agent",
      is_backgrounded: true,
    }),
  );
};

test("task reads and stops remain available during permission callbacks and stop acknowledgement is not completion", async () => {
  const f = fixture();
  await taskStart(f);
  const signal = new AbortController();
  const permission = f.options().canUseTool("Bash", {}, { signal: signal.signal });
  const before = await f.session.call("tasks", {});
  assert.deepEqual(before, [
    { id: "task-one", description: "Inspect dependencies", status: "running", kind: "local_agent" },
  ]);
  before[0].status = "stopped";
  await f.session.call("stop_task", { taskId: "task-one" });
  const awaitedResult7 = await f.session.call("tasks", {});
  assert.equal(awaitedResult7[0].status, "running");
  await f.session.observeTasks(
    taskFrame("task_notification", { status: "stopped", summary: "Stopped by the provider" }),
  );
  const awaitedResult8 = await f.session.call("tasks", {});
  assert.equal(awaitedResult8[0].status, "stopped");
  await assert.rejects(f.session.call("stop_task", { taskId: "task-one" }), /inactive/u);
  await assert.rejects(f.session.call("stop_task", { taskId: "foreign-task" }), /Unknown/u);
  assert.deepEqual(f.calls, [["stop", "task-one"]]);
  f.session.close();
  const awaitedResult9 = await permission;
  assert.equal(awaitedResult9.behavior, "deny");
});

test("task run ordering rejects stale progress and foreign session frames without reviving terminal tasks", async () => {
  const f = fixture();
  await taskStart(f);
  await f.session.observeTasks(
    taskFrame("task_notification", { status: "failed", summary: "Failed natively" }),
  );
  await f.session.observeTasks(
    taskFrame("task_progress", { description: "Late progress", summary: "Must be ignored" }),
  );
  const awaitedResult10 = await f.session.call("tasks", {});
  assert.equal(awaitedResult10[0].status, "failed");
  await f.session.observeTasks(
    taskFrame("task_started", {
      run_id: "run-002",
      description: "Resumed native task",
      task_type: "local_agent",
    }),
  );
  await f.session.observeTasks(
    taskFrame("task_notification", { status: "stopped", summary: "Old run ended" }),
  );
  await f.session.observeTasks(
    taskFrame("task_progress", {
      session_id: "other-session",
      run_id: "run-003",
      description: "Foreign session",
    }),
  );
  await f.session.observeTasks(
    taskFrame("task_progress", { run_id: undefined, description: "Unversioned stale update" }),
  );
  assert.deepEqual(await f.session.call("tasks", {}), [
    { id: "task-one", description: "Resumed native task", status: "running", kind: "local_agent" },
  ]);
  await f.session.observeTasks(
    taskFrame("task_updated", {
      run_id: "run-002",
      patch: { description: "Native patch", status: "killed" },
    }),
  );
  const awaitedResult11 = await f.session.call("tasks", {});
  assert.equal(awaitedResult11[0].status, "stopped");
  f.session.close();
});

test("background membership disappearance reports unknown until a native terminal event confirms its outcome", async () => {
  const f = fixture();
  await taskStart(f);
  await f.session.observeTasks({
    type: "system",
    subtype: "background_tasks_changed",
    session_id: "native-session",
    tasks: [],
  });
  const awaitedResult12 = await f.session.call("tasks", {});
  assert.equal(awaitedResult12[0].status, "unknown");
  await assert.rejects(f.session.call("stop_task", { taskId: "task-one" }), /inactive/u);
  await f.session.observeTasks(
    taskFrame("task_notification", { status: "completed", summary: "Finished successfully" }),
  );
  await f.session.observeTasks({
    type: "system",
    subtype: "background_tasks_changed",
    session_id: "native-session",
    tasks: [
      {
        task_id: "task-one",
        run_id: "run-001",
        task_type: "local_agent",
        description: "Late snapshot",
      },
    ],
  });
  const awaitedResult13 = await f.session.call("tasks", {});
  assert.equal(awaitedResult13[0].status, "completed");
  f.session.close();
});

test("task cache is bounded and rejects evicted or malformed identifiers before invoking the SDK", async () => {
  const f = fixture();
  await taskStart(f);
  await f.session.observeTasks(
    taskFrame("task_notification", { status: "completed", summary: "Ended" }),
  );
  for await (const index of Array.from({ length: 300 }, (_, value) => value)) {
    await f.session.observeTasks(
      taskFrame("task_started", { task_id: `task-${index}`, description: "Running" }),
    );
  }
  const awaitedResult14 = await f.session.call("tasks", {});
  assert.equal(awaitedResult14.length, 256);
  for await (const taskId of ["task-one", "task-0", "", "bad\nidentifier"]) {
    await assert.rejects(f.session.call("stop_task", { taskId }), /Unknown/u);
  }
  assert.deepEqual(f.calls, []);
  f.session.close();
});

test("the official SDK message iterator emits ordered task updates independently of the parent turn result", async () => {
  const emitted = [];

  const endDeferred = Promise.withResolvers();
  const end = endDeferred.promise;
  const finish = endDeferred.resolve;
  const messages = [
    { type: "system", subtype: "init", session_id: "native-session" },
    taskFrame("task_started", { description: "Native background task", task_type: "local_bash" }),
    { type: "result", subtype: "success", is_error: false },
    taskFrame("task_progress", { description: "Native background task", summary: "Still running" }),
    taskFrame("task_notification", { status: "completed", summary: "Done" }),
  ];
  const sdk = {
    async *[Symbol.asyncIterator]() {
      for (const message of messages) {
        yield message;
      }
      yield* await end;
    },
    initializationResult: async () => ({}),
    close: () => finish(),
  };
  const session = new AgentSession(
    () => sdk,
    (value) => emitted.push(value),
  );
  await session.initialize({ cliExecutable: "claude", options: {} });
  async function waitForCompletion() {
    if (
      emitted.some((message) => message.type === "sdk_task" && message.task.status === "completed")
    ) {
      return;
    }
    await immediate();
    return waitForCompletion();
  }
  await waitForCompletion();
  assert.deepEqual(
    emitted
      .filter((message) => message.type === "sdk_task")
      .map((message) => [message.task.status, message.task.summary]),
    [
      ["running", undefined],
      ["running", "Still running"],
      ["completed", "Done"],
    ],
  );
  const awaitedResult15 = await session.call("tasks", {});
  assert.equal(awaitedResult15[0].status, "completed");
  await session.shutdown();
  await session.pump;
});

test("query disposal rejects a pending task stop and subsequent task controls cannot reach native code", async () => {
  const f = fixture();
  await taskStart(f);
  const stopDeferred = Promise.withResolvers();
  const rejectStop = stopDeferred.reject;
  f.session.query.stopTask = () => stopDeferred.promise;
  const { close } = f.session.query;
  f.session.query.close = () => {
    rejectStop(new Error("SDK query closed"));
    close();
  };
  const stopping = f.session.call("stop_task", { taskId: "task-one" });
  await assert.rejects(f.session.call("stop_task", { taskId: "task-one" }), /already pending/u);
  await f.session.shutdown();
  await assert.rejects(stopping, /closed/u);
  await assert.rejects(f.session.call("tasks", {}), /closed/u);
  await assert.rejects(f.session.call("stop_task", { taskId: "task-one" }), /closed/u);
  assert.equal(f.session.taskStops.size, 0);
});

test("bounded task text keeps the complete cached list within the runtime IPC record limit", async () => {
  const f = fixture();
  await start(f);
  await f.session.observeTasks({ type: "system", subtype: "init", session_id: "native-session" });
  const text = "\u0001".repeat(10_000);
  for await (const index of Array.from({ length: 256 }, (_, value) => value)) {
    await f.session.observeTasks(
      taskFrame("task_progress", {
        task_id: `task-${index}`,
        description: text,
        summary: text,
        task_type: text,
      }),
    );
  }
  const tasks = await f.session.call("tasks", {});
  assert.equal(tasks[0].description.length, 2048);
  assert.equal(tasks[0].summary.length, 2048);
  assert.equal(tasks[0].kind.length, 256);
  assert.ok(Buffer.byteLength(JSON.stringify(tasks)) < 8 * 1024 * 1024);
  f.session.close();
});
