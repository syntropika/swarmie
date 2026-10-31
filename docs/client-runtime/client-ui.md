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

The base Browser package is `@swarmie/plugin-browser` in `plugins/browser`.
Its `/shell`, `/views`, `/settings` and `/onboarding` exports retain separate State,
commands and Tembra services. The `/onboarding/views` export contains the guide.
Feature plugins contribute sections and configuration through these contracts.

Conversations owns stable record projections and metadata updates. Application Shell owns
navigation history transitions. Views owns tab commands and restoration rules. These
modules update their existing Tembra State; they do not create another store.
The combined shell/view projection is read-only. Consumers write through `shellState`
or `views`, so one update cannot copy stale fields into the other owner.

Submission and stop handling stay together in `client/submissions.ts`. Project persistence
and known optional plugin activations have separate private modules. These are application
implementation details, not another container or lifecycle authority.

`client/remote.ts` performs dynamic method lookup only. Each node adapter retains
its own validation, cancellation and session checks. `conversation-values.ts` and
`provider-values.ts` validate their respective wire values. Catalog reads use the
bounded sequential traversal in `node-pages.ts`; only a matching revision conflict
can restart a read, once.

Node execution publishes restore-completion notifications for the exact connection.
The Composer observes these notifications directly. Catalog readiness does not write
an unchanged execution snapshot. In-flight managed calls retain their observation
slot until settlement, including the interval before admission is published.

For node conversations, metadata edits use the restored connection and run in order
for each conversation and session. Each edit reads the node catalog, applies the requested
change, and sends the positional `update(id, title, pinned, archived)` call. The local
owner commits the confirmed values after success. A rejected edit does not change local
metadata or affect an unrelated edit. The client does not retry failed calls or move
edits to a replacement session. Local conversations commit to their local owner.

## Conversation panels

New side chat opens the current conversation in a View tab. The dedicated Conversation
panel receives a new, empty draft with a different identity. The tab reads the original
record by its conversation ID. Its messages, draft, node binding and execution state stay
with that record. Closing a View tab does not delete the conversation. Closing its
underlying RPC channel cancels that channel's invocation; physical work remains owned
until settlement and the outcome is recorded for subsequent observation.

View expansion uses a temporary conversation tab. Restoring the dedicated panel removes
only this temporary tab. Side chat tabs stay open. On phones, New side chat selects the
new Conversation panel. The original conversation stays available under View.

The Conversation menu provides Rename, New side chat, Fork, Open in new window and Archive.
The Fork actions are disabled. Summary opens an empty panel. Press its header button again
to close it. The Summary panel has no close control or content.

Request status appears in the conversation after the latest message. Connection status
and draft validation messages appear above the composer. The shared `ThinkingIndicator`
shows `Working` during execution. It does not show a reasoning transcript.

Select Conversation in Settings to change Tool details. Grouped is the default. It shows
one closed disclosure for each adjacent set of tool results. Separate shows one closed
disclosure for each result. Hidden removes these rows. Open a disclosure to read its
bounded plain text. Counts include only the results in the loaded history window.
The current node projection does not provide named tool calls or their live state.

This display preference applies to all conversations in the client. The browser entry
saves it in local browser storage. Invalid stored values select Grouped. If storage is
blocked, the choice remains available for the current session. Display changes do not
change stored messages, model requests or tool execution. Request status, history limits
and unknown-outcome recovery remain visible in every mode.

## Startup and connections

Browser embeddings may serve a public JSON descriptor at `GET /__swarmie/node`, containing
`id`, `label`, `principal` and `url`. Startup validates these values and configures that node
automatically. An absolute WebSocket URL or a same-origin absolute path is accepted; a path
uses `ws` or `wss` according to the application's HTTP scheme. A missing descriptor leaves
manual connection setup available. Native embedding configuration takes precedence.
The descriptor supplies identity expectations, not authentication or provider readiness.

