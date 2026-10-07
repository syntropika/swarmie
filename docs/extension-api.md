# Extension API

An external harness implements `mpai::Harness` and returns a `BackendConnection` with an `AgentBackend`, native session reference, capabilities, and an ordered `mpsc::Receiver<BackendEvent>`. The adapter crate has the same access to the core as any third-party crate.

`AgentBackend::prompt` returns when accepted. A native protocol that answers only at completion, such as ACP, should write the prompt and deliver its eventual completion through the ordered backend event stream. Never wait for a full model turn inside `prompt`, and never send completion through a separate channel that can overtake preceding content.

Backend events:

- `Content`: a semantic event plus optional native JSON. Core-owned lifecycle kinds are ignored.
- `TurnFinished`: the provider's terminal reason plus raw completion, emitted after all preceding turn content.
- `NativeSessionChanged`: a replacement native reference for future resume.
- `Exited`: an unexpected connection/process failure. The actor fails its active turn, saves the failed state, and ends the event stream.

Capabilities represent implemented support. Default methods reject unsupported operations. Steering is an instruction to active execution and is distinct from another prompt. A backend must not silently emulate steering when its native interface does not provide it.

`RuntimeExtension` offers `before_spawn`, `after_spawn`, `before_prompt`, `on_event`, `on_permission`, and `on_session_closed`. The runtime stores trait objects registered through `RuntimeBuilder::extension`; no dynamic loading or service container is needed.

`on_permission` returns `None` to leave a request open for a client, or a `PermissionResponse` selecting an offered option. The engine validates IDs/options before writing native responses. Requests can be answered once; active-turn requests are closed when that turn ends. A background request arriving after a parent turn ends remains independently answerable with no fabricated parent turn. Pending requests are cancelled when execution closes. `PermissionResponse.data` contains structured answers when the native request requires them. Original questions/options remain available through metadata and raw events.

`SessionStore` implements `save`, `load`, and `list`, with optional ordered event append/replay for durable transcripts. It can live in an independent crate. A store must use the runtime UUID as its key and retain the native provider reference unchanged. The engine surfaces save and journal errors as events; initial session-save failures return an error from spawn. Clients check advertised transcript durability before relying on restart replay.

The external-harness example is a separate workspace package depending only on `mpai`, async support, and serialization. It demonstrates registration, prompt middleware, ordered events, and cleanup without importing a built-in adapter.

## Extension commands

`RuntimeExtension::commands` returns public `ExtensionCommand` descriptors (`name`, `title`, `mutates`). `Runtime::extensions` exposes installed extension IDs and their commands. `Runtime::invoke_extension(id, command, arguments)` validates the advertised command before calling `RuntimeExtension::invoke`.

Commands execute outside the session actor's ordered hook pipeline, so filesystem or network work does not hold session events behind a hook. Extension implementations own command arguments, results, concurrency and resource lifetimes. Existing hook-only extensions remain valid with the default empty command list.

JSON-RPC clients use `extension/list` and `extension/invoke`. The latter takes `extensionId`, `command`, and an `arguments` object. No Git-specific method or type is added to the core protocol.

The optional `mpai-git` package implements `project.open`, `worktree.list`, `worktree.create`, `worktree.remove`, and `review`. Enable it in the daemon with `serve --git-worktrees DIR`. Opened repository paths serve as project IDs, and registered worktree paths serve as worktree IDs. Worktree creation uses a new branch and resolves an optional `base` to a commit before running Git. Removal is restricted to clean secondary worktrees inside the extension's managed directory and retains the branch. Review returns file status, a unified patch against HEAD, and explicit truncation flags. Git and its result schema belong entirely to this package.
