//! Test-only managed operation barrier for observing real admitted JavaScript generations.

use std::{
    cell::{Cell as Flag, RefCell},
    rc::Rc,
    task::{Poll, Waker},
};

use tembra::{core, prelude::*};

#[tembra::contract(local, name = "swarmie.acceptance-task-gate", version = 1)]
pub trait TaskGate {
    #[state]
    fn entered(&self) -> u32;
    #[portable]
    async fn await_gate(&self) -> Result<u32>;
    #[portable]
    async fn release_gate(&self) -> Result<()>;
}

pub const FACET_ID: &str = "fixture.task-gate";

pub fn facet() -> Result<NativeFacet> {
    NativeFacet::new(FACET_ID, vec![], |_, setup| {
        setup.serve_with_options(
            TaskGate,
            |_, _, declaration| {
                Ok(Gate {
                    entered: declaration.state("entered", 0)?,
                    opened: Rc::new(Flag::new(false)),
                    waiters: Rc::new(RefCell::new(Vec::new())),
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

struct Gate {
    entered: Cell<u32>,
    opened: Rc<Flag<bool>>,
    waiters: Rc<RefCell<Vec<Waker>>>,
}

impl TaskGateProvider for Gate {
    fn entered(&self) -> Readable<u32> {
        self.entered.readable()
    }
    async fn await_gate(&self, context: Context) -> Result<u32> {
        self.entered.change(&context, |value| {
            *value += 1;
            Ok(())
        })?;
        futures::future::poll_fn(|cx| {
            if self.opened.get() {
                Poll::Ready(Ok(1))
            } else {
                self.waiters.borrow_mut().push(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
    async fn release_gate(&self, _context: Context) -> Result<()> {
        self.opened.set(true);
        for waiter in self.waiters.borrow_mut().drain(..) {
            waiter.wake();
        }
        Ok(())
    }
}
