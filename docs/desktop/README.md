# Desktop workspace

Native SDK hosts the system WebView. The frontend uses React, assistant-ui for conversations and streaming Markdown, and shared HeroUI primitives and Tailwind layout utilities for general controls. The managed client gateway serves the frontend, while an independent daemon owns execution and the shared JSON-RPC API. Browser development mode can still serve frontend assets directly from the daemon.

The managed desktop launcher reuses the current Environment's verified runtime or starts a detached one. Closing the window or a client failure leaves sessions running. Reopening attaches to the same owner. Inspect it with `swarmie runtime status`; stop it explicitly with `swarmie runtime stop` (use the same `--state-dir` when configured). Changing frontend assets does not require restarting the execution owner. The default managed launcher attaches through a persistent runtime WebSocket and opens a separate client gateway; authenticated service attachment requires separate configuration.

Transient connection failures retain session state and drafts while retrying with bounded backoff. Commands target the verified runtime identity. A different retained Environment blocks reconnection and keeps saved work. Another client can observe and continue the same live session through the common protocol; unsent drafts remain local to the client. Runtime persistence across window closure does not provide automatic crash recovery, boot startup, or access while the node is offline.

## Native client boundary

The managed WebView invokes `swarmie.client` through the SDK IPC bridge. The native host forwards through a private loopback endpoint to the Rust client gateway. The native client owns node connections, heartbeat, bounded reconnection, request correlation and private credentials; the frontend consumes client observations and renders runtime projections. Built-in privileged SDK commands remain disabled, and embedded content cannot invoke the application bridge.

The gateway and window close together, while the runtime remains available. Desktop assets belong to the client. The managed launcher can attach to a headless runtime already installed on the same node. Drafts, attachment data and cached projections persist under the configured root's `client` SQLite store, keyed by full retained node identity. IPC chunks large payloads without lifting the SDK's per-message limit. The frontend waits for native intent persistence before dispatching a prompt.

After building the frontend and native host, exercise this path from the workspace root:

```sh
./target/debug/swarmie desktop --assets apps/desktop/dist --host apps/desktop/zig-out/bin/swarmie-desktop
```

Direct development windows without managed IPC configuration use the browser HTTP adapter. See [runtime distribution and connectivity](../runtime/runtime-connectivity.md) for implemented boundaries and subsequent transport work.

## Run

Build the frontend first:

```sh
bun install --frozen-lockfile
bun run --cwd apps/desktop build:web
```

From the workspace root, start the daemon:

```sh
cargo build -p app
./target/debug/swarmie serve --http 127.0.0.1:7447 \
  --desktop-assets apps/desktop/dist \
  --workbench \
  --resources-root "$HOME/.local/state/swarmie/resources"
```

In another terminal, open the native window:

```sh
bun run --cwd apps/desktop dev -- --yes
```

Node.js 24+ installs the pinned development CLI. The CLI downloads Zig 0.16.0 when needed. Linux builds require GTK4 and WebKitGTK development libraries (`libgtk-4-dev` and `libwebkitgtk-6.0-dev` on Ubuntu). macOS builds use WKWebView and require Apple's command-line tools. Run macOS operations on the configured macOS host. Release binaries run without Node.js; the managed client gateway needs built frontend assets, while the execution daemon does not.

The default desktop URL is `http://127.0.0.1:7447`. To use another local listener, set `SWARMIE_DESKTOP_URL` before launching the native binary. The URL must use HTTP and an explicit loopback IP. A browser at the daemon's local URL provides a development preview of the local desktop interface. For a browser development preview, run `mise exec -- just --command bun run --cwd apps/desktop dev:web` from the repository root. It proxies `/rpc` to the default daemon; build the assets before starting that daemon.

## Workflow

### Agent tool setup

The first verified connection opens **Set up your agent tools** when setup has not been completed for that node. All seven named tools have primary placement and independent enable/disable toggles; additional ACP tools are expandable. **Settings → Harnesses** exposes the same controls and **Open setup** reopens the dialog. Installation and authentication are separate from enablement: an executable can be installed while its login status is unknown. The inline connection form supports existing logins, provider API keys, available browser/device authorization flows, progress, cancellation, disconnection, and **Check again**. The seven named tools offer explicit managed installation on supported platforms, including required SDK execution and sign-in support. OpenCode exposes native ChatGPT and Copilot authorization; Antigravity exposes native Google URL/code sign-in on Linux and macOS. Managed npm installations require Node.js 22.19 or later and npm on the execution node. The dialog can be skipped.

