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
Settings navigation begins with General and groups personal preferences, integrations
and coding controls.
Settings keeps its title in the settings view without repeating an icon and title in
the workspace toolbar.
The shared SearchField provides a rounded search control and clear action.
Search filters entries across those groups and omits empty groups. Native history import
is not exposed by the desktop client.
Keyboard shortcuts use the shared Table and Kbd components, with per-action edit and reset
icon buttons. Editing records the next valid combination; Escape cancels. Reset restores
the default binding for that action. Duplicate bindings are rejected for edits and resets.

The composer groups the installed agent selector and computer context above the prompt.
Both use shared tertiary menu buttons sized to their labels. The node name appears in the
composer without a duplicate connection label in the sidebar footer.
Permissions sit near attachment actions; usage, model/effort, microphone and send actions
remain grouped near submission. Keep the main action identifiable and every unsupported
capability honestly disabled or explained. The composer arranges shared controls without
changing their native dimensions.

## Conversation and navigation

Projects, Recents and Pinned collapse from their headings. Heading actions reveal on hover
or keyboard focus, while collapsed indicators remain discoverable. Sorting preferences
are independent; dragging selects manual order. Pinned chats appear once. Keep per-project
New chat scoped and omit empty drafts until their first accepted message. Section chevrons
align to the trailing edge of the heading trigger. Rename, archive,
restore and pin actions retain their operation guards and recovery behavior.
Chat search opens a modal and searches titles and message text without filtering the
sidebar. Renaming opens a dedicated dialog; Save commits the captured chat name, while
Cancel and Escape leave it unchanged. Search chats, Find file and Command palette share
the SearchDialog molecule from @swarmie/ui-web. It combines HeroUI Modal with cmdk in an
integrated header and grouped command collection.
A single WorkspaceSearch workflow serves general, chat, file and command contexts. Sidebar
search starts in chats, Find file starts in files, and Command palette starts in the complete
collection. All results returns to the complete collection without dismissing the overlay.
Pinned and recent chats, project files, projects, extension commands and settings destinations
share the menu. Settings destinations reuse the navigation catalog and account visibility
rules. Chat results match titles and message text, including text parts. Files use scoped
TanStack Query reads with debounced input; pending input never shows stale matches or errors.
Tab switches groups, arrows navigate rows, and Backspace on empty input returns to all results.
The consumer supplies filtered results and actions. The molecule owns shared menu spacing
tokens and stable placement, while ordinary dialogs retain their 16px content inset.

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
Settings compose Select with a shared secondary Button trigger so selectors use the same
rounded action geometry. Selected values remain left-aligned and long labels truncate
inside the control. Menus retain Select's collection, focus and keyboard behavior.
Settings actions use the vendor medium size, including node dialogs and agent connection
forms; input fields, switches and radio controls retain their own vendor dimensions.
Dictation configuration lives under Settings → Extensions alongside contributed settings
sections, rather than in the extension Screens menu.

The user menu provides the Settings entry without a separate gear in the activity rail.
Signed-out menus omit a duplicate sign-in heading. Account menus keep signed-in identity
readable outside the actionable collection. The signed-out Account page presents a clear
connection prompt and primary sign-in action; browser previews explain that account
connection requires the desktop app.
Account security appears in Settings only
while an account is signed in. Profile,
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
The permissions popover uses a 4px dialog inset; other dialogs use the shared 16px inset.
The permission popover begins with a short approval explanation and a Learn more link to
the execution-policy documentation. Its header sits directly above the three presets,
which retain their descriptions and availability reasons without additional native fields.
The shared ListBox supplies row hover, pressed and keyboard focus feedback and a trailing
checkmark for the selected preset.
The composer trigger and preset list share the same permission icons.
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
