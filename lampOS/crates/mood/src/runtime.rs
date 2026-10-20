//! One state owner and one bounded transport worker. Bus/filesystem stalls
//! cannot delay the reducer; stalled publication expires at the consumer.
use crate::{
    engine::{Engine, Event},
    input::{INPUTS, InputAdapter},
};
use jiff::Timestamp;
use lamp_bus::{Bus, Publisher, Subscriber};
use lamp_local_time::LocalTime;
use lamp_msgs::{ClockStamp, Diagnostic, Level, MoodState};
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

pub const TICK: Duration = Duration::from_millis(5);
pub const INPUT_QUEUE: usize = 64;
pub const DRAIN_PER_TOPIC: usize = 4;
pub const MAX_BATCH: usize = 128;
pub const SHUTDOWN_WAIT: Duration = Duration::from_millis(200);
const MIN_PUBLISH_US: u64 = 20_000;
const HEARTBEAT_US: u64 = 1_000_000;
const OUTPUT_MAX_AGE_US: u64 = 250_000;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

#[derive(Clone, Debug)]
pub struct Config {
    pub enabled: bool,
    pub bus_root: PathBuf,
    pub zone: Option<String>,
    pub clock_id: Option<String>,
    pub run_for: Option<Duration>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            bus_root: PathBuf::from(lamp_bus::DEFAULT_ROOT),
            zone: None,
            clock_id: None,
            run_for: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub now_us: u64,
    pub unix_ms: u64,
    pub local_minute: Option<u16>,
}

pub trait Clock: Send + Sync + 'static {
    fn clock_id(&self) -> &str;
    fn sample(&self) -> io::Result<Sample>;
}

pub struct SystemClock {
    id: String,
    zone: LocalTime,
}

impl SystemClock {
    pub fn new(zone: &str, given_id: Option<&str>) -> io::Result<Self> {
        let id = match given_id {
            Some(id) => id.to_owned(),
            None if cfg!(target_os = "linux") => {
                fs::read_to_string("/proc/sys/kernel/random/boot_id")?
                    .trim()
                    .to_owned()
            }
            None => return Err(invalid("supply --clock-id on this development host")),
        };
        ClockStamp {
            clock_id: id.clone(),
            at_us: 0,
            sequence: 0,
        }
        .check()
        .map_err(io::Error::other)?;
        // An IANA name, or `system`: the lamp's own zone, followed as it
        // changes (#1670, as Schedule and Skills take it).
        let zone = LocalTime::zone(zone)?;
        Ok(Self { id, zone })
    }
}

impl Clock for SystemClock {
    fn clock_id(&self) -> &str {
        &self.id
    }

    fn sample(&self) -> io::Result<Sample> {
        let tick = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
        let seconds =
            u64::try_from(tick.tv_sec).map_err(|_| invalid("negative monotonic clock"))?;
        let nanos = u64::try_from(tick.tv_nsec).map_err(|_| invalid("negative monotonic clock"))?;
        let now_us = seconds
            .checked_mul(1_000_000)
            .and_then(|value| value.checked_add(nanos / 1_000))
            .ok_or_else(|| invalid("monotonic clock overflow"))?;
        let wall = Timestamp::now();
        let unix_ms =
            u64::try_from(wall.as_millisecond()).map_err(|_| invalid("wall clock before 1970"))?;
        Ok(Sample {
            now_us,
            unix_ms,
            local_minute: Some(self.zone.minute_of_day(unix_ms)),
        })
    }
}

#[derive(Default)]
pub struct Counters {
    received: AtomicU64,
    rejected: AtomicU64,
    queue_dropped: AtomicU64,
    read_errors: AtomicU64,
    publish_errors: AtomicU64,
    diagnostic_errors: AtomicU64,
    published: AtomicU64,
    old_outputs: AtomicU64,
}

#[derive(Default, Debug, Serialize)]
pub struct Report {
    pub enabled: bool,
    pub steps: u64,
    pub accepted: u64,
    pub engine_rejected: u64,
    pub received: u64,
    pub rejected: u64,
    pub queue_dropped: u64,
    pub read_errors: u64,
    pub publish_errors: u64,
    pub diagnostic_errors: u64,
    pub published: u64,
    pub old_outputs: u64,
}

