# Swarmie

Swarmie combines web and mobile clients with independently usable Rust capabilities
composed through Tembra. The native runtime is an embeddable library.

Orbs provide capability composition. The core includes plugin administration, model
providers, tool registration, an agent loop and conversations. Swarmie uses durable
agent execution; the independent libraries also support execution without persistence.
Storage is an independent interface, with a SQLite adapter for compatible atomic writes.

See [development](DEVELOPMENT.md) for setup and verification, and the
[concept guides](docs/client-runtime/index.md) for library composition and ownership.
