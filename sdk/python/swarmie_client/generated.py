# Generated from runtime/schema. Run tools/generate.py to refresh.
from __future__ import annotations
from typing import Any, Literal, Union, TypedDict, NotRequired

class Capabilities(TypedDict):
    cancel: Capability
    commands: Capability
    compact: Capability
    configure: Capability
    ephemeral: Capability
    fork: Capability
    forkAtMessage: Capability
    forkBeforeUser: Capability
    images: Capability
    metadata: dict[str, Any]
    models: Capability
    modes: Capability
    nativeSkillInputs: Capability
    operationApprovals: Capability
    outputSchema: Capability
    permissions: Capability
    queue: Capability
    quotas: Capability
    reasoning: Capability
    resume: Capability
    rollback: Capability
    steering: Capability
    submissionReconciliation: Capability
    toolsDisabled: Capability
    usage: Capability

class Capability(TypedDict):
    availability: CapabilityAvailability
    evidence: CapabilityEvidence
    reason: NotRequired[Union[str, None]]
    source: CapabilitySource
    sourceId: NotRequired[Union[str, None]]

class ConfigurationPreview(TypedDict):
    capabilities: Capabilities
    configuration: Union[list[SettingDescriptor], None]
    executionPolicy: ExecutionPolicy
    models: Union[list[ModelDescriptor], None]

class ConfigurationReceipt(TypedDict):
    configuration: list[SettingDescriptor]
    executionPolicy: ExecutionPolicy
    raw: NotRequired[Any]
    values: dict[str, Any]

class ContextStatus(TypedDict):
    available: bool
    context: WorkingContext
    reason: Union[str, None]

class ContextUsage(TypedDict):
    contextWindowTokens: Union[int, None]
    percent: Union[float, None]
    remainingTokens: Union[int, None]
    usedTokens: Union[int, None]

class DefaultDirectory(TypedDict):
    directory: DirectoryRef
    environmentId: str
    ownerScopeId: str
    revision: int

class Detection(TypedDict):
    available: bool
    capabilities: Capabilities
    executable: Union[str, None]
    harness: str

class DirectoryRef(TypedDict):
    id: str
    revision: int

class DirectoryResource(TypedDict):
    environmentId: str
    fingerprint: Union[str, None]
    id: str
    ownership: DirectoryOwnership
    path: str
    revision: int

class ExecutionPolicy(TypedDict):
    bindingId: Union[str, None]
    choices: list[ExecutionProfileChoice]
    controlledSettings: list[str]
    dispatchBlocked: bool
    nativeSession: Union[NativeSessionRef, None]
    observed: Union[PolicyObservation, None]
    planned: Any
    plannedProfile: Union[ExecutionProfile, None]
    reason: Union[str, None]
    requested: Union[ExecutionProfile, None]
    revision: int
    status: PolicyStatus

class ExecutionProfileChoice(TypedDict):
    application: SettingApplication
    capability: Capability
    description: str
    label: str
    mapping: PolicyMapping
    profile: ExecutionProfile

class ExtensionCommand(TypedDict):
    mutates: bool
    name: str
    title: str

class ExtensionInfo(TypedDict):
    commands: list[ExtensionCommand]
    id: str
    ui: NotRequired[Union[UiManifest, None]]
    views: list[ExtensionView]

class ExtensionView(TypedDict):
    command: str
    id: str
    kind: str
    title: str

class FileDiff(TypedDict):
    change: NotRequired[Union[str, None]]
    newText: NotRequired[Union[str, None]]
    oldText: NotRequired[Union[str, None]]
    path: str
    unified: NotRequired[Union[str, None]]

class FileLocation(TypedDict):
    column: NotRequired[Union[int, None]]
    endColumn: NotRequired[Union[int, None]]
    endLine: NotRequired[Union[int, None]]
    line: NotRequired[Union[int, None]]
    path: str

class ForkOrigin(TypedDict):
    messageId: Union[str, None]
    sequence: int
    sessionId: str
    turnId: Union[str, None]

class HarnessProbe(TypedDict):
    authState: str
    available: bool
    capabilities: Capabilities
    executable: Union[str, None]
    harness: str
    message: Union[str, None]
    version: Union[str, None]

class HostToolGrant(TypedDict):
    argumentConstraints: dict[str, list[Any]]
    extensionId: str
    toolId: str

class InputAccountExecutionOptions(TypedDict):
    configHome: NotRequired[Union[str, None]]
    env: NotRequired[dict[str, str]]

class InputAccountQuery(TypedDict):
    config: NotRequired[Any]
    cwd: str
    execution: NotRequired[InputAccountExecutionOptions]
    harness: str

class InputDefaultDirectoryUpdate(TypedDict):
    directory: InputDirectoryRef
    expectedRevision: int

