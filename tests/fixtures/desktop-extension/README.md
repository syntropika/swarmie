# External desktop HTML extension fixture

This standalone Rust crate registers a real `mpai::RuntimeExtension` through public path dependencies on the core, protocol and workspace packages. It lives outside the root Cargo workspace. Its `kind: "web"` view returns an HTML file, and `window.mpai.invoke` calls its advertised commands through the desktop's scoped frame bridge.

Build the desktop frontend, then run from the repository root:

```sh
npm --prefix apps/desktop run build:web
cargo run --manifest-path tests/fixtures/desktop-extension/Cargo.toml -- \
  apps/desktop/dist tests/fixtures/desktop-extension/view.html \
  /tmp/mpai-extension-fixture-storage 127.0.0.1:7448
```

Point a built Native SDK host at the printed loopback URL using `MPAI_DESKTOP_URL` and `MPAI_DESKTOP_MANAGED=1`. Use an isolated profile when conducting acceptance tests. Open an ordinary folder through **Projects and worktrees**, create a chat, then open **Workspace tools** and select **External HTML fixture**.

The HTML automatically displays and records an own-command echo, rejection of the workspace extension's `files.list` command, inaccessible parent document/storage, frame storage isolation, native bridge visibility, and absence of a bearer literal in the frame document. The fixture writes receipt JSON to standard error; redirect logs outside the repository. These checks exercise the public author-facing `window.mpai.invoke` contract without a debugger or a browser automation API.

Copy `view.html` outside the repository for testing edits. Change both occurrences of `ONE` to `TWO`, then press **Reload view**. `view.html` is read afresh on each invocation, so the heading and roundtrip message must change without restarting the Rust process. This demonstrates HTML reload, not runtime loading or hot replacement of Rust libraries.

The minimal HTTP fixture binds only to a loopback IP, serves built frontend assets and `/rpc`, and rejects foreign Origin or Host values. To test an authenticated host, set `MPAI_FIXTURE_TOKEN` to a disposable test credential before starting the fixture, then enter it through the desktop's **Settings** panel. The frame receives neither the token nor a native SDK bridge. No model provider is connected. Stop the fixture with Ctrl-C to shut down the runtime and any workspace-owned resources. Screenshots, logs, receipts, temporary HTML edits and acceptance reports belong outside the repository.
