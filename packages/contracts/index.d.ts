// Generated from runtime/schema. Run tools/generate.py to refresh.
export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };
export type AccountQuotaResult =
  | { harness: string; quotas: JsonValue; status: "available" }
  | { harness: string; message: string; status: "unsupported" }
  | { harness: string; message: string; status: "error" };
export type AgentTask = {
  description: string;
  id: string;
  kind?: string | null;
  status: AgentTaskStatus;
  summary?: string | null;
};
export type AgentTaskStatus = "running" | "completed" | "failed" | "stopped" | "unknown";
export type Capabilities = {
  backgroundTasks: Capability;
  cancel: Capability;
  commands: Capability;
  compact: Capability;
  configure: Capability;
  ephemeral: Capability;
  fork: Capability;
  forkAtMessage: Capability;
  forkBeforeUser: Capability;
  images: Capability;
  metadata: {} & Record<string, JsonValue>;
  models: Capability;
  modes: Capability;
  nativeSkillInputs: Capability;
  operationApprovals: Capability;
  outputSchema: Capability;
  permissions: Capability;
  queue: Capability;
  quotas: Capability;
  reasoning: Capability;
  resume: Capability;
  rollback: Capability;
  steering: Capability;
  submissionReconciliation: Capability;
  toolsDisabled: Capability;
  usage: Capability;
};
export type Capability = {
  availability: CapabilityAvailability;
  evidence: CapabilityEvidence;
  reason?: string | null;
  source: CapabilitySource;
  sourceId?: string | null;
};
export type CapabilityAvailability = "available" | "unavailable" | "unsupported" | "unknown";
export type CapabilityEvidence = "declared" | "negotiated";
export type CapabilitySource = "harness" | "runtime" | "extension";
export type ConfigurationPreview = {
  capabilities: Capabilities;
  configuration: Array<SettingDescriptor> | null;
  executionPolicy: ExecutionPolicy;
  models: Array<ModelDescriptor> | null;
};
export type ConfigurationReceipt = {
  configuration: Array<SettingDescriptor>;
  executionPolicy: ExecutionPolicy;
  raw?: JsonValue;
  values: {} & Record<string, JsonValue>;
};
export type ContextSource =
  | { kind: "project_folder"; projectId: string; revision: number }
  | { kind: "project_free_default"; revision: number }
  | { kind: "explicit_directory" }
  | { kind: "native_existing" };
export type ContextStatus = { available: boolean; context: WorkingContext; reason: string | null };
export type ContextUsage = {
  contextWindowTokens: number | null;
  percent: number | null;
  remainingTokens: number | null;
  usedTokens: number | null;
};
export type DefaultDirectory = {
  directory: DirectoryRef;
  environmentId: string;
  ownerScopeId: string;
  revision: number;
};
export type DeliveryKnowledge = "not_sent" | "in_flight" | "accepted" | "rejected" | "unknown";
export type Detection = {
  available: boolean;
  capabilities: Capabilities;
  executable: string | null;
  harness: string;
};
export type DirectoryOwnership = "user" | "external_owner";
export type DirectoryRef = { id: string; revision: number };
export type DirectoryResource = {
  environmentId: string;
  fingerprint: string | null;
  id: string;
  ownership: DirectoryOwnership;
  path: string;
  revision: number;
};
export type EventKind =
  | { data: AgentTask; type: "task.updated" }
  | { data: SessionSnapshot; type: "session.started" }
  | { data: JsonValue; type: "session.updated" }
  | { data: JsonValue; type: "configuration.updated" }
  | { data: ModelReroute; type: "model.rerouted" }
  | { data: JsonValue; type: "commands.updated" }
  | { data: JsonValue; type: "quotas.updated" }
  | { data: PromptQueue; type: "queue.changed" }
  | { data: { turns: number }; type: "conversation.rolled_back" }
  | { type: "session.ended" }
  | { type: "turn.started" }
  | { data: { nativeReason?: string | null; outcome: TurnOutcome }; type: "turn.ended" }
  | { data: Prompt; type: "message.user" }
  | { data: JsonValue; type: "message.assistant" }
  | { data: { item_id: string | null }; type: "message.ended" }
  | { data: JsonValue; type: "output.structured" }
  | { data: { item_id: string | null; text: string }; type: "text.delta" }
  | { data: { text: string }; type: "reasoning.delta" }
  | { data: ToolEvent; type: "tool.started" }
  | { data: ToolEvent; type: "tool.updated" }
  | { data: ToolEvent; type: "tool.completed" }
  | { data: PermissionRequest; type: "permission.requested" }
  | { data: { id: string; option: string }; type: "permission.resolved" }
  | { data: JsonValue; type: "file.changed" }
  | { data: JsonValue; type: "diff" }
  | { data: PlanUpdate; type: "plan.updated" }
  | { data: SubagentInfo; type: "subagent.updated" }
  | { data: Usage; type: "usage.updated" }
  | { data: ContextUsage; type: "context.updated" }
  | { data: SessionState; type: "state.changed" }
  | { data: { message: string }; type: "error" }
  | { data: JsonValue; type: "diagnostic" }
  | { type: "provider.raw" };
