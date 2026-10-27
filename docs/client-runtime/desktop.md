# Desktop client

`apps/desktop-client` hosts the existing `apps/web-client` interface in a Native SDK
window. Its manifest owns the application identity, installation icon, window settings
and bundled frontend. Tembra continues to own frontend plugin state and subscriptions.
The desktop host does not implement another UI or state container.

The Native SDK CLI is pinned in the root Bun workspace. Zig, Rust and packaging tools are
pinned at the repository root. A small owned `build.zig` uses the SDK's public build helper
and statically links the desktop updater. Its SDK dependency resolves the installed npm
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
system GTK 4 and WebKitGTK 6 runtime libraries.

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

## Current boundary

This host prepares the native window, shared frontend and installation identity. It does
not yet embed the Rust runtime or supply a native Tembra connection adapter. Backend
availability remains explicit in the existing Connections and Providers APIs; packaging
the frontend does not make inference or local execution available. No placeholder backend
responses, credentials or development endpoints are bundled into the desktop application.
