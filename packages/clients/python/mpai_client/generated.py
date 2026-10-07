# Generated from runtime/schema. Run tools/generate.py to refresh.
from __future__ import annotations
from typing import Any, Literal, Union, TypedDict, NotRequired

class Capabilities(TypedDict):
    cancel: bool
    compact: bool
    configure: bool
    fork: bool
    images: bool
    metadata: dict[str, Any]
    models: bool
    modes: bool
    permissions: bool
    reasoning: bool
    resume: bool
    rollback: bool
    steering: bool
    usage: bool

class ContextUsage(TypedDict):
    contextWindowTokens: Union[int, None]
    percent: Union[float, None]
    remainingTokens: Union[int, None]
    usedTokens: Union[int, None]

class Detection(TypedDict):
    available: bool
    capabilities: Capabilities
    executable: Union[str, None]
    harness: str

class ExtensionCommand(TypedDict):
    mutates: bool
    name: str
    title: str

class ExtensionInfo(TypedDict):
    commands: list[ExtensionCommand]
    id: str
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

class InputAccountExecutionOptions(TypedDict):
    configHome: NotRequired[Union[str, None]]
    env: NotRequired[dict[str, str]]

class InputAccountQuery(TypedDict):
    config: NotRequired[Any]
    cwd: str
    execution: NotRequired[InputAccountExecutionOptions]
    harness: str

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

class InputSessionExecutionOptions(TypedDict):
    configHome: NotRequired[Union[str, None]]
    env: NotRequired[dict[str, str]]
    ephemeral: NotRequired[bool]
    mcpServers: NotRequired[list[InputMcpServer]]
    outputSchema: NotRequired[Any]
    tools: NotRequired[InputToolPolicy]

class InputSessionMetadataUpdate(TypedDict):
    archived: NotRequired[Union[bool, None]]
    title: NotRequired[Union[str, None]]

class InputSpawnOptions(TypedDict):
    config: NotRequired[Any]
    cwd: str
    execution: NotRequired[InputSessionExecutionOptions]
    harness: str
    model: NotRequired[Union[str, None]]

class McpServer(TypedDict):
    name: str
    transport: McpTransport

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

class SessionExecutionOptions(TypedDict):
    configHome: Union[str, None]
    ephemeral: bool
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
    id: str
    metadata: SessionMetadata
    native: NativeSessionRef
    options: SpawnOptions
    queue: PromptQueue
    sequence: int
    state: SessionState
    turn: Union[str, None]
    turnsCompleted: int

class SpawnOptions(TypedDict):
    config: Any
    cwd: str
    execution: SessionExecutionOptions
    harness: str
    model: Union[str, None]

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

class Usage(TypedDict):
    cacheWriteTokens: Union[int, None]
    cachedInputTokens: Union[int, None]
    costUsd: Union[float, None]
    inputTokens: Union[int, None]
    outputTokens: Union[int, None]
    scope: str
    totalTokens: Union[int, None]

AccountQuotaResult = Union[dict[str, Any], dict[str, Any], dict[str, Any]]
EventKind = Union[dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any]]
InputMcpTransport = Union[dict[str, Any], dict[str, Any], dict[str, Any]]
InputPromptContent = Union[dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any]]
InputQuestionAnswer = Union[str, list[str]]
InputRollbackScope = Literal['conversation_only', 'conversation_and_files']
InputToolPolicy = Literal['enabled', 'disabled']
McpTransport = Union[dict[str, Any], dict[str, Any], dict[str, Any]]
PromptContent = Union[dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any]]
SessionState = Literal['idle', 'running', 'needs_input', 'cancelling', 'closed', 'failed']
ToolPolicy = Literal['enabled', 'disabled']
METHODS = ('agent/commands', 'agent/configuration', 'agent/configure', 'agent/kill', 'agent/models', 'agent/quotas', 'agent/spawn', 'extension/invoke', 'extension/list', 'permission/respond', 'runtime/discover', 'runtime/generate', 'runtime/history', 'runtime/history/read', 'runtime/initialize', 'runtime/probe', 'runtime/quotas', 'runtime/quotas/all', 'runtime/schema', 'session/cancel', 'session/close', 'session/compact', 'session/create', 'session/events', 'session/fork', 'session/get', 'session/import', 'session/list', 'session/metadata', 'session/prompt', 'session/queue', 'session/queue/list', 'session/queue/pause', 'session/queue/remove', 'session/queue/update', 'session/resume', 'session/rollback', 'session/steer', 'session/transcript', 'session/tree')
