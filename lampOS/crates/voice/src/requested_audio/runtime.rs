//! Two fixed background workers; the conversation thread only reads cached
//! permission and moves already checked samples. Losing permission is terminal.
use lamp_bus::Bus;
use lamp_msgs::{
    RequestedAudioLease, RequestedAudioOffer, RequestedAudioOperation, RequestedAudioOutcome,
    RequestedAudioPhase, SPEAKER_STATE_FRESH_US, SpeakerState,
};
#[cfg(test)]
use std::sync::Condvar;
use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, Thread},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MONITOR_EVERY: Duration = Duration::from_millis(20);
const CACHE_FRESH_US: u64 = 100_000;
const EXCHANGE_WAIT: Duration = Duration::from_millis(150);
const RENEW_US: u64 = 200_000;
const ACTIVE: u8 = 0;
const COMPLETED: u8 = 1;
const CANCELLED: u8 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Fence {
    session: String,
    generation: u64,
    continuity: u64,
}

struct Audible {
    fence: Fence,
    valid_since_us: u64,
    wire_until_us: u64,
}

#[derive(Default)]
struct SpeakerCache {
    last: Option<SpeakerState>,
    observed_us: Option<u64>,
    audible: Option<Audible>,
    continuity: u64,
}

impl SpeakerCache {
    /// A repeated Latest is normal. Changed equal-sequence data, regressing
    /// counters and missed monitor deadlines all break continuous permission.
    fn observe(&mut self, state: Option<SpeakerState>, now_us: u64) {
        if self.observed_us.is_some_and(|at| {
            now_us
                .checked_sub(at)
                .is_none_or(|age| age >= CACHE_FRESH_US)
        }) {
            self.audible = None;
        }
        self.observed_us = Some(now_us);
        let Some(state) = state else {
            self.audible = None;
            return;
        };
        if let Some(last) = &self.last {
            if state.session_id == last.session_id {
                if state.stamp.sequence < last.stamp.sequence
                    || state.stamp.at_us < last.stamp.at_us
                    || state.mute_generation < last.mute_generation
                    || (state.stamp.sequence == last.stamp.sequence && state != *last)
                {
                    self.audible = None;
                    return;
                }
                if state.mute_generation != last.mute_generation {
                    self.audible = None;
                }
            } else {
                self.audible = None;
            }
        }
        self.last = Some(state.clone());
        if state.muted || state.volume_raw.is_none_or(|raw| raw == 0 || raw > 100) {
            self.audible = None;
            return;
        }
        let wire_until_us = state.stamp.at_us.saturating_add(SPEAKER_STATE_FRESH_US);
        if let Some(audible) = self.audible.as_mut() {
            audible.wire_until_us = wire_until_us;
        } else if let Some(next) = self.continuity.checked_add(1) {
            self.continuity = next;
            self.audible = Some(Audible {
                fence: Fence {
                    session: state.session_id,
                    generation: state.mute_generation,
                    continuity: next,
                },
                valid_since_us: now_us,
                wire_until_us,
            });
        }
    }

    fn deadline(&self, fence: &Fence, now_us: u64) -> Option<u64> {
        let audible = self.audible.as_ref()?;
        let observed = self.observed_us?;
        let until = observed
            .saturating_add(CACHE_FRESH_US)
            .min(audible.wire_until_us);
        (audible.fence == *fence && now_us >= observed && now_us < until).then_some(until)
    }

    fn bind(&self, issued_us: u64, now_us: u64) -> Option<Fence> {
        let audible = self.audible.as_ref()?;
        if issued_us > now_us || audible.valid_since_us > issued_us {
            return None;
        }
        self.deadline(&audible.fence, now_us)?;
        Some(audible.fence.clone())
    }
}

#[derive(Default)]
struct Speaker {
    cache: Mutex<SpeakerCache>,
    #[cfg(test)]
    changed: Condvar,
}

struct Running {
    stop: AtomicBool,
    monitor: AtomicBool,
    worker: AtomicBool,
}

#[derive(Default)]
struct Capacity {
    active: AtomicBool,
    #[cfg(test)]
    lock: Mutex<()>,
    #[cfg(test)]
    changed: Condvar,
}

impl Capacity {
    fn release(&self) {
        #[cfg(test)]
        let _held = self.lock.lock().unwrap_or_else(|error| error.into_inner());
        self.active.store(false, Ordering::Release);
        #[cfg(test)]
        self.changed.notify_all();
    }
}

impl Running {
    fn live(&self) -> bool {
        !self.stop.load(Ordering::Acquire)
            && self.monitor.load(Ordering::Acquire)
            && self.worker.load(Ordering::Acquire)
    }
}

struct Shared {
    offer: RequestedAudioOffer,
    fence: Fence,
    speaker: Arc<Speaker>,
    running: Arc<Running>,
    terminal: AtomicU8,
    started: AtomicBool,
    ready: AtomicBool,
    lease_until_us: AtomicU64,
    samples: Mutex<Option<Vec<i16>>>,
    #[cfg(test)]
    changed: Condvar,
}

impl Shared {
    fn cancel(&self) {
        let _ =
            self.terminal
                .compare_exchange(ACTIVE, CANCELLED, Ordering::AcqRel, Ordering::Acquire);
        #[cfg(test)]
        self.changed.notify_all();
    }

