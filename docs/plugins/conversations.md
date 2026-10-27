# Conversations

Conversations stores immutable, caller-defined records. It does not require Agent
Loop, Durable Tasks, or PluginManager. Agent Loop provides a separate adapter that
uses these records as model history.

## Select history

A `ConversationId` identifies a history. An `EntryId` identifies one immutable record.
A `Head` selects a fixed prefix. An absent entry selects the empty root prefix.
A `HistoryPoint` selects a nonempty prefix, including an inherited entry.

`fork` creates a new conversation from a fixed history point. The branch keeps that
inherited prefix. Later appends in either conversation do not change the selected
prefix in the other. This is history branching; it does not provide collaboration
or merge policy.

Reads use `Read { at, after, limit }`. Keep `at` fixed across pages and pass the
returned cursor as `after`. A cursor from another branch is refused. The capability
caps each page at 256 entries. Reads return committed history, including while a
write is staged.

## Write typed records

Callers supply conversation identities and can retain entry identities to recognize
a write after a lost acknowledgement. Encode domain values at the storage interface:

```rust
use serde::{Deserialize, Serialize};
use swarmie_conversations::{
    Conversations, EntryDraft, Head, WriteReceipt,
};

#[derive(Serialize, Deserialize)]
enum Record {
    UserMessage(String),
}

async fn append_message(
    history: &Conversations,
    context: &tembra::Context,
    expected: Head,
) -> tembra::Result<WriteReceipt> {
    let entry = EntryDraft::encode(Record::UserMessage("Hello".into()))?;
    history.append(context, expected, vec![entry]).await
}
```

`append` compares the full expected head inside the storage transaction. A stale head
causes a conflict. It does not overwrite history. `Entry::decode` returns the
caller's record type; Conversations does not interpret its payload.

## Check the receipt

A successful call returns a `WriteReceipt` with a proposed head and confirmation:

| Confirmation | Meaning |
| --- | --- |
| `Committed` | Storage confirmed this independent write |
| `Staged` | The enclosing shared commit must still confirm the write |

Do not publish a staged head as committed. If the enclosing commit fails, that head
is not confirmed. A foreign or expired write scope must fail; it must never fall
back to an independent write. Compatible writes can join a commit only through the
same storage owner. A lost acknowledgement can leave the outcome unknown; inspect
committed records before retrying an effect.

## Storage and lifecycle

`ConversationStorage` is the backend-neutral interface. The adapter owns ordering,
principal authorization, immutable records, and transactional head checks. SQLite is
one implementation, not part of the conversation model.

The supplied Orb owns one host instance at `INSTANCE_KEY`. Each method authorizes
the invoking principal. Tembra opens and closes the capability with its physical
object lifecycle. Closing one capability releases its lease; it does not invalidate
other active leases from the same storage owner.

## Source map

- `model.rs`: identities, history selections, pages, and write receipts.
- `storage.rs`: adapter contract and typed storage errors.
- `capability.rs`: caller-facing operations and context checks.
- `orb.rs`: Tembra methods and resource lifecycle.
