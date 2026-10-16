# Native harness fixtures

The deterministic Python peers in `tests/fixtures` exercise native protocols without credentials. They cover ordering, interruption, approval responses, retry outcomes, process exit, and HTTP/SSE lifecycle behavior. Run them from the repository root with `mise exec -- just test`.

## Install live harnesses

```sh
npm ci --prefix tests/fixtures/harnesses
cargo build --workspace
```

Pi, OpenCode, and Qwen Code are pinned in the fixture package and installed only in its ignored `node_modules`. Claude Code and Codex use an existing CLI installation from `PATH`. Production crates do not depend on these fixture packages. For manual CLI runs, add the fixture binaries to `PATH` or pass their absolute executable paths in spawn configuration.

```sh
export PATH="$PWD/tests/fixtures/harnesses/node_modules/.bin:$PATH"
```

Qwen Code 0.24.7 exposes native ACP and supports both `--auth-type=openai` and `--auth-type=openai-responses`. Its custom endpoint settings use `OPENAI_API_KEY`, `OPENAI_BASE_URL`, and `OPENAI_MODEL`; `QWEN_HOME` isolates native state. Installed CLI verification covers text and a real native `read_file` tool loop against deterministic loopback peers for both protocols. This validates native protocol behavior, not a production account or arbitrary model compatibility. Bare-mode authentication in this version can fail when writing isolated settings; the verified path uses regular safe mode. The existing live runner below targets Codex, Pi, Claude, and OpenCode; it does not run the separate Qwen or Gemini protocol checks.

Gemini CLI 0.35.3 uses native ACP and Google `streamGenerateContent`. `GEMINI_API_KEY`, `GOOGLE_GEMINI_BASE_URL`, and `GEMINI_CLI_HOME` select an isolated API-key test configuration. Installed CLI checks cover text and a native `read_file` loop against a deterministic Google-protocol peer. This version's ACP omits the read result and usage counters from client events even though the actual file content reaches the model; the adapter does not invent those fields.

## Test a compatible model endpoint

Set these environment variables in your local shell. Keep credentials out of source files, command arguments, and native spawn configuration.

```sh
export SWARMIE_FIXTURE_BASE_URL=http://127.0.0.1:8091
export SWARMIE_FIXTURE_MODEL=your-model-id
read -rs SWARMIE_FIXTURE_API_KEY
export SWARMIE_FIXTURE_API_KEY
python3 tests/fixtures/live.py
```

`SWARMIE_FIXTURE_BASE_URL` is the server origin without `/v1`. Pi uses its Responses API directly. OpenCode uses Chat Completions directly. Claude uses the local fixture proxy, which translates Anthropic Messages and tool calls to Chat Completions. Claude uses bare mode with an empty tool catalog and OpenCode uses a short native agent prompt with tools disabled to bound smoke-test prompt size; Pi exercises native tool execution separately. An endpoint used for all three needs both API routes and a model catalog. The proxy binds only to loopback and is started and stopped by the runner.

The runner drives the real harnesses through the stdio daemon: two turns, a daemon restart, and native session resume. Pi also reads a real fixture file using its native tool. Each turn must end successfully with the expected model text and strictly ordered event sequences. A failure exits nonzero. Credentials remain in the environment; generated native configuration refers to the environment variable.

Select individual harnesses or use the installed Codex authentication:

```sh
python3 tests/fixtures/live.py --harness pi
python3 tests/fixtures/live.py --harness claude --harness opencode
python3 tests/fixtures/live.py --harness codex
```

Evidence and generated state go outside the repository under `<system-temp-directory>/swarmie-live-tests/live-TIMESTAMP/`. Override this with `--artifacts /absolute/external/path`. `--timeout` sets each request/turn deadline. Model behavior and endpoint compatibility can cause live tests to fail independently of the deterministic protocol tests.

For manual bridge testing:

```sh
python3 tests/fixtures/inference_proxy.py --port 8092
```

The proxy requires `SWARMIE_FIXTURE_API_KEY`, reads the same base URL/model variables, and records no request headers, prompts, or responses. It is a text/tool testing fixture; it is not a production gateway and does not translate images or arbitrary provider extensions.

## Use the proxy from the desktop GUI

Build the desktop frontend as described in `docs/desktop/README.md`. Set the fixture endpoint, model, and key in the first terminal, then keep the proxy running:

```sh
python3 tests/fixtures/inference_proxy.py --port 8092
```

Stop the existing local daemon when its turns are idle. In a second terminal, start the GUI daemon with Claude's gateway configuration, replacing `your-model-id` with the upstream model ID:

```sh
env -u ANTHROPIC_AUTH_TOKEN -u CLAUDE_CODE_OAUTH_TOKEN -u CLAUDECODE \
  SWARMIE_CLAUDE_REAL_EXECUTABLE="$(command -v claude)" \
  SWARMIE_CLAUDE_EXECUTABLE="$PWD/tests/fixtures/gui-bin/claude" \
  PATH="$PWD/tests/fixtures/gui-bin:$PATH" \
  ANTHROPIC_BASE_URL=http://127.0.0.1:8092 \
  ANTHROPIC_API_KEY=fixture-local \
  ANTHROPIC_MODEL=your-model-id \
  CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 \
  ./target/debug/swarmie --state-dir "${TMPDIR:-/tmp}/swarmie-gui-fixture/sessions" \
  serve --http 127.0.0.1:7447 --desktop-assets apps/desktop/dist \
  --git-worktrees "${TMPDIR:-/tmp}/swarmie-gui-fixture/worktrees"
```

Open or reload the desktop window, create a new chat, and choose **claude** in the composer. Leave the model field empty to use `ANTHROPIC_MODEL`. The fixture wrapper launches the real Claude Code CLI in bare mode with a short system prompt and the Read, Edit, Write, and Bash tools. Its structured protocol and permission flow still run through the real harness; the upstream endpoint supplies model inference. Set `SWARMIE_CLAUDE_TOOLS` to a comma-separated tool list to adjust the fixture catalog. Bare mode skips automatic hooks, plugins, memory, and instruction discovery. Model behavior and tool compatibility depend on the selected upstream model. This bounded test configuration does not establish full coding-agent equivalence with the default Claude configuration.

The dummy Claude key is consumed by the local proxy. The upstream credential stays in the proxy's environment, and the saved Claude OAuth login is unchanged. Keep the proxy running for the duration of the GUI test. These settings apply only to this daemon launch; restart the daemon without them to return Claude to its usual provider configuration.

## Remote transport qualification

`crates/app/tests/node_local_transports.rs` covers private local sockets, the stdio byte
proxy, SSH child ownership, reconnect identity checks and execution survival after
disconnect. Desktop unit tests cover guided SSH preparation and enrollment. These tests
do not establish live remote platform or graphical qualification.

Remote acceptance must exercise node enrollment, files and interactive terminals through
the native client gateway and a saved SSH proxy route. Use disposable state and working
directories with an existing remote runtime listener. Verify shared execution across
clients, process cleanup, interruption without prompt replay, identity continuity and
preserved user resources. See [runtime connectivity](../runtime/runtime-connectivity.md).
Keep operational evidence outside the repository.
