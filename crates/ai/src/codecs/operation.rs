use crate::codecs::error::ProviderError;
use crate::codecs::wire::{Fold, Operation, Reply};

pub(crate) mod completion;
mod listing;
mod modality;
mod verify;

pub use completion::{Block, CallFragment, Completion, Finish, ReplyFold, merge};
pub use listing::{ModelListing, ModelPage};

pub use modality::{Embedding, ImageEmbedding, Rerank, RerankRequest, Transcription};
pub use verify::{Verify, VerifyDecoder};

pub struct Whole<Op: Operation> {
    stamp: fn(&mut Op::Response, &Reply),
}

impl<Op: Operation> Whole<Op> {
    pub fn new() -> Self {
        Self::stamping(|_, _| {})
    }

    pub fn stamping(stamp: fn(&mut Op::Response, &Reply)) -> Self {
        Self { stamp }
    }
}

impl<Op: Operation> Default for Whole<Op> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Op> Fold<Op> for Whole<Op>
where
    Op: Operation<Event = std::convert::Infallible>,
    Op::End: Into<Op::Response>,
{
    fn absorb(&mut self, event: &std::convert::Infallible) -> Result<(), ProviderError> {
        match *event {}
    }

    fn finish(self, end: Op::End, reply: Reply) -> Result<Op::Response, ProviderError> {
        let mut response = end.into();
        (self.stamp)(&mut response, &reply);
        Ok(response)
    }
}
