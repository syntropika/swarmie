# Client design principles

Apply the [frontend engineering rules](frontend.md).

## Component baseline

HeroUI is the unchanged base for web controls. `packages/ui/web` reexports its
public components and types. Keep the explicitly accepted shared theme choices
below centralized; do not add wrapper-only variants or unrelated control recipes
for dimensions, shadows, validation, disabled or pending states. Use the vendor's supported props and compound
parts. Keep upstream source and notices intact.

The shared theme owns these explicit choices:

- Google Sans is the application font. One shared Tailwind/HeroUI type ramp uses
  12-pixel metadata, 13-pixel controls, 14-pixel body copy and 16/18/22-pixel
  subheadings and page headings at the default browser font size. The desktop
  density also applies on phones: form text, icons, button sizes and navigation
  rows do not grow at viewport or coarse-pointer breakpoints. Base buttons use
  36 pixels, small buttons 32 pixels and large buttons 40 pixels; application
  chrome retains its named role geometry. Root font sizing, browser zoom and
  vendor weights remain unchanged.
- Coral `#D96E5C` is the global interface accent. HeroUI supplies the semantic palette, borders and
  separators. A shared dark surface ramp tunes depth and neutral control fills. Separate
  application row tokens strengthen hover and selected fills without replacing
  those vendor control roles. Muted text uses a stronger shared neutral role.
  Fields use the frame's secondary surface with the vendor secondary border.
- Primary buttons use an inverse neutral pair: foreground as fill and background
  as text. They are near-white in dark appearance and near-black in light
  appearance. Their hover and pressed fills derive from that pair. Ordinary
  buttons, including icon-only actions, have a shared 12-pixel radius.
  Vendor interaction remains unchanged; the shared theme defines control dimensions.
- Secondary button text and icons use the neutral default foreground in both
  appearances. Do not tint ordinary secondary actions with the accent.

Composer Send and Stop retain the explicitly requested coral action color as a
local Composer exception. Do not apply this exception to ordinary primary actions.

A direct JavaScript export is not sufficient if imported CSS still restyles the
control. Keep base exports, global styles and consumer classes consistent with
this boundary. Application code may supply layout utilities. Do not reconstruct
vendor control styles in consumers or catalog examples.

## Brand color roles

The shared web token `--color-brand-accent` defines interface coral in
`packages/config/tailwind/web-tokens.css`. HeroUI uses this token for `--accent`.
Use the shared token for active navigation markers and reasoning indicators.
Do not copy the hex value into individual components.

Composer Send and Stop use this coral fill with white icons. The shared
`--color-brand-accent-foreground` token defines their white foreground.
Small accent text and keyboard focus use `--color-brand-accent-text`.
This role uses `#8D5147` in light appearance and interface coral in dark appearance.
Check contrast against the actual surface in each appearance.

The brandkit records interface coral as `coral`. Its `soft_coral` value,
`#DDA99F`, is the color in the current logo core and mascot artwork.
These artwork colors have a separate role from the interface accent.
Brand color references are in `assets/brandkit/tokens/colors.json` and
`assets/brandkit/tokens/colors.css`. Runtime theme values belong to
`packages/config`.

## Application compositions

The application owns the frame, navigation, panel arrangement, resize behavior,
content widths, toolbars and scroll regions. Adjacent desktop panes share a
boundary. Only corners meeting the outer frame round. The rail is part of the
frame. Phone content retains its upper corners and has no side or bottom frame.
The current rail destination retains the coral icon and edge marker.

The exterior frame uses the vendor secondary surface color. Project navigation and
central panes share the vendor surface color. Keep these roles separate from control surfaces: changing frame
placement must not invert fields, menus or button colors. Phone browser chrome and
the page backdrop follow the exterior frame color.

Dark appearance uses a subtle amber charcoal ramp: `#080806` for the base,
`#10100e` for navigation and central panes, `#181713` for the frame, fields and
overlays, `#1f1e19` for the Composer, `#25241f` for default controls and `#3e3c35`
for segmented selection.
Dark row hover and selection derive from the pane and foreground roles. Light
appearance, typography, status colors and the coral accent retain their values.

