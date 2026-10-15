# Native workflow acceptance

Run the built daemon and installed fixture Pi against a deterministic local Chat Completions peer:

```sh
python3 tests/fixtures/resources-acceptance/acceptance.py \
  --artifacts /absolute/path/outside/the/repository/fresh-directory
```

Prerequisites are `target/debug/swarmie`, Git, Python 3, and the Pi package installed under `tests/fixtures/harnesses`. The artifact directory must not exist. The script isolates HOME, Pi configuration, daemon state, project defaults, project and extension-managed worktrees. It never uses production provider credentials or a pre-existing daemon.

Assertions exercise a clock-fired one-shot prompt, actual Pi peer spawn/read/send, a native confirmation dialog with a queued second prompt, model-issued peer bridge operations, different-model comparison sessions in separate managed worktrees, guarded worktree cleanup, and observed token/cost aggregates retained through daemon restart. Native model pricing is explicitly configured in isolated `models.json`; the dashboard is checked against actual normalized usage events rather than estimated prompt length. Production authentication and native GUI rendering are separate acceptance concerns.

Controlled RPC/model receipts, hashes, logs, isolated native histories and result JSON remain in the supplied external artifact directory. The daemon and its child processes are shut down on both success and failure; no worktree is removed while an active session uses it.
