# Session event log

## Ownership and boundaries

`swarmie.session-store` is the durable authority for session history. Its canonical Rust
contract is in `crates/session-contracts`; the compiled SQLite provider is in
`plugins/session-store-sqlite`. It is selectable through the runtime's compiled plugin catalog
with configuration `{ "database": "sessions.sqlite" }`. It is not enabled by default.
A source checkout builds without documentation contents or hosted credentials.

Tembra owns managed objects, References, RPC, Resources, State and subscription lifecycle.
SessionStore adds immutable application history, not a second runtime or EventBus. It does
not execute agents, models, tasks, tools, hooks, memory retrieval or compaction policies.
MemoryStore remains a separate key-value capability. TaskStorage remains the durable authority
for task execution state; no transaction spans these two storage contracts.

## Persistent representation

A session has a caller-supplied opaque ID, immutable portable metadata and a committed head.
An event contains an immutable ID, a namespaced kind, a positive payload version, optional
application timestamp, portable JSON payload and optional causal identifiers. The envelope
adds session ID and a contiguous per-session sequence starting at one. Application timestamps
never establish event order. Conversation, turn, task, step, attempt and parent-event IDs are
optional; producers must preserve the identifiers they use across recovery.

Identity equality is exact UTF-8 without normalization. IDs and kinds contain 1–256 bytes,
without control characters or surrounding whitespace. Sequences and timestamps fit exact
JavaScript integers. Schema version one owns the envelope; each event kind separately owns
its payload version. Unknown kinds and future positive payload versions remain losslessly
readable. Supported version-one transcript payloads are validated before append.

SessionStore creates no hidden reasoning or credential records. Producers must redact secrets
and exclude internal reasoning not legitimately exposed by a provider before submission. Portable JSON is an extensibility
boundary, not a secret detector. Model adapters may preserve only legitimately exposed,
authorized continuation data verbatim, including signatures required for tool-call round trips.
Presentation adapters must avoid displaying opaque continuation parts as conversational text. Streaming token deltas remain transient unless a producer
explicitly aggregates them into a bounded checkpoint or settled event.

The SQLite schema has dedicated metadata, sessions, ordered events and batch receipts.
Unique constraints enforce event IDs and batch IDs within a session. SQL triggers reject
updates/deletes of events and receipts, metadata changes and session deletion. Those triggers
protect against accidental SQL mutation, not a privileged database administrator. The public
API exposes no destructive history or retention operation.

## Atomic append and uncertain outcomes

1. Validate the complete request in Tembra's portable domain and canonical encoding.
2. Begin one immediate SQLite transaction.
3. Resolve an existing batch identity before evaluating CAS. An identical committed request
   returns its original receipt, even after later events. Reusing a committed batch ID with
   different events, order or expected head fails.
4. Check that the session exists and its stored record is readable, as opening requires, then
   check the optional expected head, sequence capacity and event uniqueness. Validate every
   complete stored envelope and compaction prefix before inserting any row.
5. Insert all events, update the session head, retain the complete canonical request and its
   compact receipt, and advance the store commit sequence in the same transaction.
6. Commit SQLite, then attempt adoption of the small Tembra State watermark without an
   intervening await. Publication failure retains repair work and does not replace the receipt.

Validation, duplicate and CAS refusals write nothing. Repeated event IDs under another batch
ID are refused, even if their payloads match. A producer recovering a lost response retries the
**identical original batch**, including expected head. A successful receipt contains the exact
sequence range and store commit sequence of that batch, not a later session head.

SQLite, transport or caller cancellation failures can have unknown outcome. They are never
proof that a write did not happen. Read the authoritative prefix or retry the original identity.
Reactive publication is separate: successful SQLite reads and committed writes return their
authoritative results even if State adoption fails. Every create and append attempt, including
exact retries, refusals and failures with unknown outcome, and a managed repair activity reconcile
the watermark from current database metadata. Reads never move the durable watermark and never
publish. No detached write task outlives a native invocation; synchronous SQLite work can
complete before caller cancellation is observed.