export type ExecutionPolicy = {
  bindingId: string | null;
  choices: Array<ExecutionProfileChoice>;
  controlledSettings: Array<string>;
  dispatchBlocked: boolean;
  nativeSession: NativeSessionRef | null;
  observed: PolicyObservation | null;
  planned: JsonValue;
  plannedProfile: ExecutionProfile | null;
  reason: string | null;
  requested: ExecutionProfile | null;
  revision: number;
  status: PolicyStatus;
};
export type ExecutionProfile = "ask" | "review" | "full" | "native_custom";
export type ExecutionProfileChoice = {
  application: SettingApplication;
  capability: Capability;
  description: string;
  label: string;
  mapping: PolicyMapping;
  profile: ExecutionProfile;
};
export type ExtensionCommand = { mutates: boolean; name: string; title: string };
export type ExtensionInfo = {
  commands: Array<ExtensionCommand>;
  id: string;
  ui?: UiManifest | null;
  views: Array<ExtensionView>;
};
export type ExtensionView = { command: string; id: string; kind: string; title: string };
export type FileDiff = {
  change?: string | null;
  newText?: string | null;
  oldText?: string | null;
  path: string;
  unified?: string | null;
} & Record<string, JsonValue>;
export type FileLocation = {
  column?: number | null;
  endColumn?: number | null;
  endLine?: number | null;
  line?: number | null;
  path: string;
} & Record<string, JsonValue>;
export type ForkOrigin = {
  messageId: string | null;
  sequence: number;
  sessionId: string;
  turnId: string | null;
};
export type HarnessProbe = {
  authState: string;
  available: boolean;
  capabilities: Capabilities;
  executable: string | null;
  harness: string;
  message: string | null;
  version: string | null;
};
export type HostToolGrant = {
  argumentConstraints: {} & Record<string, Array<JsonValue>>;
  extensionId: string;
  toolId: string;
};
export type InputAccountExecutionOptions = {
  configHome?: string | null;
  env?: {} & Record<string, string>;
};
export type InputAccountQuery = {
  config?: JsonValue;
  cwd: string;
  execution?: InputAccountExecutionOptions;
  harness: string;
};
export type InputDefaultDirectoryUpdate = {
  directory: InputDirectoryRef;
  expectedRevision: number;
};
export type InputDirectoryRef = { id: string; revision: number };
export type InputDirectoryRegistration = { path: string };
export type InputHostToolGrant = {
  argumentConstraints?: {} & Record<string, Array<JsonValue>>;
  extensionId: string;
  toolId: string;
};
export type InputLocalStoreIdentity = {
  environmentId: string;
  ownerScopeId: string;
  storeId: string;
};
export type InputMcpDefinition = {
  args?: Array<string>;
  bearerTokenEnvVar?: string | null;
  command?: string | null;
  envVars?: Array<string>;
  headerEnvVars?: {} & Record<string, string>;
  transport: string;
  url?: string | null;
};
export type InputMcpMutation = {
  action: string;
  definition?: InputMcpDefinition | null;
  expectedRevision?: string | null;
  id?: string | null;
  name?: string | null;
  origin?: string | null;
  query?: InputMcpQuery;
  scope?: string | null;
};
export type InputMcpQuery = {
  configHome?: string | null;
  cwd?: string | null;
  harness?: string | null;
  sessionId?: string | null;
};
export type InputMcpServer = { name: string; transport: InputMcpTransport };
export type InputMcpTransport =
  | { args?: Array<string>; command: string; env?: {} & Record<string, string>; type: "stdio" }
  | { headers?: {} & Record<string, string>; type: "http"; url: string }
  | { headers?: {} & Record<string, string>; type: "sse"; url: string };
export type InputNativeHistoryQuery = {
  configHome?: string | null;
  cwd?: string | null;
  harness: string;
  limit?: number;
};
export type InputPermissionResponse = {
  answers?: {} & Record<string, InputQuestionAnswer>;
  data?: JsonValue;
  id: string;
  option: string;
};
export type InputProjectCreate = {
  directory?: InputDirectoryRef | null;
  launch?: InputProjectLaunchDefaults;
  name: string;
};
export type InputProjectLaunchDefaults = {
  config?: JsonValue;
  harness?: string | null;
  model?: string | null;
};
export type InputProjectUpdate = {
  archived?: boolean | null;
  directory?: InputDirectoryRef | null;
  expectedRevision: number;
  launch?: InputProjectLaunchDefaults | null;
  name?: string | null;
  projectId: string;
  unlinkDirectory?: boolean;
};
export type InputPromptContent =
  | { data: string; type: "text" }
  | { data: { data: string; mime_type: string }; type: "image" }
  | { data: { uri: string }; type: "resource" }
  | { data: { name: string; path: string }; type: "skill" };