class InputDirectoryRef(TypedDict):
    id: str
    revision: int

class InputDirectoryRegistration(TypedDict):
    path: str

class InputHostToolGrant(TypedDict):
    argumentConstraints: NotRequired[dict[str, list[Any]]]
    extensionId: str
    toolId: str

class InputLocalStoreIdentity(TypedDict):
    environmentId: str
    ownerScopeId: str
    storeId: str

class InputMcpServer(TypedDict):
    name: str
    transport: InputMcpTransport

class InputNativeHistoryQuery(TypedDict):
    configHome: NotRequired[Union[str, None]]
    cwd: NotRequired[Union[str, None]]
    harness: str
    limit: NotRequired[int]

class InputPermissionResponse(TypedDict):
    answers: NotRequired[dict[str, InputQuestionAnswer]]
    data: NotRequired[Any]
    id: str
    option: str

class InputProjectCreate(TypedDict):
    directory: NotRequired[Union[InputDirectoryRef, None]]
    launch: NotRequired[InputProjectLaunchDefaults]
    name: str

class InputProjectLaunchDefaults(TypedDict):
    config: NotRequired[Any]
    harness: NotRequired[Union[str, None]]
    model: NotRequired[Union[str, None]]

class InputProjectUpdate(TypedDict):
    archived: NotRequired[Union[bool, None]]
    directory: NotRequired[Union[InputDirectoryRef, None]]
    expectedRevision: int
    launch: NotRequired[Union[InputProjectLaunchDefaults, None]]
    name: NotRequired[Union[str, None]]
    projectId: str
    unlinkDirectory: NotRequired[bool]

class InputRuntimeIdentity(TypedDict):
    identity: InputLocalStoreIdentity
    incarnationId: str

class InputSessionExecutionOptions(TypedDict):
    configHome: NotRequired[Union[str, None]]
    env: NotRequired[dict[str, str]]
    ephemeral: NotRequired[bool]
    hostTools: NotRequired[list[InputHostToolGrant]]
    mcpServers: NotRequired[list[InputMcpServer]]
    outputSchema: NotRequired[Any]
    tools: NotRequired[InputToolPolicy]

class InputSessionMetadataUpdate(TypedDict):
    archived: NotRequired[Union[bool, None]]
    title: NotRequired[Union[str, None]]

class InputSpawnOptions(TypedDict):
    config: NotRequired[Any]
    cwd: NotRequired[str]
    execution: NotRequired[InputSessionExecutionOptions]
    harness: str
    model: NotRequired[Union[str, None]]
    projectId: NotRequired[Union[str, None]]
    projectRevision: NotRequired[Union[int, None]]

class LocalStoreIdentity(TypedDict):
    environmentId: str
    ownerScopeId: str
    storeId: str

class McpServer(TypedDict):
    name: str
    transport: McpTransport

class ModelDescriptor(TypedDict):
    description: NotRequired[Union[str, None]]
    id: str
    label: str
    metadata: NotRequired[dict[str, Any]]
    provider: NotRequired[Union[str, None]]

class ModelReroute(TypedDict):
    fromModel: str
    reason: Union[str, None]
    toModel: str

class NativeHistoryDocument(TypedDict):
    capabilities: Capabilities
    options: SpawnOptions
    records: list[NativeHistoryRecord]
    session: NativeHistorySession
    tree: list[NativeMessageNode]

class NativeHistoryRecord(TypedDict):
    kind: EventKind
    raw: Any
    turnId: Union[str, None]

class NativeHistorySession(TypedDict):
    cwd: str
    metadata: Any
    native: NativeSessionRef
    path: str
    title: str
    updatedAtMs: int

class NativeMessageNode(TypedDict):
    active: bool
    id: str
    metadata: Any
    parentId: Union[str, None]
    role: str
    text: str

class NativeSessionRef(TypedDict):
    id: str
    provider: str

class PermissionOption(TypedDict):
    id: str
    kind: str
    label: str

class PermissionRequest(TypedDict):
    command: Any
    description: Union[str, None]
    id: str
    kind: str
    metadata: Any
    options: list[PermissionOption]
    paths: list[str]
    questions: NotRequired[list[Question]]
    title: str

class PlanEntry(TypedDict):
    id: NotRequired[Union[str, None]]
    priority: NotRequired[Union[str, None]]
    status: NotRequired[Union[str, None]]
    text: NotRequired[Union[str, None]]

class PlanUpdate(TypedDict):
    entries: NotRequired[list[PlanEntry]]
    markdown: NotRequired[Union[str, None]]
    requestId: NotRequired[Union[str, None]]
    status: NotRequired[Union[str, None]]
    text: NotRequired[Union[str, None]]
    title: NotRequired[Union[str, None]]

