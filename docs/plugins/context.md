# Context and commands

The `environment` module discovers and captures scoped AGENTS instructions and
skills for the current request. See [Context sources](context-sources.md).

`swarmie-context` provides compaction plans, projection references and operation
transitions. `swarmie-commands` provides application command metadata and invocation
through ordinary Tembra references. These libraries do not create a scheduler,
history store, transport or model provider.

## Host integration

Implement `swarmie_context::Backend` for one authorized conversation. Its `snapshot`
method reads domain state. Its `compact` and `stop` methods acknowledge durable
operation changes. Use `Session::bind` in the Context Orb factory after you validate
the declaration's actual principal and session key.

`CompactionRequest` carries the logical operation and a captured `WorkFence`. The
backend issues this non-secret UUID precondition from its persistent admission
ledger. Snapshot exposes the current fence; unavailable backends can expose None.
Available compaction requires a fence. Capturing or constructing a value grants
no authority. Stop retires its exact fence under the common owner guard, and the
backend refuses old queued admissions. Repeated operations recognize their original
association without rebinding to a newer fence. History appends do not retire it.
The optional durable input retains that same fence, separately from model identity,
metadata generation and continuation compatibility.

The session declares ordinary Tembra State. The host can publish domain changes
through `Session::state`. State is an observation. Store operation progress and
the active projection through the domain storage owner. Observation must not start
work or acquire a private execution owner. Releasing an observer does not stop work.

Call `Checkpoint::validate` after you import durable progress. `Snapshot::validate`
checks the checkpoint and a 16-KiB observation budget. Session binding, inspection,
State publication and receipts enforce this validation. Failure and refusal reasons
have a 1,024-byte display budget. These bounds do not reduce user-input or model
request limits. Supply safe status text; do not copy provider payloads into State.

Use the same short admission guard as conversation execution. Release the guard
before a provider call. Persist Stop intent before you signal the exact physical
operation. Reject a Stop for another operation. Keep shutdown separate from Stop.

## Operation transitions

A `Plan` selects a fixed history prefix, an optional previous projection, a
configuration digest and a qualified strategy. Its payload remains in immutable
history. A checkpoint contains references, not transcript copies.

`Checkpoint::request` joins compatible work already in progress. New appended
messages do not extend its fixed prefix. `dispatch` marks the external operation.
Confirm this transition before you call a provider. A lost reply or process
interruption requires recognition through `Recovery`. Only `SafeToRetry` permits
another dispatch. Stop preserves uncertain and recognized effects.

Store a validated result once and pass its history reference as a `Candidate`.
Activate it only after the storage adapter verifies prefix membership, current
configuration and execution safety. Keep the complete appended suffix. Activation
does not remove canonical history. `active_for` returns only a projection qualified
for the selected configuration. `supersede` can retire a known unused result;
it cannot discard an unknown external outcome.

`ArtifactRef` contains an exact result head and its real append parent. Its checked
constructor refuses missing entries and self-following cursors. The history adapter
reads exactly that one entry with limit 1 and no continuation cursor, then checks
the operation and complete frozen plan. `Phase::Rejected` retains a recognized paid
result that is unusable or still exceeds the next request's budget. It preserves
the required obligation and cannot dispatch or activate again. Stop retains that
result reference. An unchanged automatic intent must recognize this outcome rather
than create a sequence of paid replacements.

Activation requires `request_fits` as well as configuration, prefix and execution
safety. The owner proves this with the complete actual next preparation, current
instructions, tools and suffix under its common fencing rules. A boolean supplied
by a client is not evidence.

The optional `durable-tasks` feature exports the `swarmie.context.compaction` task
definition and its immutable input. The host registers its handler with the existing
scheduler. Provider OperationId, durable TaskId and physical AttemptId are distinct.

## Textual strategy preparation

`textual::request` encodes the complete selected context as source data in one
model request. It declares no executable tools. It does not truncate source records
or perform model calls. `textual::result` accepts a nonempty checkpoint without
tool calls and preserves its reported usage and submission-protection label.
This usage describes the summary call;
it is not the next request's occupancy. The execution owner must validate the
candidate against the next prepared request before activation.

`textual::prepare` returns the selected provider's private prepared handle after
checking the complete source budget and output reserve. Oversized source fails
with `swarmie.context.summary_too_large` before inference; it is never recursively
compacted or truncated. The host confirms dispatch before executing this handle.

The optional Agent Loop `context-projection` feature stores one `ContextResult`
record containing the operation, plan and original ModelResponse. Both usable and
rejected results retain this same artifact. Reading derives a validated Summary
in memory. It does not persist a second summary record or a synthetic chat message.
`textual::project` represents that summary as model-only Assistant source with no
tool calls and explicit JSON provenance/protection. Current instructions remain
unchanged. The source is not a new User, policy, permission or canonical response.
Native opaque continuations are refused by this textual route.

