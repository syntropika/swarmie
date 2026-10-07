# Real remote workspace acceptance

Build the current daemon on Linux and macOS, then run against the configured SSH alias:

```sh
python3 tests/fixtures/remote-workspace/acceptance.py \
  --host macos-test-host \
  --remote-artifacts /absolute/remote/external/artifacts \
  --mac-binary /absolute/remote/path/to/current/mpai \
  --artifacts /absolute/local/external/path/fresh-run-directory
```

The host and remote artifact root are explicit configuration inputs. The remote binary must be an actual macOS build. The runner creates a unique controlled research directory on that host, launches the real daemon with the workbench extension and a private token file, and uses the production CLI `remote ssh --reconnect` supervisor. It does not install a user service, use an existing daemon or access a user project. SSH authentication remains the user's existing authorized SSH configuration.

Two separately persisted client registries pair/call the actual forwarded daemon. Assertions prove remote file reads/writes, an interactive `/bin/sh` PTY whose cwd is the remote project and whose uname reports Darwin, shared terminal access, a real SSH-child disconnect/reconnect with surviving remote state/PTY, missing/wrong bearer rejection, revocation by daemon token rotation/restart, explicit re-pair and local connection removal. Tokens are generated for the run, sent to the remote controller over SSH stdin and passed to local CLI processes via environment. They never appear in public arguments, terminal commands or saved receipts. Controlled private token files and registry credentials are mode 0600.

Remote/local daemon state, logs and receipts remain outside the project repository. `remote_host.py` owns the remote daemon and closes it when its SSH stdin channel ends, including failure cleanup. The local supervisor is interrupted and awaited. This fixture verifies actual remote backend boundaries and authenticated independent clients; native GUI/mobile rendering has separate acceptance ownership.
