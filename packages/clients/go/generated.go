// Generated from runtime/schema. Run tools/generate.py to refresh.
package swarmie

import "encoding/json"

type AccountQuotaResult = json.RawMessage

type Capabilities struct {
	Cancel                   Capability                 `json:"cancel"`
	Commands                 Capability                 `json:"commands"`
	Compact                  Capability                 `json:"compact"`
	Configure                Capability                 `json:"configure"`
	Ephemeral                Capability                 `json:"ephemeral"`
	Fork                     Capability                 `json:"fork"`
	ForkAtMessage            Capability                 `json:"forkAtMessage"`
	ForkBeforeUser           Capability                 `json:"forkBeforeUser"`
	Images                   Capability                 `json:"images"`
	Metadata                 map[string]json.RawMessage `json:"metadata"`
	Models                   Capability                 `json:"models"`
	Modes                    Capability                 `json:"modes"`
	NativeSkillInputs        Capability                 `json:"nativeSkillInputs"`
	OperationApprovals       Capability                 `json:"operationApprovals"`
	OutputSchema             Capability                 `json:"outputSchema"`
	Permissions              Capability                 `json:"permissions"`
	Queue                    Capability                 `json:"queue"`
	Quotas                   Capability                 `json:"quotas"`
	Reasoning                Capability                 `json:"reasoning"`
	Resume                   Capability                 `json:"resume"`
	Rollback                 Capability                 `json:"rollback"`
	Steering                 Capability                 `json:"steering"`
	SubmissionReconciliation Capability                 `json:"submissionReconciliation"`
	ToolsDisabled            Capability                 `json:"toolsDisabled"`
	Usage                    Capability                 `json:"usage"`
}

type Capability struct {
	Availability CapabilityAvailability `json:"availability"`
	Evidence     CapabilityEvidence     `json:"evidence"`
	Reason       *string                `json:"reason,omitempty"`
	Source       CapabilitySource       `json:"source"`
	SourceId     *string                `json:"sourceId,omitempty"`
}

type CapabilityAvailability string

type CapabilityEvidence string

type CapabilitySource string

type ConfigurationPreview struct {
	Capabilities    Capabilities         `json:"capabilities"`
	Configuration   *[]SettingDescriptor `json:"configuration"`
	ExecutionPolicy ExecutionPolicy      `json:"executionPolicy"`
	Models          *[]ModelDescriptor   `json:"models"`
}

type ConfigurationReceipt struct {
	Configuration   []SettingDescriptor        `json:"configuration"`
	ExecutionPolicy ExecutionPolicy            `json:"executionPolicy"`
	Raw             json.RawMessage            `json:"raw,omitempty"`
	Values          map[string]json.RawMessage `json:"values"`
}

type ContextSource = json.RawMessage

type ContextStatus struct {
	Available bool           `json:"available"`
	Context   WorkingContext `json:"context"`
	Reason    *string        `json:"reason"`
}

type ContextUsage struct {
	ContextWindowTokens *uint64  `json:"contextWindowTokens"`
	Percent             *float64 `json:"percent"`
	RemainingTokens     *uint64  `json:"remainingTokens"`
	UsedTokens          *uint64  `json:"usedTokens"`
}

type DefaultDirectory struct {
	Directory     DirectoryRef `json:"directory"`
	EnvironmentId string       `json:"environmentId"`
	OwnerScopeId  string       `json:"ownerScopeId"`
	Revision      uint64       `json:"revision"`
}

type DeliveryKnowledge string

type Detection struct {
	Available    bool         `json:"available"`
	Capabilities Capabilities `json:"capabilities"`
	Executable   *string      `json:"executable"`
	Harness      string       `json:"harness"`
}

type DirectoryOwnership string

type DirectoryRef struct {
	Id       string `json:"id"`
	Revision uint64 `json:"revision"`
}

