# Runtime distribution and connectivity

Status: accepted architecture direction. The existing implementation provides a transport-independent runtime API, retained node identity, detached local execution, durable events and submission receipts, endpoint enrollment, and optional operating-system services. The managed desktop now uses an application-specific native IPC bridge, a client-owned gateway, persistent node WebSocket channels, native connection supervision, and a separate SQLite presentation cache. Private Unix sockets and Windows same-user named pipes, a byte proxy over SSH, and a dedicated runtime-only distribution extend this path. Account-based code pairing remains planned.

## Runtime and client responsibilities

The runtime is independently installable on a prepared node, including a server with no graphical session. It owns native Harness bindings, conversations, canonical history, node-local Projects and directories, extensions, execution policy, tool requests, and persistence. It supports multiple controlling clients. A client's departure does not create a replacement native binding or stop execution.

The client owns presentation, its node catalog, connection routes, credentials for those routes, subscriptions, and node-scoped caches. In the native desktop, client application logic resides in the native connection layer; the WebView presents state and collects user intent. A client-side connection manager verifies targets and carries the common API to each runtime. Keep this responsibility separate from execution ownership even while implementations share a binary or process. A frontend bundle path is not a runtime installation or identity requirement.

A registered node retains its stable Environment/store/owner identity across runtime restarts. An incarnation identifies a particular execution owner. Neither a route, a pairing code, an account identifier nor a display name replaces those identities. Account/device grants require a separate authorization mapping; the current personal owner scope is not evidence of a verified cloud account.

Node and Environment retain the agreed prepared-host role. Projects and working directories are resources on that node. A later executor boundary for containers or managed execution does not introduce a new mandatory hierarchy into the current domain model.

## Desktop UI bridge

The native desktop uses a system WebView. Introduce an application-specific IPC bridge between that UI and the client connection manager. The bridge is a local UI/host boundary, not the transport to a remote node or a requirement to run an additional public network server.

The intended path is:

```text
UI -> native IPC bridge -> client connection manager
   -> selected node transport -> runtime -> Harness adapter
```

The UI owns rendering, navigation, focus, animations and temporary form interaction. It sends explicit user intents and renders client projections. The target native client layer owns enrollment, node selection and routing, credentials, connection lifecycle, request correlation, event recovery, submission reconciliation and node-scoped client caches. UI validation may improve feedback, but authoritative checks belong in the native layer or runtime. Native placement must not move canonical history, execution policy or Harness orchestration out of the runtime. Managed desktop drafts, attachments and workspace projections are stored in a separate client SQLite database and keyed by the complete retained node identity. Native prompt submission records intent in a durable client submission journal before dispatch. This journal is independent of replaceable presentation caches; cross-process session and journal locks serialize concurrent clients using the same client data root. Chunked IPC transfers preserve bounded SDK messages for large requests and responses; transfers are limited, expire and are consumed once before dispatch.

Expose application operations and observable projections rather than requiring UI components to reconstruct transport lifecycle or recovery rules. The managed desktop delegates node channel lifecycle, heartbeat, bounded reconnect backoff and connection observations to the native client layer. Its UI consumes observations through IPC and reconciles presentation against authoritative runtime events and receipts. Browser development access still uses HTTP and frontend observation timers; a persistent browser observation adapter remains follow-up work. Presentation reducers, form interaction and view hydration remain in the frontend. The native `client/submissions` operation supplies verified receipts, unresolved-delivery state and an authoritative session snapshot. Native observation refreshes outstanding receipts without redispatching prompts. The UI applies that projection to presentation fields; browser development access uses the same native application dispatcher through authenticated HTTP. Native `client/sessionProjection` composes snapshot, receipt state and bounded transcript paging under the verified runtime identity. The UI reduces returned events into view fields and reconciles authored drafts; it does not orchestrate native transcript recovery. Native `client/sessionOpen` additionally owns attachment decisions and returns the refreshed queue. Native launch, project, directory, control and extension operations compose their runtime calls under one captured target. Partial configuration failure reads observed controls without retrying the update. Native authored-input, history, queue, project creation, import, generated Git drafts, comparison, schedule and file-save operations now own their workflows. Views supply intent and apply returned projections. The browser observation adapter and any justified client crate extraction remain follow-up work.

