use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    time::{Duration, Instant},
};

use futures::future::LocalBoxFuture;
use serde_json::{Value, json};
use tembra::core;

/// Counts requests at the entire diagnostic Host's Driver boundary, without caller attribution.
#[derive(Clone, Default)]
pub struct Counts {
    spawns: u64,
    now_calls: u64,
    sleeps: BTreeMap<u128, u64>,
}

impl Counts {
    pub fn delta(&self, before: &Self) -> Self {
        Self {
            spawns: self.spawns - before.spawns,
            now_calls: self.now_calls - before.now_calls,
            sleeps: self
                .sleeps
                .iter()
                .map(|(duration, count)| {
                    (
                        *duration,
                        count - before.sleeps.get(duration).copied().unwrap_or(0),
                    )
                })
                .collect(),
        }
    }

    pub fn json(&self) -> Value {
        json!({"spawn_requests": self.spawns, "now_calls": self.now_calls,
            "sleep_requests_by_ns": self.sleeps})
    }
}

pub struct Driver {
    pub inner: Rc<tembra_native::TokioDriver>,
    counts: RefCell<Counts>,
}

impl Driver {
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            inner: tembra_native::TokioDriver::new(),
            counts: RefCell::default(),
        })
    }

    pub fn counts(&self) -> Counts {
        self.counts.borrow().clone()
    }
}

impl core::Driver for Driver {
    fn spawn(&self, task: LocalBoxFuture<'static, ()>) {
        self.counts.borrow_mut().spawns += 1;
        self.inner.spawn(task);
    }

    fn sleep(&self, duration: Duration) -> LocalBoxFuture<'static, ()> {
        *self
            .counts
            .borrow_mut()
            .sleeps
            .entry(duration.as_nanos())
            .or_default() += 1;
        self.inner.sleep(duration)
    }

    fn now(&self) -> Duration {
        self.counts.borrow_mut().now_calls += 1;
        self.inner.now()
    }
}

pub struct Snapshot {
    wall: Instant,
    cpu_ns: Option<u64>,
    counts: Counts,
    process_elapsed_ns: u128,
}

impl Snapshot {
    pub fn take(driver: &Driver, origin: Instant) -> Self {
        Self {
            counts: driver.counts(),
            cpu_ns: thread_cpu_ns(),
            wall: Instant::now(),
            process_elapsed_ns: origin.elapsed().as_nanos(),
        }
    }

    pub fn window(&self, end: &Self) -> Value {
        json!({
            "wall_ns": end.wall.duration_since(self.wall).as_nanos(),
            "thread_cpu_ns": self.cpu_ns.zip(end.cpu_ns).map(|(begin,end)| end-begin),
            "start_process_ns": self.process_elapsed_ns,
            "end_process_ns": end.process_elapsed_ns,
            "global_driver_delta": end.counts.delta(&self.counts).json(),
        })
    }

    pub fn elapsed_ns(&self) -> u128 {
        self.wall.elapsed().as_nanos()
    }
}

#[cfg(target_os = "linux")]
pub fn thread_cpu_ns() -> Option<u64> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime writes one initialized timespec at this valid pointer.
    if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut time) } != 0 {
        return None;
    }
    Some(u64::try_from(time.tv_sec).ok()? * 1_000_000_000 + u64::try_from(time.tv_nsec).ok()?)
}

#[cfg(not(target_os = "linux"))]
pub fn thread_cpu_ns() -> Option<u64> {
    None
}

#[cfg(target_os = "linux")]
pub fn thread_id() -> Option<i32> {
    // SAFETY: gettid takes no pointers and has no caller preconditions.
    Some(unsafe { libc::gettid() })
}

#[cfg(not(target_os = "linux"))]
pub fn thread_id() -> Option<i32> {
    None
}

// Exported breakpoints let a launched debugger bracket execution without production changes.
// Both calls occur outside the snapshots, so debugger stops are excluded from these windows.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn swarmie_diagnostic_execution_begin() {
    std::hint::black_box(());
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn swarmie_diagnostic_execution_end() {
    std::hint::black_box(());
}
