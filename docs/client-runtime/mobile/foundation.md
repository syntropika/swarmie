# Mobile client foundation

## Accepted product direction

The required mobile entry flow is Swarmie account sign-in, followed by AI provider selection,
then access to conversations. The initial provider choices are Kimi, Alibaba, OpenAI and
GitHub Copilot. This list defines the intended product scope; it does not establish that
provider authentication or execution integrations are implemented.

Welcome and Swarmie account access share an email-first native screen. Continuing uses a trusted
identity-service result to show either an existing account's available passkey/password methods
or account creation. Passkey sign-in is primary when supported for the account; its secondary
action switches to a password field. The password view offers a secondary passkey action only
when available. The device-unlock explanation appears only on the passkey view. Magic-link
sign-in is outside the current scope. First-party access does not hand off to a browser.

New accounts choose a password, verify email with a six-digit code, and may add a passkey or
skip that step. Password rules, account discovery, challenges, sessions and verification authority
belong to the identity service. Native credential ceremonies must use the operating system's
passkey interface. These requirements are separate from AI-provider OAuth integrations.

Individual conversations, streaming chat and image attachments remain in the active scope.
Group conversations have been removed from the mobile implementation and its runtime extension.
Routines remain set aside from the active product scope; the existing development implementation
is retained. Swarmie Cloud owns scheduling when routines return to the product; the phone does
not become the authoritative scheduler. Cloud implementation is deferred to a separate session.

The existing runtime URL and device-credential onboarding is an implementation of the earlier
development slice and requires redesign around Swarmie account identity. Secure storage and
connection restoration must follow the resulting account and provider credential contracts;
the existing connection flow is not the accepted end-user onboarding.

Provider access has two distinct modes. The primary mode connects the user's provider account
through that provider's OAuth flow to use the user's existing subscription. The alternative
mode is bring-your-own-key (BYOK), in which the user supplies an API key. Do not use BYOK as
an umbrella term for subscription-based OAuth access.

Each provider requires an individual authorization integration. Before enabling subscription
access, verify that the provider supports the required OAuth flow, scopes and subscription
entitlements for this integration. Successful account sign-in alone does not establish model
access or permission to use a subscription. API key availability must also be verified per
provider; a consumer subscription and API access must not be treated as interchangeable.
Exact provider capabilities and credential lifecycles remain integration prerequisites.

### Proposed shared desktop and mobile access

The proposed product direction applies the same Swarmie sign-in and provider-connection flow
to desktop and mobile. Provider account access through OAuth and subscription entitlements,
manual API key access, and harness selection are separate responsibilities.

The default onboarding should focus on the Swarmie account and supported provider access.
Harness-specific execution belongs at agent launch: an agent may be explicitly dedicated
to a selected harness. A selected provider does not implicitly choose a harness, and a
provider credential does not establish that an arbitrary harness can consume it. Each
execution adapter must honor its supported authentication and execution capabilities.

Additional access exceptions and the exact meaning of a dedicated harness remain open.
This proposal does not remove existing desktop harness controls or implement a replacement
execution engine. Those changes require a separate implementation scope.

### Provider connection presentation

Manual API key entry is a secondary path where supported. Present it as a visually subdued,
accessible action such as "Use an API key" under additional connection options, rather than
as the primary onboarding form. Keep the field concealed until the user chooses that path;
do not conceal the action from accessibility navigation or make it depend on a secret gesture.

Swarmie sign-in and provider authorization are separate steps. A selected provider or an
authenticated Swarmie account is not evidence of an authorized, usable provider connection.
Access supplied or billed by Swarmie is outside the agreed provider access model.

The selected onboarding design combines sparse, focused screens with continuous transitions
between steps. Onboarding is monochromatic: white, black and neutral grays, with no colored
accents. Use the original monochrome logo and mark from `assets/brandkit`, including neutral
buttons, links, focus rings and selected states. Keep the primary action stable during
transitions, provide reduced-motion behavior, and reveal manual
API key entry only after an explicit secondary action. Native account presentation now lives
in `apps/mobile/src/account`. It uses the separate `@swarmie/ui-native` primitives
and brandkit neutral tokens, with no surrounding gray card. Content occupies header, form
and bottom action zones; secondary actions are light-gray filled pills. The canonical mono
brandkit logo is bundled without altering the shared web mark.

The shared native controls now wrap HeroUI Native 1.0.10 as a reversible library trial.
Buttons, fields, and labels retain the approved neutral presentation through config-owned
semantic mappings. Verification keeps six visual cells over one accessible vendor input
for numeric entry and one-time-code autofill. The library trial does not configure identity.

