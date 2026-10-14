//! Clock for explicitly stepped coordinator fixtures, never a runtime option.
//!
//! A scope belongs to one test thread. All participating IPC peers run in its
//! callback before a logical step advances. Reading the clock does no work;
//! spawned threads and unscoped tests retain the actual monotonic clock.
//! Real child startup, exit observation and cleanup deadlines stay on the host
//! clock. Logical scenario durations are not measurements of host/lamp latency.
use std::{
    cell::{Cell, RefCell},
    io,
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
    time::{Duration, Instant},
};

const STEP: Duration = Duration::from_millis(2);

#[derive(Clone, Copy, Debug)]
pub(crate) enum Phase {
    Run,
    /// The coordinator's one shared, real cleanup deadline.
    Shutdown {
        deadline: Instant,
    },
}

type Pump = Box<dyn FnMut(Phase) -> io::Result<()>>;

struct State {
    id: u64,
    epoch_us: u64,
    epoch: Instant,
    elapsed: Duration,
    host_deadline: Instant,
    pump: Option<Pump>,
    pumping: Option<Phase>,
}

thread_local! {
    static ACTIVE: RefCell<Option<State>> = const { RefCell::new(None) };
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
}

/// Dropping a scope restores the host clock and releases its fixture state.
/// The Rc marker prevents moving a scope to another thread or sharing it.
pub(crate) struct Scope {
    id: u64,
    _thread: PhantomData<Rc<()>>,
}

impl Scope {
    pub(crate) fn enter(watchdog: Duration) -> io::Result<Self> {
        let epoch = Instant::now();
        let host_deadline = epoch
            .checked_add(watchdog)
            .filter(|_| !watchdog.is_zero())
            .ok_or_else(|| io::Error::other("invalid scenario watchdog"))?;
        ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            if active.is_some() {
                return Err(io::Error::other("nested scenario clock scope"));
            }
            let id = NEXT_ID.with(|next| {
                let id = next.get();
                let successor = id
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("scenario clock identity exhausted"))?;
                next.set(successor);
                Ok::<_, io::Error>(id)
            })?;
            *active = Some(State {
                id,
                epoch_us: lamp_ipc::monotonic_us(),
                epoch,
                elapsed: Duration::ZERO,
                host_deadline,
                pump: None,
                pumping: None,
            });
            Ok(Self {
                id,
                _thread: PhantomData,
            })
        })
    }

    pub(crate) fn set_pump(
        &self,
        pump: impl FnMut(Phase) -> io::Result<()> + 'static,
    ) -> io::Result<()> {
        // Never run or drop caller-owned state while the TLS RefCell is borrowed.
        let mut pump: Option<Pump> = Some(Box::new(pump));
        ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            let state = active
                .as_mut()
                .filter(|state| state.id == self.id)
                .ok_or_else(|| io::Error::other("scenario clock scope is no longer active"))?;
            if state.pumping.is_some() || state.pump.is_some() {
                return Err(io::Error::other(
                    "scenario peers already installed or stepping",
                ));
            }
            state.pump = pump.take();
            Ok(())
        })
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        // Drop must also be safe during unwinding/TLS teardown. No API exposes a
        // RefCell borrow; callbacks and their destructors always run outside it.
        let removed = ACTIVE.try_with(|active| {
            let mut active = active.try_borrow_mut().ok()?;
            if active.as_ref().is_some_and(|state| state.id == self.id) {
                active.take()
            } else {
                None
            }
        });
        drop(removed);
    }
}

pub(crate) fn is_active() -> bool {
    ACTIVE.with(|active| active.borrow().is_some())
}

pub(crate) fn monotonic_us() -> u64 {
    ACTIVE.with(|active| {
        active
            .borrow()
            .as_ref()
            .map_or_else(lamp_ipc::monotonic_us, |state| {
                state.epoch_us
                    + u64::try_from(state.elapsed.as_micros()).expect("bounded scenario time")
            })
    })
}

pub(crate) fn instant() -> Instant {
    ACTIVE.with(|active| {
        active
            .borrow()
            .as_ref()
            .map_or_else(Instant::now, |state| state.epoch + state.elapsed)
    })
}

