//! Opt-in replacement for the exclusive GPIO worker. Controls reports the
//! switch; this Voice child combines it with its controller's fresh permission.
//! On a lamp with no slide (`no-slide`) Controls says so instead, and the
//! controller (the software mute) alone decides.
//! Bus/disk work stays in this process, outside the interaction owner's loop.
use crate::{
    privacy::{PRIVACY_FRESH_US, PrivacyGate},
    privacy_gap::RIDE_US,
    process::boot_text,
    transport::WorkerChannels,
    wire::{Control, WorkerEvent},
};
use lamp_bus::{Bus, Kind, Message, Notice, Subscriber};
use lamp_msgs::{ClockStamp, ControlsState, MicrophonePrivacy, PrivacySwitch};
use lamp_turn_owner::{BootId, BoundaryGuard, MonoTime, Permission, Snapshot};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const ENVIRONMENT: &str = "LAMP_PRIVACY_CONTROLS";
pub const ROOT_FLAG: &str = "--controls-root";
/// The privacy worker's argument for a lamp with no slide. Its environment
/// is cleared (`worker_command`), so the route reaches it only this way.
pub const NO_SLIDE_FLAG: &str = "--no-slide";
/// Only the conversation worker rides expiry; the muted wait keeps its
/// existing restart behavior. The child's environment is cleared.
pub const GAP_RIDE_FLAG: &str = "--gap-ride";
const DRAIN_LIMIT: usize = 32;
#[cfg(any(target_os = "linux", test))]
pub(crate) const INDICATOR_SUBSCRIBER: &str = "privacy_ring";

fn switch_subscriber(bus: &Bus) -> io::Result<Subscriber> {
    bus.subscribe_typed(ControlsState::TOPIC, "voice_privacy", ControlsState::TYPE)
}

fn invalid(why: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.into())
}

/// Which privacy route `LAMP_PRIVACY_CONTROLS` selects. Controls, Voice and
/// Ears read the same value; any other is off, as builds before `no-slide`
/// read that value, so a rollback is the route off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// `on`: Controls reads the slide; a lease needs it read open and the
    /// controller's permission.
    Slide,
    /// `no-slide`: the lamp has no slide and Controls says so in each
    /// `controls/state`; a lease needs that publication fresh and the
    /// controller's permission, so the software mute alone closes it.
    NoSlide,
}

impl Route {
    pub fn from_value(switch: Option<&str>) -> Option<Self> {
        match switch {
            Some("on") => Some(Self::Slide),
            Some("no-slide") => Some(Self::NoSlide),
            _ => None,
        }
    }
}

/// The selected route and its Bus root, or None with the route off.
pub fn selected(switch: Option<&str>, root: Option<&str>) -> io::Result<Option<(PathBuf, Route)>> {
    let Some(route) = Route::from_value(switch) else {
        return Ok(None);
    };
    let root = PathBuf::from(root.unwrap_or("/run/lampos-bus"));
    if !root.is_absolute() {
        return Err(invalid("privacy Bus root must be absolute"));
    }
    Ok(Some((root, route)))
}

pub fn selected_root(switch: Option<&str>, root: Option<&str>) -> io::Result<Option<PathBuf>> {
    Ok(selected(switch, root)?.map(|(root, _)| root))
}

pub fn from_environment() -> io::Result<Option<(PathBuf, Route)>> {
    selected(
        std::env::var(ENVIRONMENT).ok().as_deref(),
        std::env::var("LAMP_BUS_ROOT").ok().as_deref(),
    )
}

/// On a lamp with no slide the software mute is the only thing that closes
/// the microphones, so the voice needs its persisted record
/// (`--controls-mute`): without it a restart would forget a mute. Refused
/// before anything starts.
pub fn refuse_no_slide_without_mute(route: Option<Route>, mute_record: bool) -> io::Result<()> {
    if route == Some(Route::NoSlide) && !mute_record {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LAMP_PRIVACY_CONTROLS=no-slide needs --controls-mute: with no slide the software mute alone closes the microphones, and without its record a restart would forget it",
        ));
    }
    Ok(())
}

/// The privacy worker's arguments after its role. Its environment is
/// cleared, so the route goes as [`NO_SLIDE_FLAG`].
pub fn worker_arguments(selected: Option<(&Path, Route)>) -> io::Result<Vec<String>> {
    match selected {
        None => Ok(Vec::new()),
        Some((root, route)) if root.is_absolute() => {
            let mut arguments = vec![
                ROOT_FLAG.into(),
                root.to_str()
                    .ok_or_else(|| invalid("privacy root must be UTF-8"))?
                    .into(),
            ];
            if route == Route::NoSlide {
                arguments.push(NO_SLIDE_FLAG.into());
            }
            Ok(arguments)
        }
        Some(_) => Err(invalid("privacy root must be absolute")),
    }
}

/// The conversation's selected worker, with its explicitly passed ride setting.
/// The direct GPIO route and the muted wait never receive this flag.
pub fn conversation_arguments(
    selected: Option<(&Path, Route)>,
    ride: bool,
) -> io::Result<Vec<String>> {
    let mut arguments = worker_arguments(selected)?;
    if ride && !arguments.is_empty() {
        arguments.push(GAP_RIDE_FLAG.into());
    }
    Ok(arguments)
}

/// What the privacy worker reads back from [`worker_arguments`]: exactly
/// `--controls-root ROOT`, then `--no-slide` on a lamp with no slide, then
/// the conversation's optional `--gap-ride`.
pub fn worker_route(arguments: &[String]) -> io::Result<(PathBuf, Route, bool)> {
    let (arguments, ride) = match arguments.split_last() {
        Some((flag, arguments)) if flag == GAP_RIDE_FLAG => (arguments, true),
        _ => (arguments, false),
    };
    let (root, route) = match arguments {
        [flag, root] if flag == ROOT_FLAG => (root, Route::Slide),
        [flag, root, mode] if flag == ROOT_FLAG && mode == NO_SLIDE_FLAG => (root, Route::NoSlide),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "privacy worker takes --controls-root ROOT [--no-slide] [--gap-ride]",
            ));
        }
    };
    let root = PathBuf::from(root);
    if !root.is_absolute() {
        return Err(io::Error::other("privacy root must be absolute"));
    }
    Ok((root, route, ride))
}

pub fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis().min(u128::from(u64::MAX)) as u64)
}

/// One accepted observation for the gates and the lease.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Observation {
    /// The slide's acquisition, with its level.
    Slide(PrivacySwitch),
    /// On a lamp with no slide, the Controls publication saying so: its own
    /// stamp, read as not muted, with no physical level. Only the
    /// controller (the software mute) closes the lease then.
    NoSlide(ClockStamp),
}

impl Observation {
    /// The acquisition's sequence and time, or the publication's.
    pub fn stamp(&self) -> &ClockStamp {
        match self {
            Self::Slide(value) => &value.stamp,
            Self::NoSlide(stamp) => stamp,
        }
    }

    /// What the gates take as muted: the slide's level, which an accepted
    /// slide observation always has, or not muted with no slide.
    pub fn muted(&self) -> io::Result<bool> {
        match self {
            Self::Slide(value) => value.muted.ok_or_else(|| invalid("missing switch level")),
            Self::NoSlide(_) => Ok(false),
        }
    }

    /// The physical level for the lease: never inferred with no slide.
    pub fn physical_muted(&self) -> Option<bool> {
        match self {
            Self::Slide(value) => value.muted,
            Self::NoSlide(_) => None,
        }
    }
}

/// A restart or discontinuity ends this worker, so the supervisor must close
/// and verify cleanup before a fresh worker can learn another producer. Its
/// kind (a slide, or none) is set when it is made and never changes.
pub struct SwitchFence {
    clock: String,
    no_slide: bool,
    session: Option<String>,
    header_sequence: u64,
    publication: Option<ClockStamp>,
    last: Option<PrivacySwitch>,
    /// The next message resyncs the order rather than continues it
    /// ([`SwitchFence::resync`]).
    resync: bool,
}

impl SwitchFence {
    /// For `on`: each state must carry the slide's evidence.
    pub fn new(clock: String) -> Self {
        Self {
            clock,
            no_slide: false,
            session: None,
            header_sequence: 0,
            publication: None,
            last: None,
            resync: false,
        }
    }

    /// Takes the next message as a resync, not a continuation: a gap before it,
    /// or since the last message taken, is not a discontinuity. For what the
    /// worker reads before its first datagram, that one included, in its first
    /// second ([`resyncs`]). Until Controls' publisher finds the worker's new
    /// socket (every 5 ms, `selected_bus`), the worker reads the topic's kept
    /// value, and a state published in between reaches only that file. Controls
    /// sends the switch's whole state each time, so the newest is the truth (the
    /// first builder on #1722, 20 Oct 14:42; #1569 row 45: `gap 1 at seq 5` in 4
    /// of a probing build's 5 failures under load). The publisher, its clock and
    /// the order going forward are checked as ever, and from the second datagram
    /// on a gap fails closed.
    pub fn resync(&mut self) {
        self.resync = true;
    }

    /// For `no-slide`: each state must say no slide is wired and carry no
    /// slide evidence.
    pub fn no_slide(clock: String) -> Self {
        Self {
            no_slide: true,
            ..Self::new(clock)
        }
    }

