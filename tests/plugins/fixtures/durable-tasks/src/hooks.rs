//! Native lifecycle observers registered through actual Tembra hook contributions.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

use futures::FutureExt;
use swarmie_task_contracts::{DurableTasksRef, TaskLifecycleEvent, TaskLookup, task_lifecycle};
use tembra::{core, prelude::*};
use tembra_hooks::{HookArgs, hook_handler};

/// Shared assertions observe managed entries without owning their execution.
#[derive(Default)]
pub struct Observer {
    pub events: RefCell<Vec<(String, TaskLifecycleEvent)>>,
    pub fail: Cell<bool>,
    pub hold: Cell<bool>,
    pub started: Cell<bool>,
    pub cancelled: Cell<bool>,
    pub reentrant_cancel: RefCell<Option<(DurableTasksRef, String)>>,
    lease: RefCell<Option<core::InvocationLease>>,
}

impl Observer {
    /// Release actual physical ownership after logical cancellation has been observed.
    pub fn release(&self) {
        drop(self.lease.borrow_mut().take());
    }
}

/// Each observer is an ordinary host-owned native contribution with explicit priority.
pub fn facet(name: &str, priority: i64, observer: Rc<Observer>) -> Result<NativeFacet> {
    let name = name.to_owned();
    let definition = task_lifecycle()?;
    let contract = Rc::new(
        core::contracts::Contract::local(
            format!("fixture.lifecycle-observer.{name}"),
            1,
            serde_json::json!({}),
            BTreeMap::from([(
                "observe".into(),
                definition.descriptor().contribution_member(),
            )]),
        )
        .map_err(|error| RuntimeError::invalid("fixture_hook_contract", error.to_string()))?,
    );
    NativeFacet::new(
        format!("fixture.lifecycle-observer.{name}"),
        vec![],
        move |_, setup| {
            let observer = observer.clone();
            let name = name.clone();
            let definition = definition.clone();
            setup.core_mut().serve(
                contract.clone(),
                Rc::new(move |_, _, declaration| {
                    definition.register(declaration, name.clone(), priority, "observe")?;
                    let owner = declaration.invocation_owner();
                    let observer = observer.clone();
                    let name = name.clone();
                    let handler = hook_handler(Rc::new(
                        move |args: HookArgs<TaskLifecycleEvent, ()>, context: core::Context| {
                            let observer = observer.clone();
                            let owner = owner.clone();
                            let name = name.clone();
                            async move {
                                let event = args.input().clone();
                                observer.events.borrow_mut().push((name, event));
                                observer.started.set(true);
                                let reentrant = observer.reentrant_cancel.borrow_mut().take();
                                if let Some((tasks, id)) = reentrant {
                                    tasks.cancel(TaskLookup { id }).await?;
                                    tasks.resume().await?;
                                }
                                if observer.hold.get() {
                                    *observer.lease.borrow_mut() = Some(owner.retain(&context)?);
                                    context.cancellation().cancelled().await;
                                    observer.cancelled.set(true);
                                }
                                if observer.fail.get() {
                                    return Err(RuntimeError::invalid(
                                        "fixture_observer_failure",
                                        "expected lifecycle observation failure",
                                    ));
                                }
                                Ok(())
                            }
                            .boxed_local()
                        },
                    ));
                    Ok(core::Implementation::new().method(
                        "observe",
                        move |args: HookArgs<TaskLifecycleEvent, ()>, context| {
                            handler(args, context)
                        },
                    ))
                }),
                core::ServeOptions {
                    lifetime: core::Lifetime::Host,
                    ..Default::default()
                },
            )
        },
    )
}
