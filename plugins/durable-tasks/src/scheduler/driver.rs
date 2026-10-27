use super::{Engine, SchedulerDiagnostic};
use crate::{TaskChange, TaskId, execution::Scope};
use futures::{FutureExt, StreamExt, channel::mpsc, stream::FuturesUnordered};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    time::Duration,
};
use tembra::Context;

impl Engine {
    pub(super) async fn drive(
        self: Rc<Self>,
        receive: mpsc::UnboundedReceiver<()>,
    ) -> tembra::Result<()> {
        let host = self.host.clone();
        host.root(crate::HOST_PRINCIPAL, move |context| {
            self.run_loop(context, receive).boxed_local()
        })
        .await
    }

    async fn run_loop(
        self: Rc<Self>,
        context: Context,
        mut receive: mpsc::UnboundedReceiver<()>,
    ) -> tembra::Result<()> {
        let mut active: BTreeMap<TaskId, Rc<Scope>> = BTreeMap::new();
        let mut calls = FuturesUnordered::new();
        let mut held = BTreeSet::new();
        let interval = Duration::from_millis((self.options.lease_duration_ms.get() / 3).max(1));
        let mut timer = (self.options.timer)(interval).fuse();
        let mut waiting_timer = futures::future::pending::<()>().boxed_local().fuse();
        let mut closing = self.signal.cancelled().boxed_local().fuse();
        loop {
            if self.is_stopping(&context) {
                self.control.stopping.set(true);
                for scope in active.values() {
                    scope.stop();
                }
            }
            if let Err(reason) = self.authority.check() {
                self.control.status.borrow_mut().diagnostic =
                    Some(SchedulerDiagnostic::Storage(reason.clone().into()));
                for scope in active.values() {
                    scope.revoke();
                }
            }
            if self.control.retry.replace(false) {
                self.cursor.set(0);
                held.clear();
                self.control.status.borrow_mut().interrupted.clear();
                self.control.status.borrow_mut().diagnostic = None;
            }
            let capacity = if self.options.max_executions == 0 {
                usize::MAX
            } else {
                self.options.max_executions.saturating_sub(active.len())
            };
            if self.control.dirty.replace(false)
                && self.control.enabled.get()
                && !self.control.stopping.get()
                && self.authority.check().is_ok()
                && capacity > 0
            {
                match self.plan(&active, &held, capacity).await {
                    Ok((selected, more)) => {
                        if more {
                            self.control.wake(false);
                        }
                        waiting_timer = match self.next_wait.get() {
                            Some(duration) => (self.options.timer)(duration).fuse(),
                            None => futures::future::pending().boxed_local().fuse(),
                        };
                        let grants = self.claim(&context, &selected, &mut held).await;
                        // Claims can settle after cancellation; never dispatch them then.
                        if self.is_stopping(&context) {
                            self.control.stop();
                            let releases = grants
                                .into_iter()
                                .map(|g| TaskChange::Release {
                                    id: g.id,
                                    attempt: g.assignment.attempt,
                                })
                                .collect();
                            if self.authority.check().is_ok() {
                                let _ = self.authority.commit(releases).await;
                            }
                        } else {
                            for grant in grants {
                                let id = grant.id;
                                let scope = Scope::new(grant, self.authority.clone());
                                active.insert(id, scope.clone());
                                calls.push(self.clone().invoke(scope).boxed_local());
                            }
                        }
                    }
                    Err(diagnostic) => {
                        self.control.status.borrow_mut().diagnostic = Some(diagnostic);
                    }
                }
            }
            self.control.status.borrow_mut().active = active.len();
            if self.control.stopping.get() && active.is_empty() {
                break;
            }
            futures::select_biased! {
                _ = closing => {
                    self.control.stop();
                    for scope in active.values() { scope.stop(); }
                    closing = futures::future::pending().boxed_local().fuse();
                },
                _ = waiting_timer => {
                    self.control.wake(false);
                    waiting_timer = futures::future::pending().boxed_local().fuse();
                },
                _ = timer => {
                    if context.check().is_err() { self.control.stop(); }
                    if let Err(reason) = self.renew(&active).await {
                        self.control.status.borrow_mut().diagnostic = Some(SchedulerDiagnostic::Storage(reason.into()));
                    }
                    if self.control.enabled.get() { self.control.wake(false); }
                    timer = (self.options.timer)(interval).fuse();
                },
                done = async {
                    if calls.is_empty() { futures::future::pending().await }
                    else { calls.next().await }
                }.fuse() => {
                    if let Some(done) = done {
                        let mut settled = |(id, diagnostic)| {
                            active.remove(&id);
                            if let Some(diagnostic) = diagnostic {
                                held.insert(id);
                                self.control.status.borrow_mut().interrupted.push(id);
                                self.control.status.borrow_mut().diagnostic = Some(diagnostic);
                            }
                        };
                        settled(done);
                        for _ in 1..64 {
                            // Bound work before the outer loop polls timers and control messages.
                            if self.is_stopping(&context) || self.authority.check().is_err() {
                                break;
                            }
                            // Register the actual scheduler waker when a pending invocation is polled.
                            let done = futures::future::poll_fn(|cx| {
                                std::task::Poll::Ready(match calls.poll_next_unpin(cx) {
                                    std::task::Poll::Ready(done) => done,
                                    std::task::Poll::Pending => None,
                                })
                            }).await;
                            match done {
                                Some(done) => settled(done),
                                None => break,
                            }
                        }
                        self.control.wake(false);
                    }
                },
                message = receive.next().fuse() => {
                    if message.is_none() { self.control.stop(); }
                },
            }
            let mut yielded = false;
            futures::future::poll_fn(|cx| {
                if yielded {
                    std::task::Poll::Ready(())
                } else {
                    yielded = true;
                    cx.waker().wake_by_ref();
                    std::task::Poll::Pending
                }
            })
            .await;
        }
        self.control.status.borrow_mut().enabled = false;
        Ok(())
    }
}