fn advance(state: &mut State, by: Duration) -> io::Result<()> {
    state.elapsed = state
        .elapsed
        .checked_add(by)
        .filter(|elapsed| elapsed.as_micros() <= u128::from(u64::MAX - state.epoch_us))
        .filter(|elapsed| state.epoch.checked_add(*elapsed).is_some())
        .ok_or_else(|| io::Error::other("scenario clock overflow"))?;
    Ok(())
}

/// Explicit stale-input fault injection inside a peer callback. Ordinary
/// scenarios only advance by STEP after all peers complete their work.
pub(crate) fn elapse_for_fault(by: Duration) -> io::Result<()> {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let state = active
            .as_mut()
            .filter(|state| matches!(state.pumping, Some(Phase::Run)))
            .ok_or_else(|| io::Error::other("clock fault requires an active run step"))?;
        advance(state, by)
    })
}

/// Pump one complete bounded peer step, then advance exactly two milliseconds.
/// Cleanup pumps retain the current logical instant and the real shared budget.
/// Returns false for an ordinary unscoped test. A callback panic invalidates its
/// scope before resuming unwind, so catching that panic cannot keep a half-step.
pub(crate) fn step(phase: Phase) -> io::Result<bool> {
    let taken = ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let Some(state) = active.as_mut() else {
            return Ok(None);
        };
        if state.pumping.is_some() {
            return Err(io::Error::other("recursive scenario peer step"));
        }
        if matches!(phase, Phase::Run) && Instant::now() >= state.host_deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "scenario host watchdog",
            ));
        }
        let pump = state
            .pump
            .take()
            .ok_or_else(|| io::Error::other("scenario peers not installed"))?;
        state.pumping = Some(phase);
        Ok(Some((state.id, pump)))
    })?;
    let Some((id, mut pump)) = taken else {
        return Ok(false);
    };
    let outcome = catch_unwind(AssertUnwindSafe(|| pump(phase)));
    if let Err(panic) = outcome {
        let removed = ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            if active.as_ref().is_some_and(|state| state.id == id) {
                active.take()
            } else {
                None
            }
        });
        drop(removed);
        drop(pump);
        resume_unwind(panic);
    }
    let result = outcome.expect("panic resumed above");
    let mut pump = Some(pump);
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let state = active
            .as_mut()
            .filter(|state| state.id == id)
            .ok_or_else(|| io::Error::other("scenario clock scope changed during peer step"))?;
        state.pumping = None;
        state.pump = pump.take();
        result?;
        if matches!(phase, Phase::Run) {
            if Instant::now() >= state.host_deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "scenario host watchdog",
                ));
            }
            advance(state, STEP)?;
        }
        Ok(true)
    })
}

