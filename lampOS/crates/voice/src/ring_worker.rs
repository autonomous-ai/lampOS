//! Sole, bounded ring writer. Control authority is never renewed by frame data.
use crate::{
    ring_face::{Face, Lit, Mood},
    ring_light::RingLight,
    ring_sight::RingSight,
    ring_wire::{RingBlankReason, RingFeedback, RingFrame, RingPhase, RingRejection},
    transport::WorkerChannels,
    wire::{Control, WorkerEvent},
};
use lamp_ipc::monotonic_us;
use lamp_ring::{
    BlankReason, BlankReceipt, ChannelCeiling, Failure, FrameSink, GuardedRing, PIXEL_COUNT,
};
use lamp_turn_owner::{BootId, BoundaryGuard, Error as AuthorityError, MonoTime, OutputKind};
use std::{io, time::Duration};

pub const CONTROL_BUDGET: usize = 16;
pub const FRAME_MAX_AGE_US: u64 = 100_000;
pub const INITIAL_AUTHORITY_US: u64 = 250_000;
// The active directed clock starts after this worker. Reserve time for privacy
// worker startup, input readiness and cooperative shutdown. Authority still
// expires independently; this ceiling does not extend any permission lease.
pub const MAX_RUNTIME_US: u64 = (crate::coordinator::MAX_DIRECTED_SECONDS + 30) * 1_000_000;
const TICK_US: u64 = 2_000;
/// A resting frame is painted this often: forty a second, enough for a slow
/// breath and little enough to leave the loop to cues and control.
const RESTING_INTERVAL_US: u64 = 25_000;

/// Nonblocking operations only; errors, including feedback backpressure, fail
/// closed. This seam permits deterministic cross-channel ordering tests.
pub trait RingLink {
    fn receive_control(&mut self) -> io::Result<Option<Control>>;
    fn receive_frame(&mut self) -> io::Result<Option<RingFrame>>;
    fn publish(&mut self, event: WorkerEvent) -> io::Result<()>;
}

impl RingLink for WorkerChannels {
    fn receive_control(&mut self) -> io::Result<Option<Control>> {
        self.control.receive()
    }
    fn receive_frame(&mut self) -> io::Result<Option<RingFrame>> {
        self.data.receive()
    }
    fn publish(&mut self, event: WorkerEvent) -> io::Result<()> {
        self.control.send(event)
    }
}

fn now() -> MonoTime {
    MonoTime::from_micros(monotonic_us())
}

/// What a ring that runs all day takes from the bus: the lamp's own light
/// ([`crate::ring_light`]), and with the eye ([`crate::ring_face`], task 34),
/// where the person is ([`crate::ring_sight`]). A ring with neither has
/// [`Senses::default`].
#[derive(Default)]
pub struct Senses {
    /// Enabled cap intake, dark until a complete valid snapshot is accepted.
    pub caps: Option<crate::ring_caps::RingCaps>,
    pub light: Option<(RingLight, Lit)>,
    pub eye: bool,
    /// The strong eye ([`Face::eye_strong`]), only with `eye`.
    pub eye_strong: bool,
    pub sight: Option<RingSight>,
    /// A hand kept on the head, for its countdown ([`crate::ring_head`]).
    pub head: Option<crate::ring_head::RingHead>,
    /// The lamp's inner state, for the eye's pace at rest
    /// ([`crate::ring_mood`]).
    pub mood: Option<crate::ring_mood::RingMood>,
    pub unplug: Option<crate::ring_unplug::RingUnplug>,
    /// The primary button held, for the shutdown's warning (#3697): with
    /// `LAMP_SHUTDOWN=on`, held 5 to 15 s ([`lamp_msgs::shutdown_hold`]),
    /// the ring shows [`shutdown_warning`] in place of its face.
    pub button: Option<crate::ring_head::RingHead>,
}

/// The shutdown's warning while the primary button is held in its window
/// (#3697, `docs/shutdown.md`): the whole ring in the setup light's amber
/// ([`SetupLook::Failed`]), steady, no brighter than the setup light. Released
/// there, the lamp turns off; held past it, the warning goes and nothing is
/// asked.
pub fn shutdown_warning() -> [lamp_ring::Rgb; PIXEL_COUNT] {
    use lamp_ring::setup::{SetupLook, setup_pixels};
    setup_pixels(SetupLook::Failed, SetupLook::Failed, 1.0, 1.0)
}

/// Startup Ready is emitted only after the sink accepted its initial black frame.
/// The caller owns process supervision; a kernel SPI write cannot be preempted.
pub fn run_with_sink(
    link: impl RingLink,
    controller_boot: BootId,
    sink: impl FrameSink,
) -> io::Result<()> {
    run_with_sink_resting(link, controller_boot, sink, None)
}

/// As [`run_with_sink`], and with `resting` set the ring also shows a
/// resting face under that ceiling whenever no turn owns the lamp and the
/// microphone is permitted. See [`GuardedRing::show_resting`].
pub fn run_with_sink_resting(
    mut link: impl RingLink,
    controller_boot: BootId,
    sink: impl FrameSink,
    resting: Option<u16>,
) -> io::Result<()> {
    let resting = match resting.map(ChannelCeiling::new).transpose() {
        Ok(resting) => resting,
        Err(error) => return report_start_failure(&mut link, io::Error::other(error)),
    };
    match GuardedRing::new(sink, controller_boot) {
        Ok(ring) => run_guarded(
            link,
            controller_boot,
            ring,
            false,
            resting,
            Senses::default(),
        ),
        Err(error) => report_start_failure(&mut link, io::Error::other(error)),
    }
}

