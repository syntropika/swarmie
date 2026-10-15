# Distribution and staged updates

The release package contains the Rust runtime, the Native SDK host, and built React frontend assets. The packaged host attaches to the configured Environment's verified loopback runtime, starting a detached owner when needed, then starts a client-owned gateway and opens the system WebView through its application IPC bridge. Frontend assets are served by the client gateway and are not required by the execution owner. Closing the native window leaves the runtime and its sessions running. Explicit `swarmie runtime stop` shuts down the owner while preserving retained history. The launcher does not install an OS restart supervisor. Native harness executables and credentials remain installed and managed separately.

Build on the target operating system with Rust 1.88 or later, Node.js 24 or later, and the native system dependencies documented in `apps/desktop/README.md`:

```sh
python3 scripts/distribution/package.py --output /absolute/artifact/directory
```

Run macOS commands on the configured macOS SSH host. The default macOS signing mode is ad hoc; this verifies the integrity of the bundle locally and does not provide a Developer ID or notarization ticket. For a distributable signed bundle, supply an existing signing identity and an existing `notarytool` Keychain profile:

```sh
python3 scripts/distribution/package.py --output /absolute/artifact/directory \
  --signing identity --identity 'Developer ID Application: Example (TEAMID)' \
  --notary-profile swarmie-release \
  --release-base-url https://releases.example.com/swarmie
```

The script packages actual release binaries, verifies signed macOS bundles with `codesign --verify --deep --strict`, and creates a ZIP on macOS/Windows or tar.gz on Linux. It writes a SHA-256 sidecar. With `--release-base-url`, it also writes the updater manifest. Upload the archive and manifest to the specified HTTPS location. Use `release.py assemble` to verify and consolidate platform packages into separate desktop and runtime manifests. `--skip-build` requires already built release binaries and frontend assets.

## Standalone runtime

A separate package supports execution hosts without a desktop host or frontend:

```sh
python3 scripts/distribution/package-runtime.py --output /absolute/artifact/directory
```

This script builds only the Rust executable and includes Apache-2.0 LICENSE, Syntropika NOTICE, a runtime-only installer and package metadata. Python 3.11 or later is required to package it; installation uses Python 3.9 or later. See `../../docs/runtime-installation.md` for install, explicit user-service startup, atomic updates and preservation rules. The standalone package and desktop package have separate release manifests and must not overwrite each other's launcher without an explicit installation layout.

## Linux installation

Extract the archive, then run its bundled installer:

```sh
python3 install.py
```

This installs into `~/.local/share/swarmie/application`, links the native host and CLI into `~/.local/bin`, and creates a desktop entry. `--prefix`, `--bin-dir`, and `--desktop-dir` support explicit user-owned paths. The installer refuses to overwrite directories, launchers, or desktop entries that it does not own. To remove application files while preserving session data:

```sh
python3 install.py --uninstall
```

No daemon service is activated during installation. Optional service installation is a separate action:

```sh
swarmie service install --desktop-assets /absolute/application/resources --workbench
swarmie service enable
swarmie service start
swarmie service status
swarmie service stop
swarmie service uninstall
```

macOS uses a user LaunchAgent; Linux uses a user systemd unit; Windows uses a least-privilege user logon task. The service stores its bearer token in a private file, and the unit references that file. `swarmie service token` explicitly prints the token for pairing. `service install` writes the unit without starting it. `service enable` and `service disable` change future startup without changing current execution. Service uninstall requires an explicit stop first and preserves token and state. Linux user-manager availability after logout and macOS startup without a GUI login are separate host configuration and qualification concerns.

## Updates

Configure an HTTPS manifest explicitly; there is no built-in remote release server:

```sh
swarmie update check --manifest https://releases.example.com/swarmie/manifest.json
swarmie update stage --manifest https://releases.example.com/swarmie/manifest.json
```

Alternatively set `SWARMIE_UPDATE_MANIFEST`. Checking returns the current version, offered version, target platform and availability. Staging accepts only a newer semantic version, follows only HTTPS redirects, downloads at most 512 MiB, and publishes an archive only after its SHA-256 matches. A failed download or checksum removes the partial file. Staging does not extract an archive, execute a program, replace an installation, or restart the application. Review the staged path, close the application, then install the new Linux package or replace the macOS bundle. Application data remains separate from the installation.

Manifest format:

```json
{
  "version": "0.2.0",
  "artifacts": {
    "linux-x86_64": {
      "url": "https://releases.example.com/swarmie/swarmie-0.2.0-linux-x86_64.tar.gz",
      "sha256": "64 hexadecimal characters",
      "fileName": "swarmie-0.2.0-linux-x86_64.tar.gz"
    }
  }
}
```

Supported target keys are `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`, and `windows-aarch64`. The manifest is bounded to 64 KiB. HTTPS authenticates the configured release endpoint; the checksum checks the downloaded archive against that manifest. macOS release signing and notarization remain the publisher's responsibility.

## Verification

```sh
python3 scripts/distribution/test_install.py
cargo test -p app --bin swarmie --test remote_cli
```

Tests exercise installation and upgrade in isolated directories, refusal to overwrite unmanaged paths, preservation of session data, actual persistent runtime attachment, client closure, and explicit owner shutdown, bearer pairing, explicit HTTPS proxy origin enforcement, bounded downloads, checksum rejection, and verified archive publication. No live user installation or service is activated by these tests.

