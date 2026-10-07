# Architecture

Dependencies point toward `mpai`. Domain and session behavior consume public ports rather than harness protocols, process commands, storage implementations, or client transports. The bounded attachment utility reads local input files independently of session persistence.

```text
          CLI / stdio / WebSocket / HTTP
                       |
                   protocol
                       |
                     mpai
                   /      \
          Harness port   SessionStore port
                |              |
             adapters        storage
                |
             process
```

The workspace has six production crates. The core package is named `mpai` to avoid a collision with Rust's built-in `core` crate:

- `mpai` (`crates/core`): domain types, public ports, registry, session actor, lifecycle, hooks, and canonical event publication.
- `process`: process groups, bounded JSONL records, correlated native requests, deadlines, diagnostics, and supervised HTTP-server processes.
- `adapters`: Codex, Pi, Claude, OpenCode, and generic ACP protocol translation.
- `storage`: atomic file snapshots and ordered event journals with private file permissions.
- `protocol`: transport-independent JSON-RPC dispatcher and notifications.
- `app`: CLI and stdio/WebSocket/HTTP transport adapters.

A harness describes an installed implementation. A backend owns the connected process or API. A session is the runtime conversation handle, with a separate native provider reference. A turn receives a runtime UUID after native acceptance and ends once on native completion. The process lifecycle is independent from saved conversation history: shutting down a process does not delete its native conversation.

A session actor owns state, sequence, turn IDs, and pending requests. Provider content cannot manufacture core lifecycle events. Adapters translate native content and completion facts, retain raw frames, and expose capabilities. ACP prompt completion travels through the same native frame pipeline as its preceding content. Codex filters child-thread completion. OpenCode checks authoritative server status before accepting an idle notification.

Events flow from the actor into one ordered per-session history and broadcast stream. Global daemon notifications derive from that publication. A lagging consumer gets an explicit gap error or transport notification and can replay retained live events. A native protocol queue overflow is a fatal provider connection failure, rather than silent loss of permission or completion records. Each protocol record is limited to 8 MiB.

The extension interface is intentionally small: hooks may validate or alter spawn/prompt input, observe events in order, and resolve permission requests. Hooks run in the actor's ordered pipeline, so implementations must return promptly. Blocking hooks intentionally apply backpressure and may cause bounded native queues to fail. Extensions cannot override the engine's lifecycle invariants through content events.

Extensions can also advertise commands through the same public trait. Clients discover them with `extension/list` and invoke them with `extension/invoke`. Commands run outside the session actor's hook pipeline. The optional `mpai-git` package in `extensions/git` owns repository discovery, managed worktrees, and working-tree review; the daemon registers it only when configured. The desktop client in `apps/desktop` hosts a React frontend with assistant-ui and shadcn/ui in Native SDK's system WebView. It uses these commands and the existing session protocol over same-origin HTTP. The daemon serves frontend assets only when explicitly configured on a loopback listener. Git operations and desktop presentation remain outside the core.

File storage persists resumable references, metadata, queues, and the canonical ordered runtime event journal. The provider separately owns its native conversation history. Atomic rename and filesystem synchronization protect snapshots; load and journal failures remain explicit. Clients page durable events through `session/transcript` rather than depend on the bounded live buffer for restart recovery. The in-process registry retains weak handles, while the daemon retains open handles across client disconnects. Dropping the last in-process handle requests cleanup.

On Unix, every owned child is launched into a separate process group. Normal shutdown and forced termination clean up descendants. Drop signals the process group synchronously as well as notifying the asynchronous supervisor, so startup failures do not leave a server behind when the application exits.
