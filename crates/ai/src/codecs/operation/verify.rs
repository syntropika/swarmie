use super::Whole;

use crate::codecs::error::ProviderError;
use crate::codecs::wire::{Call, Decoder, Flow, Free, Operation, Out, WireEvent, WireFrame};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verify;

impl Operation for Verify {
    type Request = ();
    type Event = std::convert::Infallible;
    type End = ();
    type Response = ();
    type Fold = Whole<Self>;
    type Emit = Free;

    fn fold(_request: &Self::Request, _call: &mut Call<'_>) -> Self::Fold {
        Whole::new()
    }
}

#[derive(Debug, Default)]
pub struct VerifyDecoder;

impl<'id> Decoder<'id, Verify> for VerifyDecoder {
    type Event = ();

    fn classify(&self, _frame: WireFrame) -> WireEvent<Self::Event> {
        WireEvent::Known(())
    }

    fn decode(&mut self, _event: (), out: Out<'id, Verify>) -> Result<Flow, ProviderError> {
        Ok(out.end(()))
    }

    fn eof(&mut self, out: Out<'id, Verify>) -> Result<Flow, ProviderError> {
        Ok(out.end(()))
    }
}
