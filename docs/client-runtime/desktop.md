# Desktop client

`apps/desktop-client` hosts the existing `apps/web-client` interface in a Native SDK
window. Its manifest owns the application identity, installation icon, window settings
and bundled frontend. Tembra continues to own frontend plugin state and subscriptions.
The desktop host does not implement another UI or state container.

The Native SDK CLI is pinned in the root Bun workspace. Zig, Rust and packaging tools are
pinned at the repository root. A small owned `build.zig` uses the SDK's public build helper
and statically links the desktop updater and local runtime. Its SDK dependency resolves the installed npm
package; no upstream checkout, copied framework or separate plugin loader is used.

## Development and qualification

Run commands from the repository root after the frozen dependency installation:

```sh
just desktop-dev
just desktop-build
just desktop-check
```

Development starts the shared client through the manifest's managed development server.
React Fast Refresh updates component edits without restarting the native window. Edits
outside a refresh boundary can reload the page; native host changes require rebuilding.
The development and build commands prepare the shared generated UI before loading it.
The Native SDK closes the development server when the desktop host exits. The installed application
loads the frontend from its bundled `dist/` directory through `zero://app`; it does not
need a frontend development server.

The system WebView is used on each platform. Linux builds require GTK 4 and WebKitGTK 6
development libraries. macOS builds require Xcode command line tools. Windows builds
require the platform prerequisites supported by the pinned Native SDK release.

`desktop-check` validates the manifest and icon, then runs the native source-selection
tests with the SDK's headless platform. A successful headless test or frontend export
does not qualify native window behavior or another operating system.

## Application icon and packaging

The `icons` field in `app.json` points to `assets/icon.png`, a square 1024px Swarmie
application icon matching the mobile identity. Native generates platform icon formats
from that source: macOS bundle icons, Windows icon resources and Linux launcher images.
The same asset is supplied to the running desktop host.

Build and package on the operating system being targeted:

```sh
just desktop-package linux
just desktop-package macos
just desktop-package windows
```

Packages are written below the desktop application's ignored `zig-out/` directory.
macOS signing and notarization, Windows installer distribution and release publication
are separate distribution steps; producing a local package does not perform them.

## Tagged releases and updates

The [desktop release workflow](../../.github/workflows/desktop-release.yml) runs when a
`vMAJOR.MINOR.PATCH` tag is pushed. Prerelease suffixes such as `-beta.1` are supported.
The tag sets the package version before building; the Native SDK manifest uses its required
numeric MAJOR.MINOR.PATCH form, while Velopack retains the complete prerelease version.
All four targets must succeed:
Linux x64, Windows x64, macOS Apple Silicon and macOS Intel. The workflow assembles a
draft, uploads every installer, full update package and architecture feed, then publishes
the completed GitHub release. It does not rewrite an already published release.
Prereleases do not become the stable update source. Manual workflow runs build artifacts
without publishing a release or creating a tag.

Velopack owns installation and updates on all three operating systems. Linux uses an
AppImage, Windows an installer/portable distribution, and macOS an application installer
and portable bundle. The full package contains the executable and frontend assets, so
replacement cannot leave the old UI paired with the new host. Linux still requires the
system GTK 4 and WebKitGTK 6 runtime libraries. The Windows installer bootstraps the
Visual C++ 2022 runtime and Edge WebView2 when missing. Windows executables use the
same dynamic Microsoft C runtime as the Rust library. Portable Windows packages require
these system dependencies to be installed already.

Installed applications check the stable GitHub Release source in the background when
launched. A native prompt offers **Update** or **Later**. Only Update downloads and stages
the new version, then requests a graceful application exit and relaunch. Checks never
block first rendering. Download/staging failure leaves the current application running.
Downloaded updates are not applied automatically on a later startup without consent.
Development mode and unpackaged binaries do not check or install updates.

The SDK's macOS-only updater is intentionally not enabled alongside Velopack. The browser
client and mobile application do not run the desktop installer. The update source is a
build-time `SWARMIE_UPDATE_URL`; the default is the application's GitHub Releases URL.
No GitHub token or signing private key is embedded in the application. Release assets
must be readable by installed clients; private downloads require a separate distribution
source rather than embedding repository credentials.