#[cfg(target_os = "linux")]
pub fn run(mut link: WorkerChannels, controller_boot: BootId) -> io::Result<()> {
    match lamp_ring::linux::open(controller_boot) {
        Ok((ring, _configuration)) => {
            run_guarded(link, controller_boot, ring, false, None, Senses::default())
        }
        Err(error) => report_start_failure(&mut link, error),
    }
}

/// The bounded directed worker uses the same caps as the continuous worker.
#[cfg(target_os = "linux")]
pub fn run_with_caps(
    mut link: WorkerChannels,
    controller_boot: BootId,
    bus_root: Option<&std::path::Path>,
) -> io::Result<()> {
    let senses = Senses {
        caps: Some(open_caps(bus_root)),
        ..Senses::default()
    };
    match lamp_ring::linux::open(controller_boot) {
        Ok((ring, _configuration)) => run_guarded(link, controller_boot, ring, false, None, senses),
        Err(error) => report_start_failure(&mut link, error),
    }
}

#[cfg(target_os = "linux")]
fn open_caps(bus_root: Option<&std::path::Path>) -> crate::ring_caps::RingCaps {
    let opened = bus_root
        .ok_or_else(|| io::Error::other("output caps require --bus-root"))
        .and_then(|root| {
            boot_clock().and_then(|clock| crate::ring_caps::RingCaps::on(root, &clock))
        });
    match opened {
        Ok(caps) => caps,
        Err(error) => {
            eprintln!(
                "lamp-voice ring: output caps unavailable; ordinary output stays dark: {error}"
            );
            crate::ring_caps::RingCaps::dark()
        }
    }
}

/// What the continuous ring runs with, as `lamp-voice ring` is started.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Default)]
pub struct RingOptions<'a> {
    pub resting: Option<u16>,
    pub bus_root: Option<&'a std::path::Path>,
    pub eye: bool,
    pub eye_strong: bool,
    pub head_touch: bool,
    pub light_requests: bool,
    pub skill_light_speaking: bool,
    pub light_keep: Option<&'a std::path::Path>,
    pub output_caps: bool,
}

/// Supervisor-only lifetime selection. This removes the finite test ceiling,
/// not the parent transport or initial-lease watchdogs; an installed
/// authority that runs out holds the ring dark until a fresh one (#1178).
/// With `bus_root`, the lamp's own light is taken from the bus there and
/// what the ring shows is said on it ([`crate::ring_light`]); a bus that
/// cannot be used is said once and the ring runs without. With `eye`, the
/// face is the eye ([`crate::ring_face`]), and looks where the eyes on the
/// same bus see a person ([`crate::ring_sight`]). With `head_touch`, a hand
/// kept on the head dims it, a countdown to sleep ([`crate::ring_head`]).
/// With a bus, the face at rest breathes the lamp's one breath where Mood
/// keeps it on that bus ([`crate::ring_mood`]). With `light_requests`, lights asked for that lapse are shown over the
/// person's ([`crate::ring_requests`], #940), and with
/// `skill_light_speaking` as well the one shown tints the Speaking face
/// ([`Face::skill_light`]). With `light_keep`, the
/// person's light is kept there across a restart ([`crate::ring_light`]).
#[cfg(target_os = "linux")]
pub fn run_continuous(
    mut link: WorkerChannels,
    controller_boot: BootId,
    options: RingOptions<'_>,
) -> io::Result<()> {
    let RingOptions {
        resting,
        bus_root,
        eye,
        eye_strong,
        head_touch,
        light_requests,
        skill_light_speaking,
        light_keep,
        output_caps,
    } = options;
    let caps = output_caps.then(|| open_caps(bus_root));
    let light = bus_root.and_then(|root| match RingLight::on_keeping(root, light_keep) {
        Ok((mut light, begins)) => {
            light.tint_speaking(light_requests && skill_light_speaking);
            // Without them the ring still shows the person's light.
            if light_requests
                && let Err(error) = boot_clock().and_then(|clock| light.take_requests(root, &clock))
            {
                eprintln!("lamp-voice ring: lights asked for that lapse are not taken: {error}");
            }
            Some((light, begins))
        }
        Err(error) => {
            eprintln!("lamp-voice ring: the lamp's light is not on the bus: {error}");
            None
        }
    });
    let sight = bus_root
        .filter(|_| eye)
        .and_then(|root| match RingSight::on(root) {
            Ok(sight) => Some(sight),
            Err(error) => {
                eprintln!("lamp-voice ring: the eye cannot see where people are: {error}");
                None
            }
        });
    let head =
        bus_root.filter(|_| head_touch).and_then(|root| {
            match crate::ring_head::RingHead::on(root) {
                Ok(head) => Some(head),
                Err(error) => {
                    eprintln!("lamp-voice ring: the head's hand cannot be read: {error}");
                    None
                }
            }
        });
    // No switch of its own: Mood is off unless `LAMP_MOOD=on`, and with no
    // fresh mood the face breathes as before.
    let mood = bus_root.and_then(|root| match crate::ring_mood::RingMood::on(root) {
        Ok(mood) => Some(mood),
        Err(error) => {
            eprintln!("lamp-voice ring: the lamp's inner state cannot be read: {error}");
            None
        }
    });
    let unplug = bus_root
        .filter(|_| lamp_body::unplug::enabled())
        .map(crate::ring_unplug::RingUnplug::on)
        .transpose()?;
    // Only with the switch on, which the voice hands this worker (#3697).
    let button = bus_root
        .filter(|_| lamp_msgs::shutdown_switched_on())
        .map(|root| crate::ring_head::RingHead::on_source(root, lamp_msgs::TouchSource::Primary))
        .transpose()?;
    let senses = Senses {
        caps,
        unplug,
        button,
        light,
        eye,
        eye_strong,
        sight,
        head,
        mood,
    };
    let resting = match resting.map(ChannelCeiling::new).transpose() {
        Ok(resting) => resting,
        Err(error) => return report_start_failure(&mut link, io::Error::other(error)),
    };
    match lamp_ring::linux::open(controller_boot) {
        Ok((ring, _configuration)) => {
            run_guarded(link, controller_boot, ring, true, resting, senses)
        }
        Err(error) => report_start_failure(&mut link, error),
    }
}

