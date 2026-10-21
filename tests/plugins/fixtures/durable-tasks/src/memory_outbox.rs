//! Test-only atomic in-memory outbox. All state disappears with its provider Resource.

use std::collections::{BTreeMap, VecDeque};

use swarmie_task_contracts::*;
use tembra::{Result, RuntimeError};

type Position = (u64, i64);
const START: Position = (0, -1);
const MAX_EVENTS: usize = 4096;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_CONSUMERS: usize = 64;

#[derive(Default)]
pub(crate) struct MemoryOutbox {
    events: VecDeque<(TaskOutboxEvent, usize)>,
    consumers: BTreeMap<String, (Position, Position)>,
    floor: Option<TaskOutboxEventId>,
    bytes: usize,
}

fn invalid(message: &str) -> RuntimeError {
    RuntimeError::invalid("task_storage_invalid", message)
}

fn position(id: &TaskOutboxEventId, store_id: &str) -> Result<Position> {
    if id.store_id != store_id
        || id.sequence == 0
        || id.sequence > MAX_PORTABLE_INTEGER
        || id.ordinal >= 256
    {
        return Err(invalid(
            "outbox identity belongs to another store or is invalid",
        ));
    }
    Ok((id.sequence, i64::from(id.ordinal)))
}

fn id(position: Position, store_id: &str) -> Option<TaskOutboxEventId> {
    (position != START).then(|| TaskOutboxEventId {
        store_id: store_id.into(),
        sequence: position.0,
        ordinal: position.1 as u32,
    })
}

impl MemoryOutbox {
    pub fn status(&self, store_id: &str) -> TaskOutboxStatus {
        let acknowledged = self.consumers.values().map(|(ack, _)| *ack).min();
        let compactable_events = acknowledged.map_or(0, |ack| {
            self.events
                .iter()
                .take_while(|(event, _)| (event.id.sequence, i64::from(event.id.ordinal)) <= ack)
                .count() as u64
        });
        TaskOutboxStatus {
            store_id: store_id.into(),
            limits: TaskOutboxLimits {
                max_events: MAX_EVENTS as u64,
                max_bytes: MAX_BYTES as u64,
                max_consumers: MAX_CONSUMERS as u32,
            },
            retained_after: self.floor.clone(),
            retained_events: self.events.len() as u64,
            retained_bytes: self.bytes as u64,
            compactable_events,
            pending_events: self.events.len() as u64 - compactable_events,
            consumers: self
                .consumers
                .iter()
                .map(|(consumer_id, (ack, delivered))| TaskOutboxConsumerState {
                    consumer_id: consumer_id.clone(),
                    store_id: store_id.into(),
                    acknowledged: id(*ack, store_id),
                    delivered: id(*delivered, store_id),
                    retained_after: self.floor.clone(),
                })
                .collect(),
        }
    }

    fn floor(&self) -> Position {
        self.floor
            .as_ref()
            .map_or(START, |id| (id.sequence, i64::from(id.ordinal)))
    }

    fn boundary(&self, point: Position) -> Result<()> {
        if point == START
            || point == self.floor()
            || self
                .events
                .iter()
                .any(|(event, _)| (event.id.sequence, i64::from(event.id.ordinal)) == point)
        {
            Ok(())
        } else {
            Err(invalid("outbox position is not a retained boundary"))
        }
    }

    /// Validates complete append capacity without changing any provider state.
    pub fn prepare(
        &self,
        batch: &TaskTransaction,
        records: &BTreeMap<String, TaskRecord>,
        store_id: &str,
        sequence: u64,
    ) -> Result<Vec<(TaskOutboxEvent, usize)>> {
        let mut events = Vec::with_capacity(batch.writes.len());
        let mut bytes = 0;
        for (ordinal, write) in batch.writes.iter().enumerate() {
            let event = TaskOutboxEvent::from_records(
                TaskOutboxEventId {
                    store_id: store_id.into(),
                    sequence,
                    ordinal: ordinal as u32,
                },
                records.get(&write.record.id),
                &write.record,
                batch.writes.len() as u32,
            );
            let size = task_wire_bytes(&event)?;
            if size > 4096 {
                return Err(RuntimeError::invalid(
                    "task_storage_too_large",
                    "outbox event exceeds 4096 bytes",
                ));
            }
            bytes += size;
            events.push((event, size));
        }
        if self.events.len().saturating_add(events.len()) > MAX_EVENTS
            || self.bytes.saturating_add(bytes) > MAX_BYTES
        {
            return Err(RuntimeError::unavailable(
                "task_storage_outbox_full",
                "fixture outbox capacity reached",
            ));
        }
        Ok(events)
    }

