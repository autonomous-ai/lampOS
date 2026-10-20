//! A service restart token comes from a physical or software mute, followed by
//! verified cleanup. Unknown privacy is never resumable on the direct GPIO
//! route; on the Controls route it is closed and read again
//! ([`PrivacyUnavailable`], #795). Failed cleanup never is.
use std::{fmt, io};

pub(crate) type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug)]
pub(crate) struct PhysicalMute {
    pub(crate) observed_at_us: u64,
    pub(crate) onboarding: Option<crate::onboarding::privacy::Checkpoint>,
}
impl fmt::Display for PhysicalMute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "physical mute observed at {} us", self.observed_at_us)
    }
}
impl std::error::Error for PhysicalMute {}

/// The coordinator has applied its software inhibit. This never asserts that
/// the physical privacy switch was observed closed.
#[derive(Debug)]
pub(crate) struct SoftwareMute {
    pub(crate) observed_at_us: u64,
    /// A hand held on the head put the lamp to sleep (#711).
    pub(crate) asleep: bool,
}
impl fmt::Display for SoftwareMute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "software mute observed at {} us", self.observed_at_us)
    }
}
impl std::error::Error for SoftwareMute {}

/// The selected Controls route's switch could not be read: no source, a
/// fault, stale or broken evidence, or its worker gone (#795; lamp-4ace,
/// 2026-10-16 10:04, a crash loop). Closed, as a mute is: the microphones
/// stay shut, and after verified cleanup the service waits for fresh
/// evidence instead of exiting. Reopening still needs that evidence and its
/// steady open time. Only the selected route raises it; the direct GPIO
/// route keeps ending on unknown privacy.
#[derive(Debug)]
pub(crate) struct PrivacyUnavailable {
    pub(crate) why: String,
}
impl fmt::Display for PrivacyUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "privacy switch unreadable: {}", self.why)
    }
}
impl std::error::Error for PrivacyUnavailable {}

/// A [`PrivacyUnavailable`] for `why`.
pub(crate) fn unavailable(why: impl fmt::Display) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(PrivacyUnavailable {
        why: why.to_string(),
    })
}

/// The same, carried by an `io::Error` where only that fits.
pub(crate) fn unavailable_io(why: impl fmt::Display) -> io::Error {
    io::Error::other(PrivacyUnavailable {
        why: why.to_string(),
    })
}

/// Whether `error` is a [`PrivacyUnavailable`], itself or inside an
/// `io::Error`. A failed cleanup never is: [`finish`] hides it.
pub(crate) fn is_unavailable(error: &(dyn std::error::Error + 'static)) -> bool {
    error.is::<PrivacyUnavailable>()
        || error
            .downcast_ref::<io::Error>()
            .and_then(io::Error::get_ref)
            .is_some_and(|inner| inner.is::<PrivacyUnavailable>())
}

#[derive(Debug)]
struct CleanupFailed {
    original: Option<Box<dyn std::error::Error + Send + Sync>>,
    cleanup: io::Error,
}
impl fmt::Display for CleanupFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(original) = &self.original {
            write!(f, "{original}; ")?;
        }
        write!(f, "verified cleanup failed: {}", self.cleanup)
    }
}
impl std::error::Error for CleanupFailed {}

/// Cleanup failure must not retain a downcastable mute restart token.
pub(crate) fn finish<T>(result: Result<T>, cleanup: io::Result<()>) -> Result<T> {
    match cleanup {
        Ok(()) => result,
        Err(cleanup) => Err(Box::new(CleanupFailed {
            original: result.err(),
            cleanup,
        })),
    }
}

use crate::{
    privacy::{PrivacyGate, PrivacyState},
    process::{SessionDirectory, Worker, new_boot},
    termination::Termination,
    wire::{Control, WorkerEvent},
};
use lamp_ipc::monotonic_us;
use lamp_turn_owner::{BootId, Controller, MonoTime, Permission};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WaitOutcome {
    Allowed,
    Stopped,
}

/// Only this GPIO worker exists during the wait. Its authority snapshot remains
/// denied; observing stable unmute never grants microphone or camera permission.
#[cfg(test)]
pub(crate) fn wait(
    executable: &Path,
    termination: &Termination,
    on_state: impl FnMut(PrivacyState, BootId, u64) -> Result<()>,
) -> Result<WaitOutcome> {
    wait_with_gate(executable, termination, on_state, || Ok(true))
}

