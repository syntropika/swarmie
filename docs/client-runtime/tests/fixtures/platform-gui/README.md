# Native window and attachment acceptance

Install the fixture-local dependencies described in `../native-os/README.md` and run `../resources-gui/install.py` for fixture-local OCR. Build the actual daemon, Native SDK host and web assets, then start an isolated run:

```sh
dbus-run-session -- python3 tests/fixtures/platform-gui/acceptance.py \
  --artifacts /absolute/external/path/fresh-run \
  --media /absolute/path/to/controlled/media-fixtures
```

The runner owns a private Xvfb display, Openbox, two separate GTK windows, an actual daemon, and the Native SDK system WebView. The selected GTK window remains outside the host's viewport so native window capture sees its real pixels. Its AT-SPI controls share the daemon's private accessibility bus. All UI actions use OS input; screenshots use actual X11 client bounds. No browser automation or simulated window pixels are used.

JSON-line actions reuse the desktop fixture's input, capture, restart and RPC inspection commands. `assert-prompt` verifies the selected fixture conversation's actual user event, context markers and image count. File chooser input is a separate native dialog: explicitly focus its observed window before entering a location or selecting files. Do not refocus the application while operating the dialog. Additional GUI-created conversations have their own session journals; inspect the exact selected session when verifying their prompts.

The controlled scripted harness proves application attachment transport and context provenance. Compare dispatched image bytes and stored PDF/audio/video/opaque-file bytes with the original fixture files; do not infer provider media understanding. All source data, UI captures, journals and receipts remain outside the repository. Owned programs are closed on exit. The enclosing D-Bus session ends independently after the runner.
