//! One account node-linking intent, independent of vault values and application presentation.
//! Protected key storage and authenticated account transport belong to the embedding.
mod carrier;
mod offer;
mod protocol;
pub use carrier::*;

pub use offer::{ClaimedKey, ClaimedOffer, EndpointKeys, LinkOffer, OfferOwner};
pub use protocol::*;

mod cloud;
pub use cloud::CloudDevice;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("invalid device-linking value")]
    Invalid,
    #[error("device-linking authentication failed")]
    Forbidden,
    #[error("the provisional linking review was cancelled")]
    Cancelled,
    #[error("the linking offer expired")]
    Expired,
    #[error("the linking offer was already claimed")]
    Claimed,
    #[error("device-linking state changed")]
    Stale,
    #[error("device-linking state could not be authenticated")]
    Integrity,
    #[error("device-linking capacity is unavailable")]
    Exhausted,
    #[error("device-linking service is unavailable")]
    Unavailable,
    #[error("the original linking result is unknown")]
    Unknown,
}

pub type Result<T> = std::result::Result<T, Error>;

mod cloud_stream;
pub use cloud_stream::CloudStream;

mod flow;
pub use flow::{LinkJournal, LinkReview, LinkTarget, TargetClaim};