/// The host's boot, the clock a light request's claim is on.
#[cfg(target_os = "linux")]
fn boot_clock() -> io::Result<String> {
    Ok(std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?
        .trim()
        .to_owned())
}

fn check_runtime(started: u64, at: u64, continuous: bool) -> io::Result<()> {
    let age = at
        .checked_sub(started)
        .ok_or_else(|| io::Error::other("ring worker clock regressed"))?;
    if !continuous && age >= MAX_RUNTIME_US {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "ring worker duration expired",
        ));
    }
    Ok(())
}

/// No authority within [`INITIAL_AUTHORITY_US`] of the start ends the ring.
/// An installed one that ran out with no renewal does not (#1178): whether
/// it has, as `Ok(true)`. The ring's own guard blanks a cue whose lease ran
/// out and refuses every frame and rest under it, so the ring is dark until
/// a fresh one comes; a voice whose loop was held up a moment goes on.
fn check_authority(started: u64, at: u64, expires_at: Option<u64>) -> io::Result<bool> {
    match expires_at {
        Some(expires) => Ok(at >= expires),
        None if at.saturating_sub(started) >= INITIAL_AUTHORITY_US => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "ring authority never came",
        )),
        None => Ok(false),
    }
}

/// The ring held dark while its loop is alive (#1178), and since when, for
/// the journal: its authority ran out with no renewal, or a newer one came
/// past its time. That newer one may have revoked what the older lease
/// allows, so nothing is painted until one newer than it installs.
#[derive(Default)]
struct Lapse {
    ran_out_us: Option<u64>,
    /// The revision of the newest successor that came too late, and when.
    late: Option<(u64, u64)>,
}

impl Lapse {
    /// Whether frames and rest are held back for a successor that came late.
    fn dark(&self) -> bool {
        self.late.is_some()
    }
}

fn report_start_failure(link: &mut impl RingLink, error: io::Error) -> io::Result<()> {
    let _ = link.publish(WorkerEvent::Fault {
        code: bounded(&format!("ring startup: {error}")),
    });
    Err(error)
}

/// As [`run_with_sink_resting`], with the senses an all-day ring takes from
/// the bus, for host tests of the same loop the device's worker runs.
pub fn run_with_sink_senses(
    mut link: impl RingLink,
    controller_boot: BootId,
    sink: impl FrameSink,
    resting: Option<u16>,
    senses: Senses,
) -> io::Result<()> {
    let resting = match resting.map(ChannelCeiling::new).transpose() {
        Ok(resting) => resting,
        Err(error) => return report_start_failure(&mut link, io::Error::other(error)),
    };
    match GuardedRing::new(sink, controller_boot) {
        Ok(ring) => run_guarded(link, controller_boot, ring, false, resting, senses),
        Err(error) => report_start_failure(&mut link, io::Error::other(error)),
    }
}