    fn limit(&self, now_us: u64, playback: bool) -> Option<u64> {
        let check = || {
            if self.terminal.load(Ordering::Acquire) != ACTIVE
                || !self.running.live()
                || now_us < self.offer.issued_us
                || (playback && !self.ready.load(Ordering::Acquire))
            {
                return None;
            }
            let mut until = if self.started.load(Ordering::Acquire) {
                self.offer.finish_before_us
            } else {
                self.offer.start_before_us
            };
            until = until.min(
                self.speaker
                    .cache
                    .try_lock()
                    .ok()?
                    .deadline(&self.fence, now_us)?,
            );
            let lease = self.lease_until_us.load(Ordering::Acquire);
            if playback || lease != 0 {
                until = until.min(lease);
            }
            (now_us < until).then_some(until)
        };
        let permitted = check();
        if permitted.is_none() {
            self.cancel();
        }
        permitted
    }
}

/// One original requested recording. No method performs I/O, waits for a lock,
/// joins a worker, or renews an original time bound.
pub(crate) struct Ticket {
    shared: Arc<Shared>,
}

impl Ticket {
    pub(crate) fn live(&self, now_us: u64) -> bool {
        self.shared.limit(now_us, false).is_some()
    }

    pub(crate) fn take_ready(&self, now_us: u64) -> Option<Vec<i16>> {
        // Preparation is allowed before a Start reply; asking early must not
        // consume or cancel the still-pending offer.
        if !self.live(now_us) || !self.shared.ready.load(Ordering::Acquire) {
            return None;
        }
        self.shared.limit(now_us, true)?;
        self.shared.samples.try_lock().ok()?.take()
    }

    pub(crate) fn may_play(&self, now_us: u64) -> bool {
        self.output_lease_us(now_us).is_some()
    }

    pub(crate) fn started(&self, now_us: u64) -> bool {
        if self.shared.limit(now_us, true).is_none() {
            return false;
        }
        self.shared.started.store(true, Ordering::Release);
        self.shared.terminal.load(Ordering::Acquire) == ACTIVE
    }

    /// The final output owner caps its own permit by this remaining duration.
    /// A standard 100 ms chunk permit cannot outlive any underlying authority.
    pub(crate) fn output_lease_us(&self, now_us: u64) -> Option<u64> {
        self.shared.limit(now_us, true).map(|until| until - now_us)
    }