export type InputQuestionAnswer = string | Array<string>;
export type InputRollbackScope = "conversation_only" | "conversation_and_files";
export type InputRuntimeIdentity = { identity: InputLocalStoreIdentity; incarnationId: string };
export type InputSessionExecutionOptions = {
  configHome?: string | null;
  env?: {} & Record<string, string>;
  ephemeral?: boolean;
  hostTools?: Array<InputHostToolGrant>;
  mcpServers?: Array<InputMcpServer>;
  outputSchema?: JsonValue;
  tools?: InputToolPolicy;
};
export type InputSessionMetadataUpdate = { archived?: boolean | null; title?: string | null };
export type InputSpawnOptions = {
  config?: JsonValue;
  cwd?: string;
  execution?: InputSessionExecutionOptions;
  harness: string;
  model?: string | null;
  projectId?: string | null;
  projectRevision?: number | null;
};
export type InputToolPolicy = "enabled" | "disabled";
export type LocalStoreIdentity = { environmentId: string; ownerScopeId: string; storeId: string };
export type McpCatalog = {
  entries: Array<McpEntry>;
  limitations: Array<string>;
  restartSessions: Array<string>;
};
export type McpDefinition = {
  args: Array<string>;
  bearerTokenEnvVar: string | null;
  command: string | null;
  envVars: Array<string>;
  headerEnvVars: {} & Record<string, string>;
  transport: string;
  url: string | null;
};
export type McpEntry = {
  actions: Array<string>;
  authStatus: string;
  credentialFields: Array<string>;
  definition: McpDefinition;
  enabled: boolean;
  harness: string | null;
  id: string;
  name: string;
  nativeName: string;
  origin: string;
  projectPath: string | null;
  readOnlyReason: string | null;
  requiresRestart: boolean;
  revision: string;
  runtimeStatus: string;
  scope: string;
  source: string | null;
  tools: Array<string>;
};
export type McpReceipt = {
  action: string;
  authorizationUrl: string | null;
  entry: McpEntry | null;
  restartSessions: Array<string>;
};
export type McpServer = { name: string; transport: McpTransport };
export type McpTransport =
  | { args: Array<string>; command: string; type: "stdio" }
  | { type: "http"; url: string }
  | { type: "sse"; url: string };
export type ModelDescriptor = {
  description?: string | null;
  id: string;
  label: string;
  metadata?: {} & Record<string, JsonValue>;
  provider?: string | null;
};
export type ModelReroute = { fromModel: string; reason: string | null; toModel: string };
export type NativeHistoryDocument = {
  capabilities: Capabilities;
  options: SpawnOptions;
  records: Array<NativeHistoryRecord>;
  session: NativeHistorySession;
  tree: Array<NativeMessageNode>;
};
export type NativeHistoryRecord = { kind: EventKind; raw: JsonValue; turnId: string | null };
export type NativeHistorySession = {
  cwd: string;
  metadata: JsonValue;
  native: NativeSessionRef;
  path: string;
  title: string;
  updatedAtMs: number;
};
export type NativeMessageNode = {
  active: boolean;
  id: string;
  metadata: JsonValue;
  parentId: string | null;
  role: string;
  text: string;
};
export type NativeSessionRef = { id: string; provider: string };
export type PermissionOption = { id: string; kind: string; label: string };
export type PermissionRequest = {
  command: JsonValue;
  description: string | null;
  id: string;
  kind: string;
  metadata: JsonValue;
  options: Array<PermissionOption>;
  paths: Array<string>;
  questions?: Array<Question>;
  title: string;
};
export type PlanEntry = {
  id?: string | null;
  priority?: string | null;
  status?: string | null;
  text?: string | null;
} & Record<string, JsonValue>;
export type PlanUpdate = {
  entries?: Array<PlanEntry>;
  markdown?: string | null;
  requestId?: string | null;
  status?: string | null;
  text?: string | null;
  title?: string | null;
} & Record<string, JsonValue>;
export type PolicyEvidence = "native_receipt" | "native_acknowledgement";
export type PolicyMapping = "equivalent" | "partial" | "unsupported" | "unknown";
export type PolicyObservation = {
  coverage: Array<string>;
  evidence: PolicyEvidence;
  limitations: Array<string>;
  native: JsonValue;
  nativeVersion: string | null;
  profile: ExecutionProfile | null;
};
export type PolicyStatus =
  | "staged"
  | "acknowledged"
  | "effective"
  | "rejected"
  | "partial"
  | "unknown";
export type ProjectLaunchDefaults = {
  config: JsonValue;
  harness: string | null;
  model: string | null;
};
export type ProjectRecord = {
  archived: boolean;
  directory: DirectoryRef | null;
  id: string;
  launch: ProjectLaunchDefaults;
  name: string;
  revision: number;
};
export type Prompt = { content: Array<PromptContent> };
export type PromptContent =
  | { data: string; type: "text" }
  | { data: { data: string; mime_type: string }; type: "image" }
  | { data: { uri: string }; type: "resource" }
  | { data: { name: string; path: string }; type: "skill" };