The account presentation port is intentionally unconfigured for live identity. It rejects access
rather than guessing account existence or granting a session. No hosted transport or native
passkey ceremony is implemented by this UI change. The development-only `/onboarding-preview`
route (also enabled explicitly by `EXPO_PUBLIC_AUTH_PREVIEW=1` for an isolated review build)
uses labeled in-memory fixtures, sends no email and never grants runtime access. Existing
connected-runtime behavior is retained; the manual connection screen remains accessible in
development. Provider onboarding still requires its own integration.

Account mutations are never retried automatically. Navigating or changing email cancels the
active request and invalidates stale results; duplicate submissions are guarded synchronously.
Credential-bearing tasks release their captured inputs on execution, and mutation caches hold
neither authentication results nor challenge handles. Passwords and codes are not persisted.

The implementation inventory below describes the current code, including the earlier routine
experiment. It does not mean that the accepted onboarding flow is implemented or that deferred
features belong in the next mobile release.

## Current implementation

Swarmie's mobile app lives in `apps/mobile` and targets iOS and Android using Expo SDK 57,
React Native 0.86, React 19.2, TypeScript and Hermes. It connects to an independently running
runtime through HTTPS and an existing device credential. This slice does not enroll accounts,
allocate hosts, publish updates, configure store products or activate hosted services.

## Implemented slice

- Persistent runtime connection with on-device SecureStore credentials and runtime-scoped Query caches.
- List, create and explicitly resume supported individual agent sessions; stream their ordered journals through the shared Rust SDK.
- Send text and one PNG, JPEG or WebP attachment up to 1 MiB when the agent advertises image support.
- Keep an unresolved send and its original idempotency key in AsyncStorage before dispatch.
  Check its retained runtime receipt before another send. No automatic mutation retries.
- Show agent activity, stop a response when cancellation is supported, and answer permission requests
  and single-choice/free-text human input. Multiple-choice requests remain visible and direct the user
  to another connected client. This slice does not auto-answer approvals.
- Create one-shot or fixed-interval scheduled prompts, pause/resume future deliveries, cancel future
  occurrences while retaining history, and inspect the latest 64 delivery attempts.

These interactions call real runtime methods and extension commands. No synthetic chat, agent,
completion, billing or routine data is displayed as integrated functionality.

## Repository boundaries

The app composes routes, capability controllers and native presentation primitives. Expo Router
owns typed routes, stacks and tabs. Safe Area Context, Gesture Handler, Screens, Keyboard Controller,
Reanimated and Worklets provide native interaction foundations. Swarmie's existing DOM components
cannot render on React Native. Shared native controls live in `packages/ui/native`, exported
as `@swarmie/ui-native`, with their own dependencies, compiler project and Storybook.
The web library lives independently in `packages/ui/web` (`@swarmie/ui-web`) and does not
depend on React Native or Uniwind. Shared tokens belong to `packages/config/tailwind`.
Mobile loads `@swarmie/config/tailwind/native-tokens.css`, mapped to the brandkit neutral
palette, alongside `@swarmie/config/tailwind/web-tokens.css` for existing runtime screens.
Uniwind compiles those Tailwind 4 tokens without introducing a Tailwind 3 toolchain.
The current appearance follows Swarmie's light theme; dark appearance qualification is pending.

`NativeProvider` contains the raw HeroUI provider and disables vendor animations for reduced
motion. The app owns gesture, safe-area, and keyboard roots. The native stylesheet imports
the config's `hero-native.css` vendor mappings and emits utility overrides after vendor
component rules so the same overrides work in React Native Web. Vendor choice remains
provisional; adapting a library does not move capability state or transport into shared UI.

`@swarmie/react-native` in `sdk/react-native` is a thin Expo module over `sdk/wit/client.wit` and its
existing C ABI. Swift and Kotlin/JNI marshal UTF-8 commands, move blocking operations off the UI
thread, and release returned native strings. Android explicitly converts between Java strings and
UTF-8 so non-ASCII text survives the JNI boundary. Rust in `crates/client` owns HTTP, RPC envelopes,
deadlines, cancellation, event cursors and handle lifetime. Mobile does not add a second JS networking
engine or use the browser-only HTTP export. Runtime result schemas validate data before it enters
Query caches; Zod validates device configuration and extension payloads.

The existing resources extension owns schedules. SQLite `extension_state` persists them
next to canonical sessions, event journals and submission receipts. It does not create another
scheduler database, service dependency or mobile foreground clock. Hosted identity, multi-user
collaboration authorization, relay routing, billing and enrollment remain in the independent cloud
repository.

## Local development

Run commands from the parent repository root with its pinned tools, Bun 1.4.3, root isolated
linker configuration, and single Bun lockfile:

```sh
just setup
just mobile-check
just resources-check
just mobile-export
```

A native development client is required. Expo Go cannot supply the Swarmie Rust binding. For Android,
install Android Studio's SDK/NDK and a supported Java toolchain, set `ANDROID_NDK_HOME`, then run:

