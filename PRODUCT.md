# Product

<!-- impeccable:product-schema 1 -->

## Platform

adaptive

## Users

Developers running coding-agent sessions against local folders and projects.

## Product Purpose

Provide a usable, extensible multi-harness runtime and a desktop workspace.

## Stack

Rust runtime with ports and adapters. Native SDK hosts a system WebView. React and assistant-ui render the desktop interface. Shared Tailwind primitives use Radix behavior and cmdk for free-text ComboBox keyboard interaction. Official assistant-ui Elements are vendored with their MIT license for the command palette and response actions. Clients communicate with the daemon through JSON-RPC.

## Capabilities and Constraints

The local desktop application targets Linux and macOS through Native SDK; the browser interface is a development preview. The runtime works headlessly. Product features can be optional extensions. The desktop opens Git repositories, creates isolated worktrees, runs installed harnesses, handles permissions, and reviews changes against HEAD. Git operations belong to an extension rather than the runtime core. Message copying shares fallback, temporary confirmation, cleanup, and visible errors across user and assistant messages. Pull request merging remains the user's responsibility.

## Product Principles

Keep the core small. Advertise actual capabilities. Preserve native harness behavior. Prove extension contracts with working features before generalizing them.

## Visual Commitment

A dark, quiet workspace with an activity rail, a resizable project sidebar, centered conversations, and a rounded bottom composer with a darker computer and harness cap. General controls and selected rows use the shared Radix and Tailwind design system. Tailwind classes style new primitives from static theme tokens; older application layout/content CSS remains. Global platform fonts follow the installed application, with a 16px root and 14px/13px/12px base/small/extra-small ramp. Tailwind dark variants follow the .dark class and the popover color aliases the shared floating surface. Conversation content takes priority. The command palette groups Actions and Conversations with substring filtering, wrapping keyboard navigation, and active-row scrolling. Outgoing messages use a bubble with a separate right-aligned row for a real stored HH:mm timestamp and Copy. The row is hidden until message hover or keyboard focus within, reserving 28px to prevent layout shift. Missing timestamps stay omitted until an existing connected session transcript supplies a valid date; backfill preserves content and existing dates and does not launch or resume a session. User and assistant copy controls share Copy/Copied labels, readable muted styling with foreground hover, and a green confirmation check. Assistant responses reveal Copy and the supported icon-only Fork from here on whole-message hover or keyboard focus within, reserving a 28px row with a 150ms reduced-motion-aware opacity transition. Copy gives temporary confirmation; fork requires native support and a message anchor. Lucide is the sole action-icon library; the context wheel retains its progress geometry. Settings is a full workspace view with section navigation and centered content; project management and Git review use secondary panels. Global New chat starts without a project in the configured chat directory, defaulting to Documents and configurable in Settings → General; per-project chat actions remain scoped. Empty drafts stay out of navigation until the first accepted message. Project entry is available through Open project in the command palette or the hover/focus Add new project action in the Projects heading. The composer keeps the installed harness selector with an embedded state dot at the cap left and This computer at the far right. Permissions sit immediately beside the attachment plus on the composer left; context usage, model/effort, microphone, and send controls sit at the lower right. There is no bottom caption.

## Sidebar Navigation

Projects, Recents, and Pinned collapse from their headings. Expanded chevrons and heading actions reveal on hover or keyboard focus; collapsed chevrons stay visible. Projects exposes sorting and Add new project, while project-row New chat remains scoped. Section sorting preferences are independent: Projects and Pinned default to manual order, Recents to recent activity. Chat dragging selects manual order. Pinned chats appear once, with an Unpin action visible at rest; other chat pin and more actions reveal on hover/focus. Chat options provide rename, pin/unpin, and archive/restore with busy-session restrictions and undo. Empty drafts remain hidden. These behaviors are independently implemented with shared Radix/Tailwind controls from the installed application reference.

## Native Session Configuration

Model and configuration choices come from the selected installed harness. Before the first prompt, runtime/configuration reads native choices through a temporary connection only when the harness advertises ephemeral support; it sends no prompt, saves no chat, and closes the connection after reading. The picker does not start a session to load models. Preview reads pause while busy, retain disabled cached choices, and refresh on returning to idle; other preview failures remain visible. Codex exposes its current default before first send, and the effective default is passed when the prompted session starts. Reasoning effort uses advertised discrete choices and commits on slider release. Live configuration and session actions follow actual harness capabilities. Executable discovery does not establish authentication readiness.

The left permissions trigger shows Ask for approval, Approve for me, or Full access from exact effective native settings, with Custom permissions for nonmatching settings. Its popover contains only these three presets as Radix radio choices. Each selection batches advertised native fields; unsupported presets are disabled. Approve for me requires Codex's actual advertised approvalsReviewer auto_review alongside on-request approval and workspace-write sandbox. Full access maps to native unrestricted settings and uses orange text. Pre-session overrides and live/idle constraints remain effective.

The right effort popover centers readable #aaa8ef violet effort text and the model between Fast mode and reset actions, with a Radix slider below. The lightning toggle changes only the native advertised fast boolean; it never changes effort and stays disabled when unsupported. Reset restores effort captured when the menu opened. The center button opens the separate advertised model catalog; harnesses without effort show that picker beside Fast mode. The context wheel preceding the model reports actual usage percentages and token counts on hover/focus, or honestly waits for harness usage data. The harness selector's tooltip names the current state and explains green Idle, violet Running, amber Needs input/Cancelling, red Failed, and the combined gray Closed / Disconnected legend; Starting also uses violet.

Composer density follows the installed local ChatGPT reference through source evidence. Compact controls use shared 28px sm/icon-sm sizes; general buttons default to 32px, with a 40px large variant. Primary actions are pale; effort uses a violet accent and circular white thumb. Composer actions use capsule/circle classes, and the computer/harness cap keeps its localized lighter hover. Draft padding and footer grouping are application composition. The cap harness opens a shared Radix installed-only Dropdown/Menu with icons and a selected indicator; other harness forms use shared Radix Select.
