# Product

<!-- impeccable:product-schema 1 -->

## Platform

adaptive

## Users

Developers running coding-agent sessions against local folders and projects.

## Product Purpose

Provide a usable, extensible multi-harness runtime and a desktop workspace.

## Stack

Rust runtime with ports and adapters. Native SDK hosts a system WebView. React and assistant-ui render the desktop interface. Shared HeroUI atoms provide control behavior and Tailwind composes application layouts. Official assistant-ui Elements are vendored with their MIT license for the command palette and response actions. Clients communicate with the daemon through JSON-RPC.

## Capabilities and Constraints

The local desktop application targets Linux and macOS through Native SDK; the browser interface is a development preview. The runtime works headlessly. Product features can be optional extensions. The desktop opens Git repositories, creates isolated worktrees, runs installed harnesses, handles permissions, and reviews changes against HEAD. Git operations belong to an extension rather than the runtime core. Message copying shares fallback, temporary confirmation, cleanup, and visible errors across user and assistant messages. Pull request merging remains the user's responsibility.

## Product Principles

Keep the core small. Advertise actual capabilities. Preserve native harness behavior. Prove extension contracts with working features before generalizing them.

## Native Account Experience

Accounts are optional for local projects, chat files, Harness authentication and nodes. The activity
rail account control shows the current identity and avatar, with a user-icon fallback, and offers
Profile, Settings, Account security and Sign out when signed in. Settings remains available signed
out. Profile and Account security are sections of the full Settings workspace. Security explains
this installation's credential-vault or owner-session storage and scoped logout; browser login has
an independent session. Account changes do not alter Harness authentication or publish local files.

The persistent native owner retains sign-in attempts, protected credentials and the authorized browser
URL independently of a view or window. Reopening Settings can continue a pending browser sign-in.
The rail and Settings share one account store per window, observing sanitized owner snapshots with
independent client cursors. Owner restart reconciles current state, and reconnect never replays a
mutation. Observation itself performs no hosted profile read or credential renewal. Errors and stale
availability remain visible while local work stays usable.

Name and avatar writes use confirmed revisions and mutation receipts. An edited name retains the
revision at which editing began: cross-window updates neither replace nor implicitly rebase the
draft. Explicit Refresh profile rebases the retained draft, and changing accounts clears drafts and
receipts. See [native accounts](../runtime/accounts.md) for lifecycle qualification and remaining
platform and hosted-browser limits.

## Visual Commitment

Use the shared [Swarmie design system](../DESIGN.md): Google Sans, neutral semantic surfaces,
monochrome actions, the HeroUI Medium radius base and optional labeled category accents.
The desktop arranges an activity rail, resizable navigation, centered conversation content
and a bottom composer. This composition does not introduce another palette, control
library or compact typography scale. Canonical artwork retains its original lettering.

Settings is a full workspace view; project management and review use secondary panels.
Global New chat starts without a project in the configured chat directory. Empty drafts
stay out of navigation until their first accepted message. Message action rows reveal on
hover or keyboard focus without shifting content. Copy gives temporary confirmation;
message forking requires actual native support and a retained anchor.

## Sidebar Navigation

Projects, Recents, and Pinned collapse from their headings. Expanded chevrons and heading actions reveal on hover or keyboard focus; collapsed chevrons stay visible. Projects exposes sorting and Add new project, while project-row New chat remains scoped. Section sorting preferences are independent: Projects and Pinned default to manual order, Recents to recent activity. Chat dragging selects manual order. Pinned chats appear once, with an Unpin action visible at rest; other chat pin and more actions reveal on hover/focus. Chat options provide rename, pin/unpin, and archive/restore with busy-session restrictions and undo. Empty drafts remain hidden. These application behaviors compose shared HeroUI controls.

## Native Session Configuration

Model and configuration choices come from the selected installed harness. Before the first prompt, runtime/configuration reads native choices through a temporary connection only when the harness advertises ephemeral support; it sends no prompt, saves no chat, and closes the connection after reading. The picker does not start a session to load models. Preview reads pause while busy, retain disabled cached choices, and refresh on returning to idle; other preview failures remain visible. Codex exposes its current default before first send, and the effective default is passed when the prompted session starts. Reasoning effort uses advertised discrete choices and commits on slider release. Live configuration and session actions follow actual harness capabilities. Executable discovery does not establish authentication readiness.

The left permissions trigger shows Ask for approval, Approve for me, or Full access from exact effective native settings, with Custom permissions for nonmatching settings. Its popover contains only these three presets as shared radio choices. Each selection batches advertised native fields; unsupported presets are disabled. Approve for me requires Codex's actual advertised approvalsReviewer auto_review alongside on-request approval and workspace-write sandbox. Full access maps to native unrestricted settings and uses orange text. Pre-session overrides and live/idle constraints remain effective.

The right effort popover centers readable #aaa8ef violet effort text and the model between Fast mode and reset actions, with a shared Slider below. The lightning toggle changes only the native advertised fast boolean; it never changes effort and stays disabled when unsupported. Reset restores effort captured when the menu opened. The center button opens the separate advertised model catalog; harnesses without effort show that picker beside Fast mode. The context wheel preceding the model reports actual usage percentages and token counts on hover/focus, or honestly waits for harness usage data. The harness selector's tooltip names the current state and explains green Idle, violet Running, amber Needs input/Cancelling, red Failed, and the combined gray Closed / Disconnected legend; Starting also uses violet.

Controls retain the shared HeroUI sizes and variants. Primary actions use the monochrome theme pair; session-specific effort remains a labeled workflow value. Composer actions use capsule/circle classes, and the computer/harness cap keeps its localized lighter hover. Draft padding and footer grouping are application composition. The cap harness opens a shared HeroUI installed-only Dropdown with icons and a selected indicator; other harness forms use shared HeroUI Select.
