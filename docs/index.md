# Implementation guides

Swarmie composes independent capabilities through Tembra. The embedding selects
capabilities, supplies I/O and owns the executor.

| Concept | Guide |
| --- | --- |
| Rust verification and source boundaries | [Rust source checks](rust-source-checks.md) |
| Host startup and shutdown | [Runtime](runtime.md) |
| Desired plugins and observed mounts | [PluginManager](plugins/plugin-manager.md) |
| Authority and optional approval extensions | [Permissions](plugins/permissions.md) |
| Write ownership and atomic preparation | [Storage](storage.md) |
| Recoverable work and leases | [Durable Tasks](plugins/durable-tasks.md) |
| Model and tool execution | [Agent Loop](plugins/agent-loop.md) |
| Project grouping and the persistent inbox | [Projects and activity](plugins/projects-activity.md) |
| Persistent history and branches | [Conversations](plugins/conversations.md) |
| Context projections and application commands | [Context and commands](plugins/context.md) |
| Independent protocol preparation and transport | [AI protocol library](ai.md) |
| Provider requests and streaming | [Providers](plugins/providers.md) |
| Cloud account sessions and device linking | [Swarmie Cloud client](plugins/cloud.md) |
| Account selection and credential workflows | [Provider accounts](plugins/provider-accounts.md) |
| Model metadata and preparation | [Model catalog](plugins/model-catalog.md) |
| Skills and scoped project instructions | [Context sources](plugins/context-sources.md) |
| Tool discovery and provider routing | [Tools](plugins/tools.md) |
| Local and HTTP MCP tool connections | [MCP](plugins/mcp.md) |
| Node identity and remote destinations | [Node connections](client-runtime/node-connections.md) |
| Native capability composition | [Node capability composition](client-runtime/node-capabilities.md) |
| Shared protected hosting and independent executable | [Node hosting](client-runtime/node-hosting.md) |
| Browser composition and views | [Client UI](client-runtime/client-ui.md) |

Frontend rules and design guidance remain in [Frontend](client-runtime/frontend.md) and
[Design principles](client-runtime/design-principles.md). Setup and verification commands are in
[Development](../DEVELOPMENT.md).

## Vault

See the [node vault contract](client-runtime/vault.md).