type DirectoryResource struct {
	EnvironmentId string             `json:"environmentId"`
	Fingerprint   *string            `json:"fingerprint"`
	Id            string             `json:"id"`
	Ownership     DirectoryOwnership `json:"ownership"`
	Path          string             `json:"path"`
	Revision      uint64             `json:"revision"`
}

type EventKind = json.RawMessage

type ExecutionPolicy struct {
	BindingId          *string                  `json:"bindingId"`
	Choices            []ExecutionProfileChoice `json:"choices"`
	ControlledSettings []string                 `json:"controlledSettings"`
	DispatchBlocked    bool                     `json:"dispatchBlocked"`
	NativeSession      json.RawMessage          `json:"nativeSession"`
	Observed           json.RawMessage          `json:"observed"`
	Planned            json.RawMessage          `json:"planned"`
	PlannedProfile     json.RawMessage          `json:"plannedProfile"`
	Reason             *string                  `json:"reason"`
	Requested          json.RawMessage          `json:"requested"`
	Revision           uint64                   `json:"revision"`
	Status             PolicyStatus             `json:"status"`
}

type ExecutionProfile string

type ExecutionProfileChoice struct {
	Application SettingApplication `json:"application"`
	Capability  Capability         `json:"capability"`
	Description string             `json:"description"`
	Label       string             `json:"label"`
	Mapping     PolicyMapping      `json:"mapping"`
	Profile     ExecutionProfile   `json:"profile"`
}

type ExtensionCommand struct {
	Mutates bool   `json:"mutates"`
	Name    string `json:"name"`
	Title   string `json:"title"`
}

type ExtensionInfo struct {
	Commands []ExtensionCommand `json:"commands"`
	Id       string             `json:"id"`
	Views    []ExtensionView    `json:"views"`
}

type ExtensionView struct {
	Command string `json:"command"`
	Id      string `json:"id"`
	Kind    string `json:"kind"`
	Title   string `json:"title"`
}

type FileDiff struct {
	Change  *string `json:"change,omitempty"`
	NewText *string `json:"newText,omitempty"`
	OldText *string `json:"oldText,omitempty"`
	Path    string  `json:"path"`
	Unified *string `json:"unified,omitempty"`
}

type FileLocation struct {
	Column    *uint64 `json:"column,omitempty"`
	EndColumn *uint64 `json:"endColumn,omitempty"`
	EndLine   *uint64 `json:"endLine,omitempty"`
	Line      *uint64 `json:"line,omitempty"`
	Path      string  `json:"path"`
}

type ForkOrigin struct {
	MessageId *string `json:"messageId"`
	Sequence  uint64  `json:"sequence"`
	SessionId string  `json:"sessionId"`
	TurnId    *string `json:"turnId"`
}

type HarnessProbe struct {
	AuthState    string       `json:"authState"`
	Available    bool         `json:"available"`
	Capabilities Capabilities `json:"capabilities"`
	Executable   *string      `json:"executable"`
	Harness      string       `json:"harness"`
	Message      *string      `json:"message"`
	Version      *string      `json:"version"`
}

type HostToolGrant struct {
	ArgumentConstraints map[string][]json.RawMessage `json:"argumentConstraints"`
	ExtensionId         string                       `json:"extensionId"`
	ToolId              string                       `json:"toolId"`
}

type InputAccountExecutionOptions struct {
	ConfigHome *string           `json:"configHome,omitempty"`
	Env        map[string]string `json:"env,omitempty"`
}

type InputAccountQuery struct {
	Config    json.RawMessage               `json:"config,omitempty"`
	Cwd       string                        `json:"cwd"`
	Execution *InputAccountExecutionOptions `json:"execution,omitempty"`
	Harness   string                        `json:"harness"`
}

type InputDefaultDirectoryUpdate struct {
	Directory        InputDirectoryRef `json:"directory"`
	ExpectedRevision uint64            `json:"expectedRevision"`
}

type InputDirectoryRef struct {
	Id       string `json:"id"`
	Revision uint64 `json:"revision"`
}

type InputDirectoryRegistration struct {
	Path string `json:"path"`
}