After optional embedding discovery, the shell starts without waiting for a node connection
or a capability inventory. Connections
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
The native host and presentation fixtures read the product choices from
`plugins/providers/catalog.json`: ChatGPT, Claude, Kimi, and Local. Settings and
Onboarding both consume the selected node's Providers State. They do not define
separate product lists. Existing accounts remain visible if their provider is outside
the current connection choices. Kimi uses an API key and the OpenAI-compatible adapter.
Models is not a separate frontend plugin. Onboarding places the same configuration UI
used by Settings; it owns neither provider implementation nor credentials. The guide opens
automatically only after the node catalog has loaded successfully with no configured accounts.
Loading, errors and unavailable catalog data do not establish an unconfigured node. An existing
account skips first-use configuration even when temporarily unavailable. Once skipped or
completed, catalog changes do not reopen the guide during that client session. A new client
session checks node configuration again rather than relying on a browser completion flag.
The guide cannot be dismissed before completion and has no manual reopening or plugin toggle
in Settings. Narrow screens use the shared sheet presentation.
The browser bootstrap enables node services after discovery. Its provider adapter reads
`swarmie.providers.settings` from the verified connection and fences results by node,
principal and connection generation. Embeddings can instead supply an independent backend;
without a catalog the guide remains closed rather than inferring missing configuration.

The wide guide separates setup progress and optional identity from the provider workspace
in two joined surfaces. Provider choices use responsive tiles from the shared connection
flow. Configuration and completion retain the same progress panel and dialog dimensions.

Settings groups compact account rows beneath persistent provider headings. Provider
and account search combines with a stock multiple-selection provider picker. Narrow
containers reflow the same controls without expanding rows or changing their scale.
Configuration opens a separate Settings page with a return control to Providers.
It shows public account identity, model defaults, and available models. The shared
`ReasoningIndicator` shows each level's relative position in that model's ordered scale.
Visible labels retain the provider's names. The same indicator is used in the Composer.
It does not compare reasoning budgets between models. Each model row has a compact
thinking-range control. It opens an informational side panel with that model's supported
levels. Opening or closing the popover does not change the saved reasoning default. Accounts remain
visible when their provider is absent from the configuration catalog. Connecting or
adding an account places the shared configuration flow in a responsive dialog;
successful configuration closes the dialog and the subscribed projection supplies
the updated account list. Onboarding continues to embed that same connection flow.
This presentation does not invent usage, balance, connection tests, account removal
or global model defaults absent from the provider contracts.

The node stores a default model, default reasoning level, and auxiliary model for each
account. `set_defaults` checks the current account catalog before it saves a change.
New conversations use these defaults. Existing conversation drafts retain their selections.
Automatic main-model selection uses the first model in the account catalog. Without a
saved reasoning value, selection uses Medium when available, then the first supported
level. A model with no reasoning levels sends no explicit effort.
`ProvidersApi.auxiliarySelection` resolves the saved auxiliary model. Automatic auxiliary
selection uses the main model because the current catalog has no small-model recommendation.
This selection interface does not start title, progress, or summary generation jobs.

An embedding supplies `ProvidersBackend` with a subscribed catalog and independently
optional configuration, authorization, discovery, and model-default capabilities.
`ProvidersApi` exposes each as a complete interface or `null`. Their methods are `configure`,
`begin`, `detect`, and `save`.
Screens check availability before mounting a form.

Configuration results and authorization sessions update the owner projection even outside
Onboarding. Closing the client releases catalog subscriptions and authorization observation.
The authorization UI can open a browser sign-in, show a device code, or accept a pasted
callback. It clears callback input before forwarding it. The adapter must implement the
actual authorization protocol. Access tokens, refresh tokens, PKCE verifiers and callbacks
do not enter catalog State or the inspector. A returning-account authorization URL may
include the provider-required signed identity-token hint; that front-channel instruction
is sensitive and must not be logged or included in diagnostic captures.

The native composition offers ChatGPT browser OAuth, API-key providers and a local model
server. OAuth reuses the Providers authentication module: PKCE, state, nonce,
issued registration and signed identity verification remain native. Public instructions
travel through a managed `swarmie.providers.authorization` session. On another device,
the user can paste the complete loopback callback URL into the existing callback form.
Credentials stay in the operating system credential store; account and model metadata are
saved separately. Refresh is serialized and rotated credentials are saved before inference.
Reconnect reuses the stored registration when the node can read and validate it; otherwise
it starts a fresh authorization. Successful fresh authorization can replace an unreadable
credential for the same verified identity, without treating it as a usable rollback point.

