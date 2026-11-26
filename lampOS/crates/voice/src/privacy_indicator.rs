//! Optional quiescent physical-mute ring worker. SPI never runs in the sampler
//! or the service wait loop. The supervisor must verify this writer's shutdown
//! receipt and process exit before starting another ring owner.
//!
//! The person's look (#8696, `docs/ring-choreography.md`, "The person's
//! look"): with `LAMP_PRIVACY_LIGHT=on` in the voice, the wait passes
//! [`LOOK_FLAG`] and the kept file's path ([`ring_arguments`]), and this
//! worker paints red, a dimmer red or nothing by it. A thread of its own
//! ([`LookWatch`]) looks at the file every [`LOOK_EVERY`] and reads it again
//! when it changed, so the 2 ms step does no file I/O. The look is read once
//! before the first paint, so a new showing begins in the kept look. It is
//! picked only after the ring's own guards, which it never reaches: nothing
//! on the mute's path reads it.
// Off Linux only the worker's flags are used (the muted wait builds them on
// every target); the writer and its state run on Linux and in tests.
#![cfg_attr(not(any(target_os = "linux", test)), allow(dead_code, unused_imports))]
use crate::{
    ring_wire::{RingBlankReason, RingFeedback},
    ring_worker::RingLink,
    wire::{Control, WorkerEvent},
};
use lamp_ipc::monotonic_us;
use lamp_ring::{
    BlankReason, BlankReceipt, FrameSink, GuardedRing, PrivacyLook, PrivacyObservation,
};
use lamp_turn_owner::{BootId, BoundaryGuard, Error as AuthorityError, MonoTime};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use std::time::Duration;

const CONTROL_BUDGET: usize = 16;
const INITIAL_AUTHORITY_US: u64 = 250_000;
#[cfg(target_os = "linux")]
const TICK: Duration = Duration::from_millis(2);

/// The worker's flag before the kept look's path (#8696).
pub const LOOK_FLAG: &str = "--privacy-light";

/// How often [`LookWatch`] looks at the kept file: a set with the slide
/// closed shows within this and one step, well inside the first builder's
/// about 1 s.
pub const LOOK_EVERY: Duration = Duration::from_millis(100);

/// The flags after `worker ring` for the muted indicator: `--privacy
/// --bus-root ROOT`, and the kept look's flag and path with
/// `LAMP_PRIVACY_LIGHT=on` in the voice. The worker's environment is
/// cleared, so the switch reaches it only as this flag.
pub fn ring_arguments(root: &str) -> Vec<String> {
    ring_arguments_with(root, lamp_msgs::privacy_light::on_in_environment())
}

fn ring_arguments_with(root: &str, look: bool) -> Vec<String> {
    let mut args = vec![
        "--privacy".to_owned(),
        "--bus-root".to_owned(),
        root.to_owned(),
    ];
    if look {
        args.push(LOOK_FLAG.to_owned());
        args.push(lamp_msgs::privacy_light::PATH.to_owned());
    }
    args
}

/// The ring's look for a kept one.
fn painted(look: lamp_msgs::privacy_light::Look) -> PrivacyLook {
    match look {
        lamp_msgs::privacy_light::Look::Red => PrivacyLook::Red,
        lamp_msgs::privacy_light::Look::Dim => PrivacyLook::Dim,
        lamp_msgs::privacy_light::Look::Off => PrivacyLook::Off,
    }
}

fn code(look: PrivacyLook) -> u8 {
    match look {
        PrivacyLook::Red => 0,
        PrivacyLook::Dim => 1,
        PrivacyLook::Off => 2,
    }
}

fn decoded(code: u8) -> PrivacyLook {
    match code {
        1 => PrivacyLook::Dim,
        2 => PrivacyLook::Off,
        _ => PrivacyLook::Red,
    }
}

/// What tells a changed file from the one last read: its inode (a whole
/// write is a rename), size and modification time. None while missing.
type Stamp = Option<(u64, u64, Option<std::time::SystemTime>)>;

fn stamp(path: &Path) -> Stamp {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(path)
        .ok()
        .map(|meta| (meta.ino(), meta.len(), meta.modified().ok()))
}