Expose a bounded application interface for correlated requests, responses, event subscriptions and connection observations. Keep node-route credentials and SSH/process supervision in the client connection layer. Forward common runtime operations without introducing a second Harness-specific API in the UI. The runtime owns canonical execution and history; the client manager owns route lifecycle and observation.

The managed native host registers the `swarmie.client` application command and keeps the built-in privileged bridge disabled. Only the trusted primary WebView origin, window and label may invoke it. Asynchronous IO work returns completions on the native UI loop; teardown cancels outstanding bridge work. The Zig window host forwards through a private, credential-protected loopback endpoint to the Rust client gateway. The gateway lives with the client launcher and holds persistent node channels. The bridge credential is absent from frontend assets and requests reject browser Origin headers. Embedded documents and extension frames retain their scoped interface rather than gain native connection privileges.

Desktop frontend assets are served by the client gateway independently of runtime installation. Closing the WebView or terminating the client connection manager releases client subscriptions and connections without stopping the runtime. The planned browser observation adapter will expose the same logical UI interface through an authenticated WebSocket gateway, since browser clients have no native IPC bridge. Keep the application contract independent of a specific WebView SDK.

## Persistent account authority

Account credential ownership, restoration, renewal and revalidation belong to native core interfaces
and a persistent headless host. The desktop connection manager attaches to that authority and exposes
sanitized snapshots; it must not become the credential or refresh owner solely because it launches
the UI. Closing all windows does not prevent active core/runtime work from refreshing its own
authorized session. If the headless owner is stopped, renewal resumes after its next startup and
before work requiring fresh authority.

