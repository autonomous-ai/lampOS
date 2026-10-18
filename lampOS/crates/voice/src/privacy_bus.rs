//! Opt-in replacement for the exclusive GPIO worker. Controls reports the
//! switch; this Voice child combines it with its controller's fresh permission.
//! Bus/disk work stays in this process, outside the interaction owner's loop.
use crate::{
    privacy::{PRIVACY_FRESH_US, PrivacyGate},
    process::boot_text,
    transport::WorkerChannels,
    wire::{Control, WorkerEvent},
};
use lamp_bus::{Bus, Kind, Message, Notice, Subscriber};
use lamp_msgs::{ClockStamp, ControlsState, MicrophonePrivacy, PrivacySwitch};
use lamp_turn_owner::{BootId, BoundaryGuard, MonoTime, Permission};
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
const DRAIN_LIMIT: usize = 32;
#[cfg(any(target_os = "linux", test))]
pub(crate) const INDICATOR_SUBSCRIBER: &str = "privacy_ring";

fn switch_subscriber(bus: &Bus) -> io::Result<Subscriber> {
    bus.subscribe_typed(ControlsState::TOPIC, "voice_privacy", ControlsState::TYPE)
}

fn invalid(why: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.into())
}

pub fn selected_root(switch: Option<&str>, root: Option<&str>) -> io::Result<Option<PathBuf>> {
    if switch != Some("on") {
        return Ok(None);
    }
    let root = PathBuf::from(root.unwrap_or("/run/lampos-bus"));
    if !root.is_absolute() {
        return Err(invalid("privacy Bus root must be absolute"));
    }
    Ok(Some(root))
}

pub fn root_from_environment() -> io::Result<Option<PathBuf>> {
    selected_root(
        std::env::var(ENVIRONMENT).ok().as_deref(),
        std::env::var("LAMP_BUS_ROOT").ok().as_deref(),
    )
}

pub fn worker_arguments(root: Option<&Path>) -> io::Result<Vec<String>> {
    match root {
        None => Ok(Vec::new()),
        Some(root) if root.is_absolute() => Ok(vec![
            ROOT_FLAG.into(),
            root.to_str()
                .ok_or_else(|| invalid("privacy root must be UTF-8"))?
                .into(),
        ]),
        Some(_) => Err(invalid("privacy root must be absolute")),
    }
}

pub fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis().min(u128::from(u64::MAX)) as u64)
}

/// A restart or discontinuity ends this worker, so the supervisor must close
/// and verify cleanup before a fresh worker can learn another producer.
pub struct SwitchFence {
    clock: String,
    session: Option<String>,
    header_sequence: u64,
    publication: Option<ClockStamp>,
    last: Option<PrivacySwitch>,
}

impl SwitchFence {
    pub fn new(clock: String) -> Self {
        Self {
            clock,
            session: None,
            header_sequence: 0,
            publication: None,
            last: None,
        }
    }

