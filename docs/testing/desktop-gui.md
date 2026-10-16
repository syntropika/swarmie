# Native desktop acceptance fixture

This fixture launches the real Native SDK system WebView, a disposable real daemon,
a dynamically allocated Xvfb display and a private D-Bus session. It uses OS-level
`xdotool` input and `ffmpeg` X11 screenshots. It does not launch a browser automation
engine. Its mock, Codex and read-only GitHub peers are deterministic.

Build the daemon, native host and frontend first. Required executables are Python 3,
Xvfb, xdotool, ffmpeg and dbus-run-session. Optional tesseract enables screenshot OCR
assertions. The fixture snapshots frontend assets and records build fingerprints,
so later frontend builds cannot mix assets in a running acceptance session.

Run from the project root with a fresh artifact directory outside the repository:

```sh
python3 -u tests/fixtures/desktop-gui/acceptance.py \
  --artifacts /absolute/external/artifacts/native-gui/new-run
```

The process prints its private URL, session, display and window. Feed one JSON action
per input line, or pass `--script /absolute/path/actions.json` containing an array.
All children have disconnected stdin; ffmpeg uses `-nostdin`. End stdin to stop every
owned process group and write `cleanup.json`. Do not kill unrelated processes or use
the user's daemon on port 7447. Use a fresh directory for each independent run.
HOME, XDG configuration, storage and the configured Downloads directory belong to
that artifact directory. The mock accepts one additional prompt after its initial
transcript, allowing a GUI context submission to be checked in the real journal.

Example actions:

```json
[
  {"action":"capture","name":"initial"},
  {"action":"click","x":520,"y":780},
  {"action":"type","text":"A saved composer draft"},
  {"action":"key","key":"ctrl+p"},
  {"action":"type","text":"hello"},
  {"action":"wait","seconds":1},
  {"action":"capture","name":"finder"},
  {"action":"key","key":"Return"},
  {"action":"wait","seconds":1},
  {"action":"capture","name":"file-open"},
  {"action":"restart-host"},
  {"action":"capture","name":"restored"},
  {"action":"resize","width":390,"height":844},
  {"action":"capture","name":"mobile"}
]
```

Other actions are `drag` (`x`, `y`, `toX`, `toY`), `click` with `button:5` for wheel
down, and `rpc` with `method`/`params`. A key can be an array for repeated keys.
`clipboard-seed` with `text` and `clipboard-read` with optional `equals` use Gdk 4
through the system Python installation. The helper refuses the user's display and
requires the explicit dynamically allocated fixture display. Its seed owner is an
owned child; no clipboard command may target another display. These actions can
verify an actual user-initiated Copy button and an unchanged private clipboard after
an isolated frame's harmless handler probe.
The `--output-fixtures` directory may contain `fixture.pdf`, `fixture.png`,
`fixture.wav` and `fixture.webm`; they are copied into the disposable project and
advertised by real ordered `file.changed` mock events. These events prove output
discovery and rendering, not that a live model generated or understood the media.

Do not equate completed input actions with passed acceptance. Inspect screenshots
after discovery, edit, navigation, preview and restart. File finder queries require
time for the actual asynchronous result before Enter. Check the dirty bullet,
editor contents and enabled Save button before and after restarting. Verify actual
JavaScript state changes in the browser preview. For stateless Codex quota checks,
use New chat without sending a prompt, open Usage and click Read codex quotas;
compare session/list before and after and inspect `peer.jsonl` for account-only
requests. Configured keyboard shortcuts should be invoked outside their recorder.

Artifacts include screenshots, optional OCR text, action receipts, RPC receipts,
peer protocol calls, build hashes, isolated storage, host logs and cleanup status.
`capture` can specify `contains:["expected text"]` when tesseract is installed;
missing OCR never counts as a pass. Keep conclusions and screenshots outside the
repository. Report unsupported native capabilities and unavailable platform tools
with their exact visible errors, and leave untested provider or platform behavior
explicitly unverified.
