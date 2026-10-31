set shell := ["bash", "-euo", "pipefail", "-c"]
set positional-arguments

rust_version := `sed -n 's/^rust = "\([^"]*\)"$/\1/p' mise.toml`

default:
    @just --list

# Install shared tools and dependencies from the repository root.
setup:
    mise install
    mise exec -- bun install --frozen-lockfile

dev:
    bun run dev:web

storybook:
    bun run storybook

fmt:
    bun run format

check:
    bun run check
    just foundation-check

test:
    bun run test

build:
    bun run build:web

build-storybook:
    bun run build:storybook

changelog:
    git-cliff --config cliff.toml

# Mobile UI development and static export do not qualify native runtime support.
dev-mobile:
    EXPO_NO_TELEMETRY=1 bun run dev:mobile

mobile-export:
    EXPO_NO_TELEMETRY=1 bun run --cwd apps/mobile-client export

mobile-check:
    bun run check:mobile

mobile-config:
    EXPO_NO_TELEMETRY=1 bun run --cwd apps/mobile-client config:public

mobile-runtime platform:
    EXPO_NO_TELEMETRY=1 bun run --cwd apps/mobile-client expo-updates runtimeversion:resolve --platform {{platform}}

# Signing maintenance remains independent of the suspended native SDK build.
mobile-signing operation="inspect" variant="development" profile="appstore":
    bundle exec fastlane ios signing operation:{{ quote(operation) }} variant:{{ quote(variant) }} profile:{{ quote(profile) }}

mobile-signing-check:
    bundle exec ruby -c fastlane/Fastfile
    bundle exec ruby -c fastlane/Matchfile
    bundle exec ruby scripts/distribution/test_apple_signing.rb
    bundle exec ruby scripts/distribution/test_signing_identity.rb
    bundle exec ruby scripts/distribution/test_apple_signing_storage.rb
    bundle exec ruby scripts/distribution/test_apple_signing_cli.rb

tembra-plugins:
    bun run plugins:build

tembra-browser:
    bun scripts/tembra/browser.ts

rust-lock:
    cargo +{{rust_version}} generate-lockfile

rust-fmt:
    cargo +{{rust_version}} fmt --all

rust-fmt-check:
    cargo +{{rust_version}} fmt --all --check

rust-clippy *arguments:
    cargo +{{rust_version}} clippy --locked --workspace --all-targets "$@" -- -D warnings

rust-test *arguments:
    cargo +{{rust_version}} test --locked "$@"

rust-clean *arguments:
    cargo +{{rust_version}} clean "$@"

rust-run *arguments:
    cargo +{{rust_version}} run --locked "$@"

rust-doc:
    RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -D rustdoc::broken_intra_doc_links" cargo +{{rust_version}} doc --locked --workspace --no-deps

# Run typed source-policy checks with the same pinned toolchain as the workspace.
rust-xtask *arguments:
    cargo +{{rust_version}} run --locked --package xtask -- "$@"

rust-source-check:
    just rust-xtask check

rust-check: rust-fmt-check rust-source-check rust-clippy
    cargo +{{rust_version}} test --locked --workspace
    just rust-doc

# Build and execute an independent orb through the published SDK and native engine.
tembra-authoring-test: tembra-plugins
    bun test tests/fixtures/tembra-sdk/sdk.test.ts

foundation-check: project-context-check rust-check model-catalog-check storage-check tools-check context-check agent-loop-check model-provider-check runtime-rpc-check ai-check ai-auth-check

# Qualify context transitions and commands without another scheduler or provider engine.
context-check:
    cargo +{{rust_version}} test --locked -p swarmie-context -p swarmie-commands --all-features
    cargo +{{rust_version}} test --locked -p swarmie-context --no-default-features
    cargo +{{rust_version}} clippy --locked -p swarmie-context -p swarmie-commands --all-features --all-targets -- -D warnings
    RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -D rustdoc::broken_intra_doc_links" cargo +{{rust_version}} doc --locked -p swarmie-context -p swarmie-commands --all-features --no-deps

# Qualify the Context client projection and its composition in the existing client.
context-browser-check:
    bun run ui:elements:prepare
    bun run --filter @swarmie/plugin-context-browser typecheck
    bun run --filter @swarmie/client typecheck
    bun run lint:code plugins/context/browser/src plugins/context/browser/test apps/web-client/test/context.test.tsx apps/web-client/src/client/context-selection.ts apps/web-client/src/client/initialize.ts apps/web-client/src/shell/pinned-context.tsx apps/web-client/src/conversations/composer
    bun test plugins/context/browser/test apps/web-client/test
    bun run build:client