Host the account capability independently of Harness or project initialization. It may share a
user-scoped runtime process when those lifecycle guarantees hold; otherwise it requires an
independent headless hosting boundary. Account operations target their authorized owner rather
than the node selected for a chat. User account credentials and explicitly enrolled runtime/device
grants are distinct authorities; connecting to a remote node does not transfer a human refresh token.
See the [native account integration plan](https://github.com/syntropika/swarmie-cloud/blob/develop/docs/account-integration-plan.md)
in the private hosted-service repository.

## Runtime-only installation

Provide a runtime-only release and installation path containing the executable, required runtime dependencies, and supported extension installation/configuration. It must not require frontend assets, a system WebView, a native window host, or a desktop session. Native Harness executables and their credentials are prepared on the execution node; connecting a client does not copy that client's installations or credentials.

Support explicit service installation, start, stop, status, update, and removal. Keep service configuration, application data, and client presentation independent. Installation must preserve existing node data and user directories. Service removal and client unlinking have separate effects from stopping execution or deleting data. Updates that require runtime restart must expose that effect rather than implicitly interrupt active work.

The runtime-only packager emits a native executable, installer, package metadata, Apache-2.0 license and notice. It has no desktop or private-service dependency. Installation never stops active processes. Unix updates can replace managed binaries while an existing process retains its loaded executable; Windows requires an explicit stop before replacing an in-use executable. Subsequent starts use the replacement. Removal preserves user data and unknown files, and refuses an installation still referenced by a user service. The service CLI supports installation, start, stop, status, enable, disable and removal; removal requires execution to be stopped explicitly. Current supervision uses a user service. System-service scope and startup without an interactive login remain qualification work.

## Common connection contract

Use a persistent bidirectional connection per selected node route, supporting correlated concurrent requests, unsolicited events, surfaced user/tool requests and responses, heartbeat, bounded queues, subscriptions, and explicit connection lifecycle. Preserve the current common API and native translation behind Harness adapters.

Initialization verifies protocol support, stable node identity, current incarnation and relevant capabilities. Target checks continue through dispatch. A route replacement or changed identity cannot retarget pending work. Connection state, node authorization, Harness authentication, directory continuity and execution readiness remain separate observations.

Recover subscriptions from authoritative snapshots, durable event cursors and submission receipts. An RPC request ID correlates messages; a retained submission key identifies intentional prompt delivery. Retain uncertainty after an interrupted dispatch and never automatically resend a prompt. A slow or disconnected client must not block other clients indefinitely or silently lose an approval request. Reopening a transport does not promise native process recovery after a runtime failure.

## Transport adapters

| Route | Implementation or planned adapter |
| --- | --- |
| Native client to local runtime | WebSocket framing over a private Unix socket or a same-user Windows named pipe, reached through native IPC and the client gateway. HTTP(S)/WS(S) endpoint routes remain available. |
| Browser client to its gateway | WebSocket over HTTP(S), with explicit authenticated origin and destination routing. |
| Client to an SSH node | HTTP Upgrade/WebSocket bytes carried through a supervised SSH stdio child to `swarmie runtime proxy`, connected to an existing remote local socket. |
| Account-linked remote node | WSS service-mediated transport with authenticated device grants, multiplexed node streams, sequence acknowledgments and recovery cursors. |
| Files and previews | HTTP where appropriate; resource access retains the selected node and execution policy. |
| Explicit process integration | Stdio for applicable CLI/embedded use, with documented process ownership rather than the persistent desktop default. |

The Unix socket requires an owned private parent directory and uses same-user filesystem access independently of HTTP bearer authentication. The endpoint record publishes `localSocket`; clients use that verified route. Long data-root paths select a short private socket location keyed by the canonical data root, without moving state or user directories. The remote proxy copies bytes and connects only to an existing listener; it does not install or start a runtime. Native connection supervision owns its SSH child and reconnects through the common identity checks. Reconnection is distinct from installing, updating or restarting a runtime. Additional platform and live-network qualification remains separate from local protocol and process-lifetime tests.

Local startup attaches automatically to an existing verified runtime or starts one when needed. The runtime remains alive when the client exits. Local, SSH and directly reachable endpoint routes are available without an account. Accounts do not gate direct SSH access; hosted discovery, pairing, relay connectivity and synchronized node catalogs have separate account authority. Signing in does not automatically publish saved routes, credentials, Projects or conversation history.

## Account-linked code pairing

A separately installed server can be linked from an external client through a short-lived pairing code. The pairing service provides authenticated discovery and connectivity; the runtime remains on the user's server. Registering that node is not managed-cloud provisioning and does not imply uploading its Projects or complete history.

The intended user flow is:

1. Install and start the runtime on the server without opening a graphical client.
2. Explicitly enable account-mediated remote access and establish the node's authorized enrollment.
3. Start a pairing attempt on the server and display a short-lived code or equivalent link/QR representation.
4. In the external client, choose Add node and enter the code under the intended authenticated account/device.
5. Redeem the attempt into an authorization grant for that client device and the exact enrolled node. Show and verify the node identity before making it available for work.
6. Establish the persistent runtime connection, discover its resources and Harness readiness, and continue existing conversations through the same runtime API.

The code is a temporary pairing artifact, not a reusable runtime bearer credential. Attempts must expire, support cancellation, limit guesses, and permit one successful atomic redemption. Bind redemption to the intended account/device and enrolled node. Expiration, replay, account mismatch or node replacement must not create a grant. Repeated authorization or additional routes to the same verified node must not duplicate the node.

The grant persists independently of the code and transport address. Clients use separately issued authentication material for later connections. List authorized devices and allow their access to be revoked, including active streams according to the defined authorization policy. Revoking one client does not stop the runtime or revoke another client's grant. Linking a device grants no collaboration membership or workspace administration by implication.

For connections across networks without existing direct reachability, the account mode requires a rendezvous/relay service. Both the runtime and controller can establish outbound connections to it, avoiding a requirement to expose an inbound runtime port. A code alone does not create network reachability. Direct endpoint and SSH operation remain available without this service.

The exact code format/lifetime, optional server-side confirmation, account enrollment mechanism, device-key storage, token scopes/rotation, active revocation semantics, relay envelope schema and payload-encryption contract remain to be specified. Do not claim end-to-end encryption from WSS or device signatures alone. Internal runtime API compatibility is not required during development, but supported external Harness behavior remains subject to its agreed integration scope.

## Remaining implementation slices

1. Extend the implemented desktop IPC and persistent request/event channels with a persistent browser observation adapter and qualify additional client/platform behavior. Retain current identity and submission guards.
2. Broaden live-network and platform qualification of the implemented local socket and supervised SSH byte proxy, including guided updates and additional failure recovery; explicit default-runtime installation/startup is qualified.
3. Qualify system-service scope, unattended boot, logout and crash supervision for the runtime-only distribution. Guided user-startup controls are implemented; ordinary SSH connection and package installation do not enable services implicitly.
4. Implement account/device enrollment, code pairing, grants/revocation and relay routing over the established connection contract.
5. Add managed execution allocation and a separate executor implementation when the product requires them.

Runtime-only installation and account-free direct/SSH access do not depend on code pairing or relay implementation.

## Acceptance evidence

Verify installation on a server without frontend or graphical dependencies; service readiness without an interactive client; automatic local attachment; independent concurrent clients; continued execution after a client exits; node identity continuity across reconnects; refusal of replaced targets; event recovery without prompt resend; and loading existing node resources rather than copying them into a new conversation.

For pairing, verify expiration, cancellation, bounded guessing, simultaneous redemption yielding only one grant, account/device/node binding, duplicate-node avoidance, revocation of the intended client's active and future access, and preservation of other clients' execution. Test relay connection loss and uncertain RPC delivery separately from process restart. Pairing and relay cases remain future qualification requirements; the private socket, SSH proxy and runtime-only package have separate implementation tests.

## Windows transport

Windows native clients automatically attach through a user-only named pipe keyed by the canonical runtime root. The pipe rejects network clients and has a DACL limited to the current user. HTTP and network WebSocket endpoints continue to require a bearer credential. A saved pipe route uses `pipe://localhost/?path=ENCODED_PATH` and cannot name a remote pipe host.

SSH discovers the remote shell with read-only checks. Linux/macOS use their POSIX shell; Windows uses encoded PowerShell commands compatible with Windows OpenSSH's cmd.exe or PowerShell default shell. Python reads bootstrap headers and relays runtime process streams with a restricted handle inheritance list. Windows SSH access therefore requires Python 3.9 or later in addition to PowerShell; WebSocket frames and installation archives never pass through text pipelines. Python preparation source and arguments travel in a bounded stdin header before archive bytes, preserving cmd.exe's command-length limit and avoiding shell interpolation of user paths. Mutating commands are sent once and are not retried through a different shell.

Detached Windows owners request process-job breakaway, so an SSH or client job closing cannot terminate the owner when host job policy permits breakaway. If the host denies that capability, startup fails instead of silently promising persistent execution. Native x86_64 OpenSSH checks verify this process lifetime. Detached owner creation explicitly inherits only its null input and runtime log handles, so unrelated controller and SSH handles cannot keep the preparing connection open. Host policy denying job breakaway remains an explicit startup failure.

## Default detached transport authentication

A detached owner stores a private generated bearer in `runtime/transport.token` under its data root and passes that file to its HTTP transport. Unauthenticated HTTP initialization receives 401. Same-user private sockets and named pipes remain the native attachment route and do not expose this bearer to the UI. Existing malformed token files are preserved and rejected. Explicit `serve` configuration retains its separate transport policy.

## Windows directory and client state

Retained directory bindings use the volume identifier, 128-bit filesystem object identifier and creation time from an opened directory handle. Renaming the directory preserves its object fingerprint; replacing a folder at the original path changes it and blocks retained-context dispatch until explicit relinking. Missing-directory inspection does not create a replacement. The Windows desktop selects a writable `client/webview2` profile beneath its data root, independently of the executable installation prefix and Project directories.

## Account owner

The optional [native account owner](accounts.md) is an independently hosted per-user process.
Its session, callback and renewal lifecycle do not depend on the desktop gateway or an execution
node. Native account commands do not travel through the selected-node transport.
