import { describe, expect, it, vi, beforeEach } from "vitest";
import { newThread, reduceEvents, type Event, type Thread } from "./protocol";


const event = (
  sequence: number,
  type: string,
  data: unknown,
  sessionId = "s",
): Event => ({ sessionId, sequence, type, data, turnId: "turn" });

describe("ordered provider replay", () => {
  it("records the actual rerouted model without replacing the requested selection", () => {
    const initial = {...newThread("p","w","codex"),sessionId:"s",model:"requested-model"};
    const rerouted = reduceEvents(initial,[event(1,"turn.started",{}),event(2,"model.rerouted",{fromModel:"requested-model",toModel:"actual-model",reason:"fixture"}),event(3,"turn.ended",{outcome:"completed"})]);
    expect(rerouted.model).toBe("requested-model");
    expect(rerouted.actualModel).toBe("actual-model");
    expect(reduceEvents(rerouted,[event(4,"turn.started",{})]).actualModel).toBeUndefined();
  });
  it("keeps sparse tool identity and input, appends deltas, and retains output on denial", () => {
    const initial = { ...newThread("p", "w", "codex"), sessionId: "s" };
    const updated = reduceEvents(initial, [
      event(1, "tool.started", {id:"tool",name:"Bash",status:"running",input:{command:"fixture"},metadata:{kind:"command",locations:["hello.txt"]}}),
      event(2, "tool.updated", {id:"tool",name:null,status:"running",input:null,output:"first",metadata:{kind:null,outputDelta:"first"}}),
      event(3, "tool.updated", {id:"tool",name:null,status:"running",input:null,output:" second",metadata:{outputDelta:" second",progress:"working"}}),
      event(4, "tool.completed", {id:"tool",name:null,status:"denied",input:null,output:null,metadata:{outputDelta:null}}),
    ]);
    expect(updated.tools[0]).toMatchObject({name:"Bash",input:{command:"fixture"},output:"first second",status:"denied",metadata:{kind:"command",locations:["hello.txt"],progress:"working"}});
    expect(initial.tools).toEqual([]);
    expect(reduceEvents(updated,[event(4,"tool.completed",{id:"tool",output:"duplicate"})]).tools[0].output).toBe("first second");
  });
  it("preserves the provider's authentication error instead of displaying null", () => {
    const thread = { ...newThread("p", "w", "claude"), sessionId: "s" };
    const message =
      "Failed to authenticate: OAuth session expired and could not be refreshed";
    expect(
      reduceEvents(thread, [
        { ...event(1, "error", { message: "null" }), raw: { result: message } },
      ]).error,
    ).toBe(message);
    expect(
      reduceEvents(thread, [event(1, "error", { message: null })]).error,
    ).toContain("Check its authentication");
  });
  it("deduplicates pages, ignores other sessions and preserves separate native text items", () => {
    const initial = { ...newThread("p", "w", "codex"), sessionId: "s" };
    const first = [
      event(1, "message.user", { content: [{ type: "text", data: "Hello" }] }),
      event(2, "text.delta", { text: "Reading.", item_id: "commentary" }),
    ];
    const once = reduceEvents(initial, first);
    const twice = reduceEvents(once, [
      ...first,
      event(99, "text.delta", { text: "Wrong thread" }, "other"),
      event(3, "text.delta", { text: "Done.", item_id: "answer" }),
    ]);
    expect(twice.sequence).toBe(3);
    expect(twice.messages).toHaveLength(2);
    expect(twice.messages[1].content).toEqual([
      { type: "text", text: "Reading.\n\nDone." },
    ]);
    expect(initial.messages).toHaveLength(0);
  });
  it("retains concurrent permission requests and clears only the answered request", () => {
    const initial = { ...newThread("p", "w", "codex"), sessionId: "s" };
    const pending = reduceEvents(initial, [
      event(1, "permission.requested", {
        id: "a",
        title: "First",
        options: [],
      }),
      event(2, "permission.requested", {
        id: "b",
        title: "Second",
        options: [],
      }),
      event(3, "permission.resolved", { id: "a" }),
    ]);
    expect(pending.permissions.map((p) => p.id)).toEqual(["b"]);
    const finished = reduceEvents(pending, [
      event(4, "state.changed", "running"),
      event(5, "turn.ended", { outcome: "completed" }),
    ]);
    expect(finished.state).toBe("idle");
    expect(finished.permissions).toEqual([]);
  });
});

it("renders rich event state without losing concurrent subagent updates", () => {
  const initial = { ...newThread("p", "w", "pi"), sessionId: "s" };
  const result = reduceEvents(initial, [
    event(1, "reasoning.delta", { text: "Thinking" }),
    event(2, "plan.updated", {
      steps: [{ title: "Inspect", status: "completed" }],
    }),
    event(3, "subagent.updated", {
      id: "child",
      name: "Reviewer",
      status: "running",
    }),
    event(4, "subagent.updated", { id: "child", status: "complete" }),
    event(5, "context.updated", {
      usedTokens: 2000,
      contextWindowTokens: 10000,
      percent: 20,
    }),
    event(6, "queue.changed", { items: [], paused: true }),
  ]);
  expect(result.reasoning).toBe("Thinking");
  expect(result.subagents).toHaveLength(1);
  expect(result.subagents![0].name).toBe("Reviewer");
  expect(result.context?.percent).toBe(20);
  expect(result.queuePaused).toBe(true);
});
it("replays a successful rewind without displaying provider-removed turns", () => {
  let thread: Thread = { ...newThread("p", "w", "codex"), sessionId: "s" };
  thread = reduceEvents(thread, [
    event(1, "message.user", { content: [{ type: "text", data: "First" }] }),
    event(2, "text.delta", { text: "First answer" }),
    event(3, "turn.ended", {}),
  ]);
  thread = reduceEvents(thread, [
    {
      ...event(4, "message.user", {
        content: [{ type: "text", data: "Second" }],
      }),
      turnId: "second",
    },
    { ...event(5, "text.delta", { text: "Second answer" }), turnId: "second" },
    event(6, "conversation.rolled_back", { turns: 1 }),
  ]);
  expect(thread.messages).toHaveLength(2);
  expect(thread.turnGroups).toHaveLength(1);
});
it("keeps image attachments visible after durable replay", () => {
  const thread = reduceEvents(
    { ...newThread("p", "w", "codex"), sessionId: "s" },
    [
      event(1, "message.user", {
        content: [
          { type: "text", data: "Describe it" },
          { type: "image", data: { mime_type: "image/png", data: "AAAA" } },
        ],
      }),
    ],
  );
  expect(thread.messages[0].content).toEqual([
    { type: "text", text: "Describe it" },
    { type: "image", image: "data:image/png;base64,AAAA" },
  ]);
});

it("retains the persisted user event time without manufacturing a date for unknown history", () => {
  const thread = { ...newThread("", "/project", "codex"), sessionId: "native" };
  const timestampMs = Date.parse("2026-10-04T19:02:00Z");
  const result = reduceEvents(thread, [
    { sessionId: "native", sequence: 1, timestampMs, type: "message.user", data: { content: [{ type: "text", data: "Saved" }] } },
    { sessionId: "native", sequence: 2, type: "message.user", data: { content: [{ type: "text", data: "Unknown time" }] } },
  ]);
  expect(result.messages[0].createdAt?.getTime()).toBe(timestampMs);
  expect(result.messages[1].createdAt).toBeUndefined();
});