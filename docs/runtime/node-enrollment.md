# Node enrollment and selection

Status: the SQLite node registry, automatic local startup, desktop node selection and settings, node-scoped client state, and a local common-protocol gateway are implemented. Prepared endpoints and configured SSH byte-proxy enrollment are supported. Actual Linux-to-macOS SSH and native macOS runtime behavior are qualified with disposable installations. SSH alias discovery and reviewed installation remain subsequent work.

## Product contract

The user adds a node and chooses where new work runs. Node and Environment describe the same prepared execution host. Its Runtime is the process that owns native execution. Clients observe and control that Runtime through the common protocol.

The local node, **This computer**, connects automatically when the desktop client starts. It does not require an Add node action, enrollment wizard, or a manual Connect step. **Add node** is for additional hosts. The form verifies an already running HTTP(S) or WS(S) endpoint, or checks a configured SSH host and existing runtime socket. The SSH path invokes an installed runtime proxy and never prepares a new execution owner. Neither local startup nor SSH enrollment requires an application cloud account or collaboration Workspace. Harness credentials remain subject to each integration. Account-based device pairing, relay connectivity, and hosted-node provisioning are later modes; their absence does not block the local and SSH baseline.

Enrollment records a verified retained Environment. Runtime restarts change its incarnation, not its enrollment. Display names, host aliases, ports, and transport routes are metadata, not identity. A pending setup does not appear as a usable enrolled node until verification succeeds.

## Standalone server and code pairing: planned

A runtime-only installation on a server requires no desktop session or frontend bundle. An external client can attach through an existing authorized endpoint or SSH route. The later account mode supports explicitly enrolled nodes and short-lived code pairing, with device-specific grants and revocation. The code establishes authorization through the pairing service; it does not itself provide network reachability. Across networks without a direct route, the runtime and client use an outbound relay connection. Local and SSH use remain account-free.

See [runtime distribution and connectivity](runtime-connectivity.md) for the accepted boundaries, pairing flow, open details and implementation slices. Code pairing and relay remain planned. Runtime-only packaging and lifecycle installation are available separately from the Add node form.

## Node list and selection

Use **Settings → Nodes** for enrollment, connection management, and observed availability. This single section contains **Control this computer** and **Control other devices** tabs. An **SSH** tab appears only when a registered remote node already has an SSH route; it lists those routes without discovery or installation controls. The local row says **Connects automatically** and has no Connect or Disconnect control. Remote rows show a connection switch and an actions menu containing **Details** and **Open node**.

**Details** shows the last verified timestamp, saved connection names and addresses, revision-guarded route removal, and that node's Harness installation and authentication readiness. The local details also offer **Connection access** for its bearer token. A historical timestamp does not establish current availability, and a connected Runtime does not establish that every Harness can execute.

Client startup automatically attaches to **This computer**, independently of which node is selected for viewing or new work. The shared **View node** selector sits at the right end of the raised composer cap, defaulting initially to **This computer**. It switches the displayed node state instance and the catalog used for new work. Remember the last deliberately selected enrolled node for subsequent new chats. If that node becomes unavailable, keep the selection visible and offer another node for a new chat; do not silently redirect work.

Opening an existing Project or chat selects its retained node. An existing chat's node label describes its execution location. It is not an editable destination control. Moving execution to another node remains a separate, deferred operation.

Show only the selected node's Projects and Harness observations during initial implementation. A later combined catalog must preserve the node beside each entry. Equal directory paths, Project names, or native session IDs on different nodes do not establish shared resources.

## Automatic local startup

Run this flow automatically on every desktop client startup, before local execution commands become available. The user does not add the local node manually.

1. Obtain the configured local data root and its retained Environment identity through the local connection manager.
2. Attach to the verified owner or start it using the persistent-runtime launcher.
3. Verify protocol compatibility and live identity.
4. Internally record or refresh **This computer**, reusing its existing record when the retained identity matches. Show it as the automatic local node, not as a setup task.
5. Load its Project catalog and passive Harness discovery. Probe a selected Harness when execution readiness needs active qualification.