Floating overlays share the frame's secondary surface color. The Composer uses
the tertiary surface, with its context strip on the secondary surface. The
navigation divider uses the vendor separator role and retains its resize behavior.
The current shape preview uses one shared 12-pixel radius for buttons, fields,
floating menus and their options, tabs, modals, upper sheet corners, grouped
settings regions and outer frame corners. Flat joins between adjacent panes stay
flat. The Composer keeps its explicit 24-pixel radius in both desktop and phone
layouts; its overlapping context strip uses the shared 12-pixel radius.

`ChromeButton` is an explicitly named application layout helper. It selects stock
button variants and supplies role geometry and icon placement. It is not the base
Button contract. `ActionMenuPopover` supplies bounded application menu width and
scroll layout. `ActionMenuDialog` supplies a shared compact inset for action-menu
content; informational popovers retain vendor spacing. HeroUI owns menu and item
interaction. Action menus share a four-pixel outer inset, eight-pixel row inset
and gap, and 16-pixel leading icons. Captions align with the leading icon edge.
Checks and submenu chevrons retain their vendor scale. Menu widths fit content
within viewport bounds; the model picker retains its deliberate column width.
Simple menu rows share a 30-pixel minimum across desktop and phone layouts, with
the same text, icon and padding scale. Rows with descriptions grow with their content. Selection indicators keep
a dedicated leading slot; plain actions retain their icon slot at the same edge.

The Composer project picker uses one stock Dropdown menu with a single-selection
section and a separate full-width Create project action. The vendor owns keyboard
navigation, typeahead, dismissal and focus restoration. Project commands retain
draft staging and existing-conversation assignment through their owner API.

`ActionRow` combines a leading stock action and independent trailing controls.
The row paints shared hover and keyboard-focus feedback across the full group
width, including its padding. The enclosing group clips the outer corners;
switches and secondary actions remain separate interactive elements. View tabs fit their
title and close control, shrink when space is limited and keep the close control
inside the painted tab without nesting interactive elements.

Button icon geometry targets direct SVG children. Composed provider marks own their
inner logo and ring dimensions. Do not resize those parts through an ancestor button.

Keyboard focus uses the shared contrast-qualified coral text role. Meaningful
control indicators use their own neutral token rather than strengthening every
decorative border. Check text, selected states and hover against their actual
adjacent surfaces in both appearances; decorative depth alone is not a state cue.

`ResponsiveDialog` composes stock Modal on desktop and stock bottom Drawer on
phones. It owns presentation choice, safe-area space and bounded content scroll.
The exported Modal and Drawer remain unchanged. Their corners, fill, elevation,
focus and dismissal behavior belong to HeroUI. Keep an open presentation stable
when the viewport changes.

The Composer owns its overlapping context strip, inset alignment and text-driven
input growth. `ComposerPanel` and `ComposerTextArea` hold this local composition.
The generic TextArea remains a direct vendor export. Retain the approximately
100-pixel empty text surface and the same internal scale on phones. Avoid changes
to unrelated form fields when adjusting the Composer.

`ModelPicker` composes provider identities, account selection and a model list.
Its bounded columns and independent scrolling are product layout. Choosing a
model confirms account and model, then closes the picker. Add provider and Add
account open provider settings. `ReasoningEffort` maps consumer-owned level IDs to
a stock discrete Slider; it does not recreate slider appearance or assign a token
budget.

## Behavior and qualification

Keep provider authority, node binding, conversation ownership, persistence and
runtime capability contracts outside presentation. A visual migration must not
undo menu dismissal, independent side-chat records, draft preservation, directory
cancellation or session fencing. Node menus show configured connection state and
do not invent a live service. Project folder links do not grant filesystem access.

Dark is the initial appearance when no valid preference is stored. Keep System,
Light and Dark choices and preserve the user's saved selection. System resolves the vendor theme without
copying its palette. Preserve keyboard interaction, accessible names, browser zoom,
localization and reduced motion. Measure stock behavior before proposing a narrow
correction; do not retain a workaround for geometry introduced by our own styles.
Record genuine upstream limitations instead of silently reintroducing recipes.

The catalog uses the same direct exports and theme as consumers. Keep layout
fixtures and product compositions distinct from base component examples. Browser
viewport evidence does not qualify a physical Safari phone or a native renderer.
The native UI remains a separate provisional implementation. The website keeps
its marketing composition and light appearance while using the same base web
controls.
