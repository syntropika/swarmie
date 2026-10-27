# Client design principles

These rules preserve the accepted interface. Apply them with the
[frontend engineering rules](frontend.md).

## Shared appearance

Use thin chrome, clear dividing lines, compact icons, and restrained salmon accents.
Keep colors, typography, menu geometry, and spacing in shared controls and tokens.
Maintain the same design intent across surfaces. Do not share DOM controls with native
renderers.

Support system, light, and dark appearance. Preserve reduced motion and browser zoom.
The shared light theme has a white canvas, gray panels and white fields. Fields use
a visible border without a shadow. Input, SearchField, Select and TextArea use the
same field tokens. The dark theme uses its separate surface and field colors.
In the light theme, the projects and chats column uses a gray surface.
The selected conversation uses a separate gray and a heavier font.
Its control exposes the current-page state.
The icon rail uses a lighter gray. The desktop rail is 45 pixels wide.
Desktop rail buttons are 35 pixels square, with
18 pixel icons and 15 pixel gaps. Phone rail buttons keep a 40 pixel touch target.
The desktop header is 45 pixels high. Its icon action buttons are 28 pixels square.
These header sizes do not change controls in the content area.
Category tabs use `Tabs.ListContainer` with `appearance="filters"`. Each selected
or hovered tab has its own gray surface; the group has no filled background.
Standard text fields use a stronger border on focus. They also use an outline in
forced-color mode. The Composer has one outer border around its input and controls.
Drag handles use fill feedback. Restore focus when a menu or dialog closes.

Shared tokens own each control role and its text size. Application code selects a role
from the public component. It does not set a new size for that role. Navigation headings
use the navigation text role. Secondary navigation text uses the caption role.
Conversation messages and the Composer use the conversation text role. The shared class merger keeps text size
and foreground color independent.

Drawer layouts below 768 pixels and coarse pointers use the touch density.
Menu and navigation rows use the shared row heights. Controls inside a row fit within
its height. Composer labels truncate when necessary. Their full accessible names remain.
Shared outline icons use one stroke token.

## Conversation and layout

The conversation is the initial surface. Center the transcript and Composer in the same
720px maximum width. Place the project and conversation title inline in the header.
The execution-node indicator describes current work. The Composer selects the requested
destination.

Keep navigation and content splitters within their bounds. Panel movement and resizing
must preserve the draft and reading position. Virtualize long histories and retain reading
anchors. Keep message actions visible on hover or keyboard focus without shifting content.

Views owns tabs. The public Tabs component provides selection and keyboard movement.
Only the selected tab is in the tab sequence. Each close button is separate from its tab.
The selected tab's close button is also in the tab sequence. Closing a tab restores focus
to the selected tab if focus stays in the list or moves to the page body. Expanding Views
makes the conversation available as a tab. Closing the final View restores the conversation.
If closure removes the focused View control, focus returns to the visible Conversation input.
Focus in another control is preserved. Settings occupies a complete application section.
On phones, navigation and its icon rail open together as an overlay. When Conversation
and View both exist, provide a compact bottom switch between them.

## Plugins and feedback

Each plugin exposes configuration, About information, and declared uses. Its switch
controls activation; the adjacent gear opens configuration. Optional plugins contribute
through the contract of the screen or control they extend.

Use skeletons for pending content and progressive reconnect feedback for interrupted
access. Give committed messages and transient execution information stable identities.
The development inspector shows live functional state and errors through subscriptions.
It must not expose credentials or authorization callbacks.