/// The kept look, followed on a thread of its own: read once at
/// [`LookWatch::start`], then looked at every `every` and read again when it
/// changed. Red whenever it can't be read ([`lamp_msgs::privacy_light::read`]).
pub(crate) struct LookWatch {
    current: Arc<AtomicU8>,
    going: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl LookWatch {
    pub(crate) fn start(path: PathBuf, every: Duration) -> io::Result<Self> {
        let mut seen = stamp(&path);
        let current = Arc::new(AtomicU8::new(code(painted(
            lamp_msgs::privacy_light::read(&path),
        ))));
        let going = Arc::new(AtomicBool::new(true));
        let thread = {
            let (current, going) = (current.clone(), going.clone());
            std::thread::Builder::new()
                .name("privacy-look".into())
                .spawn(move || {
                    while going.load(Ordering::Acquire) {
                        // Woken at once by the drop, so a stop never waits
                        // out a look.
                        std::thread::park_timeout(every);
                        if !going.load(Ordering::Acquire) {
                            return;
                        }
                        let now = stamp(&path);
                        if now != seen {
                            seen = now;
                            let look = painted(lamp_msgs::privacy_light::read(&path));
                            current.store(code(look), Ordering::Release);
                        }
                    }
                })?
        };
        Ok(Self {
            current,
            going,
            thread: Some(thread),
        })
    }

    /// The look last read.
    pub(crate) fn look(&self) -> PrivacyLook {
        decoded(self.current.load(Ordering::Acquire))
    }
}

impl Drop for LookWatch {
    fn drop(&mut self) {
        self.going.store(false, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

fn look_name(look: PrivacyLook) -> &'static str {
    match look {
        PrivacyLook::Red => "red",
        PrivacyLook::Dim => "dim",
        PrivacyLook::Off => "off",
    }
}

/// Only the supervisor's explicit `ring --privacy --bus-root PATH` dispatch
/// calls this. Workers do not read the feature environment themselves.
#[cfg(target_os = "linux")]
pub fn run(
    mut channels: crate::transport::WorkerChannels,
    controller_boot: BootId,
    root: &std::path::Path,
    look: Option<&Path>,
) -> io::Result<()> {
    // The bus is read by the watch's own thread, never between this
    // writer's Stop and expiry checks (#795 review).
    let watch = match crate::privacy_bus::LeaseWatch::open(
        root,
        crate::privacy_bus::INDICATOR_SUBSCRIBER,
    ) {
        Ok(watch) => watch,
        Err(error) => return startup_failure(&mut channels, error),
    };
    // Read before the ring opens, so the first frame is in the kept look.
    let looks = match look.map(|path| LookWatch::start(path.to_path_buf(), LOOK_EVERY)) {
        None => None,
        Some(Ok(watch)) => Some(watch),
        Some(Err(error)) => return startup_failure(&mut channels, error),
    };
    // The waking face lets go at once for it, with no fade (#1681).
    let (ring, _) = match lamp_ring::linux::open_privacy(controller_boot) {
        Ok(opened) => opened,
        Err(error) => return startup_failure(&mut channels, error),
    };
    run_guarded(channels, controller_boot, ring, looks, move || {
        watch
            .latest()?
            .map(|lease| {
                Ok(PrivacyObservation {
                    controller_boot: crate::process::parse_boot(&lease.controller_boot)?,
                    microphone_generation: lease.microphone_generation,
                    observed_at: MonoTime::from_micros(lease.observed_us),
                    expires_at: MonoTime::from_micros(lease.expires_at_us),
                    permitted: lease.permitted,
                    physical_muted: lease.physical_muted,
                })
            })
            .transpose()
    })
}

#[cfg(target_os = "linux")]
fn startup_failure(link: &mut impl RingLink, error: io::Error) -> io::Result<()> {
    let _ = link.publish(WorkerEvent::Fault {
        code: bounded(&format!("privacy ring startup: {error}")),
    });
    Err(error)
}

#[cfg(target_os = "linux")]
fn run_guarded(
    mut link: impl RingLink,
    controller_boot: BootId,
    mut ring: GuardedRing<impl FrameSink>,
    looks: Option<LookWatch>,
    mut observation: impl FnMut() -> io::Result<Option<PrivacyObservation>>,
) -> io::Result<()> {
    let result = (|| {
        // The constructor has already completed the startup black write.
        link.publish(WorkerEvent::Ready)?;
        let mut state = State::new(controller_boot, monotonic_us());
        state.looks = looks;
        loop {
            if state.step(&mut link, &mut ring, &mut observation)? {
                return Ok(());
            }
            std::thread::sleep(TICK);
        }
    })();
    if let Err(original) = result {
        return failed(&mut link, &mut ring, original);
    }
    Ok(())
}

fn failed(
    link: &mut impl RingLink,
    ring: &mut GuardedRing<impl FrameSink>,
    original: io::Error,
) -> io::Result<()> {
    let mut detail = format!("privacy ring: {original}");
    // An SPI fault already attempted its one cleanup frame. Other faults blank
    // once before optional feedback; there is no retry or replacement writer.
    if !ring.is_faulted() && !ring.is_stopped() {
        match ring.control_lost() {
            Ok(receipt) => {
                if let Err(error) = blanked(link, receipt) {
                    detail.push_str(&format!("; cleanup receipt: {error}"));
                }
            }
            Err(error) => detail.push_str(&format!("; cleanup: {error}")),
        }
    }
    let code = bounded(&detail);
    if let Err(error) = link.publish(WorkerEvent::Fault { code: code.clone() }) {
        return Err(io::Error::other(format!("{code}; fault feedback: {error}")));
    }
    Err(io::Error::other(code))
}

fn bounded(detail: &str) -> String {
    detail.chars().take(512).collect()
}

fn blanked(link: &mut impl RingLink, receipt: BlankReceipt) -> io::Result<()> {
    let reason = match receipt.reason {
        BlankReason::Shutdown => RingBlankReason::Shutdown,
        BlankReason::ControlLost => RingBlankReason::ControlLost,
        BlankReason::AuthorityLost(reason) => RingBlankReason::AuthorityLost {
            reason: reason.into(),
        },
        BlankReason::Startup | BlankReason::ExplicitOff => {
            return Err(io::Error::other("unexpected privacy ring blank reason"));
        }
    };
    link.publish(WorkerEvent::Ring {
        report: RingFeedback::Blanked {
            reason,
            write_started_at_us: receipt.write.started_at().as_micros(),
            write_finished_at_us: receipt.write.finished_at().as_micros(),
        },
    })
}

struct State {
    authority: BoundaryGuard,
    started_at: u64,
    previous_at: u64,
    /// Whether the indication just held begins a showing, for `lamp/did`.
    shown: crate::did_ring::Episode,
    /// The showings told as `lamp/did`, in any look.
    notices: u64,
    /// The person's kept look, with [`LOOK_FLAG`]; red without it.
    looks: Option<LookWatch>,
    /// The look last said in the journal while shown.
    said: Option<PrivacyLook>,
}

enum Drained {
    Empty,
    Budget,
    Stopped,
}

impl State {
    fn new(controller_boot: BootId, at_us: u64) -> Self {
        Self {
            authority: BoundaryGuard::new(controller_boot, MonoTime::from_micros(at_us)),
            started_at: at_us,
            previous_at: at_us,
            shown: crate::did_ring::Episode::default(),
            notices: 0,
            looks: None,
            said: None,
        }
    }

    /// The look to paint now: the watch's last read, red without one.
    fn look(&self) -> PrivacyLook {
        self.looks
            .as_ref()
            .map_or(PrivacyLook::Red, LookWatch::look)
    }

    fn heartbeat(&mut self, at_us: u64) -> io::Result<()> {
        if at_us < self.previous_at {
            return Err(io::Error::other("privacy ring clock regressed"));
        }
        self.previous_at = at_us;
        let expired = self.authority.state().map_or_else(
            || at_us.saturating_sub(self.started_at) >= INITIAL_AUTHORITY_US,
            |snapshot| at_us >= snapshot.expires_at().as_micros(),
        );
        if expired {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "privacy ring controller heartbeat expired",
            ));
        }
        Ok(())
    }

