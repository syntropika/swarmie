# Mobile dependency and feature matrix

The initial client uses Expo **57.0.30**, React **19.2.3** and React Native **0.86.3**.
This combination comes from the installed Expo SDK's `bundledNativeModules.json`, checked
against the [official SDK compatibility table](https://docs.expo.dev/versions/latest/).
The root Bun lockfile fixes exact resolutions. Reanimated **4.5.1** and Worklets **0.10.1**
are selected together from that same supported matrix. The native minimums are iOS 16.4
and Android API 24. Hermes is the SDK default; exported native bundles are Hermes bytecode.

## Installed packages

An installed capability may be intentionally dormant; its status is described below.
Versions show the locally resolved package version, rather than only the manifest range.
Workspace packages share this repository's source and are not published by this setup.

| Package | Resolved version | Responsibility / activation |
| --- | --- | --- |
| `@react-native-async-storage/async-storage` | `2.2.0` | Retained unresolved authored inputs; no secrets or transcript cache |
| `@swarmie/contracts` | `0.1.0` | Runtime-owned wire DTOs and method-selected response validators |
| `@swarmie/react-native` | `0.1.0` | Thin Swift/Kotlin/JNI bridge to canonical Rust client |
| `@swarmie/ui-native` | `0.0.0` | Shared native presentation wrappers; reversible HeroUI Native trial, independent of web UI |
| `@tanstack/react-query` | `5.104.2` | Server snapshots, journals, mutation lifecycle and invalidation |
| `expo` | `57.0.30` | SDK 57 foundation and native module autolinking |
| `expo-application` | `57.0.3` | Local app version in Settings |
| `expo-asset` | `57.0.20` | Bundled assets and Router resources |
| `expo-audio` | `57.0.5` | Deferred voice/playback; recording and background modes disabled |
| `expo-auth-session` | `57.0.16` | Deferred account authorization adapter; no issuer configured |
| `expo-build-properties` | `57.0.22` | Reusable minimum iOS target and Android TLS policy |
| `expo-constants` | `57.0.22` | Router and application configuration |
| `expo-crypto` | `57.0.4` | Local random idempotency keys |
| `expo-dev-client` | `57.0.20` | Local native development launcher |
| `expo-device` | `57.0.2` | Local OS metadata in Settings |
| `expo-document-picker` | `57.0.3` | Explicit user-selected image attachment |
| `expo-file-system` | `57.0.7` | Read selected cached image; remove only its app-owned temporary copy |
| `expo-font` | `57.0.4` | Font assets; native body uses platform font |
| `expo-image` | `57.0.5` | Deferred optimized attachment previews |
| `expo-image-manipulator` | `57.0.21` | Deferred image resizing/compression |
| `expo-image-picker` | `57.0.20` | Deferred system photo-picker adapter; camera/microphone/photo permissions disabled |
| `expo-linking` | `57.0.13` | Router deep-link handling |
| `expo-media-library` | `57.0.5` | Deferred media export; library permissions disabled |
| `expo-modules-core` | `57.0.23` | Thin Swarmie native module API |
| `expo-notifications` | `57.0.23` | Deferred functional notifications; no permission requests or push registration |
| `expo-router` | `57.0.28` | Typed routes, native navigation stacks and tabs |
| `expo-secure-store` | `57.0.4` | Runtime device credential and endpoint |
| `expo-splash-screen` | `57.0.10` | Native splash lifecycle |
| `expo-status-bar` | `57.0.1` | Status bar appearance |
| `expo-system-ui` | `57.0.4` | Native appearance support |
| `expo-updates` | `57.0.26` | Installed update capability; OTA disabled, no update endpoint |
| `expo-video` | `57.0.5` | Deferred playback; background and picture-in-picture disabled |
| `expo-web-browser` | `57.0.3` | Deferred browser authentication handoff |
| `heroui-native` | `1.0.10` | Transitive through shared native UI; button/input/label/text-field trial, with raw provider setup |
| `react` | `19.2.3` | Expo-supported React renderer |
| `react-dom` | `19.2.3` | Web development export only |
| `react-native` | `0.86.3` | Native renderer; Hermes and New Architecture |
| `react-native-gesture-handler` | `2.32.0` | Native gesture root |
| `react-native-keyboard-controller` | `1.21.9` | Composer keyboard behavior |
| `react-native-reanimated` | `4.5.1` | Native motion dependency for keyboard/navigation |
| `react-native-safe-area-context` | `5.7.0` | Insets and safe touch regions |
| `react-native-screens` | `4.26.2` | Native screen lifecycle |
| `react-native-svg` | `15.15.4` | Navigation icons |
| `react-native-web` | `0.21.4` | Browser layout development only; no browser transport in native app |
| `react-native-webview` | `13.16.1` | Deferred document/auth/computer adapter boundary; no WebView rendered |
| `react-native-worklets` | `0.10.1` | Reanimated-compatible worklet runtime |
| `uniwind` | `1.12.2` | Stable Tailwind 4 native compilation |
| `zod` | `4.6.7` | Configuration and extension schema validation |
| `@swarmie/config` | `0.0.0` | Shared strict native TypeScript preset and web/native semantic token stylesheets |
| `@types/react` | `19.2.18` | React type declarations aligned to the Expo matrix |
| `tailwindcss` | `4.3.3` | Shared root Tailwind 4 toolchain |
| `typescript` (root) | `7.0.2` | Shared root compiler; app does not install a second version |

Expo recommends TypeScript 6 for its template. This repository already pins TypeScript 7.0.2;
mobile consumes that root compiler and passes its strict checks. It does not add an app-local
TypeScript installation or change the other applications' compiler version.

## Compatibility and persistence choices

Expo's own [installation checks](https://docs.expo.dev/more/expo-cli/#install) select functional
module versions. The app uses pinned Bun 1.4.3, the root isolated linker configuration,
workspace, compiler, formatter, linter and lockfile;
it has no package-local lint/format policy or lockfile. Native project files are generated with
Expo prebuild. No custom configuration plugin is introduced: the Swarmie native module exists
because the current Rust transport boundary is a product requirement.

[Uniwind](https://docs.uniwind.dev/quickstart) supports the root Tailwind 4 toolchain directly.
This avoids adding a second styling compiler version or adopting a prerelease styling framework.
The generated native typing file lives in ignored Expo state; handwritten source uses shared policy.
The app imports native controls from `@swarmie/ui-native`. Shared token stylesheets are
exported by `@swarmie/config/tailwind`; mobile loads native tokens for account onboarding
and web tokens for existing runtime screens. The independent `@swarmie/ui-web` package
does not supply mobile dependencies. The two UI packages own separate compiler projects
and Storybooks; React Native Web and Uniwind catalog tooling stay in the native package.
The trial's vendor mappings live in `packages/config/tailwind/hero-native.css`.
`NativeProvider` adapts the raw vendor provider and reduced-motion policy; the app retains
gesture, safe-area, and keyboard roots. Six verification cells retain one accessible vendor
input for autofill rather than adopting segmented vendor input. The trial remains reversible
and does not implement identity, provider authorization, or native credential ceremonies.

[AsyncStorage](https://docs.expo.dev/versions/latest/sdk/async-storage/) is sufficient for the
small asynchronous unresolved-input journal. It stores authored text, an optional bounded image,
and an idempotency key. No mirrored server transcript is persisted.
[TanStack Query](https://tanstack.com/query/latest/docs/framework/react/overview) owns server state
in memory. [SecureStore](https://docs.expo.dev/versions/latest/sdk/securestore/) owns credentials;
no encryption key or credential is stored in AsyncStorage.

[MMKV](https://github.com/margelo/react-native-mmkv) was evaluated and deferred. Its current major
uses Nitro Modules and synchronous native storage. This slice has no measured synchronous read
bottleneck that warrants another native runtime or duplicate key-value store. If preferences later
need that profile, keep MMKV's responsibility distinct from Query, the outbox and SecureStore.

## Deferred or excluded integrations

| Capability | Decision |
| --- | --- |
| Calendar recurrence / IANA timezone rules | Requires runtime occurrence and DST semantics; current routines use absolute instants plus fixed elapsed intervals. |
| Durable task leases, recovery and result delivery | Runtime orchestration work remains; existing scheduled prompts and delivery receipts are not task completion. |
| Remote computer session | Type-only capability/adapter boundary; no route, no connection, no permissions and no display/input controls. |
| Audio calls, dictation and speech | Functional modules are dormant; no microphone activation. |
| Document/canvas surfaces and richer integrations | Capability adapters and usable content need separate product work. |
| Purchasing / in-app purchases | No SDK or product configuration. A future entitlement adapter belongs at the hosted contract boundary after requirements exist. |
| Analytics, advertising, attribution, replay and diagnostic uploads | Excluded; no SDKs or upload endpoints introduced in the mobile graph. |
| Hosted push and OTA services | No project IDs, push registration, update URLs or hosted build configuration. Updates are disabled. |

The dependency review includes installed Expo config plugins and Android manifest permission removals.
Core Expo tooling has its own development-service dependencies; CLI entry points set
`EXPO_NO_TELEMETRY=1`. Local development errors remain visible without adding an upload service.
This review does not claim to audit or remove pre-existing unrelated workspace tool dependencies.