    pub fn accept(
        &mut self,
        message: &Message,
        now_us: u64,
        unix_ms: u64,
    ) -> io::Result<Option<PrivacySwitch>> {
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
            || message.gap != 0
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
            || (self.header_sequence != 0 && self.header_sequence.checked_add(1) != Some(h.seq))
            || self.publication.as_ref().is_some_and(|old| {
                state.stamp.sequence <= old.sequence || state.stamp.at_us < old.at_us
            })
        {
            return Err(invalid("privacy Controls publisher/clock/order changed"));
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
        self.header_sequence = h.seq;
        self.publication = Some(state.stamp);
        self.last = Some(value.clone());
        Ok(changed.then_some(value))
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
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    for _ in 0..16 {
        match channels.control.receive()? {
            Some(Control::Stop) => return Ok(false),
            Some(Control::Authority { snapshot }) => {
                guard.install(MonoTime::from_micros(lamp_ipc::monotonic_us()), snapshot)?
            }
            Some(_) => return Err(invalid("invalid Controls privacy worker command").into()),
            None => return Ok(true),
        }
    }
    Err(invalid("privacy authority drain overflow").into())
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
        })
    }

    pub fn poll(&mut self) -> io::Result<Option<MicrophonePrivacy>> {
        let now_us = lamp_ipc::monotonic_us();
        if self.last_now.is_some_and(|last| now_us < last) {
            return Err(invalid("privacy reader clock regressed"));
        }
        self.last_now = Some(now_us);
        for count in 0..DRAIN_LIMIT {
            let before = counters(&self.subscriber);
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
            if message.gap != 0
                || message.publisher_restarted
                || self.current.as_ref().is_some_and(|old| {
                    old.session_id != value.session_id
                        || old.controller_boot != value.controller_boot
                        || value.stamp.sequence != old.stamp.sequence.saturating_add(1)
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

/// The only publisher of `voice/privacy`. A bus stall cannot renew its lease;
/// Voice and Ears independently expire their final-boundary permissions.
/// Evidence that cannot be had ends it cleanly, said as [`UNREADABLE`];
/// anything else ends it as an error.
pub fn run(
    mut channels: WorkerChannels,
    controller: BootId,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match serve(&mut channels, controller, root) {
        Err(error) if error.is::<Unreadable>() => {
            channels.control.send(WorkerEvent::Fault {
                code: format!("{UNREADABLE}{error}"),
            })?;
            Ok(())
        }
        other => other,
    }
}

fn serve(
    channels: &mut WorkerChannels,
    controller: BootId,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let clock = lamp_controls::runtime::clock_id(None)?;
    let bus = lamp_controls::privacy::selected_bus(Bus::new(root)?);
    let mut subscriber = switch_subscriber(&bus)?;
    let mut publisher = bus.publisher_declared(
        MicrophonePrivacy::TOPIC,
        &MicrophonePrivacy::declaration(),
        "voice",
    )?;
    let mut fence = SwitchFence::new(clock.clone());
    let mut guard = BoundaryGuard::new(controller, MonoTime::from_micros(lamp_ipc::monotonic_us()));
    let mut gate = CaptureLeaseGate::default();
    let mut last: Option<PrivacySwitch> = None;
    let mut published_at = None;
    let mut previous_permission = None;
    let mut sequence = 0u64;
    let began = lamp_ipc::monotonic_us();
    channels.control.send(WorkerEvent::Ready)?;
    loop {
        if !authority(channels, &mut guard)? {
            return Ok(());
        }
        let now = lamp_ipc::monotonic_us();
        if guard
            .state()
            .is_some_and(|state| state.expires_at().as_micros() <= now)
            || (guard.state().is_none() && now.saturating_sub(began) >= 250_000)
        {
            return Err(invalid("privacy controller lease expired").into());
        }
        let mut empty = false;
        for _ in 0..DRAIN_LIMIT {
            let before = counters(&subscriber);
            let message = subscriber.recv(Duration::ZERO).map_err(unreadable)?;
            if before != counters(&subscriber) {
                return Err(unreadable("invalid Controls message discarded"));
            }
            let Some(message) = message else {
                empty = true;
                break;
            };
            let now = lamp_ipc::monotonic_us();
            if let Some(value) = fence.accept(&message, now, wall_ms()).map_err(unreadable)? {
                let muted = value
                    .muted
                    .ok_or_else(|| unreadable("missing switch level"))?;
                gate.observe(value.stamp.sequence, value.stamp.at_us, now, muted);
                // Revoke the main owner before any Bus publication/disk work.
                channels.control.send(WorkerEvent::Privacy {
                    acquired_at_us: value.stamp.at_us,
                    muted,
                })?;
                last = Some(value);
            }
        }
        if !empty {
            return Err(unreadable("Controls privacy drain overflow"));
        }
        notices(&mut subscriber).map_err(unreadable)?;
        // Revocation arriving during Bus I/O wins before another lease.
        if !authority(channels, &mut guard)? {
            return Ok(());
        }
        let now = lamp_ipc::monotonic_us();
        let physical = last.as_ref().filter(|sample| {
            now.checked_sub(sample.stamp.at_us)
                .is_some_and(|age| age < PRIVACY_FRESH_US)
        });
        if physical.is_none() && now.saturating_sub(began) >= PRIVACY_FRESH_US {
            return Err(unreadable("physical privacy source expired"));
        }
        if let Some(state) = guard.state() {
            let permitted = empty && physical.is_some() && gate.permitted(state, now);
            let level = physical.and_then(|sample| sample.muted);
            let identity = (permitted, level, state.microphone_generation());
            if previous_permission != Some(identity)
                || published_at.is_none_or(|at| now.saturating_sub(at) >= 10_000)
            {
                sequence = sequence
                    .checked_add(1)
                    .ok_or_else(|| invalid("privacy publisher sequence exhausted"))?;
                let observed_us = physical.map_or(0, |sample| sample.stamp.at_us);
                let mut expires_at_us = state
                    .expires_at()
                    .as_micros()
                    .min(now.saturating_add(PRIVACY_FRESH_US));
                if level.is_some() {
                    expires_at_us = expires_at_us.min(observed_us.saturating_add(PRIVACY_FRESH_US));
                }
                let value = MicrophonePrivacy {
                    stamp: ClockStamp {
                        clock_id: clock.clone(),
                        at_us: now,
                        sequence,
                    },
                    session_id: publisher.boot().to_owned(),
                    controller_boot: boot_text(controller),
                    microphone_generation: state.microphone_generation(),
                    observed_us,
                    expires_at_us,
                    permitted,
                    physical_muted: level,
                };
                value.check()?;
                publisher.publish_json(now, &value)?;
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
    use std::sync::atomic::AtomicU64;

    struct Root(PathBuf);
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn selected_endpoints_bind_and_read_a_real_bus() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        // The lease is fresh for PRIVACY_FRESH_US on the host's clock. An
        // attempt in which the host stalled that long between the publish and
        // the read says nothing either way, so it is made again on a new bus,
        // up to five times (#1569).
        for _ in 0..5 {
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
            };
            publisher.publish_json(at, &lease).unwrap();
            let read = reader.poll();
            if lamp_ipc::monotonic_us() - at < PRIVACY_FRESH_US {
                assert_eq!(read.unwrap(), Some(lease));
                return;
            }
        }
        panic!("the host stalled past the lease's freshness in 5 of 5 attempts");
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
        // it says nothing either way, so it is made again on a new bus, up to
        // five times (#1569).
        for _ in 0..5 {
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
        panic!("the host stalled past the lease's freshness in 5 of 5 attempts");
    }
}