class PolicyObservation(TypedDict):
    coverage: list[str]
    evidence: PolicyEvidence
    limitations: list[str]
    native: Any
    nativeVersion: Union[str, None]
    profile: Union[ExecutionProfile, None]

class ProjectLaunchDefaults(TypedDict):
    config: Any
    harness: Union[str, None]
    model: Union[str, None]

class ProjectRecord(TypedDict):
    archived: bool
    directory: Union[DirectoryRef, None]
    id: str
    launch: ProjectLaunchDefaults
    name: str
    revision: int

class Prompt(TypedDict):
    content: list[PromptContent]

class PromptQueue(TypedDict):
    items: list[QueuedPrompt]
    paused: bool

class Question(TypedDict):
    allowText: bool
    id: str
    multiple: bool
    options: list[QuestionOption]
    prompt: str
    secret: bool

class QuestionOption(TypedDict):
    description: NotRequired[Union[str, None]]
    id: str
    label: str

class QueuedPrompt(TypedDict):
    createdAtMs: int
    id: str
    prompt: Prompt

class RuntimeEvent(TypedDict):
    raw: NotRequired[Any]
    sequence: int
    sessionId: str
    source: str
    timestampMs: int
    turnId: Union[str, None]
    type: NotRequired[str]
    data: NotRequired[Any]

class RuntimeIdentity(TypedDict):
    identity: LocalStoreIdentity
    incarnationId: str

class SessionExecutionOptions(TypedDict):
    configHome: Union[str, None]
    ephemeral: bool
    hostTools: list[HostToolGrant]
    mcpServers: list[McpServer]
    outputSchema: Any
    tools: ToolPolicy

class SessionMetadata(TypedDict):
    archived: bool
    createdAtMs: int
    forkOrigin: Union[ForkOrigin, None]
    title: Union[str, None]
    updatedAtMs: int

class SessionSnapshot(TypedDict):
    capabilities: Capabilities
    executionPolicy: ExecutionPolicy
    id: str
    metadata: SessionMetadata
    native: NativeSessionRef
    options: SpawnOptions
    queue: PromptQueue
    sequence: int
    state: SessionState
    turn: Union[str, None]
    turnsCompleted: int
    workingContext: Union[WorkingContext, None]

class SettingChoice(TypedDict):
    description: NotRequired[Union[str, None]]
    id: str
    label: str
    metadata: NotRequired[dict[str, Any]]

class SettingDescriptor(TypedDict):
    application: SettingApplication
    choices: NotRequired[list[SettingChoice]]
    description: NotRequired[Union[str, None]]
    id: str
    label: str
    metadata: NotRequired[dict[str, Any]]
    type: SettingType
    value: NotRequired[Any]

class SpawnOptions(TypedDict):
    config: Any
    cwd: str
    execution: SessionExecutionOptions
    harness: str
    model: Union[str, None]
    projectId: Union[str, None]
    projectRevision: Union[int, None]

class SubagentInfo(TypedDict):
    background: NotRequired[Union[bool, None]]
    id: NotRequired[Union[str, None]]
    model: NotRequired[Union[str, None]]
    parentSessionId: NotRequired[Union[str, None]]
    parentToolId: NotRequired[Union[str, None]]
    role: NotRequired[Union[str, None]]
    status: NotRequired[Union[str, None]]
    summary: NotRequired[Union[str, None]]
    textDelta: NotRequired[Union[str, None]]
    title: NotRequired[Union[str, None]]
    tokens: NotRequired[Union[int, None]]
    usage: NotRequired[Union[Usage, None]]

class SubmissionReceipt(TypedDict):
    delivery: DeliveryKnowledge
    error: Union[str, None]
    id: str
    idempotencyKey: str
    outcome: Union[TurnOutcome, None]
    recoveryRequired: bool
    sessionId: str
    turnId: str

class ToolDetails(TypedDict):
    command: NotRequired[Union[str, None]]
    cwd: NotRequired[Union[str, None]]
    diffs: NotRequired[list[FileDiff]]
    kind: NotRequired[Union[str, None]]
    locations: NotRequired[list[FileLocation]]
    progress: NotRequired[Union[ToolProgress, None]]
    subagent: NotRequired[Union[SubagentInfo, None]]
    title: NotRequired[Union[str, None]]

class ToolEvent(TypedDict):
    details: NotRequired[Union[ToolDetails, None]]
    id: str
    input: Any
    metadata: Any
    name: Union[str, None]
    output: Any
    status: str

class ToolProgress(TypedDict):
    elapsedSeconds: NotRequired[Union[float, None]]
    message: NotRequired[Union[str, None]]
    percent: NotRequired[Union[float, None]]

class UiCommand(TypedDict):
    id: str
    runtimeCommand: NotRequired[Union[str, None]]
    shortcut: NotRequired[Union[str, None]]
    title: str
    when: list[UiCondition]

