use std::{future::Future, path::Path};
use swarmie_session_contracts::*;
use swarmie_session_store_sqlite::Configuration;
use tembra::{core, prelude::*};

pub fn run<F, Fut>(test: F) -> Result<()>
where
    F: FnOnce(Host, std::rc::Rc<tembra_native::TokioDriver>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let driver = tembra_native::TokioDriver::new();
    let host = Host::with_driver("session-server", "application", driver.clone())?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(driver.run_until(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(30),
                test(host, driver.clone()),
            )
            .await
            .expect("session acceptance timed out")
        }))
}

pub async fn open(
    host: &Host,
    path: &Path,
    driver: std::rc::Rc<dyn core::Driver>,
) -> Result<SessionStoreRef> {
    host.install(
        swarmie_session_store_sqlite::facet(
            &Configuration {
                database: path.into(),
            },
            driver,
        )?,
        core::LoadOptions::default(),
    )?
    .await?;
    host.get(SessionStore, SessionStoreInput {}).await
}

pub async fn create(store: &SessionStoreRef, id: &str) -> Result<SessionRecord> {
    store
        .create(CreateSession {
            session_id: id.into(),
            metadata: serde_json::json!({}),
        })
        .await
}

pub fn event(id: impl Into<String>, kind: &str, payload: serde_json::Value) -> NewEvent {
    NewEvent {
        event_id: id.into(),
        kind: kind.into(),
        version: 1,
        correlation: Correlation::default(),
        occurred_at_ms: None,
        payload,
    }
}

pub fn message(id: impl Into<String>, role: &str, text: &str) -> NewEvent {
    event(
        id,
        MESSAGE,
        serde_json::json!({"role":role,"content":text,"reply_to":null,"tool_call_id":null}),
    )
}

pub fn batch(session: &str, id: impl Into<String>, events: Vec<NewEvent>) -> AppendEvents {
    AppendEvents {
        session_id: session.into(),
        batch_id: id.into(),
        expected_head: None,
        events,
    }
}
