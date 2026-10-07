---
name: MPAI Desktop
description: A quiet local coding-agent workspace with shared Radix and Tailwind primitives.
colors:
  workspace-background: '#181818'
  rail-background: '#111111'
  sidebar-background: '#171717'
  panel-background: '#161616'
  message-surface: '#242424'
  composer-surface: '#303030'
  context-surface: '#252525'
  attachment-surface: '#333'
  connected: '#48bd80'
  error-background: '#332322'
  error-foreground: '#e8b6af'
  permission-background: '#24241e'
  permission-border: '#54513c'
  permission-foreground: '#e8deb2'
  addition-background: '#173622'
  addition-foreground: '#b7dfbd'
  deletion-background: '#412323'
  deletion-foreground: '#f0c0bf'
  foreground: '#e3e3e3'
  muted: '#aaa'
  border: '#383838'
  focus: '#aaa8ef'
  accent: '#6966ce'
  control-primary: '#e3e3e3'
  control-primary-foreground: '#181818'
  control-secondary: '#303030'
  control-hover: '#3a3a3a'
  danger: '#ed7474'
  floating-surface: '#212121'
  white: '#fff'
  composer-cap-action-hover: '#4a4a4a'
  harness-idle: '#89d6a3'
  harness-working: '#aaa8ef'
  harness-waiting: '#e9bd71'
  harness-unavailable: '#777777'
  permission-full-access: '#ff984e'
typography:
  headline:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize: 28px
    fontWeight: 400
    letterSpacing: -0.025em
  headline-compact:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize: 24px
    fontWeight: 400
    letterSpacing: -0.025em
  title:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize: 14px
    fontWeight: 600
  body:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize: 14px
    fontWeight: 400
    lineHeight: 1.8
  body-compact:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize: 13px
    fontWeight: 400
    lineHeight: 1.8
  label:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize: 12px
    fontWeight: 500
  panel-body:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize: 12px
    fontWeight: 400
    lineHeight: 1.6
  mono:
    fontFamily: '"SFMono-Regular", Consolas, "Liberation Mono", monospace'
    fontSize: 12px
    lineHeight: 1.7
  diff:
    fontFamily: '"SFMono-Regular", Consolas, "Liberation Mono", monospace'
    fontSize: 11px
    lineHeight: 1.8
  settings-heading:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize: 24px
    fontWeight: 500
  settings-heading-compact:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize: 22px
    fontWeight: 500
  control:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize: 13px
    fontWeight: 500
    lineHeight: 1.5
  modal-heading:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize: 20px
    fontWeight: 600
    lineHeight: 28px
  base:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize: 14px
    lineHeight: 1.5
  small:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize: 13px
    lineHeight: 1.5
  extra-small:
    fontFamily: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif
    fontSize: 12px
    lineHeight: 1.5
rounded:
  code: 4px
  field: 8px
  button: 8px
  badge: 6px
  dialog: 20px
  attachment: 10px
  shell: 12px
  context: 16px
  message: 17px
  composer: 22px
  circle: 50%
  floating: 12px
  floating-superellipse: 15px
  pill: 9999px
spacing:
  '4': 4px
  '6': 6px
  '8': 8px
  '10': 10px
  '12': 12px
  '14': 14px
  '16': 16px
  '17': 17px
  '18': 18px
  '20': 20px
  '24': 24px
  '28': 28px
  '32': 32px
