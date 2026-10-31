use crate::codecs::wasm_compat::WasmCompatSend;
use crate::codecs::wire::{Free, Operation};

pub trait Reassemble<Event>: Default + WasmCompatSend + 'static {
    fn absorb(&mut self, event: &Event);

    fn finish(self) -> serde_json::Value;
}

pub(crate) fn keep(
    map: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: &serde_json::Value,
) {
    match map.get_mut(key) {
        Some(held) if !value.is_null() && held != value => *held = value.clone(),
        Some(_) => {}
        None => {
            map.insert(key.to_owned(), value.clone());
        }
    }
}

pub(crate) fn slot<'a, T: Default>(
    map: &'a mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    kind: fn(T) -> serde_json::Value,
) -> &'a mut serde_json::Value {
    let slot = map.entry(key).or_insert(serde_json::Value::Null);
    if std::mem::discriminant(slot) != std::mem::discriminant(&kind(T::default())) {
        *slot = kind(T::default());
    }
    slot
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Unreassembled;

impl<Event> Reassemble<Event> for Unreassembled {
    fn absorb(&mut self, _event: &Event) {}

    fn finish(self) -> serde_json::Value {
        serde_json::Value::Null
    }
}

pub trait Serves<Op: Operation> {}

impl<Op: Operation<Emit = Free>> Serves<Op> for Unreassembled {}