    pub fn for_route(clock: String, route: Route) -> Self {
        match route {
            Route::Slide => Self::new(clock),
            Route::NoSlide => Self::no_slide(clock),
        }
    }

    /// The next accepted observation of this fence's kind, if new.
    pub fn observe(
        &mut self,
        message: &Message,
        now_us: u64,
        unix_ms: u64,
    ) -> io::Result<Option<Observation>> {
        if self.no_slide {
            self.accept_no_slide(message, now_us, unix_ms)
                .map(|stamp| Some(Observation::NoSlide(stamp)))
        } else {
            Ok(self
                .accept(message, now_us, unix_ms)?
                .map(Observation::Slide))
        }
    }

    /// A slide fence's next new acquisition. A state saying no slide is
    /// wired has none, and is refused.
    pub fn accept(
        &mut self,
        message: &Message,
        now_us: u64,
        unix_ms: u64,
    ) -> io::Result<Option<PrivacySwitch>> {
        if self.no_slide {
            return Err(invalid("a no-slide privacy fence reads no slide"));
        }
        let (h, state) = self.publication(message, now_us, unix_ms)?;
        if state.no_privacy_slide {
            return Err(invalid(
                "Controls says no slide is wired, but LAMP_PRIVACY_CONTROLS=on reads one",
            ));
        }
        let value = state
            .privacy
            .ok_or_else(|| invalid("Controls has no selected privacy source"))?;
        value.check().map_err(io::Error::other)?;
        if value.stamp.clock_id != self.clock
            || value.stamp.at_us > now_us
            || now_us - value.stamp.at_us >= PRIVACY_FRESH_US
            || value.muted.is_none()
            || value.fault.is_some()
        {
            return Err(invalid("physical privacy acquisition unavailable"));
        }
        let changed = match &self.last {
            Some(old)
                if old.reader_session_id != value.reader_session_id
                    || value.stamp.at_us < old.stamp.at_us
                    || value.stamp.sequence < old.stamp.sequence =>
            {
                return Err(invalid("privacy reader restarted or acquisition regressed"));
            }
            Some(old) if value.stamp.sequence == old.stamp.sequence => {
                if old != &value {
                    return Err(invalid("privacy acquisition identity changed"));
                }
                false
            }
            _ => true,
        };
        self.session = Some(state.session_id);
        self.header_sequence = h;
        self.publication = Some(state.stamp);
        self.last = Some(value.clone());
        self.resync = false;
        Ok(changed.then_some(value))
    }

    /// A no-slide fence's next publication: it must say no slide is wired
    /// and carry no slide evidence. With every envelope, session, order and
    /// freshness check a slide's state has, the publication itself is the
    /// observation, and each accepted one is new.
    pub fn accept_no_slide(
        &mut self,
        message: &Message,
        now_us: u64,
        unix_ms: u64,
    ) -> io::Result<ClockStamp> {
        if !self.no_slide {
            return Err(invalid("a slide privacy fence reads the slide"));
        }
        let (h, state) = self.publication(message, now_us, unix_ms)?;
        if !state.no_privacy_slide || state.privacy.is_some() {
            return Err(invalid(
                "Controls does not say this lamp has no slide (LAMP_PRIVACY_CONTROLS=no-slide)",
            ));
        }
        self.session = Some(state.session_id);
        self.header_sequence = h;
        self.publication = Some(state.stamp.clone());
        self.resync = false;
        Ok(state.stamp)
    }

    /// A Controls state whose envelope, publisher, clock, order and freshness
    /// all hold, with its Bus sequence; nothing is kept until the caller's
    /// own checks pass.
    fn publication(
        &self,
        message: &Message,
        now_us: u64,
        unix_ms: u64,
    ) -> io::Result<(u64, ControlsState)> {
        let h = &message.header;
        if h.topic != ControlsState::TOPIC
            || h.source != "controls"
            || h.kind != Kind::Latest
            || h.type_name.as_deref() != Some(ControlsState::TYPE)
            || h.enc != lamp_bus::JSON
            || h.seq == 0
            || h.expires_ms != Some(300)
            || h.boot.len() != 16
            || !h.boot.bytes().all(|b| b.is_ascii_hexdigit())
            || message.expired
            || h.expired_at(unix_ms)
            || h.at_unix_ms > unix_ms.saturating_add(lamp_bus::EXPIRY_GRACE_MS)
            || (message.gap != 0 && !self.resync)
            || message.publisher_restarted
        {
            return Err(invalid(
                "invalid or discontinuous privacy Controls envelope",
            ));
        }
        let state: ControlsState = message.parse()?;
        state.check().map_err(io::Error::other)?;
        if state.session_id != h.boot
            || state.stamp.clock_id != self.clock
            || state.stamp.at_us != h.at_us
            || state.stamp.at_us > now_us
            || now_us - state.stamp.at_us >= PRIVACY_FRESH_US
            || self
                .session
                .as_ref()
                .is_some_and(|session| *session != state.session_id)
            || h.seq <= self.header_sequence
            || (!self.resync
                && self.header_sequence != 0
                && self.header_sequence.checked_add(1) != Some(h.seq))
            || self.publication.as_ref().is_some_and(|old| {
                state.stamp.sequence <= old.sequence || state.stamp.at_us < old.at_us
            })
        {
            return Err(invalid("privacy Controls publisher/clock/order changed"));
        }
        Ok((h.seq, state))
    }
}

fn counters(subscriber: &Subscriber) -> (u64, u64, u64, u64) {
    (
        subscriber.malformed(),
        subscriber.expired(),
        subscriber.wrong_type(),
        subscriber.repeated(),
    )
}

/// A LOW observation retires this child's permission to Ears. A slow Bus drain
/// cannot combine LOW, later stable HIGH and an old Allowed controller into a
/// new grant. The main owner still receives every HIGH for its quiet wait;
/// reopening capture requires verified cleanup and a fresh worker.
#[derive(Default)]
pub struct CaptureLeaseGate {
    physical: PrivacyGate,
    mute_seen: bool,
}

impl CaptureLeaseGate {
    pub fn observe(&mut self, sequence: u64, acquired_us: u64, now_us: u64, muted: bool) {
        self.mute_seen |= muted;
        self.physical.observe(
            sequence,
            MonoTime::from_micros(acquired_us),
            MonoTime::from_micros(now_us),
            muted,
        );
    }

    pub fn permitted(&self, state: lamp_turn_owner::Snapshot, now_us: u64) -> bool {
        !self.mute_seen
            && self.physical.allowed(MonoTime::from_micros(now_us))
            && state.issued_at().as_micros() <= now_us
            && now_us < state.expires_at().as_micros()
            && state.microphone_permission() == Permission::Allowed
    }
}

