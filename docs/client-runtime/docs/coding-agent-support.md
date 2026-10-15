# Agent tool support and setup

## Agreed scope

The named agent tools in scope are Codex, Claude Code, Cursor, Grok Build, OpenCode, Google Antigravity, and Pi. Generic Agent Client Protocol (ACP) connections remain supported alongside them. A named agent is a product integration even when its provider-facing transport is ACP.

Use **Agent tools** in the interface for installed agent applications such as Codex and Claude Code, and **Agent tool** when selecting one. **Harness** remains the internal runtime port and protocol terminology. Each agent has an independent enable/disable toggle in setup and Settings; choosing one does not disable the others.

Prefer a provider's supported native integration where it exposes the required agent behavior. ACP is appropriate when it is the provider's supported integration surface. Transport choice must preserve actual permissions, native history, lifecycle, and capability reporting; an SDK does not automatically support every operation.

## Current runtime and remaining work

This table describes implemented adapters and accepted directions separately. Registration and fixture coverage do not establish successful execution against an authenticated installation.

| Agent tool | Runtime ID | Current adapter | Remaining direction |
| --- | --- | --- | --- |
| Codex | `codex` | Native `codex app-server` JSON-RPC | Retain the native integration and existing-login probe. |
| Claude Code | `claude` | Official Claude Agent SDK through a supervised Node worker | Use the SDK for session execution and callbacks. Retain the separate native CLI authentication probe and native history ownership. |
| Cursor | `cursor` | Official Cursor SDK Bridge through authenticated local Connect services | Use the dedicated SDK adapter. CLI ACP availability and login do not establish SDK availability or authentication. |
| Grok Build | `grok` | ACP through `grok agent stdio`, with version-gated xAI extensions | Retain the reviewed extension set for version `1.0.52`. Qualify other installed versions before expanding extension capabilities. |
| OpenCode | `opencode` | Native local HTTP API and SSE, or a configured existing server | Retain the native server integration. |
| Google Antigravity | `antigravity` | Native `agy` stream-json | Managed installation verifies the official release manifest and SHA-512. Keep Google account sign-in native-owned; expose only verified authentication interfaces. |
| Pi | `pi` | Native `pi --mode rpc` | Retain the native RPC integration. |
| Generic ACP | Configured ID | ACP JSON-RPC with negotiated capabilities | Retain configurable executable/arguments and honest capability negotiation. |

All seven named IDs have primary setup placement and dedicated installation guidance. Additional ACP tools appear in an expanded section. Registration, installation, authentication, and enablement are separate states.

Authentication probes confirm status for Codex and Claude Code. Cursor checks the SDK account when a runtime API key is configured, without creating an agent or sending a prompt. Other adapters can report an installed executable with authentication **unknown**. Neither a successful version check nor an enabled toggle means the agent is signed in.

## First-run behavior

After connecting to a verified node, show setup if that node has no completed setup preference. The same controls are available in **Settings → Agent tools**, which also offers **Open setup**.

- Detect installed executables and run the supported read-only readiness probes.
- Reuse a confirmed existing login, connect with a supported API key, or start an available provider-native authorization flow. Unknown authentication remains visibly unverified.
- Let the user enable several installed agents or disable all of them. An unavailable agent cannot be enabled.
- Show provider selection where the tool supports multiple model providers, one connection per tool, explicit credential storage, authorization progress, input when required, cancellation, disconnection, and **Check again**.
- Offer explicit managed installation for the seven named tools on supported platforms. Install into runtime-owned release directories, including the SDK execution or sign-in support required by that tool. Show prerequisites and retain official instructions where managed installation is unavailable; discovery never downloads dependencies or starts installers.
- Allow skipping or completing setup. Store completion and enablement preferences under the retained node identity, without storing credentials.

Disabled agents are excluded from new-chat choices, pre-session model preview, native-history import choices, and comparison launches. New-session creation also checks enablement. Existing sessions keep their agent and can continue. Completing setup may choose an enabled default only for an untouched empty draft; it does not rewrite an existing conversation or an explicitly configured draft.

## Connection methods