components:
  button-primary:
    backgroundColor: '{colors.control-primary}'
    textColor: '{colors.control-primary-foreground}'
    rounded: '{rounded.button}'
    height: 32px
    padding: 0 12px
    typography: '{typography.control}'
  button-primary-hover:
    backgroundColor: '{colors.white}'
  button-secondary:
    backgroundColor: '{colors.control-secondary}'
    textColor: '{colors.foreground}'
    rounded: '{rounded.button}'
    height: 32px
    padding: 0 12px
    typography: '{typography.control}'
  button-ghost:
    backgroundColor: transparent
    textColor: '{colors.foreground}'
    rounded: '{rounded.button}'
    height: 32px
    padding: 0 12px
    typography: '{typography.control}'
  button-ghost-hover:
    backgroundColor: '{colors.control-hover}'
  composer-button-ghost:
    backgroundColor: transparent
    textColor: '{colors.foreground}'
    rounded: '{rounded.pill}'
    height: 28px
    padding: 0 10px
    typography: '{typography.control}'
  composer-send:
    backgroundColor: '{colors.control-primary}'
    textColor: '{colors.control-primary-foreground}'
    rounded: '{rounded.pill}'
    height: 28px
    width: 28px
    padding: '0'
  input:
    backgroundColor: '{colors.control-secondary}'
    textColor: '{colors.foreground}'
    rounded: '{rounded.field}'
    height: 36px
    padding: 0 12px
    typography: '{typography.small}'
  chat-row:
    backgroundColor: transparent
    textColor: '{colors.foreground}'
    rounded: '{rounded.button}'
    height: 32px
    padding: 0 64px 0 12px
  sidebar-section-toggle:
    backgroundColor: transparent
    textColor: '{colors.muted}'
    typography: '{typography.label}'
    rounded: '{rounded.button}'
    height: 28px
    padding: '0'
  sidebar-icon-action:
    textColor: '{colors.muted}'
    rounded: '{rounded.button}'
    height: 28px
    width: 28px
    padding: '0'
  chip:
    backgroundColor: '{colors.control-secondary}'
    textColor: '{colors.muted}'
    rounded: '{rounded.badge}'
    padding: 2px 6px
  attachment-chip:
    backgroundColor: '{colors.attachment-surface}'
    rounded: '{rounded.attachment}'
    padding: 4px 8px
  user-message-bubble:
    backgroundColor: '{colors.message-surface}'
    rounded: '{rounded.message}'
    padding: 12px 16px
  message-icon-action:
    textColor: '{colors.muted}'
    rounded: '{rounded.pill}'
    height: 28px
    width: 28px
    padding: '0'
  permission-card:
    backgroundColor: '{colors.permission-background}'
    rounded: '{rounded.attachment}'
    padding: 14px
    typography: '{typography.panel-body}'
  composer:
    backgroundColor: '{colors.composer-surface}'
    rounded: '{rounded.composer}'
  composer-context:
    backgroundColor: '{colors.context-surface}'
    rounded: 16px 16px 0 0
    padding: 0 4px
  project-dialog:
    backgroundColor: '{colors.floating-surface}'
    textColor: '{colors.foreground}'
    rounded: '{rounded.dialog}'
    padding: 20px
    width: min(522px, 100%)
  project-source-box:
    backgroundColor: transparent
    rounded: '{rounded.attachment}'
    padding: 18px 12px
  floating-frame:
    backgroundColor: '{colors.floating-surface}'
    textColor: '{colors.foreground}'
    rounded: '{rounded.floating}'
  effort-track:
    backgroundColor: '{colors.control-hover}'
    rounded: '{rounded.pill}'
    height: 24px
  effort-label:
    textColor: '{colors.focus}'
  effort-fill:
    backgroundColor: '{colors.accent}'
  effort-thumb:
    backgroundColor: '{colors.white}'
    rounded: '{rounded.pill}'
    size: 28px
  harness-status-dot:
    backgroundColor: '{colors.harness-idle}'
    rounded: '{rounded.circle}'
    height: 8px
    width: 8px
  context-usage:
    textColor: '{colors.muted}'
    rounded: '{rounded.pill}'
    height: 28px
    width: 28px
    padding: '0'
  composer-cap-button-hover:
    backgroundColor: '{colors.composer-cap-action-hover}'
  composer-cap-button-pressed:
    backgroundColor: '{colors.composer-cap-action-hover}'
---

# Design System: MPAI Desktop

## Overview

**Creative North Star: "The Quiet Local Workbench"**

