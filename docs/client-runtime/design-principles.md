# Client design principles

These rules preserve the accepted interface. Apply them with the
[frontend engineering rules](frontend.md).

## Shared appearance

Use thin chrome, clear dividing lines, compact icons, and restrained salmon accents.
Keep colors, typography, menu geometry, and spacing in shared controls and tokens.
Maintain the same design intent across surfaces. Do not share DOM controls with native
renderers.

Support system, light, and dark appearance. Preserve reduced motion and browser zoom.
Text fields and drag handles avoid outlines. Use fill or caret feedback while
keeping keyboard focus visible. Restore focus when a menu or dialog closes.

## Conversation and layout

The conversation is the initial surface. Center the transcript and Composer in the same
720px maximum width. Place the project and conversation title inline in the header.
The execution-node indicator describes current work. The Composer selects the requested
destination.

Keep navigation and content splitters within their bounds. Panel movement and resizing
must preserve the draft and reading position. Virtualize long histories and retain reading
anchors. Keep message actions visible on hover or keyboard focus without shifting content.

Views owns tabs. Expanding Views makes the conversation available as a tab. Closing the
final view restores the conversation. Settings occupies a complete application section.
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