    fn controls(
        &mut self,
        link: &mut impl RingLink,
        ring: &mut GuardedRing<impl FrameSink>,
        budget: &mut usize,
    ) -> io::Result<Drained> {
        while *budget > 0 {
            let Some(control) = link.receive_control()? else {
                return Ok(Drained::Empty);
            };
            *budget -= 1;
            match control {
                Control::Stop => {
                    if let Some(receipt) = ring.shutdown().map_err(io::Error::other)? {
                        blanked(link, receipt)?;
                    }
                    return Ok(Drained::Stopped);
                }
                Control::Authority { snapshot } => {
                    if self.authority.state().is_some_and(|current| {
                        current.revision() == snapshot.revision() && current != snapshot
                    }) {
                        return Err(io::Error::other("conflicting privacy ring authority"));
                    }
                    match self
                        .authority
                        .install(MonoTime::from_micros(monotonic_us()), snapshot)
                    {
                        Ok(()) => {}
                        Err(AuthorityError::StaleSnapshot) => continue,
                        Err(AuthorityError::ExpiredState)
                            if self.authority.state().is_some_and(|current| {
                                snapshot.revision() <= current.revision()
                            }) =>
                        {
                            continue;
                        }
                        Err(error) => return Err(io::Error::other(error)),
                    }
                    if let Some(receipt) =
                        ring.install_authority(snapshot).map_err(io::Error::other)?
                    {
                        blanked(link, receipt)?;
                    }
                }
                Control::StartCapture
                | Control::ConnectReference { .. }
                | Control::ProviderOutputCapacity { .. }
                | Control::NameGrant { .. } => {
                    return Err(io::Error::other("invalid privacy ring control command"));
                }
            }
        }
        Ok(Drained::Budget)
    }

