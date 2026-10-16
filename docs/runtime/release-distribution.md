# Release distribution

GitHub Releases is the primary distribution channel for the desktop application and standalone runtime. Each version provides native packages for Linux, macOS and Windows, on x86_64 and aarch64. A server needs the matching runtime package and Python 3.9 or later for installation; it does not need Rust, Node.js, frontend assets, or a graphical session. Harness executables and credentials remain separately managed.

Publishing reusable libraries or a developer-oriented `cargo install` package can be considered later. Crate publication is not part of this distribution flow.

## Manual preparation

The parent repository's `Prepare release assets` workflow runs only through `workflow_dispatch`. Provide an existing version tag, such as `v0.1.0`. The tag must contain the release tooling, and its version must match the workspace Cargo metadata, desktop manifest and desktop npm package. Create the tag only after the intended changes are committed and reviewed. The workflow neither creates tags nor changes repository visibility.

The six native builds use Rust 1.94.0, Node.js 24, Python 3.12 and the pinned native desktop CLI. Each checkout excludes the private submodule. Distribution-tool tests, native updater and SSH-preparation tests, and frontend behavior tests run during preparation. Packaging includes Apache-2.0 license and notice files. Uploaded job artifacts include only archives, SHA-256 sidecars and partial release manifests; expanded application bundles and unrelated repository files are excluded.

The assembly job requires both package kinds for all six platforms. It rejects missing or duplicate platforms, mixed versions, unexpected manifest fields, mismatched versioned download URLs, unsafe filenames, linked files, oversized inputs, and archive/checksum disagreement. It validates all inputs before creating a fresh output directory and never overwrites an existing output set. The final `release-assets` workflow artifact contains:

- `swarmie-VERSION-PLATFORM.tar.gz` for Linux desktop packages and `.zip` for macOS and Windows desktop packages;
- `swarmie-runtime-VERSION-PLATFORM.tar.gz` for standalone runtimes;
- one `.sha256` sidecar per archive;
- consolidated `manifest.json` and `runtime-manifest.json`, each with six platform entries;
- `SHA256SUMS`, covering the archives, sidecars and consolidated manifests.

Independent local assembly uses the same validation:

```sh
python3 scripts/distribution/release.py validate-tag --tag v0.1.0
python3 scripts/distribution/release.py assemble \
  --tag v0.1.0 \
  --repository syntropika/swarmie \
  --artifacts /absolute/downloaded/package-artifacts \
  --output /absolute/new/release-assets
```

Repeated `--platform` arguments allow a smaller local preparation set. The production workflow always requires all six platforms.

## Review and publication

The `create_draft` input defaults to false. When explicitly selected, a separate job receives repository write permission and creates a new draft release with the verified assets. It requires the existing tag and refuses to modify a release already associated with it. Build and assembly jobs have read-only repository permission. The workflow never publishes a release, replaces existing release assets, or changes repository visibility.

The current macOS desktop packages use ad-hoc signing, and standalone runtime binaries are unsigned. Production signing, notarization, minimum supported operating-system qualification, and a complete live CI matrix run remain pending. A successful assembly is integrity verification, not proof of production signing or platform qualification. This change prepares tooling; it does not create a release or run the remote workflow.

Private or draft release assets are not anonymously downloadable by ordinary clients. Before public distribution, use trusted local runtime packages or an accessible HTTPS manifest. Do not embed hosted-service or repository credentials in public client code.

## First installation and updates

Native SSH first installation defaults to the runtime manifest under the client's own versioned release:

```text
https://github.com/syntropika/swarmie/releases/download/vVERSION/runtime-manifest.json
```

It selects the remote platform and does not follow a mutable latest-release URL. This keeps a client's default first installation on its matching runtime version. An explicit `SWARMIE_RUNTIME_MANIFEST` override, from the native process environment or build configuration, supports another trusted HTTPS source. Matching local packages take precedence and allow offline installation. A missing or inaccessible release causes a download failure; inspection itself does not install or modify the remote node.

Desktop and runtime manifests remain separate. Update checking and staging require an explicitly selected newer-version manifest and never implicitly replace an installation, restart execution, or resend work. See [standalone runtime installation](runtime-installation.md) for installer ownership and lifecycle rules.

## Automatic startup

The native node view and `service startup` CLI provide explicit inspection and user-service enablement without interrupting an active runtime. Linux can explicitly enable user lingering for startup before login. macOS uses a user GUI LaunchAgent and Windows uses a least-privilege user logon task. Administrative system-service scope and live reboot/logout/crash qualification remain separate work. See [automatic startup](automatic-startup.md).