fn authority(
    channels: &mut WorkerChannels,
    guard: &mut BoundaryGuard,
    lapse: &mut DeniedLapse,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    for _ in 0..16 {
        match channels.control.receive()? {
            Some(Control::Stop) => return Ok(false),
            Some(Control::Authority { snapshot }) => {
                match guard.install(MonoTime::from_micros(lamp_ipc::monotonic_us()), snapshot) {
                    Ok(()) => {}
                    // Queued while the board was held up, it lapsed in the
                    // queue (#4332): a Denied-only one is skipped, as the
                    // provider workers skip theirs; any other still ends.
                    // The lease held, older still, lapsed first.
                    Err(lamp_turn_owner::Error::ExpiredState) if denied_only(snapshot) => {
                        lapse.began(snapshot.expires_at().as_micros());
                        if let Some(held) = guard.state() {
                            lapse.began(held.expires_at().as_micros());
                        }
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            Some(_) => return Err(invalid("invalid Controls privacy worker command").into()),
            None => return Ok(true),
        }
    }
    Err(invalid("privacy authority drain overflow").into())
}

/// The longest a lapsed Denied-only authority is ridden (#4332): capture's
/// own bound on a closed microphone, as the speaker's lapse.
pub(crate) const DENIED_LAPSE_MOST_US: u64 = 1_500_000;
/// With no authority yet, how long after its start the worker waits for its
/// first, as it always has.
const FIRST_AUTHORITY_US: u64 = 250_000;

/// Whether `snapshot` grants nothing: the microphones and the camera both
/// Denied, and no turn. Only such a lease's lapse is ridden (#4332).
pub fn denied_only(snapshot: Snapshot) -> bool {
    snapshot.microphone_permission() == Permission::Denied
        && snapshot.camera_permission() == Permission::Denied
        && snapshot.owner().is_none()
}

/// What the worker's authority is at a look ([`DeniedLapse::look`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Held {
    /// A current authority: the worker publishes from it.
    Current,
    /// None current, and none owed yet, or a Denied-only one lapsed and is
    /// ridden: nothing is published, so the microphones stay closed.
    Waiting,
    /// A current authority again after a ridden lapse of this many ms.
    Rearmed(u64),
}

/// A lapse of the worker's Denied-only authority, ridden (#4332). On
/// lamp-06c1's cold boot the whole board stalled ~0.9 s on memory; the
/// wait's Denied authority lapsed in the worker's queue and the worker
/// ended on "authority snapshot has expired", and the voice with it, muted
/// by the restart guard. Such a lease grants nothing, so its lapse only
/// waits for the next with nothing published, at most
/// [`DENIED_LAPSE_MOST_US`] from its expiry. A lapse of a lease that grants
/// anything, no authority ever, or one past that bound still ends the
/// worker as before; the switch's own reading (`SourceGap`, the publisher's
/// notices) is checked on every look as before, ride or not.
#[derive(Debug, Default)]
pub(crate) struct DeniedLapse {
    since_us: Option<u64>,
}

impl DeniedLapse {
    /// A lease lapsed at `expired_us`: the lapse began then, unless an
    /// earlier one did. For a Denied-only authority that came already
    /// lapsed, and the lease held before it.
    pub(crate) fn began(&mut self, expired_us: u64) {
        self.since_us = Some(
            self.since_us
                .map_or(expired_us, |since| since.min(expired_us)),
        );
    }

    /// The authority the worker holds, `state`, at `now_us`, the worker
    /// having begun at `began_us`.
    pub(crate) fn look(
        &mut self,
        state: Option<Snapshot>,
        began_us: u64,
        now_us: u64,
    ) -> Result<Held, Box<dyn std::error::Error + Send + Sync>> {
        let expired = || invalid("privacy controller lease expired").into();
        match state {
            Some(state) if now_us < state.expires_at().as_micros() => {
                Ok(match self.since_us.take() {
                    Some(since) => Held::Rearmed(now_us.saturating_sub(since) / 1_000),
                    None => Held::Current,
                })
            }
            Some(state) if !denied_only(state) => Err(expired()),
            Some(state) => {
                self.began(state.expires_at().as_micros());
                self.riding(now_us).map_err(|()| expired())
            }
            None if self.since_us.is_some() => self.riding(now_us).map_err(|()| expired()),
            None if now_us.saturating_sub(began_us) >= FIRST_AUTHORITY_US => Err(expired()),
            None => Ok(Held::Waiting),
        }
    }

    fn riding(&self, now_us: u64) -> Result<Held, ()> {
        match self.since_us {
            Some(since) if now_us.saturating_sub(since) < DENIED_LAPSE_MOST_US => Ok(Held::Waiting),
            _ => Err(()),
        }
    }
}

/// One look at the authority the worker holds ([`DeniedLapse::look`]),
/// saying a re-arm once, at whichever look finds it.
fn look(
    lapse: &mut DeniedLapse,
    state: Option<Snapshot>,
    began_us: u64,
    now_us: u64,
) -> Result<Held, Box<dyn std::error::Error + Send + Sync>> {
    let held = lapse.look(state, began_us, now_us)?;
    if let Held::Rearmed(lapsed_ms) = held {
        eprintln!("{}", rearmed_line(lapsed_ms));
    }
    Ok(held)
}

/// The authority a pass publishes from, looked at after its Bus I/O at
/// `now_us`, the lease's own stamp. The pass's first look came before that
/// I/O, and a board stalled in it may have lapsed the lease since (#4332):
/// a Denied-only one is then ridden, nothing published, under the same
/// bound; one that grants anything ends the worker, as at the first look.
/// Published from the first look, the lease would carry the lapsed expiry
/// and fail its own check, ending the voice.
fn publishing(
    lapse: &mut DeniedLapse,
    state: Option<Snapshot>,
    began_us: u64,
    now_us: u64,
) -> Result<Option<Snapshot>, Box<dyn std::error::Error + Send + Sync>> {
    Ok(match look(lapse, state, began_us, now_us)? {
        Held::Waiting => None,
        Held::Current | Held::Rearmed(_) => state,
    })
}

/// The journal's line for a ridden lapse, once a fresh authority came.
pub(crate) fn rearmed_line(lapsed_ms: u64) -> String {
    format!(
        "lamp-voice: privacy worker re-armed its denied lease after {lapsed_ms} ms lapsed (microphones and camera stayed closed)"
    )
}

fn notices(subscriber: &mut Subscriber) -> io::Result<()> {
    if subscriber
        .notices()
        .iter()
        .any(|notice| matches!(notice, Notice::SourceGone | Notice::Late))
    {
        return Err(invalid("privacy publisher unavailable"));
    }
    Ok(())
}

/// Reader for the isolated muted-ring worker. It never learns a replacement
/// publisher in place; an invalid stream removes its indication and exits.
pub struct LeaseReader {
    subscriber: Subscriber,
    clock: String,
    current: Option<MicrophonePrivacy>,
    last_now: Option<u64>,
    /// When it was opened: its reads before the first datagram, in its first
    /// second, may follow a gap ([`resyncs`]), as the privacy worker's.
    opened_us: u64,
}

impl LeaseReader {
    pub fn open(root: &Path, name: &str) -> io::Result<Self> {
        Self::with_clock(root, name, lamp_controls::runtime::clock_id(None)?)
    }

    pub fn with_clock(root: &Path, name: &str, clock: String) -> io::Result<Self> {
        let bus = lamp_controls::privacy::selected_bus(Bus::new(root)?);
        Ok(Self {
            subscriber: bus.subscribe_typed(
                MicrophonePrivacy::TOPIC,
                name,
                MicrophonePrivacy::TYPE,
            )?,
            clock,
            current: None,
            last_now: None,
            opened_us: lamp_ipc::monotonic_us(),
        })
    }

    /// Until the first datagram, in its first second, a value read may follow
    /// a gap and skip sequence numbers, as the privacy worker's fence does
    /// (#1569 row 45, Third's note on #2244): before the voice's publisher
    /// finds this reader's socket it reads the topic's kept value, and a
    /// lease published in between reaches only that file. Each lease is
    /// whole, so the newest is the truth. Its session, controller, order,
    /// time and generation are checked as ever. Before, the first datagram's
    /// gap ended the reader, and the muted ring lost its red at the start.
    pub fn poll(&mut self) -> io::Result<Option<MicrophonePrivacy>> {
        let now_us = lamp_ipc::monotonic_us();
        if self.last_now.is_some_and(|last| now_us < last) {
            return Err(invalid("privacy reader clock regressed"));
        }
        self.last_now = Some(now_us);
        for count in 0..DRAIN_LIMIT {
            let before = counters(&self.subscriber);
            let resync = resyncs(
                self.subscriber.statistics().datagrams,
                self.opened_us,
                lamp_ipc::monotonic_us(),
            );
            let message = self.subscriber.recv(Duration::ZERO)?;
            let now_us = lamp_ipc::monotonic_us();
            if before != counters(&self.subscriber) {
                return Err(invalid("invalid privacy message discarded"));
            }
            let Some(message) = message else {
                notices(&mut self.subscriber)?;
                return Ok(self
                    .current
                    .as_ref()
                    .filter(|value| value.fresh_at(&self.clock, now_us))
                    .cloned());
            };
            let value = MicrophonePrivacy::from_message(&message, &self.clock, now_us, wall_ms())
                .map_err(io::Error::other)?;
            if (message.gap != 0 && !resync)
                || message.publisher_restarted
                || self.current.as_ref().is_some_and(|old| {
                    old.session_id != value.session_id
                        || old.controller_boot != value.controller_boot
                        // A backstop: `from_message` already ties it to the
                        // bus's own sequence.
                        || value.stamp.sequence <= old.stamp.sequence
                        || (!resync && value.stamp.sequence != old.stamp.sequence.saturating_add(1))
                        || value.stamp.at_us < old.stamp.at_us
                        || value.microphone_generation < old.microphone_generation
                })
            {
                return Err(invalid("privacy lease stream changed or lost a value"));
            }
            self.current = Some(value);
            if count == DRAIN_LIMIT - 1 {
                return Err(invalid("privacy lease drain overflow"));
            }
        }
        unreachable!()
    }
}

/// How often the lease watch reads the bus.
const WATCH_EVERY: Duration = Duration::from_millis(2);

/// A [`LeaseReader`] on a thread of its own (#795 review): one receive from
/// the bus passes over any number of messages it refuses, so the muted
/// ring's writer never reads the bus itself, between its Stop and expiry
/// checks. It takes what this last found, still fresh at that moment, and
/// a reader's failure once and for good. The thread is told to stop when
/// this is dropped and is not waited for.
pub struct LeaseWatch {
    found: Arc<Mutex<Result<Option<MicrophonePrivacy>, String>>>,
    stop: Arc<AtomicBool>,
    clock: String,
}

impl LeaseWatch {
    pub fn open(root: &Path, name: &str) -> io::Result<Self> {
        Self::start(LeaseReader::open(root, name)?)
    }

    pub fn start(mut reader: LeaseReader) -> io::Result<Self> {
        let found = Arc::new(Mutex::new(Ok(None)));
        let stop = Arc::new(AtomicBool::new(false));
        let clock = reader.clock.clone();
        let (writing, stopping) = (Arc::clone(&found), Arc::clone(&stop));
        thread::Builder::new()
            .name("voice-privacy-lease".into())
            .spawn(move || {
                while !stopping.load(Ordering::Acquire) {
                    let polled = reader.poll().map_err(|error| error.to_string());
                    let failed = polled.is_err();
                    if let Ok(mut slot) = writing.lock() {
                        *slot = polled;
                    }
                    if failed {
                        return;
                    }
                    thread::sleep(WATCH_EVERY);
                }
            })?;
        Ok(Self { found, stop, clock })
    }

    /// The lease the reader last found, if it is still fresh now, or the
    /// reader's failure. It reads no bus and never waits on one.
    pub fn latest(&self) -> io::Result<Option<MicrophonePrivacy>> {
        let now_us = lamp_ipc::monotonic_us();
        let found = self
            .found
            .lock()
            .map_err(|_| invalid("privacy lease watch failed"))?;
        match &*found {
            Err(why) => Err(invalid(why.clone())),
            Ok(lease) => Ok(lease
                .as_ref()
                .filter(|value| value.fresh_at(&self.clock, now_us))
                .cloned()),
        }
    }
}

impl Drop for LeaseWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

/// The switch's evidence could not be had or used: none, a fault, stale,
/// broken or discontinuous. Not a crash: the worker says so to the voice
/// (`WorkerEvent::Fault`, [`UNREADABLE`]) and ends cleanly, so the voice
/// keeps the microphones closed and reads again with a fresh worker (#795).
#[derive(Debug)]
struct Unreadable(String);
impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Unreadable {}

/// How the worker's fault code begins when the switch could not be read.
pub const UNREADABLE: &str = "privacy switch unreadable: ";

fn unreadable(why: impl std::fmt::Display) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(Unreadable(why.to_string()))
}

/// The worker's bounded expiry episode. A delayed observation never extends
/// it: the bound begins when the last acquisition actually expired, and only
/// a stable open gate (or a physical mute) finishes it. Used by the real bus
/// loop and by the coordinator's stepped worker fixture.
pub(crate) struct SourceGap {
    on: bool,
    since: Option<u64>,
}

impl SourceGap {
    pub(crate) fn new(on: bool) -> Self {
        Self { on, since: None }
    }

    pub(crate) fn active(&self) -> bool {
        self.since.is_some()
    }

    pub(crate) fn check(
        &self,
        now_us: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self
            .since
            .is_some_and(|since| now_us.saturating_sub(since) >= RIDE_US)
        {
            return Err(unreadable("physical privacy source expired"));
        }
        Ok(())
    }

    /// Recovery may already be queued when the worker next runs. Report
    /// the old acquisition's expiry before replacing it with that reading.
    pub(crate) fn before_observation(
        &mut self,
        last_us: Option<u64>,
        permitted: bool,
        now_us: u64,
    ) -> Result<Option<WorkerEvent>, Box<dyn std::error::Error + Send + Sync>> {
        self.check(now_us)?;
        if self.on
            && permitted
            && let Some(last_us) = last_us
            && now_us.saturating_sub(last_us) >= PRIVACY_FRESH_US
        {
            return self.missing(last_us.saturating_add(PRIVACY_FRESH_US), permitted, now_us);
        }
        Ok(None)
    }

    pub(crate) fn missing(
        &mut self,
        since_us: u64,
        permitted: bool,
        now_us: u64,
    ) -> Result<Option<WorkerEvent>, Box<dyn std::error::Error + Send + Sync>> {
        if !self.active() {
            if !self.on || !permitted {
                return Err(unreadable("physical privacy source expired"));
            }
            self.since = Some(since_us);
            self.check(now_us)?;
            return Ok(Some(WorkerEvent::PrivacyUnavailable { since_us }));
        }
        self.check(now_us)?;
        Ok(None)
    }

    pub(crate) fn recovered(&mut self) {
        self.since = None;
    }
}

/// The only publisher of `voice/privacy`. A bus stall cannot renew its lease;
/// Voice and Ears independently expire their final-boundary permissions.
/// Source expiry can ride one bounded episode after a grant. Otherwise,
/// evidence that cannot be had ends it cleanly, said as [`UNREADABLE`];
/// anything else ends it as an error.
pub fn run(
    mut channels: WorkerChannels,
    controller: BootId,
    root: &Path,
    route: Route,
    ride: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match serve(&mut channels, controller, root, route, ride) {
        Err(error) if error.is::<Unreadable>() => {
            channels.control.send(WorkerEvent::Fault {
                code: format!("{UNREADABLE}{error}"),
            })?;
            Ok(())
        }
        other => other,
    }
}

/// The last accepted observation if it is still fresh at `now_us`: younger
/// than 50 ms, a slide's acquisition or, with no slide, the publication.
pub fn fresh(last: Option<&Observation>, now_us: u64) -> Option<&Observation> {
    last.filter(|observed| {
        now_us
            .checked_sub(observed.stamp().at_us)
            .is_some_and(|age| age < PRIVACY_FRESH_US)
    })
}

/// Whether the worker grants capture now: its queue fully drained, a fresh
/// observation, and the capture gate with the controller's permission. The
/// same with a slide or with none; with none, only the controller (the
/// software mute) can deny it.
pub fn grants(
    gate: &CaptureLeaseGate,
    state: Snapshot,
    observed: Option<&Observation>,
    drained: bool,
    now_us: u64,
) -> bool {
    drained && observed.is_some() && gate.permitted(state, now_us)
}

/// The lease published at `stamp`, from the controller's state and the last
/// observation still fresh. It expires by the controller's deadline, 50 ms
/// after `stamp`, and 50 ms after that observation. On a lamp with no slide
/// the observation is the Controls publication saying so: the lease says
/// `no_slide`, and its physical level stays unknown.
pub fn lease(
    stamp: ClockStamp,
    session_id: &str,
    controller: BootId,
    state: Snapshot,
    observed: Option<&Observation>,
    permitted: bool,
) -> MicrophonePrivacy {
    let observed_us = observed.map_or(0, |value| value.stamp().at_us);
    let mut expires_at_us = state
        .expires_at()
        .as_micros()
        .min(stamp.at_us.saturating_add(PRIVACY_FRESH_US));
    // An accepted slide observation always has its level, so this is the
    // slide's own bound, and with no slide the publication's.
    if observed.is_some() {
        expires_at_us = expires_at_us.min(observed_us.saturating_add(PRIVACY_FRESH_US));
    }
    MicrophonePrivacy {
        stamp,
        session_id: session_id.to_owned(),
        controller_boot: boot_text(controller),
        microphone_generation: state.microphone_generation(),
        observed_us,
        expires_at_us,
        permitted,
        physical_muted: observed.and_then(Observation::physical_muted),
        no_slide: matches!(observed, Some(Observation::NoSlide(_))),
    }
}

/// The receipt onboarding's privacy step takes: proof of the physical switch,
/// so only a slide's observation gives one. A lamp with no slide has no
/// reading to give (#1802's step is then not available; #2086's no-slide).
fn receipt(
    observation: &Observation,
    controls_session: &str,
) -> Option<crate::wire::PrivacyReceipt> {
    match observation {
        Observation::Slide(physical) => Some(crate::wire::PrivacyReceipt {
            controls_session: controls_session.to_owned(),
            physical: physical.clone(),
        }),
        Observation::NoSlide(_) => None,
    }
}

/// How long after the worker starts its reads may still resync the fence's
/// order: its first second (the first builder on #2244). Controls'
/// publisher finds a new subscriber within 5 ms (`selected_bus`), and with
/// no datagram for twice `controls/state`'s declared 100 ms the bus says
/// its publisher is late and the worker ends, except during an enabled
/// source-expiry ride bounded independently by [`RIDE_US`].
const RESYNC_FOR_US: u64 = 1_000_000;

/// Whether the next message read resyncs the fence's order
/// ([`SwitchFence::resync`]): none has come as a datagram yet, and the
/// worker started under [`RESYNC_FOR_US`] ago. After either, a gap fails
/// closed as before, datagrams or not.
fn resyncs(datagrams: u64, began_us: u64, now_us: u64) -> bool {
    datagrams == 0 && now_us.saturating_sub(began_us) < RESYNC_FOR_US
}

fn serve(
    channels: &mut WorkerChannels,
    controller: BootId,
    root: &Path,
    route: Route,
    ride: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let clock = lamp_controls::runtime::clock_id(None)?;
    let bus = lamp_controls::privacy::selected_bus(Bus::new(root)?);
    let mut subscriber = switch_subscriber(&bus)?;
    let mut publisher = bus.publisher_declared(
        MicrophonePrivacy::TOPIC,
        &MicrophonePrivacy::declaration(),
        "voice",
    )?;
    let mut fence = SwitchFence::for_route(clock.clone(), route);
    let mut guard = BoundaryGuard::new(controller, MonoTime::from_micros(lamp_ipc::monotonic_us()));
    let mut gate = CaptureLeaseGate::default();
    let mut gap = SourceGap::new(ride);
    let mut last: Option<Observation> = None;
    let mut published_at = None;
    let mut previous_permission = None;
    let mut granted = false;
    let mut sequence = 0u64;
    let mut lapse = DeniedLapse::default();
    let began = lamp_ipc::monotonic_us();
    channels.control.send(WorkerEvent::Ready)?;
    loop {
        if !authority(channels, &mut guard, &mut lapse)? {
            return Ok(());
        }
        let now = lamp_ipc::monotonic_us();
        // Expiry wins even if a fresh reading is queued after the deadline.
        gap.check(now)?;
        // A lapsed Denied-only authority is ridden, nothing published, until
        // a fresh one (#4332); any other lapse ends the worker as before.
        look(&mut lapse, guard.state(), began, now)?;
        let mut empty = false;
        for _ in 0..DRAIN_LIMIT {
            let before = counters(&subscriber);
            // Until the first datagram, in the worker's first second, the
            // order begins again with each message ([`resyncs`]).
            let resync = resyncs(
                subscriber.statistics().datagrams,
                began,
                lamp_ipc::monotonic_us(),
            );
            let message = subscriber.recv(Duration::ZERO).map_err(unreadable)?;
            if before != counters(&subscriber) {
                return Err(unreadable("invalid Controls message discarded"));
            }
            let Some(message) = message else {
                empty = true;
                break;
            };
            let now = lamp_ipc::monotonic_us();
            if resync {
                fence.resync();
            }
            // With no slide each new publication is fed as not muted, so the
            // owner's gate, generation and Denied-while-muted rules run as
            // for a slide read open: only the controller closes it.
            if let Some(value) = fence
                .observe(&message, now, wall_ms())
                .map_err(unreadable)?
            {
                let muted = value.muted().map_err(unreadable)?;
                let acquired = value.stamp();
                let permitted = granted
                    && guard
                        .state()
                        .is_some_and(|state| state.microphone_permission() == Permission::Allowed);
                if let Some(event) = gap.before_observation(
                    last.as_ref().map(|value| value.stamp().at_us),
                    permitted,
                    now,
                )? {
                    gate.physical.close();
                    channels.control.send(event)?;
                }
                gate.observe(acquired.sequence, acquired.at_us, now, muted);
                // Revoke the main owner before any Bus publication/disk work.
                channels.control.send(WorkerEvent::Privacy {
                    acquired_at_us: acquired.at_us,
                    muted,
                    receipt: receipt(&value, &message.header.boot),
                })?;
                last = Some(value);
            }
        }
        if !empty {
            return Err(unreadable("Controls privacy drain overflow"));
        }
        if !ride {
            notices(&mut subscriber).map_err(unreadable)?;
        }
        // Revocation arriving during Bus I/O wins before another lease.
        if !authority(channels, &mut guard, &mut lapse)? {
            return Ok(());
        }
        let now = lamp_ipc::monotonic_us();
        gap.check(now)?;
        // Looked at again after the Bus I/O, at the stamp the lease takes: a
        // board stalled since the first look may have lapsed it (#4332).
        let publishing = publishing(&mut lapse, guard.state(), began, now)?;
        let physical = fresh(last.as_ref(), now);
        if physical.is_none() && now.saturating_sub(began) >= PRIVACY_FRESH_US {
            let since = last
                .as_ref()
                .map_or(began, |value| value.stamp().at_us)
                .saturating_add(PRIVACY_FRESH_US);
            let permitted = granted
                && guard
                    .state()
                    .is_some_and(|state| state.microphone_permission() == Permission::Allowed);
            if let Some(event) = gap.missing(since, permitted, now)? {
                gate.physical.close();
                // Say unavailable before publication/disk work. This survives
                // a delayed coordinator reading recovery in the same batch.
                channels.control.send(event)?;
            }
        }
        // Late can also accompany the first recovered datagram. Evaluate it
        // before clearing the episode; a changed publisher remains fatal.
        if ride
            && subscriber.notices().iter().any(|notice| {
                matches!(notice, Notice::SourceGone)
                    || matches!(notice, Notice::Late) && !gap.active()
            })
        {
            return Err(unreadable("privacy publisher unavailable"));
        }
        if physical.is_some()
            && matches!(
                gate.physical.state(MonoTime::from_micros(now)),
                crate::privacy::PrivacyState::Allowed | crate::privacy::PrivacyState::Muted
            )
        {
            gap.recovered();
        }
        if let Some(state) = publishing {
            let permitted = grants(&gate, state, physical, empty, now);
            let level = physical.and_then(Observation::physical_muted);
            let identity = (permitted, level, state.microphone_generation());
            if previous_permission != Some(identity)
                || published_at.is_none_or(|at| now.saturating_sub(at) >= 10_000)
            {
                sequence = sequence
                    .checked_add(1)
                    .ok_or_else(|| invalid("privacy publisher sequence exhausted"))?;
                let value = lease(
                    ClockStamp {
                        clock_id: clock.clone(),
                        at_us: now,
                        sequence,
                    },
                    publisher.boot(),
                    controller,
                    state,
                    physical,
                    permitted,
                );
                value.check()?;
                publisher.publish_json(now, &value)?;
                granted |= permitted;
                previous_permission = Some(identity);
                published_at = Some(now);
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// How often a lease-freshness test tries again when the host stalled
    /// past PRIVACY_FRESH_US (50 ms) before it could look: an attempt like
    /// that says nothing either way. Twenty, a pause after each, so the
    /// attempts span a busy host's stall of a few seconds rather than a
    /// quarter of one; a whole-lib run on a loaded Mac stalled five back to
    /// back (#1569, Reviewer-three's row, 30 Oct). A reader or watch that
    /// never finds the lease still fails every attempt. The two
    /// first-second resync tests take the same: with ten tries back to back,
    /// a whole-lib run on this Mac failed the kept-lease one in 2 of 2 runs
    /// (#1569, 30 Oct).
    const STALLED_ATTEMPTS: usize = 20;
    const AFTER_A_STALL: Duration = Duration::from_millis(100);
    use std::sync::atomic::AtomicU64;

    /// A controller of a new boot, its microphones and camera as given at
    /// `at_us`, as the voice's wait sets both Denied.
    fn controller(
        at_us: u64,
        microphone: Permission,
        camera: Permission,
    ) -> (lamp_turn_owner::Controller, BootId) {
        let boot = crate::process::new_boot().unwrap();
        let at = MonoTime::from_micros(at_us);
        let mut controller = lamp_turn_owner::Controller::new(boot, at);
        controller
            .set_microphone_permission(at, microphone)
            .unwrap();
        controller.set_camera_permission(at, camera).unwrap();
        (controller, boot)
    }

    /// lamp-06c1's cold boot, 30 Oct (#4332, fault A): one pass of the
    /// voice's privacy wait held 890.8 ms with the board on memory (5 major
    /// faults, 6 blocked, 1250 dirty pages), its worker's Denied lease
    /// lapsed, and the worker ended, and the voice with it. Now the lapse is
    /// ridden with nothing published and nothing granted, and the next
    /// authority re-arms it, said once, with how long it lapsed.
    #[test]
    fn a_denied_lease_lapsed_by_a_900_ms_stall_is_ridden_and_re_armed_once() {
        let began = 10_000_000;
        let (mut controller, boot) = controller(began, Permission::Denied, Permission::Denied);
        let mut guard = BoundaryGuard::new(boot, MonoTime::from_micros(began));
        let mut lapse = DeniedLapse::default();
        let gate = CaptureLeaseGate::default();
        let first = controller.snapshot(MonoTime::from_micros(began)).unwrap();
        assert!(denied_only(first));
        guard.install(MonoTime::from_micros(began), first).unwrap();
        assert_eq!(
            lapse.look(guard.state(), began, began + 1_000).unwrap(),
            Held::Current
        );
        // Nothing renewed for 900 ms: past the lease's 250 ms it is ridden.
        let mut rode = 0;
        for now in (began + 250_000..=began + 900_000).step_by(10_000) {
            let held = lapse.look(guard.state(), began, now).unwrap();
            assert_eq!(held, Held::Waiting, "at {now}");
            assert!(
                !gate.permitted(guard.state().unwrap(), now),
                "the microphones opened"
            );
            rode += 1;
        }
        assert!(rode > 60);
        let after = began + 900_000;
        let fresh = controller.snapshot(MonoTime::from_micros(after)).unwrap();
        assert!(denied_only(fresh));
        guard.install(MonoTime::from_micros(after), fresh).unwrap();
        let mut lines = Vec::new();
        for now in [after + 200, after + 1_000, after + 2_000] {
            match lapse.look(guard.state(), began, now).unwrap() {
                Held::Rearmed(lapsed_ms) => lines.push(rearmed_line(lapsed_ms)),
                held => assert_eq!(held, Held::Current),
            }
        }
        assert_eq!(
            lines,
            [
                "lamp-voice: privacy worker re-armed its denied lease after 650 ms lapsed (microphones and camera stayed closed)"
            ]
        );
        assert!(!gate.permitted(guard.state().unwrap(), after + 2_000));
    }

    /// The 06c1 journal itself (#4332): after the stall the worker's queue
    /// held the wait's Denied authority already lapsed, and its install
    /// ended the worker with "authority snapshot has expired", exit 1. The
    /// drain now skips it, and the lease held before it lapsed first; the
    /// next fresh one re-arms. One that grants anything still ends it.
    #[test]
    fn a_lapsed_denied_authority_in_the_queue_is_skipped_and_a_granting_one_still_ends() {
        let directory = crate::process::SessionDirectory::create().unwrap();
        let (mut coordinator, mut worker, _input) =
            crate::process::Worker::cooperative_fixture(&directory.path, "privacy");
        let stalled = lamp_ipc::monotonic_us().saturating_sub(1_200_000);
        let (mut denied, boot) = controller(stalled, Permission::Denied, Permission::Denied);
        let mut guard = BoundaryGuard::new(boot, MonoTime::from_micros(stalled));
        let mut lapse = DeniedLapse::default();
        let held = denied.snapshot(MonoTime::from_micros(stalled)).unwrap();
        guard.install(MonoTime::from_micros(stalled), held).unwrap();
        let queued = denied
            .snapshot(MonoTime::from_micros(stalled + 100_000))
            .unwrap();
        coordinator
            .channels
            .control
            .send(Control::Authority { snapshot: queued })
            .unwrap();
        let now = lamp_ipc::monotonic_us();
        assert!(authority(&mut worker, &mut guard, &mut lapse).unwrap());
        assert_eq!(guard.state(), Some(held), "a lapsed one is never installed");
        assert_eq!(lapse.since_us, Some(held.expires_at().as_micros()));
        assert_eq!(
            lapse.look(guard.state(), stalled, now).unwrap(),
            Held::Waiting
        );
        let fresh = denied
            .snapshot(MonoTime::from_micros(lamp_ipc::monotonic_us()))
            .unwrap();
        coordinator
            .channels
            .control
            .send(Control::Authority { snapshot: fresh })
            .unwrap();
        assert!(authority(&mut worker, &mut guard, &mut lapse).unwrap());
        assert!(matches!(
            lapse
                .look(guard.state(), stalled, lamp_ipc::monotonic_us())
                .unwrap(),
            Held::Rearmed(ms) if ms >= 900
        ));

        let (mut granting, boot) = controller(stalled, Permission::Allowed, Permission::Denied);
        let mut guard = BoundaryGuard::new(boot, MonoTime::from_micros(stalled));
        let queued = granting
            .snapshot(MonoTime::from_micros(stalled + 100_000))
            .unwrap();
        coordinator
            .channels
            .control
            .send(Control::Authority { snapshot: queued })
            .unwrap();
        let error = authority(&mut worker, &mut guard, &mut DeniedLapse::default()).unwrap_err();
        assert_eq!(error.to_string(), "authority snapshot has expired");
        let _ = coordinator.channels.control.send(Control::Stop);
        drop(_input);
        let _ = coordinator.shutdown();
        let _ = std::fs::remove_dir(&directory.path);
    }

    /// Only a lease that grants nothing is ridden (#4332): a lapse of one
    /// that grants the microphones or the camera, or leaves either unknown,
    /// ends the worker at its expiry as before, so do no authority within
    /// 250 ms of the start and a Denied lapse past its bound (a voice gone).
    #[test]
    fn a_lapse_of_a_lease_that_grants_anything_or_past_its_bound_still_ends_the_worker() {
        let began = 10_000_000;
        for (microphone, camera) in [
            (Permission::Allowed, Permission::Denied),
            (Permission::Denied, Permission::Allowed),
            (Permission::Unknown, Permission::Denied),
            (Permission::Denied, Permission::Unknown),
        ] {
            let (mut controller, _) = controller(began, microphone, camera);
            let state = controller.snapshot(MonoTime::from_micros(began)).unwrap();
            assert!(!denied_only(state));
            let mut lapse = DeniedLapse::default();
            assert_eq!(
                lapse.look(Some(state), began, began + 249_999).unwrap(),
                Held::Current
            );
            let error = lapse.look(Some(state), began, began + 250_000).unwrap_err();
            assert_eq!(error.to_string(), "privacy controller lease expired");
        }
        let mut lapse = DeniedLapse::default();
        assert_eq!(
            lapse.look(None, began, began + 249_999).unwrap(),
            Held::Waiting
        );
        assert!(lapse.look(None, began, began + 250_000).is_err());
        let (mut controller, _) = controller(began, Permission::Denied, Permission::Denied);
        let state = controller.snapshot(MonoTime::from_micros(began)).unwrap();
        let expired = state.expires_at().as_micros();
        let mut lapse = DeniedLapse::default();
        assert_eq!(
            lapse
                .look(Some(state), began, expired + DENIED_LAPSE_MOST_US - 1)
                .unwrap(),
            Held::Waiting
        );
        assert!(
            lapse
                .look(Some(state), began, expired + DENIED_LAPSE_MOST_US)
                .is_err()
        );
    }

    /// The pass's first look comes before its Bus I/O (Reviewer-three on
    /// #4647): a board stalled in that I/O lapses the Denied lease after the
    /// look said Current. Published from that look, the lease carried the
    /// lapsed expiry and failed its own check, "invalid microphone privacy
    /// lease", and the worker ended. The publish now looks again, rides the
    /// lapse with nothing published, and re-arms at the next authority.
    #[test]
    fn a_denied_lease_lapsed_between_the_two_looks_is_ridden_not_published() {
        let began = 10_000_000;
        let (mut controller, boot) = controller(began, Permission::Denied, Permission::Denied);
        let mut guard = BoundaryGuard::new(boot, MonoTime::from_micros(began));
        let mut lapse = DeniedLapse::default();
        let first = controller.snapshot(MonoTime::from_micros(began)).unwrap();
        guard.install(MonoTime::from_micros(began), first).unwrap();
        assert_eq!(
            look(&mut lapse, guard.state(), began, began + 1_000).unwrap(),
            Held::Current
        );
        // What the worker publishes from the pass's decision, checked as
        // the publish checks it.
        let publish = |state, at_us| {
            lease(
                ClockStamp {
                    clock_id: "test-clock".into(),
                    at_us,
                    sequence: 1,
                },
                "0123456789abcdef",
                boot,
                state,
                None,
                false,
            )
            .check()
        };
        // The Bus I/O held 900 ms: the lease lapsed after the first look.
        let now = began + 900_000;
        let published = publishing(&mut lapse, guard.state(), began, now).unwrap();
        if let Some(state) = published {
            publish(state, now).unwrap();
        }
        assert_eq!(published, None, "a lapsed lease was published");
        assert_eq!(
            look(&mut lapse, guard.state(), began, now + 2_000).unwrap(),
            Held::Waiting
        );
        let fresh = controller
            .snapshot(MonoTime::from_micros(now + 3_000))
            .unwrap();
        guard
            .install(MonoTime::from_micros(now + 3_000), fresh)
            .unwrap();
        assert_eq!(
            publishing(&mut lapse, guard.state(), began, now + 4_000).unwrap(),
            Some(fresh)
        );
        publish(fresh, now + 4_000).unwrap();
    }

    /// A lease that grants anything and lapses between the two looks still
    /// ends the worker, at the second look, as at the first.
    #[test]
    fn a_granting_lease_lapsed_between_the_two_looks_still_ends_the_worker() {
        let began = 10_000_000;
        for (microphone, camera) in [
            (Permission::Allowed, Permission::Denied),
            (Permission::Denied, Permission::Allowed),
        ] {
            let (mut controller, boot) = controller(began, microphone, camera);
            let mut guard = BoundaryGuard::new(boot, MonoTime::from_micros(began));
            let mut lapse = DeniedLapse::default();
            let state = controller.snapshot(MonoTime::from_micros(began)).unwrap();
            guard.install(MonoTime::from_micros(began), state).unwrap();
            assert_eq!(
                look(&mut lapse, guard.state(), began, began + 1_000).unwrap(),
                Held::Current
            );
            let error = publishing(&mut lapse, guard.state(), began, began + 900_000).unwrap_err();
            assert_eq!(error.to_string(), "privacy controller lease expired");
        }
    }

    #[test]
    fn source_expiry_is_ridden_only_when_enabled_after_permission() {
        for (on, permitted) in [(false, true), (true, false), (false, false)] {
            let error = SourceGap::new(on)
                .missing(100_000, permitted, 100_000)
                .unwrap_err();
            assert!(error.is::<Unreadable>());
        }
        let mut gap = SourceGap::new(true);
        assert!(matches!(
            gap.missing(100_000, true, 102_000).unwrap(),
            Some(WorkerEvent::PrivacyUnavailable { since_us: 100_000 })
        ));
        assert!(gap.missing(120_000, true, 122_000).unwrap().is_none());
        assert!(gap.check(100_000 + RIDE_US - 1).is_ok());
        assert!(gap.check(100_000 + RIDE_US).is_err());
        assert!(gap.check(100_000 + RIDE_US + 1).is_err());
        // A reader first scheduled after the entire budget cannot restart it.
        assert!(
            SourceGap::new(true)
                .missing(100_000, true, 100_000 + RIDE_US)
                .is_err()
        );
    }

    #[test]
    fn only_recovery_finishes_the_source_episode_and_allows_a_new_one() {
        let mut gap = SourceGap::new(true);
        gap.missing(100_000, true, 100_000).unwrap();
        // A fresh but not yet stable reading leaves the original deadline.
        gap.check(900_000).unwrap();
        assert!(gap.missing(950_000, true, 950_000).unwrap().is_none());
        assert!(gap.check(1_100_000).is_err());
        gap.recovered();
        assert!(!gap.active());
        assert!(matches!(
            gap.missing(2_000_000, true, 2_000_000).unwrap(),
            Some(WorkerEvent::PrivacyUnavailable {
                since_us: 2_000_000
            })
        ));
    }

    #[test]
    fn recovery_queued_after_a_source_stall_reports_the_old_expiry_first() {
        let mut gap = SourceGap::new(true);
        assert!(
            gap.before_observation(Some(100_000), true, 149_999)
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            gap.before_observation(Some(100_000), true, 160_000)
                .unwrap(),
            Some(WorkerEvent::PrivacyUnavailable { since_us: 150_000 })
        ));
        assert!(gap.active());
        assert!(
            gap.before_observation(Some(160_000), true, 170_000)
                .unwrap()
                .is_none()
        );
        assert!(gap.check(150_000 + RIDE_US).is_err());
        // Off and a worker that has not granted capture keep their old path.
        assert!(
            SourceGap::new(false)
                .before_observation(Some(100_000), true, 160_000)
                .unwrap()
                .is_none()
        );
        assert!(
            SourceGap::new(true)
                .before_observation(Some(100_000), false, 160_000)
                .unwrap()
                .is_none()
        );
    }

    struct Root(PathBuf);
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// #1569 row 45: the order begins again only before the worker's first
    /// datagram and within its first second (Third and the first builder on
    /// #2244).
    #[test]
    fn the_order_begins_again_only_before_the_first_datagram_in_the_first_second() {
        assert!(super::resyncs(0, 5_000, 5_000));
        assert!(super::resyncs(0, 5_000, 5_000 + super::RESYNC_FOR_US - 1));
        assert!(!super::resyncs(0, 5_000, 5_000 + super::RESYNC_FOR_US));
        assert!(!super::resyncs(1, 5_000, 5_001));
        // A clock read before the start is the start.
        assert!(super::resyncs(0, 5_000, 4_000));
    }

    #[test]
    fn a_receipt_comes_only_with_a_slides_reading() {
        let stamp = lamp_msgs::ClockStamp {
            clock_id: "test-clock".into(),
            at_us: 100,
            sequence: 7,
        };
        let physical = lamp_msgs::PrivacySwitch {
            stamp: stamp.clone(),
            reader_session_id: "reader".into(),
            muted: Some(false),
            fault: None,
        };
        let given = super::receipt(&Observation::Slide(physical.clone()), "0123456789abcdef")
            .expect("a slide's reading is the receipt");
        assert_eq!(given.controls_session, "0123456789abcdef");
        assert_eq!(given.physical, physical);
        assert!(
            super::receipt(&Observation::NoSlide(stamp), "0123456789abcdef").is_none(),
            "with no slide there is no switch reading to prove"
        );
    }

    #[test]
    fn selected_endpoints_bind_and_read_a_real_bus() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        // The lease is fresh for PRIVACY_FRESH_US on the host's clock. An
        // attempt in which the host stalled that long between the publish and
        // the read says nothing either way, so it is made again on a new bus
        // ([`STALLED_ATTEMPTS`], #1569).
        for _ in 0..STALLED_ATTEMPTS {
            let root = Root(PathBuf::from(format!(
                "/tmp/vpb-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )));
            std::fs::create_dir(&root.0).unwrap();
            let bus = lamp_controls::privacy::selected_bus(Bus::new(&root.0).unwrap());
            // Use the production endpoint constructors and names. A name rejected
            // by the transport must fail here before a native/device handoff.
            let _switch = switch_subscriber(&bus).unwrap();
            let mut reader =
                LeaseReader::with_clock(&root.0, INDICATOR_SUBSCRIBER, "host_fixture".into())
                    .unwrap();
            let mut publisher = bus
                .publisher_declared(
                    MicrophonePrivacy::TOPIC,
                    &MicrophonePrivacy::declaration(),
                    "voice",
                )
                .unwrap();
            let at = lamp_ipc::monotonic_us();
            let lease = MicrophonePrivacy {
                stamp: ClockStamp {
                    clock_id: "host_fixture".into(),
                    at_us: at,
                    sequence: 1,
                },
                session_id: publisher.boot().to_owned(),
                controller_boot: "1".repeat(32),
                microphone_generation: 1,
                observed_us: at,
                expires_at_us: at + PRIVACY_FRESH_US,
                permitted: false,
                physical_muted: Some(true),
                no_slide: false,
            };
            publisher.publish_json(at, &lease).unwrap();
            let read = reader.poll();
            if lamp_ipc::monotonic_us() - at < PRIVACY_FRESH_US {
                assert_eq!(read.unwrap(), Some(lease));
                return;
            }
            thread::sleep(AFTER_A_STALL);
        }
        panic!(
            "the host stalled past the lease's freshness in {STALLED_ATTEMPTS} of {STALLED_ATTEMPTS} attempts"
        );
    }

    /// Third's note on #2244 (#1569 row 45): the muted ring's reader takes
    /// the kept lease past a gap before its first datagram, in its first
    /// second, as the worker's fence does. This publisher never finds the
    /// reader, so the reader has only the topic's kept value, and two leases
    /// put down at once skip one there. Past its first second the same gap
    /// ends it, as a lost value. (A lease from another session, or going
    /// back, `from_message` refuses before the reader's own checks, as a
    /// lease's session and sequence must be the bus's own.)
    #[test]
    fn the_lease_reader_resyncs_on_kept_leases_only_before_its_first_datagram_in_its_first_second()
    {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        const LOST: &str = "privacy lease stream changed or lost a value";
        let sequence = |reader: &LeaseReader| reader.current.as_ref().map(|l| l.stamp.sequence);
        // The kept file is looked at once a rescan at most: read until the
        // newest lease is taken, or the reader ends, within 500 ms.
        let read_on = |reader: &mut LeaseReader, newest: u64| -> io::Result<()> {
            let until = std::time::Instant::now() + Duration::from_millis(500);
            while sequence(reader) != Some(newest) {
                reader.poll()?;
                assert!(std::time::Instant::now() < until, "never read {newest}");
                thread::sleep(Duration::from_millis(2));
            }
            Ok(())
        };
        // A lease still lives 50 ms. An attempt in which the host stalled
        // past envelope or payload freshness says nothing about resync,
        // so it is made again on a new bus, up to [`STALLED_ATTEMPTS`] times
        // with [`AFTER_A_STALL`] after each (#1569). The
        // fixture's separate publication period does not extend those
        // bounds or turn an expired lease into permission.
        let stalled = |error: &io::Error| {
            let text = error.to_string();
            text.contains("discarded") || text.contains("stale") || text.contains("unavailable")
        };
        let attempt = |past_first_second: bool| -> io::Result<()> {
            let root = Root(PathBuf::from(format!(
                "/tmp/vpl-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )));
            std::fs::create_dir(&root.0).unwrap();
            // Looks for subscribers at its first publication only.
            let never_looks = Bus::new(&root.0)
                .unwrap()
                .rescan_every(Duration::from_secs(3_600));
            // Kept values do not renew the subscriber's datagram heartbeat.
            // Isolate resync from its unrelated 20 ms Late notice: both reads
            // remain bounded at 500 ms, and the actual lease still lasts 50 ms.
            let declaration = MicrophonePrivacy::declaration().every(Duration::from_secs(1));
            let mut publisher = never_looks
                .publisher_declared(MicrophonePrivacy::TOPIC, &declaration, "voice")
                .unwrap();
            let session = publisher.boot().to_owned();
            let mut lease = |sequence: u64| {
                let at = lamp_ipc::monotonic_us();
                let lease = MicrophonePrivacy {
                    stamp: ClockStamp {
                        clock_id: "host_fixture".into(),
                        at_us: at,
                        sequence,
                    },
                    session_id: session.clone(),
                    controller_boot: "1".repeat(32),
                    microphone_generation: 1,
                    observed_us: at,
                    expires_at_us: at + PRIVACY_FRESH_US,
                    permitted: false,
                    physical_muted: Some(true),
                    no_slide: false,
                };
                publisher.publish_json(at, &lease).unwrap();
            };
            // Its one look, before the reader is there.
            lease(1);
            let mut reader =
                LeaseReader::with_clock(&root.0, INDICATOR_SUBSCRIBER, "host_fixture".into())
                    .unwrap();
            // Each lease read just after it is put down, so it is fresh.
            lease(2);
            read_on(&mut reader, 2)?;
            if past_first_second {
                reader.opened_us = lamp_ipc::monotonic_us().saturating_sub(RESYNC_FOR_US);
            }
            lease(3);
            lease(4);
            let read = read_on(&mut reader, 4);
            assert_eq!(reader.subscriber.statistics().datagrams, 0);
            match read {
                Ok(()) => assert!(!past_first_second, "a gap taken past the first second"),
                Err(error) if stalled(&error) => return Err(error),
                Err(error) => {
                    assert!(past_first_second, "{error}");
                    assert!(error.to_string().contains(LOST), "{error}");
                }
            }
            Ok(())
        };
        for past_first_second in [false, true] {
            let mut tries = 0;
            loop {
                match attempt(past_first_second) {
                    Ok(()) => break,
                    Err(error) => {
                        assert!(stalled(&error), "{error}");
                        tries += 1;
                        assert!(
                            tries < STALLED_ATTEMPTS,
                            "the host stalled in {STALLED_ATTEMPTS} of {STALLED_ATTEMPTS} attempts: {error}"
                        );
                        thread::sleep(AFTER_A_STALL);
                    }
                }
            }
        }
    }

    /// The reviewer's P3 on #2289: once a lease has come as a datagram, a gap
    /// ends the reader as a lost value, even inside its first second. Here
    /// the voice's publisher finds the reader, so leases flow as datagrams,
    /// and then one comes a sequence ahead: a valid lease written straight to
    /// the reader's socket, as a lost one would leave it.
    #[test]
    fn after_a_datagram_a_gap_ends_the_lease_reader_inside_its_first_second() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        const LOST: &str = "privacy lease stream changed or lost a value";
        // As the kept-lease test: a host stall past a lease's 50 ms, or past
        // the topic's 10 ms beat, says nothing, and is tried again.
        let stalled = |error: &io::Error| {
            let text = error.to_string();
            text.contains("discarded") || text.contains("stale") || text.contains("unavailable")
        };
        let attempt = || -> io::Result<()> {
            let root = Root(PathBuf::from(format!(
                "/tmp/vpd-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )));
            std::fs::create_dir(&root.0).unwrap();
            let mut reader =
                LeaseReader::with_clock(&root.0, INDICATOR_SUBSCRIBER, "host_fixture".into())
                    .unwrap();
            let bus = lamp_controls::privacy::selected_bus(Bus::new(&root.0).unwrap());
            let mut publisher = bus
                .publisher_declared(
                    MicrophonePrivacy::TOPIC,
                    &MicrophonePrivacy::declaration(),
                    "voice",
                )
                .unwrap();
            let session = publisher.boot().to_owned();
            let lease = |sequence: u64| {
                let at = lamp_ipc::monotonic_us();
                MicrophonePrivacy {
                    stamp: ClockStamp {
                        clock_id: "host_fixture".into(),
                        at_us: at,
                        sequence,
                    },
                    session_id: session.clone(),
                    controller_boot: "1".repeat(32),
                    microphone_generation: 1,
                    observed_us: at,
                    expires_at_us: at + PRIVACY_FRESH_US,
                    permitted: false,
                    physical_muted: Some(true),
                    no_slide: false,
                }
            };
            // Leases, each read just after it is put down, until one has
            // come as a datagram.
            let mut sequence = 0;
            let until = std::time::Instant::now() + Duration::from_millis(500);
            while reader.subscriber.statistics().datagrams == 0 {
                sequence += 1;
                let next = lease(sequence);
                publisher.publish_json(next.stamp.at_us, &next).unwrap();
                thread::sleep(Duration::from_millis(2));
                reader.poll()?;
                assert!(std::time::Instant::now() < until, "no datagram came");
            }
            assert!(lamp_ipc::monotonic_us() - reader.opened_us < RESYNC_FOR_US);
            // One a sequence ahead, straight to the reader's socket.
            let ahead = lease(sequence + 2);
            let header = lamp_bus::Header {
                topic: MicrophonePrivacy::TOPIC.into(),
                kind: Kind::Latest,
                enc: lamp_bus::JSON.into(),
                type_name: Some(MicrophonePrivacy::TYPE.into()),
                source: "voice".into(),
                boot: session.clone(),
                seq: sequence + 2,
                at_us: ahead.stamp.at_us,
                at_unix_ms: wall_ms(),
                expires_ms: Some(50),
            };
            let mut datagram = serde_json::to_vec(&header).unwrap();
            datagram.push(b'\n');
            datagram.extend_from_slice(&serde_json::to_vec(&ahead).unwrap());
            let socket = std::os::unix::net::UnixDatagram::unbound().unwrap();
            for entry in std::fs::read_dir(root.0.join("voice/privacy/subs")).unwrap() {
                socket.send_to(&datagram, entry.unwrap().path()).unwrap();
            }
            let until = std::time::Instant::now() + Duration::from_millis(500);
            loop {
                match reader.poll() {
                    Ok(_) => {
                        assert!(std::time::Instant::now() < until, "the gap was taken");
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(error) if stalled(&error) => return Err(error),
                    Err(error) => {
                        assert!(error.to_string().contains(LOST), "{error}");
                        return Ok(());
                    }
                }
            }
        };
        let mut tries = 0;
        while let Err(error) = attempt() {
            assert!(stalled(&error), "{error}");
            tries += 1;
            assert!(
                tries < STALLED_ATTEMPTS,
                "the host stalled in {STALLED_ATTEMPTS} of {STALLED_ATTEMPTS} attempts: {error}"
            );
            thread::sleep(AFTER_A_STALL);
        }
    }

    /// Messages that look right but say another type, written straight to
    /// every subscriber's socket on `voice/privacy` under `root`, each new,
    /// until `going` is false or 3 s pass: what a receive refuses and passes
    /// over within the one call.
    pub(crate) fn refused_flood(
        root: &Path,
        going: Arc<AtomicBool>,
        lease: &MicrophonePrivacy,
    ) -> thread::JoinHandle<u64> {
        let subscribers: Vec<_> = std::fs::read_dir(root.join("voice/privacy/subs"))
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        assert!(!subscribers.is_empty(), "a subscriber's socket");
        let payload = serde_json::to_vec(lease).unwrap();
        thread::spawn(move || {
            let socket = std::os::unix::net::UnixDatagram::unbound().unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            let mut header = lamp_bus::Header {
                topic: MicrophonePrivacy::TOPIC.into(),
                kind: Kind::Latest,
                enc: lamp_bus::JSON.into(),
                type_name: Some(ControlsState::TYPE.into()),
                source: "voice".into(),
                boot: "f".repeat(16),
                seq: 0,
                at_us: lamp_ipc::monotonic_us(),
                at_unix_ms: 0,
                expires_ms: None,
            };
            let mut sent = 0;
            while going.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
                header.seq += 1;
                header.at_unix_ms = wall_ms();
                let mut datagram = serde_json::to_vec(&header).unwrap();
                datagram.push(b'\n');
                datagram.extend_from_slice(&payload);
                for path in &subscribers {
                    if socket.send_to(&datagram, path).is_ok() {
                        sent += 1;
                    }
                }
            }
            sent
        })
    }

    /// #795 review: the muted ring's writer reads the lease from the watch,
    /// whose own thread reads the bus. Through a flood of refused messages
    /// each answer comes at once, a lease is not kept past its freshness,
    /// and the refused input still ends the reader for good.
    #[test]
    fn the_lease_watch_answers_at_once_through_refused_input_and_lets_a_lease_expire() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        // The lease is fresh for PRIVACY_FRESH_US on the host's clock. An
        // attempt in which the host stalled that long before the watch found
        // it says nothing either way, so it is made again on a new bus
        // ([`STALLED_ATTEMPTS`], #1569).
        for _ in 0..STALLED_ATTEMPTS {
            let root = Root(PathBuf::from(format!(
                "/tmp/vpw-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )));
            std::fs::create_dir(&root.0).unwrap();
            let bus = lamp_controls::privacy::selected_bus(Bus::new(&root.0).unwrap());
            let reader =
                LeaseReader::with_clock(&root.0, INDICATOR_SUBSCRIBER, "host_fixture".into())
                    .unwrap();
            let mut publisher = bus
                .publisher_declared(
                    MicrophonePrivacy::TOPIC,
                    &MicrophonePrivacy::declaration(),
                    "voice",
                )
                .unwrap();
            let at = lamp_ipc::monotonic_us();
            let lease = MicrophonePrivacy {
                stamp: ClockStamp {
                    clock_id: "host_fixture".into(),
                    at_us: at,
                    sequence: 1,
                },
                session_id: publisher.boot().to_owned(),
                controller_boot: "1".repeat(32),
                microphone_generation: 1,
                observed_us: at,
                expires_at_us: at + PRIVACY_FRESH_US,
                permitted: false,
                physical_muted: Some(true),
                no_slide: false,
            };
            publisher.publish_json(at, &lease).unwrap();
            // Watched from once its publisher is there, as the writer starts
            // after Voice's privacy worker.
            let watch = LeaseWatch::start(reader).unwrap();
            // Found while it is fresh. A refusal or a miss within that time fails;
            // past it, the host stalled and the attempt says nothing.
            let found = loop {
                let latest = watch.latest();
                if latest
                    .as_ref()
                    .is_ok_and(|found| found.as_ref() == Some(&lease))
                {
                    break true;
                }
                if lamp_ipc::monotonic_us() - at >= PRIVACY_FRESH_US {
                    break false;
                }
                latest.unwrap();
                thread::sleep(Duration::from_millis(1));
            };
            if !found {
                thread::sleep(AFTER_A_STALL);
                continue;
            }
            let going = Arc::new(AtomicBool::new(true));
            let flood = refused_flood(&root.0, going.clone(), &lease);
            let mut slowest = Duration::ZERO;
            let mut stale = None;
            for _ in 0..200 {
                let asked = std::time::Instant::now();
                let answer = watch.latest();
                slowest = slowest.max(asked.elapsed());
                if lamp_ipc::monotonic_us() > lease.expires_at_us + 1_000 {
                    stale = Some(answer.as_ref().map(Option::is_some).unwrap_or(false));
                }
                thread::sleep(Duration::from_millis(1));
            }
            going.store(false, Ordering::Release);
            let sent = flood.join().unwrap();
            // The first refused message already ends the reader, and its socket
            // with it: the flood stops reaching anything soon after.
            assert!(sent > 0, "{sent}");
            assert!(slowest < Duration::from_millis(5), "{slowest:?}");
            // Past its freshness the lease is gone, whatever the thread found.
            assert_eq!(stale, Some(false));
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while watch.latest().is_ok() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "refused input ends it"
                );
                thread::sleep(Duration::from_millis(1));
            }
            thread::sleep(Duration::from_millis(20));
            assert!(watch.latest().is_err(), "for good");
            return;
        }
        panic!(
            "the host stalled past the lease's freshness in {STALLED_ATTEMPTS} of {STALLED_ATTEMPTS} attempts"
        );
    }
}