Disabled agents are excluded from new work and checked again before session creation. Existing chats keep their agent tool and remain usable. Completion and disabled-agent preferences are scoped to retained node identity and contain no provider credentials. Connection values travel through transient RPC to the selected runtime; API keys use its system credential vault or explicitly selected session-only memory. Native authorization retains provider-owned credential storage. Public setup receipts contain no credentials. Extra accounts, rotation, inference request metrics, and replay remain outside this screen. See [agent tool support and setup](../runtime/coding-agent-support.md) for supported methods and remaining work.

### Projects and conversations

Use **New chat** in the sidebar, the command palette, or the new-conversation shortcut (default **Mod+N**) to start without a project. The default working directory is the daemon host's native Documents folder, resolved with `dirs` 7's `document_dir`, with the home directory's `Documents` child as fallback. In **Settings → General**, enter an existing absolute folder path and choose **Save directory**, or choose **Use Documents** to reset. The daemon validates and canonicalizes custom paths. The choice affects future chats; existing chats retain their working directory. Hover or focus an existing project row to reveal its plus action, which creates a chat scoped to that project.

To add a project, open the command palette (default **Mod+K**) and choose **Open project**, or hover/focus the **Projects** section and choose **Add new project**. This opens the centered **Create project** dialog. Enter a project name, click **Add**, enter an absolute folder path, and click **Add folder** to commit that source. Click **Create project** to open it. The current flow supports one source folder and manual path entry; a native directory picker and multi-root projects are unavailable. The official Elements palette groups **Actions** and **Conversations**, filters labels by case-insensitive substring, wraps arrow-key navigation, scrolls the active row into view, and runs the active item on Enter. Escape dismisses the shared HeroUI Modal and restores focus. Custom project names survive reopening and project metadata refresh.

The source menu also offers **Create a new folder** and **Clone a repository** when the Workspace extension is available. Enter the parent folder and project name; clone mode additionally requires a repository URL. Creation can initialize a Git repository. **Cancel** or Escape dismisses an idle dialog without opening or creating anything; failed submission keeps the dialog open with an error. Ordinary folders are supported. Existing-project context and project-management actions open the right panel, where worktree creation, cleanup, scratch workspaces, and publishing remain available. Git worktree operations require a repository with at least one commit. Create an optional worktree for isolation. Choose an installed harness from the shared HeroUI Dropdown at the left of the raised charcoal composer cap. Its tertiary trigger contains the current-state dot instead of a harness icon. Hover or focus the whole trigger for the current state and color legend; click it to open the selector. Idle is green, Starting/Running violet, Needs input/Cancelling amber, Failed red, and the legend combines Closed / Disconnected in gray. Menu rows retain harness icons, a checked current selection, and Escape focus restoration; other harness forms use shared HeroUI Select. The shared **View node** selector sits at the right end of the raised composer cap, shows the displayed node, and changes its state instance and new-work catalog. Existing chats remain on their original node. The lower-right model control displays the selected or actual native model label and opens the harness’s advertised model choices. Before the first prompt, the client calls `runtime/configuration`: the runtime opens a temporary native connection only for harnesses advertising `ephemeral: true`, reads configuration/models without sending a prompt or saving a chat, and closes the connection. The picker does not start a session to load models. It previews only while idle, keeps cached choices disabled while busy, and refreshes when the session returns to idle. Busy-race preview text is suppressed; other preview errors remain visible. The prompted session normally starts on first send; Codex’s current default is visible beforehand, and the effective default is passed when the session starts. Model catalogs are never hardcoded.

The sidebar groups chats by project, time or execution state through its workspace menu and supports chat pins, ordering, snooze, archive/restore, renaming, message search and configurable keyboard shortcuts. In project grouping, **Projects**, **Recents**, and **Pinned** headings collapse their lists. Expanded chevrons and heading controls reveal on hover/focus; collapsed chevrons stay visible. **Pinned** appears only when it contains visible chats. Pinned chats are excluded from Recents and project lists.

Each section's options menu offers **Recent activity** and **Manual order** independently. Preferences persist in `swarmie.sidebar-sorting`; Projects defaults to manual, Recents to recent activity, and Pinned to manual. Project recent activity uses its latest conversation activity; chats sort by updated time with created time as fallback. Project chat lists follow the Recents preference. Dragging a chat changes the applicable chat preference to manual and retains order undo.