The Quiet Local Workbench gives developers a calm place to run coding-agent conversations against local folders and projects. The approved Codex reference supplies the dark shell, activity rail, resizable sidebar, centered reading column, and rounded composer with a darker computer and harness cap. MPAI uses its own terminal mark and exposes its actual workspace capabilities.

General controls use shared primitives in components/ui. Radix provides popover, Select, menu, dialog, Checkbox, Switch, Collapsible, tooltip, RadioGroup, and Slider behavior; cmdk provides free-text ComboBox keyboard behavior. Tailwind classes style the new primitives using the static theme in styles.css. Existing application layout and conversation-content CSS remains in place. Assistant-ui retains specialized conversation, Markdown, composer, and streaming behavior. Its vendored MIT-licensed Elements supply the command palette and response-copy actions, with capability-dependent host callbacks. Settings replaces the sidebar and conversation with section navigation and a centered content column; project entry opens a centered dialog and project management/review open secondary panels.

**Key Characteristics:**

- Neutral workspace layers with pale primary actions and a violet effort accent.
- Platform system typography and outlined Lucide action icons.
- A centered reading column and context-topped bottom composer.
- Shared Radix and Tailwind controls inside independently scrolling workspace regions.
- Shared focus states and restrained motion with reduced-motion support.

## Colors

The shell retains charcoal content surfaces. Shared primitives use the static Tailwind theme; the frontmatter records source values and the sidecar carries floating shadows and motion. Preview ramps are display aids rather than production scales. Tailwind dark variants follow the .dark class, and color-popover aliases the shared floating surface so vendored Elements dark styles use the same material.

### Primary

- **Pale Action** (`control-primary`, `control-primary-foreground`): primary buttons and send/stop, brightening to white on hover.
- **Effort Violet** (`accent`): effort fill, selected switches, checkboxes, and radio choices. **Soft Violet** (`focus`) marks keyboard focus and gives the model picker effort label its readable #aaa8ef color.

### Neutral

- **Workspace / Rail / Sidebar / Panel Charcoal** (`workspace-background`, `rail-background`, `sidebar-background`, `panel-background`): the established shell hierarchy.
- **Message / Composer / Context Charcoal** (`message-surface`, `composer-surface`, `context-surface`): user messages, drafting surface, and computer/harness cap.
- **Attachment Charcoal** (`attachment-surface`): attached-context content tags.
- **Reading Ink / Muted Ink** (`foreground`, `muted`): shared control text and metadata.
- **Secondary / Hover Charcoal** (`control-secondary`, `control-hover`): fields, secondary controls, and highlighted menu/button states.
- **Lighter Cap Action** (`composer-cap-action-hover`): the localized Tailwind hover/pressed classes for the computer and harness cap actions.
- **Soft Boundary** (`border`): shared fields and outline controls.
- **Floating Charcoal** (`floating-surface`): popovers, selection menus, dropdowns, tooltips, and dialog surfaces.


### Semantic States

- **Connected Green** (`connected`): the live daemon indicator.
- **Harness Idle / Working / Waiting / Unavailable** (`harness-idle`, `harness-working`, `harness-waiting`, `harness-unavailable`): green Idle, violet Starting/Running, amber Needs input/Cancelling, and gray Closed/Disconnected. Failed uses `danger`.
- **Full Access Orange** (`permission-full-access`): the Full access permission row and its selected composer trigger.
- **Error Clay** (`error-background`, `error-foreground`): content error banners and failures.
- **Permission Olive** (`permission-background`, `permission-border`, `permission-foreground`): decision requests.
- **Added Green** (`addition-background`, `addition-foreground`) and **Deleted Red** (`deletion-background`, `deletion-foreground`): unified diff content.

**The Shared Primitive Rule.** Use the shared components and static Tailwind theme for new general controls. Style new primitives through Tailwind classes; preserve Radix/cmdk keyboard semantics and assistant-ui's specialized conversation behavior. Existing application layout CSS remains supported.

**The Semantic Color Rule.** Use content status colors for connection, permission, errors, and diffs. Ordinary navigation uses the shared neutral variants.