To build the release assets locally, install the pinned .NET SDK, then run from the root:

```sh
dotnet tool install vpk --version 1.2.161 --tool-path target/desktop-tools
bun scripts/distribution/desktop.ts prepare v0.1.0
just desktop-build
bun run package:desktop:release v0.1.0
```

Version preparation changes the checkout's manifest. CI performs it in an isolated checkout.
Generated installers, update packages and feeds are under `target/desktop-release/<target>`.
`just desktop-check` covers consent/failure behavior, tag identity and native host tests.
The workflow also exercises installer activation callbacks before packaging.

### Optional signing

Signing is disabled unless configured. No local certificate is exported automatically.
macOS builds remain ad-hoc signed when no Developer ID identity is supplied; operating
system installation warnings and restrictions still apply. This is distinct from successful
packaging or update-package checksum verification.

To enable macOS application signing later, configure repository variable
`DESKTOP_MACOS_SIGNING_IDENTITY`, and secrets `DESKTOP_MACOS_CERTIFICATE_P12` (base64 PKCS#12)
and `DESKTOP_MACOS_CERTIFICATE_PASSWORD`. The workflow imports them into a temporary
keychain, signs through Velopack, and removes temporary signing material. The variable
must remain unset until both secrets exist. Certificate export, installer signing,
notarization and Windows Authenticode credentials are separate opt-in provisioning steps.

## Local execution node

`swarmie-node` supplies the worker and byte channels for the embedded local node.
`apps/desktop-client/runtime` supplies the Desktop C ABI and WebView adapters.
The native host owns its worker thread, Tokio executor, profile lock,
node identity file and state directory. It installs the
[node capabilities](node-capabilities.md) from their owning capability packages.
Each capability declares its own services; the host selects and installs them.
Tembra owns capability resources and physical cleanup.
No standalone runtime process or listener is required.

The node has a persistent identity and a local `desktop-user` principal. A file lock
permits one host to write its state directory. MCP definitions are stored per principal
in a JSON file. Writes compare the expected inventory and replace the file through a
temporary file. An unreadable file is reported; it is not replaced with empty settings.
Connection state and tool receipts stay in memory. They do not survive a restart.

The `swarmie.desktop.runtime` bridge command carries Tembra RPC frames. The main
WebView is permitted to use this command from the bundled application origin or the
managed development origin. Other windows and WebViews are refused. The browser
adapter verifies node identity through the ordinary Connections interface. Channels
have distinct generations. Old frames cannot enter a replacement channel.

The bridge bounds frames, pending calls, references and output queues. Its C ABI poll
requires an output buffer of at least 512 KiB; smaller buffers are refused before queued
frames are consumed. Polling drains a bounded batch independently for each channel.
It does not retry capability mutations. Page reload closes the old RPC channel. Capability
resources remain under Tembra ownership; a page reload does not authorize tool replay.
Application exit closes the channel and waits for the host to close.

**Settings → Plugins → MCPs** configures local commands and remote HTTP servers on
this node. See [MCP](../plugins/mcp.md). The host also composes provider settings,
native authorization, Conversations storage and the existing Agent Loop. Provider secrets
remain in native OS credential custody. Bounded metadata and authorization instructions
cross the bridge; returning-account instructions may contain a signed identity-token hint
intended for the provider. Access and refresh tokens stay native.
Browser and mobile clients require an execution node exposing these capabilities.


## Node directory browser

The client node composition supplies `swarmie.directories` through the host's Tembra byte channel.
The native embedding uses the operating system user home as the permitted root. It reads
`HOME`, or `USERPROFILE` when `HOME` is absent. Without either value, it does not install
this service. The service accepts only the bound desktop principal.

The directory reader resolves paths and checks them against that root. It omits files
and links that resolve outside the root. Parent navigation stops at the root. Reads run
on the host's blocking executor. They do not write to the filesystem. Each listing scans
at most 4096 entries. A page contains at most 64 directories and has a 24 KiB encoding
budget. Unsupported path encodings, inaccessible directories and exceeded limits return
errors. Directory results are live reads, not a filesystem snapshot or sandbox guarantee.

The web client uses this service for Add folder in Projects. The browser device does not
supply the directory tree. Changes to native services require a desktop runtime rebuild.
