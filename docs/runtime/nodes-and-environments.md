# Nodes and Environments

Status: personal-node architecture direction, with implemented identity guards, persistent desktop attachment, SQLite node registration, a common CLI connection manager, node-scoped client state and caches, desktop node selection and settings, verified gateway routing, and basic connection recovery. Explicit default-runtime SSH installation and startup are implemented. Guided updates, unattended boot, cloud allocation, and full native recovery remain subsequent work.

## Domain boundaries

An Environment is a prepared execution node. Node and Environment describe the same placement role; this design adds no machine-to-Environment hierarchy. A preparation recipe, selected directory, worktree, or cloud session area is a resource or setup on an Environment.

An Environment serves unrelated Projects and conversations. Individual use does not require a collaboration Workspace or a cloud account. Connecting or signing in does not grant collaborators access to the Environment's files or upload them.

The initial personal baseline has one Environment and one execution owner, with multiple controlling clients. Clients, native Harness processes, and the execution owner have independent lifetimes. Additional participants in one shared session address the same execution binding; they do not create independent native writers merely by joining.

## Identity contract: implemented

The retained identity contains `environmentId`, `storeId`, and `ownerScopeId`. The Environment ID identifies the prepared personal execution context; the store ID pins its retained host records. The owner scope is a local personal scope, not a verified account identifier or a collaboration permission grant.

The runtime adds `incarnationId` for one execution owner. Runtime clones share it; creating another owner generates a new value. Restarting the owner against the same retained store preserves the retained identity and changes the incarnation. A different data root creates another retained identity.

URLs, SSH destinations, display names, and operating-system paths are connection or resource metadata. They do not establish Environment identity. The current identity is retained with the store, not anchored to physical hardware. Copying a store can copy its identity; clone detection and re-enrollment require a later explicit design.

`runtime/initialize` publishes `runtimeIdentity`. Saved CLI connections retain its stable `identity` and check the protocol and identity before calls. Native CLI and desktop SSH supervision pin the verified identity and check subsequent proxy handshakes. A changed incarnation with the same retained identity permits reconnection. A changed retained identity stops reconnection and preserves the existing connection record. Selecting a replacement requires removing that saved connection and deliberately pairing again.

Saved CLI calls attach the freshly observed `runtimeIdentity` as a top-level JSON-RPC `expectedRuntime`. The receiving owner compares it before method dispatch, including batch items and notifications. A changed identity or incarnation returns `runtime_target_mismatch` (`-32005`) with `effectKnowledge: "none"`; a notification is discarded without effects. This closes the gap between checking a target and sending a command if the endpoint changes in between.

The target guard is optional for general protocol consumers. Its automatic consumers are the saved remote CLI and desktop connection manager. Other clients must adopt it before claiming equivalent protection. A target guard identifies the intended owner; bearer authentication and transport verification remain independent.

## Node registration and connectivity

The [node enrollment and selection specification](node-enrollment.md) defines the local/SSH flows, availability states, client registry, destination scoping, and acceptance criteria.

The implemented connection manager records automatic local attachment and verified CLI routes in the configured root's SQLite database. Several routes to one retained identity produce one node. Transactional revisions reject conflicting updates and stale reconnections, including removal followed by re-enrollment. `swarmie node list` reads the retained catalog; its timestamps do not establish live connectivity. Removing routes preserves the identity record and remote execution. Credential files remain separate from ordinary metadata. Client state and browser caches are scoped by the full retained node identity. Desktop selection and gateway routing are implemented for registered HTTP, WebSocket, and supervised SSH proxy routes. Explicit default-runtime installation/startup is implemented; automatic alias discovery and guided updates remain pending.

The product action is **Add node**. A Runtime is the execution process used on that node, not the object the user registers. A node record describes the retained Environment identity, display name, connection configuration, and last observed availability. Restarting the Runtime does not enroll a new node.

The desktop client automatically connects to this computer on startup, attaching to its verified owner or starting one when needed. The local node requires no Add node action or manual enrollment, and has no Connect or Disconnect control. The coordinator observes it even after initial failure or while a remote node is viewed, retries temporary transport failures with bounded backoff, and reloads the catalog on recovery. Authentication and retained-identity mismatch remain blockers; local credentials are entered through Details → Connection access. Add node registers prepared endpoints or supervised SSH proxy routes. Guided SSH enrollment uses existing authenticated host access, offers explicit default-runtime installation or startup when needed, verifies the Environment identity, and saves that exact connection. Preparation must not silently install software or replace a live owner. Reconnecting opens transport to the enrolled node and verifies its current incarnation; it does not create a new node or move an existing session.