## Typography

**Body and UI Font:** the platform sans-serif stack in the frontmatter.
**Code Font:** the frontmatter's monospace stack for code and diffs.

**Character:** quiet and utilitarian. The document distinguishes conversation content from the shared primitive type ramp. The root font size is 16px; the body uses text-base at 14px. The global font-sans stack matches the installed application.

### Hierarchy

- **Headline / Compact:** regular-weight empty-chat welcome.
- **Title:** semibold panel headings; the toolbar uses a compact medium-weight label (13px).
- **Body / Compact:** conversation prose, with relaxed assistant leading; user messages use tighter leading (1.65).
- **Label / Panel Body:** shell metadata and secondary content, not replacements for shared control styles.
- **Base / Small / Extra Small:** the shared text-base (14px), text-sm (13px), and text-xs (12px) ramp, with 1.5 leading. Compact badges use the source-defined 11px step.
- **Control:** medium-weight 13px button text; effort's model-name content uses the 12px muted label.
- **Modal Heading:** the project dialog's semibold heading.
- **Settings Heading / Compact:** the full Settings content heading and source-defined narrow-window variant.
- **Mono / Diff:** code blocks and selectable patches.

Markdown headings descend through the source-defined sizes (18px, 17px, 15px) at semibold weight. Keep contextual typography subordinate to the conversation.

**The Conversation First Rule.** Keep shell content headings restrained and use the shared type tokens inside general controls.

## Layout

The full-height shell uses an activity rail (50px), a sidebar defaulting to 290px and resizing from 220px to 360px, a flexible conversation, and an optional detail panel. The toolbar spans the shell (36px). The main region retains a small outer inset (4px). Reading content and the composer share a centered maximum width (736px).

History scrolls independently above the anchored composer dock. Empty conversations center the terminal mark and welcome in the available history region. The raised context cap is inset from the composer edges (12px) with a slight overlap (1px), a minimum height (36px), horizontal padding (4px), and grouping gaps (4px). The installed harness selector sits at the left with its current-state dot inside the trigger; This computer sits at the far right. Permissions sit immediately after the attachment plus action on the left. A context-usage wheel precedes the model and reasoning effort controls, microphone, and send/stop at the lower right. There is no caption below the composer. The cap controls use 28px shared small buttons with capsule corners and localized #4a4a4a hover/pressed classes, remaining transparent at rest.

Source-defined intermediate desktop layouts (761–1100px) hide the sidebar when a detail panel opens, cap that panel at 370px or 42vw, and reduce conversation insets to 24px. At 760px and below, the sidebar becomes a dismissible overlay, detail panels become right overlays, and controls wrap. These responsive rules describe source behavior; no fresh narrow-viewport validation is claimed for this primitive migration.

Open project in the command palette and Add new project beside the Projects heading both open the existing centered project entry through the shared Radix Dialog with a source-defined width limit (522px) and bounded vertical scrolling. The Elements command palette is capped at 384px, with its own frame inside a transparent, unpadded Radix Dialog wrapper; context dialogs are capped at 640px. Width and scroll constraints belong to application composition; ordinary dialog padding and material come from the shared primitive.

Settings spans the sidebar and main columns while retaining the rail and toolbar. It has a section navigation column (240px), a centered content column capped at 640px, and content padding (48px 40px). At 760px and below, navigation becomes a horizontal row and content padding becomes 26px 18px. General, Notifications, Import, Keyboard shortcuts, and Connections are the implemented sections.

The frontmatter's spacing steps describe recurring content and shell gaps. They coexist with the shared primitive classes rather than replacing their dimensions.

## Elevation & Depth

Depth comes from tonal layering and restrained boundaries. Shared popovers, menus, tooltips, and ordinary dialogs use the floating shadow; the specialized composer keeps its thin focus ring. Narrow-window sidebar/detail overlays retain directional shadows.

### Shadow Vocabulary

