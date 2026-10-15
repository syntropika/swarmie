# Shared frontend engineering rules

These rules apply to TypeScript, React, shared UI, and frontend tooling in `swarmie`.
Run development commands from this repository's root. Package-specific compiler projects
and build inputs stay beside their consumers.

Read [Swarmie design principles](design-principles.md) before changing frontend interfaces
or shared controls. Apply that guidance alongside the engineering rules below.

For the native UI trial, read and use the repository-local
[HeroUI Native skill](../../.agents/skills/heroui-native/SKILL.md). Future HeroUI web work
uses the separate [HeroUI React skill](../../.agents/skills/heroui-react/SKILL.md).
Keep each skill scoped to its platform. The native choice remains provisional, and the
existing web implementation is not being migrated by this trial.

## Root configuration and package ownership

The root owns `oxlint.config.ts`, `oxfmt.config.ts`, `bun.lock`, shared TypeScript presets
in `packages/config/ts`, `mise.toml`, `justfile`, `rustfmt.toml`, and
`cliff.toml`. Do not introduce another formatter, linter, toolchain configuration, or
copy of a shared preset inside an application or package.

Use the pinned Bun 1.4.3 with the root `bunfig.toml` isolated linker and
`bun install --frozen-lockfile`. Shared compiler settings, fonts, Tailwind setup, and
theme tokens belong in `packages/config`. Neutral components, presentation contracts,
and stories belong in the separate `packages/ui/web` (`@swarmie/ui-web`) and
`packages/ui/native` (`@swarmie/ui-native`) workspace packages. Their dependency graphs,
compiler projects, and Storybooks remain independent. Runtime wire contracts remain in
`packages/contracts`. Language SDKs and their generation
and packaging tools belong in `sdk/`; the executable npm runtime package belongs in
`packages/runtime`. The TypeScript SDK joins the root Bun workspace from `sdk/typescript`.

This repository owns all TypeScript source: desktop, mobile, the product website in
`apps/web`, the public documentation site in `apps/docs`, browser account presentation
in `packages/auth`, and the account HTTP client in `packages/api`. All consume the root
Bun workspace, lockfile, compiler references, and lint/format policy. Each application
has one `src` tree; framework routes live in `src/app` for mobile and documentation.
Desktop React code, its isolated demo, and native shell entries share `apps/desktop/src`.

`swarmie-cloud` is an independent Rust backend repository with its own toolchain,
service tests, deployment configuration, and release lifecycle. It contains no TypeScript
workspace, frontend copies, or frontend lint/format configuration. Neither source repository
needs the other's checkout to install, verify, or build. Keep backend authority, session
verification, billing, and commercial entitlements in hosted services. The TypeScript
account client and account presentation do not implement those server responsibilities.
The public runtime and its SDK remain independent of account UI and hosted credentials.

## Project-local imports

Use `~/` for application source imports, resolved from the owning project's root:

| Project | Root-relative examples |
| --- | --- |
| Mobile (`apps/mobile`) | `~/src`, `~/assets` |
| Desktop (`apps/desktop`) | `~/src`, `~/src/demo` |
| Documentation site (`apps/docs`) | `~/src/app`, `~/src/components`, `~/src/lib` |
| Website (`apps/web`) | `~/src` |
| Account presentation (`packages/auth`) | `~/src` |

There is no monorepo-wide `~` root. Vite 8 uses `resolve.tsconfigPaths: true` to resolve
each importer against its owning compiler project; Expo and Next consume their project's
TypeScript paths. The desktop compiler includes both main and demo source, and its component
generator configuration follows the same aliases. Keep public `@swarmie/*` package imports.
Node configuration and runtime filesystem references retain appropriate relative paths,
including Expo's configuration import of `locales.json`. Do not blanket-rewrite vendored
internal imports. Backend changes stay in their independent repository; builds require
neither another source checkout nor documentation contents.

## TypeScript and lint policy

Shared packages use strict TypeScript with checked indexed access, exact
optional property contracts, explicit type-only imports, and the shared compiler presets.
The desktop and SDK retain their existing strict compiler projects during this relocation;
applying additional compiler flags requires updating their consumers together. Preserve each consumer's
module target, environment types, emission settings, and source inclusion boundaries.
Validate untrusted data at its owning boundary before updating application state.
Prefer precise types and discriminated unions over `any`, chained assertions, widening
then asserting, or optional access used to conceal an invalid state.

