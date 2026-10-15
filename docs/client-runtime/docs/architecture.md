# Architecture

Dependencies point toward `swarmie`. Domain and session behavior consume public ports rather than harness protocols, process commands, storage implementations, or client transports. The bounded attachment utility reads local input files independently of session persistence.

```text
          CLI / stdio / WebSocket / HTTP
                       |
                   protocol
                       |
                     swarmie
                   /      \
          Harness port   SessionStore port
                |              |
             adapters        storage
                |
             process
```

The workspace has six production crates. The core package is named `swarmie` to avoid a collision with Rust's built-in `core` crate:

- `swarmie` (`crates/core`): domain types, public ports, registry, session actor, lifecycle, hooks, and canonical event publication.
- `process`: process groups, bounded JSONL records, correlated native requests, deadlines, diagnostics, and supervised HTTP-server processes.
- `adapters`: native Codex, Pi, OpenCode, and Antigravity protocol translation, Claude Agent SDK worker integration, Cursor SDK Bridge integration, and named/generic ACP integrations. See [agent tool support and setup](coding-agent-support.md) for provider transports and runtime support assets.
- `storage`: transactional SQLite host state, stable store/Environment identities, bootstrap settings, and private state permissions.
- `protocol`: transport-independent JSON-RPC dispatcher and notifications.
- `app`: CLI composition with separate `src/runtime/` transport/lifetime and `src/client/` native connection/persistence module groups. Execution and client ownership remain distinct even though their composition shares a crate.

A harness describes an installed implementation. A backend owns the connected process or API. A session is the runtime conversation handle, with a separate native provider reference. A turn receives a runtime UUID after native acceptance and ends once on native completion. The process lifecycle is independent from saved conversation history: shutting down a process does not delete its native conversation.

A session actor owns state, sequence, turn IDs, and pending requests. Provider content cannot manufacture core lifecycle events. Adapters translate native content and completion facts, retain raw provider payloads, and expose capabilities. SDK integrations retain public SDK messages; the SDK owns its private CLI wire. Runtime-owned support processes provide the SDK language boundary on the execution node, independently of desktop and browser lifetimes. ACP prompt completion travels through the same native frame pipeline as its preceding content. Codex filters child-thread completion. OpenCode checks authoritative server status before accepting an idle notification.

Events flow from the actor into one ordered per-session history and broadcast stream. Global daemon notifications derive from that publication. A lagging consumer gets an explicit gap error or transport notification and can replay retained live events. A native protocol queue overflow is a fatal provider connection failure, rather than silent loss of permission or completion records. Each protocol record is limited to 8 MiB.

The extension interface is intentionally small: hooks may validate or alter spawn/prompt input, observe events in order, and resolve permission requests. Hooks run in the actor's ordered pipeline, so implementations must return promptly. Blocking hooks intentionally apply backpressure and may cause bounded native queues to fail. Extensions cannot override the engine's lifecycle invariants through content events.

Extensions can also advertise commands through the same public trait. Clients discover them with `extension/list` and invoke them with `extension/invoke`. Commands run outside the session actor's hook pipeline. The optional `swarmie-git` package in `extensions/git` owns repository discovery, managed worktrees, and working-tree review; the daemon registers it only when configured. The desktop client in `apps/desktop` hosts a React frontend with assistant-ui and shadcn/ui in Native SDK's system WebView. The managed desktop uses an application IPC bridge and a client-owned native gateway with persistent runtime channels. Private Unix sockets and supervised SSH byte proxies carry the same WebSocket-framed protocol; direct HTTP(S)/WS(S) routes remain available. Browser development access uses same-origin HTTP. The daemon serves frontend assets only when explicitly configured on a loopback listener. Git operations and desktop presentation remain outside the core.

The SQLite store persists resumable references, metadata, queues, submission evidence and the canonical ordered runtime event journal under a configurable application root. Bootstrap settings locate and pin the database; they do not duplicate session or Project preferences. The default root is `~/.swarmie`, independent of execution directories and native-owned history. A configured missing database is an explicit failure, not a request to create replacement history.

`SessionStore::commit(SessionWrite)` is the atomic storage seam. Runtime events commit with their corresponding snapshots before publication. Queue and live metadata operations roll back together with their events on failure. Native import and fork inheritance commit their host projection together. Submission intent and attempt evidence are retained before native dispatch; terminal facts precede any recovery projection. No database transaction spans a native call. SQLite uses WAL, full synchronization, foreign keys, retained-event guards and private files; blocking engine work runs outside async executor threads. The memory adapter uses one lock for the same grouped writes.

Clients page durable events through `session/transcript` rather than depend on the bounded live buffer for restart recovery. The in-process registry retains weak handles, while the daemon retains open handles across client disconnects. Dropping the last in-process handle requests cleanup. The host also owns immutable directory resources, Project and default-directory revisions, retained session contexts, and structured extension namespaces. The resources extension stores its operational records in the host database; its credential vault remains separate.

On Unix, every owned child is launched into a separate process group. Normal shutdown and forced termination clean up descendants. Drop signals the process group synchronously as well as notifying the asynchronous supervisor, so startup failures do not leave a server behind when the application exits.

## Local Projects and execution contexts

A Project has a stable identity and links a node-local directory resource. Its name, archive state, directory selection and nonsecret launch defaults advance together under an expected-revision guard. Unlinking or archiving a Project retains prior revisions and user files. Directory registration records a canonical locator and continuity evidence without copying or owning the folder.

Before native creation, the host durably prepares a `WorkingContext` with separate context, session and binding IDs. It retains the directory record and selection provenance: a Project revision, a project-free default revision, an explicit launcher directory, or native-existing history. Concurrent sessions can share one directory. Changes to Project or default selections affect new preparations; resume and fork use the retained original selection. A fork receives new context/session/binding IDs.

