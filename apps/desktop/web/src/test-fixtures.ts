import type {
  Capabilities,
  Capability,
  ClientProject,
  ExecutionPolicy,
  PreparedInput,
  EventKind,
  RuntimeEvent,
  SessionOpenProjection,
  SessionSnapshot,
  SpawnOptions,
  SubmissionReceipt,
} from "@swarmie/contracts";
import { decodeContract } from "@swarmie/contracts/validation";
import { render as renderView } from "@testing-library/react";
import type { RenderOptions } from "@testing-library/react";
import { createElement } from "react";
import type { ReactNode } from "react";
import { vi } from "vitest";

import { isRecord } from "~/web/src/protocol";
import { NodeQueryBoundary } from "~/web/src/query-boundary";
import { WorkspaceStore } from "~/web/src/workspace";
import type { Workspace } from "~/web/src/workspace";

/** A fixture holds an external acknowledgement until the test explicitly settles it. */
export function deferred<T>() {
  let complete: ((value: T | PromiseLike<T>) => void) | undefined;
  let fail: ((reason: Error) => void) | undefined;
  // The test owns this callback acknowledgement and must settle it before releasing its request owner.
  // oxlint-disable-next-line promise/avoid-new -- This external request gate models settlement and cancellation at the I/O adapter boundary.
  const promise = new Promise<T>((resolve, reject) => {
    complete = resolve;
    fail = reject;
  });
  if (complete === undefined || fail === undefined) {
    throw new Error("Fixture gate was not initialized.");
  }
  let opened = false;
  return {
    promise,
    resolve: complete,
    reject: fail,
    request: async () => {
      opened = true;
      return await promise;
    },
    started: () => opened,
  };
}
export function fixtureCapability(
  availability: Capability["availability"] = "unsupported",
): Capability {
  return { availability, source: "harness", evidence: "declared" };
}
export function fixtureCapabilities(patch: Partial<Capabilities> = {}): Capabilities {
  return {
    queue: fixtureCapability("available"),
    commands: fixtureCapability(),
    quotas: fixtureCapability(),
    backgroundTasks: fixtureCapability(),
    ephemeral: fixtureCapability(),
    outputSchema: fixtureCapability(),
    toolsDisabled: fixtureCapability(),
    forkAtMessage: fixtureCapability(),
    forkBeforeUser: fixtureCapability(),
    nativeSkillInputs: fixtureCapability(),
    submissionReconciliation: fixtureCapability(),
    resume: fixtureCapability(),
    fork: fixtureCapability(),
    steering: fixtureCapability(),
    cancel: fixtureCapability(),
    permissions: fixtureCapability(),
    operationApprovals: fixtureCapability(),
    reasoning: fixtureCapability(),
    usage: fixtureCapability(),
    models: fixtureCapability(),
    configure: fixtureCapability(),
    rollback: fixtureCapability(),
    compact: fixtureCapability(),
    modes: fixtureCapability(),
    images: fixtureCapability(),
    metadata: {},
    ...patch,
  };
}
export function fixturePolicy(patch: Partial<ExecutionPolicy> = {}): ExecutionPolicy {
  return {
    revision: 0,
    requested: null,
    status: "unknown",
    planned: null,
    plannedProfile: null,
    observed: null,
    choices: [],
    controlledSettings: [],
    dispatchBlocked: false,
    reason: null,
    bindingId: null,
    nativeSession: null,
    ...patch,
  };
}
export function fixtureOptions(patch: Partial<SpawnOptions> = {}): SpawnOptions {
  return {
    harness: "mock",
    cwd: "/project",
    projectId: null,
    projectRevision: null,
    model: null,
    config: {},
    execution: {
      configHome: null,
      mcpServers: [],
      tools: "enabled",
      hostTools: [],
      outputSchema: null,
      ephemeral: false,
    },
    ...patch,
  };
}
export function fixtureSession(patch: Partial<SessionSnapshot> = {}): SessionSnapshot {
  return {
    id: "session",
    workingContext: null,
    executionPolicy: fixturePolicy(),
    native: { provider: "mock", id: "native-session" },
    options: fixtureOptions(),
    capabilities: fixtureCapabilities(),
    state: "idle",
    sequence: 0,
    turn: null,
    turnsCompleted: 0,
    metadata: { title: null, archived: false, createdAtMs: 0, updatedAtMs: 0, forkOrigin: null },
    queue: { items: [], paused: false },
    ...patch,
  };
}
export function fixtureProjection(
  snapshot = fixtureSession(),
  patch: Partial<SessionOpenProjection> = {},
): SessionOpenProjection {
  return {
    snapshot,
    entries: [],
    unresolved: false,
    events: [],
    activation: { status: "attached" },
    ...patch,
  };
}
export function fixtureReceipt(patch: Partial<SubmissionReceipt> = {}): SubmissionReceipt {
  return {
    id: "receipt",
    sessionId: "session",
    idempotencyKey: "fixture-submission",
    turnId: "turn",
    delivery: "accepted",
    recoveryRequired: false,
    outcome: null,
    error: null,
    ...patch,
  };
}
export function fixtureInput(text: string, patch: Partial<PreparedInput> = {}): PreparedInput {
  return {
    text,
    content: [{ type: "text", data: text }],
    attachments: [],
    contextItems: [],
    afterSequence: 0,
    ...patch,
  };
}
export function fixtureProject(patch: Partial<ClientProject> = {}): ClientProject {
  return {
    id: "project",
    revision: 1,
    name: "Project",
    path: "/project",
    git: false,
    worktrees: [],
    ...patch,
  };
}
export function fixtureEvent<E extends EventKind>(
  event: E,
  patch: Partial<Omit<RuntimeEvent, keyof EventKind>> = {},
): E & Omit<RuntimeEvent, keyof EventKind> {
  return {
    sessionId: "session",
    sequence: 1,
    turnId: null,
    source: "mock",
    timestampMs: 0,
    ...event,
    ...patch,
  };
}
export function fixtureWorkspace(patch: Partial<Workspace> = {}): Workspace {
  return {
    projects: [],
    threads: [],
    activeId: "",
    harnesses: [],
    disabledHarnesses: [],
    connected: false,
    git: false,
    busy: false,
    error: "",
    extensions: [],
    cacheSaving: false,
    notifications: {
      // The workspace keeps its actual notification preference defaults.
      ...new WorkspaceStore().getSnapshot().notifications,
    },
    chatDirectory: "",
    ...patch,
  };
}
/** Preserve the real DOM window while installing only the native IPC capability. */
export function nativeRequestFixture(
  request: (method: string, params: unknown) => Promise<unknown>,
) {
  const invoke = vi.fn(async (_command: string, payload: unknown) => {
    if (
      !isRecord(payload) ||
      payload.operation !== "rpc" ||
      !isRecord(payload.request) ||
      typeof payload.request.method !== "string"
    ) {
      throw new Error("Unexpected native fixture operation.");
    }
    return {
      response: {
        jsonrpc: "2.0",
        id: payload.request.id,
        result: await request(payload.request.method, payload.request.params),
      },
    };
  });
  if (typeof window === "undefined" || typeof document === "undefined") {
    vi.stubGlobal("window", { location: { search: "?client=native" }, zero: { invoke } });
    return {
      invoke,
      restore: () => {
        vi.unstubAllGlobals();
      },
    };
  }
  const previousUrl = window.location.href;
  const previous = Object.getOwnPropertyDescriptor(window, "zero");
  window.history.replaceState(null, "", "?client=native");
  Object.defineProperty(window, "zero", { value: { invoke }, configurable: true });
  return {
    invoke,
    restore: () => {
      window.history.replaceState(null, "", previousUrl);
      if (previous) {
        Object.defineProperty(window, "zero", previous);
      } else {
        Reflect.deleteProperty(window, "zero");
      }
    },
  };
}

