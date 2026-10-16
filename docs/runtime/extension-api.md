# Extension API

An external harness implements `swarmie::Harness` and returns a `BackendConnection` with an `AgentBackend`, native session reference, capabilities, and an ordered `mpsc::Receiver<BackendEvent>`. The adapter crate has the same access to the core as any third-party crate.

`AgentBackend::prompt` returns when accepted. A native protocol that answers only at completion, such as ACP, should write the prompt and deliver its eventual completion through the ordered backend event stream. Never wait for a full model turn inside `prompt`, and never send completion through a separate channel that can overtake preceding content.

Backend events:

- `Content`: a semantic event plus optional native JSON. Core-owned lifecycle kinds are ignored.
- `TurnFinished`: the provider's terminal reason plus raw completion, emitted after all preceding turn content.
- `NativeSessionChanged`: a replacement native reference for future resume. Emit it only when native identity changes; initialization frames confirming the current identity must not invalidate its policy or pending requests.
- `Exited`: an unexpected connection/process failure. The actor fails its active turn, saves the failed state, and ends the event stream.

Capabilities represent implemented support. Default methods reject unsupported operations. Steering is an instruction to active execution and is distinct from another prompt. A backend must not silently emulate steering when its native interface does not provide it.

`RuntimeExtension` offers `before_spawn`, `after_spawn`, `before_prompt`, `on_event`, `on_permission`, and `on_session_closed`. The runtime stores trait objects registered through `RuntimeBuilder::extension`; no dynamic loading or service container is needed.

`permission_authority()` declares the single host decision authority. More than one authority is rejected before native launch or preview. Only that authority receives `on_permission`; other extensions observe requests through `on_event`. An error leaves the request unresolved with no fallback. Explicit common `ask` and `review` selections prevent automatic approvals of unresolved native requests while preserving offered denials. See [execution policy](execution-policy.md).

`on_permission` returns `None` to leave a request open for a client, or a `PermissionResponse` selecting an offered option. The engine validates IDs/options before writing native responses. Requests can be answered once; active-turn requests are closed when that turn ends. A background request arriving after a parent turn ends remains independently answerable with no fabricated parent turn. Pending requests are cancelled when execution closes. `PermissionResponse.data` contains structured answers when the native request requires them. Original questions/options remain available through metadata and raw events.

`SessionStore` implements atomic `commit(SessionWrite)`, snapshot reads, a local catalog, guarded catalog changes, context preparation and retained-context reads. Convenience `save` and event append methods delegate to atomic commit; durable transcript reads and structured extension-state namespaces are explicit store capabilities. It can live in an independent crate. A store must use the runtime UUID as its key and retain the native provider reference unchanged. The engine surfaces save and journal errors as events; initial session-save failures return an error from spawn. Clients check advertised transcript durability before relying on restart replay.

The external-harness example is a separate workspace package depending only on `swarmie`, async support, and serialization. It demonstrates registration, prompt middleware, ordered events, and cleanup without importing a built-in adapter.

## Extension commands

`RuntimeExtension::commands` returns public `ExtensionCommand` descriptors (`name`, `title`, `mutates`). `Runtime::extensions` exposes installed extension IDs and their commands. `Runtime::invoke_extension(id, command, arguments)` validates the advertised command before calling `RuntimeExtension::before_invoke` and `RuntimeExtension::invoke`. When arguments contain a `sessionId`, the contextual hook receives the authoritative retained context and current availability. Filesystem commands should reject an unavailable directory and conflicting selections; host-only history commands can still proceed. The runtime does not turn this hook into a permission sandbox.

Commands execute outside the session actor's ordered hook pipeline, so filesystem or network work does not hold session events behind a hook. Extension implementations own command arguments, results, concurrency and resource lifetimes. Existing hook-only extensions remain valid with the default empty command list.

