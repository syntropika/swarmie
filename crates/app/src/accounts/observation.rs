// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
//! Each client keeps a cursor; lagging clients reconcile to the current sanitized snapshot.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use swarmie::{Error, Result};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cursor {
    owner_id: uuid::Uuid,
    revision: u64,
}
#[derive(Clone, Serialize)]
struct Observation {
    cursor: Cursor,
    account: Value,
}
pub(super) struct Observations(watch::Sender<Observation>);
impl Observations {
    pub fn new(account: Value) -> Self {
        Self(
            watch::channel(Observation {
                cursor: Cursor {
                    owner_id: uuid::Uuid::new_v4(),
                    revision: 0,
                },
                account,
            })
            .0,
        )
    }
    pub fn publish(&self, account: Value) {
        self.0.send_if_modified(|value| {
            if value.account == account {
                return false;
            }
            value.account = account;
            value.cursor.revision += 1;
            true
        });
    }
    pub async fn observe(&self, params: Value, stop: &CancellationToken) -> Result<Value> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Params {
            cursor: Option<Cursor>,
        }
        let params: Params = serde_json::from_value(params)?;
        let mut receiver = self.0.subscribe();
        let current = receiver.borrow_and_update().clone();
        if params.cursor.as_ref() == Some(&current.cursor) {
            tokio::select! {
                _ = stop.cancelled() => return Err(Error::Provider("account owner stopped".into())),
                _ = tokio::time::sleep(Duration::from_secs(25)) => {},
                result = receiver.changed() => result.map_err(|_| Error::Provider("account observation ended".into()))?,
            }
        }
        let value = receiver.borrow().clone();
        Ok(serde_json::to_value(value)?)
    }
}
