# Native Git workflow acceptance

This fixture drives the actual NativeSDK/system WebKit interface with OS input. It imports the shared desktop helper without changing it. An installed Pi CLI receives a real staged or branch diff through a deterministic localhost Chat Completions peer. The peer asserts that generation has no tools and that a custom recovered native configuration instruction reaches its system prompt.

The disposable repository and bare remote perform actual Git operations. A stateful `gh` peer records private body-file bytes/modes, draft creation, checks, comments, reviews and viewed-file state. Repository publication creates a local bare repository and performs an actual push. It never contacts a hosting service, uses a user credential or permits merge. All HOME/agent/state/model paths belong to the fresh external artifact directory; the GUI host retains real HOME only so WebKit's sandbox can enclose its private XDG paths.

Run from the project root:

```sh
python3 tests/fixtures/git-gui/acceptance.py --artifacts /absolute/external/fresh-run
```

Read the ready record, then send JSON lines to stdin. Shared actions include `click`, `key`, `type`, `capture`, `wait` and `restart-host`. Multiline text uses explicit OS Return keys. Additional actions are:

- `hosting-failure` with an `operation` such as `pr create`, `api graphql` or `repo create`: reject the next matching controlled hosting call.
- `seed-created`: after GUI creation of `gui-created`, seed its committed publication content as fixture setup.
- `verify`: require actual local/remote head equality, generated commit/PR text, comments/reviews and exact private PR body.
- `verify-projects`: require created/published/cloned head and file equality and an actual scratch directory.

`--script /absolute/actions.json` replays an action array and fails on action assertions. In text fields, `{{artifacts}}` expands to the fresh output path. Captures always require visual inspection when OCR is unavailable. The accepted replay, screenshots and reports belong outside the repository. Native coordinates describe the captured 1280×900 interface; inspect captures when layout changes.

EOF closes the native session and all owned process groups, stops the model server and checks recorded descendants. Cleanup receipts do not target the user's daemon or other applications. Python cache files should also be directed outside the repository with `PYTHONPYCACHEPREFIX`.