Canonical requests are retained with receipts to make equality exact rather than hash-only.
This costs additional disk space. There is no batch receipt expiry or automatic data deletion.
A future retention policy must preserve replay, deduplication and external blob ownership.

## Durability and concurrency

The provider requires a dedicated file on a local filesystem supporting OS locks and SQLite
WAL. It uses `synchronous=FULL`, foreign keys and immediate write transactions. One live
provider owns an OS lock for a database; its many Tembra clients may append concurrently.
CAS distinguishes competing appends against the same head. Symlink aliases share ownership;
hard-linked databases are refused on platforms exposing link counts. Renaming, unlinking or
replacing open database or lock files is unsupported.

A zero busy timeout refuses external contention promptly instead of waiting on Tembra's local
executor. Retry transient `session_store_io` errors using the original identity. SQLite's WAL
recovers committed frames and discards uncommitted work after process death. FULL durability
still depends on correct filesystem/device synchronization; process-kill tests are not physical
power-loss qualification. Synchronous admission bounds work per request but does not promise
a latency deadline for an unhealthy filesystem.

Schema ownership and the current supported structure are checked in a read-only transaction
before changing journal mode. Foreign, unrelated and unsupported schemas are refused without
adopting WAL, rewriting rows or deleting data. A fresh empty database is initialized atomically
in an immediate transaction that revalidates admission. The current schema indexes session,
conversation and turn identity. Reopening this schema preserves event JSON, heads, immutable
triggers and batch receipts. No prior storage format is interpreted or converted. Resource
closure releases the connection and ownership lock without deleting sessions, events, SQLite
sidecars or receipts.

## Bounded historical reads and reactive recovery

`events` accepts a session, source position, optional fixed upper sequence, optional conversation
and turn IDs, row limit, byte budget and continuation. Both selectors combine with AND. An absent
conversation selector intentionally reads the entire session, including uncorrelated events.
Model-context consumers of multiplexed sessions must select their conversation explicitly;
session-wide instructions require a separate deliberate policy and are not implicitly mixed in.
The first page freezes the committed upper sequence. The continuation records store ID, session
ID, source position, upper sequence and both filters. SQLite indexes conversation and combined
conversation/turn scans; filtered envelopes are checked against their immutable JSON correlation.
Later appends never invalidate a cursor over immutable history. Another store/session/filter
or impossible prefix is refused. On continuation, leave `after_sequence` zero; the cursor owns
that position. A complete page has no next cursor. To tail, start a new query after the completed
prefix's `through_sequence`. Filtering retains original sequence numbers; advance by source
watermarks, never by displayed entry counts.

Each append admits 1–128 events and at most 768 KiB of canonical JSON. Each complete stored
event admits at most 256 KiB. Session metadata creation admits at most 64 KiB. Event pages
admit 1–128 rows and 4 KiB–768 KiB, including wrappers and a continuation. Projection queries
require at least 5 KiB and reserve 1 KiB for their larger wrapper. A row that cannot fit the
requested budget fails explicitly; the caller may increase it. Admission and returned-page
checks use canonical UTF-8, including escaping, rather than character count. These budgets
leave room below pinned Tembra's one-MiB portable ingestion ceiling; transport envelopes keep
their own limits. Page replay uses bounded row decoding and one lookahead, not an entire log.
Event admission also reserves the complete page and projection nesting: payloads may contain
at most 124 nested containers under pinned Tembra's 128-container limit. Metadata may contain
127. Deeper values are refused before mutation, even when their append request itself would
fit, so every admitted event remains readable through the SessionStore Reference.

Oversized content belongs in a producer-owned durable blob store. `BlobReference` describes
URI, digest, byte count and media type; it does not implement retrieval, authorization or blob
retention. Publish a blob before its event, retain it with history, and never place credentials
in its URI. A failed append can leave an orphan blob that its owner must reconcile.

