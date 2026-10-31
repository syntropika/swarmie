use super::Whole;

use crate::codecs::model::ModelList;
use crate::codecs::wire::{Call, Free, Operation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelListing;

#[derive(Debug, Clone)]
pub struct ModelPage {
    pub models: ModelList,
    pub next: Option<String>,
}

impl Operation for ModelListing {
    type Request = Option<String>;
    type Event = std::convert::Infallible;
    type End = ModelPage;
    type Response = ModelPage;
    type Fold = Whole<Self>;
    type Emit = Free;

    fn fold(_request: &Self::Request, _call: &mut Call<'_>) -> Self::Fold {
        Whole::new()
    }
}
