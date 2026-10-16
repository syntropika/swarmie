# Native resources GUI acceptance

This disposable fixture drives the real NativeSDK window through X11 input and captures its rendered pixels. It runs an installed native Codex CLI through the daemon against an isolated deterministic OpenAI Responses endpoint. Project defaults and the optional private credential vault are edited through the GUI. The fixture supplies native authentication in its isolated daemon environment. RPC is used only for controlled Git setup and read-only verification.

Dependencies are an installed Codex CLI, current daemon and desktop builds, Xvfb, xdotool, ffmpeg, `/usr/bin/python3` with GObject introspection, and fixture-local OCR packages. Extract extra OCR packages without modifying the system:

```sh
python3 tests/fixtures/resources-gui/install.py --receipt /absolute/external/artifacts/packages.json
python3 tests/fixtures/resources-gui/acceptance.py --artifacts /absolute/external/artifacts/new-run
```

The controller prints its isolated daemon URL, project, managed worktree, native configuration directory, native configuration JSON, and expected authorization hash. The synthetic credential remains in the private fixture file. Send JSON lines on stdin, for example:

```json
{"action":"click","x":1251,"y":18}
{"action":"capture","name":"workspace-tools"}
{"action":"clipboard-seed","text":"Nonsecret configuration JSON"}
{"action":"key","key":["ctrl+a","ctrl+v"]}
{"action":"secret-type"}
{"action":"restart-host"}
{"action":"resize","width":1400,"height":1000}
{"action":"inspect-private-storage"}
```

Use screenshots to locate controls after panels expand or scroll. Clipboard seeding is for nonsecret text only. `secret-type` sends the generated synthetic value to the focused native password field through stdin, recording only its length. Large JSON fields should use the private X11 clipboard instead of simulated typing. No browser automation, JavaScript evaluation, Playwright, or CDP is used.

The acceptance sequence is:

1. Open Workspace tools → Project settings. Set harness `codex`, model `gpt-5.4`, managed worktree `gui-defaults`, and the complete printed native configuration JSON. Set `approvalPolicy` to `on-request` and keep `sandbox` at `read-only`. Save project defaults. Expand Credentials and store the synthetic value under `gui-provider-key` to verify the separate private vault; this vault does not inject launch credentials.
2. Start a new chat and send `Verify the GUI project defaults.`. Capture `native-defaults-response` and read session events. The native configuration file has an unreachable fallback endpoint: successful execution proves GUI endpoint forwarding. Authentication comes from the daemon's fixture environment.
3. Restart the native window. Create a conversation, explicitly select the main project worktree in the Project pane and set its initial model to `gpt-5.2`; restart again, select this restored conversation, and send `Verify the restored conversation on the main workspace.`. Capture `restored-explicit-launch`, `defaults-native-response`, and `defaults-after-restart`.
4. Read `session/list`, both `session/events`, and `usage.dashboard` with the main project cwd, worktree cwd, and no cwd. In the GUI Usage pane capture `usage-main-model` with By model expanded, then `usage-all-models` with All projects selected. Select the first conversation, open its Usage pane, capture `usage-worktree-model`, and scroll the usage table horizontally to capture `usage-cost-columns`. Each scope shows 100 input / 10 output tokens; All projects shows 200 / 20 and two model rows. Costs remain visibly unknown because this native peer does not report cost.
5. Run `inspect-private-storage`, then end stdin to stop every owned process. Run `verify.py --artifacts /absolute/external/artifacts/new-run` to check the captured receipts and decoded WebKit localStorage.

The public receipts contain authorization presence and SHA256 only. The private credential store is intentionally a local file with mode 0600, not an OS keychain. Codex may write an environment snapshot containing credentials under its private mode-0700 configuration directory; the verifier reports these native-owned copies separately and does not claim they are encrypted or absent. This fixture verifies application snapshot/journal/localStorage redaction and credential forwarding, not upstream subscription authentication, billing, macOS rendering, or known native cost reports.

All sessions, credentials, caches, screenshots, receipts, Git projects, and managed worktrees live under the external artifact directory. The controller owns and terminates its daemon, native host, private D-Bus session, Xvfb, model server, and clipboard owners; it never connects to a user daemon.