- **Floating Frame:** shared floating panels and ordinary dialogs (`0 10px 15px -3px #0003, 0 4px 6px -4px #0003, 0 0 0 1px #ffffff1a, inset 0 0 0 1px #ffffff0a`).
- **Sidebar Overlay:** narrow sidebar (`8px 0 22px #0005`).
- **Detail Overlay:** narrow right panel (`-8px 0 22px #0005`).
- **Composer Focus:** specialized draft-container ring (`0 0 0 1px #4c4c4c`).

**The Tonal Depth Rule.** Use neutral content layers for workspace hierarchy and the shared frame material for floating panels.

## Shapes

Shared buttons, fields, menu items, and disclosures use gently rounded control corners. Badges use the smaller badge corner. Floating frames use a round fallback, with the documented superellipse corner when supported; dialogs use their own round corner. The effort thumb is white and circular, while its track and fill are fully rounded.

Content retains the established shell corners, user-message shape, rounded composer, top-only context-cap corners, and smaller attachment/permission surfaces. Keep assistant prose directly on the conversation ground.

## Components

### Buttons

The shared Button renders a native DOM button with Tailwind variants and sizes. Default buttons are 32px high; sm is 28px and lg is 40px. Small text actions have 10px horizontal padding; other text actions have 12px. Standard and small icon buttons are 32px and 28px square with zero padding. Corners are 8px, internal gaps are 6px, and text uses the shared 13px medium role. Primary actions are pale with a dark foreground; secondary actions use the secondary surface, while ghost actions start transparent and gain the hover surface. Keyboard focus uses a 2px ring with a 2px background offset. Disabled controls use opacity 0.4. Composer actions use sm/icon-sm; computer, harness, permissions, model, attachment, microphone, and send/stop add rounded-full classes for capsules or circles. Default size elsewhere remains 32px. Send/cancel preserves assistant-ui's DOM handlers and refs.

### Chips

The shared Badge uses compact 11px text, a 6px corner, and 2px 6px padding with neutral or destructive variants. Specialized attachment and context content tags retain their compact source-defined wrappers, truncation, image previews, and named removal actions. Removal uses the shared Button.

### Cards / Containers

Project, permission, queue, and content regions use their actual source-defined surface and spacing. Permission requests carry their warm border and text, readable context, and wrapping shared actions. Ordinary assistant text remains free-flowing. Content containers retain their source-defined material and spacing.

### Inputs / Fields

Shared Input and Textarea use Tailwind field classes: secondary material, a thin boundary, 8px corners, 13px text, and visible focus rings. Input is 36px high; Textarea starts at 80px minimum height with vertical padding. InputGroup positions an icon beside the shared Input. Radix backs Select, Checkbox, RadioGroup, and Switch, preserving named controls and selected/disabled states. The cmdk-backed ComboBox accepts suggested branches and arbitrary typed references. The hidden file-upload input is activated through a named shared Button. Assistant-ui's multiline composer remains specialized.

### Navigation

The rail, project sidebar, chat rows, and Settings section list compose shared button actions. Use lucide-react as the sole action-icon library; keep controls named and focus restoration intact. The context-wheel SVG remains meaningful progress geometry rather than an action icon. The composer harness uses the shared Radix Dropdown/Menu with an sm ghost trigger, installed-only choices, icons in menu rows, and a selected indicator. Its cap trigger replaces the harness icon with a current-state dot; hovering or focusing the whole trigger opens the status tooltip, and clicking opens the selector. Other harness fields use shared Radix Select. Across composer, Settings, and forms, an empty selector reads No harness installed. Discovery does not establish authentication readiness.

### Sidebar Sections and Chat Actions

In project grouping, Projects, Recents, and Pinned use collapsible section headings. Their 28px title rows use 12px medium muted text, a 12px Lucide chevron, and 28px icon actions with 14px Lucide icons. Expanded chevrons and title actions reveal on section hover or keyboard focus within; collapsed chevrons remain visible. Projects reveals its sorting options and Add new project, which opens the existing centered project dialog. Each project row similarly reveals a New chat plus action on project hover/focus, keeping the new conversation scoped to that project.

