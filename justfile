set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# Install shared tools and dependencies from the repository root.
setup:
    mise install
    mise exec -- bun install --frozen-lockfile

dev:
    bun run dev:desktop

storybook:
    bun run storybook

fmt:
    cargo fmt --all
    bun run format

check:
    cargo fmt --all -- --check
    cargo clippy --locked --workspace --all-targets -- -D warnings
    bun run check

test:
    cargo test --locked --workspace
    bun run test

build:
    cargo build --locked --release -p app
    bun run build:desktop

build-storybook:
    bun run build:storybook

changelog:
    git-cliff --config cliff.toml

docs:
    RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --no-deps --document-private-items

# Stage current-host native libraries before language SDK workflows.
sdk-build:
    python3 sdk/tools/build_native.py

sdk-check:
    cargo fmt -p swarmie-client -p swarmie-client-ffi -p swarmie-client-node -- --check
    cargo clippy --locked -p swarmie-client -p swarmie-client-ffi -p swarmie-client-node --all-targets -- -D warnings
    RUSTDOCFLAGS="-D warnings" cargo doc --locked -p swarmie-client -p swarmie-client-ffi -p swarmie-client-node --no-deps --document-private-items
    bun run --cwd sdk/typescript typecheck

sdk-test swift_host: sdk-build
    cargo test --locked -p swarmie-client -p swarmie-client-ffi
    python3 scripts/client-conformance.py --swift-host {{swift_host}}

sdk-package binary output:
    python3 sdk/tools/package_local.py --binary {{binary}} --output {{output}}

# macOS builds run on a configured macOS host.
sdk-build-apple:
    python3 sdk/tools/build_native.py --apple

# Mobile commands run from this workspace and use local native tooling.
dev-mobile:
    EXPO_NO_TELEMETRY=1 bun run dev:mobile

mobile-prebuild platform:
    EXPO_NO_TELEMETRY=1 bun run --cwd apps/mobile prebuild --platform {{platform}}

sdk-build-mobile platform:
    python3 sdk/tools/build_mobile.py {{platform}}

mobile-run platform:
    EXPO_NO_TELEMETRY=1 bun run --cwd apps/mobile {{platform}}

mobile-export:
    EXPO_NO_TELEMETRY=1 bun run --cwd apps/mobile export

mobile-check:
    bun run check:mobile

resources-check:
    cargo fmt -p swarmie-resources -- --check
    cargo clippy --locked -p swarmie-resources --all-targets -- -D warnings
    cargo test --locked -p swarmie-resources
    RUSTDOCFLAGS="-D warnings" cargo doc --locked -p swarmie-resources --no-deps --document-private-items
