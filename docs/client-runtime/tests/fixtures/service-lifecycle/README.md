# Native terminal opening acceptance

Build the daemon, frontend and native host, then run:

```sh
python3 tests/fixtures/service-lifecycle/native_open.py \
  --artifacts /absolute/path/outside/the/repository/native-open
```

The fixture uses its own daemon, state, resources, Xvfb display and private D-Bus session. It invokes the actual terminal CLI with an explicit native host, then through a packaged sibling layout. Both windows must display the requested session transcript and title. A project path containing spaces and query punctuation verifies URL encoding. Captures use X11 and the system WebView; the fixture owns and stops every launched process. It never changes default URL associations or the user's daemon. Text assertions require `tesseract`; use `--visual-only` when it is unavailable, then inspect both screenshots manually. That mode reports the visual review as pending.

Live service acceptance must separately run the actual user systemd or launchd manager. Inspect existing `swarmie.service` or `com.swarmie.daemon` state first and never replace or stop an existing user service. Check authenticated readiness, rejection without credentials, stop/start with a changed PID, exact durable snapshot and transcript preservation, and uninstall preserving credentials and state. Keep credentials private and all receipts outside the repository. On Linux, an isolated `XDG_CONFIG_HOME` unit may require an explicitly owned user-manager link; remove that link during cleanup without enabling the service.