Local attachment does not wait for a selected Project, an opened chat, or an application-cloud sign-in. Attaching the local node does not change the destination of a selected remote Project or existing remote chat. A client on another computer attaches to that computer's own local node; it does not rebind previously enrolled remote identities.

No project, worktree, or session is created just by attaching or internally recording the local node. Startup does not change the selected working directory, native approval settings, or another client's session.

While automatic attachment is in progress, show Connecting. The node coordinator observes the local connection throughout the client run, including after an initial startup failure and while a remote node is viewed. Temporary transport failures use bounded retry backoff; recovery reloads the node catalog. There is no manual local Retry or Disconnect control. Authentication and retained-identity mismatch remain actual blockers: enter a required token through **Details → Connection access**, or the access action shown before the local catalog can load, and investigate an identity mismatch before reconnecting. An occupied owner lock, an unresponsive existing owner, incompatible launch configuration, or a missing retained database does not authorize replacing the owner or creating empty history in its place.

## Add a prepared endpoint: implemented

Open **Settings → Nodes → Control other devices → Add**. The focused **Add node** dialog asks for an unused connection name, the address of a prepared Runtime, and an optional access token. Verification records the retained identity before the route becomes usable. Failed verification keeps the dialog and entered values intact; canceling clears the token. While verification is busy, duplicate submission and dismissal are blocked. Multiple differently named routes to one identity share one node. An occupied name is rejected instead of replacing a route.

The composer cap's **View node** selector changes the viewed node and destination for new work. **Open node** in Settings selects the same retained state instance; Settings stays open through node selection and active-thread initialization. Existing chats keep their original execution node. Use a remote row's connection switch to connect or pause this client's observation, and **Details** to remove an individual saved route using its displayed revision. Removing the last remote route marks that node unregistered and keeps its cached work. A removed active node stays visible; it cannot execute until a connection is added again.

The direct form does not install software or create an SSH route. Selection uses the first saved route in connection-name order; automatic route failover remains pending. The SSH form below owns its proxy subprocess through the native client.

## Add an SSH host: implemented baseline

1. Choose **Add node → Connection method → SSH**. Enter an unused connection name, an existing SSH alias or explicit host, and use its default runtime. An optional socket override is available under **Advanced** for a custom runtime installation.
2. **Check connection** uses configured OpenSSH with batch authentication and ordinary host-key verification. Unknown hosts, passwords, or interactive key prompts require explicit setup outside this form. The application never bypasses host-key verification or copies private keys.
3. Check for `swarmie` on the noninteractive remote command path or at the managed `~/.local/bin/swarmie` launcher. Report missing installation without making changes.
4. Connect through `swarmie runtime proxy`, with an optional socket override. Verify the protocol, retained Environment identity and current runtime incarnation. The remote runtime must already be running; the proxy neither starts nor owns it.
5. Display readiness or the required installation/service/socket correction. For the default runtime, offer **Install and connect** when installation is absent and a verified package source and supported Python/platform are available; offer **Start and connect** when the installed runtime is unavailable. Both actions are explicit and verify identity before enrollment. A connection becomes enrollable only after a valid check. Changing host or the optional socket override invalidates that check.
6. **Add node** sends the prepared runtime identity. Recheck it before saving; a changed owner requires another check. Persist route metadata and load its existing Projects and chats without importing duplicate conversations.

Native observation owns the SSH subprocess and reconnect scheduling. Each new connection verifies the saved retained identity; replacement with another Environment blocks commands. Runtime restarts retain identity but require the caller's current incarnation. Accepted prompts are not automatically resent after transport loss.

Explicit default-runtime installation and startup are implemented. The native client freshly checks remote state, verifies and transfers a matching runtime-only archive, invokes the existing user-owned installer, starts or attaches through the serialized owner bootstrap, and verifies the runtime. The same native operation enrolls that exact identity; the view presents the result. Busy preparation blocks dismissal and repeated submissions. Failure clears the prepared target and requests a new check because remote changes may have completed. No mutation is automatically replayed.