Node enrollment and local/SSH execution do not require the application's cloud account or collaboration Workspace. Harness credentials and authentication remain subject to each integration. Across networks, existing SSH reachability or a user-managed VPN can provide connectivity. A future account-based rendezvous, device pairing, and secure relay can simplify access without changing where execution occurs or implicitly copying Projects, credentials, and history to a cloud store.

Cloud-managed execution is a separate enrollment/provisioning mode. Choosing it allocates hosted execution resources. Registering a user's existing computer only establishes authorized connectivity; it does not provision a cloud node. Removing a node connection must not stop its shared owner, remove remote history, or delete user directories.

## Execution placement and directories

Before starting native execution, select the Environment, resolve its resources, and qualify the chosen Harness. The desktop selects a retained node state instance. Its local gateway verifies and forwards commands to that destination daemon, where native execution and resource resolution occur. The View node selector at the right end of the raised composer cap switches the displayed instance and new-work catalog. Selecting another node never migrates an existing chat. Settings remains open through node selection and active-thread initialization.

For local execution, a Project links a user-owned directory registered on that Environment. Project-free sessions use its owner's selected default directory. An explicit launch directory is also supported. The session retains its original working context across Project/default edits and reconnects. Missing or replaced directories block dependent execution while retained history remains readable.

Several sessions can use one folder. Local execution does not automatically create per-session directories or worktrees. Choosing a directory does not restrict the Harness to that directory; approvals and filesystem access remain governed by the execution policy and actual native support.

Managed cloud execution can use a session parent area containing worktrees, outputs, documents, and scratch. Task-dedicated and reusable allocations are both supported design directions. Their sharing, ownership, lifetime, and actual isolation must be described separately. Ending a turn or closing a client does not by itself authorize deleting resources or stopping an allocation used by other work.

Selecting another Environment does not copy files, credentials, installed software, or native sessions. Initial selection must fail clearly when required resources are unavailable. Transfer and migration need separate contracts.

## Connection and readiness

Connection state belongs to a client-to-Environment connection. It must distinguish connecting, connected, reconnecting, unavailable, and target mismatch without changing the retained Environment identity.

Readiness has independent observations: reachable transport, compatible runtime, available retained store, selected directory continuity, installed Harness release, authentication evidence, and effective capabilities. An initialized runtime does not establish that a Harness is authenticated or that a particular session can execute. Existing discovery, probe, configuration, and session-context methods provide these observations; a node view should compose them and preserve their uncertainty.

A persisted last connection is metadata, not proof that a node is currently online. A reconnecting view should preserve readable history and drafts while guarding new commands against the verified owner.

## Transport direction

The accepted target design is specified in [runtime distribution and connectivity](runtime-connectivity.md), including a standalone headless runtime, client-owned connection management, persistent local/SSH channels, and later account/device code pairing. Those target capabilities are distinct from the current transport behavior below.

The common JSON-RPC operations define Runtime behavior independently of the transport. The managed desktop sends application operations through native IPC to its client-owned gateway. The gateway retains bidirectional node WebSocket channels for concurrent requests and unsolicited events. Browser development clients still send HTTP requests and poll for presentation updates; gateway forwarding to nodes uses persistent channels. Desktop enrollment accepts prepared endpoints and supervised SSH proxy routes. Explicit default-runtime installation/startup and ongoing desktop SSH supervision are implemented; alias discovery and guided updates remain pending.

The accepted transport layer uses a persistent bidirectional connection carrying requests, responses, events, and approval requests together. The implemented local-socket adapter carries WebSocket bytes for native local clients; browser clients use an authenticated gateway. SSH carries WebSocket bytes through a runtime proxy to the remote local socket. Stdio remains available for explicit process integrations. A later authenticated account relay provides discovery, device pairing and connectivity across networks. Managed desktop persistent node channels and native supervision are implemented through the runtime HTTP listener’s `/ws` endpoint. Local sockets/proxies and desktop SSH supervision are implemented. Persistent browser observations and account relay remain follow-up work.

The Runtime remains independent of attached clients. Every adapter must preserve retained-node and exact-incarnation checks before dispatch. Reconnection must reconcile submission receipts and event cursors against authoritative Runtime state, retain uncertainty about interrupted commands, and never resend a prompt automatically. Connection loss, client disposal, and changing the viewed node must not stop an execution owner or delete history or user files.

