# Session integration

## Contracts and ownership

SessionStore is the source of truth for its committed session events. TaskStorage is the source
of truth for durable task transitions. The Agent Loop owns execution, context policy and
translation into Model Contracts. Inference DTOs, including provider extensions, belong to
Model Contracts. Session `Message` is a persistent transcript payload with opaque structured
content; it is not a competing hierarchy of inference content parts.

The Agent Loop Contract requires stable turn, session and conversation IDs, a committed input-event
ID, model selection and bounded execution limits. Its task checkpoints reference event and
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
2. Persist the task/session binding described below, then submit a durable turn referencing
   that committed message. Retain stable session, conversation, turn, task, step and attempt
   identities as applicable. There is no cross-store transaction.
3. Before each repeatable external operation, persist an intention identifying the operation.
   Retain the complete append request or reconstruct it deterministically, including CAS input.
4. Build model input from a frozen source prefix with the exact conversation selector. Read all
   context pages, resolve unfamiliar kinds/versions, then apply the agent's plugins and translate into Model Contracts. Record
   authorized assembled input or a durable blob reference if exact request replay is needed.
5. Append the settled model/tool result using the stable operation identity. Only then advance
   the task checkpoint to reference that event. A crash after the append and before checkpoint
   advancement is repaired by discovering/retrying that same result append.
6. An intention without a result is an uncertain external effect, not proof that it never ran.
   Apply the effect's own idempotency/reconciliation protocol. Unsafe ambiguous effects must
   await application-owned resolution; replaying Session history does not replay effects.
7. Record turn settlement as a Session fact once the owner confirms it. Observe State and read
   conversation- and turn-filtered source pages to follow progress. A cancel request and physical
   cancellation settlement are distinct facts and must not be inferred from each other.

A checkpoint can lag a Session result without data loss. Conversely, if a task commits before
Session append, reconciliation can record the task's current state but cannot recreate every
missing transition. Store ownership and task execution ownership remain separate concepts.

## Required Agent Loop conversation admission

SessionStore deliberately permits session-wide administrative reads and base projections.
`EventQuery::new(session_id)` is a full-session query even for `ModelContext`; its optional
selector is a storage capability, not an Agent Loop safety policy. A loop reconstructing one
conversation must enforce these requirements at its own public and recovery boundaries:

- Require a valid nonempty conversation ID on turn admission; persist it with the task/session
  binding and immutable turn identity before runnable execution. Missing identity is an error,
  including for a session that currently happens to contain only one conversation.
- Check that the committed input event belongs to that exact session and conversation. Propagate
  the identity through turn context, checkpoints, hooks and all produced message/event correlation.
- On initial execution, resume and external-result reconciliation, construct every context source
  query with the persisted conversation selector. Keep it on every continuation and reject a
  mismatched cursor. Never fall back to a full-session scan when a binding or selector is absent.
- Validate context contributions against the admitted identity. Cross-conversation retrieval or
  session-wide instructions require an explicit authorized context policy; they are not implicit
  consequences of omitted correlation. Administrative history exploration remains unrestricted
  by this loop-level guard.

Agent Loop acceptance must interleave two conversations in one session, reuse a turn label,
include unscoped entries, and force pagination. Verify exact context isolation on initial run
and after process recovery; missing conversation identity and an input event from the other
conversation must fail before task admission or any model/tool call. Recover with the persisted
binding even when the caller's current conversation changes. These checks are an integration
release gate. This SessionStore branch does not implement or claim the Agent Loop guard.

## Durable task routing association

The outbox fact identity `(store_id, sequence, ordinal)` identifies a TaskStorage transition;
it does not identify a SessionStore destination. Generic tasks need not contain `session_id`.
The Agent Loop or embedding integration adapter must own a durable routing table with at least:

- Key: `(task_store_id, task_id)`; task IDs alone are insufficient across store replacement.
- Destination: `(session_store_id, session_id, conversation_id)` and the stable turn ID when known.
- Lineage: root task ID, parent task ID and an immutable routing-format version.
- Admission: pending/active routing intent and an explicit excluded disposition for unrelated tasks.

Persist an exact routing intent before task submission. Its stable task ID makes a crash before
submission reconcilable by checking TaskStorage and retrying submission. A crash after task
commit and before marking the binding active is reconciled from that same durable intent;
consumers may resolve a committed task against its pending intent after validating the match.
If the scheduler can execute immediately, the intent must already contain all routing data and
the handler must refuse external effects until that data is durably available. Merely writing
a binding after an ordinary runnable submit leaves an unsafe crash window. There is no implied
cross-store atomicity; an embedding that cannot enforce this ordering needs an explicit gated
admission protocol in its task owner.

Child submission inherits the parent's exact persisted destination and root identity. Persist
its own binding before admission, under the child's stable ID and source store identity. An
explicit child conversation fork chooses and freezes its new conversation before submission.
The outbox's `parent` field can validate or reconcile lineage but cannot substitute for a durable
root route. Do not resolve children from transient hook context or a mutable current session.
Keep historical bindings immutable or retain their per-fact routing version; retries must never
redirect an already ingested fact after a route change.

