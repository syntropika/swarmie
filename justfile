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