The root Oxlint policy extends Ultracite core, React, and TanStack rules and includes
anti-slop, shared UI, and Tailwind enforcement. Type-aware rules reject floating promises,
unsafe operations, invalid asynchronous contracts, and non-exhaustive union handling.
Do not silence a rejected promise with `void`. Handle its failure at the boundary that
owns the operation. Ban `ts-ignore` and `ts-nocheck`; `ts-expect-error` needs a meaningful
description. Type assertions need a safety explanation and must not replace validation.
Do not mock modules as a substitute for adapters with explicit contracts.

Function declarations and arrow functions are both valid. Preserve intentional object
insertion order. Keep rule exceptions narrow, tied to a concrete contract, and documented
in English. JavaScript tooling has no checked TypeScript contract; native JavaScript rules
still apply. Tests can implement synchronous fixture adapters without fake awaits.
Do not disable shared rules to make new violations disappear.

## Capability ownership and application workflows

Organize code by the capability that owns it. Routes compose capability entries;
presentation components receive data and callbacks; hooks or controllers own behavior.
Avoid global collections of unrelated hooks or views and generic `domain` or `infra`
directories. Keep provider SDKs behind application-owned adapters.

Use Effect where an application workflow needs typed failures, external validation,
and adapter coordination. Adapt promise dependencies once at their I/O boundary.
Use TanStack Query for reads and mutations with pending, error, success, caching,
or invalidation state. Do not mirror that lifecycle in separate `useState` variables.
Local state is appropriate for editable input, open panels, and presentation choices.
Pure formatting and static JSX do not require Effect wrappers.

Model multi-step workflows with discriminated unions and exhaustive matching. Define
cancellation, stale-result handling, retries, and duplicate-submission guards at the
workflow boundary. One-use authentication proofs and subscription submissions have no
automatic retry. Isolate Query clients per server request and clear account-scoped
caches when identity changes. Use `type-fest` only when an existing type makes an
invariant clearer; keep the import type-only.

## Shared UI and composition

Inspect the relevant `@swarmie/ui-web` or `@swarmie/ui-native` exports and stories before
adding a control or repeated pattern.
Reuse a primitive, extend its coherent contract, or add a neutral shared primitive when
actual consumers establish the responsibility. Avoid duplicated fields, selectors,
notices, and buttons across frontends.

Native clients use `@swarmie/ui-native`. Keep native and DOM dependencies and compiler
projects separate so platform ambient types do not change web contracts. The native
package owns its React Native Web and Uniwind Storybook tooling; the web package has no
native renderer or styling dependencies. Each package has its own Storybook and checks
its actual platform props. The mobile entry owns its Uniwind stylesheet and must include
`packages/ui/native/src` in Tailwind scanning.

Shared token stylesheets live in `packages/config/tailwind`, exported as
`@swarmie/config/tailwind/web-tokens.css`, `native-tokens.css`, and `tooling-tokens.css`.
The tooling stylesheet combines the two token sets for shared tooling. Keep the existing
`base.css` foundation in that package; do not create another token package. Mobile currently
loads both palettes because its existing runtime screens use web semantic colors while
account onboarding uses the monochrome native tokens.

The reversible HeroUI Native 1.0.10 trial stays behind `@swarmie/ui-native` wrappers for
buttons, inputs, labels, and text fields. `NativeProvider` wraps the raw vendor provider
and disables its animations when reduced motion is requested; applications own gesture,
safe-area, and keyboard roots. `@swarmie/config/tailwind/hero-native.css` maps vendor
semantics to existing native neutrals. The native stylesheet emits utilities after the
vendor's unlayered component rules so wrapper overrides also apply in React Native Web.
Keep this adaptation replaceable and preserve the existing web implementation.

Use Atomic Design as a practical vocabulary: atoms are neutral controls, molecules
compose presentation or accessibility contracts, and domain organisms and pages remain
with their capability. Do not create empty taxonomy levels. Compose using children,
explicit variants, named parts, slots, and context only when parts actually share state.
Avoid components with unrelated boolean modes. Native props such as `disabled` and
focused lifecycle props such as `loading` remain appropriate.

Pass refs and native props through primitives. Preserve semantics, accessible labels,
descriptions, keyboard behavior, and focus restoration. Keep labels, translations,
routing, transport, and domain actions with the consumer. Shared UI must not import
hosted capabilities, TanStack routing or Query state, or API transports. Add meaningful
Storybook examples when a shared contract introduces interaction or accessibility
behavior. A static Storybook build does not execute play functions.

