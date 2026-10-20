//! Optional quiescent physical-mute ring worker. SPI never runs in the sampler
//! or the service wait loop. The supervisor must verify this writer's shutdown
//! receipt and process exit before starting another ring owner.
use crate::{
    ring_wire::{RingBlankReason, RingFeedback},
    ring_worker::RingLink,
    wire::{Control, WorkerEvent},
};
use lamp_ipc::monotonic_us;
use lamp_ring::{BlankReason, BlankReceipt, FrameSink, GuardedRing, PrivacyObservation};
use lamp_turn_owner::{BootId, BoundaryGuard, Error as AuthorityError, MonoTime};
use std::io;
#[cfg(target_os = "linux")]
use std::time::Duration;

const CONTROL_BUDGET: usize = 16;
const INITIAL_AUTHORITY_US: u64 = 250_000;
#[cfg(target_os = "linux")]
const TICK: Duration = Duration::from_millis(2);

/// Only the supervisor's explicit `ring --privacy --bus-root PATH` dispatch
/// calls this. Workers do not read the feature environment themselves.
#[cfg(target_os = "linux")]
pub fn run(
    mut channels: crate::transport::WorkerChannels,
    controller_boot: BootId,
    root: &std::path::Path,
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
    // The waking face lets go at once for it, with no fade (#1681).
    let (ring, _) = match lamp_ring::linux::open_privacy(controller_boot) {
        Ok(opened) => opened,
        Err(error) => return startup_failure(&mut channels, error),
    };
    run_guarded(channels, controller_boot, ring, move || {
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
    mut observation: impl FnMut() -> io::Result<Option<PrivacyObservation>>,
) -> io::Result<()> {
    let result = (|| {
        // The constructor has already completed the startup black write.
        link.publish(WorkerEvent::Ready)?;
        let mut state = State::new(controller_boot, monotonic_us());
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
        }
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
                | Control::ProviderOutputCapacity { .. } => {
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
        ring.show_privacy_muted(observed)
            .map_err(io::Error::other)?;
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
}
