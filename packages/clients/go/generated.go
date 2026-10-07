// Generated from runtime/schema. Run tools/generate.py to refresh.
package mpai

import "encoding/json"

type AccountQuotaResult = json.RawMessage

type Capabilities struct {
	Cancel      bool                       `json:"cancel"`
	Compact     bool                       `json:"compact"`
	Configure   bool                       `json:"configure"`
	Fork        bool                       `json:"fork"`
	Images      bool                       `json:"images"`
	Metadata    map[string]json.RawMessage `json:"metadata"`
	Models      bool                       `json:"models"`
	Modes       bool                       `json:"modes"`
	Permissions bool                       `json:"permissions"`
	Reasoning   bool                       `json:"reasoning"`
	Resume      bool                       `json:"resume"`
	Rollback    bool                       `json:"rollback"`
	Steering    bool                       `json:"steering"`
	Usage       bool                       `json:"usage"`
}

type ContextUsage struct {
	ContextWindowTokens *uint64  `json:"contextWindowTokens"`
	Percent             *float64 `json:"percent"`
	RemainingTokens     *uint64  `json:"remainingTokens"`
	UsedTokens          *uint64  `json:"usedTokens"`
}

type Detection struct {
	Available    bool         `json:"available"`
	Capabilities Capabilities `json:"capabilities"`
	Executable   *string      `json:"executable"`
	Harness      string       `json:"harness"`
}

type EventKind = json.RawMessage

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

type InputPromptContent = json.RawMessage

type InputQuestionAnswer = json.RawMessage

type InputRollbackScope string

type InputSessionExecutionOptions struct {
	ConfigHome   *string           `json:"configHome,omitempty"`
	Env          map[string]string `json:"env,omitempty"`
	Ephemeral    *bool             `json:"ephemeral,omitempty"`
	McpServers   []InputMcpServer  `json:"mcpServers,omitempty"`
	OutputSchema json.RawMessage   `json:"outputSchema,omitempty"`
	Tools        *InputToolPolicy  `json:"tools,omitempty"`
}

type InputSessionMetadataUpdate struct {
	Archived *bool   `json:"archived,omitempty"`
	Title    *string `json:"title,omitempty"`
}

type InputSpawnOptions struct {
	Config    json.RawMessage               `json:"config,omitempty"`
	Cwd       string                        `json:"cwd"`
	Execution *InputSessionExecutionOptions `json:"execution,omitempty"`
	Harness   string                        `json:"harness"`
	Model     *string                       `json:"model,omitempty"`
}

type InputToolPolicy string

type McpServer struct {
	Name      string       `json:"name"`
	Transport McpTransport `json:"transport"`
}

type McpTransport = json.RawMessage

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

type SessionExecutionOptions struct {
	ConfigHome   *string         `json:"configHome"`
	Ephemeral    bool            `json:"ephemeral"`
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
	Capabilities   Capabilities     `json:"capabilities"`
	Id             string           `json:"id"`
	Metadata       SessionMetadata  `json:"metadata"`
	Native         NativeSessionRef `json:"native"`
	Options        SpawnOptions     `json:"options"`
	Queue          PromptQueue      `json:"queue"`
	Sequence       uint64           `json:"sequence"`
	State          SessionState     `json:"state"`
	Turn           *string          `json:"turn"`
	TurnsCompleted uint64           `json:"turnsCompleted"`
}

type SessionState string

type SpawnOptions struct {
	Config    json.RawMessage         `json:"config"`
	Cwd       string                  `json:"cwd"`
	Execution SessionExecutionOptions `json:"execution"`
	Harness   string                  `json:"harness"`
	Model     *string                 `json:"model"`
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

type Usage struct {
	CacheWriteTokens  *uint64  `json:"cacheWriteTokens"`
	CachedInputTokens *uint64  `json:"cachedInputTokens"`
	CostUsd           *float64 `json:"costUsd"`
	InputTokens       *uint64  `json:"inputTokens"`
	OutputTokens      *uint64  `json:"outputTokens"`
	Scope             string   `json:"scope"`
	TotalTokens       *uint64  `json:"totalTokens"`
}

var Methods = []string{"agent/commands", "agent/configuration", "agent/configure", "agent/kill", "agent/models", "agent/quotas", "agent/spawn", "extension/invoke", "extension/list", "permission/respond", "runtime/discover", "runtime/generate", "runtime/history", "runtime/history/read", "runtime/initialize", "runtime/probe", "runtime/quotas", "runtime/quotas/all", "runtime/schema", "session/cancel", "session/close", "session/compact", "session/create", "session/events", "session/fork", "session/get", "session/import", "session/list", "session/metadata", "session/prompt", "session/queue", "session/queue/list", "session/queue/pause", "session/queue/remove", "session/queue/update", "session/resume", "session/rollback", "session/steer", "session/transcript", "session/tree"}
