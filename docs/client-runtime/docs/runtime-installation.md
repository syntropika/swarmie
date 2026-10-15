# Standalone runtime installation

The independently installable runtime owns execution, retained sessions and node-local storage. It runs without a desktop host, frontend assets, a graphical session, the private repository, or hosted-service credentials. Harness executables and their credentials are installed separately. Installing this package does not start a runtime, enable a service, or change an existing process.

## Package

Build on the target operating system and architecture, using Python 3.11 or later and the pinned Rust toolchain:

```sh
python3 scripts/distribution/package-runtime.py --output /absolute/artifact/directory
```

The build invokes only Cargo for the `swarmie` executable. It does not invoke Node.js, the native desktop build, or the frontend build. `--skip-build` uses the existing release executable; `--binary /absolute/path/swarmie` accepts a matching native executable. The workspace version and executable version must match.

The `swarmie-runtime-VERSION-PLATFORM-ARCH.tar.gz` archive contains:

- `bin/swarmie` on Unix or `bin/swarmie.exe` on Windows
- Apache-2.0 `LICENSE` and `NOTICE` with Copyright 2026 Syntropika
- `install.py`
- `runtime-package.json`, containing the version, platform and executable checksum

A SHA-256 sidecar accompanies the archive. No desktop host, HTML, JavaScript, images or WebView libraries are included. The executable still requires the native operating-system libraries of its build target. Build each architecture on a compatible target host; the installer rejects packages for another platform or architecture.

macOS builds run on a macOS host. `--identity` optionally signs the executable with an existing signing identity and verifies that signature. Notarization is not automated by this runtime-only script. Release signing, notarization and minimum operating-system qualification remain release requirements. Native macOS runtime execution is qualified separately below.

## Install or update

Extract a trusted release archive and run its installer with Python 3.9 or later:

```sh
python3 install.py
```

The default prefix is `~/.local/share/swarmie/runtime`; the launcher is `~/.local/bin/swarmie`. Explicit `--prefix` and `--bin-dir` values support other user-owned paths. The desktop installer and standalone installer share the `swarmie` launcher name: use separate launcher directories if both installations must coexist. Neither installer replaces an unmanaged launcher.

For an update, run the new package's installer with the same installation paths. The installer verifies the executable checksum and platform, stages the replacement, and atomically replaces the executable at its stable path. On Unix, existing processes retain their loaded executable and continue their work. Windows refuses replacing an in-use executable and requires an explicit stop before an update. Subsequent starts use the replacement. Updates never implicitly stop, restart, cancel or resend work.

The installer owns only its recorded package files. It refuses to replace modified installation files and leaves additional user files untouched. Runtime state, credentials and project working directories are not installation files and must retain their lifecycle independently.

## Native SSH installation sources and startup

The client can install a missing default runtime after the user selects **Install and connect**. The default source is `https://github.com/syntropika/swarmie/releases/download/vVERSION/runtime-manifest.json`, where `VERSION` is the client version. Override it with a trusted HTTPS runtime-only release manifest through `SWARMIE_RUNTIME_MANIFEST`, either at build time or in the native client process environment. Selection uses the remote platform and may install the current release; it does not require a newer version than the client. The manifest is not a desktop-package manifest.

For local/offline distribution, provision `PLATFORM.tar.gz` and `PLATFORM.tar.gz.sha256` in the directory selected by `SWARMIE_RUNTIME_PACKAGES`, or in `runtime-packages/` beside the native client executable. The archive must contain a matching current-version runtime-only package. `package-runtime.py --client-packages /absolute/client/runtime-packages` creates these files for its native build platform. Package/checksum directories are trusted distribution inputs; an adjacent checksum does not independently authenticate an untrusted package source. The remote bootstrap independently checks the transferred archive against the checksum verified by the client.

The default URL is prepared, but no release is published by this change. Private or draft assets are not anonymously accessible; use an accessible trusted manifest or matching local packages until public distribution is available. An inaccessible or missing release fails during download. Inspection does not install prerequisites or create remote runtime state.

```sh
swarmie runtime start
swarmie remote prepare-ssh SSH_ALIAS --action install
swarmie remote prepare-ssh SSH_ALIAS --action start
```

`runtime start` starts a detached headless owner or attaches to the verified existing owner. It uses retained identity and owner/startup locks; it does not restart or replace an active owner. SSH preparation starts the default remote runtime only after an explicit action, verifies its identity, and enrolls that exact target in the native application operation. The standalone preparation CLI verifies without enrollment. The owner continues after the preparing client exits. Package installation, owner startup and operating-system service enablement remain separate operations; reboot startup and crash supervision require service configuration. The node details view provides explicit automatic-startup management; see [automatic startup](automatic-startup.md) for user scopes and remaining unattended-server qualification.

## User service

Install the service with an explicit state directory, then choose when to enable and start it:

```sh
swarmie --state-dir /absolute/runtime-data service install --port 7480
swarmie service enable
swarmie service start
swarmie service status
```

The unit runs the installed executable at its stable path and binds authenticated HTTP and WebSocket endpoints to loopback. It does not serve frontend assets. The runtime also publishes its private local socket for same-user native clients and SSH proxy connections. Service installation stores a private bearer token outside the package directory and references its file in the unit. Installing or regenerating a unit never starts or restarts the service.