Tembra State contains only `{ store_id, commit_sequence }`. It is a coalescible wake-up hint,
not a durable notification queue or event feed. Subscribe before reading/catching up; capture
the source prefix, consume it fully, then read again if the watermark advanced. On reconnect,
reacquire the Reference, verify store identity and catch up from the durable source cursor.
`identity` answers the immutable store identity read when the provider's Resource opened. It
reads no history and is the cheap probe for replacement checks; ask the live provider again
whenever replacement matters rather than retaining an earlier answer or a State value.
A slow observer does not cause unbounded SessionStore buffering: Tembra owns subscription
backpressure, delta/reset behavior and closure. Store replacement initializes State from SQLite.
Global hints may wake readers for another session; readers obtain their own committed head.

`publication_status` reports durable/published watermarks, pending lag, a bounded failure count
and the last error category for the live provider. It does not require successful State adoption
and remains a diagnostic; use `identity` for store identity checks.
One pending bit coalesces repair work; no event queue grows with observer lag. A Tembra managed activity uses
the existing Host Driver, waits 50 ms between retries and opens fresh roots. It stops on activity
cancellation before resource closure. A permanently refusing State can remain behind while
SQLite stays readable; diagnostics expose this condition. Provider construction takes that
Driver explicitly, so embeddings must pass the same Driver used by their Host.

## Deterministic projections

Conversation, trajectory and model-context projections are pure, versioned source-page folds.
They retain event IDs, sequence positions, causal IDs and original payloads. Conversation
includes `swarmie.message`, `swarmie.instructions` and `swarmie.compaction` at version one.
A message stores an open speaker role, structured content, optional reply-to event ID and
optional tool-call ID. Instruction changes retain scope and content at their original positions.

Trajectory exposes every recorded fact in source order, including unknown plugin events.
Producers record model requests/results, tool input/results, transitions, errors, cancellation,
checkpoints, explicit hook interventions, usage and costs under owned kinds. Payloads are
not manufactured from task status or inferred from missing hooks. Correlation permits clients
to group these records by turn, task, step or attempt without losing the technical timeline.
This initial trajectory is a lossless ordered ledger, not an inferred execution graph.

Model context is the deterministic ordered message/instruction base. It does not automatically
apply a compaction summary. A compaction annotation names a previous source prefix, policy and
summary; its original messages remain in conversation and storage. Agent plugins choose how
to combine compaction, memory, retrieval, dynamic system instructions, filtering and token
budgets. Model Contracts own inference DTOs; an adapter translates this storage representation
to those DTOs, without SessionStore depending on their implementation or on a provider SDK.

`omitted` counts source events absent from a projection. `uninterpreted` counts unknown kinds
or unsupported payload versions, even if trajectory preserves them. A consumer requiring exact
semantic context must inspect/interpret unfamiliar events rather than treating a base page as
a complete model request. Unknown entries are never silently discarded from source history.

Projections are stateless per page, so there is no large server snapshot cache or second copy
of authoritative conversation state. Consumers can persist disposable snapshots identified by
store, session, source prefix, projection version and context-policy version, then replay the
suffix. Version mismatch requires replay. Implementing such a cache is unnecessary for bounded
base folds; future nontrivial projections must specify their own invalidation semantics.

Conversation forks are an application decision. A future fork operation must freeze a parent
prefix and define lineage/copying and repair of incomplete effects. This provider does not
pretend that new metadata alone creates an inherited conversation, nor implement a second
durable-task scheduler or mutable document database.

## Verification commands

Run from the repository root with the pinned tools:

```sh
just tembra-sdk
bun install --frozen-lockfile
just tembra-plugins
just session-test
just session-benchmark
just rust-check
```

Generated TypeScript belongs in ignored `target/generated/contracts/session-store.ts` and is
produced from the Rust descriptor by the existing Tembra generator. The JavaScript acceptance
fixture calls the real generated binding against Rust SQLite through Tembra's executor.
The separate remote gateway in tests is a fixture for uncertain RPC outcomes, not an exposed
production service or authentication policy.