    pub(crate) fn completed(&self) {
        if self.shared.started.load(Ordering::Acquire) {
            let _ = self.shared.terminal.compare_exchange(
                ACTIVE,
                COMPLETED,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            #[cfg(test)]
            self.shared.changed.notify_all();
        }
    }

    #[cfg(test)]
    pub(crate) fn wait_ready(&self, timeout: Duration) -> bool {
        let Ok(samples) = self.shared.samples.lock() else {
            return false;
        };
        let Ok((samples, _)) =
            self.shared
                .changed
                .wait_timeout_while(samples, timeout, |samples| {
                    samples.is_none() && self.shared.terminal.load(Ordering::Acquire) == ACTIVE
                })
        else {
            return false;
        };
        samples.is_some() && self.live(lamp_ipc::monotonic_us())
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.shared.cancel();
    }
}

pub(crate) struct Service {
    queue: SyncSender<Arc<Shared>>,
    speaker: Arc<Speaker>,
    running: Arc<Running>,
    busy: Arc<Capacity>,
    monitor: Thread,
    worker: Thread,
}

impl Service {
    pub(crate) fn from_environment() -> io::Result<Option<Arc<Self>>> {
        if !super::enabled(std::env::var(super::ENVIRONMENT).ok().as_deref()) {
            return Ok(None);
        }
        // A stalled regular-file read cannot be joined on the conversation
        // thread. Reusing this one process-lifetime pair prevents session or
        // privacy restarts from accumulating detached readers. Configuration
        // and construction failure are also fixed for this process lifetime.
        static SERVICE: OnceLock<Result<Arc<Service>, String>> = OnceLock::new();
        cached(&SERVICE, || {
            let skills = std::env::var_os(crate::tools::SKILLS_ENVIRONMENT).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "requested audio requires LAMP_SKILLS_DIR",
                )
            })?;
            let clock_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
            Self::new(
                Bus::from_environment()?,
                skills.into(),
                clock_id.trim().into(),
            )
        })
        .map(Some)
    }

    pub(crate) fn new(bus: Bus, skills: PathBuf, clock_id: String) -> io::Result<Self> {
        let socket = skills.join(crate::tools::CALL_SOCKET);
        lamp_schedule::client::validate_socket(&socket).map_err(io::Error::other)?;
        lamp_msgs::ClockStamp {
            clock_id: clock_id.clone(),
            at_us: 0,
            sequence: 1,
        }
        .check()
        .map_err(io::Error::other)?;
        let speaker = Arc::new(Speaker::default());
        let running = Arc::new(Running {
            stop: AtomicBool::new(false),
            monitor: AtomicBool::new(true),
            worker: AtomicBool::new(true),
        });
        let busy = Arc::new(Capacity::default());
        let (queue, receive) = mpsc::sync_channel(1);
        let monitor = {
            let (speaker, running) = (speaker.clone(), running.clone());
            thread::Builder::new()
                .name("lamp-speaker-state".into())
                .spawn(move || {
                    let _alive = Alive(&running.monitor);
                    while !running.stop.load(Ordering::Acquire) {
                        let message = bus.latest(SpeakerState::TOPIC).ok().flatten();
                        let now_us = lamp_ipc::monotonic_us();
                        let unix_ms = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map_or(0, |age| age.as_millis().min(u128::from(u64::MAX)) as u64);
                        let state = message.as_ref().and_then(|message| {
                            SpeakerState::from_message(message, &clock_id, now_us, unix_ms).ok()
                        });
                        let Ok(mut cache) = speaker.cache.lock() else {
                            break;
                        };
                        cache.observe(state, now_us);
                        drop(cache);
                        #[cfg(test)]
                        speaker.changed.notify_all();
                        thread::park_timeout(MONITOR_EVERY);
                    }
                })?
        };
        let worker = {
            let (running, busy) = (running.clone(), busy.clone());
            thread::Builder::new()
                .name("lamp-requested-audio".into())
                .spawn(move || {
                    let _alive = Alive(&running.worker);
                    work(receive, &socket, &running, &busy);
                })
        };
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                running.stop.store(true, Ordering::Release);
                monitor.thread().unpark();
                return Err(error);
            }
        };
        Ok(Self {
            queue,
            speaker,
            running,
            busy,
            monitor: monitor.thread().clone(),
            worker: worker.thread().clone(),
        })
    }

    /// Capacity one includes the active preparation/playback, not merely the
    /// channel. A second offer is refused rather than becoming a future sound.
    pub(crate) fn offer(&self, offer: RequestedAudioOffer) -> Option<Ticket> {
        let now_us = lamp_ipc::monotonic_us();
        if offer.check().is_err() || !self.running.live() || now_us >= offer.start_before_us {
            return None;
        }
        // A refused second offer must not contend with the active worker's
        // nonblocking permission check. Claim capacity before reading its cache.
        self.busy
            .active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        let Some(fence) = self
            .speaker
            .cache
            .try_lock()
            .ok()
            .and_then(|cache| cache.bind(offer.context.issued_us, now_us))
        else {
            self.busy.release();
            return None;
        };
        let shared = Arc::new(Shared {
            offer,
            fence,
            speaker: self.speaker.clone(),
            running: self.running.clone(),
            terminal: AtomicU8::new(ACTIVE),
            started: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            lease_until_us: AtomicU64::new(0),
            samples: Mutex::new(None),
            #[cfg(test)]
            changed: Condvar::new(),
        });
        if self.queue.try_send(shared.clone()).is_err() {
            self.busy.release();
            return None;
        }
        self.worker.unpark();
        Some(Ticket { shared })
    }

    /// A fixture can establish the monitored baseline before issuing its spoken
    /// context. This seam performs no action and is absent from runtime builds.
    #[cfg(test)]
    pub(crate) fn wait_cached(&self, timeout: Duration) -> bool {
        let Ok(cache) = self.speaker.cache.lock() else {
            return false;
        };
        let Ok((cache, _)) = self
            .speaker
            .changed
            .wait_timeout_while(cache, timeout, |cache| {
                let now = lamp_ipc::monotonic_us();
                cache.bind(now, now).is_none()
            })
        else {
            return false;
        };
        let now = lamp_ipc::monotonic_us();
        cache.bind(now, now).is_some()
    }

    #[cfg(test)]
    pub(crate) fn wait_idle(&self, timeout: Duration) -> bool {
        let Ok(held) = self.busy.lock.lock() else {
            return false;
        };
        let Ok((_held, _)) = self
            .busy
            .changed
            .wait_timeout_while(held, timeout, |_| self.busy.active.load(Ordering::Acquire))
        else {
            return false;
        };
        !self.busy.active.load(Ordering::Acquire)
    }
}

fn cached(
    slot: &OnceLock<Result<Arc<Service>, String>>,
    create: impl FnOnce() -> io::Result<Service>,
) -> io::Result<Arc<Service>> {
    slot.get_or_init(|| create().map(Arc::new).map_err(|error| error.to_string()))
        .as_ref()
        .map(Arc::clone)
        .map_err(|why| io::Error::other(why.clone()))
}

impl Drop for Service {
    fn drop(&mut self) {
        self.running.stop.store(true, Ordering::Release);
        self.monitor.unpark();
        self.worker.unpark();
    }
}

struct Alive<'a>(&'a AtomicBool);
impl Drop for Alive<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

struct Busy<'a>(&'a Capacity);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}

fn exchange(socket: &Path, operation: &RequestedAudioOperation) -> Option<serde_json::Value> {
    let value = serde_json::to_value(operation).ok()?;
    let reply =
        lamp_schedule::client::exchange(socket, &value, Instant::now() + EXCHANGE_WAIT).ok()?;
    (reply["ok"] == true).then(|| reply["result"].clone())
}