`service enable` and `service disable` change future startup configuration without starting or stopping execution. On Linux the service uses the user systemd manager. Keeping a user manager running after logout, or starting it without a login session, requires separately configured lingering or equivalent host policy; installation does not enable it. The explicit `service startup enable --boot` flow can request Linux user lingering. System-wide service installation is not provided. On macOS the service uses a LaunchAgent in the user GUI domain; unattended server startup without a GUI login is not yet qualified.

Windows uses a least-privilege Task Scheduler task for the current user at sign-in. It does not provide startup without sign-in. The runtime exposes a same-user named pipe for local clients and SSH byte proxies. Remote Windows preparation and proxy attachment require OpenSSH server access, Windows PowerShell and Python 3.9 or later; it does not enable those prerequisites automatically.

A macOS LaunchAgent file is discovered on subsequent user login; `service enable` clears a prior disabled override, and `service disable` prevents future automatic startup. `service install` alone does not load it in the current session.

Before restarting an updated service, let active work finish or explicitly accept its interruption. Restart is an explicit sequence:

```sh
swarmie service stop
swarmie service start
```

The Linux unit requests graceful shutdown using SIGINT. Windows service stop verifies the owned task and live runtime, then requests graceful shutdown through the private local transport rather than forcibly terminating the task. An explicit stop can interrupt active work. Retained session history remains in the state directory; a runtime restart does not imply that an interrupted native Harness turn resumes automatically.

`swarmie service token` explicitly prints the private bearer token for a trusted client. Do not include it in shared logs or documents. Local operation and SSH-based same-user access do not require a hosted account.

## Remove

Stop execution and remove the service configuration before removing its package:

```sh
swarmie service stop
swarmie service uninstall
python3 install.py --uninstall
```

Service uninstall refuses a running or loaded service and refuses ambiguous manager state. It disables future startup, removes the unit and preserves the token and runtime data. The package installer refuses removal while a service unit references its installation. It removes only recorded unmodified package files and its own launcher; additional files and nonempty directories remain. No uninstall operation recursively deletes runtime state, projects or working directories.

## Downloaded updates

Supply `--release-base-url https://releases.example.com/swarmie/runtime` when packaging to generate `runtime-manifest.json`. Use a separate runtime release manifest rather than the desktop manifest:

```sh
swarmie update check --manifest https://releases.example.com/swarmie/runtime/runtime-manifest.json
swarmie update stage --manifest https://releases.example.com/swarmie/runtime/runtime-manifest.json
```

The updater stages a verified archive; it does not extract it or execute an installer. Review and extract that archive, then run its installer. The installer checks the binary against the package metadata, while the updater checks the archive against the HTTPS release manifest. A locally supplied archive requires equivalent trusted-source and checksum verification before extraction.

## Qualification

```sh
python3 tests/fixtures/runtime-distribution/test_install.py
python3 tests/fixtures/runtime-distribution/acceptance.py \
  --package /absolute/extracted/runtime-package \
  --artifacts /absolute/new/artifact-directory
```

Installer tests use disposable paths and check ownership, platform and checksum rejection, modified-file refusal, service removal requirements and preservation of user files, including unexpected files inside the installation prefix.

Linux acceptance runs the actual package executable from the generated user-service command, with an isolated systemctl fixture. It verifies that a running Harness turn completes across atomic package replacement, retained node identity and session records survive an explicit restart, active-service uninstall is refused, manager failure preserves the unit, and uninstall preserves credentials, state and working files. This qualifies the executable, generated command and service CLI behavior, not a live native systemd manager or unattended boot. No live installation or user service is changed.

macOS acceptance runs a native arm64 runtime from an isolated runtime-only package. It verifies installation, atomic replacement during accepted work, retained identity and transcript events across explicit restart, native local sockets and stdio proxies, and preservation of user files after package removal. Service-removal guards cover references to both the installed executable and its launcher, including XML-escaped launcher paths.

An isolated LaunchAgent using the generated service arguments and token file is bootstrapped through the actual user GUI launchd manager under a temporary service label. It starts successfully, is supervised across a forced process exit, retains the Environment identity after automatic restart, and prevents package removal while its unit references the installation. Cleanup unloads the temporary job and retains its state and credentials. This qualifies the native runtime and generated LaunchAgent behavior; it does not qualify login/reboot enablement, a system daemon without GUI login, release signing, notarization, or the native desktop window.

## Windows installation and updates

Run the included installer with `python install.py`. Default installation remains under the user home `.local/share/swarmie/runtime`; `.local/bin/swarmie.exe` is an owned native launcher copy. Unicode paths do not require a batch launcher. The managed executable is also resolved directly by SSH preparation when the user PATH has not been changed. State defaults to the user home `.swarmie`, independently of the installation.

Windows refuses replacement or removal of an in-use executable. Stop execution explicitly before an update. The installer checks existing files and the launcher before changing them, verifies recorded ownership and rejects reparse-point installation paths. Uninstall checks registered Task Scheduler actions as well as the saved XML definition, so loss of the definition does not authorize removing an executable still used by a task. Installer failure does not authorize restarting or stopping work automatically.

Native Windows x86_64 qualification covers package construction, Unicode installation paths, owned native launchers, in-use update refusal, retained directory identifiers, local named-pipe authentication boundaries and real Task Scheduler launch. A native desktop installed under a protected system prefix stores its WebView2 profile under the client data directory. Closing that window preserves the separate execution owner. Live OpenSSH checks cover installing a missing runtime from a verified archive, reconnecting after the preparing connection closes, enrollment and saved RPC. These checks use development GNU binaries; they do not qualify release signing, MSVC builds, ARM64 execution, unattended boot or external Harness authentication.
