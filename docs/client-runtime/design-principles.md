# Swarmie design principles

These principles guide client interfaces, shared components, and design reviews. Read them
with [frontend engineering rules](frontend.md) and the relevant platform design document.
They describe accepted guidance; they do not certify that every existing screen already
meets it.

The approach is informed by [HeroUI's design principles](https://heroui.com/en/docs/react/getting-started/design-principles.mdx),
particularly semantic intent, composition, accessibility, and predictable typed contracts.
The rules below adapt those ideas to Swarmie's actual platform and runtime boundaries.
HeroUI Native is wired through the shared native wrappers as a reversible trial; permanent
vendor selection remains provisional.
The reference describes library goals, not guarantees about Swarmie's implementation.

## Make the next action clear

Name control variants for their responsibility, such as primary or secondary. Keep the
main action easy to locate and alternatives understandable in context. Reveal optional
configuration when the user needs it, without hiding essential choices from accessibility
navigation. Labels should explain the user's action; internal providers, transports, and
implementation details belong outside product copy unless they affect that decision.

## Share meaning, respect the platform

`packages/config` owns compiler settings, fonts, Tailwind foundations, and semantic tokens.
`packages/ui/web` and `packages/ui/native` own their respective rendering contracts,
components, and stories. Use `@swarmie/ui-web` for DOM interfaces and `@swarmie/ui-native`
for native ones. Reusing intent and token ownership does not require identical markup,
renderer dependencies, or interaction mechanics. Keep native ambient types out of web
compiler projects. Mobile currently loads both token palettes because existing runtime
screens use web colors while onboarding uses native neutrals.

## Compose controls around clear ownership

Build neutral primitives that accept content, native props, and refs. Compose related
parts through children, slots, or focused shared state when that state actually exists.
Avoid interfaces that accumulate unrelated boolean modes. Capability controllers own
navigation, requests, cancellation, and account state; primitives render supplied values
and callbacks. Keep discriminated workflow states and exhaustive handling at the boundary
that owns the operation. Do not turn a shared button or field into an authentication client.

## Preserve the approved design in implementation

The approved onboarding uses the canonical monochrome brandkit logo, a white canvas,
neutral ink, gray secondary pills, and distributed header, form, and footer zones. Its
fields have a 60-unit minimum height, 24-unit left padding, and 20-unit labels and values
at the base native scale. Labels use medium weight; fields remain borderless on focus,
with subtle neutral fill feedback. Match these decisions through shared tokens and
controls. A vendor's defaults do not replace the accepted prototype. The native treatment
does not authorize changing the incumbent web design.

## Treat accessibility as interaction behavior

On the web, preserve semantic roles, keyboard operation, visible focus, dismissal, and
focus restoration. On native platforms, preserve accessible names and announcements,
dynamic type, usable touch areas, safe areas, and keyboard reachability. Keep labels
visible and allow long text to wrap. Icons need accessible actions rather than unexplained
symbols; decorative icons should not duplicate announcements. Communicate errors through
text and behavior, not color alone. Verify these properties on the surface that ships.

## Design the entire operation

Specify pending, disabled, error, empty, and recovery behavior alongside the successful
state. Prevent duplicate submissions, discard stale results, and preserve user-owned work.
Do not retry one-use authentication operations automatically. Account discovery must use
a trusted result; client presentation must not guess account existence or mint sessions.
Native credential ceremonies belong behind adapters using the operating system interface.
Hosted identity and passkey ceremonies remain pending; preview fixtures do not establish
live authentication or provider access.

## Use motion to support continuity

Motion should explain a transition or preserve orientation without delaying the next
action. Keep the main onboarding action stable between steps. Honor reduced motion and
avoid making animation necessary to understand state. Describe intended behavior before
adding effects; installed animation dependencies are not evidence that a transition is
implemented or qualified.

## Verify platforms independently and keep vendors replaceable

Maintain separate dependency graphs, TypeScript projects, and Storybooks for web and
native UI. Root checks and catalog builds cover both, but a browser-rendered native story
does not qualify device keyboards, gestures, accessibility, or credential ceremonies.
Record actual checks and remaining native qualification honestly.

A library trial should exercise representative controls against the accepted design and
interaction contracts. Keep vendor adaptation inside its owning platform package. When
the evaluation ends, choose the implementation and remove superseded paths rather than
maintaining duplicate control systems indefinitely. Vendor selection remains provisional
until that decision is made.
