import type {
  NativeClientMethods,
  RuntimeResults,
  RuntimeIdentity,
  SshReadiness,
  LocalStoreIdentity,
} from "@swarmie/contracts";
import runtimeSchema from "@swarmie/contracts/schema" with { type: "json" };
import { decodeRuntimeResult, decodeContract } from "@swarmie/contracts/validation";
import type { SchemaObject } from "ajv";
import Ajv from "ajv/dist/2020.js";

import type { CachedWorkspace } from "~/src/cache";
import type { ClientNode } from "~/src/nodes";
import { isRecord } from "~/src/protocol";
import type { Detection, ExtensionInfo, Review, Thread, Project } from "~/src/protocol";
import type { AgentToolFlow, AgentToolSetup } from "~/src/provider-setup";

type NativeResults = { [M in keyof NativeClientMethods]: NativeClientMethods[M]["result"] };
type ConnectionResults = NativeResults & {
  "nodes/list": { nodes: ClientNode[]; selectedNodeId?: string };
  "nodes/select": unknown;
  "nodes/pair": unknown;
  "nodes/pairSsh": unknown;
  "nodes/removeRoute": unknown;
  "nodes/disconnect": unknown;
  "nodes/inspectSsh": SshReadiness;
  "nodes/prepareSsh": SshReadiness;
  "runtime/initialize": { name: string; protocolVersion: string; runtimeIdentity: RuntimeIdentity };
  "runtime/discover": Detection[];
  "runtime/probe": Detection;
  "extension/list": ExtensionInfo[];
  "runtime/agentToolsSetup": AgentToolSetup;
  "runtime/agentToolsDisconnect": AgentToolSetup;
  "runtime/agentToolsConnect": AgentToolFlow;
  "runtime/agentToolsFlow": AgentToolFlow;
  "runtime/agentToolsInput": AgentToolFlow;
  "runtime/agentToolsCancel": AgentToolFlow;
  "runtime/agentToolsInstall": AgentToolFlow;
};
export type ConnectionMethodResults = Omit<RuntimeResults, keyof ConnectionResults> &
  ConnectionResults;
export type ConnectionMethod = keyof ConnectionMethodResults;
type DecoderMap = { [M in ConnectionMethod]: (value: unknown) => ConnectionMethodResults[M] };

const engine = new Ajv({ strict: false, validateFormats: false });