# Qualify the independent loop and its explicitly optional task adapter.
agent-loop-check:
    cargo +{{rust_version}} test --locked -p swarmie-agent-loop --no-default-features
    cargo +{{rust_version}} test --locked -p swarmie-agent-loop --no-default-features --features conversations
    cargo +{{rust_version}} test --locked -p swarmie-agent-loop --no-default-features --features durable-tasks
    cargo +{{rust_version}} test --locked -p swarmie-agent-loop --all-features
    cargo +{{rust_version}} clippy --locked -p swarmie-agent-loop --no-default-features --features conversations --all-targets -- -D warnings
    cargo +{{rust_version}} clippy --locked -p swarmie-agent-loop --all-features --all-targets -- -D warnings

agent-loop-dependencies:
    cargo +{{rust_version}} tree --locked -p swarmie-agent-loop --no-default-features --edges normal

# Update workspace membership while retaining existing external dependency pins.
rust-lock-workspace:
    cargo +{{rust_version}} update --workspace

# Qualify native HTTP adapters and the explicitly opt-in scripted provider.
model-provider-check:
    cargo +{{rust_version}} test --locked -p swarmie-providers --no-default-features
    cargo +{{rust_version}} test --locked -p swarmie-model-provider-ai --no-default-features
    cargo +{{rust_version}} test --locked -p swarmie-model-provider-ai --features mock
    cargo +{{rust_version}} clippy --locked -p swarmie-model-provider-ai --all-features --all-targets -- -D warnings

# Qualify the published SDK separately from native Rust library checks.
sdk-check: tembra-authoring-test

# Qualify tool registration independently and with the optional manager bridge.
tools-check:
    cargo +{{rust_version}} test --locked -p swarmie-tools --no-default-features
    cargo +{{rust_version}} test --locked -p swarmie-tools --all-features
    cargo +{{rust_version}} clippy --locked -p swarmie-tools --all-features --all-targets -- -D warnings

# Inspect the default tool capability graph without optional composition.
tools-dependencies:
    cargo +{{rust_version}} tree --locked -p swarmie-tools --no-default-features --edges normal

# Develop and qualify the independently mounted node client.
client-dev:
    bun run dev:client

client-build:
    bun run build:client

client-check:
    bun run --filter @swarmie/client typecheck
    bun run test:client

# Desktop hosts the shared web client through the pinned Native SDK.
desktop-dev:
    bun run dev:desktop

desktop-build:
    SWARMIE_MODEL_CATALOG_REFRESH="$(date +%s)-$$" bun run build:desktop

# Qualify shared protected hosting and both concrete local adapters.
node-check:
    cargo +{{rust_version}} fmt --all --check
    cargo +{{rust_version}} clippy --locked -p swarmie-node -p swarmie-node-host -p swarmie-desktop-runtime --all-targets -- -D warnings
    cargo +{{rust_version}} test --locked -p swarmie-node -p swarmie-node-host -p swarmie-desktop-runtime
    RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links" cargo +{{rust_version}} doc --locked -p swarmie-node -p swarmie-node-host -p swarmie-desktop-runtime --no-deps

node-build:
    cargo +{{rust_version}} build --locked --release -p swarmie-node-host

node-dependencies:
    cargo +{{rust_version}} tree --locked -p swarmie-node-host --edges normal

desktop-bridge-check:
    zig test apps/desktop-client/src/bridge-frame.zig

desktop-check: desktop-bridge-check
    cargo +{{rust_version}} fmt --all --check
    cargo +{{rust_version}} clippy --locked -p swarmie-desktop-updates -p swarmie-desktop-runtime -p swarmie-git --all-features --all-targets -- -D warnings
    cargo +{{rust_version}} test --locked -p swarmie-desktop-updates -p swarmie-desktop-runtime -p swarmie-git --all-features
    RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links" cargo +{{rust_version}} doc --locked -p swarmie-desktop-updates -p swarmie-desktop-runtime -p swarmie-git --all-features --no-deps
    bun test scripts/distribution/desktop.test.ts
    bun run build:desktop:updater
    bun run check:desktop
    bun run test:desktop

desktop-package platform:
    SWARMIE_MODEL_CATALOG_REFRESH="$(date +%s)-$$" bun run build:desktop
    bun run package:desktop --target {{ quote(platform) }}


