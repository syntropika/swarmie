# Authority and optional approval extensions

Each capability owns its engine, operation rules and extension interfaces. The runtime
composes declarations and transports invocations; it does not interpret pending secret
operations or model-submission exceptions. Tembra owns references, State and resource lifetime.

`swarmie-permissions` provides native authority primitives: authenticated owner sessions,
execution bindings, one-use decisions tied to exact intents, plugin grants and revocation.
The embedding must authenticate the private channel before selecting a principal. A serialized
identifier or principal label alone cannot recreate a native authority handle. Native code
sharing an issuer remains trusted; these interfaces do not isolate arbitrary native code.

## Capability-owned interactions

Vault defines `UseDecisions` and `PrivateInputs`. Its native engine implements the pending
use, input and capture flows, preserves protected custody and exposes the private
`swarmie.vault.control` presentation endpoint. A main or child logical owner binds every
physical invocation through the existing Tembra context. Retiring a physical scope does not
retire its logical conversation owner.

Providers defines `ModelDecisions`. Its native protection engine owns model-exception
requests, the final guard and its accepted preference in `model-protection.json`. The optional
private `swarmie.providers.protection` endpoint presents safe metadata and accepts exact
one-use decisions. Decisions remain bound to the original owner, attempt, destination,
content seal and policy generation. Provider protection remains usable independently of
Vault unlock or hosted account access.

The Browser may combine these read-only projections for presentation. It sends each command
to its owning capability. Neither UI state nor the runtime host can substitute an approval
flag for an exact native decision.

## Required dependencies and optional extensions

Required composition dependencies are distinct from optional extension interfaces. Missing
an optional presentation or approval adapter does not prevent the base capability from
mounting. A configured operation that needs no approval can execute without an approval UI.
If an operation still requires approval and no decision adapter is available, that operation
is unavailable; other operations remain usable. Removing an extension does not change an
accepted authorization policy or implicitly grant access.

An embedding can supply its own typed decision adapters, omit private presentation endpoints,
or select an explicitly unprotected standalone model configuration where supported. The native
application selects its final guard explicitly. The Plugin Manager's required dependency
rules continue to govern capabilities essential to that selected composition; optional
extensions must not be added to those requirements merely because they enhance a capability.

## Native application composition

`swarmie-vault-native` is separate from the base Vault crate because its native SQLite,
Cloud-client and protected-custody adapters depend on those concrete implementations.
The minimal runtime library remains independent of this native composition. The native
executable selects the Vault and Providers engines and admits their private endpoints only
on its authenticated private channel. Public and agent transports cannot acquire them.