/// Observe this owned fixture child's successful exit without consuming it.
/// The supervisor must still perform its real wait/reap and exit checks.
pub(crate) fn wait_child_exited(pid: u32, deadline: Instant) -> io::Result<()> {
    use rustix::process::{Pid, WaitId, WaitIdOptions, waitid};
    let child = i32::try_from(pid)
        .ok()
        .and_then(Pid::from_raw)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid fixture child PID"))?;
    loop {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("fixture child {pid} exit observation timed out"),
            ));
        }
        match waitid(
            WaitId::Pid(child),
            WaitIdOptions::EXITED | WaitIdOptions::NOWAIT | WaitIdOptions::NOHANG,
        ) {
            Ok(Some(status)) if status.exit_status() == Some(0) => return Ok(()),
            Ok(Some(status)) => {
                return Err(io::Error::other(format!(
                    "fixture child {pid} exited abnormally: {status:?}"
                )));
            }
            Ok(None) => std::thread::sleep(
                Duration::from_millis(1).min(deadline.saturating_duration_since(Instant::now())),
            ),
            Err(rustix::io::Errno::INTR) => {}
            Err(error) => {
                return Err(io::Error::other(format!(
                    "fixture child {pid} exit observation failed: {error}"
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    fn scope() -> Scope {
        let scope = Scope::enter(Duration::from_secs(10)).unwrap();
        scope.set_pump(|_| Ok(())).unwrap();
        scope
    }

    #[test]
    fn host_pause_does_not_age_a_scoped_packet_but_logical_time_does() {
        use crate::{
            process::{SessionDirectory, new_boot},
            transport::WorkerChannels,
        };
        let _scope = scope();
        let directory = SessionDirectory::create().unwrap();
        let parent = new_boot().unwrap();
        let worker = new_boot().unwrap();
        let mut tx = WorkerChannels::bind(&directory.path, "probe", parent, worker, false).unwrap();
        let mut rx = WorkerChannels::bind(&directory.path, "probe", parent, worker, true).unwrap();
        tx.connect(&directory.path, "probe", false).unwrap();
        rx.connect(&directory.path, "probe", true).unwrap();
        let before = monotonic_us();
        tx.control.send(7_u64).unwrap();
        std::thread::sleep(Duration::from_millis(70));
        assert_eq!(monotonic_us(), before);
        assert_eq!(rx.control.receive::<u64>().unwrap(), Some(7));
        tx.control.send(8_u64).unwrap();
        for _ in 0..50 {
            step(Phase::Run).unwrap();
        }
        assert_eq!(monotonic_us() - before, 100_000);
        assert!(
            rx.control.receive::<u64>().is_err(),
            "production 100 ms envelope boundary must still reject"
        );
    }

    #[test]
    fn concurrent_scopes_and_new_threads_do_not_share_a_clock() {
        let barrier = Arc::new(Barrier::new(2));
        let threads: Vec<_> = [3, 8]
            .into_iter()
            .map(|steps| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let _scope = scope();
                    let before = monotonic_us();
                    barrier.wait();
                    for _ in 0..steps {
                        step(Phase::Run).unwrap();
                    }
                    barrier.wait();
                    assert_eq!(monotonic_us() - before, steps * 2_000);
                    std::thread::spawn(|| {
                        assert!(!is_active());
                        let before = lamp_ipc::monotonic_us();
                        let value = monotonic_us();
                        assert!(value >= before && value <= lamp_ipc::monotonic_us());
                    })
                    .join()
                    .unwrap();
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert!(!is_active());
    }

    #[test]
    fn callback_keeps_clock_active_and_rejects_nesting_and_recursion() {
        let scope = Scope::enter(Duration::from_secs(10)).unwrap();
        let before = monotonic_us();
        scope
            .set_pump(move |_| {
                assert!(is_active());
                assert_eq!(monotonic_us(), before);
                assert!(Scope::enter(Duration::from_secs(1)).is_err());
                assert!(step(Phase::Run).is_err());
                Ok(())
            })
            .unwrap();
        step(Phase::Run).unwrap();
        assert_eq!(monotonic_us(), before + 2_000);
        assert!(scope.set_pump(|_| Ok(())).is_err());
    }

    #[test]
    fn callback_panic_releases_state_and_old_scope_cannot_clear_its_successor() {
        let old = Scope::enter(Duration::from_secs(10)).unwrap();
        let owned = Rc::new(());
        let captured = owned.clone();
        old.set_pump(move |_| {
            let _keep = &captured;
            panic!("injected peer panic")
        })
        .unwrap();
        assert!(catch_unwind(|| step(Phase::Run)).is_err());
        assert!(!is_active());
        assert_eq!(Rc::strong_count(&owned), 1);
        let next = scope();
        drop(old);
        assert!(is_active());
        step(Phase::Run).unwrap();
        drop(next);
        assert!(!is_active());
    }

    #[test]
    fn shutdown_steps_keep_logical_time_and_share_the_given_host_deadline() {
        let scope = Scope::enter(Duration::from_secs(10)).unwrap();
        let deadline = Instant::now() + Duration::from_millis(300);
        let calls = Rc::new(Cell::new(0));
        let observed = calls.clone();
        scope
            .set_pump(move |phase| {
                assert!(matches!(phase, Phase::Shutdown { deadline: d } if d == deadline));
                assert!(elapse_for_fault(Duration::from_millis(50)).is_err());
                observed.set(observed.get() + 1);
                Ok(())
            })
            .unwrap();
        let before = (monotonic_us(), instant());
        step(Phase::Shutdown { deadline }).unwrap();
        step(Phase::Shutdown { deadline }).unwrap();
        assert_eq!((monotonic_us(), instant()), before);
        assert_eq!(calls.get(), 2);
    }
}
