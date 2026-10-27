//! Synthetic task orb. It imports neither PluginManager nor a storage adapter.

use futures::{
    FutureExt,
    channel::oneshot,
    future::{LocalBoxFuture, Shared},
};
use std::{cell::RefCell, num::NonZeroU32, rc::Rc};
use swarmie_durable_tasks::{Execution, TaskDefinition, TaskMethod};
use swarmie_foundation_counter as counter;
use tembra::{Host, Orb, Service};

pub const SERVICE: &str = "fixture.documents";

pub fn definition() -> TaskDefinition {
    TaskDefinition::new(SERVICE, NonZeroU32::new(1).unwrap()).unwrap()
}

/// Test-only coordination; no timing sleeps or external effects.
pub struct Control {
    pub seen: RefCell<Vec<u64>>,
    pause_after: Option<u64>,
    reached: RefCell<Option<oneshot::Sender<()>>>,
    reached_wait: Shared<LocalBoxFuture<'static, ()>>,
}

impl Control {
    pub fn new(pause_after: Option<u64>) -> Rc<Self> {
        let (send, receive) = oneshot::channel();
        Rc::new(Self {
            seen: RefCell::new(vec![]),
            pause_after,
            reached: RefCell::new(Some(send)),
            reached_wait: async move {
                let _ = receive.await;
            }
            .boxed_local()
            .shared(),
        })
    }

    pub async fn reached(&self) {
        self.reached_wait.clone().await;
    }
}

pub fn orb(host: Host, control: Rc<Control>) -> Orb {
    let service = Service::new(SERVICE, move |_| Ok((host.clone(), control.clone()))).method(
        TaskMethod::Execute.as_str(),
        |env, context, execution: Execution| async move {
            let total = execution.input().as_u64().ok_or_else(|| {
                tembra::Error::invalid("fixture.documents.input", "expected document count")
            })?;
            let start = execution.checkpoint().and_then(|v| v.as_u64()).unwrap_or(0);
            let reference = env.0.get(
                counter::SERVICE,
                vec![execution.id().to_string()],
                &context.principal()?,
            )?;
            for index in start + 1..=total {
                execution.check(&context)?;
                let _: u64 = reference
                    .call(&context, counter::Method::Increment.as_str(), (1_u64,))
                    .await?;
                execution.commit(&context, index.into()).await?;
                env.1.seen.borrow_mut().push(index);
                if env.1.pause_after == Some(index) {
                    if let Some(send) = env.1.reached.borrow_mut().take() {
                        let _ = send.send(());
                    }
                    execution.cancelled().await;
                    execution.check(&context)?;
                }
            }
            Ok(swarmie_durable_tasks::TaskOutcome::Completed(
                serde_json::Value::from(total),
            ))
        },
    );
    Orb::new(SERVICE, 1).service(service)
}
