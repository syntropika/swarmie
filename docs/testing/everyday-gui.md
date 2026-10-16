# Native everyday GUI acceptance fixture

This fixture extends the desktop GUI helper with a disposable Git project, a real modified working file, a text attachment, deterministic acknowledgement turns, and a final permission turn. It launches the actual Native SDK app in a private Xvfb and private D-Bus session, using its system WebView. Input uses xdotool; screenshots use ffmpeg. It does not launch a browser or inject JavaScript.

Run from the repository root with an external artifact directory:

```sh
python3 -u tests/fixtures/everyday-gui/acceptance.py --artifacts /absolute/external/artifacts/everyday-gui/new-run
```

Send one JSON action per stdin line. Base actions include click, key, type, drag, resize, capture, wait, rpc, restart-host, and private clipboard operations. Additional actions:

- `inspect-state`: read the actual owned WebKit local-storage workspace and save it under `name`.
- `assert-prompt`: assert the latest real daemon user event using `contains`, `excludes`, or `equals`; save the event under `name`.
- `seed-timers`: stop only the owned host, seed two existing conversations with future snooze expiries, and restart. Observe both expiry transitions without UI input.
- `seed-age-boundaries`: stop only the owned host and seed two unsent GUI-created conversations near the one-day and seven-day boundaries. Explicit empty panels prevent unrelated initial layout reconciliation from changing the fixture timestamps. Select Recent activity before this action, then observe both transitions without UI input.

Use a new artifact directory for each run. The helper snapshots frontend assets and records executable and asset hashes. A restarted window must belong to the current owned host process group; a closing previous GTK window is ignored. EOF invokes guarded cleanup for all owned children. Inspect cleanup.json and check the recorded process groups after completion.

The controlled mock peer is a fixture, not authenticated upstream-provider evidence. Assertions use actual daemon journal events; persisted context state separately records message, terminal, and review provenance. Save screenshots and all run receipts outside the repository. Native dialogs require observing the actual dialog before entering a fixture path. Wait for typed text to appear before sending; OS input can queue under load.