Installation uses the remote user's managed prefix without administrator access, profile edits, key copying, or application-cloud credentials. A custom socket override requires separate preparation. Existing or incompatible installations are not automatically replaced or updated. Harness installation and authentication remain separate. Automatic alias discovery, guided updates, unattended boot and system-service scope remain pending. Setup checking itself never installs, updates, starts, or restarts the remote owner.

A node can be registered with no ready Harness. Report **No ready Harness** in its details and let the user prepare or authenticate one before execution. Runtime connection readiness and Harness readiness remain independent facts.

Canceling setup closes only its temporary checks and transport. It does not delete files, stop an already shared owner, undo completed installation changes, or remove remote history. A preparation failure does not claim rollback or prove that no changes occurred; the next attempt must inspect actual state before taking further action.

Adding another verified route to the same retained Environment updates its connectivity rather than duplicating its node, Projects, or sessions. A route that reports a different retained identity cannot replace an existing node automatically. Store-clone enrollment and detection remain deferred; this baseline does not claim hardware identity.

## Availability and actions

These are client-to-node connection observations. They do not replace native session lifecycle or Harness authentication state.

| Displayed state or blocker | Meaning | Current client behavior |
| --- | --- | --- |
| Not connected | A registered remote node has no active verified transport | Turn on its connection switch. |
| Connecting | Reachability and identity verification are in progress | Wait for verification; duplicate management actions are disabled. |
| Connected | Transport and compatible Runtime identity were verified | Open node or inspect Details; a remote switch can pause observation. |
| Unavailable | Connection failed; its reported reason remains visible | Temporary transport failures retry with bounded backoff; a remote switch can request a connection. |
| Authentication required | The reported error identifies missing Runtime credentials | Supply the required token; local access is in Details → Connection access. |
| Identity mismatch | This route reports another retained Environment | Inspect the target; do not silently replace the retained identity. |
| Removed | The remote node's last saved route was removed | Keep readable cached work; add a verified route before execution. |

Last seen remains historical metadata after disconnect. Do not infer that the machine is powered off from a network error, or that its native sessions have failed from a lost client transport.

Harness details report installation/version evidence, authentication evidence, capabilities, and readiness reasons from the common discovery and probe contracts. Validate the selected directory and session context when opening or executing work; a node-wide connection check does not certify every folder.

## Client records and authoritative state

The local connection manager owns the node registry under the client's configured application data root. Store ordinary connection metadata in its SQLite state, consistently with other local application settings. Use private credential storage separately; public node records contain credential references, never bearer tokens or private key material. Reuse OpenSSH-managed authentication where applicable.

The implemented registry groups multiple verified connection names under one retained Environment identity. `swarmie node list` reads this catalog without checking liveness; `swarmie remote list` reads its saved routes. Desktop startup records the verified automatic local node and initializes the selection only if no preference exists. The desktop selector restores this preference after automatic local attachment.

Route changes use transactional expected revisions. A registry-wide revision counter prevents an old verification or SSH reconnection from replacing a removed and re-enrolled route. Calls verify the retained identity, check that the route is still current, and dispatch with the observed exact Runtime target. Removing a route retains the node identity and selection for future history associations; after its last route is removed, a remote node is marked unregistered. The local node remains registered independently of user-added routes.

Bearer credentials currently use immutable private files under `credentials/node-connections` in the configured root, referenced by opaque IDs in SQLite. Unix directories use mode 0700 and files use 0600. This is filesystem credential storage, without an encryption or system-keychain guarantee. Failed registry writes remove newly created credential files; replacement cleans old files after commit. A crash or canceled write can leave an unused private file. A cleanup failure after removal reports that the connection was already removed. No historical development JSON importer is provided, and existing files remain untouched.