const string = { type: "string" };
const number = { type: "number" };
const boolean = { type: "boolean" };
const json = {
  anyOf: [
    { type: "null" },
    { type: "string" },
    { type: "number" },
    { type: "boolean" },
    { type: "array", items: { $ref: "#/$defs/ClientJsonValue" } },
    { type: "object", additionalProperties: { $ref: "#/$defs/ClientJsonValue" } },
  ],
};
const dictionary = { type: "object", additionalProperties: json };
const reference = (name: string) => ({ $ref: `#/$defs/${name}` });
const array = <T extends object>(items: T) => ({ type: "array", items });
function object<T extends object>(properties: T, required: (keyof T)[] = []) {
  return { type: "object", properties, required };
}
const nullableString = { type: ["string", "null"] };
const worktree = object(
  {
    id: string,
    path: string,
    branch: nullableString,
    isMain: boolean,
    locked: boolean,
    prunable: boolean,
    setup: object({ completed: boolean, message: string }, ["completed"]),
  },
  ["id", "path", "branch", "isMain", "locked", "prunable"],
);
const project = object(
  {
    id: string,
    revision: number,
    name: string,
    path: string,
    git: boolean,
    gitId: string,
    worktrees: array(worktree),
    error: string,
  },
  ["id", "revision", "name", "path", "git", "worktrees"],
);
const attachment = object(
  {
    id: string,
    name: string,
    mime: string,
    data: string,
    image: boolean,
    binary: boolean,
    path: string,
    assetId: string,
  },
  ["id", "name", "mime", "data", "image"],
);
const context = object(
  {
    id: string,
    kind: { enum: ["file", "message", "terminal", "review", "resource"] },
    label: string,
    text: string,
    path: string,
    startLine: number,
    endLine: number,
    sourceId: string,
    skill: object({ name: string, path: string }, ["name", "path"]),
  },
  ["id", "kind", "label", "text"],
);
const preparedInput = object(
  {
    id: string,
    queueId: string,
    text: string,
    attachments: array(attachment),
    contextItems: array(context),
    content: array(reference("PromptContent")),
    afterSequence: number,
  },
  ["text", "attachments", "contextItems", "content", "afterSequence"],
);
const projection = object(
  {
    entries: array(
      object(
        {
          key: string,
          sessionId: string,
          receipt: reference("SubmissionReceipt"),
          input: { anyOf: [preparedInput, { type: "null" }] },
        },
        ["key", "sessionId", "receipt"],
      ),
    ),
    unresolved: boolean,
    snapshot: reference("SessionSnapshot"),
    inputs: array(preparedInput),
  },
  ["entries", "unresolved", "snapshot"],
);
const openProjection = object(
  {
    ...projection.properties,
    events: array(reference("RuntimeEvent")),
    activation: object(
      {
        status: { enum: ["attached", "inactive", "blocked", "unavailable"] },
        error: object({ code: number, message: string, data: json }, ["code", "message"]),
      },
      ["status"],
    ),
    queue: reference("PromptQueue"),
  },
  [...projection.required, "events", "activation"],
);
const controls = object(
  {
    configuration: array(reference("SettingDescriptor")),
    models: array(reference("ModelDescriptor")),
    values: dictionary,
    executionPolicy: reference("ExecutionPolicy"),
  },
  ["configuration", "models", "values", "executionPolicy"],
);
const recoveredDraft = object(
  { draft: string, attachments: array(attachment), contextItems: array(context) },
  ["draft", "attachments", "contextItems"],
);
const field = object({ id: string, label: string, secret: boolean, placeholder: string }, [
  "id",
  "label",
]);
const provider = object(
  {
    id: string,
    label: string,
    methods: array(
      object(
        {
          id: string,
          label: string,
          kind: { enum: ["api_key", "browser", "device", "existing"] },
          fields: array(field),
          help: string,
        },
        ["id", "label", "kind"],
      ),
    ),
  },
  ["id", "label", "methods"],
);
const setup = object(
  {
    harness: string,
    installed: boolean,
    authState: { enum: ["authenticated", "unauthenticated", "unknown"] },
    manual: boolean,
    flow: { $ref: "#/$defs/ClientFlow" },
    storage: object({ persistent: boolean, description: string }, ["persistent", "description"]),
    providers: array(provider),
    message: string,
    connection: object({ provider: string, method: string, storage: string, verified: boolean }, [
      "provider",
      "method",
      "storage",
      "verified",
    ]),
    installation: object({ supported: boolean, message: string }, ["supported"]),
  },
  ["harness", "installed", "authState", "storage", "providers"],
);
const flow = object(
  {
    id: string,
    status: { enum: ["pending", "connected", "completed", "failed", "cancelled"] },
    operation: { enum: ["install", "connect"] },
    message: string,
    authorizationUrl: string,
    userCode: string,
    expiresAt: string,
    inputFields: array(field),
  },
  ["id", "status"],
);
const detection = object(
  {
    harness: string,
    available: boolean,
    capabilities: reference("Capabilities"),
    executable: nullableString,
    version: nullableString,
    authentication: json,
    error: nullableString,
    authState: string,
    message: nullableString,
  },
  ["harness", "available"],
);
const extension = object(
  {
    id: string,
    ui: { anyOf: [reference("UiManifest"), { type: "null" }] },
    views: array(
      object({ id: string, title: string, kind: string, command: string }, [
        "id",
        "title",
        "kind",
        "command",
      ]),
    ),
    commands: array(
      object({ name: string, title: string, mutates: boolean }, ["name", "title", "mutates"]),
    ),
  },
  ["id"],
);
const route = object(
  {
    name: string,
    transport: object(
      { kind: string, url: string, host: string, socketPath: string, path: string },
      ["kind"],
    ),
    authenticated: boolean,
    revision: number,
  },
  ["name", "transport", "authenticated", "revision"],
);
const nodeSchema = object(
  {
    identity: reference("LocalStoreIdentity"),
    displayName: string,
    local: boolean,
    registered: boolean,
    lastVerifiedAtMs: number,
    connections: array(route),
  },
  ["identity", "displayName", "local", "registered", "lastVerifiedAtMs", "connections"],
);
const sshReadiness = object(
  {
    status: {
      enum: [
        "ready",
        "ssh-client-unavailable",
        "ssh-unavailable",
        "not-installed",
        "invalid-installation",
        "runtime-unavailable",
        "incompatible-runtime",
      ],
    },
    installed: { type: ["boolean", "null"] },
    ready: boolean,
    version: string,
    nextAction: string,
    runtimeIdentity: reference("RuntimeIdentity"),
    preparation: object(
      {
        available: boolean,
        action: { enum: ["install", "start"] },
        platform: nullableString,
        installPath: string,
        reason: string,
      },
      ["available"],
    ),
  },
  ["status", "installed", "ready", "nextAction"],
);
const privateSchemas = {
  "nodes/list": object({ nodes: array(nodeSchema), selectedNodeId: string }, ["nodes"]),
  "nodes/select": json,
  "nodes/pair": json,
  "nodes/pairSsh": json,
  "nodes/removeRoute": json,
  "nodes/disconnect": json,
  "nodes/inspectSsh": sshReadiness,
  "nodes/prepareSsh": sshReadiness,
  "runtime/initialize": object(
    { name: string, protocolVersion: string, runtimeIdentity: reference("RuntimeIdentity") },
    ["name", "protocolVersion", "runtimeIdentity"],
  ),
  "runtime/discover": array(detection),
  "runtime/probe": detection,
  "extension/list": array(extension),
  "runtime/agentToolsSetup": setup,
  "runtime/agentToolsDisconnect": setup,
  "runtime/agentToolsConnect": flow,
  "runtime/agentToolsFlow": flow,
  "runtime/agentToolsInput": flow,
  "runtime/agentToolsCancel": flow,
  "runtime/agentToolsInstall": flow,
  "client/sessionProjection": object(
    { ...projection.properties, events: array(reference("RuntimeEvent")) },
    [...projection.required, "events"],
  ),
  "client/submissions": projection,
  "client/sessionCreate": reference("SessionSnapshot"),
  "client/sessionPrepare": reference("ConfigurationPreview"),
  "client/sessionOpen": openProjection,
  "client/sessionControls": controls,
  "client/sessionConfigure": reference("ConfigurationReceipt"),
  "client/projectOpen": project,
  "client/projectGet": project,
  "client/projectCatalog": array(project),
  "client/projectDefaults": object(
    {
      harness: string,
      model: nullableString,
      config: dictionary,
      executionProfile: nullableString,
      cwd: string,
      revision: number,
    },
    ["harness", "revision"],
  ),
  "client/chatDirectory": reference("DirectoryResource"),
  "client/extensionInvoke": json,
  "client/worktree": object(
    { tree: { anyOf: [worktree, { type: "null" }] }, worktrees: array(worktree) },
    ["tree", "worktrees"],
  ),
  "client/sessionRecover": openProjection,
  "client/sessionInput": object({
    input: preparedInput,
    receipt: reference("SubmissionReceipt"),
    result: json,
    title: string,
    metadataError: json,
    queue: reference("PromptQueue"),
  }),
  "client/sessionQueue": object(
    { items: array(reference("QueuedPrompt")), paused: boolean, input: preparedInput },
    ["items", "paused"],
  ),
  "client/sessionHistory": object(
    { projection: openProjection, draft: { anyOf: [recoveredDraft, { type: "null" }] } },
    ["projection"],
  ),
  "client/projectCreate": project,
  "client/gitText": object(
    { draft: object({ message: string, title: string, body: string }), truncated: boolean },
    ["draft", "truncated"],
  ),
  "client/prProjection": json,
  "client/fileSave": json,
  "client/comparison": json,
  "client/sessionImport": openProjection,
  "client/schedulePut": object(
    { created: boolean, session: reference("SessionSnapshot"), result: json },
    ["created", "session", "result"],
  ),
  "nodes/startup": object(
    {
      environmentId: string,
      available: boolean,
      installed: boolean,
      enabled: { type: ["boolean", "null"] },
      running: { type: ["boolean", "null"] },
      trigger: { enum: ["login", "boot"] },
      bootReady: boolean,
      canEnableBoot: boolean,
      reason: string,
      notice: string,
      executionChanged: boolean,
      supervisionPendingRestart: boolean,
    },
    [
      "available",
      "installed",
      "enabled",
      "running",
      "trigger",
      "bootReady",
      "canEnableBoot",
      "reason",
      "notice",
    ],
  ),
};