## Remote connections

Pair a reachable daemon with `swarmie remote pair NAME URL --token TOKEN`. HTTP(S) roots normalize to `/rpc`; WebSocket connections accept WS(S) endpoints. Pairing verifies the actual daemon before saving private credentials. `remote list` returns connection metadata without tokens, `remote call NAME METHOD --params '{}'` uses the saved connection, and `remote remove NAME` removes it.

Saved routes now use the SQLite catalog inside the configured application data root (`--state-dir`), shared with automatic local desktop registration. `swarmie node list` groups routes by retained Environment identity, includes the local node after desktop attachment, and exposes historical verification timestamps without probing availability. Bearer tokens use separate private files under `credentials/node-connections`; ordinary metadata contains opaque references. `remote remove` removes one route and its credential without stopping the destination or deleting its records. Concurrent updates and stale SSH reconnections cannot overwrite a changed or re-enrolled route.

The current development schema is version 3. Use a fresh development data root when an older internal schema is rejected. No schema migration or historical connection importer is provided; existing files remain untouched.

HTTP runtimes publish a private Unix socket on supported platforms. `runtime status` reports its actual `localSocket` path. Local native clients use that socket; other platform clients retain loopback WebSocket access. Long application data roots use a deterministic short socket address in a private user directory. Unix directory and socket permissions establish same-user authorization separately from HTTP bearer authorization.

For SSH, configure ordinary OpenSSH access and install and start the runtime explicitly on the destination. The runtime executable must be available as `swarmie` on its noninteractive SSH command path. Inspect the installation and existing owner, then register its verified socket:

```sh
swarmie remote inspect-ssh user@host --socket /absolute/runtime/socket
swarmie remote ssh workstation user@host --socket /absolute/runtime/socket
```

These commands never install, start, update, or restart a remote runtime. Missing installation and unavailable runtime readiness produce explicit next steps. SSH uses the configured host-key and key authentication with batch mode; unknown hosts and interactive authentication must be resolved explicitly outside enrollment.

The client starts an owned SSH subprocess that invokes `swarmie runtime proxy`, adding `--socket PATH` only for an explicit override on the remote host. The proxy copies the common WebSocket byte stream between stdio and the existing Unix socket without opening an execution owner. Closing or removing this client connection reaps the owned SSH child and leaves remote sessions running. Sharing and SSH backgrounding are disabled so this connection cannot take ownership of another client transport. Native observation reconnects with bounded backoff and verifies the saved retained identity and current incarnation before dispatch. Dispatched prompts are never replayed automatically.

Desktop **Add node → Connection method → SSH** exposes this check-and-enroll sequence. Verification returns the exact runtime identity; enrollment rechecks it and refuses a changed owner. Host or socket edits invalidate a prepared check. Automatic SSH installation, password prompts inside the desktop, alias discovery, and live cross-platform SSH qualification remain separate follow-up work.

For mobile or browser access, place an authenticated loopback daemon behind an HTTPS reverse proxy and explicitly authorize that public origin:

```sh
swarmie serve --http 127.0.0.1:7480 --desktop-assets /absolute/frontend/assets \
  --token-file /private/token-file --public-origin https://agents.example.com
```

The proxy must preserve the authorized HTTPS Host and Origin and forward bearer authorization. Other origins, mismatched hosts and missing tokens remain rejected. The runtime does not provision TLS certificates or a public tunnel.

For an explicitly configured external browser SSH forward whose local port differs from the daemon port, explicitly pin the forwarded origin on the daemon, for example `--public-origin http://127.0.0.1:43123`. This requires a bearer token and accepts HTTP only at a loopback address or localhost. Configure the SSH forward to use that exact local port. Host and Origin must match the pinned origin; other plaintext browser origins remain rejected.

To open a project or saved session from a terminal, use `swarmie open --url URL --project PATH --session UUID`. For HTTP URLs at `127.0.0.1` or `[::1]`, packaged installations launch their sibling Native SDK host and select the requested project and session. Development builds accept `--host /absolute/path/to/swarmie-desktop`. The existing daemon must serve the frontend; the command opens a window without starting another daemon. The native window remains open after the CLI exits. Explicit missing or invalid host paths fail. HTTPS URLs and installations without a native host use the operating system's URL opener. `--print` returns the encoded URL without launching a client.

## Release preparation

See [release distribution](../../docs/release-distribution.md) for the manual six-platform workflow, verified manifest assembly, optional draft creation and implemented user-startup management.

## Windows desktop

Native Windows packaging builds the WebView2 desktop host and matching Rust service executable, includes the architecture-matched WebView2 loader, and creates a portable ZIP with licenses, frontend resources and native binaries. The target computer needs the WebView2 runtime. Packaging does not install WebView2 or modify system registration. Windows x86_64 and aarch64 release jobs use native runners; downloaded executables remain unsigned until production signing is configured.

See [automatic startup](../../docs/automatic-startup.md) for explicit node controls and platform service scopes. Native x86_64 development checks qualify the desktop window, private transport, real OpenSSH installation and RPC, directory bindings, and Task Scheduler launch. The prepared release matrix still requires native MSVC and ARM64 execution, signing, and reboot/logout qualification.