fn run_guarded(
    mut link: impl RingLink,
    controller_boot: BootId,
    mut ring: GuardedRing<impl FrameSink>,
    continuous: bool,
    resting: Option<ChannelCeiling>,
    senses: Senses,
) -> io::Result<()> {
    let result = (|| {
        link.publish(WorkerEvent::Ready)?;
        service(
            &mut link,
            &mut ring,
            controller_boot,
            continuous,
            resting,
            senses,
        )
    })();
    if let Err(original) = result {
        // GuardedRing already performs its sole cleanup on a write fault. Do
        // not turn that failure into repeated cleanup writes. A transport/control
        // fault gets exactly one control_lost blank, before feedback is attempted.
        let mut detail = format!("ring worker: {original}");
        if !ring.is_faulted() && !ring.is_stopped() {
            match ring.control_lost() {
                Ok(receipt) => {
                    if let Err(error) = blanked(&mut link, receipt) {
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
        return Err(io::Error::other(code));
    }
    Ok(())
}

fn bounded(text: &str) -> String {
    text.chars().take(512).collect()
}

fn blanked(link: &mut impl RingLink, receipt: BlankReceipt) -> io::Result<()> {
    let reason = match receipt.reason {
        BlankReason::Shutdown => RingBlankReason::Shutdown,
        BlankReason::ControlLost => RingBlankReason::ControlLost,
        BlankReason::AuthorityLost(reason) => RingBlankReason::AuthorityLost {
            reason: reason.into(),
        },
        BlankReason::Startup | BlankReason::ExplicitOff => {
            return Err(io::Error::other("unexpected worker blank reason"));
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

fn reject(link: &mut impl RingLink, frame: RingFrame, reason: RingRejection) -> io::Result<()> {
    link.publish(WorkerEvent::Ring {
        report: RingFeedback::Rejected {
            owner: frame.permit.owner(),
            phase: frame.phase,
            requested_at_us: frame.requested_at_us,
            reason,
        },
    })
}

/// True means Stop completed. Exhausting the shared control budget defers data;
/// it never proves that the queue was empty or permits a frame write.
fn controls(
    link: &mut impl RingLink,
    ring: &mut GuardedRing<impl FrameSink>,
    guard: &mut BoundaryGuard,
    remaining: &mut usize,
    lapse: &mut Lapse,
) -> io::Result<bool> {
    while *remaining > 0 {
        let Some(command) = link.receive_control()? else {
            return Ok(false);
        };
        *remaining -= 1;
        match command {
            Control::Stop => {
                if let Some(receipt) = ring.shutdown().map_err(io::Error::other)? {
                    blanked(link, receipt)?;
                }
                return Ok(true);
            }
            Control::Authority { snapshot } => {
                if guard.state().is_some_and(|current| {
                    current.revision() == snapshot.revision() && current != snapshot
                }) {
                    return Err(io::Error::other(
                        "ring authority conflicts at same revision",
                    ));
                }
                match guard.install(now(), snapshot) {
                    Ok(()) => {}
                    // These packets cannot extend either installed lease.
                    Err(AuthorityError::StaleSnapshot) => continue,
                    Err(AuthorityError::ExpiredState)
                        if guard
                            .state()
                            .is_some_and(|current| snapshot.revision() <= current.revision()) =>
                    {
                        continue;
                    }
                    // A newer state that came past its time (the voice's loop
                    // held up a moment, #1178) may revoke privacy or
                    // ownership. Never keep displaying under the older allowed
                    // lease: blank now, and paint nothing until one newer than
                    // it installs. The ring goes on.
                    Err(AuthorityError::ExpiredState) => {
                        // Blanked once: the ring is dark from the first, and
                        // more writes would only add to a bus under stress.
                        if lapse.late.is_none() {
                            let receipt = ring.lapse().map_err(io::Error::other)?;
                            blanked(link, receipt)?;
                        }
                        let at = monotonic_us();
                        let since = match lapse.late {
                            Some((_, since)) => since,
                            None => {
                                eprintln!(
                                    "lamp-voice ring: a newer authority came {} ms past its time; dark until a fresh one",
                                    at.saturating_sub(snapshot.expires_at().as_micros()) / 1_000
                                );
                                at
                            }
                        };
                        let newest = lapse.late.map_or(snapshot.revision(), |(late, _)| {
                            late.max(snapshot.revision())
                        });
                        lapse.late = Some((newest, since));
                        continue;
                    }
                    Err(error) => return Err(io::Error::other(error)),
                }
                if let Some(receipt) = ring.install_authority(snapshot).map_err(io::Error::other)? {
                    blanked(link, receipt)?;
                }
                if let Some((late, since)) = lapse.late
                    && snapshot.revision() > late
                {
                    eprintln!(
                        "lamp-voice ring: a fresh authority {} ms after one came late; it goes on",
                        monotonic_us().saturating_sub(since) / 1_000
                    );
                    lapse.late = None;
                }
            }
            Control::StartCapture
            | Control::ConnectReference { .. }
            | Control::ProviderOutputCapacity { .. } => {
                return Err(io::Error::other("invalid ring control command"));
            }
        }
    }
    Ok(false)
}

fn service(
    link: &mut impl RingLink,
    ring: &mut GuardedRing<impl FrameSink>,
    controller_boot: BootId,
    continuous: bool,
    resting: Option<ChannelCeiling>,
    senses: Senses,
) -> io::Result<()> {
    let started = monotonic_us();
    let mut guard = BoundaryGuard::new(controller_boot, now());
    let mut pending = None;
    // One face for cues and for rest, so that each eases into the other.
    let mut face = Face::new();
    // The lamp's own light, which is that face at rest: the one last asked
    // for on the bus, and from here on whatever is asked for.
    let Senses {
        mut caps,
        light,
        eye,
        eye_strong,
        mut sight,
        mut head,
        mut mood,
        mut unplug,
        mut button,
    } = senses;
    let mut light = light.map(|(light, begins)| {
        face.light(begins);
        light
    });
    face.eye(eye);
    face.eye_strong(eye_strong);
    let mut rested_at = 0u64;
    // From zero, so a late startup black write in GuardedRing::new is said too.
    let mut late_said = LateSaid::default();
    let mut lapse = Lapse::default();
    let mut cap_ceiling = if caps.is_some() { 0 } else { 120 };
    // With LAMP_STALLS=on: a pass of this loop, or a gap between two, of
    // 50 ms or more is recorded, against its lease (#1178).
    let mut stalls = crate::stalls::Stalls::from_environment("ring");
    loop {
        // A late SPI write (the SD/MMC host re-tuning holds the bus) is a
        // late frame; only a run of them lasting 2 s ends the ring (#911,
        // #1620). Said at most once a minute, with how long the run has been.
        if let Some(line) = late_said.line(ring.late_writes(), ring.last_late(), monotonic_us()) {
            eprintln!("{line}");
        }
        let tick = monotonic_us();
        let stall_pass = stalls.begin();
        let mut budget = CONTROL_BUDGET;
        if controls(link, ring, &mut guard, &mut budget, &mut lapse)? {
            return Ok(());
        }
        if let Some(receipt) = ring.tick().map_err(io::Error::other)? {
            blanked(link, receipt)?;
        }
        let at = monotonic_us();
        check_runtime(started, at, continuous)?;
        let ran_out = check_authority(
            started,
            at,
            guard.state().map(|state| state.expires_at().as_micros()),
        )?;
        match (ran_out, lapse.ran_out_us) {
            (true, None) => {
                eprintln!(
                    "lamp-voice ring: its authority ran out with no renewal; dark until a fresh one"
                );
                lapse.ran_out_us = Some(at);
            }
            (false, Some(since)) => {
                eprintln!(
                    "lamp-voice ring: a fresh authority {} ms after the last ran out; it goes on",
                    at.saturating_sub(since) / 1_000
                );
                lapse.ran_out_us = None;
            }
            _ => {}
        }
        // The head is optional input: read after revocation and expiry, and
        // only a bounded few at a time (#849 review).
        if let Some(head) = head.as_mut() {
            face.hand(head.tick(at));
        }
        if let Some(caps) = caps.as_mut() {
            let next = caps.tick(at);
            if next < cap_ceiling {
                // A tighter cap must not leave an earlier bright frame
                // latched while no new turn frame arrives. This is an
                // explicit protective blank, not lost turn authority.
                ring.off().map_err(io::Error::other)?;
            }
            cap_ceiling = next;
        }
        if budget > 0 && pending.is_none() {
            pending = link.receive_frame()?;
        }
        // One shared budget, including controls that arrived during the data
        // receive. A pending frame cannot create another fresh budget or lease.
        if controls(link, ring, &mut guard, &mut budget, &mut lapse)? {
            return Ok(());
        }
        if let Some(frame) = pending {
            let at = monotonic_us();
            let reason = if frame.requested_at_us == 0
                || frame.requested_at_us > at
                || frame.permit.issued_at().as_micros() > frame.requested_at_us
                || frame.snapshot.issued_at().as_micros() > frame.requested_at_us
            {
                Some(RingRejection::FutureRequest)
            } else if at - frame.requested_at_us >= FRAME_MAX_AGE_US {
                Some(RingRejection::ExpiredRequest)
            } else if frame.ceiling > 120 {
                Some(RingRejection::InvalidCeiling)
            } else if RingPhase::from_snapshot(frame.snapshot) != Some(frame.phase) {
                Some(RingRejection::PhaseMismatch)
            } else {
                None
            };
            if let Some(reason) = reason {
                pending = None;
                reject(link, frame, reason)?;
            } else if budget > 0
                && let Some(state) = guard.state()
            {
                if frame.snapshot.boot() != controller_boot {
                    pending = None;
                    reject(link, frame, RingRejection::WrongBoot)?;
                } else if frame.snapshot.revision() <= state.revision() {
                    if frame.snapshot.revision() == state.revision() && frame.snapshot != state {
                        return Err(io::Error::other("ring snapshot conflicts at same revision"));
                    }
                    pending = None;
                    // Heartbeats can overtake data. Never install its old state:
                    // require matching owner/phase and check the original permit
                    // against the latest priority authority at the final boundary.
                    if frame.snapshot.owner() != state.owner() {
                        reject(link, frame, RingRejection::StaleOwner)?;
                    } else if RingPhase::from_snapshot(state) != Some(frame.phase) {
                        reject(link, frame, RingRejection::PhaseMismatch)?;
                    } else if lapse.dark() {
                        // A newer authority came late: nothing under the older.
                        reject(link, frame, RingRejection::ExpiredState)?;
                    } else if let Err(reason) = guard.check(now(), frame.permit, OutputKind::Light)
                    {
                        reject(link, frame, reason.into())?;
                    } else {
                        let ceiling = ChannelCeiling::new(frame.ceiling.min(cap_ceiling))
                            .map_err(io::Error::other)?;
                        if let Some(sight) = sight.as_mut() {
                            face.toward(sight.tick(monotonic_us()));
                        }
                        face.accent(frame.accent, frame.accent_at_us);
                        let at = monotonic_us();
                        let warning = warning(button.as_mut().and_then(|button| button.tick(at)));
                        let presentation = unplug
                            .as_ref()
                            .filter(|_| face.lit().on && warning.is_none())
                            .and_then(|unplug| unplug.prepare(at));
                        let pixels = warning.unwrap_or_else(|| {
                            presentation.as_ref().map_or_else(
                                || draw(&mut face, &frame, at, ceiling.get()),
                                |presentation| presentation.pixels,
                            )
                        });
                        match ring.show(&pixels, ceiling, frame.permit) {
                            Ok(write) => {
                                if let (Some(unplug), Some(presentation)) =
                                    (unplug.as_mut(), presentation.as_ref())
                                {
                                    unplug.drew(write.finished_at().as_micros(), presentation)?;
                                }
                                link.publish(WorkerEvent::Ring {
                                    report: RingFeedback::Presented {
                                        owner: frame.permit.owner(),
                                        phase: frame.phase,
                                        requested_at_us: frame.requested_at_us,
                                        write_started_at_us: write.started_at().as_micros(),
                                        write_finished_at_us: write.finished_at().as_micros(),
                                    },
                                })?;
                            }
                            Err(error) => match error.cause {
                                Failure::Authority(reason)
                                    if error.cleanup_error.is_none() && !ring.is_faulted() =>
                                {
                                    // A deadline can cross the final device check.
                                    // GuardedRing reconciles it; no fabricated
                                    // successful blank/write timestamp is emitted.
                                    reject(link, frame, reason.into())?;
                                }
                                _ => return Err(io::Error::other(error)),
                            },
                        }
                    }
                }
            }
        }
        // Rest only when no cue is waiting and control still has budget: a
        // resting frame never delays a cue or a revocation. The ring refuses
        // it, with no write, unless nobody owns the lamp and it may listen.
        let at = monotonic_us();
        if let Some(light) = light.as_mut() {
            if let Some(asked) = light.tick(at) {
                face.light(asked);
            }
            face.pattern(light.pattern());
            face.fade(light.fade(), at);
            face.tints_speaking(light.tints_speaking());
            face.skill_light(light.held());
        }
        if let Some(mood) = mood.as_mut() {
            face.breath(mood.tick(at));
        }
        if let Some(ceiling) = resting
            && pending.is_none()
            && budget > 0
            && !lapse.dark()
            && at.saturating_sub(rested_at) >= RESTING_INTERVAL_US
        {
            rested_at = at;
            let ceiling = ChannelCeiling::new(u16::from(ceiling.get()).min(cap_ceiling))
                .map_err(io::Error::other)?;
            let warning = warning(button.as_mut().and_then(|button| button.tick(at)));
            let presentation = unplug
                .as_ref()
                .filter(|_| face.lit().on && warning.is_none())
                .and_then(|unplug| unplug.prepare(at));
            if let Some(pixels) = warning {
                ring.show_resting(&pixels, ceiling)
                    .map_err(io::Error::other)?;
            } else if let (Some(unplug), Some(presentation)) =
                (unplug.as_mut(), presentation.as_ref())
            {
                if let Some(receipt) = ring
                    .show_resting(&presentation.pixels, ceiling)
                    .map_err(io::Error::other)?
                {
                    unplug.drew(receipt.finished_at().as_micros(), presentation)?;
                }
            } else {
                let pixels = face.render(Mood::Resting, 0, at, ceiling.get());
                let own_light = face.rests_on_its_light(at);
                rest(ring, &pixels, ceiling, own_light, light.as_mut())?;
            }
        }
        stall_pass.done();
        if let Some(wait) = TICK_US.checked_sub(monotonic_us().saturating_sub(tick)) {
            std::thread::sleep(Duration::from_micros(wait));
        }
    }
}

/// [`shutdown_warning`] while the primary button is held (`held`, as
/// Controls last said) in the shutdown's window, or `None`.
fn warning(held: Option<u64>) -> Option<[lamp_ring::Rgb; PIXEL_COUNT]> {
    held.filter(|&held| lamp_msgs::shutdown_hold(held))
        .map(|_| shutdown_warning())
}

/// Late writes are said at most this often.
const LATE_SAID_EVERY_US: u64 = 60_000_000;

/// The journal of late SPI writes: the newest one, at most once a minute,
/// with how many went unsaid since the last line.
#[derive(Default)]
struct LateSaid {
    /// Late writes seen so far.
    seen: u64,
    /// Of them, the ones counted into a line.
    said: u64,
    at_us: Option<u64>,
}

impl LateSaid {
    fn line(
        &mut self,
        late_writes: u64,
        last: Option<lamp_ring::LateWrite>,
        now_us: u64,
    ) -> Option<String> {
        if late_writes == self.seen {
            return None;
        }
        self.seen = late_writes;
        let last = last?;
        if self
            .at_us
            .is_some_and(|at| now_us.saturating_sub(at) < LATE_SAID_EVERY_US)
        {
            return None;
        }
        let unsaid = late_writes - self.said - 1;
        self.said = late_writes;
        self.at_us = Some(now_us);
        let mut line = format!(
            "lamp-voice ring: SPI write {} us, over {} us, late for {} ms",
            last.elapsed_us,
            lamp_ring::WRITE_BUDGET_US,
            last.run_us / 1_000
        );
        if unsaid > 0 {
            line.push_str(&format!(" ({unsaid} more since the last line)"));
        }
        Some(line)
    }
}

/// Paints the resting frame and, only if the ring wrote it, says the light
/// drawn: a frame it refused (the privacy switch, a turn) says nothing.
/// `own_light`: whether the face shows the lamp's own light in it, for a
/// requested light's reach (`light/reached`).
fn rest<S: FrameSink>(
    ring: &mut GuardedRing<S>,
    pixels: &[lamp_ring::Rgb; PIXEL_COUNT],
    ceiling: ChannelCeiling,
    own_light: bool,
    light: Option<&mut RingLight>,
) -> io::Result<()> {
    if let Some(receipt) = ring
        .show_resting(pixels, ceiling)
        .map_err(io::Error::other)?
        && let Some(light) = light
    {
        light.drew(receipt.finished_at().as_micros(), own_light);
    }
    Ok(())
}

/// A frame's face at `at_us`. A frame of the head tap's "hm?" starts the
/// tap's window at rest (#711), so only a tap the voice answers gets the
/// listening face after its turn.
fn draw(
    face: &mut Face,
    frame: &RingFrame,
    at_us: u64,
    ceiling: u8,
) -> [lamp_ring::Rgb; PIXEL_COUNT] {
    if frame.listening {
        face.hum_frame(frame.permit.owner().turn(), at_us);
    }
    face.render(frame.mood(), frame.level, at_us, ceiling)
}

#[cfg(test)]
mod drawn_tests {
    use super::*;
    use crate::ring_light::DRAWN_TOPIC;
    use lamp_bus::Bus;
    use lamp_msgs::Light;
    use lamp_turn_owner::{AdmissionState, CaptureState, Controller, Permission};

    struct Sink(usize);
    impl FrameSink for Sink {
        fn write_frame(&mut self, _: &lamp_ring::EncodedFrame) -> io::Result<()> {
            self.0 += 1;
            Ok(())
        }
    }

    fn ring_with(permission: Permission, turn: bool) -> GuardedRing<Sink> {
        let boot = crate::process::new_boot().unwrap();
        let mut controller = Controller::new(boot, now());
        controller
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(now().as_micros() + 500_000);
        controller
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        controller
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
        if turn {
            let owner = controller
                .admit(now(), lamp_turn_owner::AdmittedInput::NewTurn)
                .unwrap();
            controller.input_ended(now(), owner).unwrap();
        }
        // Closed after the input was ready, as the switch closes it.
        controller
            .set_microphone_permission(now(), permission)
            .unwrap();
        let mut ring = GuardedRing::new(Sink(0), boot).unwrap();
        ring.install_authority(controller.snapshot(now()).unwrap())
            .unwrap();
        ring
    }

    /// A frame of a new turn's waiting phase, the hum's when `listening`.
    fn turn_frame(listening: bool) -> RingFrame {
        let boot = crate::process::new_boot().unwrap();
        let mut controller = Controller::new(boot, now());
        controller
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(now().as_micros() + 500_000);
        controller
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        controller
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
        let owner = controller
            .admit(now(), lamp_turn_owner::AdmittedInput::NewTurn)
            .unwrap();
        controller.input_ended(now(), owner).unwrap();
        let plan = controller
            .plan_output(now(), owner, OutputKind::Light, None)
            .unwrap();
        let permit = controller.issue_output(now(), plan, 100_000).unwrap();
        let snapshot = controller.snapshot(now()).unwrap();
        RingFrame {
            snapshot,
            permit,
            phase: RingPhase::from_snapshot(snapshot).unwrap(),
            ceiling: 40,
            requested_at_us: monotonic_us(),
            level: 0,
            attending: false,
            offline: false,
            listening,
            accent: 0,
            accent_at_us: 0,
        }
    }

    /// First on #705, 17 Oct 07:26: the face at rest after a tap only when
    /// the voice's hum plays. Drawn, a hum's frame opens the tap's window,
    /// so the resting frames after it are not the lamp's own light until
    /// the window and its settle are over; another turn's frame opens none.
    #[test]
    fn only_a_frame_of_the_hum_opens_the_taps_window_at_rest() {
        let at = 5_000_000_000;
        let end = at + crate::ring_face::TAP_LISTENS_US + 600_000;
        let mut face = Face::new();
        draw(&mut face, &turn_frame(false), at, 40);
        assert!(face.rests_on_its_light(at + 1));
        draw(&mut face, &turn_frame(true), at, 40);
        assert!(!face.rests_on_its_light(at + 1));
        assert!(!face.rests_on_its_light(end - 1));
        assert!(face.rests_on_its_light(end));
    }

    #[test]
    fn a_draw_the_ring_refuses_says_nothing_and_one_it_makes_says_the_light() {
        let root = std::path::PathBuf::from(format!("/tmp/lv-drawn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let (mut light, _) = RingLight::on(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let pixels = [lamp_ring::Rgb::new(6, 4, 2); PIXEL_COUNT];
        let ceiling = ChannelCeiling::new(40).unwrap();
        // The privacy switch closed: refused, nothing drawn is said.
        let mut closed = ring_with(Permission::Denied, false);
        rest(&mut closed, &pixels, ceiling, true, Some(&mut light)).unwrap();
        assert!(bus.latest(DRAWN_TOPIC).unwrap().is_none());
        // A turn owns the lamp: refused, nothing said either.
        let mut owned = ring_with(Permission::Allowed, true);
        rest(&mut owned, &pixels, ceiling, true, Some(&mut light)).unwrap();
        assert!(bus.latest(DRAWN_TOPIC).unwrap().is_none());
        // Allowed and nobody owns the lamp: drawn, and said.
        let mut open = ring_with(Permission::Allowed, false);
        rest(&mut open, &pixels, ceiling, true, Some(&mut light)).unwrap();
        let drawn: Light = bus.latest(DRAWN_TOPIC).unwrap().unwrap().parse().unwrap();
        assert_eq!(&drawn, light.face());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// #1166: a requested light is reached only on resting frames the ring
    /// wrote. With the privacy switch closed every frame is refused and
    /// nothing is said; drawn, it is said once 350 ms have shown it.
    #[test]
    fn a_requested_light_is_reached_only_on_resting_frames_the_ring_draws() {
        use lamp_msgs::{Claim, LightReached, LightRequest, Priority};
        use std::time::Instant;
        let root = std::path::PathBuf::from(format!("/tmp/lv-reach-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let (mut light, _) = RingLight::on(&root).unwrap();
        light.take_requests(&root, "boot-test").unwrap();
        let bus = Bus::new(&root).unwrap();
        let mut reached = bus.subscribe(LightReached::TOPIC, "test").unwrap();
        let mut glows = bus
            .publisher_declared(LightRequest::TOPIC, &LightRequest::declaration(), "skills")
            .unwrap();
        let asked = Light::new(true, "white", 5).unwrap();
        glows
            .publish_json(
                1,
                &LightRequest {
                    claim: Claim {
                        source: "skill:desk-tutor".into(),
                        priority: Priority::Skill,
                        clock_id: "boot-test".into(),
                        until_us: monotonic_us() + 20_000_000,
                    },
                    light: asked.clone(),
                    pattern: None,
                    fade_ms: None,
                },
            )
            .unwrap();
        let started = Instant::now();
        while light.face() != &asked {
            assert!(started.elapsed() < Duration::from_secs(5), "never taken");
            light.tick(monotonic_us());
            std::thread::sleep(Duration::from_millis(2));
        }
        let pixels = [lamp_ring::Rgb::new(6, 4, 2); PIXEL_COUNT];
        let ceiling = ChannelCeiling::new(40).unwrap();
        // The privacy switch closed: half a second of refused frames.
        let mut closed = ring_with(Permission::Denied, false);
        for _ in 0..20 {
            rest(&mut closed, &pixels, ceiling, true, Some(&mut light)).unwrap();
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(reached.recv(Duration::ZERO).unwrap().is_none());
        // Drawn every 20 ms (a ring made afresh within each authority's
        // half second): said once, 350 ms on.
        let first = Instant::now();
        let mut open = ring_with(Permission::Allowed, false);
        let mut made = Instant::now();
        let said = loop {
            assert!(first.elapsed() < Duration::from_secs(3), "never said");
            if made.elapsed() > Duration::from_millis(200) {
                open = ring_with(Permission::Allowed, false);
                made = Instant::now();
            }
            rest(&mut open, &pixels, ceiling, true, Some(&mut light)).unwrap();
            if let Some(message) = reached.recv(Duration::ZERO).unwrap() {
                break message.parse::<LightReached>().unwrap();
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(first.elapsed() >= Duration::from_micros(crate::ring_light::REACHED_AFTER_US));
        assert_eq!(
            (said.source.as_str(), said.light),
            ("skill:desk-tutor", asked)
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod late_said_tests {
    use super::*;
    use lamp_ring::LateWrite;

    #[test]
    fn late_writes_are_said_at_most_once_a_minute_with_the_unsaid_counted() {
        let late = |elapsed_us, run_us| Some(LateWrite { elapsed_us, run_us });
        let mut said = LateSaid::default();
        assert_eq!(said.line(0, None, 1), None);
        assert_eq!(
            said.line(1, late(13_180, 340_000), 1_000).as_deref(),
            Some("lamp-voice ring: SPI write 13180 us, over 10000 us, late for 340 ms")
        );
        // The same count again, and two more within the minute: nothing.
        assert_eq!(said.line(1, late(13_180, 340_000), 2_000), None);
        assert_eq!(said.line(2, late(14_000, 360_000), 3_000), None);
        assert_eq!(said.line(3, late(12_000, 12_000), 59_000_999), None);
        assert_eq!(
            said.line(4, late(11_500, 11_500), 60_001_000).as_deref(),
            Some(
                "lamp-voice ring: SPI write 11500 us, over 10000 us, late for 11 ms (2 more since the last line)"
            )
        );
    }
}

#[cfg(test)]
mod lifetime_tests {
    use super::*;
    #[test]
    fn private_continuous_option_only_removes_finite_worker_ceiling() {
        assert!(check_runtime(1, MAX_RUNTIME_US + 1, false).is_err());
        for at in [MAX_RUNTIME_US + 1, 86_400_000_001, 604_800_000_001] {
            assert!(check_runtime(1, at, true).is_ok());
        }
        assert!(check_runtime(2, 1, true).is_err());
        let late = 700_000_000;
        assert!(check_runtime(1, late, true).is_ok());
        assert!(!check_authority(1, late, Some(late + 100_000)).unwrap());
        // An installed authority that ran out holds the ring dark, and never
        // ends it (#1178); none at all past the start's allowance does.
        assert!(check_authority(1, late, Some(late)).unwrap());
        assert!(check_authority(1, late, None).is_err());
        assert!(check_authority(1, INITIAL_AUTHORITY_US + 1, None).is_err());
        assert!(!check_authority(1, INITIAL_AUTHORITY_US, None).unwrap());
    }
}

#[cfg(test)]
mod shutdown_warning_tests {
    use super::*;
    use lamp_controls::{input::Level, touch::*};
    use lamp_msgs::{ClockStamp, TouchSource, TouchState};

    #[test]
    fn the_warning_shows_only_while_the_button_is_held_in_the_shutdown_window() {
        for (held, shows) in [
            (None, false),
            (Some(1_000_000), false),
            (Some(3_000_000), false),
            (Some(4_999_999), false),
            (Some(5_000_000), true),
            (Some(6_000_000), true),
            (Some(15_000_000), true),
            (Some(15_000_001), false),
            (Some(20_000_000), false),
        ] {
            assert_eq!(warning(held).is_some(), shows, "{held:?}");
        }
        // The setup light's amber, the whole ring, no brighter than it.
        let amber = shutdown_warning();
        assert!(
            amber
                .iter()
                .all(|pixel| *pixel == amber[0] && pixel.red == 24 && pixel.blue == 0),
            "{:?}",
            amber[0]
        );
    }

    /// The ring reads the primary button's time from Controls' own touch
    /// state, as Controls' touch engine says it while the button is held.
    #[test]
    fn the_primary_buttons_time_is_read_from_controls_touch_state() {
        let mut engine = TouchEngine::new(
            TouchConfig {
                enabled: true,
                primary: Some("primary99".into()),
                ..TouchConfig::default()
            },
            "boot",
            "controls",
        )
        .unwrap();
        let mut state = None;
        for (sequence, at) in (0..=6_000_000u64).step_by(50_000).enumerate() {
            let sample = GpioSample {
                stamp: ClockStamp {
                    clock_id: "boot".into(),
                    at_us: at,
                    sequence: sequence as u64 + 1,
                },
                session_id: "gpio".into(),
                primary: Some(at > 0),
                reset: None,
                head_pads: Vec::<Level>::new(),
            };
            engine.ingest_gpio(at, sample).unwrap();
            state = Some(engine.tick(at).state);
        }
        let state: TouchState = state.unwrap();
        let root = std::path::PathBuf::from(format!("/tmp/lrw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut said = bus
            .publisher_declared(TouchState::TOPIC, &TouchState::declaration(), "controls")
            .unwrap();
        said.publish_json(1, &state).unwrap();
        let message = bus.latest(TouchState::TOPIC).unwrap().unwrap();
        let held = crate::ring_head::held_of(&message, TouchSource::Primary).map(|(held, _)| held);
        assert_eq!(held, Some(5_950_000));
        assert!(warning(held).is_some());
        assert_eq!(crate::ring_head::held_of(&message, TouchSource::Head), None);
        drop(said);
        std::fs::remove_dir_all(root).unwrap();
    }
}
