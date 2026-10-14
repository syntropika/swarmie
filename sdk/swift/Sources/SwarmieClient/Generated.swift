// Generated from runtime/schema. Run tools/generate.py to refresh.
public typealias AccountQuotaResult = JSONValue
public struct Capabilities: Codable, Sendable {
    public var cancel: Capability
    public var commands: Capability
    public var compact: Capability
    public var configure: Capability
    public var ephemeral: Capability
    public var fork: Capability
    public var forkAtMessage: Capability
    public var forkBeforeUser: Capability
    public var images: Capability
    public var metadata: [String: JSONValue]
    public var models: Capability
    public var modes: Capability
    public var nativeSkillInputs: Capability
    public var operationApprovals: Capability
    public var outputSchema: Capability
    public var permissions: Capability
    public var queue: Capability
    public var quotas: Capability
    public var reasoning: Capability
    public var resume: Capability
    public var rollback: Capability
    public var steering: Capability
    public var submissionReconciliation: Capability
    public var toolsDisabled: Capability
    public var usage: Capability
    public init(cancel: Capability, commands: Capability, compact: Capability, configure: Capability, ephemeral: Capability, fork: Capability, forkAtMessage: Capability, forkBeforeUser: Capability, images: Capability, metadata: [String: JSONValue], models: Capability, modes: Capability, nativeSkillInputs: Capability, operationApprovals: Capability, outputSchema: Capability, permissions: Capability, queue: Capability, quotas: Capability, reasoning: Capability, resume: Capability, rollback: Capability, steering: Capability, submissionReconciliation: Capability, toolsDisabled: Capability, usage: Capability) {
        self.cancel = cancel
        self.commands = commands
        self.compact = compact
        self.configure = configure
        self.ephemeral = ephemeral
        self.fork = fork
        self.forkAtMessage = forkAtMessage
        self.forkBeforeUser = forkBeforeUser
        self.images = images
        self.metadata = metadata
        self.models = models
        self.modes = modes
        self.nativeSkillInputs = nativeSkillInputs
        self.operationApprovals = operationApprovals
        self.outputSchema = outputSchema
        self.permissions = permissions
        self.queue = queue
        self.quotas = quotas
        self.reasoning = reasoning
        self.resume = resume
        self.rollback = rollback
        self.steering = steering
        self.submissionReconciliation = submissionReconciliation
        self.toolsDisabled = toolsDisabled
        self.usage = usage
    }
}
public struct Capability: Codable, Sendable {
    public var availability: CapabilityAvailability
    public var evidence: CapabilityEvidence
    public var reason: String?
    public var source: CapabilitySource
    public var sourceId: String?
    public init(availability: CapabilityAvailability, evidence: CapabilityEvidence, reason: String? = nil, source: CapabilitySource, sourceId: String? = nil) {
        self.availability = availability
        self.evidence = evidence
        self.reason = reason
        self.source = source
        self.sourceId = sourceId
    }
}
public typealias CapabilityAvailability = String
public typealias CapabilityEvidence = String
public typealias CapabilitySource = String
public struct ConfigurationPreview: Codable, Sendable {
    public var capabilities: Capabilities
    public var configuration: [SettingDescriptor]?
    public var executionPolicy: ExecutionPolicy
    public var models: [ModelDescriptor]?
    public init(capabilities: Capabilities, configuration: [SettingDescriptor]? = nil, executionPolicy: ExecutionPolicy, models: [ModelDescriptor]? = nil) {
        self.capabilities = capabilities
        self.configuration = configuration
        self.executionPolicy = executionPolicy
        self.models = models
    }
}
public struct ConfigurationReceipt: Codable, Sendable {
    public var configuration: [SettingDescriptor]
    public var executionPolicy: ExecutionPolicy
    public var raw: JSONValue?
    public var values: [String: JSONValue]
    public init(configuration: [SettingDescriptor], executionPolicy: ExecutionPolicy, raw: JSONValue? = nil, values: [String: JSONValue]) {
        self.configuration = configuration
        self.executionPolicy = executionPolicy
        self.raw = raw
        self.values = values
    }
}
public typealias ContextSource = JSONValue
public struct ContextStatus: Codable, Sendable {
    public var available: Bool
    public var context: WorkingContext
    public var reason: String?
    public init(available: Bool, context: WorkingContext, reason: String? = nil) {
        self.available = available
        self.context = context
        self.reason = reason
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
public struct DefaultDirectory: Codable, Sendable {
    public var directory: DirectoryRef
    public var environmentId: String
    public var ownerScopeId: String
    public var revision: UInt64
    public init(directory: DirectoryRef, environmentId: String, ownerScopeId: String, revision: UInt64) {
        self.directory = directory
        self.environmentId = environmentId
        self.ownerScopeId = ownerScopeId
        self.revision = revision
    }
}
public typealias DeliveryKnowledge = String
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
public typealias DirectoryOwnership = String
public struct DirectoryRef: Codable, Sendable {
    public var id: String
    public var revision: UInt64
    public init(id: String, revision: UInt64) {
        self.id = id
        self.revision = revision
    }
}
public struct DirectoryResource: Codable, Sendable {
    public var environmentId: String
    public var fingerprint: String?
    public var id: String
    public var ownership: DirectoryOwnership
    public var path: String
    public var revision: UInt64
    public init(environmentId: String, fingerprint: String? = nil, id: String, ownership: DirectoryOwnership, path: String, revision: UInt64) {
        self.environmentId = environmentId
        self.fingerprint = fingerprint
        self.id = id
        self.ownership = ownership
        self.path = path
        self.revision = revision
    }
}
public typealias EventKind = JSONValue
public struct ExecutionPolicy: Codable, Sendable {
    public var bindingId: String?
    public var choices: [ExecutionProfileChoice]
    public var controlledSettings: [String]
    public var dispatchBlocked: Bool
    public var nativeSession: JSONValue
    public var observed: JSONValue
    public var planned: JSONValue
    public var plannedProfile: JSONValue
    public var reason: String?
    public var requested: JSONValue
    public var revision: UInt64
    public var status: PolicyStatus
    public init(bindingId: String? = nil, choices: [ExecutionProfileChoice], controlledSettings: [String], dispatchBlocked: Bool, nativeSession: JSONValue, observed: JSONValue, planned: JSONValue, plannedProfile: JSONValue, reason: String? = nil, requested: JSONValue, revision: UInt64, status: PolicyStatus) {
        self.bindingId = bindingId
        self.choices = choices
        self.controlledSettings = controlledSettings
        self.dispatchBlocked = dispatchBlocked
        self.nativeSession = nativeSession
        self.observed = observed
        self.planned = planned
        self.plannedProfile = plannedProfile
        self.reason = reason
        self.requested = requested
        self.revision = revision
        self.status = status
    }
}
public typealias ExecutionProfile = String
public struct ExecutionProfileChoice: Codable, Sendable {
    public var application: SettingApplication
    public var capability: Capability
    public var description: String
    public var label: String
    public var mapping: PolicyMapping
    public var profile: ExecutionProfile
    public init(application: SettingApplication, capability: Capability, description: String, label: String, mapping: PolicyMapping, profile: ExecutionProfile) {
        self.application = application
        self.capability = capability
        self.description = description
        self.label = label
        self.mapping = mapping
        self.profile = profile
    }
}
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
    public var ui: JSONValue?
    public var views: [ExtensionView]
    public init(commands: [ExtensionCommand], id: String, ui: JSONValue? = nil, views: [ExtensionView]) {
        self.commands = commands
        self.id = id
        self.ui = ui
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
public struct HostToolGrant: Codable, Sendable {
    public var argumentConstraints: [String: [JSONValue]]
    public var extensionId: String
    public var toolId: String
    public init(argumentConstraints: [String: [JSONValue]], extensionId: String, toolId: String) {
        self.argumentConstraints = argumentConstraints
        self.extensionId = extensionId
        self.toolId = toolId
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
public struct InputDefaultDirectoryUpdate: Codable, Sendable {
    public var directory: InputDirectoryRef
    public var expectedRevision: UInt64
    public init(directory: InputDirectoryRef, expectedRevision: UInt64) {
        self.directory = directory
        self.expectedRevision = expectedRevision
    }
}
public struct InputDirectoryRef: Codable, Sendable {
    public var id: String
    public var revision: UInt64
    public init(id: String, revision: UInt64) {
        self.id = id
        self.revision = revision
    }
}
public struct InputDirectoryRegistration: Codable, Sendable {
    public var path: String
    public init(path: String) {
        self.path = path
    }
}
public struct InputHostToolGrant: Codable, Sendable {
    public var argumentConstraints: [String: [JSONValue]]?
    public var extensionId: String
    public var toolId: String
    public init(argumentConstraints: [String: [JSONValue]]? = nil, extensionId: String, toolId: String) {
        self.argumentConstraints = argumentConstraints
        self.extensionId = extensionId
        self.toolId = toolId
    }
}
public struct InputLocalStoreIdentity: Codable, Sendable {
    public var environmentId: String
    public var ownerScopeId: String
    public var storeId: String
    public init(environmentId: String, ownerScopeId: String, storeId: String) {
        self.environmentId = environmentId
        self.ownerScopeId = ownerScopeId
        self.storeId = storeId
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
public struct InputProjectCreate: Codable, Sendable {
    public var directory: JSONValue?
    public var launch: InputProjectLaunchDefaults?
    public var name: String
    public init(directory: JSONValue? = nil, launch: InputProjectLaunchDefaults? = nil, name: String) {
        self.directory = directory
        self.launch = launch
        self.name = name
    }
}
public struct InputProjectLaunchDefaults: Codable, Sendable {
    public var config: JSONValue?
    public var harness: String?
    public var model: String?
    public init(config: JSONValue? = nil, harness: String? = nil, model: String? = nil) {
        self.config = config
        self.harness = harness
        self.model = model
    }
}
public struct InputProjectUpdate: Codable, Sendable {
    public var archived: Bool?
    public var directory: JSONValue?
    public var expectedRevision: UInt64
    public var launch: JSONValue?
    public var name: String?
    public var projectId: String
    public var unlinkDirectory: Bool?
    public init(archived: Bool? = nil, directory: JSONValue? = nil, expectedRevision: UInt64, launch: JSONValue? = nil, name: String? = nil, projectId: String, unlinkDirectory: Bool? = nil) {
        self.archived = archived
        self.directory = directory
        self.expectedRevision = expectedRevision
        self.launch = launch
        self.name = name
        self.projectId = projectId
        self.unlinkDirectory = unlinkDirectory
    }
}
public typealias InputPromptContent = JSONValue
public typealias InputQuestionAnswer = JSONValue
public typealias InputRollbackScope = String
public struct InputRuntimeIdentity: Codable, Sendable {
    public var identity: InputLocalStoreIdentity
    public var incarnationId: String
    public init(identity: InputLocalStoreIdentity, incarnationId: String) {
        self.identity = identity
        self.incarnationId = incarnationId
    }
}
public struct InputSessionExecutionOptions: Codable, Sendable {
    public var configHome: String?
    public var env: [String: String]?
    public var ephemeral: Bool?
    public var hostTools: [InputHostToolGrant]?
    public var mcpServers: [InputMcpServer]?
    public var outputSchema: JSONValue?
    public var tools: InputToolPolicy?
    public init(configHome: String? = nil, env: [String: String]? = nil, ephemeral: Bool? = nil, hostTools: [InputHostToolGrant]? = nil, mcpServers: [InputMcpServer]? = nil, outputSchema: JSONValue? = nil, tools: InputToolPolicy? = nil) {
        self.configHome = configHome
        self.env = env
        self.ephemeral = ephemeral
        self.hostTools = hostTools
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
    public var cwd: String?
    public var execution: InputSessionExecutionOptions?
    public var harness: String
    public var model: String?
    public var projectId: String?
    public var projectRevision: UInt64?
    public init(config: JSONValue? = nil, cwd: String? = nil, execution: InputSessionExecutionOptions? = nil, harness: String, model: String? = nil, projectId: String? = nil, projectRevision: UInt64? = nil) {
        self.config = config
        self.cwd = cwd
        self.execution = execution
        self.harness = harness
        self.model = model
        self.projectId = projectId
        self.projectRevision = projectRevision
    }
}
public typealias InputToolPolicy = String
public struct LocalStoreIdentity: Codable, Sendable {
    public var environmentId: String
    public var ownerScopeId: String
    public var storeId: String
    public init(environmentId: String, ownerScopeId: String, storeId: String) {
        self.environmentId = environmentId
        self.ownerScopeId = ownerScopeId
        self.storeId = storeId
    }
}
public struct McpServer: Codable, Sendable {
    public var name: String
    public var transport: McpTransport
    public init(name: String, transport: McpTransport) {
        self.name = name
        self.transport = transport
    }
}
public typealias McpTransport = JSONValue
public struct ModelDescriptor: Codable, Sendable {
    public var description: String?
    public var id: String
    public var label: String
    public var metadata: [String: JSONValue]?
    public var provider: String?
    public init(description: String? = nil, id: String, label: String, metadata: [String: JSONValue]? = nil, provider: String? = nil) {
        self.description = description
        self.id = id
        self.label = label
        self.metadata = metadata
        self.provider = provider
    }
}
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
public typealias PolicyEvidence = String
public typealias PolicyMapping = String
public struct PolicyObservation: Codable, Sendable {
    public var coverage: [String]
    public var evidence: PolicyEvidence
    public var limitations: [String]
    public var native: JSONValue
    public var nativeVersion: String?
    public var profile: JSONValue
    public init(coverage: [String], evidence: PolicyEvidence, limitations: [String], native: JSONValue, nativeVersion: String? = nil, profile: JSONValue) {
        self.coverage = coverage
        self.evidence = evidence
        self.limitations = limitations
        self.native = native
        self.nativeVersion = nativeVersion
        self.profile = profile
    }
}
public typealias PolicyStatus = String
public struct ProjectLaunchDefaults: Codable, Sendable {
    public var config: JSONValue
    public var harness: String?
    public var model: String?
    public init(config: JSONValue, harness: String? = nil, model: String? = nil) {
        self.config = config
        self.harness = harness
        self.model = model
    }
}
public struct ProjectRecord: Codable, Sendable {
    public var archived: Bool
    public var directory: JSONValue
    public var id: String
    public var launch: ProjectLaunchDefaults
    public var name: String
    public var revision: UInt64
    public init(archived: Bool, directory: JSONValue, id: String, launch: ProjectLaunchDefaults, name: String, revision: UInt64) {
        self.archived = archived
        self.directory = directory
        self.id = id
        self.launch = launch
        self.name = name
        self.revision = revision
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
public struct RuntimeIdentity: Codable, Sendable {
    public var identity: LocalStoreIdentity
    public var incarnationId: String
    public init(identity: LocalStoreIdentity, incarnationId: String) {
        self.identity = identity
        self.incarnationId = incarnationId
    }
}
public struct SessionExecutionOptions: Codable, Sendable {
    public var configHome: String?
    public var ephemeral: Bool
    public var hostTools: [HostToolGrant]
    public var mcpServers: [McpServer]
    public var outputSchema: JSONValue
    public var tools: ToolPolicy
    public init(configHome: String? = nil, ephemeral: Bool, hostTools: [HostToolGrant], mcpServers: [McpServer], outputSchema: JSONValue, tools: ToolPolicy) {
        self.configHome = configHome
        self.ephemeral = ephemeral
        self.hostTools = hostTools
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
    public var executionPolicy: ExecutionPolicy
    public var id: String
    public var metadata: SessionMetadata
    public var native: NativeSessionRef
    public var options: SpawnOptions
    public var queue: PromptQueue
    public var sequence: UInt64
    public var state: SessionState
    public var turn: String?
    public var turnsCompleted: UInt64
    public var workingContext: JSONValue
    public init(capabilities: Capabilities, executionPolicy: ExecutionPolicy, id: String, metadata: SessionMetadata, native: NativeSessionRef, options: SpawnOptions, queue: PromptQueue, sequence: UInt64, state: SessionState, turn: String? = nil, turnsCompleted: UInt64, workingContext: JSONValue) {
        self.capabilities = capabilities
        self.executionPolicy = executionPolicy
        self.id = id
        self.metadata = metadata
        self.native = native
        self.options = options
        self.queue = queue
        self.sequence = sequence
        self.state = state
        self.turn = turn
        self.turnsCompleted = turnsCompleted
        self.workingContext = workingContext
    }
}
public typealias SessionState = String
public typealias SettingApplication = String
public struct SettingChoice: Codable, Sendable {
    public var description: String?
    public var id: String
    public var label: String
    public var metadata: [String: JSONValue]?
    public init(description: String? = nil, id: String, label: String, metadata: [String: JSONValue]? = nil) {
        self.description = description
        self.id = id
        self.label = label
        self.metadata = metadata
    }
}
public struct SettingDescriptor: Codable, Sendable {
    public var application: SettingApplication
    public var choices: [SettingChoice]?
    public var description: String?
    public var id: String
    public var label: String
    public var metadata: [String: JSONValue]?
    public var type: SettingType
    public var value: JSONValue?
    public init(application: SettingApplication, choices: [SettingChoice]? = nil, description: String? = nil, id: String, label: String, metadata: [String: JSONValue]? = nil, type: SettingType, value: JSONValue? = nil) {
        self.application = application
        self.choices = choices
        self.description = description
        self.id = id
        self.label = label
        self.metadata = metadata
        self.type = type
        self.value = value
    }
}
public typealias SettingType = String
public struct SpawnOptions: Codable, Sendable {
    public var config: JSONValue
    public var cwd: String
    public var execution: SessionExecutionOptions
    public var harness: String
    public var model: String?
    public var projectId: String?
    public var projectRevision: UInt64?
    public init(config: JSONValue, cwd: String, execution: SessionExecutionOptions, harness: String, model: String? = nil, projectId: String? = nil, projectRevision: UInt64? = nil) {
        self.config = config
        self.cwd = cwd
        self.execution = execution
        self.harness = harness
        self.model = model
        self.projectId = projectId
        self.projectRevision = projectRevision
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
public struct SubmissionReceipt: Codable, Sendable {
    public var delivery: DeliveryKnowledge
    public var error: String?
    public var id: String
    public var idempotencyKey: String
    public var outcome: JSONValue
    public var recoveryRequired: Bool
    public var sessionId: String
    public var turnId: String
    public init(delivery: DeliveryKnowledge, error: String? = nil, id: String, idempotencyKey: String, outcome: JSONValue, recoveryRequired: Bool, sessionId: String, turnId: String) {
        self.delivery = delivery
        self.error = error
        self.id = id
        self.idempotencyKey = idempotencyKey
        self.outcome = outcome
        self.recoveryRequired = recoveryRequired
        self.sessionId = sessionId
        self.turnId = turnId
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
public typealias TurnOutcome = String
public typealias UiClient = String
public struct UiCommand: Codable, Sendable {
    public var id: String
    public var runtimeCommand: String?
    public var shortcut: String?
    public var title: String
    public var when: [UiCondition]
    public init(id: String, runtimeCommand: String? = nil, shortcut: String? = nil, title: String, when: [UiCondition]) {
        self.id = id
        self.runtimeCommand = runtimeCommand
        self.shortcut = shortcut
        self.title = title
        self.when = when
    }
}
public typealias UiCondition = String
public struct UiContribution: Codable, Sendable {
    public var clients: [UiClient]
    public var command: String?
    public var enabledWhen: [UiCondition]
    public var icon: JSONValue?
    public var id: String
    public var kind: UiContributionKind
    public var order: Int64
    public var slot: UiSlot
    public var text: String?
    public var title: String
    public var when: [UiCondition]
    public init(clients: [UiClient], command: String? = nil, enabledWhen: [UiCondition], icon: JSONValue? = nil, id: String, kind: UiContributionKind, order: Int64, slot: UiSlot, text: String? = nil, title: String, when: [UiCondition]) {
        self.clients = clients
        self.command = command
        self.enabledWhen = enabledWhen
        self.icon = icon
        self.id = id
        self.kind = kind
        self.order = order
        self.slot = slot
        self.text = text
        self.title = title
        self.when = when
    }
}
public typealias UiContributionKind = String
public typealias UiIcon = String
public struct UiManifest: Codable, Sendable {
    public var commands: [UiCommand]
    public var panels: [UiSurface]
    public var screens: [UiSurface]
    public var slots: [UiContribution]
    public var version: UInt64
    public init(commands: [UiCommand], panels: [UiSurface], screens: [UiSurface], slots: [UiContribution], version: UInt64) {
        self.commands = commands
        self.panels = panels
        self.screens = screens
        self.slots = slots
        self.version = version
    }
}
public typealias UiSlot = String
public struct UiSurface: Codable, Sendable {
    public var clients: [UiClient]
    public var command: String?
    public var id: String
    public var title: String
    public init(clients: [UiClient], command: String? = nil, id: String, title: String) {
        self.clients = clients
        self.command = command
        self.id = id
        self.title = title
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
public struct WorkingContext: Codable, Sendable {
    public var bindingId: String
    public var directory: DirectoryResource
    public var environmentId: String
    public var id: String
    public var revision: UInt64
    public var sessionId: String
    public var source: ContextSource
    public init(bindingId: String, directory: DirectoryResource, environmentId: String, id: String, revision: UInt64, sessionId: String, source: ContextSource) {
        self.bindingId = bindingId
        self.directory = directory
        self.environmentId = environmentId
        self.id = id
        self.revision = revision
        self.sessionId = sessionId
        self.source = source
    }
}
public let protocolMethods: [String] = ["agent/commands","agent/configuration","agent/configure","agent/kill","agent/models","agent/quotas","agent/spawn","directory/get","directory/register","extension/invoke","extension/list","permission/respond","project/create","project/get","project/list","project/update","runtime/configuration","runtime/discover","runtime/generate","runtime/history","runtime/history/read","runtime/initialize","runtime/probe","runtime/quotas","runtime/quotas/all","runtime/schema","runtime/shutdown","session/cancel","session/close","session/compact","session/context","session/create","session/events","session/fork","session/get","session/import","session/list","session/metadata","session/prompt","session/queue","session/queue/list","session/queue/pause","session/queue/remove","session/queue/update","session/resume","session/rollback","session/steer","session/submission","session/submission/reconcile","session/transcript","session/tree","settings/directory","settings/setDirectory"]
