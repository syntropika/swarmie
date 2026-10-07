# Distribution and staged updates

The release package contains the Rust runtime, the Native SDK host, and built React frontend assets. The packaged host starts a managed daemon on a fresh loopback port, then opens the system WebView against that daemon. Closing the native window closes its daemon. Native harness executables and credentials remain installed and managed separately.

Build on the target operating system with Rust 1.88 or later, Node.js 24 or later, and the native system dependencies documented in `apps/desktop/README.md`:

```sh
python3 scripts/distribution/package.py --output /absolute/artifact/directory
```

Run macOS commands on the configured macOS SSH host. The default macOS signing mode is ad hoc; this verifies the integrity of the bundle locally and does not provide a Developer ID or notarization ticket. For a distributable signed bundle, supply an existing signing identity and an existing `notarytool` Keychain profile:

```sh
python3 scripts/distribution/package.py --output /absolute/artifact/directory \
  --signing identity --identity 'Developer ID Application: Example (TEAMID)' \
  --notary-profile mpai-release \
  --release-base-url https://releases.example.com/mpai
```

The script packages actual release binaries, verifies signed macOS bundles with `codesign --verify --deep --strict`, and creates a ZIP on macOS or tar.gz on Linux. It writes a SHA-256 sidecar. With `--release-base-url`, it also writes the updater manifest. Upload the archive and manifest to the specified HTTPS location. Multiple architecture builds can be combined by collecting their entries into the same manifest's `artifacts` object. `--skip-build` requires already built release binaries and frontend assets.

## Linux installation

Extract the archive, then run its bundled installer:

```sh
python3 install.py
```

This installs into `~/.local/share/mpai/application`, links the native host and CLI into `~/.local/bin`, and creates a desktop entry. `--prefix`, `--bin-dir`, and `--desktop-dir` support explicit user-owned paths. The installer refuses to overwrite directories, launchers, or desktop entries that it does not own. To remove application files while preserving session data:

```sh
python3 install.py --uninstall
```

No daemon service is activated during installation. Optional service installation is a separate action:

```sh
mpai service install --desktop-assets /absolute/application/resources --workbench
mpai service start
mpai service status
mpai service stop
mpai service uninstall
```

macOS uses a user LaunchAgent; Linux uses a user systemd unit. The service stores its bearer token in a private file, and the unit references that file. `mpai service token` explicitly prints the token for pairing. `service install` writes the unit without starting it.

## Updates

Configure an HTTPS manifest explicitly; there is no built-in remote release server:

```sh
mpai update check --manifest https://releases.example.com/mpai/manifest.json
mpai update stage --manifest https://releases.example.com/mpai/manifest.json
```

Alternatively set `MPAI_UPDATE_MANIFEST`. Checking returns the current version, offered version, target platform and availability. Staging accepts only a newer semantic version, follows only HTTPS redirects, downloads at most 512 MiB, and publishes an archive only after its SHA-256 matches. A failed download or checksum removes the partial file. Staging does not extract an archive, execute a program, replace an installation, or restart the application. Review the staged path, close the application, then install the new Linux package or replace the macOS bundle. Application data remains separate from the installation.

Manifest format:

```json
{
  "version": "0.2.0",
  "artifacts": {
    "linux-x86_64": {
      "url": "https://releases.example.com/mpai/mpai-0.2.0-linux-x86_64.tar.gz",
      "sha256": "64 hexadecimal characters",
      "fileName": "mpai-0.2.0-linux-x86_64.tar.gz"
    }
  }
}
```

Supported target keys are `linux-x86_64`, `linux-aarch64`, `macos-x86_64`, and `macos-aarch64`. The manifest is bounded to 64 KiB. HTTPS authenticates the configured release endpoint; the checksum checks the downloaded archive against that manifest. macOS release signing and notarization remain the publisher's responsibility.

## Verification

```sh
python3 scripts/distribution/test_install.py
cargo test -p app --bin mpai --test remote_cli
```

Tests exercise installation and upgrade in isolated directories, refusal to overwrite unmanaged paths, preservation of session data, actual managed daemon readiness and teardown, bearer pairing, explicit HTTPS proxy origin enforcement, bounded downloads, checksum rejection, and verified archive publication. No live user installation or service is activated by these tests.

## Remote connections

Pair a reachable daemon with `mpai remote pair NAME URL --token TOKEN`. HTTP(S) roots normalize to `/rpc`; WebSocket connections accept WS(S) endpoints. Pairing verifies the actual daemon before saving private credentials. `remote list` returns connection metadata without tokens, `remote call NAME METHOD --params '{}'` uses the saved connection, and `remote remove NAME` removes it.

For a private SSH connection, keep a local loopback forward open:

```sh
mpai remote ssh workstation user@host --remote-port 7480 --local-port 7490 --token TOKEN
```

This verifies forwarding and daemon authentication before saving the connection. Interrupting the command closes its SSH tunnel. The daemon may remain bound to remote loopback. The Native SDK shell accepts only an explicit local HTTP origin; open the local forwarded URL for remote desktop access.

Add `--reconnect` to supervise dropped SSH forwards. It retains the same local port, retries with exponential delays from 500 ms to 30 seconds, and resets the delay after a connection stays healthy for 30 seconds. Each attempt must verify daemon authentication again. JSON-line receipts report `ssh.connected`, `ssh.disconnected`, `ssh.reconnecting` and `ssh.stopped`, with attempt number, timestamp, owned SSH PID and retry delay where applicable. They omit bearer credentials. Ctrl-C cancels connection attempts and delays, reaps the owned child, and never starts another forward. Without this flag, a dropped SSH process ends the command. Each forward disables SSH connection sharing and backgrounding so it cannot take ownership of another configured SSH connection.

For mobile or browser access, place an authenticated loopback daemon behind an HTTPS reverse proxy and explicitly authorize that public origin:

```sh
mpai serve --http 127.0.0.1:7480 --desktop-assets /absolute/frontend/assets \
  --token-file /private/token-file --public-origin https://agents.example.com
```

The proxy must preserve the authorized HTTPS Host and Origin and forward bearer authorization. Other origins, mismatched hosts and missing tokens remain rejected. The runtime does not provision TLS certificates or a public tunnel.

For a system WebView reached through an SSH forward whose local port differs from the daemon port, explicitly pin the forwarded origin on the daemon, for example `--public-origin http://127.0.0.1:43123`. This requires a bearer token and accepts HTTP only at a loopback address or localhost. Configure the SSH forward to use that exact local port. Host and Origin must match the pinned origin; other plaintext browser origins remain rejected.

To open a project or saved session from a terminal, use `mpai open --url URL --project PATH --session UUID`. For HTTP URLs at `127.0.0.1` or `[::1]`, packaged installations launch their sibling Native SDK host and select the requested project and session. Development builds accept `--host /absolute/path/to/mpai-desktop`. The existing daemon must serve the frontend; the command opens a window without starting another daemon. The native window remains open after the CLI exits. Explicit missing or invalid host paths fail. HTTPS URLs and installations without a native host use the operating system's URL opener. `--print` returns the encoded URL without launching a client.