type InputHostToolGrant struct {
	ArgumentConstraints map[string][]json.RawMessage `json:"argumentConstraints,omitempty"`
	ExtensionId         string                       `json:"extensionId"`
	ToolId              string                       `json:"toolId"`
}

type InputLocalStoreIdentity struct {
	EnvironmentId string `json:"environmentId"`
	OwnerScopeId  string `json:"ownerScopeId"`
	StoreId       string `json:"storeId"`
}

type InputMcpServer struct {
	Name      string            `json:"name"`
	Transport InputMcpTransport `json:"transport"`
}

type InputMcpTransport = json.RawMessage

type InputNativeHistoryQuery struct {
	ConfigHome *string `json:"configHome,omitempty"`
	Cwd        *string `json:"cwd,omitempty"`
	Harness    string  `json:"harness"`
	Limit      *uint64 `json:"limit,omitempty"`
}

type InputPermissionResponse struct {
	Answers map[string]InputQuestionAnswer `json:"answers,omitempty"`
	Data    json.RawMessage                `json:"data,omitempty"`
	Id      string                         `json:"id"`
	Option  string                         `json:"option"`
}

type InputProjectCreate struct {
	Directory json.RawMessage             `json:"directory,omitempty"`
	Launch    *InputProjectLaunchDefaults `json:"launch,omitempty"`
	Name      string                      `json:"name"`
}

type InputProjectLaunchDefaults struct {
	Config  json.RawMessage `json:"config,omitempty"`
	Harness *string         `json:"harness,omitempty"`
	Model   *string         `json:"model,omitempty"`
}

type InputProjectUpdate struct {
	Archived         *bool           `json:"archived,omitempty"`
	Directory        json.RawMessage `json:"directory,omitempty"`
	ExpectedRevision uint64          `json:"expectedRevision"`
	Launch           json.RawMessage `json:"launch,omitempty"`
	Name             *string         `json:"name,omitempty"`
	ProjectId        string          `json:"projectId"`
	UnlinkDirectory  *bool           `json:"unlinkDirectory,omitempty"`
}

type InputPromptContent = json.RawMessage

type InputQuestionAnswer = json.RawMessage

type InputRollbackScope string

type InputRuntimeIdentity struct {
	Identity      InputLocalStoreIdentity `json:"identity"`
	IncarnationId string                  `json:"incarnationId"`
}

type InputSessionExecutionOptions struct {
	ConfigHome   *string              `json:"configHome,omitempty"`
	Env          map[string]string    `json:"env,omitempty"`
	Ephemeral    *bool                `json:"ephemeral,omitempty"`
	HostTools    []InputHostToolGrant `json:"hostTools,omitempty"`
	McpServers   []InputMcpServer     `json:"mcpServers,omitempty"`
	OutputSchema json.RawMessage      `json:"outputSchema,omitempty"`
	Tools        *InputToolPolicy     `json:"tools,omitempty"`
}

type InputSessionMetadataUpdate struct {
	Archived *bool   `json:"archived,omitempty"`
	Title    *string `json:"title,omitempty"`
}

type InputSpawnOptions struct {
	Config          json.RawMessage               `json:"config,omitempty"`
	Cwd             *string                       `json:"cwd,omitempty"`
	Execution       *InputSessionExecutionOptions `json:"execution,omitempty"`
	Harness         string                        `json:"harness"`
	Model           *string                       `json:"model,omitempty"`
	ProjectId       *string                       `json:"projectId,omitempty"`
	ProjectRevision *uint64                       `json:"projectRevision,omitempty"`
}

type InputToolPolicy string

type LocalStoreIdentity struct {
	EnvironmentId string `json:"environmentId"`
	OwnerScopeId  string `json:"ownerScopeId"`
	StoreId       string `json:"storeId"`
}

type McpServer struct {
	Name      string       `json:"name"`
	Transport McpTransport `json:"transport"`
}

type McpTransport = json.RawMessage

