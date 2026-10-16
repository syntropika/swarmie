# Desktop composition

The canonical [Swarmie design system](../DESIGN.md) owns colors, typography, geometry,
spacing, shadows, motion, icons and brand applications. This document describes how the
desktop arranges those resources. It does not define another primitive library or density.

## Workspace

Use the shared NavigationShell for the activity rail and responsive, resizable navigation.
Conversation content remains central; settings and capability workflows can replace that
canvas or open secondary panels. Preserve user-controlled navigation width, available
collapse behavior, keyboard resizing and focus restoration when temporary navigation closes.
Application controllers own routes, selected capability and project/session state.

The composer groups the installed agent selector and computer context above the prompt.
Permissions sit near attachment actions; usage, model/effort, microphone and send actions
remain grouped near submission. Keep the main action identifiable and every unsupported
capability honestly disabled or explained. The composer arranges shared controls without
changing their native dimensions.

## Conversation and navigation

Projects, Recents and Pinned collapse from their headings. Heading actions reveal on hover
or keyboard focus, while collapsed indicators remain discoverable. Sorting preferences
are independent; dragging selects manual order. Pinned chats appear once. Keep per-project
New chat scoped and omit empty drafts until their first accepted message. Rename, archive,
restore and pin actions retain their operation guards and recovery behavior.

Messages expose copy and supported message actions on hover or keyboard focus without
shifting surrounding content. Display only retained timestamps and real provider-supported
anchors. Copy confirmation is temporary; failed operations remain visible. A colored
agent-state dot or permission marker supplements a readable label rather than replacing it.

## Forms, menus and dialogs

Compose TextField, Input, Label, Description and FieldError directly. Use Select for bounded
choices, ComboBox for suggestions/custom values, Dropdown for action menus and Popover for
free-form floating content. Modal owns modal dismissal, focus and overlay behavior. Busy
operations preserve an honest pending state and the workflow's required dismissal guards.
There are no parallel Dialog, OptionSelect or SuggestionField wrappers.

Account menus keep the identity block readable outside the actionable collection. Profile,
account security and session management retain the full settings navigation. Errors stay
near the affected operation. Native sign-out does not imply deleting local user files.
Agent tool setup follows advertised capability and readiness rather than treating executable
discovery as proof of an authenticated connection.

## Model, permissions and background tasks

Agent tool, model, effort and permission choices come from the selected installed integration.
Read pre-session choices without prompting or persisting a conversation where temporary
configuration access is supported. Keep unavailable cached choices honest, discard stale
results and respect live/idle restrictions. Configuration commits follow their actual
control lifecycle; no local visual scale invents provider capabilities.

The permissions control names its effective preset, including custom or unsupported states.
Usage reports actual retained counters or a readable waiting state. Background tasks keep
stable identities, pending actions, recoverable errors and a reachable close action while
individual actions are busy.

## Category accents

Keep canvas, fields, cards and actions neutral. Optional mint, lilac, peach and mustard
chips or avatar fallback fills identify named groups or participants. Their assignments
are contextual examples, not fixed provider, permission, success or warning meanings.
Use the shared dark tint and foreground pair in dark mode, with readable labels.

## Responsive behavior and motion

Panels adapt to the available container; the workspace owns viewport-dependent navigation.
Allow action groups and long names to wrap. Keep essential actions keyboard-reachable and
restore focus after temporary surfaces close. Use the shared reduced-motion-aware feedback;
animation must not delay a subsequent action or be required to understand state.
