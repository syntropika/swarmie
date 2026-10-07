# Installed Codex exact skill-path acceptance

Build the daemon and install the native Codex CLI, then run:

```sh
python3 tests/fixtures/native-skills/acceptance.py \
  --artifacts /absolute/new/path/outside/the/repository/native-skills
```

The fixture launches the installed Codex app-server through the actual mpai Codex adapter. It creates a fresh configuration home, uses a loopback deterministic Responses endpoint with authentication disabled, and supplies no user credentials. Two discovered skills have the same name and different paths and instructions. Separate native threads select each path through typed skill input. The fixture verifies that both paths were listed by the actual app-server and only the selected skill's instructions reach the actual model request, then checks the completed response through mpai events.

The local Responses peer substitutes the model endpoint, not the Codex executable or app-server. This proves native skill discovery, exact path loading, model-request injection and runtime event delivery. It does not establish the behavior of a remote model or account authentication. Receipts record controlled instruction blocks and markers without HTTP headers or unrelated prompt content. All state, configuration and evidence remain outside the repository. The runtime and local endpoint stop during cleanup.

For native GUI acceptance, run `gui_acceptance.py` with the same arguments in an interactive terminal, or provide `--script /outside/the/repository/actions.json` containing an array of OS input actions. It opens the actual Native SDK system WebView with an actual Codex-backed thread. Use the composer's slash picker to select the second `same-name` skill and send the prompt, then send `{"action":"verify-model","selected":"second"}` to verify the selected file's instructions at the actual Responses endpoint. Other actions use the desktop GUI fixture's `click`, `type`, `key`, `wait` and `capture` operations. All interaction and capture use the X11 OS surface. The daemon and Codex use a fresh fixture HOME and CODEX_HOME; only the GUI host retains the real home needed by WebKit's operating system sandbox, with isolated XDG data/cache and a private D-Bus session. The real user runtime directory is preserved.