    /// One bounded tick; true means cooperative Stop has completed. No frame
    /// data is valid for this worker. Control is drained again after bus I/O so
    /// a Stop received during that read wins before the next red write.
    fn step(
        &mut self,
        link: &mut impl RingLink,
        ring: &mut GuardedRing<impl FrameSink>,
        observation: &mut impl FnMut() -> io::Result<Option<PrivacyObservation>>,
    ) -> io::Result<bool> {
        let mut budget = CONTROL_BUDGET;
        match self.controls(link, ring, &mut budget)? {
            Drained::Stopped => return Ok(true),
            Drained::Budget => {
                self.heartbeat(monotonic_us())?;
                if let Some(receipt) = ring.tick().map_err(io::Error::other)? {
                    blanked(link, receipt)?;
                }
                return Ok(false);
            }
            Drained::Empty => {}
        }
        self.heartbeat(monotonic_us())?;
        if let Some(receipt) = ring.tick().map_err(io::Error::other)? {
            blanked(link, receipt)?;
        }
        if link.receive_frame()?.is_some() {
            return Err(io::Error::other(
                "frame data is invalid for the privacy ring",
            ));
        }
        // A source failure racing cooperative shutdown must still allow Stop
        // to blank and confirm this writer's exit. No observation is applied
        // before the second priority drain, including when the read fails.
        let observed = observation();
        match self.controls(link, ring, &mut budget)? {
            Drained::Stopped => return Ok(true),
            Drained::Budget => {
                observed?;
                self.heartbeat(monotonic_us())?;
                if let Some(receipt) = ring.tick().map_err(io::Error::other)? {
                    blanked(link, receipt)?;
                }
                return Ok(false);
            }
            Drained::Empty => {}
        }
        let observed = observed?;
        self.heartbeat(monotonic_us())?;
        // Picked only now, after the guards' evidence was taken: the look
        // changes the frame, never whether one may be painted.
        let look = self.look();
        let written = ring
            .show_privacy_look(observed, look)
            .map_err(io::Error::other)?;
        // The look is told only with the switch on: off, the journal is as
        // before (the Reviewer-lead's P3 on #8733).
        if !ring.privacy_shown() {
            self.said = None;
        } else if let Some(receipt) = written
            && self.looks.is_some()
            && self.said != Some(look)
        {
            eprintln!(
                "lamp-voice: privacy light: {} (painted at_us {})",
                look_name(look),
                receipt.started_at().as_micros()
            );
            self.said = Some(look);
        }
        // Held, in any look, dark too (#8696): told once as the showing
        // begins, that the lamp was private and nothing from while it was
        // (#7757). The person's app shows the mute whatever the ring shows.
        if ring.privacy_shown() && self.shown.seen(monotonic_us()) {
            self.notices += 1;
            crate::did_ring::told(crate::did_ring::Face::PrivacyMuted);
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ring_wire::RingFrame;
    use lamp_ring::EncodedFrame;
    use lamp_turn_owner::{Controller, Permission};
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};

    #[derive(Default)]
    struct Link {
        controls: VecDeque<Control>,
        published: Vec<WorkerEvent>,
        after_poll: Option<Rc<RefCell<VecDeque<Control>>>>,
    }
    impl RingLink for Link {
        fn receive_control(&mut self) -> io::Result<Option<Control>> {
            Ok(self.controls.pop_front().or_else(|| {
                self.after_poll
                    .as_ref()
                    .and_then(|queue| queue.borrow_mut().pop_front())
            }))
        }
        fn receive_frame(&mut self) -> io::Result<Option<RingFrame>> {
            Ok(None)
        }
        fn publish(&mut self, event: WorkerEvent) -> io::Result<()> {
            self.published.push(event);
            Ok(())
        }
    }
    #[derive(Clone, Default)]
    struct Sink(Rc<RefCell<Vec<EncodedFrame>>>);
    impl FrameSink for Sink {
        fn write_frame(&mut self, frame: &EncodedFrame) -> io::Result<()> {
            self.0.borrow_mut().push(frame.clone());
            Ok(())
        }
    }
    fn setup() -> (State, GuardedRing<Sink>, Sink, Link, PrivacyObservation) {
        let boot = BootId::new([7; 16]).unwrap();
        let at = monotonic_us();
        let mut controller = Controller::new(boot, MonoTime::from_micros(at));
        controller
            .set_microphone_permission(MonoTime::from_micros(at), Permission::Denied)
            .unwrap();
        controller
            .set_camera_permission(MonoTime::from_micros(at), Permission::Denied)
            .unwrap();
        let snapshot = controller.snapshot(MonoTime::from_micros(at)).unwrap();
        let sink = Sink::default();
        let ring = GuardedRing::new(sink.clone(), boot).unwrap();
        let link = Link {
            controls: VecDeque::from([Control::Authority { snapshot }]),
            ..Link::default()
        };
        let observation = PrivacyObservation {
            controller_boot: boot,
            microphone_generation: snapshot.microphone_generation(),
            observed_at: MonoTime::from_micros(at),
            expires_at: MonoTime::from_micros(at + 50_000),
            permitted: false,
            physical_muted: Some(true),
        };
        (State::new(boot, at), ring, sink, link, observation)
    }

    /// #795 review: the writer's observation is the lease watch's, whose
    /// own thread reads the bus. A Stop that arrives while refused messages
    /// flood the lease's topic is served at once, before any red write.
    #[test]
    fn stop_during_refused_input_on_the_lease_topic_is_served_at_once() {
        use crate::privacy_bus::{INDICATOR_SUBSCRIBER, LeaseReader, LeaseWatch};
        use lamp_msgs::{ClockStamp, MicrophonePrivacy};
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let root = std::path::PathBuf::from(format!("/tmp/vpi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        let watch = LeaseWatch::start(
            LeaseReader::with_clock(&root, INDICATOR_SUBSCRIBER, "host_fixture".into()).unwrap(),
        )
        .unwrap();
        let at = monotonic_us();
        let lease = MicrophonePrivacy {
            stamp: ClockStamp {
                clock_id: "host_fixture".into(),
                at_us: at,
                sequence: 1,
            },
            session_id: "f".repeat(16),
            controller_boot: "1".repeat(32),
            microphone_generation: 1,
            observed_us: at,
            expires_at_us: at + crate::privacy::PRIVACY_FRESH_US,
            permitted: false,
            physical_muted: Some(true),
            no_slide: false,
        };
        let going = Arc::new(AtomicBool::new(true));
        let flood = crate::privacy_bus::tests::refused_flood(&root, going.clone(), &lease);
        std::thread::sleep(std::time::Duration::from_millis(20));
        let (mut state, mut ring, sink, mut link, observed) = setup();
        let queued = Rc::new(RefCell::new(VecDeque::new()));
        link.after_poll = Some(queued.clone());
        let asked = std::time::Instant::now();
        let stopped = state.step(&mut link, &mut ring, &mut || {
            queued.borrow_mut().push_back(Control::Stop);
            Ok(watch.latest()?.map(|_| observed))
        });
        let took = asked.elapsed();
        going.store(false, Ordering::Release);
        flood.join().unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert!(stopped.unwrap());
        assert!(took < std::time::Duration::from_millis(10), "{took:?}");
        assert!(ring.is_stopped());
        assert!(
            sink.0
                .borrow()
                .iter()
                .all(|frame| *frame == EncodedFrame::black())
        );
    }

    #[test]
    fn stop_queued_during_a_bus_poll_wins_before_any_red_write() {
        let (mut state, mut ring, sink, mut link, observed) = setup();
        let queued = Rc::new(RefCell::new(VecDeque::new()));
        link.after_poll = Some(queued.clone());
        assert!(
            state
                .step(&mut link, &mut ring, &mut || {
                    queued.borrow_mut().push_back(Control::Stop);
                    Ok(Some(observed))
                })
                .unwrap()
        );
        assert!(
            sink.0
                .borrow()
                .iter()
                .all(|frame| *frame == EncodedFrame::black())
        );
        assert!(ring.is_stopped());
        assert!(link.published.iter().any(|event| matches!(
            event,
            WorkerEvent::Ring {
                report: RingFeedback::Blanked {
                    reason: RingBlankReason::Shutdown,
                    ..
                }
            }
        )));
    }

    #[test]
    fn stop_queued_during_a_failed_bus_poll_still_confirms_shutdown() {
        let (mut state, mut ring, sink, mut link, observed) = setup();
        state
            .step(&mut link, &mut ring, &mut || Ok(Some(observed)))
            .unwrap();
        assert_ne!(sink.0.borrow().last(), Some(&EncodedFrame::black()));
        let queued = Rc::new(RefCell::new(VecDeque::new()));
        link.after_poll = Some(queued.clone());
        assert!(
            state
                .step(&mut link, &mut ring, &mut || {
                    queued.borrow_mut().push_back(Control::Stop);
                    Err(io::Error::other("publisher stopped during poll"))
                })
                .unwrap()
        );
        assert!(ring.is_stopped());
        assert_eq!(sink.0.borrow().last(), Some(&EncodedFrame::black()));
        assert_eq!(
            link.published
                .iter()
                .filter(|event| matches!(
                    event,
                    WorkerEvent::Ring {
                        report: RingFeedback::Blanked {
                            reason: RingBlankReason::Shutdown,
                            ..
                        }
                    }
                ))
                .count(),
            1
        );
        assert!(
            !link
                .published
                .iter()
                .any(|event| matches!(event, WorkerEvent::Fault { .. }))
        );
    }

    #[test]
    fn reader_error_blanks_a_displayed_mute_and_never_retries() {
        let (mut state, mut ring, sink, mut link, observed) = setup();
        assert!(
            !state
                .step(&mut link, &mut ring, &mut || Ok(Some(observed)))
                .unwrap()
        );
        assert_ne!(sink.0.borrow().last(), Some(&EncodedFrame::black()));
        let error = state
            .step(&mut link, &mut ring, &mut || {
                Err(io::Error::other("lost privacy source"))
            })
            .unwrap_err();
        assert!(failed(&mut link, &mut ring, error).is_err());
        assert_eq!(sink.0.borrow().last(), Some(&EncodedFrame::black()));
        assert!(ring.is_faulted());
        assert_eq!(sink.0.borrow().len(), 3);
    }

    #[test]
    fn a_full_control_tick_does_not_read_the_bus_or_paint() {
        let (mut state, mut ring, sink, mut link, _) = setup();
        let snapshot = match link.controls.front().unwrap() {
            Control::Authority { snapshot } => *snapshot,
            _ => unreachable!(),
        };
        link.controls = (0..CONTROL_BUDGET)
            .map(|_| Control::Authority { snapshot })
            .collect();
        assert!(
            !state
                .step(&mut link, &mut ring, &mut || panic!(
                    "bus read after exhausted priority budget"
                ))
                .unwrap()
        );
        assert_eq!(sink.0.borrow().len(), 1);
    }

    #[test]
    fn bus_observations_cannot_extend_missing_or_expired_controller_authority() {
        let boot = BootId::new([7; 16]).unwrap();
        let mut missing = State::new(boot, 10);
        assert!(missing.heartbeat(10 + INITIAL_AUTHORITY_US - 1).is_ok());
        assert_eq!(
            missing
                .heartbeat(10 + INITIAL_AUTHORITY_US)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );

        let (mut state, mut ring, sink, mut link, observed) = setup();
        state
            .step(&mut link, &mut ring, &mut || Ok(Some(observed)))
            .unwrap();
        let expires = state.authority.state().unwrap().expires_at().as_micros();
        let error = state.heartbeat(expires).unwrap_err();
        assert!(failed(&mut link, &mut ring, error).is_err());
        assert_eq!(sink.0.borrow().last(), Some(&EncodedFrame::black()));
        assert!(ring.show_privacy_muted(Some(observed)).is_err());
    }

    #[test]
    fn a_wrong_controller_incarnation_blanks_the_existing_indication() {
        let (mut state, mut ring, sink, mut link, observed) = setup();
        state
            .step(&mut link, &mut ring, &mut || Ok(Some(observed)))
            .unwrap();
        let at = MonoTime::from_micros(monotonic_us());
        let mut stranger = Controller::new(BootId::new([8; 16]).unwrap(), at);
        link.controls.push_back(Control::Authority {
            snapshot: stranger.snapshot(at).unwrap(),
        });
        let error = state
            .step(&mut link, &mut ring, &mut || {
                panic!("source read after wrong controller")
            })
            .unwrap_err();
        assert!(failed(&mut link, &mut ring, error).is_err());
        assert_eq!(sink.0.borrow().last(), Some(&EncodedFrame::black()));
        assert!(ring.is_faulted());
    }

    // The person's look (#8696).

    /// A muted wait's controller, kept so each step can renew it.
    fn live() -> (State, GuardedRing<Sink>, Sink, Link, Controller) {
        let boot = BootId::new([7; 16]).unwrap();
        let at = MonoTime::from_micros(monotonic_us());
        let mut controller = Controller::new(boot, at);
        controller
            .set_microphone_permission(at, Permission::Denied)
            .unwrap();
        controller
            .set_camera_permission(at, Permission::Denied)
            .unwrap();
        let sink = Sink::default();
        let ring = GuardedRing::new(sink.clone(), boot).unwrap();
        (
            State::new(boot, at.as_micros()),
            ring,
            sink,
            Link::default(),
            controller,
        )
    }

    /// One step with a fresh authority and a fresh physical LOW (or none).
    fn fresh_step(
        state: &mut State,
        ring: &mut GuardedRing<Sink>,
        link: &mut Link,
        controller: &mut Controller,
        low: bool,
    ) -> bool {
        let at = MonoTime::from_micros(monotonic_us());
        let snapshot = controller.snapshot(at).unwrap();
        link.controls.push_back(Control::Authority { snapshot });
        let observation = PrivacyObservation {
            controller_boot: snapshot.boot(),
            microphone_generation: snapshot.microphone_generation(),
            observed_at: at,
            expires_at: MonoTime::from_micros(at.as_micros() + 50_000),
            permitted: false,
            physical_muted: Some(true),
        };
        state
            .step(link, ring, &mut || Ok(low.then_some(observation)))
            .unwrap()
    }

    fn kept(name: &str, look: lamp_msgs::privacy_light::Look) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("vpl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("privacy.json");
        keep(&path, look);
        path
    }

    /// Written whole, as the change helper writes it: beside, then renamed.
    fn keep(path: &Path, look: lamp_msgs::privacy_light::Look) {
        let next = path.with_extension("next");
        std::fs::write(&next, lamp_msgs::privacy_light::bytes(look)).unwrap();
        std::fs::rename(&next, path).unwrap();
    }

    #[test]
    fn the_indicator_follows_a_look_written_while_it_shows() {
        use lamp_msgs::privacy_light::Look;
        let path = kept("follows", Look::Red);
        let (mut state, mut ring, sink, mut link, mut controller) = live();
        state.looks = Some(LookWatch::start(path.clone(), LOOK_EVERY).unwrap());
        for _ in 0..10 {
            fresh_step(&mut state, &mut ring, &mut link, &mut controller, true);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(sink.0.borrow().last().unwrap().peak() >= 2, "red shows");
        for (look, painted) in [
            (Look::Dim, PrivacyLook::Dim),
            (Look::Off, PrivacyLook::Off),
            (Look::Red, PrivacyLook::Red),
        ] {
            keep(&path, look);
            let asked = std::time::Instant::now();
            while state.look() != painted {
                assert!(
                    asked.elapsed() < LOOK_EVERY * 2,
                    "{look:?} not followed within {:?}",
                    LOOK_EVERY * 2
                );
                fresh_step(&mut state, &mut ring, &mut link, &mut controller, true);
                std::thread::sleep(Duration::from_millis(2));
            }
            let before = sink.0.borrow().len();
            fresh_step(&mut state, &mut ring, &mut link, &mut controller, true);
            let frames = sink.0.borrow();
            // A changed look is painted on the very next step.
            assert_eq!(frames.len(), before + 1, "{look:?}");
            let last = frames.last().unwrap();
            match painted {
                PrivacyLook::Dim => assert!((1..=4).contains(&last.peak())),
                PrivacyLook::Off => assert_eq!(*last, EncodedFrame::black()),
                PrivacyLook::Red => assert!((2..=10).contains(&last.peak())),
            }
            drop(frames);
            assert!(ring.privacy_shown());
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn the_indicator_starts_in_the_kept_look_with_no_red_frame_first() {
        use lamp_msgs::privacy_light::Look;
        for (look, most) in [(Look::Off, 0), (Look::Dim, 4)] {
            let path = kept("starts", look);
            let (mut state, mut ring, sink, mut link, mut controller) = live();
            // Watched slowly, so only the read at start can give the look.
            state.looks = Some(LookWatch::start(path.clone(), Duration::from_secs(60)).unwrap());
            for _ in 0..20 {
                fresh_step(&mut state, &mut ring, &mut link, &mut controller, true);
                std::thread::sleep(Duration::from_millis(2));
            }
            let frames = sink.0.borrow();
            assert!(frames.len() >= 2, "{look:?}");
            assert!(
                frames.iter().all(|frame| frame.peak() <= most),
                "{look:?}: a frame above {most}"
            );
            drop(frames);
            assert!(ring.privacy_shown());
            // The mute notice is told in every look, dark too.
            assert_eq!(state.notices, 1, "{look:?}");
            let _ = std::fs::remove_dir_all(path.parent().unwrap());
        }
    }

    #[test]
    fn the_indicator_without_the_flag_paints_red() {
        assert_eq!(
            ring_arguments_with("/run/lampos-bus", false),
            ["--privacy", "--bus-root", "/run/lampos-bus"]
        );
        assert_eq!(
            ring_arguments_with("/run/lampos-bus", true),
            [
                "--privacy",
                "--bus-root",
                "/run/lampos-bus",
                LOOK_FLAG,
                lamp_msgs::privacy_light::PATH
            ]
        );
        // A kept off is never read without the flag.
        let path = kept("noflag", lamp_msgs::privacy_light::Look::Off);
        let (mut state, mut ring, sink, mut link, mut controller) = live();
        fresh_step(&mut state, &mut ring, &mut link, &mut controller, true);
        assert!(sink.0.borrow().last().unwrap().peak() >= 2);
        assert_eq!(state.notices, 1);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// Off changes only the light (#8696): the worker keeps running dark and
    /// tells the wait exactly what it tells it in red, a lost LOW blanking
    /// and a Stop's shutdown receipt among it, so the one-writer handoff and
    /// everything the mute decides are the same in all three looks.
    #[test]
    fn the_look_never_changes_what_the_worker_tells_the_wait() {
        use lamp_msgs::privacy_light::Look;
        let told = |look: Look| {
            let path = kept("tells", look);
            let (mut state, mut ring, _sink, mut link, mut controller) = live();
            state.looks = Some(LookWatch::start(path.clone(), LOOK_EVERY).unwrap());
            let mut stopped = Vec::new();
            for low in [true, true, true, false, false, true, true] {
                stopped.push(fresh_step(
                    &mut state,
                    &mut ring,
                    &mut link,
                    &mut controller,
                    low,
                ));
                std::thread::sleep(Duration::from_millis(3));
            }
            link.controls.push_back(Control::Stop);
            stopped.push(state.step(&mut link, &mut ring, &mut || Ok(None)).unwrap());
            let _ = std::fs::remove_dir_all(path.parent().unwrap());
            let events: Vec<String> = link
                .published
                .iter()
                .map(|event| match event {
                    WorkerEvent::Ring {
                        report: RingFeedback::Blanked { reason, .. },
                    } => format!("blanked {reason:?}"),
                    other => format!("{other:?}"),
                })
                .collect();
            (stopped, events, ring.is_stopped(), state.notices)
        };
        let red = told(Look::Red);
        assert_eq!(red.0.last(), Some(&true));
        assert!(red.2);
        assert!(red.1.iter().any(|event| event.contains("Shutdown")));
        assert_eq!(told(Look::Dim), red);
        assert_eq!(told(Look::Off), red);
    }

    /// The first builder's binding rule (#8696): nothing on the mute's path
    /// reads the person's look. The look lives only in the ring's frames,
    /// this worker, the kept file's reader, root's change helper and the
    /// app's two kinds.
    #[test]
    fn nothing_on_the_mute_path_reads_the_privacy_look() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut files = Vec::new();
        for name in [
            "privacy.rs",
            "privacy_bus.rs",
            "physical_privacy.rs",
            "service_privacy.rs",
            "service_privacy_controls.rs",
            "hand_controls.rs",
            "coordinator.rs",
            "slide_wake.rs",
        ] {
            let path = crates.join("voice/src").join(name);
            assert!(
                path.is_file(),
                "{} is gone: update this scan",
                path.display()
            );
            files.push(path);
        }
        for folder in ["controls/src", "ears/src", "turn-owner/src"] {
            let mut left = vec![crates.join(folder)];
            while let Some(folder) = left.pop() {
                for entry in std::fs::read_dir(&folder).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_dir() {
                        left.push(path);
                    } else if path.extension().is_some_and(|kind| kind == "rs") {
                        files.push(path);
                    }
                }
            }
        }
        assert!(files.len() > 12, "{files:?}");
        for path in files {
            let text = std::fs::read_to_string(&path).unwrap();
            for name in [
                "privacy_light",
                "PrivacyLook",
                "show_privacy_look",
                "LAMP_PRIVACY_LIGHT",
                "privacy.json",
                "LookWatch",
                "LOOK_FLAG",
            ] {
                assert!(
                    !text.contains(name),
                    "{} names {name}: the mute's path must not read the look",
                    path.display()
                );
            }
        }
    }
}