/// Wait without opening a worker until the additional gate allows it. Once the
/// GPIO worker exists, poll the gate on every loop, including an unchanged
/// closed slide. A false gate never substitutes for a physical observation.
/// The callback runs only while audio workers are absent.
pub(crate) fn wait_with_gate(
    executable: &Path,
    termination: &Termination,
    on_state: impl FnMut(PrivacyState, BootId, u64) -> Result<()>,
    mut additional_gate: impl FnMut() -> Result<bool>,
) -> Result<WaitOutcome> {
    loop {
        if termination.signal_name().is_some() {
            return Ok(WaitOutcome::Stopped);
        }
        if additional_gate()? {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let directory = SessionDirectory::create()?;
    let boot = new_boot()?;
    let worker =
        Worker::spawn_with_tick(executable, &directory.path, "privacy", boot, &[], || {
            termination.check()
        })?;
    let result = wait_worker_with_gate(worker, boot, termination, on_state, additional_gate);
    // Worker endpoints and the GPIO owner have gone before removing this empty
    // directory. Refuse another cycle if anything unexpected remains.
    finish(result, fs::remove_dir(&directory.path))
}

#[cfg(test)]
pub(crate) fn wait_worker(
    worker: Worker,
    boot: BootId,
    termination: &Termination,
    on_state: impl FnMut(PrivacyState, BootId, u64) -> Result<()>,
) -> Result<WaitOutcome> {
    wait_worker_with_gate(worker, boot, termination, on_state, || Ok(true))
}

pub(crate) fn wait_worker_with_gate(
    mut worker: Worker,
    boot: BootId,
    termination: &Termination,
    on_state: impl FnMut(PrivacyState, BootId, u64) -> Result<()>,
    additional_gate: impl FnMut() -> Result<bool>,
) -> Result<WaitOutcome> {
    let result = wait_loop_with_gate(&mut worker, boot, termination, on_state, additional_gate);
    // Bounded cooperative cleanup is outside every active audio loop. A forced
    // kill or abnormal exit stays a failure even when unmute was observed.
    let shutdown = worker.shutdown();
    finish(result, shutdown)
}

#[cfg(test)]
fn wait_loop(
    worker: &mut Worker,
    boot: BootId,
    termination: &Termination,
    on_state: impl FnMut(PrivacyState, BootId, u64) -> Result<()>,
) -> Result<WaitOutcome> {
    wait_loop_with_gate(worker, boot, termination, on_state, || Ok(true))
}

/// How long the wait may go without one fresh privacy observation.
const UNAVAILABLE_WAIT: Duration = Duration::from_secs(1);

#[cfg(test)]
thread_local! {
    /// A test's own wait for the same rule, on its thread only (#1806).
    static TEST_UNAVAILABLE_WAIT: std::cell::Cell<Option<Duration>> =
        const { std::cell::Cell::new(None) };
}

/// [`UNAVAILABLE_WAIT`], or in a test that set one, its own.
fn unavailable_wait() -> Duration {
    #[cfg(test)]
    if let Some(wait) = TEST_UNAVAILABLE_WAIT.with(std::cell::Cell::get) {
        return wait;
    }
    UNAVAILABLE_WAIT
}

fn wait_loop_with_gate(
    worker: &mut Worker,
    boot: BootId,
    termination: &Termination,
    mut on_state: impl FnMut(PrivacyState, BootId, u64) -> Result<()>,
    mut additional_gate: impl FnMut() -> Result<bool>,
) -> Result<WaitOutcome> {
    let mut controller = Controller::new(boot, MonoTime::from_micros(monotonic_us()));
    controller
        .set_microphone_permission(MonoTime::from_micros(monotonic_us()), Permission::Denied)?;
    controller.set_camera_permission(MonoTime::from_micros(monotonic_us()), Permission::Denied)?;
    let mut gate = PrivacyGate::default();
    let mut sequence = 0u64;
    let mut previous_state = None;
    let mut last_publish = None;
    // While nothing is open, an observation that arrived late (the worker's
    // 5 ms reads delayed past the 50 ms freshness by a busy boot: lamp-4ace's
    // power cut on 2026-10-15 ended the voice at 13.56 s this way) only keeps
    // the gate closed. The wait ends only when no fresh observation has come
    // for `UNAVAILABLE_WAIT`. Every clock here is CLOCK_MONOTONIC, so a wall
    // clock set by NTP after a power cut changes nothing.
    let mut fresh_since = Instant::now();
    loop {
        if termination.signal_name().is_some() {
            return Ok(WaitOutcome::Stopped);
        }
        // This callback may persist a local clear while no audio is open.
        // Read and validate physical observations afterward: a slow write must
        // not grant Allowed from the state observed before that write.
        let additionally_allowed = additional_gate()?;
        worker.check_running()?;
        let at = monotonic_us();
        if last_publish.is_none_or(|last| at.saturating_sub(last) >= 25_000) {
            worker.channels.control.send(Control::Authority {
                snapshot: controller.snapshot(MonoTime::from_micros(at))?,
            })?;
            last_publish = Some(at);
        }
        let mut empty = false;
        for _ in 0..16 {
            match worker.channels.control.receive::<WorkerEvent>()? {
                Some(WorkerEvent::Privacy {
                    receipt: _,
                    acquired_at_us,
                    muted,
                }) => {
                    sequence = sequence
                        .checked_add(1)
                        .ok_or_else(|| io::Error::other("privacy sequence overflow"))?;
                    gate.observe(
                        sequence,
                        MonoTime::from_micros(acquired_at_us),
                        MonoTime::from_micros(monotonic_us()),
                        muted,
                    );
                }
                Some(WorkerEvent::Fault { code }) => return Err(io::Error::other(code).into()),
                Some(_) => return Err(io::Error::other("unexpected privacy wait event").into()),
                None => {
                    empty = true;
                    break;
                }
            }
        }
        let at = monotonic_us();
        let state = gate.state(MonoTime::from_micros(at));
        if state != PrivacyState::Unavailable {
            fresh_since = Instant::now();
        } else if fresh_since.elapsed() >= unavailable_wait() {
            return Err(
                io::Error::other("physical privacy observation unavailable while waiting").into(),
            );
        }
        if previous_state != Some(state) {
            on_state(state, boot, at)?;
            previous_state = Some(state);
        }
        if additionally_allowed && empty && state == PrivacyState::Allowed {
            return Ok(WaitOutcome::Allowed);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::privacy::PRIVACY_FRESH_US;
    use crate::transport::WorkerChannels;
    use lamp_turn_owner::Permission;
    use std::{
        cell::Cell,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
    };

    #[derive(Clone, Copy)]
    pub(crate) enum Script {
        Unmute,
        Muted,
        Missing,
        Gap,
        Stale,
        /// One observation that arrives late, as on a busy boot, then fresh
        /// ones that unmute.
        LateThenUnmute,
        Fault,
        Invalid,
    }
    pub(crate) struct Peer {
        // Keep mock endpoints alive through the parent's final exit check.
        pub(crate) channels: WorkerChannels,
        pub(crate) received_stop: bool,
        pub(crate) denied_heartbeats: usize,
    }
    pub(crate) fn mock(
        directory: &Path,
        script: Script,
        muted_seen: Arc<AtomicBool>,
    ) -> (Worker, thread::JoinHandle<Peer>) {
        mock_with_slide(directory, script, muted_seen, None)
    }

    fn mock_with_slide(
        directory: &Path,
        script: Script,
        muted_seen: Arc<AtomicBool>,
        slide: Option<Arc<AtomicBool>>,
    ) -> (Worker, thread::JoinHandle<Peer>) {
        let (mut worker, mut peer, input) = Worker::cooperative_fixture(directory, "privacy");
        // A Stop ends the child at once, and its exit is waited for on the
        // stopping thread, not on this peer's, which a busy host may not run
        // within the 300 ms a stopped worker has (#1232): `Worker::shutdown`
        // runs this hook after its Stop, and so does a cleanup of its own
        // through `Worker::run_on_stop` (`service_privacy_controls`, #1249).
        // The peer still ends it on Stop for a caller that runs neither.
        let input = Arc::new(std::sync::Mutex::new(Some(input)));
        let pid = worker.fixture_pid();
        let ended = Arc::clone(&input);
        worker.on_stop(move || {
            drop(ended.lock().unwrap().take());
            crate::scenario_clock::wait_child_exited(pid, Instant::now() + Duration::from_secs(5))
        });
        let task = thread::spawn(move || {
            let mut ticks = 0usize;
            let mut heartbeats = 0;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                for _ in 0..16 {
                    match peer.control.receive::<Control>().unwrap() {
                        Some(Control::Stop) => {
                            drop(input.lock().unwrap().take());
                            return Peer {
                                channels: peer,
                                received_stop: true,
                                denied_heartbeats: heartbeats,
                            };
                        }
                        Some(Control::Authority { snapshot }) => {
                            assert!(snapshot.owner().is_none());
                            assert_eq!(snapshot.microphone_permission(), Permission::Denied);
                            assert_eq!(snapshot.camera_permission(), Permission::Denied);
                            heartbeats += 1;
                        }
                        Some(_) => panic!("non-privacy command during mute wait"),
                        None => break,
                    }
                }
                if heartbeats != 0 {
                    let at = monotonic_us();
                    let event = match script {
                        Script::Missing => None,
                        Script::Gap if ticks == 0 => Some(WorkerEvent::Privacy {
                            receipt: None,
                            acquired_at_us: at,
                            muted: true,
                        }),
                        Script::Stale if ticks == 0 => Some(WorkerEvent::Privacy {
                            receipt: None,
                            acquired_at_us: at - PRIVACY_FRESH_US,
                            muted: true,
                        }),
                        Script::LateThenUnmute => Some(WorkerEvent::Privacy {
                            receipt: None,
                            acquired_at_us: if ticks == 0 {
                                at - PRIVACY_FRESH_US
                            } else {
                                at
                            },
                            muted: ticks < 4,
                        }),
                        Script::Fault if ticks == 0 => Some(WorkerEvent::Fault {
                            code: "synthetic GPIO fault".into(),
                        }),
                        Script::Invalid if ticks == 0 => Some(WorkerEvent::Ready),
                        Script::Muted | Script::Unmute => {
                            let muted = slide.as_ref().map_or_else(
                                || matches!(script, Script::Muted) || ticks < 4,
                                |slide| slide.load(Ordering::SeqCst),
                            );
                            if muted {
                                muted_seen.store(true, Ordering::SeqCst);
                            }
                            Some(WorkerEvent::Privacy {
                                receipt: None,
                                acquired_at_us: at,
                                muted,
                            })
                        }
                        _ => None,
                    };
                    if let Some(event) = event {
                        peer.control.send(event).unwrap();
                    }
                    ticks += 1;
                }
                assert!(
                    Instant::now() < deadline,
                    "privacy mock never received Stop"
                );
                thread::sleep(Duration::from_millis(5));
            }
        });
        (worker, task)
    }

    /// Whether a missing executable starts a child here, as under user-mode
    /// emulation (`qemu-aarch64` 7.2): `spawn` succeeds and the child exits
    /// 127, so no wait can see `NotFound`. A real kernel never does, so CI
    /// never skips, and fails if it would (#1212).
    fn a_missing_executable_starts_here(executable: &Path) -> bool {
        use std::io::Write;
        let Ok(mut child) = std::process::Command::new(executable).spawn() else {
            return false;
        };
        let _ = child.wait();
        assert!(
            std::env::var_os("CI").is_none(),
            "a missing executable started a child on CI"
        );
        // Straight to stderr, beside the test's name: libtest captures
        // `eprintln!` and shows it only for a failing test (the reviewer on
        // #1211).
        let _ = writeln!(
            std::io::stderr(),
            "skipped the NotFound part: a missing executable starts a child here, \
             as under user-mode emulation"
        );
        true
    }

    #[test]
    fn software_gate_waits_before_opening_any_privacy_worker() {
        let directory = SessionDirectory::create().unwrap();
        let absent_worker = directory.path.join("absent-privacy-worker");
        if !a_missing_executable_starts_here(&absent_worker) {
            let ungated = wait(&absent_worker, &Termination::inactive(), |_, _, _| {
                panic!("the nonexistent worker cannot report a state")
            })
            .unwrap_err();
            assert_eq!(
                ungated.downcast_ref::<io::Error>().unwrap().kind(),
                io::ErrorKind::NotFound
            );
        }
        let calls = Cell::new(0);
        let error = wait_with_gate(
            // If the wait tries to spawn a child, it fails before the fourth
            // callback. No executable, GPIO, or audio is used by this test.
            &absent_worker,
            &Termination::inactive(),
            |_, _, _| panic!("no worker should exist behind a closed software gate"),
            || {
                calls.set(calls.get() + 1);
                if calls.get() == 4 {
                    Err(Box::new(SoftwareMute {
                        observed_at_us: 17,
                        asleep: false,
                    }))
                } else {
                    Ok(false)
                }
            },
        )
        .unwrap_err();
        assert_eq!(calls.get(), 4);
        assert_eq!(
            error.downcast_ref::<SoftwareMute>().unwrap().observed_at_us,
            17
        );
        assert!(error.downcast_ref::<PhysicalMute>().is_none());
    }

    #[test]
    fn physically_allowed_wait_stays_closed_until_software_gate_clears() {
        let directory = SessionDirectory::create().unwrap();
        let (worker, peer) = mock(
            &directory.path,
            Script::Unmute,
            Arc::new(AtomicBool::new(false)),
        );
        let physical_allowed = Cell::new(false);
        let held_open_ticks = Cell::new(0);
        let result = wait_worker_with_gate(
            worker,
            new_boot().unwrap(),
            &Termination::inactive(),
            |state, _, _| {
                physical_allowed.set(state == PrivacyState::Allowed);
                Ok(())
            },
            || {
                if physical_allowed.get() {
                    held_open_ticks.set(held_open_ticks.get() + 1);
                }
                Ok(held_open_ticks.get() >= 8)
            },
        )
        .unwrap();
        assert_eq!(result, WaitOutcome::Allowed);
        assert_eq!(held_open_ticks.get(), 8);
        let peer = peer.join().unwrap();
        assert!(peer.received_stop && peer.denied_heartbeats >= 2);
        drop(peer.channels);
        fs::remove_dir(&directory.path).unwrap();
    }

    #[test]
    fn software_clear_ticks_under_unchanged_closed_slide_and_still_needs_unmute() {
        let directory = SessionDirectory::create().unwrap();
        let slide = Arc::new(AtomicBool::new(true));
        let (worker, peer) = mock_with_slide(
            &directory.path,
            Script::Muted,
            Arc::new(AtomicBool::new(false)),
            Some(Arc::clone(&slide)),
        );
        let state = Cell::new(PrivacyState::Unavailable);
        let closed_ticks = Cell::new(0);
        let mut states = Vec::new();
        let result = wait_worker_with_gate(
            worker,
            new_boot().unwrap(),
            &Termination::inactive(),
            |observed, _, _| {
                state.set(observed);
                states.push(observed);
                Ok(())
            },
            || {
                if state.get() == PrivacyState::Muted {
                    closed_ticks.set(closed_ticks.get() + 1);
                }
                // Clear the software gate first, then leave the physical slide
                // closed for eight more ticks before opening it independently.
                if closed_ticks.get() >= 16 {
                    slide.store(false, Ordering::SeqCst);
                }
                Ok(closed_ticks.get() >= 8)
            },
        )
        .unwrap();
        assert_eq!(result, WaitOutcome::Allowed);
        assert!(closed_ticks.get() >= 16);
        assert_eq!(
            states
                .iter()
                .filter(|&&state| state == PrivacyState::Muted)
                .count(),
            1,
            "gate ticks must not depend on repeated state-change callbacks"
        );
        assert!(states.contains(&PrivacyState::Stabilizing));
        assert_eq!(states.last(), Some(&PrivacyState::Allowed));
        let peer = peer.join().unwrap();
        assert!(peer.received_stop && peer.denied_heartbeats >= 2);
        drop(peer.channels);
        fs::remove_dir(&directory.path).unwrap();
    }

    #[test]
    fn software_mute_requires_successful_cleanup_to_remain_restartable() {
        let clean = finish::<()>(
            Err(Box::new(SoftwareMute {
                observed_at_us: 29,
                asleep: false,
            })),
            Ok(()),
        )
        .unwrap_err();
        assert!(clean.downcast_ref::<SoftwareMute>().is_some());
        assert!(clean.downcast_ref::<PhysicalMute>().is_none());
        let failed = finish::<()>(
            Err(clean),
            Err(io::Error::other("synthetic worker remains open")),
        )
        .unwrap_err();
        assert!(failed.downcast_ref::<SoftwareMute>().is_none());
        assert!(failed.downcast_ref::<PhysicalMute>().is_none());
        assert!(
            failed
                .to_string()
                .contains("software mute observed at 29 us")
        );
        assert!(failed.to_string().contains("synthetic worker remains open"));
    }

    #[test]
    fn muted_gpio_only_startup_requires_stable_unmute_and_verified_stop() {
        let directory = SessionDirectory::create().unwrap();
        let seen = Arc::new(AtomicBool::new(false));
        let (worker, peer) = mock(&directory.path, Script::Unmute, seen.clone());
        let mut states = Vec::new();
        let result = wait_worker(
            worker,
            new_boot().unwrap(),
            &Termination::inactive(),
            |state, _, _| {
                states.push(state);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(result, WaitOutcome::Allowed);
        assert!(seen.load(Ordering::SeqCst));
        assert!(states.contains(&PrivacyState::Muted));
        assert!(states.contains(&PrivacyState::Stabilizing));
        assert_eq!(states.last(), Some(&PrivacyState::Allowed));
        let peer = peer.join().unwrap();
        assert!(peer.received_stop && peer.denied_heartbeats >= 2);
        drop(peer.channels);
        fs::remove_dir(&directory.path).unwrap();
    }

    /// lamp-4ace's power cut, 2026-10-15: one privacy observation read more
    /// than 50 ms late while every part was starting ended the voice. Now it
    /// only keeps the gate closed until fresh ones come; a stale one alone
    /// still ends the wait (the next test).
    #[test]
    fn one_late_privacy_observation_at_start_does_not_end_the_wait() {
        let directory = SessionDirectory::create().unwrap();
        let (worker, peer) = mock(
            &directory.path,
            Script::LateThenUnmute,
            Arc::new(AtomicBool::new(false)),
        );
        let result = wait_worker(
            worker,
            new_boot().unwrap(),
            &Termination::inactive(),
            |_, _, _| Ok(()),
        )
        .unwrap();
        assert_eq!(result, WaitOutcome::Allowed);
        let peer = peer.join().unwrap();
        assert!(peer.received_stop);
        drop(peer.channels);
        fs::remove_dir(&directory.path).unwrap();
    }

    /// Unknown, stale, invalid or failed GPIO never provides unmute
    /// authority: one test for each, so that each runs beside the others
    /// (#1806: the five one after another took 3.1 s on ARM64).
    fn never_provides_unmute_authority(script: Script) {
        // The same rule on a tenth of the wait: a missing, broken or stale
        // reading is given up on after it, as after the lamp's second.
        TEST_UNAVAILABLE_WAIT.with(|wait| wait.set(Some(UNAVAILABLE_WAIT / 10)));
        let directory = SessionDirectory::create().unwrap();
        let (worker, peer) = mock(&directory.path, script, Arc::new(AtomicBool::new(false)));
        let mut allowed = false;
        let error = wait_worker(
            worker,
            new_boot().unwrap(),
            &Termination::inactive(),
            |state, _, _| {
                allowed |= state == PrivacyState::Allowed;
                Ok(())
            },
        )
        .unwrap_err();
        assert!(error.downcast_ref::<PhysicalMute>().is_none());
        assert!(!allowed);
        assert!(peer.join().unwrap().received_stop);
    }

    #[test]
    fn missing_gpio_never_provides_unmute_authority() {
        never_provides_unmute_authority(Script::Missing);
    }

    #[test]
    fn a_gap_in_gpio_never_provides_unmute_authority() {
        never_provides_unmute_authority(Script::Gap);
    }

    #[test]
    fn stale_gpio_never_provides_unmute_authority() {
        never_provides_unmute_authority(Script::Stale);
    }

    #[test]
    fn failed_gpio_never_provides_unmute_authority() {
        never_provides_unmute_authority(Script::Fault);
    }

    #[test]
    fn invalid_gpio_never_provides_unmute_authority() {
        never_provides_unmute_authority(Script::Invalid);
    }

    #[test]
    fn unmute_cannot_hide_privacy_worker_cleanup_failure() {
        let directory = SessionDirectory::create().unwrap();
        let (mut worker, mut peer) = Worker::sleeping_fixture(&directory.path, "privacy");
        let gate = thread::spawn(move || {
            let mut reads = 0;
            loop {
                match peer.control.receive::<Control>().unwrap() {
                    Some(Control::Stop) => return peer,
                    Some(Control::Authority { .. }) => {}
                    None => {}
                    Some(_) => panic!("unexpected control"),
                }
                peer.control
                    .send(WorkerEvent::Privacy {
                        receipt: None,
                        acquired_at_us: monotonic_us(),
                        muted: false,
                    })
                    .unwrap();
                reads += 1;
                assert!(reads < 100);
                thread::sleep(Duration::from_millis(5));
            }
        });
        let result = wait_loop(
            &mut worker,
            new_boot().unwrap(),
            &Termination::inactive(),
            |_, _, _| Ok(()),
        );
        assert_eq!(result.as_ref().unwrap(), &WaitOutcome::Allowed);
        let result = finish(result, worker.shutdown());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("forced termination")
        );
        assert!(worker.try_exit().unwrap().is_some());
        drop(gate.join().unwrap());
    }
}
