# Session integration

## Contracts and ownership

SessionStore is the source of truth for its committed session events. TaskStorage is the source
of truth for durable task transitions. The Agent Loop owns execution, context policy and
translation into Model Contracts. Inference DTOs, including provider extensions, belong to
Model Contracts. Session `Message` is a persistent transcript payload with opaque structured
content; it is not a competing hierarchy of inference content parts.

The independent Agent Loop design uses a stable turn ID, session ID, committed input-event ID,
model selection and bounded execution limits. Its task checkpoints should reference event and
operation IDs instead of copying full transcripts. This matches SessionStore's optional causal
IDs, immutable message identity and fixed-prefix projection pages. Neither the mere presence
of draft source nor a copied contract snapshot establishes final integration qualification.
The final integration must compare the accepted descriptor fingerprints and regenerate bindings.

The initial SessionStore contract is local to trusted application plugins. Remote/UI embeddings
must expose an explicitly authorized facade using Tembra's existing routing and References.
This capability does not create anonymous network access or change administration admission.

## Turn protocol

1. Create/open the session and append the user message with stable batch and event identities.
   Recover an uncertain response by retrying the exact canonical request.
2. Submit a durable turn referencing that committed message. Retain stable session, conversation,
   turn, task, step and attempt identities as applicable. There is no cross-store transaction.
3. Before each repeatable external operation, persist an intention identifying the operation.
   Retain the complete append request or reconstruct it deterministically, including CAS input.
4. Build model input from a frozen source prefix. Read all context pages, resolve unfamiliar
   kinds/versions, then apply the agent's plugins and translate into Model Contracts. Record
   authorized assembled input or a durable blob reference if exact request replay is needed.
5. Append the settled model/tool result using the stable operation identity. Only then advance
   the task checkpoint to reference that event. A crash after the append and before checkpoint
   advancement is repaired by discovering/retrying that same result append.
6. An intention without a result is an uncertain external effect, not proof that it never ran.
   Apply the effect's own idempotency/reconciliation protocol. Unsafe ambiguous effects must
   await application-owned resolution; replaying Session history does not replay effects.
7. Record turn settlement as a Session fact once the owner confirms it. Observe State and read
   turn-filtered source pages to follow progress. A cancel request and physical cancellation
   settlement are distinct facts and must not be inferred from each other.

A checkpoint can lag a Session result without data loss. Conversely, if a task commits before
Session append, reconciliation can record the task's current state but cannot recreate every
missing transition. Store ownership and task execution ownership remain separate concepts.

## Durable task observations and future outbox

The current independent lifecycle design uses Tembra series Hooks with compact
committed-transition and physical-settlement observations. It excludes task input, checkpoint
and result bodies, uses bounded best-effort delivery and records delivery diagnostics. It does
not supply durable replay. A process can die after TaskStorage commit and before hook admission,
delivery or Session append. No Session claim can repair this gap merely by subscribing earlier.

Treat a hook-derived Session entry as an **external observation**, with explicit provenance:
source store identity, task ID, revision, fact kind, source sequence when present and origin.
The current draft observation payload does not carry store identity; the embedding must bind
it to the actual selected TaskStorage identity. Never deduplicate solely by task ID/revision
across replacement stores. Physical-settlement observations need a separate fact discriminator
and stable invocation identity; they are not additional committed TaskStorage transitions.

Derive stable batch/event IDs from source identity and fact identity. Use a canonical encoding
or digest when the composed identity exceeds SessionStore's ID limit. Preserve the same payload
and batch request on retries. Successful ingestion is idempotent, but best-effort source delivery
is not thereby made durable. On reconnect or startup, scan authoritative TaskStorage snapshots
and append explicitly labeled reconciliation observations. Such snapshots recover current state;
they cannot reconstruct lost intermediate transitions or physical-settlement notifications.

A future transactional outbox must be committed atomically with selected task facts by its
owning TaskStorage implementation. A suitable fact identity is source store/commit sequence/
ordinal, with per-consumer acknowledgement and bounded scanning. A consumer appends facts to
SessionStore, confirms its receipt and only then advances the source acknowledgement. A crash
between append and acknowledgement repeats ingestion under the same identity. This provides
idempotent at-least-once ingestion when the source outbox retains facts; it does not create
an atomic transaction spanning stores. Source retention, lag, acknowledgement failure and
consumer retirement need explicit policy. This delivery neither implements nor depends on that
outbox, and never modifies task lifecycle ownership or other teams' worktrees.

## Extensions and integration checklist

A plugin owns its namespaced event kind, schema version and payload interpretation. Unknown
versions remain readable; adapters must fail or explicitly mark incomplete semantic context
rather than applying a guessed interpretation. Technical payload conventions should distinguish
intention, observed result, confirmed commit, cancellation request and physical settlement.
Record hook modifications explicitly only when the owning agent/tool policy chooses to do so.
Do not persist credentials, internal reasoning that the provider does not legitimately expose,
or every streamed token by default. Preserve provider-exposed continuation parts and opaque
signatures when authorized and necessary for round-trip model calls. The content JSON may
carry the canonical Model Contracts content array without SessionStore interpreting it.
Presentation must not automatically render opaque continuation parts as conversation text.

Before integrating independent branches, compare current accepted Contracts, event-kind
conventions, content translation, identity scoping, exact retry reconstruction and uncertainty
handling. Reconcile the shared runtime catalog and generation configuration without importing
unfinished implementations. Qualify actual Agent Loop recovery with its selected TaskStorage,
SessionStore and Model providers; SessionStore's independent acceptance cannot establish that
later end-to-end execution result.