| Agent tool | Managed connection methods |
| --- | --- |
| Codex | ChatGPT device authorization, OpenAI API key, or existing native credentials. |
| Claude Code | Anthropic Console authorization with native manual-code input when needed, Anthropic API key, or existing native credentials. |
| Cursor | Official SDK browser authorization that mints a runtime-owned API key, an explicit Cursor API key, or existing SDK credentials. CLI sign-in alone does not authenticate the bridge. |
| Grok Build | Native device authorization, xAI API key, or existing native credentials. |
| OpenCode | Native ChatGPT and GitHub Copilot device authorization; OpenAI, Anthropic, Google Gemini, xAI, or OpenRouter API key; existing provider credentials. Native authorization methods are discovered from the isolated local server. |
| Google Antigravity | Native Google browser authorization with manual code input on Linux and macOS; existing native Google credentials or a Gemini API key. Windows retains native external Google sign-in. |
| Pi | Official native SDK authorization for ChatGPT and GitHub Copilot, the five API providers listed for OpenCode, or existing provider credentials. |

Authorization methods requiring the authentication worker are advertised when its support assets and a Node.js executable are present, including support in a managed installation; starting authorization enforces Node.js 22.13 or later. API key checks use read-only provider account/catalog endpoints and never send an inference prompt. Installed tools whose native credentials cannot be independently verified can be explicitly configured with existing credentials, retaining an unverified status; known signed-out tools are rejected. A configured connection is not proof of successful model execution.

Antigravity's managed Gemini API connection uses an isolated child `HOME` and, on Windows, `USERPROFILE`, with nonsecret provider settings in that private home. The scope also applies to native tools and history inherited by that process. Existing native Google sign-in retains its ordinary provider-owned scope. No global Antigravity settings or credential files are rewritten by the API connection.

Antigravity Google authorization runs its native login terminal in the documented SSH mode, selects only an observed default Google OAuth choice, and returns the native URL and code prompt to the client. The runtime owns the terminal process and cancellation; it never submits a model task. Native keyring storage can be shared with external Antigravity sessions, so a private home is not a separate Google credential vault. Disconnect preserves the native credential store. Auth-only startup and deterministic completion/cancellation fixtures have been validated; real Google authorization completion requires the user.

## Claude SDK integration

Claude session execution uses the official `@anthropic-ai/claude-agent-sdk` package, pinned to `0.3.299`, in a runtime-owned Node worker. Rust retains the `Harness` and `AgentBackend` ports and normalizes public SDK messages into ordered runtime events. The worker owns the SDK query, streaming input, permission callbacks, hooks, and disposal. There is no direct CLI-control fallback for session execution.

The worker uses the native Claude Code system-prompt preset and preserves user/project configuration for ordinary sessions. It supports native resume/fork, model and permission changes, explicit questions and plan approval, interruption, active steering, compact, and the existing bounded rollback behavior. Tool hooks are exposed as diagnostic events. Background tasks are exposed through a bounded cache of tasks observed in the current SDK query. The public SDK provides `stopTask`, but no task-list operation; Swarmie does not invent unobserved tasks. Task identifiers belong to that session, and a stop acknowledgment remains pending until a native task notification confirms its outcome. The desktop task popover works while the parent turn is running or awaiting permission without cancelling that turn.

CLI detection/login and native history remain separate runtime concerns. The SDK worker uses the selected installed Claude CLI; its credentials remain native-owned. The worker, SDK dependencies, and Node.js 22 or later must be present on the execution node, including remote nodes. Configuration can override `sdkWorkerPath` and `sdkNodeExecutable`; environment overrides are `SWARMIE_CLAUDE_SDK_WORKER` and `SWARMIE_NODE_EXECUTABLE`. `config.executable` selects the Claude CLI, and `config.args` prepends CLI wrapper arguments through the SDK's supported process-spawn hook rather than replacing the SDK launch protocol.

SDK versions and capability exposure must be validated together. Existing fixture contracts execute through the actual SDK with a deterministic CLI peer; this does not establish successful authenticated model execution.

## Cursor native integration

