# Workspace extension

`mpai-workspace` provides optional headless commands for ordinary folders, a file editor, real interactive PTY terminals, and project actions. Register `WorkspaceExtension::new()` through the public runtime builder, or enable the daemon's `--workbench` option. Desktop asset serving enables it automatically.

Discover its commands and optional presentation descriptors with `extension/list`. Invoke commands through `extension/invoke`, using `extensionId: "workspace"`. A project is opened with `project.open {path}`; its returned `id` is the canonical folder path. Reopen retained folder paths after a daemon restart. Terminals belong to the current daemon lifetime and close when their view or daemon closes. On Unix, closure terminates the foreground and shell process groups; on Linux it also terminates separate background job groups in the owned PTY session. Processes that deliberately create a new session are outside that terminal lifetime.

| Command | Arguments | Result |
| --- | --- | --- |
| `project.open` | `path` | `id`, `path` |
| `files.list` | `projectId`, optional relative `path` | Sorted `entries` with name, path, kind, size |
| `files.read` | `projectId`, relative `path` | UTF-8 `content`, `path` |
| `files.write` | `projectId`, relative `path`, `content` | `written` |
| `terminal.start` | `projectId`, optional `program`, string `args`, `cols`, `rows` | `terminalId` |
| `terminal.read` | `terminalId`, optional byte offset `after` | `text`, `next`, `availableEnd`, `first`, `truncated`, `exited`, `exitCode` |
| `terminal.write` | `terminalId`, `data` | `written` |
| `terminal.resize` | `terminalId`, `cols`, `rows` | `resized` |
| `terminal.close` | `terminalId` | `closed` |
| `actions.list` | `projectId` | Available Cargo checks/tests and package scripts |
| `actions.run` | `projectId`, discovered `actionId` | `terminalId` |

File commands reject parent traversal and paths resolving outside the opened folder. The editor reads and writes UTF-8 files up to 1 MiB and refuses writes through symlinks. A directory listing is limited to 2,048 entries.

Terminal reads preserve ANSI sequences for a terminal renderer. Output has a 1 MiB rolling buffer and is returned in at most 64 KiB chunks. Always pass the previous `next` offset. `truncated` means output was dropped before that offset. `exited` becomes true after the reader has finished and the reply has drained the available output. Close completed action terminals to release their slots; at most eight terminals are retained. Input supports interactive shells and full-screen terminal programs, with resize events and ordinary terminal control characters.

Project actions are discovered from the workspace files at invocation time. Package scripts use the package manager indicated by the lockfile. Commands run in the opened folder with the daemon's environment. They use the same terminal read, input, and close commands as interactive shells, so a headless client can run and observe them without a desktop panel.

The view descriptors use the public `files`, `terminal`, and `actions` kinds. A client may render these descriptions or use the commands directly.

## Project management and file search

`project.create {parent,name,initializeGit?}` creates a new folder, optionally initializing an unborn Git repository. `project.clone {parent,name,url}` clones into a newly owned folder. Existing folders are never replaced; a failed clone retains its folder and returns its path in the error. `project.scratch {}` creates a private managed scratch folder and returns `id`, `path`, `scratch:true`. Project registries are ephemeral; reopen a persisted folder path after a daemon restart.

`project.publish {projectId,name,visibility?,push?,description?}` invokes `gh repo create` with the opened source folder and an `origin` remote. Visibility defaults to `private`; `push` defaults to true. This is an explicit publishing operation requiring native `gh` authentication, not part of project creation or cloning. Errors report unavailable tooling or native failure. Tests exercise structured CLI arguments through an independent peer and perform no live publication.

`files.find {projectId,query?,limit?}` recursively returns `{paths,entries,truncated,scanned}` for quick file opening and mentions. Paths are relative to the opened folder. Searches inspect at most 20,000 entries and return at most 200 matches (default 50). Hidden folders, `.git`, dependency/build folders and symlinks are skipped. The search never follows a symlink outside the project.

## Binary assets and output references

`assets.write {projectId,name,data}` accepts base64 content up to 8 MiB decoded. It stores the file below the external private workspace state root, never automatically inside the repository. Its receipt includes `assetId`, absolute `path`, file `uri`, `name`, `size`, `mime`, `kind`, `previewable`, and image dimensions when available. Uploads and their handle index use private permissions on Unix and survive daemon restarts.

`assets.metadata {assetId}` inspects a retained asset; `assets.read {assetId}` also returns base64 `data`. For an existing project output, use `{projectId,path}` with a confined relative file path instead. Read content is capped at 8 MiB. Larger files can expose metadata with `previewable:false`. Recognized image, PDF, audio, video, text and HTML types supply preview metadata; PNG dimensions are extracted from native bytes. Clients decide how to present supported media and can pass returned paths to an agent without inventing a native binary prompt type.