```sh
just sdk-build-mobile android
just mobile-prebuild android
just mobile-run android
```

The Android SDK staging recipe builds arm64-v8a and x86_64 libraries using NDK API 24 and the pinned
Rust toolchain. The Expo module restricts its ABI list to the same architectures. Generated Android
and iOS projects, native SDK binaries, Metro state and exported bundles are ignored build outputs.

Run iOS operations on a configured macOS host with Xcode, its simulators and CocoaPods available:

```sh
just sdk-build-mobile ios
just mobile-prebuild ios
just mobile-run ios
```

The run recipes start Metro alongside the native client. For later sessions with a development
client already installed, use `just dev-mobile`.

The iOS recipe reuses the Swift SDK's XCFramework builder and stages the result for Expo autolinking.
It requires disk space for dependencies and native build outputs. These are local build commands;
there is no EAS project, cloud build, store submission, signing-account change or deployment step.

Enter an HTTPS runtime endpoint and its device credential in the app. The endpoint must be reachable
from the device and use a trusted TLS certificate. Plain HTTP, URL-embedded credentials, query
credentials and fragments are rejected. A runtime administrator must provision the credential and
TLS access independently; mobile does not weaken loopback listeners or configure a public relay.
Install a runtime containing the updated resources extension to enable routine controls.

## State, failure and privacy

SecureStore owns the endpoint and credential together. Neither secrets nor endpoint URLs enter Query
keys, application logs or source. Query stores runtime snapshots and journals in memory and clears
on connection changes. AsyncStorage owns unresolved authored sends, including the stable submission
key and optional image; it is not a transcript database or secret store. Retained drafts remain scoped
to their runtime store and session after disconnect. The app reports storage failures instead of
sending without first saving the submission key. Corrupt local entries require recovery before send.

Mutations do not retry automatically. An accepted receipt means native delivery, not verified work.
Uncertain delivery requires receipt reconciliation and possibly inspection from another client.
Backgrounding cancels foreground stream ownership; foregrounding reopens the native cursor from
retained events. Stream failures trigger a journal refresh. A disconnected phone cannot stop the
runtime's independent scheduler. Cancelling a routine does not retract an already queued prompt;
stop active agent work through its advertised session cancellation control.

Expo CLI entry points disable telemetry with `EXPO_NO_TELEMETRY=1`. OTA updates are disabled and no
update URL or EAS project is configured. Functional notification modules are installed, but this slice
does not request notification permissions, register push tokens or contact the Expo Push Service.
Audio recording, camera, photo-library and remote-computer access are not requested at launch.
The image action uses the system document picker. No analytics, attribution, advertising, replay,
remote diagnostic uploader or purchasing SDK is added to the mobile dependency graph.

## Qualification and phased work

For the current library trial, `bun run check` passes formatting, TypeScript, lint, and
the complete root test suite. `just mobile-check`, Expo dependency validation, all-platform
Expo export, the desktop web build, and both independent Storybook builds also pass.
Native Storybook interaction-play qualification remains pending.

Browser-preview fidelity received an independent ship disposition for six account states
at 390 × 844, narrow-phone top and bottom views at 320 × 568, and a tablet view at
768 × 1024. The narrow screen intentionally scrolls vertically. Measured fields retain
60px height, 20px text, 24px left padding, and no border; the original monochrome logo,
white canvas, and pill controls match the approved design. Browser fixture checks cover
password and passkey sign-in, account creation, verification, code resend, and skipping
passkey enrollment. These results qualify browser-preview fidelity only. They do not
establish physical-device behavior, native accessibility, credential ceremonies, or live
account/passkey support.

Automated checks cover schema boundaries, journal replay/deduplication, timezone offsets, unresolved
input recovery, native-operation cancellation and ownership, routine restart persistence,
delivery history/pause/resume, and native module discovery on both platforms. Metro export verifies
iOS and Android Hermes bytecode generation; Android prebuild verifies configuration and autolinking
discovery. A bundle or prebuild does not
establish that Swift/Kotlin/JNI links or runs. Native builds, simulator/device gestures, keyboard
insets, attachment rendering, background/foreground recovery and accessibility still need native
qualification on provisioned build hosts. A physical-device end-to-end conversation against an
installed production harness has not yet been qualified.

Next phases are Swarmie sign-in, provider subscription OAuth and BYOK access; durable task/attempt
orchestration; calendar/timezone recurrences and richer recovery; multi-choice input; attachment
previews and other document formats; OS push notifications; and native accessibility/dark appearance
qualification. Voice, dictation, speech, audio/video playback,
documents/canvas, remote-computer adapters and purchases remain deferred. Their installed functional
modules do not imply that these experiences are active. See `orchestration.md` for the enforced
boundaries and missing runtime capabilities.