fn authorize(socket: &Path, shared: &Shared, phase: RequestedAudioPhase) -> bool {
    let now = lamp_ipc::monotonic_us();
    if shared.limit(now, false).is_none() {
        return false;
    }
    let old_until = shared.lease_until_us.load(Ordering::Acquire);
    let result = exchange(
        socket,
        &RequestedAudioOperation::AudioAuthorize {
            offer: shared.offer.clone(),
            phase,
        },
    );
    let now = lamp_ipc::monotonic_us();
    if shared.limit(now, false).is_none() || (old_until != 0 && now >= old_until) {
        return false;
    }
    let lease =
        result.and_then(|result| serde_json::from_value::<RequestedAudioLease>(result).ok());
    let Some(lease) = lease.filter(|lease| lease.check(&shared.offer, now).is_ok()) else {
        return false;
    };
    shared
        .lease_until_us
        .store(lease.until_us, Ordering::Release);
    true
}

fn prepare(socket: &Path, shared: &Shared, renew_at: &mut u64) -> bool {
    if !authorize(socket, shared, RequestedAudioPhase::Start) {
        return false;
    }
    *renew_at = lamp_ipc::monotonic_us().saturating_add(RENEW_US);
    let Ok(sound) = super::read_sound(Path::new(&shared.offer.asset)) else {
        return false;
    };
    if sound.sha256 != shared.offer.sha256
        || sound.samples.len() != shared.offer.frames as usize
        || shared.limit(lamp_ipc::monotonic_us(), false).is_none()
    {
        return false;
    }
    let Ok(mut samples) = shared.samples.lock() else {
        return false;
    };
    *samples = Some(sound.samples);
    shared.ready.store(true, Ordering::Release);
    drop(samples);
    #[cfg(test)]
    shared.changed.notify_all();
    true
}

