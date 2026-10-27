# Frontend engineering

This guide defines how to change the clients and shared controls. For current node-client
behavior, read [Client UI](client-ui.md). Keep the accepted [design principles](design-principles.md).

## Package ownership

Run frontend workflows from the repository root with the pinned toolchain. The root owns
the lockfile, compiler presets, lint rules, and formatting. Each application and browser
plugin declares its direct dependencies and inherits those presets. The client and root
qualification fixtures use the same published Tembra SDK version.

| Owner | Responsibility |
| --- | --- |
| `apps/client` | Compose the node client and coordinate its screens. |
| `plugins/*/browser` | Provide frontend capabilities and local extension contracts. |
| `packages/client-ui` | Adapt Tembra values for subscribed UI rendering. |
| `packages/config` | Share compiler settings, fonts, theme behavior, and semantic tokens. |
| `packages/ui/web` | Provide neutral DOM controls and their catalogs. |
| `packages/ui/native` | Provide native controls and their catalogs. |

Reuse public controls. Use the shared utility styling setup for presentation and owned
entry stylesheets for tokens and browser behavior. Keep DOM and native renderer dependencies
separate. Hosted authority belongs outside the clients.

## State and local resources

Tembra owns Orbs, References, State, subscriptions, and resource cleanup. Read cached
snapshots through `useSyncExternalStore` and the shared adapter. Do not add a second state
engine, container, object protocol, or cleanup runtime. Keep executable callbacks and UI components in opaque local resources, outside
serialized State and RPC values.

Keep record commands in Conversations, navigation history in Application Shell, and tab
commands in Views. The application coordinates them. Combined presentation snapshots
are read-only; write through the owner State or its commands.

Register executable contributions when their plugin activates. Remove them through the
owning Tembra resource. Closing a view or observer must not stop backend work.

Use Tembra's service-object acquisition for known local services: pass the actual service
from the installed Orb definition to `host.get` or `host.hold`. Use `self.get` for declared
same-host dependencies; Tembra requirements still apply. Factory-based Orbs pass their
actual service objects into integrations rather than creating extra Orbs to obtain types.
Keep opaque resource types explicit. A local declaration does not establish a portable
remote contract or authority.

## Capabilities and operations

A capability can be absent. An available capability must expose all its required methods.
Separate operations that can be available independently. Resolve availability at the owning
screen or contribution, then give its consumer a complete typed interface. Optional data
and presentation callbacks can remain optional.

Keep presentation, admission, and execution distinct. Preserve drafts and visible content
when panels move or access fails. Do not replay a submission with an unknown admission
outcome. Report unavailable backend operations; do not substitute history writes or
simulated responses. The current remote-admission limits are in [Client UI](client-ui.md#admission-and-current-limits).

Use strict TypeScript, discriminated states, type-only imports, and exact optional
properties. Validate values at the owner that accepts them. Handle asynchronous failures
where the operation is owned. Do not add obsolete schema branches or compatibility adapters.

Account and model updates feed subscribed projections. Never persist credentials,
authorization callbacks, or proofs in settings or inspector snapshots.

## Interaction and verification

Keep keyboard access and focus restoration. Show visible focus for keyboard-operated
controls. Text fields, the Composer and drag handles use fill or caret feedback
instead of outlines; keyboard focus must remain visible.
Keep editable mobile text at least 16px. Preserve manual zoom. Phone navigation opens as
an overlay and must not reduce the conversation width.

Run root formatting, type, lint, and affected behavior checks. Shared-control changes also
require affected application and catalog builds. A browser build does not qualify a native
device. Keep screenshots, traces, and operational logs outside the repository.
See [Development](../../DEVELOPMENT.md) for commands.