`assets.export {projectId,path,data}` saves an explicit copy of base64 content to a relative workspace path, capped at 8 MiB. The parent directory must exist and remain inside the opened workspace. Existing files and symlinks are never overwritten. The desktop uses this command for the media preview's **Save a copy** action because the native WebView does not implement browser download handling.

State defaults to `$XDG_STATE_HOME/mpai/workspace` or `$HOME/.local/state/mpai/workspace`. Embedded users can choose a root through `with_storage_root`. Stored assets are retained user data; stopping a preview or stream does not delete an uploaded asset.

## Local browser previews

`preview.start {projectId,path?}` starts a real loopback static HTTP server for the opened folder or a confined subfolder, returning `{previewId,url,path}`. Index files and their local assets are served with appropriate content types. The server validates its Host header, rejects hidden paths, parent traversal and symlink escapes, bounds request headers, and limits individual files to 8 MiB. It does not execute project scripts or start a development build automatically.

`preview.status {previewId}` returns the live receipt; `preview.stop {previewId}` closes its listener. Up to eight preview servers are retained. Previews end with the daemon lifetime. A client can display the returned URL in its browser panel; an existing project development server can be opened separately by the client.

## Devices and native window context

`platform.capabilities {}` returns the operating system, optional executable availability, window backend and permission guidance. Missing integrations return useful failures and are not represented as working native support. Linux window selection requires X11; a Wayland-only session is unsupported. macOS capture and accessibility require operating-system permission. Windows operations require PowerShell and the native desktop APIs.

| Command | Arguments | Result |
| --- | --- | --- |
| `devices.list` | None | Native Android devices/AVDs and, on macOS, available iOS simulators; integration readiness |
| `devices.open` | `platform: android/ios`, native `id` | Open an existing device or boot/open a selected simulator/AVD; AVD startup returns `terminalId` |
| `devices.capture` | `platform`, `id` | Native PNG screenshot retained as an asset |
| `devices.stream` | `platform`, `id`, optional `intervalMs` | `streamId`, loopback `url`, `format: multipart/png`, bounded interval |
| `devices.stream.status` | `streamId` | URL and running state |
| `devices.stream.stop` | `streamId` | Stop listener, readers and their child capture processes |
| `windows.list` | None | Native window identifiers, titles, process IDs and available bounds |
| `windows.capture` | Numeric native `windowId` | Selected-window PNG asset |
| `windows.context` | Numeric native `windowId` | Selected-application accessibility/native window context |

Android commands use actual `adb` device selection and `emulator` AVD startup. iOS commands are gated to macOS and use `xcrun simctl` and Simulator. Screenshot streams repeatedly capture native PNG frames through a bounded loopback multipart service (up to four streams, two readers each), rather than claiming a platform video codec. Stop streams explicitly when closing their view. Screenshots are limited to 8 MiB; stalled consumers and malformed requests terminate their reader. Device operations need an installed toolchain and an available selected device; absent devices or authentication are native failures.

Window metadata uses CoreGraphics on macOS, `wmctrl` on X11, and Windows process/window handles. Capture uses `screencapture`, ImageMagick `import`, or Win32 screen-copy APIs. Context uses the selected window's process, not arbitrary applications: macOS System Events accessibility, X11 properties plus optional `python3-pyatspi`, or Windows UI Automation. Accessibility context is bounded and missing permission/tooling is reported explicitly. Native capture can depend on compositor visibility and system restrictions. Windows code paths are platform-gated; Linux protocol-peer tests and macOS host checks do not certify a Windows installation.

Additional view descriptors use `preview`, `devices`, and `windows`; clients may render them or invoke commands headlessly. Independent native-command peers validate binary capture/streaming and exact selected identifiers without reading private desktop content. macOS-specific checks run on a macOS host, never through a claimed Linux substitute.

On an Xcode installation without `Simulator.app`, `devices.open` still reports the accepted native simulator boot with `applicationOpened:false` and an explanatory `message`; the integrated screenshot/live view remains usable. When present, the application is resolved from the selected Xcode toolchain before falling back to its LaunchServices name. Opening a simulator does not claim that its boot has finished; capture can be retried after native startup.

A real macOS capture/stream test is available as `native_macos`. It is excluded from routine tests because it requires an explicitly selected installed simulator and can open Simulator. Set `MPAI_NATIVE_SIMULATOR_UDID` and run the ignored test on macOS. The test waits for native boot, verifies a PNG capture and multipart PNG frame, reads native CoreGraphics metadata, and restores the selected device's previous shutdown state. This test has also been exercised against an actual installed simulator, beyond the independent protocol peers.
