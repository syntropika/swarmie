set shell := ["bash", "-euo", "pipefail", "-c"]
set positional-arguments

rust_version := `python3 -c 'import tomllib; print(tomllib.load(open("mise.toml", "rb"))["tools"]["rust"])'`

default:
    @just --list

# Install shared tools and dependencies from the repository root.
setup:
    mise install
    mise exec -- bun install --frozen-lockfile
    mise exec -- just tembra-sdk

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
    EXPO_NO_TELEMETRY=1 bun run --cwd apps/mobile export

mobile-check:
    bun run check:mobile

mobile-config:
    EXPO_NO_TELEMETRY=1 bun run --cwd apps/mobile config:public

mobile-runtime platform:
    EXPO_NO_TELEMETRY=1 bun run --cwd apps/mobile expo-updates runtimeversion:resolve --platform {{platform}}

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

# Tembra is consumed from one immutable public dependency revision.
tembra-bootstrap:
    python3 scripts/tembra/tooling.py bootstrap

tembra-sdk: tembra-bootstrap
    python3 scripts/tembra/tooling.py project-sdk
    python3 scripts/tembra/tooling.py verify-sdk

tembra-contracts:
    bun run plugins:contracts

tembra-plugins:
    bun run plugins:build

tembra-browser:
    RUSTUP_TOOLCHAIN={{rust_version}} python3 scripts/tembra/tooling.py browser-wasm

rust-lock:
    cargo +{{rust_version}} generate-lockfile

rust-fmt:
    cargo +{{rust_version}} fmt --all

rust-fmt-check:
    cargo +{{rust_version}} fmt --all --check

rust-clippy:
    cargo +{{rust_version}} clippy --locked --workspace --all-targets -- -D warnings

rust-test *arguments:
    cargo +{{rust_version}} test --locked "$@"

rust-run *arguments:
    cargo +{{rust_version}} run --locked "$@"

rust-doc:
    RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -D rustdoc::broken_intra_doc_links" cargo +{{rust_version}} doc --locked --workspace --no-deps

rust-check: rust-fmt-check rust-clippy
    cargo +{{rust_version}} test --locked --workspace
    just rust-doc

tembra-tooling-test:
    python3 -m unittest discover -s scripts/tembra -p 'test_*.py'

# Compare native Hook descriptors with their generated TypeScript registrations.
tembra-authoring-test: tembra-plugins
    bun test tests/plugins/lifecycle-authoring.test.ts tests/plugins/fixtures/agent-context/contracts.test.ts

foundation-check: tembra-tooling-test tembra-authoring-test rust-check
    bunx tsc --noEmit -p tests/plugins/tsconfig.json
    bunx tsc --noEmit -p scripts/tembra/tsconfig.json

# The checked browser ABI must match the dependency's pinned wasm-bindgen version.
tembra-browser-tools:
    rustup target add wasm32-unknown-unknown --toolchain {{rust_version}}
    cargo +{{rust_version}} install wasm-bindgen-cli --version 0.2.129 --locked

# Real Browser Host ESM, React, managed contribution and server RPC acceptance.
plugin-browser-check: tembra-plugins tembra-browser
    bun tests/plugins/browser/check.ts

# Session authority, replay and actual Tembra/SQLite durability acceptance.
session-test:
    just rust-test -p swarmie-session-contracts -p swarmie-session-store-sqlite

session-benchmark:
    cargo +{{rust_version}} test --locked --release -p swarmie-session-store-sqlite --test runtime session_volume_benchmark -- --ignored --nocapture

# Explicit scale qualification of the atomic conversation-index migration.
session-migration-benchmark:
    cargo +{{rust_version}} test --locked --release -p swarmie-session-store-sqlite --test migration million_event_migration_benchmark -- --ignored --nocapture

# Qualify model Contracts, the provider adapter and real Tembra Host behavior.
models-check:
    just rust-fmt-check
    cargo +{{rust_version}} clippy --locked -p swarmie-model-contracts -p swarmie-model-provider-genai --all-targets -- -D warnings
    just rust-test -p swarmie-model-contracts -p swarmie-model-provider-genai
    RUSTDOCFLAGS="-D rustdoc::broken_intra_doc_links" cargo +{{rust_version}} doc --locked -p swarmie-model-contracts -p swarmie-model-provider-genai --no-deps

# Measures State publication batching with deterministic local HTTP SSE.
models-benchmark:
    just rust-test -p swarmie-model-provider-genai benchmark_batched_state_publications -- --ignored --nocapture
