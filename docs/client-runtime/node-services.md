# Client node services

`swarmie-client-node` is the reusable node composition in `plugins/client-node`.
It installs client-facing services on a Tembra host supplied by the embedding.
It does not start a host worker, listener, process or desktop shell.

## Configuration and ownership

Call `install(&host, NodeConfiguration)` with these values:

| Field | Meaning |
| --- | --- |
| `state` | Existing profile directory for public metadata and conversation history. |
| `node` | Validated `NodeId` persisted by the embedding. |
| `oauth_host` | Non-nil OAuth host UUID persisted by the embedding. Independent of the node name. |
| `principal` | User identity authenticated by the embedding's transport. |
| `directory_root` | Optional permitted directory root. `None` omits the directory service. |

The embedding must hold an exclusive writer lock for the profile. It owns the host,
executor and shutdown. Calls require a Tokio local executor with I/O, timers and
blocking-task support. Installation does not create a profile lock or authenticate a
transport. On an installation error, close the host before releasing the profile lock.

Tembra owns service objects, References, State and physical invocation lifetime.
The embedding chooses the services and keys exposed by its RPC channel.
The composition does not expose a listener or a transport allowlist.

## Installed capabilities

The composition installs node identity, provider settings, provider authorization,
Tools, MCP settings, conversation catalog and conversation session services.
It also installs directory browsing when `directory_root` is present.

Provider settings use the common provider catalog. Configuration reads the provider or local server model catalog.
Execution uses the existing model adapters. ChatGPT browser
sign-in uses `swarmie-provider-auth`. Credentials remain in the OS credential store.
The profile stores public account metadata in `providers.json`.

MCP definitions are stored in `mcp.json`. Conversation metadata is stored in
`conversations.json`. Canonical history is stored in `conversations.sqlite` through
the existing Conversations storage adapter. Conversation sessions use the existing
Agent Loop and Tools routing. These files do not share one atomic transaction.

The directory reader resolves paths below its configured root. It does not grant
model tools access to those paths. See [Client UI](client-ui.md) for the folder picker
and current conversation admission limits.

## Desktop embedding

`apps/desktop-client/runtime` supplies the Desktop worker and C ABI. It creates the
profile directory, holds its writer lock and persists the node identity. It selects
the user directory root, passes the UUID from its existing identity file, and installs this composition with its bound principal.
It then serves bounded Tembra channels. It owns channel replacement and worker shutdown.

Other embeddings can use this composition with their own identity, principal, profile
and transport. No Desktop type or FFI call is required. The minimal
`swarmie-runtime` crate remains independent of this application composition.

## Limits

This composition uses filesystem metadata, SQLite history and OS credential custody.
It is not a backend-neutral replacement for the underlying capability interfaces.
A host can compose those libraries directly when it needs different adapters.

The composition does not supply hosted account enrollment, device linking, vault
control, a public listener or transport authentication. The embedding supplies its
own authenticated channel. Provider credentials and ordinary node capability access
are separate from private secret-entry authority.