## Styling and accessibility

Use Tailwind for component layout, typography, colors, responsiveness, focus, and motion.
Shared theme tokens, fonts, keyframes, and unsupported utility behavior belong in owned
stylesheets. Frontend entries may load their own stylesheet; the desktop terminal may
load the terminal renderer's required vendor stylesheet. Ordinary component CSS and
CSS-in-JS are prohibited. Explain necessary exceptions near their owner.

Use semantic theme utilities and shared UI contracts. Do not add inline styles, `css`
props, style elements, independent palettes, raw colors, conflicting utility classes,
or concatenated class fragments. Arbitrary values must follow the narrow root policy.
Keep token definitions consistent with their documented meaning. Native SVG geometry
attributes remain appropriate; decorative art needs correct accessibility treatment.

Keep labels persistent, focus visible, feedback announced, and controls keyboard
accessible. Color alone must not express state. Honor reduced motion. Check overflow,
long labels, touch targets, and control sizing at narrow widths when changing an interface.
Keep implementation and provider terminology out of product copy unless it helps users
decide what to do.

## Localization and telemetry

When a frontend supports localization, keep copy in typed catalogs owned by its consumer,
including accessible names,
metadata, placeholders, validation, pending messages, and errors. English defines the
key and interpolation contract. Update every supported catalog together and
verify key and placeholder coverage. Preserve canonical language tags. Use locale-aware
number and date formatting; do not concatenate translated sentence fragments.

Resolve locale deterministically with English fallback. Keep initial server HTML and
hydrated client locale consistent. Browser storage failure must not prevent switching
languages. Preserve user-generated names and other user content. Comments, identifiers,
documentation, and translation keys are English; explicitly supported catalog values
may use their target language.

Mobile language metadata lives in `apps/mobile/locales.json`: `en`, `es`, `fr`, `de`,
`pt`, `it`, `ja`, and `zh-CN`, matching the supported web languages. Its platform-owned
typed catalogs cover UI copy, errors, notices, and accessible names. Locale-aware dates
and numbers come from the locale provider. Keep domain feedback as message keys so an
already-visible error changes language without losing editable input. Shared primitives
receive localized labels from consumers; `NativeCodeField` requires its accessible label.
OS detection, preference persistence, script matching, and failure behavior are described
in [the mobile foundation](mobile/foundation.md#localization).

Keep instrumentation at transport, workflow, routing, and error boundaries. Use bounded
operation names, route templates, timing, status, and safe error types. Do not put
credentials, proofs, raw provider responses, sensitive URLs, request bodies, or arbitrary
exception text into Query keys, caches, errors shown to users, or telemetry. Export remains
optional and must not fail the user's operation. Separate public frontend configuration
from server secrets.

## Formatting and verification

Oxfmt owns formatting: two spaces, a 100-column target, trailing commas, import ordering,
and Tailwind class ordering against the shared stylesheet. Format edited code with the
root configuration. Do not leave compressed JSX or divergent manual formatting.

Run `bun run format:check`, `bun run typecheck`, `bun run lint`, and affected tests from
the root. `bun run check` combines formatting, types, lint, and tests. Package
scripts delegate shared policy to this root. Package moves require affected builds,
including both shared UI Storybooks and the desktop web build for client consumers. Report existing violations accurately; do not
claim clean checks when the inherited policy exposes them.

Run meaningful behavior tests for invalid external data, failure handling, cancellation,
authentication, and accessibility. Avoid tests that merely mirror trivial implementation
details. Inspect responsive and keyboard behavior for interface changes. Keep temporary
reports, screenshots, traces, and browser profiles outside the repositories.

## Continuous integration

The root shared-UI workflow installs the frozen dependency graph, verifies shared
formatting and lint, checks TypeScript projects, runs frontend and SDK behavior tests,
and compiles the application frontends and both Storybooks. The website, account client,
account presentation, and documentation site share that root install; there is no separate
documentation dependency lock or npm install step.
Root type checks include both UI compiler projects;
`bun run storybook` starts the web catalog and `bun run storybook:native` starts the native
catalog. `bun run build:storybook` builds both. This does not initialize another source repository or deploy a
hosted service. Runtime release jobs also install dependencies from the root with Bun.
The hosted repository owns its backend CI and publishing credentials. Frontend relocation
does not restore the retired hosted frontend deployment workflows or transfer secrets.
