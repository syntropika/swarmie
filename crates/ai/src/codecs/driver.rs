use crate::codecs::error::ProviderError;
use crate::codecs::observe::AdapterSlot;
use crate::codecs::streaming::Item;
use crate::codecs::wire::document::Reassemble;
use crate::codecs::wire::{
    Call, Decoder, Flow, Mode, Operation, Out, Request, Response, Shared, Wire, WireEvent,
};
use std::sync::{Mutex, MutexGuard};
fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn warn_unmodeled(_event_type: &str, _value: &crate::codecs::streaming::UnknownPayload) {}
pub(crate) struct Decoded<Op: Operation> {
    pub(crate) outcome: Result<Op::Response, ProviderError>,
}

pub(crate) struct Tally<'a, F> {
    slot: &'a AdapterSlot,
    analysis_only: Option<fn(&F) -> bool>,
    counted: usize,
}

pub(crate) fn triage<E>(event: WireEvent<E>) -> Result<Item<E>, ProviderError> {
    match event {
        WireEvent::Known(event) => Ok(Item::Event(event)),
        WireEvent::Unknown { event_type, value } => {
            warn_unmodeled(&event_type, &value);
            Ok(Item::Unknown(value))
        }
        WireEvent::Corrupt(error) => Err(ProviderError::from(error)),
    }
}

pub(crate) fn step<'id, Op, F, D, R>(
    decoder: &mut D,
    reassembler: Option<&mut R>,
    reply: &'id Mutex<Shared<Op>>,
    frame: F,
    tally: Option<&mut Tally<'_, F>>,
) -> Result<Flow, ProviderError>
where
    Op: Operation,
    D: Decoder<'id, Op, F>,
    R: Reassemble<D::Event>,
{
    let exempt = tally
        .as_ref()
        .and_then(|tally| tally.analysis_only)
        .is_some_and(|analysis_only| analysis_only(&frame));
    let classified = decoder.classify(frame);
    if let Some(tally) = tally {
        // A corrupt frame is never exempt, whatever its shape.
        let corrupt = matches!(classified, WireEvent::Corrupt(_));
        if corrupt || !exempt {
            tally.counted += 1;
        }
        if corrupt {
            tally.slot.corrupt(tally.counted);
        }
    }
    match triage(classified)? {
        Item::Event(event) => {
            if let Some(reassembler) = reassembler {
                reassembler.absorb(&event);
            }
            decoder.decode(event, Out::new(reply))
        }
        Item::Unknown(value) => {
            Out::new(reply).unknown(value);
            Ok(Flow::More)
        }
    }
}

fn eof<'id, Op, F, D>(
    decoder: &mut D,
    reply: &'id Mutex<Shared<Op>>,
    tally: Option<&Tally<'_, F>>,
) -> Result<Flow, ProviderError>
where
    Op: Operation,
    D: Decoder<'id, Op, F>,
{
    if let Some(tally) = tally {
        tally.slot.transport_eof(tally.counted);
    }
    let step = decoder.eof(Out::new(reply));
    if !matches!(step, Ok(Flow::Ended(_)))
        && let Some(tally) = tally
    {
        tally.slot.eof(tally.counted);
    }
    match step {
        Ok(Flow::More) => Err(ProviderError::Truncated),
        step => step,
    }
}

pub(crate) fn feed<'id, Op, F, D, R>(
    decoder: &mut D,
    mut reassembler: Option<R>,
    reply: &'id Mutex<Shared<Op>>,
    frames: impl IntoIterator<Item = F>,
) -> Result<(), ProviderError>
where
    Op: Operation,
    D: Decoder<'id, Op, F>,
    R: Reassemble<D::Event>,
{
    let fed = feed_until_end(decoder, reassembler.as_mut(), reply, frames);
    record(reply, reassembler.map(|document| document.finish()));
    fed
}

fn feed_until_end<'id, Op, F, D, R>(
    decoder: &mut D,
    mut reassembler: Option<&mut R>,
    reply: &'id Mutex<Shared<Op>>,
    frames: impl IntoIterator<Item = F>,
) -> Result<(), ProviderError>
where
    Op: Operation,
    D: Decoder<'id, Op, F>,
    R: Reassemble<D::Event>,
{
    for frame in frames {
        if let Flow::Ended(_) = step(decoder, reassembler.as_deref_mut(), reply, frame, None)? {
            return Ok(());
        }
    }
    eof(decoder, reply, None).map(drop)
}

pub(crate) fn record<Op: Operation>(
    reply: &Mutex<Shared<Op>>,
    document: Option<serde_json::Value>,
) {
    if let Some(document) = document.filter(|document| !document.is_null()) {
        lock(reply).raw = Some(document);
    }
}

pub(crate) fn settle<Op: Operation>(
    shared: Mutex<Shared<Op>>,
    fed: Result<(), ProviderError>,
    reply: crate::codecs::wire::Reply,
) -> Decoded<Op> {
    let mut shared = shared
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Err(error) = fed {
        shared.items.push_back(Err(error));
    }
    shared.document = Some(reply.raw).filter(|raw| !raw.is_null());
    shared.request_id = reply.provider_request_id;
    let mut items = Vec::new();
    while let Some(item) = shared.take() {
        let failed = item.is_err();
        items.push(item);
        if failed {
            break;
        }
    }
    let outcome = match items.last() {
        Some(Err(error)) => Err(error.clone()),
        _ => shared.conclude(&reply.provider),
    };
    if let Err(error) = &outcome
        && !matches!(items.last(), Some(Err(_)))
    {
        items.push(Err(error.clone()));
    }
    Decoded { outcome }
}

pub(crate) fn decode_reply<W: Wire>(
    wire: &W,
    request: &Request<W>,
    mode: Mode,
    frames: impl IntoIterator<Item = W::Frame>,
    raw: serde_json::Value,
) -> Result<Response<W>, ProviderError> {
    let describe = wire.describe();
    let fold = <W::Op as Operation>::fold(request, &mut Call::new(&describe, mode));
    let shared = Mutex::new(Shared::new(fold));
    let fed = feed(
        &mut wire.decoder(),
        Some(wire.reassembler()),
        &shared,
        frames,
    );
    let reply = crate::codecs::wire::Reply {
        provider: describe.name.to_owned(),
        raw,
        provider_request_id: None,
    };
    settle(shared, fed, reply).outcome
}