Retain the stable identity tuple, display name, connection mode/routes, credential reference, and selected-node preference. Persist last observation time as historical metadata. Keep the live incarnation, verified connection status, retry state, and active transport as runtime observations; reload and verify them after client restart.

The destination Runtime remains authoritative for its Projects, directory resources, working contexts, sessions, submission receipts, and event journals. Cached client history, drafts, event cursors, selected items, and pending requests now belong to a client state instance pinned to retained node identity. Client-local storage does not provide cross-client draft synchronization.

The desktop uses the local gateway for all remote operations. `/client/rpc` exposes node-list, selection, enrollment, and route-removal actions with an exact local Runtime target. `/nodes/{environmentId}/rpc` forwards the common protocol to a registered destination. Both routes use the local listener's origin, Host, and bearer authorization. They exist only on a desktop-enabled listener; ordinary daemons do not expose node administration.

The browser keeps the local gateway token in memory. Remote node credentials are sent to local enrollment and retained in private credential files, then read by the gateway for remote calls. Public catalog responses expose authentication presence without credential references or tokens. Browser clients never connect directly to saved remote URLs. The automatically observed local connection permits node administration while a remote node is viewed; its authentication must succeed before gateway administration is available.

Each forwarded command requires the caller's exact verified destination target. The gateway verifies the saved identity and current route revision before dispatch, preserves the caller's incarnation, and sends the command once. Native JSON-RPC IDs, error codes, and error data survive forwarding. Identity or incarnation mismatch reports `effectKnowledge: "none"`; transport or malformed-acknowledgment failures preserve `effectKnowledge: "unknown"`. Verification never silently retargets an operation.

Files, assets, terminals, and extension commands use the selected node's protocol route. Remote previews use `/nodes/{environmentId}/preview/{previewId}/`, including rewritten resource URLs and redirects. The relay retains its body bounds, scoped content policy, and sandbox capability behavior. Preview identifiers do not grant node administration. Workbench auxiliary storage uses the destination's configured data root under `desktop`, independently of user working folders.

Each operation captures its intended node and verified incarnation before dispatch. Changing the new-chat selection while a request is pending cannot retarget that request or apply its response to another node's view. A target mismatch blocks effects and triggers verification before any further command.

## Client state scoping: implemented

A client state instance belongs to one retained identity tuple: `environmentId`, `storeId`, and `ownerScopeId`. It cannot change destinations. Keep Projects, chats, selected items, directory observations, Harness observations, authentication tokens, submission tracking, event cursors, and pending work inside that instance. Changing which instance is displayed does not redirect its ongoing requests. Resource identifiers remain node-local; a combined view must carry the owning instance or retained node identity beside each identifier.

Browser storage keys use the full stable tuple, without the Runtime incarnation or transport address. Lightweight local storage keeps Project/chat navigation and text drafts. IndexedDB keeps attachment data, transcript projections, prompt records, file drafts, and cached queues under a separate tuple key. Each envelope carries its node identity and is checked before hydration. Two routes to one node share the storage namespace; equal resource IDs or paths on different nodes do not. Desktop notification tags also include the node identity.

A route stores its retained node association separately, so offline startup can restore that node's saved work before reconnecting. A first unverified connection does not load unscoped host history or persist node submission intent. Once verification succeeds, it binds the instance and loads that node's cache. The previous development browser keys and IndexedDB entry remain untouched; no importer assigns their unknown history to a new node.

Every RPC captures the fixed route, verified Runtime target, and current connection generation before dispatch. A response or transport failure from an earlier connection is rejected before publishing its result or disconnecting the current attachment. An obsolete response does not prove that its original operation had no effects; retained submission receipts remain the authority and prompts are not resent automatically. Owner restarts retain the stable cache namespace while requiring fresh verification.

Browser storage remains local to its origin. This implementation does not synchronize drafts between clients or resolve concurrent-window draft edits. A coordinator reuses each node state instance and polls connected instances even while another node is viewed. It continues observing the local connection after startup failure and reloads the catalog on recovery. Deliberate remote disconnect stops that instance's observation and retries. Client disposal stops polling without shutting down any owner.