export type PromptQueue = { items: Array<QueuedPrompt>; paused: boolean };
export type Question = {
  allowText: boolean;
  id: string;
  multiple: boolean;
  options: Array<QuestionOption>;
  prompt: string;
  secret: boolean;
} & Record<string, JsonValue>;
export type QuestionOption = { description?: string | null; id: string; label: string } & Record<
  string,
  JsonValue
>;
export type QueuedPrompt = { createdAtMs: number; id: string; prompt: Prompt };
export type RuntimeEvent = (
  | { data: AgentTask; type: "task.updated" }
  | { data: SessionSnapshot; type: "session.started" }
  | { data: JsonValue; type: "session.updated" }
  | { data: JsonValue; type: "configuration.updated" }
  | { data: ModelReroute; type: "model.rerouted" }
  | { data: JsonValue; type: "commands.updated" }
  | { data: JsonValue; type: "quotas.updated" }
  | { data: PromptQueue; type: "queue.changed" }
  | { data: { turns: number }; type: "conversation.rolled_back" }
  | { type: "session.ended" }
  | { type: "turn.started" }
  | { data: { nativeReason?: string | null; outcome: TurnOutcome }; type: "turn.ended" }
  | { data: Prompt; type: "message.user" }
  | { data: JsonValue; type: "message.assistant" }
  | { data: { item_id: string | null }; type: "message.ended" }
  | { data: JsonValue; type: "output.structured" }
  | { data: { item_id: string | null; text: string }; type: "text.delta" }
  | { data: { text: string }; type: "reasoning.delta" }
  | { data: ToolEvent; type: "tool.started" }
  | { data: ToolEvent; type: "tool.updated" }
  | { data: ToolEvent; type: "tool.completed" }
  | { data: PermissionRequest; type: "permission.requested" }
  | { data: { id: string; option: string }; type: "permission.resolved" }
  | { data: JsonValue; type: "file.changed" }
  | { data: JsonValue; type: "diff" }
  | { data: PlanUpdate; type: "plan.updated" }
  | { data: SubagentInfo; type: "subagent.updated" }
  | { data: Usage; type: "usage.updated" }
  | { data: ContextUsage; type: "context.updated" }
  | { data: SessionState; type: "state.changed" }
  | { data: { message: string }; type: "error" }
  | { data: JsonValue; type: "diagnostic" }
  | { type: "provider.raw" }
) & {
  raw?: JsonValue;
  sequence: number;
  sessionId: string;
  source: string;
  timestampMs: number;
  turnId: string | null;
};
export type RuntimeIdentity = { identity: LocalStoreIdentity; incarnationId: string };
export type SessionExecutionOptions = {
  configHome: string | null;
  ephemeral: boolean;
  hostTools: Array<HostToolGrant>;
  mcpServers: Array<McpServer>;
  outputSchema: JsonValue;
  tools: ToolPolicy;
};
export type SessionMetadata = {
  archived: boolean;
  createdAtMs: number;
  forkOrigin: ForkOrigin | null;
  title: string | null;
  updatedAtMs: number;
};
export type SessionSnapshot = {
  capabilities: Capabilities;
  executionPolicy: ExecutionPolicy;
  id: string;
  metadata: SessionMetadata;
  native: NativeSessionRef;
  options: SpawnOptions;
  queue: PromptQueue;
  sequence: number;
  state: SessionState;
  turn: string | null;
  turnsCompleted: number;
  workingContext: WorkingContext | null;
};
export type SessionState = "idle" | "running" | "needs_input" | "cancelling" | "closed" | "failed";
export type SettingApplication = "creation_only" | "next_turn" | "current_turn";
export type SettingChoice = {
  description?: string | null;
  id: string;
  label: string;
  metadata?: {} & Record<string, JsonValue>;
};
export type SettingDescriptor = {
  application: SettingApplication;
  choices?: Array<SettingChoice>;
  description?: string | null;
  id: string;
  label: string;
  metadata?: {} & Record<string, JsonValue>;
  type: SettingType;
  value?: JsonValue;
};
export type SettingType = "select" | "boolean" | "string" | "number";
export type SpawnOptions = {
  config: JsonValue;
  cwd: string;
  execution: SessionExecutionOptions;
  harness: string;
  model: string | null;
  projectId: string | null;
  projectRevision: number | null;
};
export type SubagentInfo = {
  background?: boolean | null;
  id?: string | null;
  model?: string | null;
  parentSessionId?: string | null;
  parentToolId?: string | null;
  role?: string | null;
  status?: string | null;
  summary?: string | null;
  textDelta?: string | null;
  title?: string | null;
  tokens?: number | null;
  usage?: Usage | null;
} & Record<string, JsonValue>;
export type SubmissionReceipt = {
  delivery: DeliveryKnowledge;
  error: string | null;
  id: string;
  idempotencyKey: string;
  outcome: TurnOutcome | null;
  recoveryRequired: boolean;
  sessionId: string;
  turnId: string;
};
export type ToolDetails = {
  command?: string | null;
  cwd?: string | null;
  diffs?: Array<FileDiff>;
  kind?: string | null;
  locations?: Array<FileLocation>;
  progress?: ToolProgress | null;
  subagent?: SubagentInfo | null;
  title?: string | null;
} & Record<string, JsonValue>;
export type ToolEvent = {
  details?: ToolDetails | null;
  id: string;
  input: JsonValue;
  metadata: JsonValue;
  name: string | null;
  output: JsonValue;
  status: string;
};
export type ToolPolicy = "enabled" | "disabled";
export type ToolProgress = {
  elapsedSeconds?: number | null;
  message?: string | null;
  percent?: number | null;
} & Record<string, JsonValue>;
export type TurnOutcome =
  | "completed"
  | "cancelled"
  | "interrupted"
  | "failed"
  | "limited"
  | "refused"
  | "handled"
  | "unknown";