On Unix, continuity checks compare the canonical locator, device/inode identity and the observed creation timestamp when available. Missing or replaced folders block native connection and new prompt/steering dispatch. Approval responses that continue execution and filesystem rollback also require the retained directory. History and stored metadata remain readable. These checks are observations before operations, not filesystem leases, access controls or guarantees against a concurrent replacement after a check. Non-Unix directory qualification is explicitly unsupported until a platform-specific identity mechanism is implemented.

Extension invocations that provide a `sessionId` receive the authoritative `ContextStatus` through `before_invoke`. Built-in workspace and Git consumers check directory availability and reject conflicting filesystem selections; resource discovery resolves the retained session directory. Extensions decide which commands require a directory, so saved templates, history and other host-only data can remain available. Direct invocations without a session are explicit extension operations. External extensions must implement the contextual hook for their filesystem consumers; it is not a sandbox.

The initial SQLite catalog uses immutable revision rows and mutable heads. Context preparation is committed before a native effect; session snapshots are subsequently committed with their event and submission records. Extension namespace writes are individually atomic but do not form one transaction with Project edits or native execution. Native handshake failure can leave a retained preparation with no public snapshot; it does not create visible conversation history. Stronger isolation, collaboration, cloud allocation and universal native directory recovery remain separate work.

## Execution policy and native evidence

The host retains common execution intent separately from planned native settings and scoped native observations. Adapters compile profiles and report equivalence, application scope and coverage through one port; clients consume server choices and generic native alternatives. Policy mutations durably record uncertainty before native effects, and partial application or unqualified replacement blocks dependent dispatch. A single declared host permission authority can handle surfaced requests without overriding explicit per-session approval intent. See [the execution-policy contract](execution-policy.md).

## Personal nodes and Environments

An Environment is a prepared execution node, independent of Project identity and working-directory selection. Initialization exposes its retained identity and the live owner incarnation. Saved remote connections pin the retained identity; receiving owners reject stale `expectedRuntime` targets before dispatch. See [nodes and Environments](nodes-and-environments.md) for the implemented identity boundary, current desktop lifetime, and ordered connection/recovery work.

The client's connection manager retains node identities, transport routes, historical verification times, and a selected-node preference in the configured root's SQLite database. Desktop startup automatically records the verified local node. Multiple routes share one retained node; transactional revisions guard concurrent changes and stale reconnection after removal. Credentials use opaque references to separate private files. Removing a route retains the node identity without changing destination Projects or execution. The destination Runtime remains authoritative for its own records. The desktop gateway exposes local node administration and forwards the common protocol to verified registered destinations without exposing remote credentials to the browser.

Client state instances are pinned to retained node identity and a fixed same-origin RPC path. Each instance owns its Project/chat catalog, selection, live observations, submission tracking, and pending work. Local storage and IndexedDB namespaces include the full Environment/store/owner tuple; cache envelopes are checked against that tuple before hydration. Routes retain a separate association for offline reopening. An owner restart does not change the cache namespace. RPC responses and transport errors from an obsolete attachment cannot update the current connection observation. Notifications also include the node identity. The desktop coordinator reuses one presentation instance per node. Managed clients receive native connection observations and query authoritative events and receipts; browser development clients poll hidden connected instances. Selection changes the viewed catalog and new-work destination; existing chats retain their owner. Scoped remote preview routes preserve resource URLs, redirects, and content policies. Browser caches do not provide cross-client synchronization or concurrent-window draft conflict resolution.

## Runtime and client lifetime

The Environment is a prepared execution node; preparation recipes and working directories are resources on that node. A Runtime owns native execution independently of connected clients. The desktop launcher discovers and verifies an existing owner or starts a detached loopback HTTP owner. Simultaneous launchers serialize bootstrap, while the retained owner lock enforces one native writer for the store. Discovery records are private connection metadata, not authority or liveness evidence.

The desktop is an ordinary common-protocol client. Closing it preserves ongoing turns and other clients' sessions. Explicit owner shutdown drains accepted requests and closes native processes without deleting retained history or user resources. Running-session attachment reuses the same actor even when its Harness cannot restart a closed native session. Connection failures preserve readable history and drafts; reconnection verifies identity and rebuilds observation without resending prompts. OS service supervision, allocation lifetime, remote reachability, and cross-client draft sharing remain separate concerns.

## Accepted connectivity and distribution direction

Client-owned node connections are separate from runtime execution ownership. Persistent bidirectional RPC/event channels support local Unix sockets and supervised SSH proxies, with an independently installable headless runtime package. Account/device code pairing and relay connectivity are subsequent adapters. See [runtime distribution and connectivity](runtime-connectivity.md) for the accepted target and its implementation status. The managed desktop uses a narrow native IPC bridge and a client-owned gateway with persistent runtime channels, native node supervision and a separate SQLite projection cache. A durable native submission journal records intent before dispatch and survives presentation-cache replacement; verified receipt reconciliation and bounded transcript hydration remain in the native client. Native application operations compose authored input and resources, history and queue changes, session launch and attachment, project/directory resolution, imports, scheduling, comparisons, generated Git drafts, file-save checks, observed controls and extension project translation under one captured runtime target. Native IPC and browser HTTP adapters use the same application dispatcher. Configuration recovery never repeats the update. The UI applies returned events and receipts to presentation and authored drafts. Shared schema and TypeScript types live in `packages/contracts`, independently of the callable TypeScript SDK. Browser development access still uses HTTP and frontend polling. Any justified client crate extraction, persistent browser observation, unattended service qualification and account-based code pairing remain follow-up work. See [the separation status](../separation-status.md) for implemented ownership and qualification limits.
