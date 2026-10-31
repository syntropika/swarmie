//! Orb-owned network reconciliation. Reading private State performs no hosted request.
use super::*;
use futures::FutureExt;
use std::{rc::Weak, time::Duration};

pub(super) struct Worker(RefCell<Option<tokio::task::JoinHandle<()>>>);

pub(super) fn resource(
    declaration: &tembra::Declaration,
    control: Weak<Control>,
) -> tembra::Result<tembra::Resource<Worker>> {
    declaration.resource(
        "vault.reconciliation",
        1,
        move || {
            Ok(Worker(RefCell::new(Some(tokio::task::spawn_local(
                async move {
                    let mut offline_delay = 2;
                    loop {
                        let Some(control) = control.upgrade() else {
                            break;
                        };
                        let online = control.refresh().await;
                        let delay = if online {
                            offline_delay = 2;
                            Duration::from_secs(15)
                        } else {
                            let delay = Duration::from_secs(offline_delay);
                            offline_delay = (offline_delay * 2).min(30);
                            delay
                        };
                        // This schedules synchronization only. It never ends an accepted unlocked session.
                        tokio::select! {
                            _ = control.reconcile_wake.notified() => {},
                            _ = tokio::time::sleep(delay) => {},
                        }
                    }
                },
            )))))
        },
        |worker, _| {
            async move {
                let task = worker.0.borrow_mut().take();
                if let Some(task) = task {
                    task.abort();
                    let _ = task.await;
                }
                Ok(())
            }
            .boxed_local()
        },
    )
}