An unresolved mapping pauses the consumer's contiguous acknowledgement prefix and exposes a
repair condition. An explicitly unrelated task may be skipped only under a durable exclusion
policy; absence of a mapping is not evidence of exclusion. Verify destination store identity
before append, and retain bindings until all required consumers and retained facts no longer
need them. Reject replacement-store mismatches rather than sending history to a same-named
session in a different database.

The default Agent Loop consumes the integration-owned `TaskSessionBindings` Contract: historical
lookup preserves the original TaskSpec, prepare retains the route, and owner admit creates the
root. Mandatory binding confirmation gates phases, actual effects and Agent Session reads/writes.
The Agent requires persisted conversation identity for context and recovery. Binding persistence
and the outbox publisher belong to the [task/session integration capability](task-session-integration.md),
while [Agent integration](agent-loop-integration.md) describes the consumer's reconciliation and
uncertain-effect boundaries. SessionStore itself implements neither task admission nor routing.

## Durable task observations and outbox

The current independent lifecycle design uses typed Tembra Hooks with series composition and compact
committed-transition and physical-settlement observations. It excludes task input, checkpoint
and result bodies, uses bounded best-effort delivery and records delivery diagnostics. It does
not supply durable replay. A process can die after TaskStorage commit and before hook admission,
delivery or Session append. No Session claim can repair this gap merely by subscribing earlier.

Treat a hook-derived Session entry as an **external observation**, with explicit provenance:
source store identity, task ID, revision, fact kind, source sequence when present and origin.
The inspected observation payload does not carry store identity; the embedding must bind
it to the actual selected TaskStorage identity. Never deduplicate solely by task ID/revision
across replacement stores. Physical-settlement observations need a separate fact discriminator
and stable invocation identity; they are not additional committed TaskStorage transitions.

Derive stable batch/event IDs from source identity and fact identity. Use a canonical encoding
or digest when the composed identity exceeds SessionStore's ID limit. Preserve the same payload
and batch request on retries. Successful ingestion is idempotent, but best-effort source delivery
is not thereby made durable. On reconnect or startup, scan authoritative TaskStorage snapshots
and append explicitly labeled reconciliation observations. Such snapshots recover current state;
they cannot reconstruct lost intermediate transitions or physical-settlement notifications.

The independently implemented TaskStorage outbox commits selected task facts atomically with
their record revisions. Its version-one event carries source store/commit sequence/ordinal,
transaction size, task identity and parent, but no session identity. It exposes bounded scanning,
per-consumer acknowledgement and retention boundaries. This implementation remains a separate
branch; this SessionStore branch does not contain or qualify an outbox consumer.

The integration consumer resolves each fact through the durable binding above, freezes a
versioned deterministic mapping, then appends one fact per Session batch using stable batch and
event IDs derived from source identity. Set `expected_head=None`: an acknowledgement lost after
a committed append must retry the identical canonical request, not a newly read session head.
Do not include a fresh timestamp, random identity or current mutable task snapshot in a retry.
Record mapping version and full source fact identity in provenance. Destination events retain
conversation, turn and task correlation. Exact batch retry returns the original receipt; a
new batch containing the same event ID is a duplicate error, not an ingestion success receipt.

Confirm the matching Session receipt before advancing the source consumer's contiguous
acknowledgement prefix. A crash between append and acknowledgement repeats that same ingestion.
Interleaved destinations and page boundaries must never acknowledge past an unresolved fact.
A source transaction can span sessions; single-fact batches make no false destination-wide
atomicity claim. Consumers requiring grouped destination semantics must explicitly handle
`transaction_size`, bounded buffering and cross-session partial delivery.

This provides idempotent at-least-once ingestion while the source retains facts and bindings.
It creates no atomic transaction spanning stores. Source retention, lag, acknowledgement
failure, excluded tasks and consumer retirement require explicit policy. The integration owner
must test process death before/after binding, task commit, Session append and acknowledgement,
child inheritance, exact retry after lost replies, mapping absence and store replacement against
the actual selected providers. Hook-only physical-settlement observations remain best effort;
the transition outbox does not manufacture durable physical-settlement facts.

The embedding must supervise the publisher as an explicitly selected managed Facet and satisfy
TaskStorage's accepted consumer-ownership contract. A consumer ID alone is not authority; do
not register or acknowledge from an unrelated caller. After confirmed prefix acknowledgement,
explicitly request source compaction under its retention policy and all required consumers.
Acknowledgement alone need not free capacity. Monitor lag, hard-capacity refusal and unresolved
bindings; no implicit skip, drain, deletion or replacement consumer is assumed. Consumer-owner
identity and dependency revisions are still being qualified in the independent task branch;
compare accepted descriptors and the actual selected providers before building the bridge.

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