function localDecoder<M extends keyof ConnectionResults>(
  method: M,
): (value: unknown) => ConnectionResults[M] {
  const resultSchema = {
    ...privateSchemas[method],
    $async: false,
    $defs: { ...runtimeSchema.$defs, ClientJsonValue: json, ClientFlow: flow },
  } satisfies SchemaObject;
  const validate = engine.compile<ConnectionResults[M]>(resultSchema);
  return (value) => {
    if (!validate(value)) {
      throw new TypeError(
        `Invalid native client response for ${method}: ${engine.errorsText(validate.errors?.slice(0, 1))}`,
      );
    }
    return value;
  };
}
const decoders: DecoderMap = {
  "mcp/list": (value) => decodeRuntimeResult("mcp/list", value),
  "mcp/mutate": (value) => decodeRuntimeResult("mcp/mutate", value),
  "mcp/reopen": (value) => decodeRuntimeResult("mcp/reopen", value),
  "nodes/list": localDecoder("nodes/list"),
  "nodes/select": localDecoder("nodes/select"),
  "nodes/pair": localDecoder("nodes/pair"),
  "nodes/pairSsh": localDecoder("nodes/pairSsh"),
  "nodes/removeRoute": localDecoder("nodes/removeRoute"),
  "nodes/disconnect": localDecoder("nodes/disconnect"),
  "nodes/inspectSsh": localDecoder("nodes/inspectSsh"),
  "nodes/prepareSsh": localDecoder("nodes/prepareSsh"),
  "agent/commands": (value) => decodeRuntimeResult("agent/commands", value),
  "agent/configuration": (value) => decodeRuntimeResult("agent/configuration", value),
  "agent/configure": (value) => decodeRuntimeResult("agent/configure", value),
  "agent/kill": (value) => decodeRuntimeResult("agent/kill", value),
  "agent/models": (value) => decodeRuntimeResult("agent/models", value),
  "agent/quotas": (value) => decodeRuntimeResult("agent/quotas", value),
  "agent/spawn": (value) => decodeRuntimeResult("agent/spawn", value),
  "directory/get": (value) => decodeRuntimeResult("directory/get", value),
  "directory/register": (value) => decodeRuntimeResult("directory/register", value),
  "extension/invoke": (value) => decodeRuntimeResult("extension/invoke", value),
  "extension/list": localDecoder("extension/list"),
  "permission/respond": (value) => decodeRuntimeResult("permission/respond", value),
  "project/create": (value) => decodeRuntimeResult("project/create", value),
  "project/get": (value) => decodeRuntimeResult("project/get", value),
  "project/list": (value) => decodeRuntimeResult("project/list", value),
  "project/update": (value) => decodeRuntimeResult("project/update", value),
  "runtime/agentToolsCancel": localDecoder("runtime/agentToolsCancel"),
  "runtime/agentToolsConnect": localDecoder("runtime/agentToolsConnect"),
  "runtime/agentToolsDisconnect": localDecoder("runtime/agentToolsDisconnect"),
  "runtime/agentToolsFlow": localDecoder("runtime/agentToolsFlow"),
  "runtime/agentToolsInput": localDecoder("runtime/agentToolsInput"),
  "runtime/agentToolsInstall": localDecoder("runtime/agentToolsInstall"),
  "runtime/agentToolsSetup": localDecoder("runtime/agentToolsSetup"),
  "runtime/configuration": (value) => decodeRuntimeResult("runtime/configuration", value),
  "runtime/discover": localDecoder("runtime/discover"),
  "runtime/generate": (value) => decodeRuntimeResult("runtime/generate", value),
  "runtime/history": (value) => decodeRuntimeResult("runtime/history", value),
  "runtime/history/read": (value) => decodeRuntimeResult("runtime/history/read", value),
  "runtime/initialize": localDecoder("runtime/initialize"),
  "runtime/probe": localDecoder("runtime/probe"),
  "runtime/quotas": (value) => decodeRuntimeResult("runtime/quotas", value),
  "runtime/quotas/all": (value) => decodeRuntimeResult("runtime/quotas/all", value),
  "runtime/schema": (value) => decodeRuntimeResult("runtime/schema", value),
  "runtime/shutdown": (value) => decodeRuntimeResult("runtime/shutdown", value),
  "session/cancel": (value) => decodeRuntimeResult("session/cancel", value),
  "session/close": (value) => decodeRuntimeResult("session/close", value),
  "session/compact": (value) => decodeRuntimeResult("session/compact", value),
  "session/context": (value) => decodeRuntimeResult("session/context", value),
  "session/create": (value) => decodeRuntimeResult("session/create", value),
  "session/events": (value) => decodeRuntimeResult("session/events", value),
  "session/fork": (value) => decodeRuntimeResult("session/fork", value),
  "session/get": (value) => decodeRuntimeResult("session/get", value),
  "session/import": (value) => decodeRuntimeResult("session/import", value),
  "session/list": (value) => decodeRuntimeResult("session/list", value),
  "session/metadata": (value) => decodeRuntimeResult("session/metadata", value),
  "session/prompt": (value) => decodeRuntimeResult("session/prompt", value),
  "session/queue": (value) => decodeRuntimeResult("session/queue", value),
  "session/queue/list": (value) => decodeRuntimeResult("session/queue/list", value),
  "session/queue/pause": (value) => decodeRuntimeResult("session/queue/pause", value),
  "session/queue/remove": (value) => decodeRuntimeResult("session/queue/remove", value),
  "session/queue/update": (value) => decodeRuntimeResult("session/queue/update", value),
  "session/resume": (value) => decodeRuntimeResult("session/resume", value),
  "session/rollback": (value) => decodeRuntimeResult("session/rollback", value),
  "session/steer": (value) => decodeRuntimeResult("session/steer", value),
  "session/submission": (value) => decodeRuntimeResult("session/submission", value),
  "session/submission/reconcile": (value) =>
    decodeRuntimeResult("session/submission/reconcile", value),
  "session/task/stop": (value) => decodeRuntimeResult("session/task/stop", value),
  "session/tasks": (value) => decodeRuntimeResult("session/tasks", value),
  "session/transcript": (value) => decodeRuntimeResult("session/transcript", value),
  "session/tree": (value) => decodeRuntimeResult("session/tree", value),
  "settings/directory": (value) => decodeRuntimeResult("settings/directory", value),
  "settings/setDirectory": (value) => decodeRuntimeResult("settings/setDirectory", value),
  "client/sessionProjection": localDecoder("client/sessionProjection"),
  "client/submissions": localDecoder("client/submissions"),
  "client/sessionCreate": localDecoder("client/sessionCreate"),
  "client/sessionPrepare": localDecoder("client/sessionPrepare"),
  "client/sessionOpen": localDecoder("client/sessionOpen"),
  "client/sessionControls": localDecoder("client/sessionControls"),
  "client/sessionConfigure": localDecoder("client/sessionConfigure"),
  "client/projectOpen": localDecoder("client/projectOpen"),
  "client/projectGet": localDecoder("client/projectGet"),
  "client/projectCatalog": localDecoder("client/projectCatalog"),
  "client/projectDefaults": localDecoder("client/projectDefaults"),
  "client/chatDirectory": localDecoder("client/chatDirectory"),
  "client/extensionInvoke": localDecoder("client/extensionInvoke"),
  "client/worktree": localDecoder("client/worktree"),
  "client/sessionRecover": localDecoder("client/sessionRecover"),
  "client/sessionInput": localDecoder("client/sessionInput"),
  "client/sessionQueue": localDecoder("client/sessionQueue"),
  "client/sessionHistory": localDecoder("client/sessionHistory"),
  "client/projectCreate": localDecoder("client/projectCreate"),
  "client/gitText": localDecoder("client/gitText"),
  "client/prProjection": localDecoder("client/prProjection"),
  "client/fileSave": localDecoder("client/fileSave"),
  "client/comparison": localDecoder("client/comparison"),
  "client/sessionImport": localDecoder("client/sessionImport"),
  "client/schedulePut": localDecoder("client/schedulePut"),
  "nodes/startup": localDecoder("nodes/startup"),
};

