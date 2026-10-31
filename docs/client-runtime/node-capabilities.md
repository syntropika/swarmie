# Node capability composition

`swarmie-node::composition` selects capability declarations and native adapters for the host.
Each capability owns its service implementation and composition. The host chooses its
profile, authenticated principal and permitted services. It does not implement model,
conversation or Context behavior.

## Configuration and ownership

Declare `orbs(&host, ServicesConfiguration, execution_owner)` and mount the returned
capabilities through the runtime Plugin Manager. Supply these configuration values:

| Field | Meaning |
| --- | --- |
| `state` | Existing profile directory for public metadata and conversation history. |
| `node` | Validated `NodeId` persisted by the embedding. |
| `oauth_host` | Non-nil OAuth host UUID persisted by the embedding. Independent of the node name. |
| `principal` | User identity authenticated by the embedding's transport. |
| `environment` | Optional host configuration for native skills and project instruction sources. |
| `directory_root` | Optional permitted directory root. `None` omits the directory service. |

The embedding must hold an exclusive writer lock for the profile. It owns the host,
executor and shutdown. Calls require a Tokio local executor with I/O, timers and
blocking-task support. Installation does not create a profile lock or authenticate a
transport. On an installation error, close the host before releasing the profile lock.

Tembra owns service objects, References, State and physical invocation lifetime.
The embedding chooses the services and keys exposed by its RPC channel.
The composition does not expose a listener or a transport allowlist. Native adapters
use `swarmie_runtime::rpc`, the existing Tembra byte contract. Transport adapters
move frames; service declarations retain operation behavior and physical lifetime.

## Capability owners

| Owner | Implementation |
| --- | --- |
| `swarmie_providers::native` | Provider settings, authorization, credential references and owned file I/O. |
| `swarmie_providers::native::NativeModelProtection` | Final content guard, model exceptions and optional private approval presentation. |
| `swarmie-vault-native` | Protected Vault custody, secret interactions and optional private approval presentation. |
| `swarmie_mcp::settings` | Server definitions, administration and the host-selected file adapter. |
| `swarmie-projects` | Project records, membership and per-node folder links. |
| `swarmie-activity` | The persistent principal-scoped inbox. |
| `swarmie-conversations` | Canonical conversation history. |
| `swarmie-conversation-runtime` | Managed sessions that integrate existing history, Agent Loop, Context and Tools. |
| `swarmie_context::environment` | Scoped instruction and skill acquisition. |
| `swarmie_runtime` | Bootstrap, node identity, directory contract and the shared Tembra RPC contract. |
| `swarmie-node` | Host-selected native adapters, profile ownership and physical hosting. |
| `@swarmie/plugin-browser/connections` | Browser connection, reconnection and session fencing. |

The conversation integration package lives in `plugins/conversations/runtime`. It
uses the existing Agent Loop and history library. The history library does not depend
on its execution consumer.

## Installed capabilities

The composition declares provider settings, provider authorization, Tools, MCP
settings, Projects, Activity, conversation catalog, conversation session, Context and Commands services.
The runtime supplies node identity through its separate managed plugin.
It also installs directory browsing when `directory_root` is present.

Provider settings use the common provider catalog. Configuration reads the provider or local server model catalog.
Execution uses `swarmie_providers::ai` with one operational model catalog cache.
Context preparation reads metadata without credential access. Content admission
precedes credential custody and physical model dispatch. ChatGPT browser
sign-in uses `swarmie_providers::auth`. Credentials remain in the OS credential store.
The profile stores public account metadata in `providers.json`.

Projects and Activity use `collections.sqlite` in the selected runtime profile.
Their records persist independently of open client windows. See
[Projects and activity](../plugins/projects-activity.md) for commands and scope.

MCP definitions are stored in `mcp.json`. Conversation metadata is stored in
`conversations.json`. Canonical history is stored in `conversations.sqlite` through
the existing Conversations storage adapter. Conversation sessions use the existing
Agent Loop and Tools routing. These files do not share one atomic transaction.

The directory reader resolves paths below its configured root. It does not grant
model tools access to those paths. See [Client UI](client-ui.md) for the folder picker
and current conversation admission limits.

## Desktop embedding

`swarmie-node` supplies the shared worker, profile locks, persistent identity and
protected native composition. `apps/desktop-client/runtime` supplies the Desktop C
ABI and origin-restricted WebView adapters. Desktop selects its public and protected
profile roots and retains the shared node until application shutdown. The explicit
independent executable uses the same library. See [Node hosting](node-hosting.md)
for lifecycle, local authentication and current transport limits.

Other embeddings can use this composition with their own identity, principal, profile
and transport. No Desktop type or FFI call is required. The minimal
`swarmie-runtime` crate remains independent of this application composition.

Private approval operations belong to their capability engines. The host selects and connects
their declarations; it does not maintain a mixed queue of secret and model operations.
See [Permissions](../plugins/permissions.md) for required dependencies and optional extensions.

## Limits

This composition uses filesystem metadata, SQLite history and OS credential custody.
It is not a backend-neutral replacement for the underlying capability interfaces.
A host can compose those libraries directly when it needs different adapters.

The composition does not supply hosted account enrollment, device linking, vault
control, a public listener or transport authentication. The embedding supplies its
own authenticated channel. Provider credentials and ordinary node capability access
are separate from private secret-entry authority.

See [Skills and project instructions](../plugins/context-sources.md) for source capture,
explicit skill selection and native session reads.