# Qualify the optional upstream byte RPC entry without adding a transport engine.
runtime-rpc-check:
    cargo +{{rust_version}} test --locked -p swarmie-runtime --features rpc --test node
    cargo +{{rust_version}} clippy --locked -p swarmie-runtime --features rpc --all-targets -- -D warnings
    RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -D rustdoc::broken_intra_doc_links" cargo +{{rust_version}} doc --locked -p swarmie-runtime --features rpc --no-deps

# Qualify storage without its default backend and the independent runtime.
storage-check:
    cargo +{{rust_version}} test --locked -p swarmie-storage --no-default-features
    cargo +{{rust_version}} clippy --locked -p swarmie-storage --no-default-features --all-targets -- -D warnings
    RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -D rustdoc::broken_intra_doc_links" cargo +{{rust_version}} doc --locked -p swarmie-storage --no-default-features --no-deps
    cargo +{{rust_version}} clippy --locked -p swarmie-runtime --no-default-features --features rpc --lib -- -D warnings

# Repeat synthetic workloads and compare compatible performance reports.
perf *arguments:
    bun run perf "$@"

perf-check:
    bun run test:perf
    cargo +{{rust_version}} clippy --locked -p swarmie-fixture-performance --all-features --all-targets -- -D warnings

# Refresh public metadata into Cargo OUT_DIR and embed it in the library.
model-catalog-build:
    SWARMIE_MODEL_CATALOG_REFRESH="$(date +%s)-$$" cargo +{{rust_version}} build --locked -p swarmie-model-catalog

model-catalog-check:
    cargo +{{rust_version}} test --locked -p swarmie-model-catalog
    cargo +{{rust_version}} clippy --locked -p swarmie-model-catalog --all-targets -- -D warnings
    RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -D rustdoc::broken_intra_doc_links" cargo +{{rust_version}} doc --locked -p swarmie-model-catalog --no-deps

# Qualify optional native custody and transport features without changing default composition.
rust-clippy-native:
    cargo +{{rust_version}} clippy --locked -p swarmie-vault -p swarmie-permissions --all-features --all-targets -- -D warnings

# Qualify independent linking, native account custody and the shared bootstrap contract.
device-linking-check:
    cargo +{{rust_version}} clippy --locked -p swarmie-device-linking --all-features --all-targets -- -D warnings
    cargo +{{rust_version}} test --locked -p swarmie-device-linking --all-features
    RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -D rustdoc::broken_intra_doc_links" cargo +{{rust_version}} doc --locked -p swarmie-device-linking --all-features --no-deps

# Qualify the independent protocol library without mounting it in a runtime.
ai-check:
    just rust-xtask check-provider-boundary
    cargo +{{rust_version}} test --locked -p swarmie-ai
    cargo +{{rust_version}} test --locked -p swarmie-ai --features telemetry
    cargo +{{rust_version}} clippy --locked -p swarmie-ai --all-features --all-targets -- -D warnings
    RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -D rustdoc::broken_intra_doc_links" cargo +{{rust_version}} doc --locked -p swarmie-ai --all-features --no-deps

# Qualify the optional credential handoff without changing current provider consumers.
ai-auth-check:
    cargo +{{rust_version}} test --locked -p swarmie-provider-auth
    cargo +{{rust_version}} test --locked -p swarmie-provider-auth --features ai
    cargo +{{rust_version}} clippy --locked -p swarmie-provider-auth --all-features --all-targets -- -D warnings
    RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -D rustdoc::broken_intra_doc_links" cargo +{{rust_version}} doc --locked -p swarmie-provider-auth --all-features --no-deps

ai-dependencies:
    cargo +{{rust_version}} tree --locked -p swarmie-ai --edges normal

# Qualify native project content independently and through conversation composition.
project-context-check:
    cargo +{{rust_version}} test --locked -p swarmie-project-context
    cargo +{{rust_version}} test --locked -p swarmie-agent-loop --all-features
    cargo +{{rust_version}} test --locked -p swarmie-context --test textual --test budget
    cargo +{{rust_version}} test --locked -p swarmie-client-node --test checkout_admission
    cargo +{{rust_version}} clippy --locked -p swarmie-project-context -p swarmie-client-node --all-features --all-targets -- -D warnings
    RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -D rustdoc::broken_intra_doc_links" cargo +{{rust_version}} doc --locked -p swarmie-project-context -p swarmie-client-node --no-deps