type ModelDescriptor struct {
	Description *string                    `json:"description,omitempty"`
	Id          string                     `json:"id"`
	Label       string                     `json:"label"`
	Metadata    map[string]json.RawMessage `json:"metadata,omitempty"`
	Provider    *string                    `json:"provider,omitempty"`
}

type ModelReroute struct {
	FromModel string  `json:"fromModel"`
	Reason    *string `json:"reason"`
	ToModel   string  `json:"toModel"`
}

type NativeHistoryDocument struct {
	Capabilities Capabilities          `json:"capabilities"`
	Options      SpawnOptions          `json:"options"`
	Records      []NativeHistoryRecord `json:"records"`
	Session      NativeHistorySession  `json:"session"`
	Tree         []NativeMessageNode   `json:"tree"`
}

type NativeHistoryRecord struct {
	Kind   EventKind       `json:"kind"`
	Raw    json.RawMessage `json:"raw"`
	TurnId *string         `json:"turnId"`
}

type NativeHistorySession struct {
	Cwd         string           `json:"cwd"`
	Metadata    json.RawMessage  `json:"metadata"`
	Native      NativeSessionRef `json:"native"`
	Path        string           `json:"path"`
	Title       string           `json:"title"`
	UpdatedAtMs uint64           `json:"updatedAtMs"`
}

type NativeMessageNode struct {
	Active   bool            `json:"active"`
	Id       string          `json:"id"`
	Metadata json.RawMessage `json:"metadata"`
	ParentId *string         `json:"parentId"`
	Role     string          `json:"role"`
	Text     string          `json:"text"`
}

type NativeSessionRef struct {
	Id       string `json:"id"`
	Provider string `json:"provider"`
}

type PermissionOption struct {
	Id    string `json:"id"`
	Kind  string `json:"kind"`
	Label string `json:"label"`
}

type PermissionRequest struct {
	Command     json.RawMessage    `json:"command"`
	Description *string            `json:"description"`
	Id          string             `json:"id"`
	Kind        string             `json:"kind"`
	Metadata    json.RawMessage    `json:"metadata"`
	Options     []PermissionOption `json:"options"`
	Paths       []string           `json:"paths"`
	Questions   []Question         `json:"questions,omitempty"`
	Title       string             `json:"title"`
}

type PlanEntry struct {
	Id       *string `json:"id,omitempty"`
	Priority *string `json:"priority,omitempty"`
	Status   *string `json:"status,omitempty"`
	Text     *string `json:"text,omitempty"`
}

type PlanUpdate struct {
	Entries   []PlanEntry `json:"entries,omitempty"`
	Markdown  *string     `json:"markdown,omitempty"`
	RequestId *string     `json:"requestId,omitempty"`
	Status    *string     `json:"status,omitempty"`
	Text      *string     `json:"text,omitempty"`
	Title     *string     `json:"title,omitempty"`
}

type PolicyEvidence string

type PolicyMapping string

type PolicyObservation struct {
	Coverage      []string        `json:"coverage"`
	Evidence      PolicyEvidence  `json:"evidence"`
	Limitations   []string        `json:"limitations"`
	Native        json.RawMessage `json:"native"`
	NativeVersion *string         `json:"nativeVersion"`
	Profile       json.RawMessage `json:"profile"`
}

type PolicyStatus string

type ProjectLaunchDefaults struct {
	Config  json.RawMessage `json:"config"`
	Harness *string         `json:"harness"`
	Model   *string         `json:"model"`
}

type ProjectRecord struct {
	Archived  bool                  `json:"archived"`
	Directory json.RawMessage       `json:"directory"`
	Id        string                `json:"id"`
	Launch    ProjectLaunchDefaults `json:"launch"`
	Name      string                `json:"name"`
	Revision  uint64                `json:"revision"`
}

type Prompt struct {
	Content []PromptContent `json:"content"`
}

type PromptContent = json.RawMessage

type PromptQueue struct {
	Items  []QueuedPrompt `json:"items"`
	Paused bool           `json:"paused"`
}