The dedicated Cursor adapter uses the official [SDK Bridge](https://cursor.com/docs/sdk/bridge), pinned to `1.0.37`, behind the existing `Harness` port. It embeds the Cursor SDK and exposes authenticated `sdk.v1` Connect services on loopback HTTP. The bridge is a provider runtime dependency, not an inference proxy.

The adapter owns:

- Versioned bridge discovery, supervised launch, readiness handshake, authenticated loopback transport, and process cleanup.
- Native agent/run identifiers, resume, streaming, cancellation, terminal failures, and model discovery mapped to existing runtime contracts.
- Explicit capability declarations for permissions, structured questions, and other operations. Unsupported SDK behavior must remain unavailable rather than silently approving work.
- Credential resolution in the runtime. Keep provider credentials distinct from the bridge's local bearer token and from browser preferences. The published bridge documentation requires an API key for agent/catalog calls; reusing a CLI login must be verified rather than assumed.
- Contract fixtures and bridge startup validation. Preserve existing user-owned native history; SDK agent identifiers are distinct from CLI ACP conversation identifiers.

Cursor supports native resume, model/mode selection, streaming, usage, cancellation, and local SDK history discovery, read, and import. History uses the pinned bridge's public `ListAgents`, `GetAgent`, and `ListAgentMessages` services against existing SDK stores. These reads require no provider API key or account/catalog lookup and do not create, resume, or prompt an agent. Swarmie preserves store locations and native contents; the provider bridge may perform its own SQLite bookkeeping. A configuration-directory override identifies an existing SDK store containing `index.db`.

The imported tree retains native checkpoint turns and raw metadata. Unreported outcomes remain unknown, and imported messages do not claim usable fork anchors. Manual approvals, fork, and Cursor CLI history import remain unavailable in this SDK path. SDK agent identifiers and stores are distinct from CLI ACP conversation identifiers; replacing ACP does not convert them.

Strict tool disabling additionally requires the pinned distribution's `manifest.json` to confirm `sdk.v1` and SDK version `1.0.37`. A manually selected older bridge cannot advertise this feature merely because it reports the same protocol version. Unsupported tool filtering is rejected before agent creation.

Cursor can use a configured API key or the official SDK browser authorization flow. Runtime setup supplies `CURSOR_API_KEY` privately at launch; bridge bearer authentication is a separate per-process credential. `SWARMIE_CURSOR_EXECUTABLE`, `CURSOR_SDK_BRIDGE_BIN`, or `config.executable` can select the bridge. Installing Cursor Agent CLI alone does not verify SDK availability.

## Grok ACP extensions

Grok retains its supported ACP transport. Swarmie enables the reviewed xAI extension set only when initialization advertises `grokShell: true` and agent version `1.0.52`; other versions keep portable ACP behavior with an explicit unavailable or unknown capability reason. The reviewed public source revision is `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`.

The extension mapping supports active steering through `_x.ai/interject`, conversation or conversation-and-files rollback through native rewind points and execution, and session-owned background-task listing and stopping through `_x.ai/task/list` and `_x.ai/task/kill`. Native task, subagent, and usage notifications become typed runtime events, including reported cost, cache, and context fields. Duplicate or partial usage updates must not double-count costs. Rewind preserves native conflict errors and validates the returned target and mode before accepting success.

Native history import, fork, explicit compact, and arbitrary subagent control remain unavailable. Swarmie does not forward arbitrary vendor methods or infer a task-to-terminal identifier mapping.

## Task control and history routing

`session/tasks` returns typed task summaries for an existing session; `session/task/stop` targets a native task identifier owned by that session. Both require the advertised `backgroundTasks` capability and the usual runtime/session identity checks. They remain available during parent prompt acceptance and permission waits. `task.updated` carries native-confirmed state; a stop receipt does not cancel the parent turn or fabricate a terminal task outcome.

Native history resolves managed executable/support paths and the selected profile's configuration home through a credential-free overlay. An explicit history directory takes precedence. Read-only history remains accessible when the credential vault is unavailable or a connection is disconnected, without silently reconnecting it for execution. The desktop Import section exposes Cursor's SDK source explicitly and discards delayed results when the provider query or runtime node changes.

## Runtime support assets

SDK dependencies belong to the execution node. A desktop dependency or an installed frontend does not satisfy the headless runtime's requirements. Stage the pinned support assets explicitly, outside the checkout:

```sh
python3 scripts/distribution/stage-agent-tools.py --output <support-directory> --tools all
python3 scripts/distribution/package-runtime.py --output <package-directory> --agent-tools <support-directory>
```

Desktop and local client package commands also accept `--agent-tools`. Claude support contains the worker and locked production SDK dependencies; Cursor support contains its versioned standalone bridge. Authentication support contains a separate worker with pinned Cursor and Pi SDK dependencies, resolved from a managed tool installation, `agent-tools/auth`, or `SWARMIE_AGENT_AUTH_WORKER`. Managed installation includes the appropriate locked support assets and does not require a source checkout. Node.js 22 or later and an installed Claude CLI remain prerequisites for Claude execution; SDK authentication requires Node.js 22.13 or later. Staging and packaging are explicit operations; discovery does not download dependencies or initiate login.

Installed bundles expose an `agent-tools` directory through the runtime's package layout. `SWARMIE_AGENT_TOOLS_DIR` can select a configured support directory. The npm runtime launcher forwards the packaged support directory and its Node executable. The Windows standalone installer supplies an owned launcher-location hint because its launcher is a copied executable. Installation and update validate the support inventory and preserve unrelated user files.

## Managed installation and native authorization

The runtime installs managed copies under its own data directory. Installing a managed copy does not replace the user's global CLI or alter package-manager configuration. Each installation publishes an immutable release and switches its active receipt only after verification; cancellation or failure preserves the previous release. Managed executable and support paths apply to readiness, native authorization, credential validation, and session execution without changing the runtime process's global environment.

Codex, Claude Code, OpenCode, and Pi use pinned official npm packages. Claude installs its Agent SDK worker alongside the CLI; Pi and Cursor install their native SDK authentication worker. Cursor installs the pinned official standalone SDK Bridge with SHA-256 verification. Grok uses its official release artifacts and a native version check; its publisher currently provides no checksum, so its verification is explicitly limited to the official HTTPS source and version response. Antigravity retains its official manifest and SHA-512 verification.

Managed npm and SDK installations currently support Linux and macOS on x64 and arm64 and require Node.js 22.19 or later and npm. Grok and Antigravity additionally support their published Windows artifacts. Unsupported platforms retain the native installation guide. An installed Cursor or Pi without browser sign-in assets offers **Install sign-in support**, which installs a managed copy with its support assets before authorization.

OpenCode authorization uses only the local server's health, provider authorization, and OAuth callback endpoints, with private configuration/data homes and random loopback HTTP authentication. ChatGPT device authorization avoids a fixed browser callback port. Device and manual-code receipts expose only validated authorization URLs, codes, and sanitized progress; native OpenCode owns credential writes. No chat or model prompt is created during setup.

Claude's managed browser authorization is for Anthropic Console. Its official Agent SDK documentation requires Anthropic approval before third-party applications offer claude.ai subscription login or limits. Reusing independently configured native credentials does not authorize Swarmie to advertise managed subscription OAuth.

## Managed setup and future observability

The runtime owns provider connection profiles, native authorization processes, verified installation, and credentials. Browser and native desktop clients use transient `runtime/agentToolsSetup`, `runtime/agentToolsConnect`, `runtime/agentToolsFlow`, `runtime/agentToolsInput`, `runtime/agentToolsCancel`, `runtime/agentToolsDisconnect`, and `runtime/agentToolsInstall` requests under the captured runtime identity. Credential values must never enter client snapshots, persisted request journals, chat events, or public setup receipts.

Managed API keys use a system credential vault namespace scoped to the retained runtime owner and agent tool, separate from the optional Swarmie account. When secure storage is unavailable, the user must explicitly choose session-only storage; there is no plaintext key-file fallback. Connection metadata is nonsecret. Native authorization uses provider-owned credential storage and is labeled accordingly. Disconnect removes Swarmie's connection and owned keys while preserving external credentials and native history; new native launches require an explicit reconnection, while live sessions keep running.

Private execution overlays supply credentials and managed launch configuration for spawn, resume, fork, settings preview, generation, and quota queries. They are not serializable and are not written into public session options. Conflicting explicit credentials or connection configuration fail with a generic error that does not disclose either value.

Antigravity installation uses a platform-specific official manifest, an allowlisted HTTPS download location, bounded transfer and extraction, SHA-512 verification, and atomic activation of a runtime-owned release directory. Cancellation and failure preserve the previously active executable. Installation completion triggers a readiness check and does not imply authentication or hardware compatibility. Native browser sign-in is offered only where the native authorization interface has been verified, including Antigravity's supervised terminal flow.

The first version exposes no extra-account management or rotation controls. Keep agent enablement, installation state, authentication state, credential ownership, and session execution separate so later account profiles can be added without redefining an enabled agent as an account.

Managed installation and authentication belong to the runtime on the execution node. The client displays progress and opens a provider authorization URL supplied by that runtime. Installer validation, cancellation, errors, and readiness rechecks must have explicit states before this flow is advertised as supported.

Native protocol diagnostics and ordered runtime events provide a foundation for session debugging. They are distinct from model HTTP request metrics or request replay. A future local inference proxy requires per-provider endpoint support and verified credential/routing behavior; neither ACP nor an SDK connection guarantees that model requests can be redirected. Request replay and session replay also need separate contracts. These capabilities are not implemented by the setup screen.
