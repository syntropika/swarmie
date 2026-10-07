# Native GUI remote workspace and simulator acceptance

This fixture runs the real Linux NativeSDK/WebKit window against a token-protected daemon on a configured macOS host, using the production `mpai remote ssh --reconnect` transport. The native GUI is a second private client: its token must be entered through Settings and Reconnect before workspace actions are usable. The actual daemon runs on macOS and all project files and PTY commands execute there.

Build current daemon/native host/frontend assets locally and a current macOS release on a configured macOS host. Synchronize current desktop assets into the remote source's `apps/desktop/dist` without rebuilding stale remote frontend source. Install the fixture-local OCR packages using `tests/fixtures/resources-gui/install.py` if needed, then run:

```sh
python3 tests/fixtures/remote-gui/acceptance.py \
  --host macos-test-host \
  --mac-source /absolute/remote/path/to/current/source \
  --remote-artifacts /absolute/remote/external/artifacts \
  --artifacts /absolute/local/external/artifacts/new-run
```

The host, current remote source checkout, and external artifact roots are explicit configuration inputs. The controller creates private synthetic tokens and isolated remote state. It selects the local forward port first and pins that exact authenticated HTTP loopback origin using `--public-origin`; the Mac daemon may choose a different listener port. No arbitrary origin or unauthenticated access is enabled.

Send JSON lines to operate actual OS controls:

```json
{"action":"capture","name":"initial-auth-settings"}
{"action":"click","x":20,"y":935}
{"action":"click","x":1180,"y":220}
{"action":"token-type"}
{"action":"click","x":1190,"y":266}
{"action":"capture","name":"remote-authenticated"}
{"action":"disconnect"}
{"action":"rotate"}
{"action":"token-type","generation":"replacement"}
{"action":"repair-cli"}
{"action":"remote-verify"}
```

Locate controls from native screenshots after each layout change. `token-type` sends the private generated token to the focused password field through stdin and records only its length. Use the private X11 clipboard for nonsecret long paths or commands; never seed actual credentials through the ordinary clipboard action. There is no browser automation, DOM inspection, CDP, Playwright, or JavaScript evaluation.

The acceptance scope includes GUI token entry, actual remote file editing and a real interactive PTY (`pwd`, `uname -s`, file writes), 390-pixel rendering, a terminated SSH child and production supervisor reconnect, rejected old credentials after token rotation, and explicit GUI reauthentication. Read-only `rpc` actions and `remote-verify` inspect resulting backend/native filesystem state; they must not replace GUI mutations.

For simulator proof, open Workspace tools → Devices through the GUI, select a simulator that was initially Shutdown, and register its ownership before clicking Open device:

```json
{"action":"owned-device","deviceId":"actual-selected-UDID"}
```

Capture to message must add an actual PNG attachment. Start and stop Live view through the native GUI; authenticated RPC frame polling must render actual simulator pixels over the existing SSH connection. The opaque GUI stream UUID is not inspected through browser internals; this fixture verifies visible start/stop, while separate backend tests verify rejection after stop. The controller restores only registered initially Shutdown simulators, recording their actual final state; existing running devices are never claimed or shut down.

End stdin to stop all owned local processes and the remote daemon and restore owned simulator state. Every report, image, trace, private token, local GUI cache, remote project and receipt lives under external artifact roots. Public receipts never contain token values. Private CLI connection/token files may contain credentials with mode 0600; GUI authentication remains in memory. This verifies cross-host remote workspace operations and simulator rendering, not installed harness authentication, physical Android devices, a native macOS desktop shell, or general internet hosting.

Retrieve the controlled remote `cleanup.json` into the local run as `mac-cleanup.json`, then verify the retained receipts:

```sh
python3 tests/fixtures/remote-gui/verify.py /absolute/external/artifacts/completed-run
```

The verifier checks the actual recorded actions, filesystem receipts, SSH replacement, native 390-pixel screenshot dimensions, GUI authentication outcomes, decoded persistent WebKit workspace storage, and owned cleanup. Inspect the simulator screenshots as well: text recognition alone cannot establish image content. A missing native Simulator application is reported separately from successful `simctl` boot/capture/live-frame support.