Each heading's sort menu offers Recent activity and Manual order with a selected indicator. Preferences are independent and saved under mpai.sidebar-sorting: Projects defaults to manual, Recents to recent activity, and Pinned to manual. Project activity derives from its conversations' updated dates; chat recent activity uses updated dates with created dates as fallback. Project chat lists follow the Recents preference. Dragging a chat into a different position switches the applicable chat preference to manual and preserves order undo.

Chat selection and row actions are sibling buttons, never nested buttons. The selection button reserves 64px on the right for controls. Unpinned rows reveal Pin chat and Chat options on hover/focus; pinned rows retain Unpin chat at rest while Chat options remains hidden until hover/focus. The shared dropdown offers Rename chat, Pin/Unpin chat, and Archive/Restore chat. Archive/restore is disabled for starting, running, needs_input, and cancelling sessions, and completed changes retain undo. Pinned appears only when visible pinned chats exist, and those chats are excluded from Recents and project lists. Empty drafts remain hidden. Pinning applies to chats.

These controls independently implement the installed application's source-observed sidebar behavior using the shared Radix/Tailwind system; no proprietary sidebar primitives are vendored.

### Dialogs and Disclosures

Project entry, file finder, and context views use the shared Radix Dialog. It provides named, centered overlays, a 20px round dialog corner, 20px padding, shared floating material/shadow, and a half-opacity black backdrop. Project content remains bounded and scrollable; the bordered source region and bottom-right Cancel/Create layout retain their composition. Create-folder and clone modes require the Workspace extension. Idle Cancel/Escape dismisses project entry; busy submission prevents dismissal and mutation, and errors remain visible.

Radix Collapsible backs disclosures with shared Tailwind trigger and indicator styling. Preserve lazy loading, initial expansion, answer identifiers, and capability-dependent actions.

### Command Palette

The official assistant-ui Elements command palette is vendored under components/assistant-ui/elements with its upstream MIT license. The host supplies controlled commands, query, activeId, and onRun. Actions and Conversations form separate groups; case-insensitive substring filtering retains displayed group order. Arrow keys wrap through the filtered rows, the active row scrolls into view, and Enter runs the active command. Search retains focus through aria-activedescendant; Radix provides named dialog semantics, Escape dismissal, and focus restoration.

Its 384px frame uses the Elements floating surface, the global popover color in dark mode, a 20px round corner, and a subtle boundary. The host Dialog is transparent and unpadded to avoid a second frame. Search uses 13.5px text with a 14px search icon; group and key labels use 11px monospace, rows use 13px text, and the list scrolls within a 288px maximum height. Selected and hover rows use restrained foreground-opacity fills; secondary labels use the shared muted color. cmdk remains specific to free-text ComboBox behavior.

### User Message Bubble and Actions

Outgoing messages use a right-aligned, content-width bubble limited to 88% of the reading column, with 17px corners, the message surface, 16px horizontal and 12px vertical padding, and 13px text at 20px leading. A separate right-aligned action row sits below the bubble with 4px gaps and a reserved 28px height. It is hidden by default, revealing time and copy on hover over the user-message group or keyboard focus within, without shifting layout. Its 150ms opacity transition honors reduced motion. When a valid stored event timestamp exists, the row displays local HH:mm in 12px tabular numerals, with the full date/time available on the time element. Missing or invalid timestamps omit the time; display never substitutes the assistant runtime's default date. A connected message with a saved session and no valid date requests read-only session/transcript backfill. Concurrent reads for that thread/session are deduplicated; existing valid dates, content, and thread update time are preserved, and a changed session discards stale results. Backfill reads stored events without resuming, launching, or sending a prompt.

Copy is a 28px circular shared ghost action with a 14px Lucide Copy/Check icon, an accessible label, and a hover/focus tooltip.

