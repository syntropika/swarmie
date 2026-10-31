# Rust source checks

The root `justfile` owns Rust verification. The unpublished `xtask` package implements
source checks that need parsed Rust or Cargo manifests. It uses the pinned toolchain
and the workspace lockfile. It has no provider credentials or release authority.

Run all source checks with `just rust-source-check`. This command also runs as part of
`just rust-check`. Run one check with `just rust-xtask check-manifests` or
`just rust-xtask check-provider-boundary`. Test the checker with `just rust-test -p xtask`.

## Workspace lints

The manifest check uses offline Cargo metadata to enumerate actual workspace members.
Every member, including test fixtures and tools, must set `[lints] workspace = true`.
Missing files, invalid manifests and an empty member set fail the check. The command
does not update dependencies or download model data.

The root Clippy policy rejects debug macros, placeholder implementations, and lock or
RefCell guards held across an await. Required Clippy commands use `--all-targets` and
`-D warnings`. Capability recipes check their supported feature combinations separately.
The `xtask` crate also rejects unwrap, expect, unchecked indexing, explicit panic and
unreachable paths, and panics in functions that return `Result`. This additional set
does not apply to all historical workspace code. New production code follows the
error-handling rules in the root agent instructions.

## Provider boundary

Provider modules in `crates/ai/src/codecs/providers/` encode and decode data. Credentials
and physical execution belong to their existing owners outside these modules.

The checker parses every Rust file under that directory. It rejects async functions,
async blocks, await expressions, known transport implementations, and direct paths or
imports for network, filesystem, process, thread and runtime environment access. It
also inspects macro tokens for async and await. Compile-time platform constants and
build variables remain permitted. Comments and string contents do not cause a match.

An explicit `#[cfg(test)]` item is excluded, so inline tests can use a local fixture
server. Other conditional branches are checked even when the current platform does
not compile them. Missing or empty source roots, symbolic links and parse errors fail.

These are syntactic checks. They do not resolve names, expand macros or prove that
indirect calls have no effects. Review aliases, helpers, dependencies and macro
expansions when changing the provider boundary. The check does not grant execution,
account or capability authority.
