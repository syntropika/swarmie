/** Assistant UI concept inventory reviewed on 2026-10-01; implementations use Swarmie contracts. */
export const ASSISTANT_ELEMENT_CATALOG = [
  {
    slug: "loading-state",
    title: "Loader",
    group: "Reasoning",
  },
  {
    slug: "thinking-indicator",
    title: "Thinking indicator",
    group: "Reasoning",
  },
  {
    slug: "streaming-text",
    title: "Streaming text",
    group: "Reasoning",
  },
  {
    slug: "typing-indicator",
    title: "Typing indicator",
    group: "Reasoning",
  },
  {
    slug: "reasoning-effort",
    title: "Reasoning effort",
    group: "Reasoning",
  },
  {
    slug: "guardrail-notice",
    title: "Guardrail notice",
    group: "Reasoning",
  },
  {
    slug: "message-pair",
    title: "Message pair",
    group: "Messages",
  },
  {
    slug: "message-branches",
    title: "Message branches",
    group: "Messages",
  },
  {
    slug: "message-actions",
    title: "Message actions",
    group: "Messages",
  },
  {
    slug: "error-state",
    title: "Error state",
    group: "Messages",
  },
  {
    slug: "message-queue",
    title: "Message queue",
    group: "Messages",
  },
  {
    slug: "edit-message",
    title: "Edit a sent message",
    group: "Messages",
  },
  {
    slug: "feedback-dialog",
    title: "Feedback dialog",
    group: "Messages",
  },
  {
    slug: "stopped-run",
    title: "Stopped run",
    group: "Messages",
  },
  {
    slug: "day-separator",
    title: "Timestamps",
    group: "Messages",
  },
  {
    slug: "speaker-identity",
    title: "Speaker identity",
    group: "Messages",
  },
  {
    slug: "regenerate-menu",
    title: "Regenerate with",
    group: "Messages",
  },
  {
    slug: "confidence-marker",
    title: "Confidence",
    group: "Messages",
  },
  {
    slug: "tool-call",
    title: "Tool call",
    group: "Tool use",
  },
  {
    slug: "tool-timeline",
    title: "Tool timeline",
    group: "Tool use",
  },
  {
    slug: "terminal-block",
    title: "Terminal block",
    group: "Tool use",
  },
  {
    slug: "code-diff",
    title: "Code diff",
    group: "Tool use",
  },
  {
    slug: "reviewable-diff",
    title: "Reviewable diff",
    group: "Tool use",
  },
  {
    slug: "file-tree",
    title: "File tree",
    group: "Tool use",
  },
  {
    slug: "elicitation-form",
    title: "Elicitation form",
    group: "Tool use",
  },
  {
    slug: "mcp-server-panel",
    title: "Server panel",
    group: "Tool use",
  },
  {
    slug: "tool-error",
    title: "Tool failure",
    group: "Tool use",
  },
  {
    slug: "permission-grant",
    title: "Permission grant",
    group: "Tool use",
  },
  {
    slug: "computer-use",
    title: "Computer use",
    group: "Tool use",
  },
  {
    slug: "code-runner",
    title: "Code runner",
    group: "Tool use",
  },
  {
    slug: "web-search",
    title: "Web search",
    group: "Knowledge",
  },
  {
    slug: "inline-citation",
    title: "Inline citation",
    group: "Knowledge",
  },
  {
    slug: "link-preview",
    title: "Link preview",
    group: "Knowledge",
  },
  {
    slug: "image-generation",
    title: "Image generation",
    group: "Knowledge",
  },
  {
    slug: "image-gallery",
    title: "Image gallery",
    group: "Knowledge",
  },
  {
    slug: "retrieval-chunks",
    title: "Retrieval chunks",
    group: "Knowledge",
  },
  {
    slug: "document-reference",
    title: "Document reference",
    group: "Knowledge",
  },
  {
    slug: "memory-chips",
    title: "Memory",
    group: "Knowledge",
  },
  {
    slug: "research-report",
    title: "Research report",
    group: "Knowledge",
  },
  {
    slug: "map-answer",
    title: "Map",
    group: "Knowledge",
  },
  {
    slug: "geo-map",
    title: "Geo map",
    group: "Knowledge",
  },
  {
    slug: "data-table",
    title: "Data table",
    group: "Structured output",
  },
  {
    slug: "number-ticker",
    title: "Number ticker",
    group: "Structured output",
  },
  {
    slug: "chart",
    title: "Chart",
    group: "Structured output",
  },
  {
    slug: "media-player",
    title: "Media player",
    group: "Structured output",
  },
  {
    slug: "web-preview",
    title: "Web preview",
    group: "Structured output",
  },
  {
    slug: "diagram",
    title: "Diagram",
    group: "Structured output",
  },
  {
    slug: "flow-graph",
    title: "Flow graph",
    group: "Structured output",
  },
  {
    slug: "activity-graph",
    title: "Activity graph",
    group: "Structured output",
  },
  {
    slug: "math-block",
    title: "Math",
    group: "Structured output",
  },
  {
    slug: "spec-sheet",
    title: "Spec sheet",
    group: "Structured output",
  },
  {
    slug: "comparison-card",
    title: "Comparison",
    group: "Structured output",
  },
  {
    slug: "timeline",
    title: "Timeline",
    group: "Structured output",
  },
  {
    slug: "job-progress",
    title: "Job progress",
    group: "Structured output",
  },
  {
    slug: "score-breakdown",
    title: "Score breakdown",
    group: "Structured output",
  },
  {
    slug: "agent-plan",
    title: "Agent plan",
    group: "Agents",
  },
  {
    slug: "subagent-list",
    title: "Subagent list",
    group: "Agents",
  },
  {
    slug: "task-card",
    title: "Task card",
    group: "Agents",
  },
  {
    slug: "agent-status",
    title: "Agent status",
    group: "Agents",
  },
  {
    slug: "approval-card",
    title: "Approval card",
    group: "Agents",
  },
  {
    slug: "option-list",
    title: "Option list",
    group: "Agents",
  },
  {
    slug: "question-flow",
    title: "Question flow",
    group: "Agents",
  },
  {
    slug: "recommendation-card",
    title: "Recommendation card",
    group: "Agents",
  },
  {
    slug: "artifact-card",
    title: "Artifact card",
    group: "Agents",
  },
  {
    slug: "todo-list",
    title: "Todo list",
    group: "Agents",
  },
  {
    slug: "agent-card",
    title: "Agent card",
    group: "Agents",
  },
  {
    slug: "agent-handoff",
    title: "Handoff",
    group: "Agents",
  },
  {
    slug: "background-inbox",
    title: "Background runs",
    group: "Agents",
  },
  {
    slug: "checkpoint-history",
    title: "Checkpoints",
    group: "Agents",
  },
  {
    slug: "schedule-card",
    title: "Schedule",
    group: "Agents",
  },
  {
    slug: "trace-waterfall",
    title: "Trace waterfall",
    group: "Observability",
  },
  {
    slug: "cost-meter",
    title: "Cost meter",
    group: "Observability",
  },
  {
    slug: "quota-banner",
    title: "Quota banner",
    group: "Observability",
  },
  {
    slug: "composer",
    title: "Composer",
    group: "Composer",
  },
  {
    slug: "composer-slash-commands",
    title: "Slash commands",
    group: "Composer",
  },
  {
    slug: "composer-mentions",
    title: "Mentions",
    group: "Composer",
  },
  {
    slug: "composer-attachments",
    title: "Attachments",
    group: "Composer",
  },
  {
    slug: "composer-model-picker",
    title: "Models",
    group: "Composer",
  },
  {
    slug: "composer-voice",
    title: "Dictation",
    group: "Composer",
  },
  {
    slug: "composer-context",
    title: "Context",
    group: "Composer",
  },
  {
    slug: "draft-restore",
    title: "Draft restore",
    group: "Composer",
  },
  {
    slug: "context-breakdown",
    title: "Context breakdown",
    group: "Composer",
  },
  {
    slug: "prompt-library",
    title: "Prompt library",
    group: "Composer",
  },
  {
    slug: "command-palette",
    title: "Command palette",
    group: "Composer",
  },
  {
    slug: "voice-conversation",
    title: "Voice conversation",
    group: "Voice",
  },
  {
    slug: "read-aloud",
    title: "Read aloud",
    group: "Voice",
  },
  {
    slug: "chat-panel",
    title: "Chat panel",
    group: "Thread",
  },
  {
    slug: "empty-state",
    title: "Empty state",
    group: "Thread",
  },
  {
    slug: "scroll-anchor",
    title: "Scroll anchor",
    group: "Thread",
  },
  {
    slug: "canvas-split",
    title: "Canvas",
    group: "Thread",
  },
  {
    slug: "connection-state",
    title: "Connection state",
    group: "Thread",
  },
  {
    slug: "shared-conversation",
    title: "Shared conversation",
    group: "Thread",
  },
  {
    slug: "conversation-search",
    title: "Search in conversation",
    group: "Thread",
  },
  {
    slug: "thread-search",
    title: "Thread search",
    group: "Thread",
  },
  {
    slug: "launcher-bubble",
    title: "Launcher",
    group: "Thread",
  },
  {
    slug: "settings-panel",
    title: "Settings",
    group: "Thread",
  },
  {
    slug: "onboarding",
    title: "Onboarding",
    group: "Thread",
  },
  {
    slug: "mobile-composer",
    title: "Mobile composer",
    group: "Thread",
  },
  {
    slug: "thread",
    title: "Thread",
    group: "AUI connected",
  },
  {
    slug: "assistant-modal",
    title: "Assistant modal",
    group: "AUI connected",
  },
  {
    slug: "assistant-sidebar",
    title: "Assistant sidebar",
    group: "AUI connected",
  },
  {
    slug: "thread-list",
    title: "Thread list",
    group: "AUI connected",
  },
  {
    slug: "thread-list-sidebar",
    title: "Thread list sidebar",
    group: "AUI connected",
  },
  {
    slug: "orb",
    title: "Orb",
    group: "AUI connected",
  },
  {
    slug: "reasoning",
    title: "Reasoning",
    group: "AUI connected",
  },
  {
    slug: "message-timing",
    title: "Message timing",
    group: "AUI connected",
  },
  {
    slug: "conversation-map",
    title: "Conversation map",
    group: "AUI connected",
  },
  {
    slug: "context-display",
    title: "Context display",
    group: "AUI connected",
  },
  {
    slug: "mcp-config",
    title: "MCP config dialog",
    group: "AUI connected",
  },
  {
    slug: "attachment",
    title: "Attachment",
    group: "AUI connected",
  },
  {
    slug: "follow-up-suggestions",
    title: "Follow-up suggestions",
    group: "AUI connected",
  },
  {
    slug: "tool-fallback",
    title: "Tool fallback",
    group: "AUI connected",
  },
  {
    slug: "tool-group",
    title: "Tool group",
    group: "AUI connected",
  },
  {
    slug: "quote",
    title: "Quote",
    group: "AUI connected",
  },
  {
    slug: "sources",
    title: "Sources",
    group: "AUI connected",
  },
  {
    slug: "image",
    title: "Image",
    group: "AUI connected",
  },
  {
    slug: "file",
    title: "File",
    group: "AUI connected",
  },
  {
    slug: "model-selector",
    title: "Model selector",
    group: "AUI connected",
  },
  {
    slug: "composer-trigger-popover",
    title: "Composer trigger popover",
    group: "AUI connected",
  },
  {
    slug: "directive-text",
    title: "Directive text",
    group: "AUI connected",
  },
  {
    slug: "markdown-text",
    title: "Markdown text",
    group: "Renderers",
  },
  {
    slug: "syntax-highlighter",
    title: "Syntax highlighter",
    group: "Renderers",
  },
  {
    slug: "shiki-highlighter",
    title: "Shiki highlighter",
    group: "Renderers",
  },
  {
    slug: "mermaid-diagram",
    title: "Mermaid diagram",
    group: "Renderers",
  },
  {
    slug: "generative-ui",
    title: "Generative UI",
    group: "Renderers",
  },
  {
    slug: "tooltip-icon-button",
    title: "Tooltip icon button",
    group: "Primitives",
  },
  {
    slug: "logos",
    title: "Model logos",
    group: "Primitives",
  },
  {
    slug: "heat-graph",
    title: "Heat graph",
    group: "Primitives",
  },
  {
    slug: "generative-stays",
    title: "Riverside Loft",
    group: "Generative",
  },
  {
    slug: "generative-booking",
    title: "Table reservation",
    group: "Generative",
  },
  {
    slug: "generative-order-status",
    title: "Order tracking",
    group: "Generative",
  },
  {
    slug: "generative-flight-tracker",
    title: "Flight tracker",
    group: "Generative",
  },
  {
    slug: "generative-portfolio",
    title: "Portfolio overview",
    group: "Generative",
  },
  {
    slug: "generative-create-event",
    title: "Create event",
    group: "Generative",
  },
  {
    slug: "generative-view-event",
    title: "View event",
    group: "Generative",
  },
  {
    slug: "generative-weather-current",
    title: "Current weather",
    group: "Generative",
  },
  {
    slug: "generative-ride-status",
    title: "Ride status",
    group: "Generative",
  },
  {
    slug: "generative-draft-email",
    title: "Draft email",
    group: "Generative",
  },
  {
    slug: "generative-cart",
    title: "Shopping cart",
    group: "Generative",
  },
  {
    slug: "generative-playlist",
    title: "Playlist",
    group: "Generative",
  },
  {
    slug: "generative-channel-message",
    title: "Channel message",
    group: "Generative",
  },
  {
    slug: "generative-receipt",
    title: "Purchase receipt",
    group: "Generative",
  },
  {
    slug: "generative-chart-area",
    title: "Weekly active users",
    group: "Generative",
  },
  {
    slug: "generative-player-card",
    title: "Player card",
    group: "Generative",
  },
  {
    slug: "generative-event-session",
    title: "Conference session",
    group: "Generative",
  },
  {
    slug: "generative-notify-confirm",
    title: "Confirm dialog",
    group: "Generative",
  },
  {
    slug: "generative-chart-line",
    title: "Response time",
    group: "Generative",
  },
  {
    slug: "generative-create-task",
    title: "Create task",
    group: "Generative",
  },
  {
    slug: "generative-software-purchase",
    title: "Software purchase",
    group: "Generative",
  },
  {
    slug: "generative-chart-bars",
    title: "Support tickets",
    group: "Generative",
  },
] as const;

export type AssistantElementSlug = (typeof ASSISTANT_ELEMENT_CATALOG)[number]["slug"];

export type AssistantElementGroup = (typeof ASSISTANT_ELEMENT_CATALOG)[number]["group"];
export type AssistantElementSlugFor<Group extends AssistantElementGroup> = Extract<
  (typeof ASSISTANT_ELEMENT_CATALOG)[number],
  { group: Group }
>["slug"];