export function decodeClientResult<M extends ConnectionMethod>(
  method: M,
  value: unknown,
): ConnectionMethodResults[M] {
  return decoders[method](value);
}

export function decodeObjectResult(value: unknown): Record<string, unknown> {
  if (!isRecord(value)) {
    throw new TypeError("Expected an extension object response.");
  }
  return value;
}
export function decodeStringResult(value: unknown): string {
  if (typeof value !== "string") {
    throw new TypeError("Expected an extension text response.");
  }
  return value;
}
export function decodePathsResult(value: unknown) {
  const result = decodeObjectResult(value);
  if (!Array.isArray(result.paths)) {
    throw new TypeError("Invalid extension file list.");
  }
  return { paths: result.paths.map((path: unknown) => decodeStringResult(path)) };
}
export function decodeFileResult(value: unknown) {
  const result = decodeObjectResult(value);
  return { content: decodeStringResult(result.content) };
}
export function decodeProjectOpenResult(value: unknown) {
  const result = decodeObjectResult(value);
  return { id: decodeStringResult(result.id), path: decodeStringResult(result.path) };
}
export function decodeReviewResult(value: unknown): Review {
  const result = decodeObjectResult(value);
  if (
    !Array.isArray(result.files) ||
    typeof result.truncated !== "boolean" ||
    typeof result.filesTruncated !== "boolean"
  ) {
    throw new TypeError("Invalid extension review result.");
  }
  return {
    diff: decodeStringResult(result.diff),
    truncated: result.truncated,
    filesTruncated: result.filesTruncated,
    files: result.files.map((file: unknown) => {
      const entry = decodeObjectResult(file);
      return { path: decodeStringResult(entry.path), status: decodeStringResult(entry.status) };
    }),
  };
}