Hover or focus a chat row for **Pin chat** and **Chat options**. Pinned rows keep **Unpin chat** visible at rest; their ellipsis still reveals on hover/focus. Options provide **Rename chat**, **Pin/Unpin chat**, and **Archive/Restore chat**. Archive/restore is disabled while starting, running, waiting for input, or cancelling; successful changes preserve undo. Row selection and action controls are sibling buttons. Each conversation keeps an independent draft. Empty drafts are persisted but stay out of the sidebar and command palette until the first accepted message. Restored conversations containing user messages are visible. Files can be attached, dropped or pasted into the rounded composer. Supported images use native image input, UTF-8 files become explicit file context, and PDF, audio, video and other binary files are retained as private assets and passed as actual on-disk paths. Native file tools determine which formats the harness can use. Oversized attachments are rejected before submission.

The composer places permissions directly beside the attachment **+** on the left. The shield trigger shows the effective **Ask for approval**, **Approve for me**, or **Full access** preset; nonmatching native settings show **Custom permissions**, and missing configuration shows **Permissions**. **Full access** uses orange text. The **Permissions** popover contains only these three radio choices. Each choice applies a batch of actual native configuration values and is disabled unless every value is advertised by the harness. Unsupported choices explain **Not supported by this harness**; non-live settings in an existing session explain **Set before starting a session**. Explicit pre-session overrides appear in the label and selected mode.

For Codex, **Ask for approval** uses `approvalPolicy: on-request` and `sandbox: workspace-write`, plus `approvalsReviewer: user` when that descriptor exists. **Approve for me** uses the same approval/sandbox settings with the actual advertised `approvalsReviewer: auto_review`; it is disabled without that support. **Full access** uses `approvalPolicy: never`, `sandbox: danger-full-access`, and reviewer `user` when present. For permission-mode harnesses, ask/full map to native `default`/`bypassPermissions`; automatic review is unavailable. Configuration changes preserve idle, capability, and per-field live constraints.

The lower-right controls group the context wheel, model and reasoning effort, microphone dictation, and send or stop. Hover or focus the context wheel for reported percentage and used/capacity tokens. Missing capacity remains explicit; absent usage shows **Usage not reported yet** and **Waiting for harness usage data**. There is no bottom composer caption. The effort popover centers a shared secondary model button displaying current effort and model, with a shared Slider below. The left lightning action toggles only the advertised native **Fast mode** boolean, never reasoning effort; unsupported Fast mode is disabled with an explanation. The right reset action restores the effort captured when this menu opened. The center button opens a separate shared Select model catalog, which closes on selection and reopens if configuration fails. Harnesses without effort show the model picker beside the Fast mode toggle. Dictation appends final speech-recognition results to the current draft when the browser supports speech recognition; unsupported browsers show a disabled microphone with an explanation. Denied microphone access and recognition failures surface errors.

Sending, stopping, streaming Markdown, tool activity, reasoning, plans, subagents and questions use the runtime's actual sessions. While an agent is working, **Queue** adds an editable follow-up; **Steer** appears only for harnesses that advertise steering. Queues can be paused, edited and removed. A restored queue remains paused until explicitly resumed. Composer controls load native model/configuration choices and display context and usage only when reported. Reasoning effort uses the advertised `effort`, `reasoningEffort`, or `thinkingLevel` identifier and a discrete shared Slider. Its label follows the advertised choice during dragging; the configuration commits on release. Configuration changes require idle state, and live settings follow the harness’s actual configuration support.

Outgoing user messages appear in a right-aligned bubble with a separate action row underneath. The time/copy row is hidden by default and appears when hovering the message group or focusing a control within it; its reserved 28px height prevents layout shift. When available, **HH:mm** comes from the original stored event timestamp, with full local date/time on hover; missing or invalid timestamps omit the time. When connected to an existing saved session, a message without a valid date requests read-only `session/transcript` backfill. Concurrent reads are deduplicated, existing dates and content remain intact, thread update time is preserved, and results from a changed session are discarded. This timestamp read does not resume or launch a session or run inference. **Copy** changes to **Copied** after success.