export type UiClient = "browser" | "desktop";
export type UiCommand = {
  id: string;
  runtimeCommand?: string | null;
  shortcut?: string | null;
  title: string;
  when: Array<UiCondition>;
};
export type UiCondition =
  | "connected"
  | "conversation"
  | "project"
  | "message"
  | "panel"
  | "composerReady";
export type UiContribution = {
  clients: Array<UiClient>;
  command?: string | null;
  enabledWhen: Array<UiCondition>;
  icon?: UiIcon | null;
  id: string;
  kind: UiContributionKind;
  order: number;
  slot: UiSlot;
  text?: string | null;
  title: string;
  when: Array<UiCondition>;
};
export type UiContributionKind = "action" | "text" | "section";
export type UiIcon = "microphone" | "stop" | "spinner" | "extension";
export type UiManifest = {
  commands: Array<UiCommand>;
  panels: Array<UiSurface>;
  screens: Array<UiSurface>;
  slots: Array<UiContribution>;
  version: number;
};
export type UiSlot =
  | "sidebar.primary"
  | "sidebar.footer"
  | "app.header.actions"
  | "conversation.header.actions"
  | "composer.actions"
  | "composer.context"
  | "composer.status"
  | "message.actions"
  | "message.details"
  | "project.actions"
  | "project.contextMenu"
  | "conversation.contextMenu"
  | "panel.toolbar"
  | "panel.status"
  | "settings.sections";