fn work(receive: Receiver<Arc<Shared>>, socket: &Path, running: &Running, busy: &Capacity) {
    loop {
        let shared = match receive.recv_timeout(MONITOR_EVERY) {
            Ok(shared) => shared,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) if running.stop.load(Ordering::Acquire) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
        };
        let _busy = Busy(busy);
        let mut renew_at = 0;
        if !prepare(socket, &shared, &mut renew_at) {
            shared.cancel();
        }
        while shared.limit(lamp_ipc::monotonic_us(), false).is_some() {
            if lamp_ipc::monotonic_us() >= renew_at {
                if !authorize(socket, &shared, RequestedAudioPhase::Renew) {
                    shared.cancel();
                    break;
                }
                renew_at = lamp_ipc::monotonic_us().saturating_add(RENEW_US);
            }
            thread::park_timeout(MONITOR_EVERY);
        }
        let outcome = if shared.terminal.load(Ordering::Acquire) == COMPLETED {
            RequestedAudioOutcome::Completed
        } else {
            shared.cancel();
            RequestedAudioOutcome::Cancelled
        };
        // One bounded best-effort terminal receipt, even after a cancelled or
        // ambiguous Start. No failed authorization or mutation is retried.
        let _ = exchange(
            socket,
            &RequestedAudioOperation::AudioFinish {
                offer: shared.offer.clone(),
                outcome,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::{ClockStamp, REQUESTED_AUDIO_LEASE_US, RequestedAudioContext};
    use std::{
        fs,
        io::{BufRead, BufReader, Cursor, Read, Write},
        os::unix::net::{UnixListener, UnixStream},
        sync::atomic::AtomicUsize,
        thread::JoinHandle,
    };

    fn state(at_us: u64, sequence: u64, generation: u64) -> SpeakerState {
        SpeakerState {
            stamp: ClockStamp {
                clock_id: "fixture-host".into(),
                at_us,
                sequence,
            },
            session_id: "1234567890abcdef".into(),
            mute_generation: generation,
            volume_raw: Some(40),
            muted: false,
        }
    }

    fn offer(issued_us: u64) -> RequestedAudioOffer {
        RequestedAudioOffer {
            skills_boot: "skills-fixture".into(),
            definition_generation: 1,
            offer_id: 1,
            context: RequestedAudioContext {
                worker_boot: "voice-fixture".into(),
                provider_session: 1,
                request: 1,
                privacy_generation: 1,
                call_id: "call-1".into(),
                issued_us,
            },
            skill: "recording".into(),
            asset: "/tmp/recording.wav".into(),
            sha256: "0".repeat(64),
            frames: 480,
            issued_us,
            start_before_us: issued_us + 10_000_000,
            finish_before_us: issued_us + 70_000_000,
        }
    }

    fn fixture(issued_us: u64, now_us: u64, ready: bool) -> Ticket {
        let mut cache = SpeakerCache::default();
        // Continuous acquisition, with no real wait, across the original time
        // bounds. This is distinct from merely backdating a cached timestamp.
        let mut at = issued_us;
        let mut seq = 1;
        loop {
            cache.observe(Some(state(at, seq, 0)), at);
            if at == now_us {
                break;
            }
            at = at.saturating_add(80_000).min(now_us);
            seq += 1;
        }
        let fence = cache.bind(issued_us, now_us).unwrap();
        Ticket {
            shared: Arc::new(Shared {
                offer: offer(issued_us),
                fence,
                speaker: Arc::new(Speaker {
                    cache: Mutex::new(cache),
                    changed: Condvar::new(),
                }),
                running: Arc::new(Running {
                    stop: AtomicBool::new(false),
                    monitor: AtomicBool::new(true),
                    worker: AtomicBool::new(true),
                }),
                terminal: AtomicU8::new(ACTIVE),
                started: AtomicBool::new(false),
                ready: AtomicBool::new(ready),
                lease_until_us: AtomicU64::new(if ready { now_us + 500_000 } else { 0 }),
                samples: Mutex::new(ready.then(|| vec![100; 480])),
                changed: Condvar::new(),
            }),
        }
    }

    #[test]
    fn admission_requires_a_continuously_audible_baseline_before_the_spoken_call() {
        let mut cache = SpeakerCache::default();
        cache.observe(Some(state(1000, 1, 0)), 1000);
        assert!(cache.bind(999, 1000).is_none());
        let first = cache.bind(1000, 1000).unwrap();
        cache.observe(None, 1010);
        assert!(cache.deadline(&first, 1010).is_none());
        cache.observe(Some(state(1000, 1, 0)), 1020);
        assert!(cache.bind(1000, 1020).is_none());
        assert_ne!(cache.bind(1020, 1020).unwrap(), first);
        // The mute itself was coalesced out of Latest, but its counter remains.
        cache.observe(Some(state(1030, 2, 1)), 1030);
        assert!(cache.bind(1020, 1030).is_none());
        assert!(cache.deadline(&first, 1030).is_none());
    }

    #[test]
    fn equal_sequence_changes_regression_and_restart_break_permission() {
        let mut cache = SpeakerCache::default();
        cache.observe(Some(state(1000, 2, 1)), 1000);
        let original = cache.bind(1000, 1000).unwrap();
        cache.observe(Some(state(1000, 2, 1)), 1010);
        assert!(cache.deadline(&original, 1010).is_some());
        for changed in [state(1000, 1, 1), state(1000, 3, 0), state(999, 3, 1)] {
            cache.observe(Some(changed), 1020);
            assert!(cache.bind(1020, 1020).is_none());
        }
        let mut changed = state(1000, 2, 1);
        changed.volume_raw = Some(30);
        cache.observe(Some(changed), 1030);
        assert!(cache.bind(1030, 1030).is_none());
        let mut restart = state(1040, 1, 0);
        restart.session_id = "fedcba0987654321".into();
        cache.observe(Some(restart), 1040);
        assert!(cache.deadline(&original, 1040).is_none());
        assert!(cache.bind(1030, 1040).is_none());
        assert!(cache.bind(1040, 1040).is_some());
    }

    #[test]
    fn monitor_stalls_and_the_wire_deadline_cannot_be_refreshed_by_repeated_reads() {
        let mut cache = SpeakerCache::default();
        let sample = state(1, 1, 0);
        cache.observe(Some(sample.clone()), 1);
        let original = cache.bind(1, 1).unwrap();
        assert!(cache.deadline(&original, CACHE_FRESH_US).is_some());
        assert!(cache.deadline(&original, CACHE_FRESH_US + 1).is_none());
        cache.observe(Some(sample.clone()), CACHE_FRESH_US + 1);
        assert!(cache.deadline(&original, CACHE_FRESH_US + 1).is_none());
        for now in (CACHE_FRESH_US + 1..=SPEAKER_STATE_FRESH_US + 1).step_by(20_000) {
            cache.observe(Some(sample.clone()), now);
        }
        assert!(
            cache
                .bind(SPEAKER_STATE_FRESH_US + 1, SPEAKER_STATE_FRESH_US + 1)
                .is_none()
        );
    }

    #[test]
    fn pending_preparation_is_live_and_samples_can_only_be_taken_once() {
        let ticket = fixture(1, 100, false);
        assert!(ticket.live(100));
        assert!(ticket.take_ready(100).is_none());
        assert!(ticket.live(100));
        ticket
            .shared
            .lease_until_us
            .store(500_100, Ordering::Release);
        *ticket.shared.samples.lock().unwrap() = Some(vec![100; 480]);
        ticket.shared.ready.store(true, Ordering::Release);
        assert_eq!(ticket.take_ready(100).unwrap(), vec![100; 480]);
        assert!(ticket.take_ready(100).is_none());
        assert!(ticket.live(100));
    }

    #[test]
    fn original_start_is_exact_and_started_is_idempotent_without_extending_finish() {
        let ticket = fixture(1, 10_000_000, true);
        assert_eq!(ticket.output_lease_us(10_000_000), Some(1));
        assert!(ticket.started(10_000_000));
        assert!(ticket.started(10_000_001));
        assert_eq!(ticket.shared.offer.finish_before_us, 70_000_001);
        let late = fixture(1, 10_000_001, true);
        assert!(!late.started(10_000_001));
        assert!(!late.live(10_000_001));
        let finishing = fixture(1, 70_000_000, true);
        finishing.shared.started.store(true, Ordering::Release);
        assert_eq!(finishing.output_lease_us(70_000_000), Some(1));
        assert!(!finishing.may_play(70_000_001));
    }

    #[test]
    fn output_permits_are_capped_by_both_lease_and_monitor_freshness() {
        let ticket = fixture(1, 1000, true);
        assert_eq!(ticket.output_lease_us(1000), Some(CACHE_FRESH_US));
        ticket.shared.lease_until_us.store(1005, Ordering::Release);
        assert_eq!(ticket.output_lease_us(1000), Some(5));
        assert!(!ticket.may_play(1005));
        ticket
            .shared
            .lease_until_us
            .store(501_000, Ordering::Release);
        assert!(!ticket.may_play(1005), "an expired lease is terminal");
    }

    #[test]
    fn mute_loss_and_worker_failure_never_revive_an_admitted_ticket() {
        let ticket = fixture(1, 1000, true);
        ticket
            .shared
            .speaker
            .cache
            .lock()
            .unwrap()
            .observe(Some(state(1010, 3, 1)), 1010);
        assert!(!ticket.live(1010));
        assert!(!ticket.may_play(1010));
        let failed = fixture(1, 1000, true);
        failed.shared.running.worker.store(false, Ordering::Release);
        assert!(!failed.live(1000));
        failed.shared.running.worker.store(true, Ordering::Release);
        assert!(!failed.live(1000));
    }

    #[test]
    fn completion_is_only_recorded_after_start_and_drop_cancels_without_a_lock() {
        let ticket = fixture(1, 1000, true);
        ticket.completed();
        assert_eq!(ticket.shared.terminal.load(Ordering::Acquire), ACTIVE);
        assert!(ticket.started(1000));
        ticket.completed();
        let shared = ticket.shared.clone();
        drop(ticket);
        assert_eq!(shared.terminal.load(Ordering::Acquire), COMPLETED);
        let cancelled = fixture(1, 1000, true);
        let shared = cancelled.shared.clone();
        let _held = shared.samples.lock().unwrap();
        drop(cancelled);
        assert_eq!(shared.terminal.load(Ordering::Acquire), CANCELLED);
    }

    struct Home(PathBuf);
    impl Home {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "lamp-ra-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn process_factory_reuses_the_same_workers_after_a_session_releases_its_arc() {
        let home = Home::new();
        let slot = OnceLock::new();
        let made = AtomicUsize::new(0);
        let first = cached(&slot, || {
            made.fetch_add(1, Ordering::Relaxed);
            Service::new(
                Bus::new(home.0.join("bus"))?,
                home.0.clone(),
                "fixture-host".into(),
            )
        })
        .unwrap();
        let identity = Arc::as_ptr(&first);
        drop(first);
        let next = cached(&slot, || panic!("a new session must not make more workers")).unwrap();
        assert_eq!(Arc::as_ptr(&next), identity);
        assert!(!next.running.stop.load(Ordering::Acquire));
        assert_eq!(made.load(Ordering::Acquire), 1);
    }

    #[test]
    fn process_factory_keeps_construction_failure_instead_of_retrying_each_session() {
        let slot = OnceLock::new();
        let first = cached(&slot, || Err(io::Error::other("unavailable fixture")))
            .err()
            .unwrap();
        let next = cached(&slot, || {
            panic!("failed configuration is fixed for the process")
        })
        .err()
        .unwrap();
        assert_eq!(first.to_string(), "unavailable fixture");
        assert_eq!(next.to_string(), first.to_string());
    }

    struct Peer {
        path: PathBuf,
        operations: Receiver<RequestedAudioOperation>,
        deny_renew: Arc<AtomicBool>,
        stop: Arc<AtomicBool>,
        join: Option<JoinHandle<()>>,
    }
    impl Peer {
        fn new(home: &Path, gate: Option<Receiver<()>>) -> Self {
            let path = home.join("call.sock");
            let listener = UnixListener::bind(&path).unwrap();
            let (send, operations) = mpsc::channel();
            let stop = Arc::new(AtomicBool::new(false));
            let deny_renew = Arc::new(AtomicBool::new(false));
            let join = {
                let (stop, deny_renew) = (stop.clone(), deny_renew.clone());
                thread::spawn(move || {
                    for stream in listener.incoming() {
                        let mut stream = stream.unwrap();
                        if stop.load(Ordering::Acquire) {
                            break;
                        }
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        stream
                            .set_write_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut line = String::new();
                        if BufReader::new((&stream).take(8192))
                            .read_line(&mut line)
                            .is_err()
                        {
                            continue;
                        }
                        let operation: RequestedAudioOperation =
                            serde_json::from_str(&line).unwrap();
                        send.send(operation.clone()).unwrap();
                        let result = match operation {
                            RequestedAudioOperation::AudioAuthorize { offer, phase } => {
                                if phase == RequestedAudioPhase::Start
                                    && let Some(gate) = &gate
                                {
                                    gate.recv_timeout(Duration::from_secs(2)).unwrap();
                                }
                                if phase == RequestedAudioPhase::Renew
                                    && deny_renew.load(Ordering::Acquire)
                                {
                                    serde_json::json!({"ok":false,"error":"revoked"})
                                } else {
                                    serde_json::json!({"ok":true,"result":RequestedAudioLease {
                                        skills_boot: offer.skills_boot, definition_generation: offer.definition_generation,
                                        offer_id: offer.offer_id,
                                        until_us: (lamp_ipc::monotonic_us() + REQUESTED_AUDIO_LEASE_US).min(offer.finish_before_us),
                                    }})
                                }
                            }
                            RequestedAudioOperation::AudioFinish { .. } => {
                                serde_json::json!({"ok":true,"result":{"finished":true}})
                            }
                        };
                        let _ = writeln!(stream, "{result}");
                    }
                })
            };
            Self {
                path,
                operations,
                deny_renew,
                stop,
                join: Some(join),
            }
        }
        fn next(&self) -> RequestedAudioOperation {
            self.operations
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
        }
    }
    impl Drop for Peer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            let _ = UnixStream::connect(&self.path);
            if let Some(join) = self.join.take() {
                join.join().unwrap();
            }
        }
    }

    fn recording(home: &Path) -> RequestedAudioOffer {
        let mut out = Cursor::new(Vec::new());
        {
            let mut wav = hound::WavWriter::new(
                &mut out,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 24_000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for _ in 0..480 {
                wav.write_sample(100i16).unwrap();
            }
            wav.finalize().unwrap();
        }
        let path = home.join("recording.wav");
        fs::write(&path, out.into_inner()).unwrap();
        let checked = super::super::read_sound(&path).unwrap();
        let mut offer = offer(lamp_ipc::monotonic_us());
        offer.asset = path.to_str().unwrap().into();
        offer.sha256 = checked.sha256;
        offer
    }

    fn service(home: &Path) -> (Service, lamp_bus::Publisher) {
        let bus = Bus::new(home.join("bus")).unwrap();
        let mut publisher = bus
            .publisher_declared(SpeakerState::TOPIC, &SpeakerState::declaration(), "actions")
            .unwrap();
        let mut sample = state(lamp_ipc::monotonic_us(), 1, 0);
        sample.session_id = publisher.boot().into();
        publisher.publish_json(sample.stamp.at_us, &sample).unwrap();
        let service = Service::new(bus, home.to_owned(), "fixture-host".into()).unwrap();
        assert!(service.wait_cached(Duration::from_secs(2)));
        (service, publisher)
    }

    #[test]
    fn real_worker_loads_renews_and_finishes_only_after_marked_retirement() {
        let home = Home::new();
        let peer = Peer::new(&home.0, None);
        let (service, _publisher) = service(&home.0);
        for id in [1, 2] {
            let mut offered = recording(&home.0);
            offered.offer_id = id;
            offered.context.request = id;
            offered.context.call_id = format!("call-{id}");
            let ticket = service.offer(offered.clone()).unwrap();
            assert_eq!(
                peer.next(),
                RequestedAudioOperation::AudioAuthorize {
                    offer: offered.clone(),
                    phase: RequestedAudioPhase::Start,
                },
                "offer {id}: expected Start"
            );
            assert!(ticket.wait_ready(Duration::from_secs(2)));
            assert_eq!(
                ticket.take_ready(lamp_ipc::monotonic_us()).unwrap(),
                vec![100; 480]
            );
            assert!(ticket.started(lamp_ipc::monotonic_us()));
            assert_eq!(
                peer.next(),
                RequestedAudioOperation::AudioAuthorize {
                    offer: offered.clone(),
                    phase: RequestedAudioPhase::Renew,
                },
                "offer {id}: expected Renew"
            );
            assert!(ticket.may_play(lamp_ipc::monotonic_us()));
            ticket.completed();
            drop(ticket);
            assert_eq!(
                peer.next(),
                RequestedAudioOperation::AudioFinish {
                    offer: offered,
                    outcome: RequestedAudioOutcome::Completed,
                },
                "offer {id}: expected Completed finish"
            );
            assert!(service.wait_idle(Duration::from_secs(2)));
        }
    }

    #[test]
    fn real_worker_cache_contention_cancels_before_start_and_never_revives() {
        let home = Home::new();
        let peer = Peer::new(&home.0, None);
        let offered = recording(&home.0);
        let mut ticket = fixture(offered.issued_us, offered.issued_us, false);
        Arc::get_mut(&mut ticket.shared).unwrap().offer = offered.clone();
        let shared = ticket.shared.clone();
        let held = shared.speaker.cache.lock().unwrap();
        let busy = Arc::new(Capacity::default());
        busy.active.store(true, Ordering::Release);
        let (send, receive) = mpsc::sync_channel(1);
        assert!(send.send(shared.clone()).is_ok());
        drop(send);
        // Run the actual worker with a valid asset and a cache held before
        // its first authorization check. A closed queue lets it terminate
        // after this ticket without any sleep or permission deadline change.
        let worker = {
            let socket = peer.path.clone();
            let running = shared.running.clone();
            let busy = busy.clone();
            thread::spawn(move || work(receive, &socket, &running, &busy))
        };
        assert_eq!(
            peer.next(),
            RequestedAudioOperation::AudioFinish {
                offer: offered.clone(),
                outcome: RequestedAudioOutcome::Cancelled,
            },
            "offer {}: contention must finish without Start",
            offered.offer_id
        );
        worker.join().unwrap();
        assert!(!busy.active.load(Ordering::Acquire));
        assert_eq!(shared.terminal.load(Ordering::Acquire), CANCELLED);
        assert!(!shared.ready.load(Ordering::Acquire));
        assert!(shared.samples.lock().unwrap().is_none());
        drop(held);

        // The cache can become fresh and audible again, but this ticket
        // remains terminal and cannot authorize or expose prepared samples.
        let now = lamp_ipc::monotonic_us();
        {
            let mut cache = shared.speaker.cache.lock().unwrap();
            cache.observe(Some(state(now, 2, 0)), now);
            assert!(cache.bind(now, now).is_some());
        }
        assert!(!ticket.live(now));
        assert!(ticket.take_ready(now).is_none());
        assert!(!ticket.started(now));
        assert!(!authorize(&peer.path, &shared, RequestedAudioPhase::Start));
        assert_eq!(shared.terminal.load(Ordering::Acquire), CANCELLED);
        assert!(!shared.ready.load(Ordering::Acquire));
        assert_eq!(peer.operations.try_recv(), Err(mpsc::TryRecvError::Empty));
    }

    #[test]
    fn rejected_baseline_releases_capacity_for_a_new_spoken_offer() {
        let home = Home::new();
        let (release, gate) = mpsc::channel();
        let peer = Peer::new(&home.0, Some(gate));
        let too_early = recording(&home.0);
        let (service, _publisher) = service(&home.0);
        assert!(
            service
                .speaker
                .cache
                .lock()
                .unwrap()
                .audible
                .as_ref()
                .unwrap()
                .valid_since_us
                > too_early.context.issued_us,
            "the first spoken call must precede the audible baseline"
        );
        assert!(service.offer(too_early).is_none());
        assert!(!service.busy.active.load(Ordering::Acquire));
        assert_eq!(peer.operations.try_recv(), Err(mpsc::TryRecvError::Empty));

        let mut offered = recording(&home.0);
        offered.offer_id = 2;
        offered.context.request = 2;
        offered.context.call_id = "call-2".into();
        let ticket = service.offer(offered.clone()).unwrap();
        assert_eq!(
            peer.next(),
            RequestedAudioOperation::AudioAuthorize {
                offer: offered.clone(),
                phase: RequestedAudioPhase::Start,
            },
            "only the later offer may reach Start"
        );
        drop(ticket);
        release.send(()).unwrap();
        assert_eq!(
            peer.next(),
            RequestedAudioOperation::AudioFinish {
                offer: offered,
                outcome: RequestedAudioOutcome::Cancelled,
            }
        );
        assert!(service.wait_idle(Duration::from_secs(2)));
        assert_eq!(peer.operations.try_recv(), Err(mpsc::TryRecvError::Empty));
    }

    #[test]
    fn drop_during_a_blocked_start_does_not_wait_or_accept_its_late_success() {
        let home = Home::new();
        let (release, gate) = mpsc::channel();
        let peer = Peer::new(&home.0, Some(gate));
        let (service, _publisher) = service(&home.0);
        let offered = recording(&home.0);
        let ticket = service.offer(offered.clone()).unwrap();
        assert!(matches!(
            peer.next(),
            RequestedAudioOperation::AudioAuthorize {
                phase: RequestedAudioPhase::Start,
                ..
            }
        ));
        // The gated reply keeps this ticket active while capacity is checked.
        // A second offer must be refused before it can inspect the cache.
        assert!(
            service.offer(offered).is_none(),
            "one active offer is the entire capacity"
        );
        let shared = ticket.shared.clone();
        let _held = shared.samples.lock().unwrap();
        // The peer cannot answer yet and this lock cannot be acquired. Drop
        // nevertheless completes synchronously, without waiting on either.
        drop(ticket);
        assert_eq!(shared.terminal.load(Ordering::Acquire), CANCELLED);
        release.send(()).unwrap();
        assert!(matches!(
            peer.next(),
            RequestedAudioOperation::AudioFinish {
                outcome: RequestedAudioOutcome::Cancelled,
                ..
            }
        ));
        assert!(!shared.ready.load(Ordering::Acquire));
    }

    #[test]
    fn digest_mismatch_and_revoked_renewal_end_without_reusing_a_grant() {
        for mismatch in [true, false] {
            let home = Home::new();
            let peer = Peer::new(&home.0, None);
            let (service, _publisher) = service(&home.0);
            let mut offered = recording(&home.0);
            if mismatch {
                offered.sha256 = "0".repeat(64);
            }
            peer.deny_renew.store(true, Ordering::Release);
            let ticket = service.offer(offered).unwrap();
            assert!(matches!(
                peer.next(),
                RequestedAudioOperation::AudioAuthorize {
                    phase: RequestedAudioPhase::Start,
                    ..
                }
            ));
            if !mismatch {
                assert!(ticket.wait_ready(Duration::from_secs(2)));
                assert!(ticket.started(lamp_ipc::monotonic_us()));
                assert!(matches!(
                    peer.next(),
                    RequestedAudioOperation::AudioAuthorize {
                        phase: RequestedAudioPhase::Renew,
                        ..
                    }
                ));
            }
            assert!(matches!(
                peer.next(),
                RequestedAudioOperation::AudioFinish {
                    outcome: RequestedAudioOutcome::Cancelled,
                    ..
                }
            ));
            assert!(!ticket.live(lamp_ipc::monotonic_us()));
            assert!(ticket.take_ready(lamp_ipc::monotonic_us()).is_none());
        }
    }
}