Both user and assistant copying use the shared useMessageCopy hook: only that message's text content is copied through the existing clipboard/document-copy fallback. Both controls use the exact accessible and hover/focus tooltip labels Copy and Copied, shared muted text with foreground hover, and a green success check. Success gives three-second confirmation, repeated copying resets the timer, unmount clears it, and late completion does not update an unmounted message. Copy failures surface through the workspace error state.

### Assistant Response Actions

The official vendored Elements MessageActions renders a 28px circular Copy action with a 14px outlined icon. It sits inside assistant-ui ActionBarPrimitive.Root with a reserved 28px row, hidden by default and revealed by hovering the whole assistant message or keyboard focus within. The row uses a 150ms reduced-motion-aware opacity transition and remains hidden while running. Copy sits beside a 28px circular, icon-only Fork from here action with a 14px Lucide GitFork icon and tooltip, only when an idle session advertises fork support and the response has a native anchor. Copy uses only that response's text content and the existing clipboard/document-copy fallback. Success swaps the copy icon for a green check for three seconds; timers are reset on repeated copying and cleaned up on unmount, and failures surface through the workspace error state. Host callbacks expose only implemented response actions. The upstream Elements surfaces preserve their restrained hover, focus, pressed, icon-swap, and reduced-motion states.

### Floating Panels

Shared floating classes style Radix Popover, Dropdown, Select, and tooltip surfaces with the recorded charcoal material, conditional corners, and shadow. Popovers use 16px padding; menu catalogs use 6px frame padding and shared highlighted item rows. ComboBox combines Radix positioning with cmdk keyboard behavior and focus restoration. These new primitive styles are Tailwind classes; older application CSS still composes shell, panels, and conversation content.

### Model and Session Controls

The lower-right shared sm ghost trigger displays the selected or actual native model and advertised effort label. With advertised effort choices, it opens a top-end effort popover capped at 288px with 16px shared Popover padding and a visually hidden accessible heading. The header places a shared icon-sm Fast mode lightning toggle on the left, a centered shared secondary lg button showing current effort in readable #aaa8ef violet and the muted model name on two lines with a right chevron, and a shared icon-sm reset action on the right. Fast mode toggles only the advertised native boolean fast setting; it never changes effort. Unsupported Fast mode is disabled with an explanation, and active Fast mode uses the soft violet focus color. Reset restores the selection captured when the effort menu mounted/opened; it does not choose a provider default.

A Radix discrete Slider sits below this header. Its 24px rounded track carries a violet fill and a 28px circular white thumb; noninteractive 4px circle markers are positioned as content over the track. Choice labels follow dragging, the slider announces advertised choice labels, and configuration commits only on release. The configuration identifier is the advertised effort, reasoningEffort, or thinkingLevel.

The centered model button opens a separate nested model catalog capped at 320px. The catalog is a Radix Select nested in the effort Popover, using advertised model items and selected indicators. Radix closes it on selection, and an unsuccessful configuration change reopens it; live capability and idle constraints remain effective. A harness without advertised effort shows the small model picker beside the Fast mode toggle. No model catalog or effort scale is hardcoded.

A separate shared ghost permissions trigger sits directly beside the attachment plus action on the composer left. Its shield icon accompanies the effective preset label: Ask for approval, Approve for me, or Full access. Nonmatching native settings show Custom permissions; absent configuration shows Permissions. Full access uses orange text. Pre-session overrides take precedence when deriving the label and selection.

The trigger opens a top-start shared Radix Popover capped at 440px, named Permissions, with the heading How should actions be approved? Its Radix radio picker contains only the three permission presets. Each preset batches the actual native configuration fields; it is enabled only when every required value is advertised. Unsupported presets remain visible and disabled with Not supported by this harness. Existing sessions lock presets whose fields are not live; changes also require idle state and actual configuration support.

For Codex, Ask for approval maps to approvalPolicy on-request and sandbox workspace-write, with approvalsReviewer user when that descriptor exists. Approve for me uses the same approval/sandbox pair with approvalsReviewer auto_review and requires that exact advertised value. Full access maps to never and danger-full-access, with reviewer user when present. For permissionMode-based harnesses, Ask for approval maps to default and Full access to bypassPermissions; Approve for me is unavailable. Labels reflect exact matching effective fields rather than inferred authority.