Assistant response actions use the official Elements **Copy** control inside assistant-ui's action bar. Copy includes only the selected response's text and uses the existing clipboard/document-copy fallback. A green check confirms success for three seconds; repeated copies reset the timer, unmounting clears it, and failures surface as workspace errors. **Fork from here** is an icon-only Lucide GitFork control with a named tooltip, appearing alongside copy only for an idle session with advertised fork support and a native message anchor. The assistant action row is hidden by default, revealing copy and supported fork on hover over the whole assistant message or keyboard focus within. Its reserved 28px height avoids layout shift and its 150ms opacity transition honors reduced motion; it stays hidden while running. Both copy controls use **Copy**/**Copied** for their accessible names and hover/focus tooltips, muted color with foreground hover, and a green success check. User and assistant copies share `useMessageCopy`, including fallback copying, three-second confirmation, timer cleanup, and protection against late updates after unmount.

Saved conversations replay the daemon's durable event journal and resume the same native provider reference after restart. Unsupported resume fails clearly and keeps the old conversation. A new chat is the explicit way to start fresh. Project navigation and draft text use lightweight local storage. Attachment drafts, image history and queue cache use IndexedDB. Draft writes finish asynchronously; storage failures remain visible and retain unsent content in memory. Newer local edits win over delayed cache hydration. Tokens and question answers are not persisted.

Client Projects, chats, selection, drafts, attachments, transcript projections, event cursors, and pending operations belong to a state instance pinned to the full retained node identity. Lightweight storage uses version 3 node keys; IndexedDB uses separate node keys with checked identity envelopes. Route associations restore the correct node cache while offline. Equal paths and session IDs across nodes remain separate, and owner restarts retain the cache namespace. Responses or transport failures from an earlier attachment are discarded without redirecting work or declaring that the original command had no effects. Notification tags include the node identity. Previous unscoped development entries remain untouched and are not imported. The composer cap's View node selector reuses these state instances, and the local gateway forwards commands to their verified destinations. Connected hidden nodes continue to be polled.

**Settings → Nodes** contains **Control this computer** and **Control other devices** tabs. An **SSH** tab appears only when a remote node already has a saved SSH route. This computer connects automatically, with no local Connect or Disconnect control. Its observation continues after first-startup transport failure and while a remote node is viewed; bounded retries reload the catalog on recovery. Authentication and identity mismatch remain visible blockers.

Remote rows have a connection switch and an actions menu with **Details** and **Open node**. Details shows last verification, saved route addresses, revision-guarded removal, and that node's Harness readiness. **Open node** changes the viewed catalog; Settings stays open through selection and active-thread initialization. **Add** under Control other devices opens a focused prepared-endpoint dialog for a connection name, HTTP(S) or WS(S) address, and optional access token. Failed verification retains inputs; canceling clears the token; busy verification blocks duplicate submission and dismissal. For SSH, use an active authenticated tunnel; discovery, installation, and tunnel setup are pending. Turning off a remote connection pauses only this client's observation. Removing a route retains node identity, cached drafts, remote execution, history, and files. Multiple routes share one node; the gateway currently chooses the first connection name in sorted order. Closing the client never stops an owner.

Remote commands, files, assets, terminals, and previews go through same-origin gateway routes. Browser requests use the local connection token held in memory; remote credentials are stored privately under the configured local data root and read by the gateway. Tokens and credential references are excluded from public node catalogs. Workbench auxiliary assets and generated scratch resources use each runtime's configured root under `desktop`; linked user folders keep their independent lifecycle.

**Workspace tools** discovers extension-contributed views. File and tool tabs, panel widths and unsaved file drafts are saved per conversation. Switching tool tabs preserves their active views; closing a tab releases its owned processes. The Git view supports working/staged/unstaged/base/turn review, literal-path staging, commits, ordinary push, draft pull requests, comments, reviews and host viewed-file synchronization. File restoration uses explicit native rewind scope or a protected Git checkpoint action. The workspace extension provides files, editable UTF-8 content, a real PTY terminal and discovered project/test actions. Ordinary non-Git folders work with the workspace extension. Project actions close their process after completion or cancellation. Interactive external web views use a sandbox and can invoke only their extension's advertised commands. Reloading a view reads fresh output without restarting the app; loading a new native extension binary still requires rebuilding the daemon.

The optional resources extension provides a private credential vault, project defaults, prompt templates with variables and aliases, observed prompt history, schedules, peer tools, independent model comparisons and observed usage totals. Project defaults are keyed by the original project path; project native configuration is merged before explicit thread overrides. A saved workspace must still be present in the project's known worktrees. Template aliases work in the slash picker and parameterized templates collect values inline. Saved comparisons can be reopened or removed while their individual conversations remain available. These workflows use ordinary runtime sessions and stay outside the conversational core.

Harnesses use their own credentials and provider configuration. **Settings → Nodes → Details** lists that node's installed harnesses with icons and provides version/authentication readiness checks. Harness selectors across the composer and forms include only installed supported choices and show **No harness installed** when empty. Unknown authentication state is shown honestly; executable discovery alone does not prove provider access. Install additional test harnesses under `tests/fixtures/harnesses`.

Managed native clients attach through the private same-user local socket, independently of HTTP bearer authentication. For a token-protected local daemon accessed through the browser development gateway, use **Settings → Nodes → Control this computer → Details → Connection access** to enter and verify its bearer token. If authentication prevents the catalog from loading, the local tab shows the same Connection access action directly. The token stays in memory. Browser RPC is disabled by default; `--desktop-assets` explicitly enables the daemon's own origin and Host on a loopback listener. An explicit authenticated HTTPS public origin supports remote browsers and mobile layouts. Other origins remain rejected. See [distribution and remote connections](../runtime/distribution.md) for service management, SSH reconnection, installers and verified update staging.

Optional desktop notifications are disabled by default. **Settings → Notifications** can enable completion, input and error alerts, with background-only delivery, when the WebView exposes the browser notification API and permission is granted. Unsupported WebViews report this explicitly. Privileged SDK bridge commands are disabled because embedded project and extension content must not reach host capabilities. Clipboard copying uses the WebView's clipboard API with a user-initiated document-copy fallback.

## Shared interface

Desktop consumes `@swarmie/ui-web` from `packages/ui/web`, the same public component
entry used by the website and desktop account controls. HeroUI owns primitive behavior,
geometry, focus and overlays. Shared Google Sans, semantic colors, radii and typography
come from `packages/config`; there is no desktop-specific primitive library or compact
type scale. See the [shared design system](../DESIGN.md),
[frontend rules](../frontend.md) and [desktop composition](DESIGN.md).

Applications compose TextField, Select, ComboBox, Dropdown, Popover and Modal directly.
The app owns state, labels, capability checks and layout. Controls keep their upstream
sizes and variants; application layouts do not redefine their dimensions or padding.
Native controls and their renderer contracts remain in the separate `@swarmie/ui-native`
package.

Settings replaces the sidebar and conversation while retaining the activity rail and toolbar.
Section navigation sits beside bounded content and moves above it in narrow windows.
Keyboard shortcuts provides binding editing and reset, while Nodes groups local and remote connection management. Open Settings from the user menu at the bottom of the rail.

The official [assistant-ui Elements](https://github.com/assistant-ui/assistant-ui/tree/main/packages/ui/src/components/react/assistant-ui/elements)
message-actions and surface modules remain application-owned integrations
under `src/components/assistant-ui/elements`, with their
[upstream MIT license](../../apps/desktop/src/components/assistant-ui/elements/LICENSE).
The shared SearchDialog molecule uses cmdk for search and keyboard navigation inside HeroUI
Modal. The desktop WorkspaceSearch workflow supplies chat, file, command and settings contexts.
Only implemented response actions are exposed. These conversation integrations do not
supply another general-purpose control collection.

`sidebar-section.tsx` and `sidebar-chat-actions.tsx` compose shared controls with Lucide
icons. `workspace-instance.ts` owns the node coordinator and retained state instances.
Assistant-ui owns conversation, message, Markdown, composer and streaming behavior.
Preserve runtime capability checks, answer identifiers, lazy disclosure loading and
keyboard focus restoration when composing these workflows.

## Verify

Run from the repository root:

```sh
mise exec -- just --command bun run typecheck
mise exec -- just --command bun run lint:code
mise exec -- just --command bun run --cwd apps/desktop test:web
mise exec -- just --command bun run --cwd apps/desktop build:web
```

Frontend tests cover ordered replay, native text item boundaries, concurrent requests, provider errors, native resume, refusal to replace unsupported saved sessions, preserving newer drafts, explicit queue versus steering, historical close events, and rollback turn groups. For native-host changes, also run the desktop `check` and `test` scripts on the target platform. For Rust changes, use the root `just check` and `just test` recipes; the backing suites verify session, Git, workspace and transport contracts.

## Scope

Extensions are compiled into the daemon. View descriptors keep their headless commands usable without a desktop. Runtime loading of arbitrary third-party code is not provided. Events are polled in bounded pages every 250 ms across open chats. Review and terminal output have explicit bounds. Provider-specific features remain capability dependent. Guided SSH preparation, account pairing and relay, cross-device access grants and permission management, and managed-cloud provisioning remain pending. Pull request merging is exclusively the user's responsibility.

The titlebar exposes **Background tasks** for live sessions advertising task control. Its popover lists native task status, refreshes while open, and can stop an individual task without cancelling the parent turn. A stop request waits for native confirmation.

## MCP servers

**Settings → MCP servers** manages definitions on the selected node. Add a Swarmie definition or select a harness to register a native definition. Choose a directory to discover project sources, and an open native session to inspect tools and authentication. Enable/disable, edit and uninstall act on the exact source and observed revision. Saved changes expose a **Reopen session** action for affected idle native bindings. Uninstall removes configuration and keeps executables and user-owned files. See [MCP management](../runtime/mcp-management.md) for supported adapters, credential references and native limitations.