The implemented native client channels provide request correlation, unsolicited event delivery, connection lifecycle and explicit delivery uncertainty. Presentation recovery still queries authoritative runtime event cursors and submission receipts. Preserve HTTP for ordinary files and previews. Next implement persistent browser observations and qualify guided SSH updates, additional recovery and unattended startup. Account relay is a later adapter; local and SSH operation must remain available without it. This ordering changes transport behavior without changing the common Harness operations or session ownership.

## Lifetime and recovery

| Event | Required interpretation |
| --- | --- |
| A controlling client disconnects | Reconnect to the same owner; do not create another native writer or resend unconfirmed input. |
| The transport reconnects to the same owner | Rebuild observation from authoritative snapshots, submission receipts, and retained event cursors. |
| The owner restarts with the same retained identity | Detect the new incarnation, reload retained facts, preserve uncertainty, and keep restored queues paused until deliberate continuation. |
| The endpoint now serves another retained identity | Reject the saved target before method effects and retain the original connection metadata. |
| A Harness process fails | Keep the Environment distinct from the failed native binding; report qualified recovery availability. |
| The node sleeps or shuts down | Do not promise survival of native processes; retained history and actual native resumability are separate facts. |
| A connection is removed | Unlink client connection metadata; retain remote history, Projects, directories, and execution resources. |

The desktop launcher attaches to the retained Environment's existing loopback HTTP owner or starts a detached owner. A separate startup lock serializes simultaneous first launches before database initialization. The execution-owner lock prevents competing native writers. A private endpoint record publishes the address, exact Runtime identity, asset configuration, and authentication requirement; it is discovery metadata, never proof of liveness. Attachment checks the retained store identity and a live initialization handshake before opening a client. An unavailable owner is an explicit failure rather than permission to replace it. Frontend assets belong to the client gateway and do not change execution-owner configuration.

Closing a window, exiting the launcher, or failing the client leaves the owner and its sessions running. `swarmie runtime status` inspects the configured data root. `swarmie runtime stop` verifies the live owner and sends guarded `runtime/shutdown`, then waits for the listener and owner lock to be released. Shutdown preserves retained history and user directories. This applies to discoverable loopback HTTP owners; direct stdio consumers still own their spawned daemon's lifetime.

Desktop reconnection retains the stable Environment target, verifies a fresh incarnation, and attaches the observed identity to later calls. Transport loss preserves session state and drafts; bounded retries reload authoritative snapshots, transcripts, queues, and submission receipts without resending a prompt. A different retained identity blocks reconnection. Tokens remain in memory. A running binding can be attached without native restart support; reopening a closed binding still requires the Harness's resume capability.

The native client observes the runtime, retries connections with bounded backoff and reads updated local endpoint discovery while retaining node identity. A detached process is not an operating-system restart supervisor. Boot startup, automatic crash restart, remote network exposure, and guided SSH updates are separate configuration or follow-up work. Explicit default-runtime SSH installation and detached startup are implemented. Sleep and shutdown can interrupt execution. Managed desktop drafts and cached Project/chat projections use a distinct namespace for each retained node identity in the native client's SQLite cache. Browser development clients retain origin-scoped storage. Attaching to a runtime does not transfer unsent drafts from another device.

## Ordered follow-up slices

1. Extend the implemented local/SSH channels with persistent browser observations, automatic SSH alias discovery and guided updates. Explicit default-runtime installation/startup is implemented; the SSH view is available before enrollment. Qualify unattended startup separately.
2. Extend readiness qualification and add explicit route selection or qualified failover. Node Details already shows last verification, saved routes, and node-specific Harness readiness.
3. Qualify recovery from owner failures and restarts across supported Harnesses; preserve uncertain submissions and paused retained queues.
4. Qualify richer resource navigation and multiple-client recovery against the selected node. Current session creation and path resolution use its fixed state instance and verified gateway route.
5. Implement managed cloud allocation and ownership-aware session areas through the same execution contracts when that product phase begins.

Account pairing, relay deployment, cross-device access grants and permission management, collaboration grants, store clone enrollment, execution migration, stronger isolation, cloud retention policies, and automatic allocation cleanup remain open. Broader native recovery qualification retains its existing low priority. None of these open items requires a parallel compatibility layer for earlier internal development formats.