export type UiSurface = {
  clients: Array<UiClient>;
  command?: string | null;
  id: string;
  title: string;
};
export type Usage = {
  cacheWriteTokens: number | null;
  cachedInputTokens: number | null;
  costUsd: number | null;
  inputTokens: number | null;
  outputTokens: number | null;
  scope: string;
  totalTokens: number | null;
};
export type WorkingContext = {
  bindingId: string;
  directory: DirectoryResource;
  environmentId: string;
  id: string;
  revision: number;
  sessionId: string;
  source: ContextSource;
};
export interface Methods {
  "agent/commands": { params: { sessionId: string }; result: JsonValue };
  "agent/configuration": { params: { sessionId: string }; result: Array<SettingDescriptor> };
  "agent/configure": {
    params: { sessionId: string } & { config: Record<string, JsonValue> };
    result: ConfigurationReceipt;
  };
  "agent/kill": { params: { sessionId: string }; result: null };
  "agent/models": { params: { sessionId: string }; result: Array<ModelDescriptor> };
  "agent/quotas": { params: { sessionId: string }; result: JsonValue };
  "agent/spawn": { params: InputSpawnOptions; result: SessionSnapshot };
  "directory/get": { params: InputDirectoryRef; result: DirectoryResource };
  "directory/register": { params: InputDirectoryRegistration; result: DirectoryResource };
  "extension/invoke": {
    params: { arguments?: JsonValue; command: string; extensionId: string };
    result: JsonValue;
  };
  "extension/list": { params: Record<string, JsonValue>; result: Array<ExtensionInfo> };
  "mcp/list": { params: InputMcpQuery; result: McpCatalog };
  "mcp/mutate": { params: InputMcpMutation; result: McpReceipt };
  "mcp/reopen": { params: { sessionId: string }; result: SessionSnapshot };
  "permission/respond": { params: { sessionId: string } & InputPermissionResponse; result: null };
  "project/create": { params: InputProjectCreate; result: ProjectRecord };
  "project/get": { params: { projectId: string }; result: ProjectRecord };
  "project/list": { params: Record<string, JsonValue>; result: Array<ProjectRecord> };
  "project/update": { params: InputProjectUpdate; result: ProjectRecord };
  "runtime/agentToolsCancel": {
    params: { id: string };
    result: {
      authorizationUrl?: string;
      expiresAt?: string;
      id: string;
      inputFields?: Array<{ id: string; label: string; placeholder?: string; secret?: boolean }>;
      message?: string;
      operation?: "install" | "connect";
      status: "pending" | "connected" | "completed" | "failed" | "cancelled";
      userCode?: string;
    };
  };
  "runtime/agentToolsConnect": {
    params: {
      harness: string;
      method: string;
      provider: string;
      storage: "secure" | "session";
      values: {} & Record<string, string>;
    };
    result: {
      authorizationUrl?: string;
      expiresAt?: string;
      id: string;
      inputFields?: Array<{ id: string; label: string; placeholder?: string; secret?: boolean }>;
      message?: string;
      operation?: "install" | "connect";
      status: "pending" | "connected" | "completed" | "failed" | "cancelled";
      userCode?: string;
    };
  };
  "runtime/agentToolsDisconnect": {
    params: { harness: string };
    result: {
      authState: "authenticated" | "unauthenticated" | "unknown";
      connection?: {
        method: string;
        provider: string;
        storage: "native" | "secure" | "session";
        verified: boolean;
      };
      flow?: {
        authorizationUrl?: string;
        expiresAt?: string;
        id: string;
        inputFields?: Array<{ id: string; label: string; placeholder?: string; secret?: boolean }>;
        message?: string;
        operation?: "install" | "connect";
        status: "pending" | "connected" | "completed" | "failed" | "cancelled";
        userCode?: string;
      };
      harness: string;
      installation?: { message?: string; supported: boolean };
      installed: boolean;
      manual?: boolean;
      message?: string;
      providers: Array<{
        id: string;
        label: string;
        methods: Array<{
          fields?: Array<{ id: string; label: string; placeholder?: string; secret?: boolean }>;
          help?: string;
          id: string;
          kind: "api_key" | "browser" | "device" | "existing";
          label: string;
        }>;
      }>;
      storage: { description: string; persistent: boolean };
    };
  };
  "runtime/agentToolsFlow": {
    params: { id: string };
    result: {
      authorizationUrl?: string;
      expiresAt?: string;
      id: string;
      inputFields?: Array<{ id: string; label: string; placeholder?: string; secret?: boolean }>;
      message?: string;
      operation?: "install" | "connect";
      status: "pending" | "connected" | "completed" | "failed" | "cancelled";
      userCode?: string;
    };
  };
  "runtime/agentToolsInput": {
    params: { id: string; values: {} & Record<string, string> };
    result: {
      authorizationUrl?: string;
      expiresAt?: string;
      id: string;
      inputFields?: Array<{ id: string; label: string; placeholder?: string; secret?: boolean }>;
      message?: string;
      operation?: "install" | "connect";
      status: "pending" | "connected" | "completed" | "failed" | "cancelled";
      userCode?: string;
    };
  };
  "runtime/agentToolsInstall": {
    params: { harness: string };
    result: {
      authorizationUrl?: string;
      expiresAt?: string;
      id: string;
      inputFields?: Array<{ id: string; label: string; placeholder?: string; secret?: boolean }>;
      message?: string;
      operation?: "install" | "connect";
      status: "pending" | "connected" | "completed" | "failed" | "cancelled";
      userCode?: string;
    };
  };
  "runtime/agentToolsSetup": {
    params: { harness: string };
    result: {
      authState: "authenticated" | "unauthenticated" | "unknown";
      connection?: {
        method: string;
        provider: string;
        storage: "native" | "secure" | "session";
        verified: boolean;
      };
      flow?: {
        authorizationUrl?: string;
        expiresAt?: string;
        id: string;
        inputFields?: Array<{ id: string; label: string; placeholder?: string; secret?: boolean }>;
        message?: string;
        operation?: "install" | "connect";
        status: "pending" | "connected" | "completed" | "failed" | "cancelled";
        userCode?: string;
      };
      harness: string;
      installation?: { message?: string; supported: boolean };
      installed: boolean;
      manual?: boolean;
      message?: string;
      providers: Array<{
        id: string;
        label: string;
        methods: Array<{
          fields?: Array<{ id: string; label: string; placeholder?: string; secret?: boolean }>;
          help?: string;
          id: string;
          kind: "api_key" | "browser" | "device" | "existing";
          label: string;
        }>;
      }>;
      storage: { description: string; persistent: boolean };
    };
  };
  "runtime/configuration": { params: InputSpawnOptions; result: ConfigurationPreview };
  "runtime/discover": { params: Record<string, JsonValue>; result: Array<Detection> };
  "runtime/generate": {
    params: InputSpawnOptions &
      ({ text: JsonValue } | { content: JsonValue }) & {
        content?: Array<InputPromptContent>;
        text?: string;
      } & { timeoutMs?: number };
    result: string;
  };
  "runtime/history": { params: InputNativeHistoryQuery; result: Array<NativeHistorySession> };
  "runtime/history/read": {
    params: InputNativeHistoryQuery & { nativeId: string };
    result: NativeHistoryDocument;
  };
  "runtime/initialize": {
    params: Record<string, JsonValue>;
    result: {
      eventHistoryLimit: number;
      methods: Array<string>;
      name: string;
      protocolVersion: string;
      runtimeIdentity: RuntimeIdentity;
      transcriptDurability: string | null;
      version: string;
    };
  };
  "runtime/probe": { params: { harness: string }; result: HarnessProbe };
  "runtime/quotas": { params: InputAccountQuery; result: JsonValue };
  "runtime/quotas/all": {
    params: { queries: Array<InputAccountQuery> };
    result: Array<AccountQuotaResult>;
  };
  "runtime/schema": { params: Record<string, JsonValue>; result: Record<string, JsonValue> };
  "runtime/shutdown": { params: Record<string, JsonValue>; result: null };
  "session/cancel": { params: { sessionId: string } & { turnId?: string }; result: null };
  "session/close": { params: { sessionId: string }; result: null };
  "session/compact": { params: { sessionId: string }; result: null };
  "session/context": { params: { sessionId: string }; result: ContextStatus };
  "session/create": { params: InputSpawnOptions; result: SessionSnapshot };
  "session/events": {
    params: { sessionId: string } & { afterSequence?: number; limit?: number };
    result: Array<RuntimeEvent>;
  };
  "session/fork": {
    params: { sessionId: string } & {
      execution?: InputSessionExecutionOptions;
      messageId?: string;
      position?: "after_assistant" | "before_user";
    };
    result: SessionSnapshot;
  };
  "session/get": { params: { sessionId: string }; result: SessionSnapshot };
  "session/import": {
    params: InputNativeHistoryQuery & { nativeId: string };
    result: SessionSnapshot;
  };
  "session/list": { params: Record<string, JsonValue>; result: Array<SessionSnapshot> };
  "session/metadata": {
    params: { sessionId: string } & InputSessionMetadataUpdate;
    result: SessionSnapshot;
  };
  "session/prompt": {
    params: { sessionId: string } & ({ text: JsonValue } | { content: JsonValue }) & {
        content?: Array<InputPromptContent>;
        text?: string;
      } & { idempotencyKey: string };
    result: SubmissionReceipt;
  };
  "session/queue": {
    params: { sessionId: string } & ({ text: JsonValue } | { content: JsonValue }) & {
        content?: Array<InputPromptContent>;
        text?: string;
      };
    result: QueuedPrompt;
  };
  "session/queue/list": { params: { sessionId: string }; result: PromptQueue };
  "session/queue/pause": { params: { sessionId: string } & { paused: boolean }; result: null };
  "session/queue/remove": { params: { sessionId: string } & { id: string }; result: null };
  "session/queue/update": {
    params: { sessionId: string } & ({ text: JsonValue } | { content: JsonValue }) & {
        content?: Array<InputPromptContent>;
        text?: string;
      } & { id: string };
    result: null;
  };
  "session/resume": {
    params: { sessionId: string } & { execution?: InputSessionExecutionOptions };
    result: SessionSnapshot;
  };
  "session/rollback": {
    params: { sessionId: string } & {
      expectedSequence?: number;
      scope?: InputRollbackScope;
      turns: number;
    };
    result: null;
  };
  "session/steer": {
    params: { sessionId: string } & ({ text: JsonValue } | { content: JsonValue }) & {
        content?: Array<InputPromptContent>;
        text?: string;
      };
    result: { accepted: boolean };
  };
  "session/submission": {
    params: { sessionId: string } & { idempotencyKey: string };
    result: SubmissionReceipt | null;
  };
  "session/submission/reconcile": {
    params: { sessionId: string } & { idempotencyKey: string };
    result: SubmissionReceipt | null;
  };
  "session/task/stop": { params: { sessionId: string } & { taskId: string }; result: null };
  "session/tasks": { params: { sessionId: string }; result: Array<AgentTask> };
  "session/transcript": {
    params: { sessionId: string } & { afterSequence?: number; limit?: number };
    result: Array<RuntimeEvent>;
  };
  "session/tree": { params: { sessionId: string }; result: Array<NativeMessageNode> };
  "settings/directory": { params: Record<string, JsonValue>; result: DefaultDirectory | null };
  "settings/setDirectory": { params: InputDefaultDirectoryUpdate; result: DefaultDirectory };
}
export type {
  NativeClientMethods,
  SubmissionProjection,
  SessionProjection,
  SessionLaunchIntent,
  SessionControls,
  SessionOpenProjection,
  ClientProject,
  InputAttachment,
  InputContext,
  PreparedInput,
  RecoveredDraft,
  SshReadiness,
  NodeStartup,
} from "./client.js";
export type {
  AccountCursor,
  AccountMethods,
  AccountObservation,
  AccountProfile,
  AccountSession,
  AccountSessionPage,
  AccountSnapshot,
} from "./account.js";
export type RuntimeResults = { [M in keyof Methods]: Methods[M]["result"] };
export type ContractDefinitions = {
  AccountQuotaResult: AccountQuotaResult;
  AgentTask: AgentTask;
  AgentTaskStatus: AgentTaskStatus;
  Capabilities: Capabilities;
  Capability: Capability;
  CapabilityAvailability: CapabilityAvailability;
  CapabilityEvidence: CapabilityEvidence;
  CapabilitySource: CapabilitySource;
  ConfigurationPreview: ConfigurationPreview;
  ConfigurationReceipt: ConfigurationReceipt;
  ContextSource: ContextSource;
  ContextStatus: ContextStatus;
  ContextUsage: ContextUsage;
  DefaultDirectory: DefaultDirectory;
  DeliveryKnowledge: DeliveryKnowledge;
  Detection: Detection;
  DirectoryOwnership: DirectoryOwnership;
  DirectoryRef: DirectoryRef;
  DirectoryResource: DirectoryResource;
  EventKind: EventKind;
  ExecutionPolicy: ExecutionPolicy;
  ExecutionProfile: ExecutionProfile;
  ExecutionProfileChoice: ExecutionProfileChoice;
  ExtensionCommand: ExtensionCommand;
  ExtensionInfo: ExtensionInfo;
  ExtensionView: ExtensionView;
  FileDiff: FileDiff;
  FileLocation: FileLocation;
  ForkOrigin: ForkOrigin;
  HarnessProbe: HarnessProbe;
  HostToolGrant: HostToolGrant;
  InputAccountExecutionOptions: InputAccountExecutionOptions;
  InputAccountQuery: InputAccountQuery;
  InputDefaultDirectoryUpdate: InputDefaultDirectoryUpdate;
  InputDirectoryRef: InputDirectoryRef;
  InputDirectoryRegistration: InputDirectoryRegistration;
  InputHostToolGrant: InputHostToolGrant;
  InputLocalStoreIdentity: InputLocalStoreIdentity;
  InputMcpDefinition: InputMcpDefinition;
  InputMcpMutation: InputMcpMutation;
  InputMcpQuery: InputMcpQuery;
  InputMcpServer: InputMcpServer;
  InputMcpTransport: InputMcpTransport;
  InputNativeHistoryQuery: InputNativeHistoryQuery;
  InputPermissionResponse: InputPermissionResponse;
  InputProjectCreate: InputProjectCreate;
  InputProjectLaunchDefaults: InputProjectLaunchDefaults;
  InputProjectUpdate: InputProjectUpdate;
  InputPromptContent: InputPromptContent;
  InputQuestionAnswer: InputQuestionAnswer;
  InputRollbackScope: InputRollbackScope;
  InputRuntimeIdentity: InputRuntimeIdentity;
  InputSessionExecutionOptions: InputSessionExecutionOptions;
  InputSessionMetadataUpdate: InputSessionMetadataUpdate;
  InputSpawnOptions: InputSpawnOptions;
  InputToolPolicy: InputToolPolicy;
  LocalStoreIdentity: LocalStoreIdentity;
  McpCatalog: McpCatalog;
  McpDefinition: McpDefinition;
  McpEntry: McpEntry;
  McpReceipt: McpReceipt;
  McpServer: McpServer;
  McpTransport: McpTransport;
  ModelDescriptor: ModelDescriptor;
  ModelReroute: ModelReroute;
  NativeHistoryDocument: NativeHistoryDocument;
  NativeHistoryRecord: NativeHistoryRecord;
  NativeHistorySession: NativeHistorySession;
  NativeMessageNode: NativeMessageNode;
  NativeSessionRef: NativeSessionRef;
  PermissionOption: PermissionOption;
  PermissionRequest: PermissionRequest;
  PlanEntry: PlanEntry;
  PlanUpdate: PlanUpdate;
  PolicyEvidence: PolicyEvidence;
  PolicyMapping: PolicyMapping;
  PolicyObservation: PolicyObservation;
  PolicyStatus: PolicyStatus;
  ProjectLaunchDefaults: ProjectLaunchDefaults;
  ProjectRecord: ProjectRecord;
  Prompt: Prompt;
  PromptContent: PromptContent;
  PromptQueue: PromptQueue;
  Question: Question;
  QuestionOption: QuestionOption;
  QueuedPrompt: QueuedPrompt;
  RuntimeEvent: RuntimeEvent;
  RuntimeIdentity: RuntimeIdentity;
  SessionExecutionOptions: SessionExecutionOptions;
  SessionMetadata: SessionMetadata;
  SessionSnapshot: SessionSnapshot;
  SessionState: SessionState;
  SettingApplication: SettingApplication;
  SettingChoice: SettingChoice;
  SettingDescriptor: SettingDescriptor;
  SettingType: SettingType;
  SpawnOptions: SpawnOptions;
  SubagentInfo: SubagentInfo;
  SubmissionReceipt: SubmissionReceipt;
  ToolDetails: ToolDetails;
  ToolEvent: ToolEvent;
  ToolPolicy: ToolPolicy;
  ToolProgress: ToolProgress;
  TurnOutcome: TurnOutcome;
  UiClient: UiClient;
  UiCommand: UiCommand;
  UiCondition: UiCondition;
  UiContribution: UiContribution;
  UiContributionKind: UiContributionKind;
  UiIcon: UiIcon;
  UiManifest: UiManifest;
  UiSlot: UiSlot;
  UiSurface: UiSurface;
  Usage: Usage;
  WorkingContext: WorkingContext;
};
export { decodeContract, decodeRuntimeResult } from "./validation.js";