type Question struct {
	AllowText bool             `json:"allowText"`
	Id        string           `json:"id"`
	Multiple  bool             `json:"multiple"`
	Options   []QuestionOption `json:"options"`
	Prompt    string           `json:"prompt"`
	Secret    bool             `json:"secret"`
}

type QuestionOption struct {
	Description *string `json:"description,omitempty"`
	Id          string  `json:"id"`
	Label       string  `json:"label"`
}

type QueuedPrompt struct {
	CreatedAtMs uint64 `json:"createdAtMs"`
	Id          string `json:"id"`
	Prompt      Prompt `json:"prompt"`
}

type RuntimeEvent struct {
	Raw         json.RawMessage `json:"raw,omitempty"`
	Sequence    uint64          `json:"sequence"`
	SessionId   string          `json:"sessionId"`
	Source      string          `json:"source"`
	TimestampMs uint64          `json:"timestampMs"`
	TurnId      *string         `json:"turnId"`
	Type        string          `json:"type"`
	Data        json.RawMessage `json:"data,omitempty"`
}

type RuntimeIdentity struct {
	Identity      LocalStoreIdentity `json:"identity"`
	IncarnationId string             `json:"incarnationId"`
}

type SessionExecutionOptions struct {
	ConfigHome   *string         `json:"configHome"`
	Ephemeral    bool            `json:"ephemeral"`
	HostTools    []HostToolGrant `json:"hostTools"`
	McpServers   []McpServer     `json:"mcpServers"`
	OutputSchema json.RawMessage `json:"outputSchema"`
	Tools        ToolPolicy      `json:"tools"`
}

type SessionMetadata struct {
	Archived    bool            `json:"archived"`
	CreatedAtMs uint64          `json:"createdAtMs"`
	ForkOrigin  json.RawMessage `json:"forkOrigin"`
	Title       *string         `json:"title"`
	UpdatedAtMs uint64          `json:"updatedAtMs"`
}

type SessionSnapshot struct {
	Capabilities    Capabilities     `json:"capabilities"`
	ExecutionPolicy ExecutionPolicy  `json:"executionPolicy"`
	Id              string           `json:"id"`
	Metadata        SessionMetadata  `json:"metadata"`
	Native          NativeSessionRef `json:"native"`
	Options         SpawnOptions     `json:"options"`
	Queue           PromptQueue      `json:"queue"`
	Sequence        uint64           `json:"sequence"`
	State           SessionState     `json:"state"`
	Turn            *string          `json:"turn"`
	TurnsCompleted  uint64           `json:"turnsCompleted"`
	WorkingContext  json.RawMessage  `json:"workingContext"`
}

type SessionState string

type SettingApplication string

type SettingChoice struct {
	Description *string                    `json:"description,omitempty"`
	Id          string                     `json:"id"`
	Label       string                     `json:"label"`
	Metadata    map[string]json.RawMessage `json:"metadata,omitempty"`
}

type SettingDescriptor struct {
	Application SettingApplication         `json:"application"`
	Choices     []SettingChoice            `json:"choices,omitempty"`
	Description *string                    `json:"description,omitempty"`
	Id          string                     `json:"id"`
	Label       string                     `json:"label"`
	Metadata    map[string]json.RawMessage `json:"metadata,omitempty"`
	Type        SettingType                `json:"type"`
	Value       json.RawMessage            `json:"value,omitempty"`
}

type SettingType string

type SpawnOptions struct {
	Config          json.RawMessage         `json:"config"`
	Cwd             string                  `json:"cwd"`
	Execution       SessionExecutionOptions `json:"execution"`
	Harness         string                  `json:"harness"`
	Model           *string                 `json:"model"`
	ProjectId       *string                 `json:"projectId"`
	ProjectRevision *uint64                 `json:"projectRevision"`
}