impl Report {
    fn finish(mut self, counts: &Counters) -> Self {
        self.received = counts.received.load(Ordering::Relaxed);
        self.rejected = counts.rejected.load(Ordering::Relaxed);
        self.queue_dropped = counts.queue_dropped.load(Ordering::Relaxed);
        self.read_errors = counts.read_errors.load(Ordering::Relaxed);
        self.publish_errors = counts.publish_errors.load(Ordering::Relaxed);
        self.diagnostic_errors = counts.diagnostic_errors.load(Ordering::Relaxed);
        self.published = counts.published.load(Ordering::Relaxed);
        self.old_outputs = counts.old_outputs.load(Ordering::Relaxed);
        self
    }
}

/// Poll returns at most MAX_BATCH metadata events. It must discard raw payloads
/// before returning. Only this worker may touch the bus or any persisted output.
pub trait Transport: Send + 'static {
    fn session_id(&self) -> &str;
    fn poll(&mut self, counts: &Counters) -> io::Result<Vec<Event>>;
    fn publish(&mut self, state: &MoodState, counts: &Counters) -> io::Result<()>;
}

struct BusTransport<C> {
    clock: Arc<C>,
    inputs: Vec<Subscriber>,
    adapter: InputAdapter,
    output: Publisher,
    diagnostic: Publisher,
    diagnostic_at: Option<u64>,
    // Fields drop in declaration order. Keep the lock until bus/socket Drops
    // finish, including when their filesystem cleanup itself cannot complete.
    _owner: File,
}

fn owner_lock(root: &Path) -> io::Result<File> {
    if !root.is_absolute() {
        return Err(invalid("bus root must be absolute"));
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o770)
        .create(root)?;
    let owner = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o660)
        .open(root.join("mood.lock"))?;
    owner.try_lock().map_err(|error| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("Mood already has an owner: {error}"),
        )
    })?;
    Ok(owner)
}

impl<C: Clock> BusTransport<C> {
    fn new(root: &Path, clock: Arc<C>) -> io::Result<Self> {
        let owner = owner_lock(root)?;
        let bus = Bus::new(root)?;
        let output = bus.publisher_declared(MoodState::TOPIC, &MoodState::declaration(), "mood")?;
        let diagnostic =
            bus.publisher_declared(Diagnostic::TOPIC, &Diagnostic::declaration(), "mood")?;
        let inputs = INPUTS
            .iter()
            .map(|spec| bus.subscribe_typed(spec.topic, "mood", spec.type_name))
            .collect::<io::Result<Vec<_>>>()?;
        let adapter = InputAdapter::new(clock.clock_id().to_owned()).map_err(io::Error::other)?;
        Ok(Self {
            _owner: owner,
            clock,
            inputs,
            adapter,
            output,
            diagnostic,
            diagnostic_at: None,
        })
    }
}

impl<C: Clock> Transport for BusTransport<C> {
    fn session_id(&self) -> &str {
        self.output.boot()
    }

