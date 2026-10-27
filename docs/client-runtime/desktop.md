# Desktop client

`apps/desktop-client` hosts the existing `apps/web-client` interface in a Native SDK
window. Its manifest owns the application identity, installation icon, window settings
and bundled frontend. Tembra continues to own frontend plugin state and subscriptions.
The desktop host does not implement another UI or state container.

The Native SDK CLI is pinned in the root Bun workspace. Zig is pinned in `mise.toml`.
Native generates its build graph under `.native/`; no framework source or generated build
graph is maintained in this application.

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

## Current boundary

This host prepares the native window, shared frontend and installation identity. It does
not yet embed the Rust runtime or supply a native Tembra connection adapter. Backend
availability remains explicit in the existing Connections and Providers APIs; packaging
the frontend does not make inference or local execution available. No placeholder backend
responses, credentials or development endpoints are bundled into the desktop application.
