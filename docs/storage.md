# Storage

Storage holds domain data behind backend-neutral interfaces. Each capability defines
its own records and commit rules. A storage owner supplies a shared identity,
write-health gate, and lifetime.

Storage lives at root `storage/`. Its default `sqlite` feature exposes the implementation
in `src/sqlite/` as `swarmie_storage::sqlite`; SQLite has no separate Cargo package.
Disable default features to compile the backend-neutral API alone.

The small internal `storage/core/` package holds shared owner, scope and notification
contracts. Capabilities depend on this core, and the SQLite module implements their
storage traits. This split avoids a dependency cycle while keeping each capability
independent of a concrete backend and the composing runtime.

## Owner and leases

`StorageOwner` identifies one backing store. Clones share its state. Two owners are
not interchangeable, even when they use the same file path.

`SqliteOwner` opens its connection when the first domain lease is requested. It can
supply plugin inventory, task, and conversation storage. These leases share the
connection and owner. Closing the last lease closes the connection permanently.
Create a new owner and factories after final closure or recovery.

```rust
use swarmie_storage::sqlite::SqliteOwner;

let owner = SqliteOwner::new("swarmie.sqlite");
let inventory = owner.inventory_factory();
let tasks = owner.task_factory();
let conversations = owner.conversation_factory();
```

Pass these factories to the capability composition. Creating a factory does not
open the database. Tembra opens and closes its lease as a managed resource.

## Atomic writes

A write scope collects commands in memory. It does not hold a live transaction.
The scope derives a Tembra context that carries an opaque reference to the batch.
Participating calls must use this context and finish before the scope is sealed.

Durable Tasks performs these steps inside `Execution::commit_with`:

1. Call `StorageOwner::prepare` to create a scope.
2. Pass `WriteScope::context` to participating domain calls.
3. Call `WriteScope::seal` to obtain `PreparedChanges`.
4. Submit the batch with task progress through `TaskStorage::commit_with`.

The SQLite task adapter validates ownership and command types before it opens an
immediate transaction. It applies task changes and domain commands in that same
transaction. A refused command rolls back the entire batch.

Only compatible commands from the same owner can share a commit. The interface
does not provide atomic commits across independent stores. Storage commands must
not modify task-owned tables, publish state, perform external effects, call user
code, or control transactions.

A staged acknowledgement confirms preparation only. The encompassing execution
must confirm the commit. Dropping a scope aborts preparation and releases its
payloads. Nested scopes and writes through sealed, foreign, or cancelled scopes
are refused. Cancellation prevents further preparation and sealing; it cannot
undo a confirmed commit or an external effect.

`Execution::commit_with` creates its scope inside a short managed invocation.
Tembra releases that frame after the write settles, so sequential commits reuse
the live-frame budget. A scope created directly from a long-lived root retains its
frame until that root ends. Tembra limits a root to 4,096 live frames and context
derivation depth to 64. Dropping staged payloads alone does not release such a
root-owned frame.

Conversation storage has an explicit independent-write path when no scope is
present. A present but invalid scope always fails; it cannot fall back to an
independent commit. `SqliteOwner::stage` requires a valid scope.

## Commit outcomes

| Outcome | Meaning | Required action |
| --- | --- | --- |
| Applied | The backend confirmed the commit. | Continue with the confirmed state. |
| Rejected | Preconditions failed before commit. | Read current state before deciding whether to retry. |
| Unknown | The backend cannot confirm the commit outcome. | Recover authoritative state before further writes. |

An unknown outcome freezes the shared owner's write gate. It is not permission to
repeat an operation. Reopening does not by itself establish whether an external
effect occurred; the consuming capability must recognize or reconcile that effect.

## SQLite guarantees

Each owning connection requires WAL and `synchronous=NORMAL`. The adapter refuses
a connection that cannot establish these settings. Confirmed commits survive an
application crash; these settings do not promise persistence through power loss.
Unsupported schema versions are refused without deleting data.

SQLite work is synchronous on the calling local executor. A future returning from
this adapter does not move database work to another thread.

The task adapter separates storage operations, atomic commit handling, and row
encoding into private modules. SQL and transaction ownership stay inside the
adapter; task definitions do not need to know the backend.

## Change notifications

`ChangeSource` sends local invalidations. A notification asks a consumer to read
current state. It is not a durable event, commit receipt, or cross-process signal.
Drop a subscription to detach it.

Task commits publish their invalidations after a confirmed commit and after the
connection borrow is released. Unknown outcomes do not publish a success signal.