History reads only the complete suffix after the selected prefix and the separate
bounded artifact lookup. A result can be newer than a paused turn's selected input
without extending that input. Missing/mismatched references or invalid source fail
before a model call; there is no silent full-history fallback. A restored checkpoint
uses its frozen effective projection, even if another projection becomes active.

`History::adopt_projection` verifies a single adjacent ContextResult at PrepareTurn.
It advances the append cursor only over that evidence and records a private
projection override while keeping the immutable task Request unchanged. This
read-only helper is not a storage receipt or activation. The existing parent owner
must confirm the checkpoint and verify authority, membership, configuration, Stop,
actual physical settlement and the full next request before dispatch. Do not append
ContextResult across ModelPending/ToolPending or advance across unselected User/tool
records. Multiple intervening evidence records require an explicit verified chain;
the current helper refuses that gap instead of choosing the latest head.

## Prepared-request budget

`BudgetPolicy` checks the complete final `PreparedDescription` before model intent
is confirmed. Usable input is the smaller of the input limit and context window
minus the actual output reserve and configured headroom. Arithmetic is checked;
unknown metadata cannot establish an admission limit. Estimates retain their method
and precision. Reported usage from an earlier call is not current occupancy.

Overflow has two distinct typed outcomes. `swarmie.context.compaction_required`
identifies compactable history; `swarmie.context.input_too_large` identifies a fixed
request that already exceeds the usable budget. Both use `Code::Exhausted` and are
local refusals before dispatch. Consumers use the code, reason and `BudgetDecision`,
never diagnostic-message parsing.

The policy obtains a fixed lower bound by locally preparing current instructions,
all current tools and the complete latest User entry with its following tool chain.
It drops that inspection handle without executing it. This lower bound is not a
history selection: the owner must preserve every pending input, retain the complete
actual suffix, and prepare the full candidate request before activation. An invalid
or insufficient paid candidate must remain recognized and must not trigger an
unbounded automatic compaction loop.

Without a real User anchor, the default inspection floor is unavailable with
`swarmie.context.fixed_budget`. It does not infer irreducibility or delete Assistant
or tool records by role. An owner with an authoritative pending-input selection can
supply its explicit floor. Composed recovery must qualify that path or a valid
compaction route for a recoverable summary-only request.

An optional `Snapshot::inspection` publishes the safe description, budget decision,
fixed history head and stale flag. It does not expose private payloads. The host
marks an inspection stale when history or configuration advances and separately
publishes command availability. A stale value describes a previous preparation.

## Commands

Register `compact_command` in the catalog for the validated conversation key. Its
stable identity is `swarmie.context.compact`; `/compact` is presentation metadata.
The command handler and the direct `compact` method call the same bound backend.
Command invocation does not append an ordinary model message.
The compact command accepts only `CompactArguments { fence }`. The direct method
accepts one `CompactionRequest`. Context Stop retains its exact OperationId and
resolves that operation's existing association; it cannot stop a later successor.

The Commands Orb selects metadata by authenticated principal and object key.
Each registration has a lease. Owner closure withdraws only that registration.
Duplicate identities and aliases are refused. The target service authorizes the
domain operation. Metadata does not grant authority. Handler errors are returned
without automatic retries.

The RPC methods `invoke` and `command` each take one structured argument. Encode it
inside the ordinary Tembra argument array. The stable command identity remains
inside that argument; a slash alias is not an alternative wire identity.

The host must supply qualified provider execution and compatible durable storage
before these primitives can compact a conversation.

## Native conversation composition

The native conversation owner mounts Context and Commands on its existing Tembra
host. Both services use the authenticated conversation key. Context State observes
the same conversation owner as the transcript. Releasing an inspector does not stop
an admitted task.

Manual compact calls create an operation-bound task on the existing Durable Tasks
scheduler. The command uses that same admission method. Repeated operation IDs
recognize the original task; concurrent compatible requests join it. The persisted
admission fence prevents a stopped request from acquiring new authority.

Automatic compaction pauses the current turn at `PrepareTurn`. Its checkpoint
retains the compaction progress and original admission fence. The model receives
the complete selected prefix in one summary request. The latest User message and
its following records remain in the suffix. An unavailable native strategy uses
the textual strategy through the normal protected model route.

The owner stores the confirmed summary as one immutable `ContextResult`. It
activates a projection only after preparing the actual next request and checking
its budget. Canonical messages remain in history. An unusable result retains its
artifact without activation. Stop rotates the fence and signals local cancellation.
Interrupted or unknown dispatches require recognition and do not permit automatic
replay. The runtime does not add another catalog, cache or scheduler for Context.