    fn poll(&mut self, counts: &Counters) -> io::Result<Vec<Event>> {
        self.adapter.expire(self.clock.sample()?.now_us);
        let mut events = Vec::with_capacity(INPUTS.len() * DRAIN_PER_TOPIC);
        for (input, spec) in self.inputs.iter_mut().zip(INPUTS.iter()) {
            for _ in 0..DRAIN_PER_TOPIC {
                // recv may internally skip an invalid flood. Isolation keeps
                // the engine alive, but this is not a hard syscall deadline.
                let message = match input.recv(Duration::ZERO) {
                    Ok(Some(message)) => message,
                    Ok(None) => break,
                    Err(_) => {
                        counts.read_errors.fetch_add(1, Ordering::Relaxed);
                        break;
                    }
                };
                counts.received.fetch_add(1, Ordering::Relaxed);
                if !input
                    .declaration()
                    .is_some_and(|value| spec.accepts_declaration(value))
                {
                    counts.rejected.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                // Sample after receiving, not before an unbounded I/O call.
                let now = self.clock.sample()?;
                match self.adapter.ingest(&message, now.now_us, now.unix_ms) {
                    Ok(batch) if events.len() + batch.len() <= MAX_BATCH => events.extend(batch),
                    Ok(batch) => {
                        counts
                            .queue_dropped
                            .fetch_add(batch.len() as u64, Ordering::Relaxed);
                    }
                    Err(_) => {
                        counts.rejected.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            // Drain notices so this process does not accumulate diagnostics.
            let _ = input.notices();
        }
        Ok(events)
    }

    fn publish(&mut self, state: &MoodState, counts: &Counters) -> io::Result<()> {
        state.check().map_err(io::Error::other)?;
        self.output.publish_json(state.stamp.at_us, state)?;
        if self
            .diagnostic_at
            .is_none_or(|last| state.stamp.at_us.saturating_sub(last) >= HEARTBEAT_US)
        {
            let errors = counts
                .read_errors
                .load(Ordering::Relaxed)
                .saturating_add(counts.publish_errors.load(Ordering::Relaxed))
                .saturating_add(counts.diagnostic_errors.load(Ordering::Relaxed));
            let dropped = counts.queue_dropped.load(Ordering::Relaxed);
            let level = if errors > 0 || dropped > 0 {
                Level::Warn
            } else {
                Level::Ok
            };
            let diagnostic = Diagnostic::new(
                level,
                "mood",
                "transient character metadata; no input or output permission",
            )
            .with("received", counts.received.load(Ordering::Relaxed))
            .with("rejected", counts.rejected.load(Ordering::Relaxed))
            .with("queue_dropped", dropped)
            .with("io_errors", errors);
            if self
                .diagnostic
                .publish_json(state.stamp.at_us, &diagnostic)
                .is_err()
            {
                counts.diagnostic_errors.fetch_add(1, Ordering::Relaxed);
            }
            self.diagnostic_at = Some(state.stamp.at_us);
        }
        Ok(())
    }
}

fn send_batch(batch: Vec<Event>, sender: &SyncSender<Event>, counts: &Counters) -> bool {
    for event in batch.into_iter().take(MAX_BATCH) {
        match sender.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                counts.queue_dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
    true
}

fn changed(last: &MoodState, next: &MoodState) -> bool {
    last.asleep != next.asleep
        || last.attention != next.attention
        || (last.awake - next.awake).abs() >= 0.01
        || (last.energy - next.energy).abs() >= 0.01
        || (last.warmth - next.warmth).abs() >= 0.01
}

struct StopOnDrop(Arc<AtomicBool>);
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

/// Injected transport/clock runner used by deterministic and slow-I/O tests.
/// A shutdown timeout is an error: exit this process before starting another.
pub fn run<C: Clock, T: Transport>(
    config: &Config,
    clock: Arc<C>,
    mut transport: T,
    stop: Arc<AtomicBool>,
) -> io::Result<Report> {
    if !config.enabled {
        return Ok(Report::default());
    }
    let _stop_on_drop = StopOnDrop(stop.clone());
    let started = Instant::now();
    let sample = clock.sample()?;
    let mut engine = Engine::new(
        clock.clock_id().to_owned(),
        sample.now_us,
        sample.local_minute,
    )
    .map_err(io::Error::other)?;
    let session = transport.session_id().to_owned();
    let latest = Arc::new(Mutex::new(None::<MoodState>));
    let counts = Arc::new(Counters::default());
    let (sender, receiver) = mpsc::sync_channel(INPUT_QUEUE);
    let worker_latest = latest.clone();
    let worker_counts = counts.clone();
    let worker_stop = stop.clone();
    let worker_clock = clock.clone();
    let worker =
        thread::Builder::new()
            .name("mood-bus".into())
            .spawn(move || -> io::Result<()> {
                while !worker_stop.load(Ordering::Acquire) {
                    let batch = transport.poll(&worker_counts)?;
                    if worker_stop.load(Ordering::Acquire) {
                        break;
                    }
                    if !send_batch(batch, &sender, &worker_counts) {
                        break;
                    }
                    let output = worker_latest
                        .try_lock()
                        .ok()
                        .and_then(|mut slot| slot.take());
                    if let Some(state) = output {
                        let now = worker_clock.sample()?;
                        if state.stamp.at_us > now.now_us
                            || now.now_us - state.stamp.at_us > OUTPUT_MAX_AGE_US
                        {
                            worker_counts.old_outputs.fetch_add(1, Ordering::Relaxed);
                        } else if transport.publish(&state, &worker_counts).is_ok() {
                            worker_counts.published.fetch_add(1, Ordering::Relaxed);
                        } else {
                            worker_counts.publish_errors.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    thread::sleep(TICK);
                }
                Ok(())
            })?;
    let mut report = Report {
        enabled: true,
        ..Report::default()
    };
    let mut last: Option<MoodState> = None;
    let mut sequence = 0u64;
    let mut failure = None;
    let loop_result = (|| -> io::Result<()> {
        while !stop.load(Ordering::Acquire)
            && config.run_for.is_none_or(|limit| started.elapsed() < limit)
        {
            if worker.is_finished() {
                failure = Some(io::Error::other("Mood transport worker stopped"));
                break;
            }
            for event in receiver.try_iter().take(INPUT_QUEUE) {
                let now = clock.sample()?;
                engine
                    .advance(now.now_us, now.local_minute)
                    .map_err(io::Error::other)?;
                match engine.apply(now.now_us, event) {
                    Ok(_) => report.accepted = report.accepted.saturating_add(1),
                    Err(_) => report.engine_rejected = report.engine_rejected.saturating_add(1),
                }
            }
            let now = clock.sample()?;
            engine
                .advance(now.now_us, now.local_minute)
                .map_err(io::Error::other)?;
            report.steps = report.steps.saturating_add(1);
            let next_sequence = sequence
                .checked_add(1)
                .ok_or_else(|| io::Error::other("Mood sequence exhausted"))?;
            let state = engine
                .snapshot(now.now_us, next_sequence, &session)
                .map_err(io::Error::other)?;
            let should_publish = last.as_ref().is_none_or(|old| {
                let age = now.now_us.saturating_sub(old.stamp.at_us);
                age >= HEARTBEAT_US || (age >= MIN_PUBLISH_US && changed(old, &state))
            });
            if should_publish && let Ok(mut slot) = latest.try_lock() {
                sequence = next_sequence;
                *slot = Some(state.clone());
                last = Some(state);
            }
            thread::sleep(TICK);
        }
        Ok(())
    })();
    stop.store(true, Ordering::Release);
    drop(receiver);
    let shutdown = Instant::now();
    while !worker.is_finished() && shutdown.elapsed() < SHUTDOWN_WAIT {
        thread::sleep(TICK);
    }
    if !worker.is_finished() {
        return Err(io::Error::other(
            "Mood bus I/O did not stop; exit this process before restarting",
        ));
    }
    worker
        .join()
        .map_err(|_| io::Error::other("Mood bus worker panicked"))??;
    loop_result?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(report.finish(&counts))
}

/// The production entry point returns before all clock/filesystem/bus work
/// unless explicitly enabled. The enabled owner has no actuator handles.
/// Clock/zone and bus attachment perform synchronous startup I/O; the worker
/// shutdown bound begins after attachment, not during those startup syscalls.
pub fn serve(config: Config, stop: Arc<AtomicBool>) -> io::Result<Report> {
    if !config.enabled {
        return Ok(Report::default());
    }
    let zone = config
        .zone
        .as_deref()
        .ok_or_else(|| invalid("--zone is required when enabled"))?;
    let clock = Arc::new(SystemClock::new(zone, config.clock_id.as_deref())?);
    let transport = BusTransport::new(&config.bus_root, clock.clone())?;
    run(&config, clock, transport, stop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_failure_does_not_reclassify_a_successful_mood_publication() {
        let root = tempfile::Builder::new()
            .prefix("mod")
            .tempdir_in("/tmp")
            .unwrap();
        let clock = Arc::new(SystemClock::new("UTC", Some("diagnostic-test")).unwrap());
        let mut transport = BusTransport::new(root.path(), clock.clone()).unwrap();
        let obstruction = root.path().join(Diagnostic::TOPIC).join("subs");
        if obstruction.is_dir() {
            fs::remove_dir_all(&obstruction).unwrap();
        }
        fs::write(obstruction, b"authored non-directory obstruction").unwrap();
        let now = clock.sample().unwrap();
        let engine = Engine::new(clock.clock_id().into(), now.now_us, now.local_minute).unwrap();
        let state = engine
            .snapshot(now.now_us, 1, transport.session_id())
            .unwrap();
        let counts = Counters::default();
        transport.publish(&state, &counts).unwrap();
        assert_eq!(counts.diagnostic_errors.load(Ordering::Relaxed), 1);
        let retained: MoodState = Bus::new(root.path())
            .unwrap()
            .latest(MoodState::TOPIC)
            .unwrap()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(retained, state);
    }
}