class UiContribution(TypedDict):
    clients: list[UiClient]
    command: NotRequired[Union[str, None]]
    enabledWhen: list[UiCondition]
    icon: NotRequired[Union[UiIcon, None]]
    id: str
    kind: UiContributionKind
    order: int
    slot: UiSlot
    text: NotRequired[Union[str, None]]
    title: str
    when: list[UiCondition]

class UiManifest(TypedDict):
    commands: list[UiCommand]
    panels: list[UiSurface]
    screens: list[UiSurface]
    slots: list[UiContribution]
    version: int

class UiSurface(TypedDict):
    clients: list[UiClient]
    command: NotRequired[Union[str, None]]
    id: str
    title: str

class Usage(TypedDict):
    cacheWriteTokens: Union[int, None]
    cachedInputTokens: Union[int, None]
    costUsd: Union[float, None]
    inputTokens: Union[int, None]
    outputTokens: Union[int, None]
    scope: str
    totalTokens: Union[int, None]

class WorkingContext(TypedDict):
    bindingId: str
    directory: DirectoryResource
    environmentId: str
    id: str
    revision: int
    sessionId: str
    source: ContextSource

AccountQuotaResult = Union[dict[str, Any], dict[str, Any], dict[str, Any]]
CapabilityAvailability = Literal['available', 'unavailable', 'unsupported', 'unknown']
CapabilityEvidence = Literal['declared', 'negotiated']
CapabilitySource = Literal['harness', 'runtime', 'extension']
ContextSource = Union[dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any]]
DeliveryKnowledge = Literal['not_sent', 'in_flight', 'accepted', 'rejected', 'unknown']
DirectoryOwnership = Literal['user', 'external_owner']
EventKind = Union[dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any]]
ExecutionProfile = Literal['ask', 'review', 'full', 'native_custom']
InputMcpTransport = Union[dict[str, Any], dict[str, Any], dict[str, Any]]
InputPromptContent = Union[dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any]]
InputQuestionAnswer = Union[str, list[str]]
InputRollbackScope = Literal['conversation_only', 'conversation_and_files']
InputToolPolicy = Literal['enabled', 'disabled']
McpTransport = Union[dict[str, Any], dict[str, Any], dict[str, Any]]
PolicyEvidence = Literal['native_receipt', 'native_acknowledgement']
PolicyMapping = Literal['equivalent', 'partial', 'unsupported', 'unknown']
PolicyStatus = Literal['staged', 'acknowledged', 'effective', 'rejected', 'partial', 'unknown']
PromptContent = Union[dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any]]
SessionState = Literal['idle', 'running', 'needs_input', 'cancelling', 'closed', 'failed']
SettingApplication = Literal['creation_only', 'next_turn', 'current_turn']
SettingType = Literal['select', 'boolean', 'string', 'number']
ToolPolicy = Literal['enabled', 'disabled']
TurnOutcome = Literal['completed', 'cancelled', 'interrupted', 'failed', 'limited', 'refused', 'handled', 'unknown']
UiClient = Literal['browser', 'desktop']
UiCondition = Literal['connected', 'conversation', 'project', 'message', 'panel', 'composerReady']
UiContributionKind = Literal['action', 'text', 'section']
UiIcon = Literal['microphone', 'stop', 'spinner', 'extension']
UiSlot = Literal['sidebar.primary', 'sidebar.footer', 'app.header.actions', 'conversation.header.actions', 'composer.actions', 'composer.context', 'composer.status', 'message.actions', 'message.details', 'project.actions', 'project.contextMenu', 'conversation.contextMenu', 'panel.toolbar', 'panel.status', 'settings.sections']
METHODS = ('agent/commands', 'agent/configuration', 'agent/configure', 'agent/kill', 'agent/models', 'agent/quotas', 'agent/spawn', 'directory/get', 'directory/register', 'extension/invoke', 'extension/list', 'permission/respond', 'project/create', 'project/get', 'project/list', 'project/update', 'runtime/configuration', 'runtime/discover', 'runtime/generate', 'runtime/history', 'runtime/history/read', 'runtime/initialize', 'runtime/probe', 'runtime/quotas', 'runtime/quotas/all', 'runtime/schema', 'runtime/shutdown', 'session/cancel', 'session/close', 'session/compact', 'session/context', 'session/create', 'session/events', 'session/fork', 'session/get', 'session/import', 'session/list', 'session/metadata', 'session/prompt', 'session/queue', 'session/queue/list', 'session/queue/pause', 'session/queue/remove', 'session/queue/update', 'session/resume', 'session/rollback', 'session/steer', 'session/submission', 'session/submission/reconcile', 'session/transcript', 'session/tree', 'settings/directory', 'settings/setDirectory')