JSON-RPC clients use `extension/list` and `extension/invoke`. The latter takes `extensionId`, `command`, and an `arguments` object. No Git-specific method or type is added to the core protocol.

The optional `swarmie-git` package implements `project.open`, `worktree.list`, `worktree.create`, `worktree.remove`, and `review`. Enable it in the daemon with `serve --git-worktrees DIR`. Within this extension, opened repository paths serve as adapter IDs, independently of the host's stable Project IDs, and registered worktree paths serve as worktree IDs. Worktree creation uses a new branch and resolves an optional `base` to a commit before running Git. Removal is restricted to clean secondary worktrees inside the extension's managed directory and retains the branch. Review returns file status, a unified patch against HEAD, and explicit truncation flags. Git and its result schema belong entirely to this package.

Extensions must not change the resolved cwd, Project ID or Project revision in `before_spawn`. The host captures its selection before this hook and rejects changes before native connection. Public helpers expose directory registration, guarded Project/default updates, retained session context and extension namespace state. Credentials should remain outside public Project defaults and structured resource state.

## Agent-facing tool delivery

Extensions opt in through `RuntimeExtension::tools()` and `execute_tool()`. A `HostTool` declares an ID, description, input and result schemas, and mutation/open-world annotations. Input schemas must accept an object and reject undeclared arguments. Public commands are not automatically exposed as tools, and the default tool executor rejects unsupported operations rather than calling the legacy command executor.

A session explicitly grants tools through `execution.hostTools`. Each grant selects an extension/tool ID and may constrain argument values. All constraints must match; missing constrained arguments and values outside a grant are rejected before execution. Schema validation and host-owned context selection are independent of native approval checks. A native denial still prevents the tool from reaching this route.

The host installs a stdio MCP delivery template using `Runtime::install_host_tool_transport`. The runtime injects a private `swarmie_host` declaration for a granted session. Unsupported native MCP delivery fails explicitly. The generated declaration and its channel credential are not retained in public snapshots; grants are retained, and resume/fork create fresh channels. A conflicting user declaration of the reserved server is rejected.

The broker advertises names as `<extension>__<tool>`, while receipts retain separate extension and tool IDs. A prepared invocation captures its own ID, owning session and turn, and retained context binding. It cannot execute twice, enter another session, or execute under a later turn. Model arguments cannot select a session, Project, binding or working directory. An unavailable retained directory rejects execution.

`HostToolContext` supplies the authoritative `WorkingContext`, invocation receipt and cancellation token. Tool execution runs outside the session actor. Extensions must keep their execution asynchronous and respect cancellation before effects; child processes and background tasks require their own cleanup. The broker drops a cancelled execution future, bounds execution to 120 seconds, validates results, and discards results that race with cancellation. This does not undo completed effects or confine trusted extension code.

Cancelling or ending a turn cancels its prepared and running invocations. Closing the session, replacing native identity, losing the actor or encountering a transcript persistence failure revokes the channel. Native identity replacement requires a new connection/channel before further host tool delivery. Cancellation, state and native tool lifecycle events remain distinct facts. Invocation receipts are live connection state, not a durable recovery or replay log.

The HTTP daemon exposes a dedicated `/tools/rpc` endpoint. Its scoped credentials authorize only `tools/list`, `tools/prepare`, `tools/execute`, `tools/cancel` and `tools/status`. They do not authorize general daemon methods or arbitrary extension commands. The stdio bridge handles requests concurrently, correlates MCP cancellation notifications with prepared invocation IDs, and cancels outstanding host calls on normal input closure. Interrupted transport cleanup is bounded; commands also retain turn cancellation and execution deadlines in the host.

The initial tool is `workspace/read_file`: a bounded UTF-8 read relative to the session's retained directory. It uses the same file-reading implementation as the workbench command without requiring the model to open or select a Project. Native tools, the existing legacy command bridge, and direct native extension delivery retain their separate contracts. The broker does not establish confinement of native tools or qualify a second delivery mechanism.
