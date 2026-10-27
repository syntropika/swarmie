# Node client UI

`apps/web-client` composes the frontend. Its screens use capabilities in `plugins/*/browser`.
The application coordinates owner commands, presentation, and backend adapters.
The backend owns authoritative history, model operations, and execution.

| Capability | Frontend responsibility |
| --- | --- |
| Application Shell | Sections, navigation, history, and application commands. |
| Connections | Access to configured nodes. |
| Conversations | Drafts, message projections, and conversation extensions. |
| Views | Closeable tabs and conversation/view layout. |
| Providers | Account and model projections, selection, and configuration interfaces. |
| Projects | Project records, conversation associations, and personal folder links. |
| Settings and Onboarding | Preferences, plugin configuration, and first-use presentation. |
| Notifications and Read Aloud | Local update presentation, speech, and dictation. |
| App State | Development-only inspection. |

## Source organization

Application composition and backend coordination live in `apps/web-client/src/client`.
Screens are grouped under `shell`, `conversations`, `settings`, and `views`.
Composer action lifetime stays beside the Composer. Shared app controls remain at the
application root.

Conversations owns stable record projections and metadata updates. Application Shell owns
navigation history transitions. Views owns tab commands and restoration rules. These
modules update their existing Tembra State; they do not create another store.
The combined shell/view projection is read-only. Consumers write through `shellState`
or `views`, so one update cannot copy stale fields into the other owner.

Submission and stop handling stay together in `client/submissions.ts`. Project persistence
and known optional plugin activations have separate private modules. These are application
implementation details, not another container or lifecycle authority.

## Startup and connections

The shell starts without waiting for node networking or a capability inventory. Connections
opens independent node sessions when needed. Each session checks the expected node,
principal, and protocol version. An open socket alone is not readiness.

Connections uses Tembra RemoteHost and Transport. Browser access uses the WebSocket byte
adapter. An embedding can supply a native connector. The endpoint must serve the Tembra
protocol. Reconnect replaces the session; explicit disconnect fences late connection
attempts. Each consumer owns its acquired References and subscriptions. Ordinary capability
calls do not go through Plugin Manager. Local dependencies do not imply remote availability
or privileged backend access. See [Node connections](node-connections.md).

## State and typed local access

Tembra State holds presentation data. `packages/client-ui` exposes cached snapshots and
subscribed rendering without another state engine. Executable callbacks and UI components
stay in opaque local resources.

The published Tembra SDK derives State and managed method types directly:
`host.get(conversationsOrb.services["swarmie.conversations.ui"], "main")`.
`host.hold`, `ctx.get` and `self.get` accept the same service object. Managed calls retain
the optional trailing Context argument. There is no Swarmie acquisition wrapper or cast.

Factory-based Orbs pass their actual installed service objects into integrations.
Tembra enforces declared dependency versions and method restrictions. State inference
covers explicitly returned State handles; opaque resources still need explicit types.
RemoteHost remains a dynamic service-name boundary and does not infer remote Rust contracts.
Tembra owns acquisition, authority, invocation, and cleanup.

## Local contributions

The `ui` opaque resource of `swarmie.conversations.ui` exposes `ConversationsUiApi`:

- Composer actions, slash commands, and grouped attachment actions.
- Composer top-bar items with an owner, side, and priority.
- Navigation groups and inline header items.
- Message actions and custom renderers.
- Observers of confirmed conversation creation.

Contributions have stable IDs and owners. Ordering is deterministic. Top-bar priority
places higher values closer to the selected outer edge; stable ID breaks ties. Action
callbacks receive the conversation, node, cancellation signal, and text-insertion function.
Other contribution types use their declared inputs.

Registration returns an idempotent disposer. The contributing Tembra resource removes its
registrations when it closes. Read Aloud and the Projects integration use this pattern.
Releasing their activation References removes their contributions without clearing drafts.
Application Shell exposes a separate contract for sections and their headers or overlays.

## Providers

Providers owns accounts, available models, selection, and configuration presentation.
Models is not a separate frontend plugin. Onboarding places the same configuration UI
used by Settings; it owns neither provider implementation nor credentials. On wide screens,
the opened guide cannot be dismissed before completion. Its owner controls visibility;
first-use readiness still requires node configuration state.

An embedding supplies `ProvidersBackend` with a subscribed catalog and independently
optional configuration, authorization, and discovery capabilities. `ProvidersApi` exposes
each as a complete interface or `null`. Their methods are `configure`, `begin`, and `detect`.
Screens check availability before mounting a form.

Configuration results and authorization sessions update the owner projection even outside
Onboarding. Closing the client releases catalog subscriptions and authorization observation.
The authorization UI can open a browser sign-in, show a device code, or accept a pasted
callback. It clears callback input before forwarding it. The adapter must implement the
actual authorization protocol. Credentials and callbacks do not enter catalog State or
the inspector.

Selection belongs to an account. Effort must be supported by the selected model; models
without effort options use an empty value. Catalog updates need no manual refresh. If an
account or model disappears, retain its identity and the draft, and disable sending for
that unavailable selection.

## Admission and current limits

`ConversationExecutionApi` is an application adapter contract. The embedding supplies a
factory for the exact `NodeConnection` chosen by an operation. Its remote References must
belong to that session. Provider adapters are also application contracts, not advertised
backend administration methods.

The browser entry supplies neither execution admission nor a provider backend. It starts
with an empty provider projection and sending disabled. It does not simulate responses.
The native history operations create/get/read/append/fork do not provide submit, queue,
steer, or stop admission. Model inference alone does not provide account configuration
or authorization administration.

A new conversation is a presentation draft. It does not create node history or enter
Recent chats and project conversation lists. Its first submission requests creation.
The client binds the node before dispatch. Only an accepted acknowledgement marks the
presentation as started and notifies creation observers. Subsequent submissions do not
request creation. Changing the default node does not redirect an existing conversation.

A lost acknowledgement leaves admission unknown. Keep the draft and bound node. Do not
replay it automatically or infer rejection from a stop acknowledgement. Reject results
from an older node session. Text entered after submission must remain in the Composer.
Closing an observer is not cancellation of backend work.

**The frontend admission state is volatile.** A confirmed UI State update is not a durable
backend commit. The client has no persisted admission receipt or recovery service. A node
adapter must supply real creation, admission, and reconciliation before those guarantees
can be claimed. History append is not a substitute. Resuming queued input is unavailable
without a node admission service.

## Projects

Projects has its own Tembra State, commands, and persistence adapter. Its core does not
require Conversations or Application Shell. A separate Projects integration contributes
the application section, navigation group, header label, Composer selector, and creation
observer. Disabling it removes those contributions and returns an active Projects section
to Conversations without deleting project data.

Projects stores each conversation association once. Conversations has no project field
or project catalog. A project has a stable identity and no required execution node.
Association changes preserve draft and history. They grant neither conversation sharing
nor filesystem access. Pending project selection stays separate from saved associations
until first admission is confirmed.

The browser entry persists project records, links, and personal folder paths under a local
profile key. Folder links are grouped by node. An embedding can supply another
`ProjectsPersistence` for its profile or account. Malformed saved data is preserved and
storage failures are visible. This local store is not a shared backend authority or an
account service. It does not select files or grant access to paths. Node integration must
resolve files under its own authorization. Backend conversation history remains separate.

See [Frontend engineering](frontend.md) for contribution and verification rules, and
[Design principles](design-principles.md) for accepted interactions.