    /// Runs only after every record and receipt has been validated; append cannot fail.
    pub fn append(&mut self, events: Vec<(TaskOutboxEvent, usize)>) {
        self.bytes += events.iter().map(|(_, bytes)| bytes).sum::<usize>();
        self.events.extend(events);
    }

    pub fn register(
        &mut self,
        consumer: TaskOutboxConsumer,
        store_id: &str,
    ) -> Result<TaskOutboxConsumerState> {
        validate_identity(&consumer.id)?;
        if !self.consumers.contains_key(&consumer.id) {
            if self.consumers.len() >= MAX_CONSUMERS {
                return Err(RuntimeError::unavailable(
                    "task_storage_outbox_full",
                    "fixture consumer capacity reached",
                ));
            }
            let floor = self.floor();
            self.consumers.insert(consumer.id.clone(), (floor, floor));
        }
        let (acknowledged, delivered) = self.consumers[&consumer.id];
        Ok(TaskOutboxConsumerState {
            consumer_id: consumer.id,
            store_id: store_id.into(),
            acknowledged: id(acknowledged, store_id),
            delivered: id(delivered, store_id),
            retained_after: self.floor.clone(),
        })
    }

    pub fn read(&mut self, query: TaskOutboxQuery, store_id: &str) -> Result<TaskOutboxPage> {
        validate_identity(&query.consumer_id)?;
        if !(1..=256).contains(&query.limit) {
            return Err(invalid("outbox page limit requires 1..=256"));
        }
        let (ack, delivered) = *self
            .consumers
            .get(&query.consumer_id)
            .ok_or_else(|| invalid("outbox consumer is not registered"))?;
        let after = query
            .after
            .as_ref()
            .map(|id| position(id, store_id))
            .transpose()?
            .unwrap_or(ack);
        if after < ack || after > delivered {
            return Err(invalid(
                "outbox continuation must lie in delivered unacknowledged prefix",
            ));
        }
        self.boundary(after)?;
        let mut page = TaskOutboxPage {
            store_id: store_id.into(),
            retained_after: self.floor.clone(),
            events: vec![],
            next: id(after, store_id),
            acknowledged: id(ack, store_id),
        };
        let mut bytes = task_wire_bytes(&page)?;
        let mut last = after;
        for (event, size) in self
            .events
            .iter()
            .filter(|(event, _)| (event.id.sequence, i64::from(event.id.ordinal)) > after)
            .take(query.limit as usize)
        {
            if bytes + size + 512 > MAX_TASK_PAGE_BYTES {
                break;
            }
            bytes += size + 1;
            last = (event.id.sequence, i64::from(event.id.ordinal));
            page.events.push(event.clone());
        }
        page.next = id(last, store_id);
        validate_task_payload_size(&page)?;
        if last > delivered {
            self.consumers.get_mut(&query.consumer_id).unwrap().1 = last;
        }
        Ok(page)
    }

    pub fn acknowledge(
        &mut self,
        request: TaskOutboxAcknowledgement,
        store_id: &str,
    ) -> Result<()> {
        validate_identity(&request.consumer_id)?;
        let through = position(&request.through, store_id)?;
        let (ack, delivered) = *self
            .consumers
            .get(&request.consumer_id)
            .ok_or_else(|| invalid("outbox consumer is not registered"))?;
        if through > delivered {
            return Err(invalid("outbox acknowledgement exceeds delivered prefix"));
        }
        if through > ack {
            self.boundary(through)?;
            self.consumers.get_mut(&request.consumer_id).unwrap().0 = through;
        }
        Ok(())
    }

    pub fn remove(&mut self, request: TaskOutboxConsumer) -> Result<()> {
        validate_identity(&request.id)?;
        self.consumers.remove(&request.id);
        Ok(())
    }

    pub fn compact(&mut self, request: TaskOutboxCompaction) -> Result<TaskOutboxRetention> {
        if !(1..=4096).contains(&request.limit) {
            return Err(invalid("outbox compaction limit requires 1..=4096"));
        }
        let protected = self.consumers.values().map(|(ack, _)| *ack).min();
        let mut removed = 0;
        while removed < request.limit
            && self.events.front().is_some_and(|(event, _)| {
                protected.is_some_and(|protected| {
                    (event.id.sequence, i64::from(event.id.ordinal)) <= protected
                })
            })
        {
            let (event, bytes) = self.events.pop_front().unwrap();
            self.bytes -= bytes;
            self.floor = Some(event.id);
            removed += 1;
        }
        Ok(TaskOutboxRetention {
            removed,
            retained_events: self.events.len() as u64,
            retained_bytes: self.bytes as u64,
        })
    }
}
