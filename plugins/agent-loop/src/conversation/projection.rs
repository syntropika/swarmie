use super::{Checkpoint, History, Record, checkpoint::Phase};
use crate::{Message, model::invalid};
use std::num::NonZeroU16;
use swarmie_context::{Projection, Strategy, textual};
use swarmie_conversations::{Head, Method, Page, Read};
use tembra::Context;

impl History {
    /// Verify adoption at a paused PrepareTurn. Only the directly appended result
    /// advances the append head; the immutable task input and its head are unchanged.
    /// The owner must confirm this checkpoint through its compatible transaction
    /// before resuming. This read-only helper is not a commit receipt or activation.
    pub async fn adopt_projection(
        &self,
        context: &Context,
        checkpoint: Checkpoint,
        projection: Projection,
    ) -> tembra::Result<Checkpoint> {
        if checkpoint.phase != Phase::PrepareTurn
            || projection.artifact.after() != checkpoint.head.entry
            || projection.artifact.at().conversation != checkpoint.head.conversation
        {
            return Err(invalid(
                "projection adoption would cross unselected history",
            ));
        }
        self.projection_source(context, &projection, checkpoint.input.at)
            .await?;
        let at = projection.artifact.at();
        let mut checkpoint = checkpoint.with_projection(projection)?;
        checkpoint.head = at;
        Ok(checkpoint)
    }

    /// Resolve one immutable result using its actual append parent, separately
    /// from a paused turn's input selection. Neither lookup expands that selection.
    pub(super) async fn projection_source(
        &self,
        context: &Context,
        projection: &Projection,
        initial: Head,
    ) -> tembra::Result<Message> {
        projection.validate()?;
        if initial.conversation != projection.plan.through.conversation
            || !matches!(projection.plan.strategy, Strategy::Textual { .. })
        {
            return Err(invalid(
                "history projection is unavailable for this selection",
            ));
        }
        // The existing history provider validates the cursor against the fixed
        // initial head. This refuses a prefix that includes later task outputs or
        // drops an input outside that initial selection, without reading the prefix.
        let _: Page = self
            .reference()?
            .call(
                context,
                Method::Read.as_str(),
                Read {
                    at: initial,
                    after: Some(projection.plan.through.entry),
                    limit: NonZeroU16::MIN,
                },
            )
            .await?;
        let page: Page = self
            .reference()?
            .call(
                context,
                Method::Read.as_str(),
                Read {
                    at: projection.artifact.at(),
                    after: projection.artifact.after(),
                    limit: NonZeroU16::MIN,
                },
            )
            .await?;
        if page.next.is_some()
            || page.entries.len() != 1
            || page.entries[0].id != projection.artifact.entry()
        {
            return Err(invalid(
                "context artifact lookup did not select exactly one entry",
            ));
        }
        match page.entries[0].decode::<Record>()? {
            Record::ContextResult {
                operation,
                plan,
                response,
            } if operation == projection.operation && plan == projection.plan => {
                textual::project(projection, &textual::result(response)?)
            }
            _ => Err(invalid(
                "context artifact does not match the frozen projection",
            )),
        }
    }
}