/** Each rendered view owns its query cache and cancels observations when unmounted. */
export function renderWithQuery(ui: ReactNode, options?: RenderOptions) {
  const CustomWrapper = options?.wrapper;
  function FixtureQueryWrapper({ children }: { children: ReactNode }) {
    const content =
      CustomWrapper === undefined ? children : createElement(CustomWrapper, null, children);
    return createElement(NodeQueryBoundary, null, content);
  }
  return renderView(ui, { ...options, wrapper: FixtureQueryWrapper });
}

export function fixtureControl(element: HTMLElement) {
  if (
    element instanceof HTMLButtonElement ||
    element instanceof HTMLInputElement ||
    element instanceof HTMLSelectElement ||
    element instanceof HTMLTextAreaElement
  ) {
    return element;
  }
  throw new TypeError("Expected a form control in the rendered fixture.");
}

export function fixtureText(value: unknown): string {
  if (typeof value !== "string") {
    throw new TypeError("Expected fixture text.");
  }
  return value;
}
export function fixtureRequest(value: unknown) {
  if (!isRecord(value) || typeof value.method !== "string") {
    throw new TypeError("Expected fixture RPC request.");
  }
  const params = value.params === undefined ? {} : value.params;
  if (!isRecord(params)) {
    throw new TypeError("Expected fixture request parameters.");
  }
  if (typeof value.id !== "number" && typeof value.id !== "string") {
    throw new TypeError("Expected fixture request identifier.");
  }
  const expectedRuntime =
    value.expectedRuntime === undefined
      ? undefined
      : decodeContract("RuntimeIdentity", value.expectedRuntime);
  return { method: value.method, params, id: value.id, expectedRuntime };
}
export function fixtureRequestBody(options: RequestInit | undefined) {
  if (typeof options?.body !== "string") {
    throw new TypeError("Expected a JSON fixture request body.");
  }
  const value: unknown = JSON.parse(options.body);
  return fixtureRequest(value);
}