type SubagentInfo struct {
	Background      *bool           `json:"background,omitempty"`
	Id              *string         `json:"id,omitempty"`
	Model           *string         `json:"model,omitempty"`
	ParentSessionId *string         `json:"parentSessionId,omitempty"`
	ParentToolId    *string         `json:"parentToolId,omitempty"`
	Role            *string         `json:"role,omitempty"`
	Status          *string         `json:"status,omitempty"`
	Summary         *string         `json:"summary,omitempty"`
	TextDelta       *string         `json:"textDelta,omitempty"`
	Title           *string         `json:"title,omitempty"`
	Tokens          *uint64         `json:"tokens,omitempty"`
	Usage           json.RawMessage `json:"usage,omitempty"`
}

type SubmissionReceipt struct {
	Delivery         DeliveryKnowledge `json:"delivery"`
	Error            *string           `json:"error"`
	Id               string            `json:"id"`
	IdempotencyKey   string            `json:"idempotencyKey"`
	Outcome          json.RawMessage   `json:"outcome"`
	RecoveryRequired bool              `json:"recoveryRequired"`
	SessionId        string            `json:"sessionId"`
	TurnId           string            `json:"turnId"`
}

type ToolDetails struct {
	Command   *string         `json:"command,omitempty"`
	Cwd       *string         `json:"cwd,omitempty"`
	Diffs     []FileDiff      `json:"diffs,omitempty"`
	Kind      *string         `json:"kind,omitempty"`
	Locations []FileLocation  `json:"locations,omitempty"`
	Progress  json.RawMessage `json:"progress,omitempty"`
	Subagent  json.RawMessage `json:"subagent,omitempty"`
	Title     *string         `json:"title,omitempty"`
}

type ToolEvent struct {
	Details  json.RawMessage `json:"details,omitempty"`
	Id       string          `json:"id"`
	Input    json.RawMessage `json:"input"`
	Metadata json.RawMessage `json:"metadata"`
	Name     *string         `json:"name"`
	Output   json.RawMessage `json:"output"`
	Status   string          `json:"status"`
}

type ToolPolicy string

type ToolProgress struct {
	ElapsedSeconds *float64 `json:"elapsedSeconds,omitempty"`
	Message        *string  `json:"message,omitempty"`
	Percent        *float64 `json:"percent,omitempty"`
}

type TurnOutcome string

type Usage struct {
	CacheWriteTokens  *uint64  `json:"cacheWriteTokens"`
	CachedInputTokens *uint64  `json:"cachedInputTokens"`
	CostUsd           *float64 `json:"costUsd"`
	InputTokens       *uint64  `json:"inputTokens"`
	OutputTokens      *uint64  `json:"outputTokens"`
	Scope             string   `json:"scope"`
	TotalTokens       *uint64  `json:"totalTokens"`
}

type WorkingContext struct {
	BindingId     string            `json:"bindingId"`
	Directory     DirectoryResource `json:"directory"`
	EnvironmentId string            `json:"environmentId"`
	Id            string            `json:"id"`
	Revision      uint64            `json:"revision"`
	SessionId     string            `json:"sessionId"`
	Source        ContextSource     `json:"source"`
}

var Methods = []string{"agent/commands", "agent/configuration", "agent/configure", "agent/kill", "agent/models", "agent/quotas", "agent/spawn", "directory/get", "directory/register", "extension/invoke", "extension/list", "permission/respond", "project/create", "project/get", "project/list", "project/update", "runtime/configuration", "runtime/discover", "runtime/generate", "runtime/history", "runtime/history/read", "runtime/initialize", "runtime/probe", "runtime/quotas", "runtime/quotas/all", "runtime/schema", "runtime/shutdown", "session/cancel", "session/close", "session/compact", "session/context", "session/create", "session/events", "session/fork", "session/get", "session/import", "session/list", "session/metadata", "session/prompt", "session/queue", "session/queue/list", "session/queue/pause", "session/queue/remove", "session/queue/update", "session/resume", "session/rollback", "session/steer", "session/submission", "session/submission/reconcile", "session/transcript", "session/tree", "settings/directory", "settings/setDirectory"}
