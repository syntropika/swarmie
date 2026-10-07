// Generated from runtime/schema. Run tools/generate.py to refresh.
public typealias AccountQuotaResult = JSONValue
public struct Capabilities: Codable, Sendable {
    public var cancel: Bool
    public var compact: Bool
    public var configure: Bool
    public var fork: Bool
    public var images: Bool
    public var metadata: [String: JSONValue]
    public var models: Bool
    public var modes: Bool
    public var permissions: Bool
    public var reasoning: Bool
    public var resume: Bool
    public var rollback: Bool
    public var steering: Bool
    public var usage: Bool
    public init(cancel: Bool, compact: Bool, configure: Bool, fork: Bool, images: Bool, metadata: [String: JSONValue], models: Bool, modes: Bool, permissions: Bool, reasoning: Bool, resume: Bool, rollback: Bool, steering: Bool, usage: Bool) {
        self.cancel = cancel
        self.compact = compact
        self.configure = configure
        self.fork = fork
        self.images = images
        self.metadata = metadata
        self.models = models
        self.modes = modes
        self.permissions = permissions
        self.reasoning = reasoning
        self.resume = resume
        self.rollback = rollback
        self.steering = steering
        self.usage = usage
    }
}
public struct ContextUsage: Codable, Sendable {
    public var contextWindowTokens: UInt64?
    public var percent: Double?
    public var remainingTokens: UInt64?
    public var usedTokens: UInt64?
    public init(contextWindowTokens: UInt64? = nil, percent: Double? = nil, remainingTokens: UInt64? = nil, usedTokens: UInt64? = nil) {
        self.contextWindowTokens = contextWindowTokens
        self.percent = percent
        self.remainingTokens = remainingTokens
        self.usedTokens = usedTokens
    }
}
public struct Detection: Codable, Sendable {
    public var available: Bool
    public var capabilities: Capabilities
    public var executable: String?
    public var harness: String
    public init(available: Bool, capabilities: Capabilities, executable: String? = nil, harness: String) {
        self.available = available
        self.capabilities = capabilities
        self.executable = executable
        self.harness = harness
    }
}
public typealias EventKind = JSONValue
public struct ExtensionCommand: Codable, Sendable {
    public var mutates: Bool
    public var name: String
    public var title: String
    public init(mutates: Bool, name: String, title: String) {
        self.mutates = mutates
        self.name = name
        self.title = title
    }
}
public struct ExtensionInfo: Codable, Sendable {
    public var commands: [ExtensionCommand]
    public var id: String
    public var views: [ExtensionView]
    public init(commands: [ExtensionCommand], id: String, views: [ExtensionView]) {
        self.commands = commands
        self.id = id
        self.views = views
    }
}
public struct ExtensionView: Codable, Sendable {
    public var command: String
    public var id: String
    public var kind: String
    public var title: String
    public init(command: String, id: String, kind: String, title: String) {
        self.command = command
        self.id = id
        self.kind = kind
        self.title = title
    }
}
public struct FileDiff: Codable, Sendable {
    public var change: String?
    public var newText: String?
    public var oldText: String?
    public var path: String
    public var unified: String?
    public init(change: String? = nil, newText: String? = nil, oldText: String? = nil, path: String, unified: String? = nil) {
        self.change = change
        self.newText = newText
        self.oldText = oldText
        self.path = path
        self.unified = unified
    }
}
public struct FileLocation: Codable, Sendable {
    public var column: UInt64?
    public var endColumn: UInt64?
    public var endLine: UInt64?
    public var line: UInt64?
    public var path: String
    public init(column: UInt64? = nil, endColumn: UInt64? = nil, endLine: UInt64? = nil, line: UInt64? = nil, path: String) {
        self.column = column
        self.endColumn = endColumn
        self.endLine = endLine
        self.line = line
        self.path = path
    }
}
public struct ForkOrigin: Codable, Sendable {
    public var messageId: String?
    public var sequence: UInt64
    public var sessionId: String
    public var turnId: String?
    public init(messageId: String? = nil, sequence: UInt64, sessionId: String, turnId: String? = nil) {
        self.messageId = messageId
        self.sequence = sequence
        self.sessionId = sessionId
        self.turnId = turnId
    }
}
public struct HarnessProbe: Codable, Sendable {
    public var authState: String
    public var available: Bool
    public var capabilities: Capabilities
    public var executable: String?
    public var harness: String
    public var message: String?
    public var version: String?
    public init(authState: String, available: Bool, capabilities: Capabilities, executable: String? = nil, harness: String, message: String? = nil, version: String? = nil) {
        self.authState = authState
        self.available = available
        self.capabilities = capabilities
        self.executable = executable
        self.harness = harness
        self.message = message
        self.version = version
    }
}
public struct InputAccountExecutionOptions: Codable, Sendable {
    public var configHome: String?
    public var env: [String: String]?
    public init(configHome: String? = nil, env: [String: String]? = nil) {
        self.configHome = configHome
        self.env = env
    }
}
public struct InputAccountQuery: Codable, Sendable {
    public var config: JSONValue?
    public var cwd: String
    public var execution: InputAccountExecutionOptions?
    public var harness: String
    public init(config: JSONValue? = nil, cwd: String, execution: InputAccountExecutionOptions? = nil, harness: String) {
        self.config = config
        self.cwd = cwd
        self.execution = execution
        self.harness = harness
    }
}
public struct InputMcpServer: Codable, Sendable {
    public var name: String
    public var transport: InputMcpTransport
    public init(name: String, transport: InputMcpTransport) {
        self.name = name
        self.transport = transport
    }
}
public typealias InputMcpTransport = JSONValue
public struct InputNativeHistoryQuery: Codable, Sendable {
    public var configHome: String?
    public var cwd: String?
    public var harness: String
    public var limit: UInt64?
    public init(configHome: String? = nil, cwd: String? = nil, harness: String, limit: UInt64? = nil) {
        self.configHome = configHome
        self.cwd = cwd
        self.harness = harness
        self.limit = limit
    }
}
public struct InputPermissionResponse: Codable, Sendable {
    public var answers: [String: InputQuestionAnswer]?
    public var data: JSONValue?
    public var id: String
    public var option: String
    public init(answers: [String: InputQuestionAnswer]? = nil, data: JSONValue? = nil, id: String, option: String) {
        self.answers = answers
        self.data = data
        self.id = id
        self.option = option
    }
}
public typealias InputPromptContent = JSONValue
public typealias InputQuestionAnswer = JSONValue
public typealias InputRollbackScope = String
public struct InputSessionExecutionOptions: Codable, Sendable {
    public var configHome: String?
    public var env: [String: String]?
    public var ephemeral: Bool?
    public var mcpServers: [InputMcpServer]?
    public var outputSchema: JSONValue?
    public var tools: InputToolPolicy?
    public init(configHome: String? = nil, env: [String: String]? = nil, ephemeral: Bool? = nil, mcpServers: [InputMcpServer]? = nil, outputSchema: JSONValue? = nil, tools: InputToolPolicy? = nil) {
        self.configHome = configHome
        self.env = env
        self.ephemeral = ephemeral
        self.mcpServers = mcpServers
        self.outputSchema = outputSchema
        self.tools = tools
    }
}
public struct InputSessionMetadataUpdate: Codable, Sendable {
    public var archived: Bool?
    public var title: String?
    public init(archived: Bool? = nil, title: String? = nil) {
        self.archived = archived
        self.title = title
    }
}
public struct InputSpawnOptions: Codable, Sendable {
    public var config: JSONValue?
    public var cwd: String
    public var execution: InputSessionExecutionOptions?
    public var harness: String
    public var model: String?
    public init(config: JSONValue? = nil, cwd: String, execution: InputSessionExecutionOptions? = nil, harness: String, model: String? = nil) {
        self.config = config
        self.cwd = cwd
        self.execution = execution
        self.harness = harness
        self.model = model
    }
}
public typealias InputToolPolicy = String
public struct McpServer: Codable, Sendable {
    public var name: String
    public var transport: McpTransport
    public init(name: String, transport: McpTransport) {
        self.name = name
        self.transport = transport
    }
}
public typealias McpTransport = JSONValue
public struct ModelReroute: Codable, Sendable {
    public var fromModel: String
    public var reason: String?
    public var toModel: String
    public init(fromModel: String, reason: String? = nil, toModel: String) {
        self.fromModel = fromModel
        self.reason = reason
        self.toModel = toModel
    }
}
public struct NativeHistoryDocument: Codable, Sendable {
    public var capabilities: Capabilities
    public var options: SpawnOptions
    public var records: [NativeHistoryRecord]
    public var session: NativeHistorySession
    public var tree: [NativeMessageNode]
    public init(capabilities: Capabilities, options: SpawnOptions, records: [NativeHistoryRecord], session: NativeHistorySession, tree: [NativeMessageNode]) {
        self.capabilities = capabilities
        self.options = options
        self.records = records
        self.session = session
        self.tree = tree
    }
}
public struct NativeHistoryRecord: Codable, Sendable {
    public var kind: EventKind
    public var raw: JSONValue
    public var turnId: String?
    public init(kind: EventKind, raw: JSONValue, turnId: String? = nil) {
        self.kind = kind
        self.raw = raw
        self.turnId = turnId
    }
}
public struct NativeHistorySession: Codable, Sendable {
    public var cwd: String
    public var metadata: JSONValue
    public var native: NativeSessionRef
    public var path: String
    public var title: String
    public var updatedAtMs: UInt64
    public init(cwd: String, metadata: JSONValue, native: NativeSessionRef, path: String, title: String, updatedAtMs: UInt64) {
        self.cwd = cwd
        self.metadata = metadata
        self.native = native
        self.path = path
        self.title = title
        self.updatedAtMs = updatedAtMs
    }
}
public struct NativeMessageNode: Codable, Sendable {
    public var active: Bool
    public var id: String
    public var metadata: JSONValue
    public var parentId: String?
    public var role: String
    public var text: String
    public init(active: Bool, id: String, metadata: JSONValue, parentId: String? = nil, role: String, text: String) {
        self.active = active
        self.id = id
        self.metadata = metadata
        self.parentId = parentId
        self.role = role
        self.text = text
    }
}
public struct NativeSessionRef: Codable, Sendable {
    public var id: String
    public var provider: String
    public init(id: String, provider: String) {
        self.id = id
        self.provider = provider
    }
}
public struct PermissionOption: Codable, Sendable {
    public var id: String
    public var kind: String
    public var label: String
    public init(id: String, kind: String, label: String) {
        self.id = id
        self.kind = kind
        self.label = label
    }
}
public struct PermissionRequest: Codable, Sendable {
    public var command: JSONValue
    public var description: String?
    public var id: String
    public var kind: String
    public var metadata: JSONValue
    public var options: [PermissionOption]
    public var paths: [String]
    public var questions: [Question]?
    public var title: String
    public init(command: JSONValue, description: String? = nil, id: String, kind: String, metadata: JSONValue, options: [PermissionOption], paths: [String], questions: [Question]? = nil, title: String) {
        self.command = command
        self.description = description
        self.id = id
        self.kind = kind
        self.metadata = metadata
        self.options = options
        self.paths = paths
        self.questions = questions
        self.title = title
    }
}
public struct PlanEntry: Codable, Sendable {
    public var id: String?
    public var priority: String?
    public var status: String?
    public var text: String?
    public init(id: String? = nil, priority: String? = nil, status: String? = nil, text: String? = nil) {
        self.id = id
        self.priority = priority
        self.status = status
        self.text = text
    }
}
public struct PlanUpdate: Codable, Sendable {
    public var entries: [PlanEntry]?
    public var markdown: String?
    public var requestId: String?
    public var status: String?
    public var text: String?
    public var title: String?
    public init(entries: [PlanEntry]? = nil, markdown: String? = nil, requestId: String? = nil, status: String? = nil, text: String? = nil, title: String? = nil) {
        self.entries = entries
        self.markdown = markdown
        self.requestId = requestId
        self.status = status
        self.text = text
        self.title = title
    }
}
public struct Prompt: Codable, Sendable {
    public var content: [PromptContent]
    public init(content: [PromptContent]) {
        self.content = content
    }
}
public typealias PromptContent = JSONValue
public struct PromptQueue: Codable, Sendable {
    public var items: [QueuedPrompt]
    public var paused: Bool
    public init(items: [QueuedPrompt], paused: Bool) {
        self.items = items
        self.paused = paused
    }
}
public struct Question: Codable, Sendable {
    public var allowText: Bool
    public var id: String
    public var multiple: Bool
    public var options: [QuestionOption]
    public var prompt: String
    public var secret: Bool
    public init(allowText: Bool, id: String, multiple: Bool, options: [QuestionOption], prompt: String, secret: Bool) {
        self.allowText = allowText
        self.id = id
        self.multiple = multiple
        self.options = options
        self.prompt = prompt
        self.secret = secret
    }
}
public struct QuestionOption: Codable, Sendable {
    public var description: String?
    public var id: String
    public var label: String
    public init(description: String? = nil, id: String, label: String) {
        self.description = description
        self.id = id
        self.label = label
    }
}
public struct QueuedPrompt: Codable, Sendable {
    public var createdAtMs: UInt64
    public var id: String
    public var prompt: Prompt
    public init(createdAtMs: UInt64, id: String, prompt: Prompt) {
        self.createdAtMs = createdAtMs
        self.id = id
        self.prompt = prompt
    }
}
public struct RuntimeEvent: Codable, Sendable {
    public var raw: JSONValue?
    public var sequence: UInt64
    public var sessionId: String
    public var source: String
    public var timestampMs: UInt64
    public var turnId: String?
    public var type: String
    public var data: JSONValue?
    public init(raw: JSONValue? = nil, sequence: UInt64, sessionId: String, source: String, timestampMs: UInt64, turnId: String? = nil, type: String, data: JSONValue? = nil) {
        self.raw = raw
        self.sequence = sequence
        self.sessionId = sessionId
        self.source = source
        self.timestampMs = timestampMs
        self.turnId = turnId
        self.type = type
        self.data = data
    }
}
public struct SessionExecutionOptions: Codable, Sendable {
    public var configHome: String?
    public var ephemeral: Bool
    public var mcpServers: [McpServer]
    public var outputSchema: JSONValue
    public var tools: ToolPolicy
    public init(configHome: String? = nil, ephemeral: Bool, mcpServers: [McpServer], outputSchema: JSONValue, tools: ToolPolicy) {
        self.configHome = configHome
        self.ephemeral = ephemeral
        self.mcpServers = mcpServers
        self.outputSchema = outputSchema
        self.tools = tools
    }
}
public struct SessionMetadata: Codable, Sendable {
    public var archived: Bool
    public var createdAtMs: UInt64
    public var forkOrigin: JSONValue
    public var title: String?
    public var updatedAtMs: UInt64
    public init(archived: Bool, createdAtMs: UInt64, forkOrigin: JSONValue, title: String? = nil, updatedAtMs: UInt64) {
        self.archived = archived
        self.createdAtMs = createdAtMs
        self.forkOrigin = forkOrigin
        self.title = title
        self.updatedAtMs = updatedAtMs
    }
}
public struct SessionSnapshot: Codable, Sendable {
    public var capabilities: Capabilities
    public var id: String
    public var metadata: SessionMetadata
    public var native: NativeSessionRef
    public var options: SpawnOptions
    public var queue: PromptQueue
    public var sequence: UInt64
    public var state: SessionState
    public var turn: String?
    public var turnsCompleted: UInt64
    public init(capabilities: Capabilities, id: String, metadata: SessionMetadata, native: NativeSessionRef, options: SpawnOptions, queue: PromptQueue, sequence: UInt64, state: SessionState, turn: String? = nil, turnsCompleted: UInt64) {
        self.capabilities = capabilities
        self.id = id
        self.metadata = metadata
        self.native = native
        self.options = options
        self.queue = queue
        self.sequence = sequence
        self.state = state
        self.turn = turn
        self.turnsCompleted = turnsCompleted
    }
}
public typealias SessionState = String
public struct SpawnOptions: Codable, Sendable {
    public var config: JSONValue
    public var cwd: String
    public var execution: SessionExecutionOptions
    public var harness: String
    public var model: String?
    public init(config: JSONValue, cwd: String, execution: SessionExecutionOptions, harness: String, model: String? = nil) {
        self.config = config
        self.cwd = cwd
        self.execution = execution
        self.harness = harness
        self.model = model
    }
}
public struct SubagentInfo: Codable, Sendable {
    public var background: Bool?
    public var id: String?
    public var model: String?
    public var parentSessionId: String?
    public var parentToolId: String?
    public var role: String?
    public var status: String?
    public var summary: String?
    public var textDelta: String?
    public var title: String?
    public var tokens: UInt64?
    public var usage: JSONValue?
    public init(background: Bool? = nil, id: String? = nil, model: String? = nil, parentSessionId: String? = nil, parentToolId: String? = nil, role: String? = nil, status: String? = nil, summary: String? = nil, textDelta: String? = nil, title: String? = nil, tokens: UInt64? = nil, usage: JSONValue? = nil) {
        self.background = background
        self.id = id
        self.model = model
        self.parentSessionId = parentSessionId
        self.parentToolId = parentToolId
        self.role = role
        self.status = status
        self.summary = summary
        self.textDelta = textDelta
        self.title = title
        self.tokens = tokens
        self.usage = usage
    }
}
public struct ToolDetails: Codable, Sendable {
    public var command: String?
    public var cwd: String?
    public var diffs: [FileDiff]?
    public var kind: String?
    public var locations: [FileLocation]?
    public var progress: JSONValue?
    public var subagent: JSONValue?
    public var title: String?
    public init(command: String? = nil, cwd: String? = nil, diffs: [FileDiff]? = nil, kind: String? = nil, locations: [FileLocation]? = nil, progress: JSONValue? = nil, subagent: JSONValue? = nil, title: String? = nil) {
        self.command = command
        self.cwd = cwd
        self.diffs = diffs
        self.kind = kind
        self.locations = locations
        self.progress = progress
        self.subagent = subagent
        self.title = title
    }
}
public struct ToolEvent: Codable, Sendable {
    public var details: JSONValue?
    public var id: String
    public var input: JSONValue
    public var metadata: JSONValue
    public var name: String?
    public var output: JSONValue
    public var status: String
    public init(details: JSONValue? = nil, id: String, input: JSONValue, metadata: JSONValue, name: String? = nil, output: JSONValue, status: String) {
        self.details = details
        self.id = id
        self.input = input
        self.metadata = metadata
        self.name = name
        self.output = output
        self.status = status
    }
}
public typealias ToolPolicy = String
public struct ToolProgress: Codable, Sendable {
    public var elapsedSeconds: Double?
    public var message: String?
    public var percent: Double?
    public init(elapsedSeconds: Double? = nil, message: String? = nil, percent: Double? = nil) {
        self.elapsedSeconds = elapsedSeconds
        self.message = message
        self.percent = percent
    }
}
public struct Usage: Codable, Sendable {
    public var cacheWriteTokens: UInt64?
    public var cachedInputTokens: UInt64?
    public var costUsd: Double?
    public var inputTokens: UInt64?
    public var outputTokens: UInt64?
    public var scope: String
    public var totalTokens: UInt64?
    public init(cacheWriteTokens: UInt64? = nil, cachedInputTokens: UInt64? = nil, costUsd: Double? = nil, inputTokens: UInt64? = nil, outputTokens: UInt64? = nil, scope: String, totalTokens: UInt64? = nil) {
        self.cacheWriteTokens = cacheWriteTokens
        self.cachedInputTokens = cachedInputTokens
        self.costUsd = costUsd
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
        self.scope = scope
        self.totalTokens = totalTokens
    }
}
public let protocolMethods: [String] = ["agent/commands","agent/configuration","agent/configure","agent/kill","agent/models","agent/quotas","agent/spawn","extension/invoke","extension/list","permission/respond","runtime/discover","runtime/generate","runtime/history","runtime/history/read","runtime/initialize","runtime/probe","runtime/quotas","runtime/quotas/all","runtime/schema","session/cancel","session/close","session/compact","session/create","session/events","session/fork","session/get","session/import","session/list","session/metadata","session/prompt","session/queue","session/queue/list","session/queue/pause","session/queue/remove","session/queue/update","session/resume","session/rollback","session/steer","session/transcript","session/tree"]