## Disconnect, remove, and stop

**Disconnect**, through a remote row's connection switch, closes this client's transport and pauses its observation. The node stays registered; its owner and other clients' work continue. The automatic local node has no disconnect control and remains under automatic observation.

**Remove node** applies to user-added remote nodes and removes their registered routes while detaching their transports. Retain the stable identity as an unregistered record for local history associations. The CLI and desktop remove one named route at a time; desktop removal includes the revision observed in its catalog. The automatic local node is not a user-added record to remove; its connection is automatic, and owner shutdown remains a separate explicit operation. Keep cached readable history and unsent work associated with the original retained identity, available for recovery or export. Do not stop the owner, delete remote history, remove Projects, or delete user files. Re-enrolling the same verified identity can recover that local association.

**Stop execution on node** is a distinct owner action. Explain that it affects that owner's running sessions and other attached clients. Send the guarded common shutdown request and observe cleanup. Stopping execution is not part of disconnecting, removing a node, selecting another node, or closing the application.

## Implementation order and acceptance

1. Implemented: local SQLite registry, common CLI connection manager, and automatic local startup recording under the configured application root. No historical internal-format importer is provided; existing files remain preserved.
2. Implemented: node-scoped client state, browser cache keys, notification tags, and fixed-destination pending RPCs. The UI must retain and reuse the corresponding instance when exposing node selection.
3. Implemented: desktop Nodes settings, automatic local attachment, prepared-endpoint enrollment, selection, scoped forwarding, and remote preview relay. Connection state and selected-node Harness readiness remain separate observations.
4. Implemented: configured SSH installation/readiness checks and native persistent stdio proxy attachment using the common WebSocket protocol. Actual Linux-to-macOS transport is qualified; explicit installation/startup are implemented; alias discovery and guided updates remain pending.
5. Implemented: selection for new chats and remote Project/default-directory resolution through the destination state instance. Existing chats continue on their retained node.

The complete multi-node feature will require evidence that desktop startup connects the local node without enrollment or a manual Connect action; local attachment leaves remote chat destinations unchanged; two clients can observe one binding; disconnecting or removing one client does not stop execution; repeated enrollment does not duplicate a verified node; same-identity owner restart reconnects; foreign identity replacement blocks commands; unavailable nodes preserve readable history and drafts; equal paths and session IDs across nodes remain distinct; and node switching cannot misroute a pending operation.

Transport failures must not cause automatic prompt resend. Restored submission receipts and event cursors determine what is known. Owner failure and native resume support remain separate qualification work.

This slice verifies automatic local recording, retained selection, multiple routes per identity, independent concurrent registry writers, stale-revision rejection after re-enrollment, private credential references, and remote execution surviving route removal. Client tests additionally cover equal resource IDs and paths across nodes, independent draft/attachment/transcript restoration, delayed responses while another instance is viewed, stale responses and transport failures after reconnection, and refusal to attach foreign cached identities. Coordinator tests cover node switching, offline selection, automatic local observation and recovery, remote disconnect, removal, hidden-node polling, startup remounts, and credential caching boundaries. Integration tests use independent runtimes over HTTP and WebSocket to verify destination files, terminal execution, scoped previews, authorization, target mismatch, preserved native error data, and uncertain delivery without repeated effects. These tests qualify gateway routing rather than remote installation or network deployment.

The registry uses development schema version 3. Earlier internal schemas are rejected without modifying their retained data; use a fresh configured development root. Schema migration and legacy connection import are outside this development slice.

## Deferred work

Account pairing and relay deployment, managed-cloud provisioning, cross-device access grants and permission management, collaboration grants, store-clone enrollment, cross-node execution migration, draft synchronization, stronger isolation, and broad native recovery remain deferred. Unattended boot and additional platform-specific service readiness still need qualification. None requires application-cloud dependence in the first local/SSH flow.