Before the first prompt, the client reads runtime/configuration through a temporary native connection only when the harness advertises ephemeral support. The runtime sends no prompt, saves no chat, and closes that connection after reading configuration/models. The picker never starts a session to obtain a catalog. Preview reads run only while idle; busy sessions retain disabled cached choices and refresh on returning to idle. Busy-race preview messages are suppressed while other errors remain visible. Codex's current default is visible before first send and the effective default is passed when starting the prompted session. Missing catalogs, loading, errors, and unsupported settings stay explicit.

### Context-Topped Composer

The darker cap places the harness status/selector button on the left and This computer on the right above the rounded draft surface. This computer opens Settings; project and worktree management remain in their existing workflows. The multiline assistant-ui draft has a minimum height (56px), padding (16px 16px 8px), line height (20px), and maximum height (220px). The footer uses a minimum height (40px), padding (0 8px 8px), and gaps (4px); the right action cluster also uses 4px gaps. Controls use shared sm/icon-sm sizes through the Button API, while these inset and grouping rules belong to application composition. The attachment plus action and adjacent permissions trigger sit to the left; context usage, model and effort, dictation, and send/stop sit to the right. No caption appears below the composer. Dictation appends final recognized speech when supported; errors are visible and unsupported browsers explain the disabled microphone. Queue preserves follow-up behavior, and Steer appears only when advertised.

The harness trigger contains an 8px state dot. Its hover/focus tooltip names the current state and explains the color legend: Idle in green, Running in violet, Needs input and Cancelling in amber, Failed in red, and Closed / Disconnected combined in gray. Starting uses violet and is described as the current state when active. Unrecognized states remain explicitly unknown.

A 28px context-wheel control sits immediately before the model trigger, with a 14px ring glyph. Its hover/focus tooltip reports the actual percentage and compact used/capacity token counts when provided by the harness; used tokens and capacity determine percentage when both are valid, otherwise a reported percentage is used. Visual progress is clamped to the ring range. Missing capacity stays explicit, and unreported usage reads Usage not reported yet and Waiting for harness usage data. Accessible naming preserves the reported percentage or says usage unavailable.

Shell and panel transitions use a short settling curve (180ms, cubic-bezier(0.16, 1, 0.3, 1)); composer focus uses 140ms and working status uses a subtle 1.4s opacity pulse. Shared Tailwind controls use color/state transitions; popovers enter over 350ms with the settling curve and exit over 200ms ease-in. The reduced-motion query disables animation and transitions.

## Do's and Don'ts

### Do:

- **Do** preserve the approved dark shell, activity rail, centered conversation, and context-topped composer.
- **Do** use the shared Radix/Tailwind primitives and their static theme tokens for new controls.
- **Do** use application CSS for composition, content surfaces, width constraints, and scrolling.
- **Do** retain assistant-ui for specialized conversation, Markdown, composer, and streaming behavior.
- **Do** keep project entry centered through Open project in the command palette or Add new project in the Projects heading.
- **Do** let Settings span the sidebar and main area with its implemented sections.
- **Do** use actual harness labels, installed choices, advertised models, and native configuration identifiers.
- **Do** keep controls named, preserve keyboard focus, and honor reduced motion.
- **Do** retain useful empty, loading, unavailable, error, and unsupported states.

### Don't:

- **Don't** add custom CSS skins for new primitives; express their styles through Tailwind classes and shared tokens.
- **Don't** change the 16px root to alter shared rem dimensions.
- **Don't** add decorative gradients or large dashboard headings to this quiet workspace.
- **Don't** wrap every conversation section in a card.
- **Don't** copy the reference application's proprietary mascot or invent unavailable actions.
- **Don't** present absent harnesses as selectable or discovery as proof of authentication.
- **Don't** hardcode model catalogs or reasoning effort scales.
- **Don't** replace Lucide action icons with text glyphs or another icon library.