Selection belongs to an account. Effort must be supported by the selected model; models
without effort options use an empty value. Catalog updates need no manual refresh. If an
account or model disappears, retain its identity and the draft, and disable sending for
that unavailable selection.

## Admission and current limits

`ConversationExecutionApi` is an application adapter contract. The embedding supplies a
factory for the exact `NodeConnection` chosen by an operation. Its remote References must
belong to that session. Provider adapters are also application contracts, not advertised
backend administration methods.

The browser entry binds native admission through `swarmie.conversation.session`, with
persisted conversation metadata supplied by `swarmie.conversation.catalog`. Sending remains
disabled until the node's provider catalog and execution service are available. The native
composition uses the existing Agent Loop, Conversations storage and Tools routing; it does
not simulate responses or create another execution engine. Text messages support queue,
steer and stop. Queued invocations wait for the session's serialized execution;
the current client does not show a persistent queue of pending inputs. File attachments are currently rejected before admission.

A new conversation is a presentation draft. It does not create node history or enter
Recent chats and project conversation lists. Its first submission requests creation.
The client binds the node before dispatch. Only an accepted acknowledgement marks the
presentation as started and notifies creation observers. Subsequent submissions do not
request creation. Changing the default node does not redirect an existing conversation.

A lost acknowledgement leaves admission unknown. Keep the draft and bound node. Do not
replay it automatically or infer rejection from a stop acknowledgement. Reject results
from an older node session. Text entered after submission must remain in the Composer.
Closing the underlying RPC channel cancels its invocation. The owner records physical
settlement even when the observer is gone. Cancellation cannot undo an external effect.

**Conversation drafts and shell state are volatile.** Native admission records a submission identity and
argument digest before appending its user entry, then publishes acceptance only after a
confirmed storage commit. These application metadata and history writes are not one atomic
transaction. An interrupted preparation stays uncertain; a prepared receipt alone does not
prove admission. Duplicate identities never replay model or tool effects. Restart restores
committed messages. Model-only interruptions become stopped or failed and permit a new
explicit submission. Unfinished tools retain uncertainty until a recorded result can be
reconciled or the user explicitly acknowledges the unknown outcome. Recovery never
dispatches the tool again. It does not resume queued input automatically.

Conversation admission accepts at most 1,048,576 Unicode scalar values per text
entry. The client checks the same limit before dispatch. It rejects unpaired UTF-16
surrogates and preserves refused drafts. It does not normalize or truncate text.
The encoded transport frame and the prepared model token budget have
separate limits. The current public frame budget is 64 KiB; this path cannot yet
carry every maximum-size entry. Each conversation retains at most 1,024 submission
receipts. See [Node hosting](node-hosting.md) for the current carrier limits.
Presentation snapshots stay below 48 KiB by retaining recent messages and truncating long
display text, with explicit omission and truncation indicators. Canonical history remains
unchanged. Conversation and provider catalogs use bounded pages whose cursors refuse a
changed catalog revision. A definite pre-admission refusal preserves the draft and permits
correction; a lost acknowledgement continues to preserve uncertainty.

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

Create project uses a name field and a Source folders list. With more than one node,
the node selector changes the visible folder list. Add folder opens the node's directory
browser. Open a directory to navigate into it. Parent folder returns to its parent.
Select folder adds the current path to the form. Cancel returns without adding a path.
The project name and existing links stay in the form during navigation.
Remove deletes a link from the form. Save applies the links with the project changes.
Edit project also shows the description.

The browser calls `swarmie.directories` on the verified node session. It retains one
Reference during navigation and releases it when the picker closes. Cancellation and
node replacement discard pending results. The node supplies directory paths, parent paths
and page cursors. The client does not construct host paths from browser file handles.
An unavailable node or service shows an error and disables selection. No folder tree is
simulated. Shared dialogs become bottom sheets on phones.

The browser entry persists project records, links, and personal folder paths under a local
profile key. Folder links are grouped by node. An embedding can supply another
`ProjectsPersistence` for its profile or account. Malformed saved data is preserved and
storage failures are visible. This local store is not a shared backend authority or an
account service. It does not select files or grant access to paths. Node integration must
resolve files under its own authorization. Backend conversation history remains separate.

See [Frontend engineering](frontend.md) for contribution and verification rules, and
[Design principles](design-principles.md) for accepted interactions.
