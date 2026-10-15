# MCP management

MCP definitions belong to an execution node and its operating-system owner. The desktop **Settings → MCP servers** view manages the selected node. It combines Swarmie definitions with native definitions without importing, copying, or taking ownership of native configuration files. Configuration discovery does not connect a server. A selected open native session supplies live inventory when its adapter supports inspection.

## Definitions and origins

A Swarmie definition registers a STDIO command or a Streamable HTTP endpoint. User scope applies to future sessions on that node; project scope applies within the selected directory. A definition can target one harness or all compatible harnesses. Global definitions are injected only into harnesses advertising that transport. Harness-specific definitions must be supported before a session starts. Installation registers configuration; it does not download packages, execute a package installer, or verify that the command is installed. Uninstallation removes the selected definition and preserves executables and user-owned files.

The registry is persisted by the local session store under the reserved extension-state key `swarmie.mcp`. It does not synchronize through an account. Each shared definition receives an opaque identity and a reserved native name so that a display name cannot overwrite a native definition. The generated connection settings are compiled on every new native binding, including resume and fork.

Native definitions retain their harness, file, scope and opaque identity. Editing or removing a definition requires its observed revision. Revisions cover the source file and any separate enablement policy. A stale request is rejected before the change. TOML and JSONC edits preserve unrelated keys and comments, symlink aliases, and existing Unix file ownership and permissions. Native file writes are enabled on Unix platforms and verified on Linux; other platforms retain read-only native discovery and shared registry management. Cooperative file locks, repeated byte comparisons and atomic replacement reduce lost writes; external editors that do not take the lock can still race the final comparison and rename.

| Harness | Native sources | Enablement |
| --- | --- | --- |
| Codex | Configuration-home `config.toml`; project `.codex/config.toml` from the current directory to the repository root | Server `enabled` |
| Claude Code | Owner/configuration-home `.claude.json` or existing legacy `.config.json`; project `.mcp.json`; repository-local entries in the selected global file | User/local definitions use repository `disabledMcpServers` (validated linked worktrees share the main checkout policy); project definitions use `disabledMcpjsonServers` in the native canonical `.claude/settings.local.json`. Repository sharing requires verified Unix ownership and excludes the execution owner’s home; other contexts retain the session directory. Existing directory-local denial remains visible and must be cleared in native settings before enabling through the canonical source. Select a directory to toggle them. Native project approval remains independent. |
| OpenCode | Configuration-home `opencode.json` and `opencode.jsonc`; project counterparts | Server `enabled` |
| Pi | Agent-directory `mcp.json`; current-directory `.pi/mcp.json` | Server `enabled`; native project trust remains required |

Configuration-home overrides, native configuration path environment overrides (including `OPENCODE_CONFIG`), and the live execution owner's home are respected. Project source discovery does not grant native project trust. Managed or read-only native sources expose only the actions their adapter can qualify. Servers observed only in a live session, including native plugin servers, appear with their tools and authentication state but cannot be edited or uninstalled when this adapter cannot qualify their configuration management. Plugin package installation/removal is outside the server-definition lifecycle; the UI does not pretend that deleting a server is equivalent to uninstalling its owning package.

## Credentials

Shared definitions store environment-variable names for STDIO environment variables, HTTP headers and bearer tokens. Values are resolved from live execution overrides or the node environment when the native binding starts. Missing variables reject startup; tools-disabled sessions do not resolve them or inject managed MCPs. Snapshots, registry results and status inventory omit credential values.

Native literal environment/header values and URL credentials remain local and are hidden in the catalog. They survive ordinary edits. Native process arguments are hidden and preserved because they may contain credentials; edit them in the native source file. Native transport changes require removal and recreation. URL credentials must be removed in the native source before changing its endpoint through the UI. Shared command arguments and endpoints are owner-supplied configuration: use environment references for credentials.

## Saved and live state

Enabled/disabled is desired configuration, not proof of a running connection. The catalog reports connection state, authentication state and available tool names separately. Without qualified native evidence their values remain unknown. Configuration failures are displayed as limitations, not an empty successful native catalog.

A saved change marks affected resumable live bindings as requiring reopening. A turn already in progress may finish; subsequent prompt dispatch, steering and queued-message promotion are rejected or held until reopening. Non-resumable bindings, including ephemeral sessions, remain usable with their existing configuration; saved changes apply to new native bindings. **Reopen session** requires an idle, resumable native session. It closes the old binding, preserves its live launch credentials for the replacement, recompiles current definitions and keeps the message queue paused. Reconnection errors remain visible; the operation does not claim that a saved setting reached the native server.

Codex inspection uses native MCP status pagination, tools/auth detail and the selected native thread. Codex OAuth login returns an authorization URL; complete sign-in in the browser and refresh. Claude uses its native MCP status control. External OpenCode servers own their configuration: their sessions ignore the node registry, retain live read-only inventory, and cannot edit node-local files as though they were that server's native configuration. OpenCode supplies connection state from the selected native server; its status endpoint does not provide a tool catalog through this adapter. Authentication actions are offered only when a native control has been qualified, rather than inferred from an HTTP URL.

Pi uses its native session-only MCP registration API, qualified against versions 1.0.0 and 1.2.0. A temporary extension and manifest are retained for the lifetime of the native binding and removed during normal teardown. Credentials travel through dedicated environment variables so that Pi's command/environment expansion cannot reinterpret literal values. Conflicting native names that would override an injected definition are rejected before startup. Pi owns its MCP transports, tool registration and connection cleanup. The extension requires native registration acknowledgement and reports observed tool names; a tool listing is labelled as tools available rather than fabricated connection/authentication evidence. MCP calls use the native tool pipeline and surfaced permission questions. Disabled tools prevent the bridge from loading or connecting. Earlier Pi versions without the registration API cannot accept managed MCP definitions through this adapter.

## Protocol

| Method | Parameters | Result |
| --- | --- | --- |
| `mcp/list` | Optional `harness`, `cwd`, `sessionId`, `configHome` | `McpCatalog`: entries, limitations and affected open session IDs |
| `mcp/mutate` | `query`, `action`, optional `id`, `expectedRevision`, `origin`, `name`, `scope`, `definition` | `McpReceipt`: resulting entry, reopening requirements and optional authorization URL |
| `mcp/reopen` | `sessionId` | Reopened session snapshot |

Actions are `install`, `update`, `enable`, `disable`, `uninstall` and qualified `authenticate`. Existing entries require both identity and observed revision. The returned action list is authoritative for each entry. A query with `sessionId` binds discovery to its retained harness, working directory and configuration home; conflicting explicit harness/directory values are rejected. Operations travel through the authenticated node gateway and its runtime-identity guard. MCP management is not exposed through model-callable extension-tool channels.

## Verification

The core tests cover registry scope, node isolation, credential omission, fresh compilation, restart gating and tools-disabled behavior. Native configuration tests cover the four file adapters, independent enablement policies, stale revisions, duplicate JSON keys, comments, symlink aliases, permissions, credential preservation and HTTP reference mapping. The desktop tests cover exact-origin actions, install/remove, stale-edit errors and node switching. The optional installed-CLI acceptance tests use an independent local MCP peer to prove native Codex/Claude registration and disabling, Pi tool discovery, literal credential delivery, and absence of Pi connection side effects under disabled tools. They do not require model inference or user accounts.
