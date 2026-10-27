use crate::{
    Conversation, ConversationId, ConversationStorage, EntryDraft, Head, HistoryPoint, Page, Read,
    Write, WriteReceipt,
};
use std::rc::Rc;
use tembra::Context;

/// Store caller-defined records without an agent or scheduler.
///
/// ```
/// use serde::{Deserialize, Serialize};
/// use swarmie_conversations::{Conversations, EntryDraft, Head, WriteReceipt};
///
/// #[derive(Serialize, Deserialize)]
/// enum Fact { UserMessage(String) }
///
/// async fn append_message(
///     history: &Conversations,
///     context: &tembra::Context,
///     at: Head,
/// ) -> tembra::Result<WriteReceipt> {
///     let entry = EntryDraft::encode(Fact::UserMessage("Hello".into()))?;
///     history.append(context, at, vec![entry]).await
/// }
/// ```
///
/// With an ordinary Context, success confirms the write. With a shared write
/// Context, success only prepares it: inspect [`crate::WriteConfirmation`] and
/// wait for the enclosing commit before publishing the proposed head. Reads
/// always return committed history, including during preparation.
#[derive(Clone)]
pub struct Conversations {
    storage: Rc<dyn ConversationStorage>,
}

impl Conversations {
    /// Use the supplied storage adapter for all conversation operations.
    pub fn new(storage: Rc<dyn ConversationStorage>) -> Self {
        Self { storage }
    }

    /// Read the current committed head for the invoking principal.
    pub async fn get(
        &self,
        context: &Context,
        id: ConversationId,
    ) -> tembra::Result<Option<Conversation>> {
        context.check()?;
        Ok(self.storage.get(&context.principal()?, id).await?)
    }

    /// Read a bounded page at a fixed history selection.
    pub async fn read(&self, context: &Context, mut request: Read) -> tembra::Result<Page> {
        context.check()?;
        request.limit = request.limit.min(Read::MAX_LIMIT);
        Ok(self.storage.read(&context.principal()?, request).await?)
    }

    /// IDs are supplied by the caller so acknowledgement recovery can identify writes.
    pub async fn create(
        &self,
        context: &Context,
        id: ConversationId,
    ) -> tembra::Result<WriteReceipt> {
        self.write(context, Write::Create { id, origin: None })
            .await
    }

    /// Create a branch with an immutable inherited prefix.
    pub async fn fork(
        &self,
        context: &Context,
        id: ConversationId,
        origin: HistoryPoint,
    ) -> tembra::Result<WriteReceipt> {
        self.write(
            context,
            Write::Create {
                id,
                origin: Some(origin),
            },
        )
        .await
    }

    /// Append records only if the expected head still matches storage.
    pub async fn append(
        &self,
        context: &Context,
        at: Head,
        entries: Vec<EntryDraft>,
    ) -> tembra::Result<WriteReceipt> {
        self.write(context, Write::Append { at, entries }).await
    }

    async fn write(&self, context: &Context, write: Write) -> tembra::Result<WriteReceipt> {
        context.check()?;
        let head = write.head()?;
        let confirmation = self.storage.write(context, write).await?;
        Ok(WriteReceipt { head, confirmation })
    }

    /// Release this capability without closing other active storage leases.
    pub async fn close(&self) -> Result<(), crate::StorageError> {
        self.storage.close().await
    }
}