/** Provider errors and local connection failures retain distinct ownership markers. */
export class RpcError extends Error {
  constructor(
    message: string,
    public readonly code: number,
    public readonly data?: unknown,
    public readonly unavailable = false,
    public readonly staleResponse = false,
  ) {
    super(message);
    this.name = "RpcError";
  }
}
export function runtimeUnavailable(message: string): RpcError {
  return new RpcError(message, -32_006, undefined, true);
}
export function staleConnectionResponse(message: string): RpcError {
  return new RpcError(message, -32_005, undefined, true, true);
}
export function isRuntimeUnavailable(value: unknown): value is RpcError {
  return value instanceof RpcError && value.unavailable;
}
export function isStaleConnectionResponse(value: unknown): value is RpcError {
  return value instanceof RpcError && value.staleResponse;
}

// The cache stores this application's text/image message projection, not arbitrary provider message schemas.
engine.addKeyword({
  keyword: "validDate",
  validate: (_enabled: boolean, value: unknown) =>
    value instanceof Date && Number.isFinite(value.getTime()),
});
const message = object(
  {
    id: string,
    role: { enum: ["user", "assistant", "system"] },
    content: {
      anyOf: [
        string,
        array({
          anyOf: [
            object({ type: { const: "text" }, text: string }, ["type", "text"]),
            object({ type: { const: "image" }, image: string }, ["type", "image"]),
          ],
        }),
      ],
    },
    createdAt: { validDate: true },
  },
  ["role", "content"],
);
const permission = object(
  {
    id: string,
    title: string,
    description: string,
    options: array(object({ id: string, label: string }, ["id", "label"])),
    kind: string,
    metadata: object({
      questions: array(
        object(
          {
            id: string,
            prompt: string,
            options: array(object({ id: string, label: string }, ["id", "label"])),
            multiple: boolean,
            allowText: boolean,
            secret: boolean,
          },
          ["id", "prompt", "options"],
        ),
      ),
    }),
  },
  ["id", "title", "options"],
);
const fileLocation = object({ path: string, startLine: number, endLine: number }, ["path"]);
const frontendProject = object(project.properties, ["id", "name", "path", "worktrees"]);
const threadSchema = object(
  {
    id: string,
    title: string,
    projectId: string,
    worktreeId: string,
    harness: string,
    model: string,
    state: string,
    sequence: number,
    messages: array(message),
    tools: array(
      object(
        {
          id: string,
          name: string,
          status: string,
          input: json,
          output: json,
          metadata: dictionary,
        },
        ["id", "name", "status"],
      ),
    ),
    permissions: array(permission),
    pinned: boolean,
    projectRevision: number,
    projectDirectory: string,
    submitted: boolean,
    actualModel: string,
    sessionId: string,
    tasks: array(reference("AgentTask")),
    error: string,
    assistantId: string,
    itemId: string,
    messageAnchors: { type: "object", additionalProperties: string },
    outputFiles: array(string),
    fileDrafts: {
      type: "object",
      additionalProperties: object({ content: string, original: string }, ["content", "original"]),
    },
    submissionKeys: array(
      object({ key: string, recordId: string, receipt: reference("SubmissionReceipt") }, [
        "key",
        "recordId",
      ]),
    ),
    promptRecords: array(
      object(
        {
          id: string,
          key: string,
          afterSequence: number,
          nativeSequence: number,
          text: string,
          attachments: array(attachment),
          contextItems: array(context),
        },
        ["id", "key", "afterSequence", "text", "attachments", "contextItems"],
      ),
    ),
    capabilities: object(runtimeSchema.$defs.Capabilities.properties),
    configurations: array(reference("SettingDescriptor")),
    executionPolicy: reference("ExecutionPolicy"),
    models: array(reference("ModelDescriptor")),
    config: dictionary,
    archived: boolean,
    draft: string,
    attachments: array(attachment),
    queue: array(reference("QueuedPrompt")),
    queuePaused: boolean,
    turnId: string,
    turnGroups: array(
      object({ turnId: string, messageIds: array(string), toolIds: array(string) }, [
        "turnId",
        "messageIds",
        "toolIds",
      ]),
    ),
    reasoning: string,
    plan: json,
    subagents: array(dictionary),
    usage: dictionary,
    reviewedFiles: { type: "object", additionalProperties: string },
    contextItems: array(context),
    linkedPr: object({ number, url: string }, ["url"]),
    comparisonId: string,
    commandsRevision: number,
    createdAtMs: number,
    updatedAtMs: number,
    snoozeUntil: number,
    settled: boolean,
    layout: object({
      panel: nullableString,
      width: number,
      selectedView: string,
      openViews: array(string),
      files: array(fileLocation),
      activeFile: fileLocation,
    }),
    context: object({
      usedTokens: number,
      contextWindowTokens: number,
      remainingTokens: number,
      percent: number,
    }),
  },
  [
    "id",
    "title",
    "projectId",
    "worktreeId",
    "harness",
    "model",
    "state",
    "sequence",
    "messages",
    "tools",
    "permissions",
    "pinned",
  ],
);
const cacheDefinitions = { ...runtimeSchema.$defs, ClientJsonValue: json };
const validateThreads = engine.compile<Thread[]>({
  ...array(threadSchema),
  $async: false,
  $defs: cacheDefinitions,
});
const validateProjects = engine.compile<Project[]>({
  ...array(frontendProject),
  $async: false,
  $defs: cacheDefinitions,
});
function normalizeCachedMessage(value: unknown) {
  if (!isRecord(value) || value.createdAt === undefined || value.createdAt instanceof Date) {
    return value;
  }
  if (typeof value.createdAt !== "string") {
    throw new TypeError("Invalid cached message date.");
  }
  const createdAt = new Date(value.createdAt);
  if (!Number.isFinite(createdAt.getTime())) {
    throw new TypeError("Invalid cached message date.");
  }
  return { ...value, createdAt };
}
export function decodeCachedThreads(value: unknown): Thread[] {
  if (!Array.isArray(value)) {
    throw new TypeError("Invalid cached conversations.");
  }
  const normalized: unknown = value.map((item: unknown) =>
    isRecord(item) && Array.isArray(item.messages)
      ? { ...item, messages: item.messages.map(normalizeCachedMessage) }
      : item,
  );
  if (!validateThreads(normalized)) {
    throw new TypeError("Invalid cached conversation data.");
  }
  return normalized;
}
export function decodeCachedProjects(value: unknown): Project[] {
  if (!validateProjects(value)) {
    throw new TypeError("Invalid cached project data.");
  }
  return value;
}
export function decodeCachedWorkspace(
  value: unknown,
): CachedWorkspace & { node: LocalStoreIdentity } {
  const result = decodeObjectResult(value);
  const node = decodeContract("LocalStoreIdentity", result.node);
  if (typeof result.revision !== "number" || !Number.isFinite(result.revision)) {
    throw new TypeError("Invalid cached workspace revision.");
  }
  let metadata: CachedWorkspace["metadata"];
  if (result.metadata !== undefined) {
    const saved = decodeObjectResult(result.metadata);
    if (
      typeof saved.activeId !== "string" ||
      (saved.customChatDirectory !== undefined && typeof saved.customChatDirectory !== "string")
    ) {
      throw new TypeError("Invalid cached workspace metadata.");
    }
    metadata = {
      projects: decodeCachedProjects(saved.projects),
      activeId: saved.activeId,
      ...(typeof saved.customChatDirectory === "string"
        ? { customChatDirectory: saved.customChatDirectory }
        : {}),
    };
  }
  return {
    threads: decodeCachedThreads(result.threads),
    revision: result.revision,
    node,
    ...(metadata === undefined ? {} : { metadata }),
  };
}

export function isThreadKey(key: string): key is keyof Thread {
  return Object.hasOwn(threadSchema.properties, key);
}
