//! Runner-side physical backend: drives one `lamp-session` per attempt through
//! an operator-supplied command, maps Lamp time onto the runner clock with
//! ping/pong, and starts cached stimuli on the explicit iMac speakers.
//!
//! In serve mode one attempt is one long-lived `lamp-voice serve` runtime
//! carrying a whole multi-turn conversation. Each stimulus is prepared
//! (loaded, output opened in silence) before its trigger wait and started by
//! one original expiring grant that the player enforces at its first source
//! copy. The attempt ends with the relay's stop: SIGTERM, bounded grace,
//! forced kill, each retained separately.
//!
//! Runner software times (start requests, receipts) only locate stimuli; they
//! are not acoustic onsets. Room audio comes from a separate recorder.
use crate::{
    Result,
    events::{ClockDomain, Cue, EventKind, EventSource, RuntimeEvent, from_trace, parse_cue},
    import::load_room_evidence,
    invalid,
    physical::{
        fake_lamp::ROOM_MAX_BYTES,
        framing::{Frame, LineFramer, STREAM_LIMIT},
        protocol::{MAX_LINE_BYTES, MAX_SESSION_BYTES, RunnerLine, SessionLine},
    },
    plan::{ProviderKind, Scenario, Step},
    playback::PlaybackHistory,
    record::TriggerObservation,
    record::{ClockMapping, StimulusSource, Stratum},
    runner::{
        AttemptContext, Backend, Begin, Finished, Injection, MAX_TRIGGER_LATENESS_US, Withheld,
    },
    stimulus::{AssetIndex, SceneTiming, StimulusCatalog},
};
use lamp_ipc::{monotonic_ns, monotonic_us};
use lamp_observer::playback::GrantInput;
use lamp_turn_owner::MonoTime;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::net::UnixDatagram,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// Starts one stimulus. Implementations must return promptly; playback runs
/// on its own thread and its delivery report is collected at finish.
pub trait Player {
    fn start(
        &mut self,
        wav: &Path,
        timing: &SceneTiming,
        report_directory: &Path,
        cancel_file: &Path,
    ) -> Result<JoinHandle<Value>>;
    fn describe(&self) -> Value;
    /// Two-phase start: begin loading and verifying the WAV and opening the
    /// output in silence, without waiting. Its ready receipt arrives on
    /// `Preparing::ready`; the caller keeps consuming the session meanwhile.
    /// The stimulus starts only on one original grant through
    /// `PreparedStart::grant`. `None` when this player has no prepared start.
    fn prepare(
        &mut self,
        _wav: &Path,
        _timing: &SceneTiming,
        _report_directory: &Path,
        _cancel_file: &Path,
        _max_wait_ms: u32,
    ) -> Result<Option<Preparing>> {
        Ok(None)
    }
}

/// A two-phase player still preparing: its ready receipt, once published,
/// arrives on `ready`.
pub struct Preparing {
    pub ready: Receiver<Value>,
    pub grant: SyncSender<GrantInput>,
    pub handle: JoinHandle<Value>,
}

/// An armed two-phase player: its ready receipt, its grant channel and the
/// thread that returns its delivery report.
pub struct PreparedStart {
    pub ready: Value,
    pub grant: SyncSender<GrantInput>,
    pub handle: JoinHandle<Value>,
}

/// How long a prepared player may take to publish readiness. It runs before
/// the trigger wait, never between readiness and the start.
const PREPARE_READY_BOUND: Duration = Duration::from_secs(10);

/// How often a wait for a prepared player returns to consume the session.
const PREPARE_POLL: Duration = Duration::from_millis(2);

/// Wait for a prepared player's ready receipt within the unchanged 10 s
/// bound, calling `idle` between short polls so the session transport keeps
/// being consumed: a cue is judged when it arrives, not after preparation.
/// A player that ends first or stays silent is stopped and its failure
/// returned.
fn await_ready(preparing: Preparing, mut idle: impl FnMut()) -> Result<PreparedStart> {
    let Preparing {
        ready,
        grant,
        handle,
    } = preparing;
    let deadline = Instant::now() + PREPARE_READY_BOUND;
    let received = loop {
        match ready.recv_timeout(PREPARE_POLL) {
            Err(RecvTimeoutError::Timeout) if Instant::now() < deadline => idle(),
            other => break other,
        }
    };
    match received {
        Ok(ready) => Ok(PreparedStart {
            ready,
            grant,
            handle,
        }),
        Err(_) => {
            let _ = grant.try_send(GrantInput::Closed);
            drop(grant);
            let report = if handle.is_finished() {
                handle
                    .join()
                    .unwrap_or_else(|_| json!({"status": "panicked"}))
            } else {
                json!({"status": "no ready receipt within 10 s; detached"})
            };
            Err(invalid(&format!(
                "prepared player not ready: {} {}",
                report["status"], report["error"]
            )))
        }
    }
}

/// Explicit fixtures replace readiness polling with completed participant
/// work while retaining the same real ten-second preparation watchdog.
fn await_ready_fixture(
    preparing: Preparing,
    mut idle: impl FnMut(Instant) -> Result<()>,
) -> Result<PreparedStart> {
    let Preparing {
        ready,
        grant,
        handle,
    } = preparing;
    let deadline = Instant::now() + PREPARE_READY_BOUND;
    let failure = loop {
        match ready.try_recv() {
            Ok(ready) => {
                return Ok(PreparedStart {
                    ready,
                    grant,
                    handle,
                });
            }
            Err(TryRecvError::Disconnected) => break "ready channel closed".to_owned(),
            Err(TryRecvError::Empty) if Instant::now() >= deadline => {
                break "no ready receipt within 10 s".to_owned();
            }
            Err(TryRecvError::Empty) => {
                if let Err(error) = idle(deadline) {
                    break error.to_string();
                }
            }
        }
    };
    let _ = grant.try_send(GrantInput::Closed);
    drop(grant);
    // A report-ready acknowledgement is not the JoinHandle's completion.
    let report = if wait_fixture_thread(&handle, deadline).is_ok() {
        handle
            .join()
            .unwrap_or_else(|_| json!({"status":"panicked"}))
    } else {
        json!({"status":"no ready receipt within 10 s; detached"})
    };
    Err(invalid(&format!(
        "prepared player not ready: {} {} ({failure})",
        report["status"], report["error"]
    )))
}

fn wait_fixture_thread(handle: &JoinHandle<Value>, deadline: Instant) -> Result<()> {
    while !handle.is_finished() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                invalid(
                    "fixture player report did not finish its owned thread within the host bound",
                )
            })?;
        thread::sleep(remaining.min(Duration::from_millis(1)));
    }
    Ok(())
}

/// Cached-WAV playback through `lamp-observer` on the explicit iMac speakers.
pub struct ObserverPlayer {
    pub output_device: String,
}

impl Player for ObserverPlayer {
    fn start(
        &mut self,
        wav: &Path,
        _: &SceneTiming,
        report_directory: &Path,
        cancel_file: &Path,
    ) -> Result<JoinHandle<Value>> {
        lamp_observer::playback::validate_output_name(&self.output_device)?;
        let options = lamp_observer::playback::PlayOptions {
            output_device: self.output_device.clone(),
            wav: wav.to_path_buf(),
            output: report_directory.to_path_buf(),
            cancel_file: Some(cancel_file.to_path_buf()),
            prepared_start_ms: None,
        };
        Ok(thread::spawn(move || {
            match lamp_observer::playback::play(&options) {
                Ok(report) => report,
                Err(error) => {
                    json!({"valid": false, "status": "failed", "error": error.to_string()})
                }
            }
        }))
    }
    fn describe(&self) -> Value {
        json!({"kind": "lamp-observer playback", "output_device": self.output_device,
            "boundary": "start request and CoreAudio callback times; not acoustic onset"})
    }
    /// In-process `play_prepared`: no process is created at the grant.
    fn prepare(
        &mut self,
        wav: &Path,
        _: &SceneTiming,
        report_directory: &Path,
        cancel_file: &Path,
        max_wait_ms: u32,
    ) -> Result<Option<Preparing>> {
        lamp_observer::playback::validate_output_name(&self.output_device)?;
        let options = lamp_observer::playback::PlayOptions {
            output_device: self.output_device.clone(),
            wav: wav.to_path_buf(),
            output: report_directory.to_path_buf(),
            cancel_file: Some(cancel_file.to_path_buf()),
            prepared_start_ms: Some(max_wait_ms),
        };
        let (grant, inputs) = mpsc::sync_channel(lamp_observer::playback::MAX_GRANT_LINES + 1);
        let (published, ready) = mpsc::sync_channel(1);
        let handle = thread::spawn(move || {
            let mut publish = |receipt: &Value| {
                published
                    .try_send(receipt.clone())
                    .map_err(std::io::Error::other)
            };
            let io = lamp_observer::playback::PreparedIo {
                ready: &mut publish,
                inputs,
            };
            match lamp_observer::playback::play_prepared(&options, io) {
                Ok(report) => report,
                Err(error) => {
                    json!({"valid": false, "status": "failed", "error": error.to_string()})
                }
            }
        });
        Ok(Some(Preparing {
            ready,
            grant,
            handle,
        }))
    }
}

/// Direct-human prompts: at the trigger the operator console shows the line a
/// person in the room should say now. Nothing is played and no sound is made.
/// The prompt time is the only software boundary; reaction time and the actual
/// speech onset must come from room audio.
pub struct HumanPrompter {
    pub speaker_label: String,
}

impl Player for HumanPrompter {
    fn start(
        &mut self,
        _: &Path,
        timing: &SceneTiming,
        _: &Path,
        _: &Path,
    ) -> Result<JoinHandle<Value>> {
        let shown = monotonic_us();
        let lines: Vec<String> = timing.clips.iter().map(|clip| clip.text.clone()).collect();
        eprintln!(
            "\n>>> {} SAY NOW: {}\n",
            self.speaker_label,
            lines.join(" ... ")
        );
        let label = self.speaker_label.clone();
        Ok(thread::spawn(move || {
            json!({"valid": true, "status": "prompted_direct_human", "speaker_label": label,
                "speech_delivery_verified": false,
                "prompt_shown_host_monotonic_ns": shown * 1000, "lines": lines,
                "boundary": "prompt shown; human speech onset is later and unmeasured in software"})
        }))
    }
    fn describe(&self) -> Value {
        json!({"kind": "direct human prompts", "speaker_label": self.speaker_label,
            "note": "no playback; speech timing comes only from room audio"})
    }
}

/// Loopback player for the fake lamp-voice: sends the stimulus timing to the
/// fake room socket instead of producing sound.
pub struct FakeRoomPlayer {
    pub room: PathBuf,
}

impl FakeRoomPlayer {
    /// Use the backend's explicit clock for the immediate delivery report.
    /// The callback only reads time; it must not advance a fixture or wait.
    /// It is sampled before sending, never from the report's worker thread.
    pub fn with_clock(self, now: impl Fn() -> MonoTime) -> impl Player {
        ClockedFakeRoomPlayer { player: self, now }
    }

    fn start_with_clock(
        &self,
        timing: &SceneTiming,
        now: impl FnOnce() -> MonoTime,
    ) -> Result<JoinHandle<Value>> {
        let bytes = serde_json::to_vec(timing)?;
        if bytes.len() > ROOM_MAX_BYTES {
            return Err(invalid("stimulus timing exceeds the fake room datagram"));
        }
        let requested_ns = now()
            .as_micros()
            .checked_mul(1000)
            .ok_or_else(|| invalid("fake room delivery time overflows nanoseconds"))?;
        UnixDatagram::unbound()?.send_to(&bytes, &self.room)?;
        Ok(thread::spawn(
            move || json!({"valid": true, "status": "fake_room_delivery", "start_requested_host_monotonic_ns": requested_ns}),
        ))
    }
}

impl Player for FakeRoomPlayer {
    fn start(
        &mut self,
        _: &Path,
        timing: &SceneTiming,
        _: &Path,
        _: &Path,
    ) -> Result<JoinHandle<Value>> {
        self.start_with_clock(timing, || MonoTime::from_micros(monotonic_us()))
    }
    fn describe(&self) -> Value {
        json!({"kind": "fake room datagrams", "note": "loopback test; no audio"})
    }
}

struct ClockedFakeRoomPlayer<C> {
    player: FakeRoomPlayer,
    now: C,
}

impl<C: Fn() -> MonoTime> Player for ClockedFakeRoomPlayer<C> {
    fn start(
        &mut self,
        _: &Path,
        timing: &SceneTiming,
        _: &Path,
        _: &Path,
    ) -> Result<JoinHandle<Value>> {
        self.player.start_with_clock(timing, &self.now)
    }

    fn describe(&self) -> Value {
        self.player.describe()
    }
}

/// A deliberate fault in one prepared fake start, by 1-based prepare order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FakePlayerFault {
    /// The first copy is reached this long after the grant arrives.
    LateFirstCopy { delay_ms: u32 },
    /// The player never becomes ready.
    PrepareFails,
    /// The ready receipt is published this long after the fake runtime
    /// started (its room socket appeared), as when the observer's output is
    /// slow to open; a test can place the runtime's startup cues inside it.
    SlowPrepare { delay_ms: u32 },
}

/// Loopback two-phase player for the fake lamp-voice. It mirrors the
/// observer's prepared contract (ready receipt, one original grant for its
/// attempt, CLOCK_MONOTONIC expiry checked with a fresh sample immediately
/// before the first copy) and "copies" by sending the stimulus timing to the
/// fake room socket. It is not the observer and plays no audio.
pub struct FakePreparedRoomPlayer {
    pub room: PathBuf,
    pub faults: Vec<(usize, FakePlayerFault)>,
    pub prepared: usize,
}

/// Wait operations for the one shared prepared-fake body. The ordinary
/// implementation keeps its original host waits; a controlled fixture may
/// park only at these completed-work boundaries.
pub(super) trait PreparedWait: Send {
    fn begin(&mut self) -> Result<()> {
        Ok(())
    }
    fn slow_prepare(&mut self, room: &Path, cancel: &Path, delay_ms: u32) -> Result<()>;
    fn receive_grant(
        &mut self,
        inputs: &Receiver<GrantInput>,
        cancel: &Path,
        max_wait_ms: u32,
    ) -> Result<std::result::Result<GrantInput, RecvTimeoutError>>;
    fn delay_copy(&mut self, cancel: &Path, delay_ms: u32) -> Result<()>;
    fn delivered(&mut self) -> Result<()> {
        Ok(())
    }
}

struct HostPreparedWait;

impl PreparedWait for HostPreparedWait {
    fn slow_prepare(&mut self, room: &Path, _: &Path, delay_ms: u32) -> Result<()> {
        let until = monotonic_us() + 15_000_000;
        while !room.exists() && monotonic_us() < until {
            thread::sleep(Duration::from_millis(2));
        }
        thread::sleep(Duration::from_millis(u64::from(delay_ms)));
        Ok(())
    }

    fn receive_grant(
        &mut self,
        inputs: &Receiver<GrantInput>,
        _: &Path,
        max_wait_ms: u32,
    ) -> Result<std::result::Result<GrantInput, RecvTimeoutError>> {
        Ok(inputs.recv_timeout(Duration::from_millis(u64::from(max_wait_ms))))
    }

    fn delay_copy(&mut self, _: &Path, delay_ms: u32) -> Result<()> {
        thread::sleep(Duration::from_millis(u64::from(delay_ms)));
        Ok(())
    }
}

impl FakePreparedRoomPlayer {
    /// Use one explicit clock for preparation, grant receipt and first copy.
    /// Reads must be pure; clones must read the same clock domain. The worker
    /// owns a clone, while resource waits, fault delays and joins use host time.
    pub fn with_clock(
        self,
        now: impl Fn() -> MonoTime + Clone + Send + Sync + 'static,
    ) -> impl Player {
        ClockedFakePreparedRoomPlayer { player: self, now }
    }

    fn prepare_with_ns_clock(
        &mut self,
        wav: &Path,
        timing: &SceneTiming,
        cancel_file: &Path,
        max_wait_ms: u32,
        now_ns: impl Fn(&'static str) -> Result<u64> + Send + Sync + 'static,
    ) -> Result<Option<Preparing>> {
        self.prepare_with_wait(wav, timing, cancel_file, max_wait_ms, now_ns, |_| {
            Ok(Box::new(HostPreparedWait))
        })
    }

    pub(super) fn prepare_with_wait(
        &mut self,
        wav: &Path,
        timing: &SceneTiming,
        cancel_file: &Path,
        max_wait_ms: u32,
        now_ns: impl Fn(&'static str) -> Result<u64> + Send + Sync + 'static,
        wait: impl FnOnce(&str) -> Result<Box<dyn PreparedWait>>,
    ) -> Result<Option<Preparing>> {
        self.prepared += 1;
        let number = self.prepared;
        let fault = self
            .faults
            .iter()
            .find(|(n, _)| *n == number)
            .map(|(_, f)| *f);
        let bytes = serde_json::to_vec(timing)?;
        if bytes.len() > ROOM_MAX_BYTES {
            return Err(invalid("stimulus timing exceeds the fake room datagram"));
        }
        let source_sha256 = lamp_sound_fixtures::hash(&fs::read(wav)?);
        let room = self.room.clone();
        let cancel_file = cancel_file.to_path_buf();
        let attempt_id = format!("fake-{number}-{}", now_ns("attempt ID")?);
        let mut wait = wait(&attempt_id)?;
        let (grant, inputs) = mpsc::sync_channel::<GrantInput>(9);
        let (published, ready) = mpsc::sync_channel(1);
        let handle = thread::spawn(move || {
            let preparing = wait.begin().and_then(|()| {
                if let Some(FakePlayerFault::SlowPrepare { delay_ms }) = fault {
                    wait.slow_prepare(&room, &cancel_file, delay_ms)
                } else {
                    Ok(())
                }
            });
            if let Err(error) = preparing {
                return json!({"attempt_id": attempt_id, "valid": false,
                    "scope": "fake prepared start; no audio",
                    "counters": {"source_started_host_ns": 0},
                    "status": "failed", "error": error.to_string()});
            }
            let ready_ns = match now_ns("readiness") {
                Ok(now) => now,
                Err(error) => {
                    return json!({"attempt_id": attempt_id, "valid": false,
                        "scope": "fake prepared start; no audio",
                        "counters": {"source_started_host_ns": 0},
                        "status": "failed", "error": error.to_string()});
                }
            };
            let mut report = json!({"attempt_id": attempt_id, "valid": false,
                "scope": "fake prepared start; no audio", "counters": {"source_started_host_ns": 0}});
            if fault == Some(FakePlayerFault::PrepareFails) {
                report["status"] = json!("failed");
                report["error"] = json!("fake prepare failure");
                return report;
            }
            let receipt = json!({"schema": 1, "kind": "prepared_ready", "attempt_id": attempt_id,
                "ready_monotonic_ns": ready_ns, "clock": "CLOCK_MONOTONIC",
                "source_sha256": source_sha256, "max_wait_ms": max_wait_ms});
            report["prepared_start"] = json!({"ready": receipt});
            if published.try_send(receipt).is_err() {
                report["status"] = json!("ready_not_published");
                return report;
            }
            let input = match wait.receive_grant(&inputs, &cancel_file, max_wait_ms) {
                Ok(input) => input,
                Err(error) => {
                    report["status"] = json!("failed");
                    report["error"] = json!(error.to_string());
                    return report;
                }
            };
            let received = match now_ns("grant receipt") {
                Ok(now) => now,
                Err(error) => {
                    report["status"] = json!("failed");
                    report["error"] = json!(error.to_string());
                    return report;
                }
            };
            let decision = match input {
                Err(_) => Err("grant_timeout"),
                Ok(GrantInput::Closed) => Err("stopped_before_grant"),
                Ok(GrantInput::Malformed) => Err("grant_refused"),
                Ok(GrantInput::Line(line)) => {
                    let grant: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
                    let expires = grant["expires_monotonic_ns"].as_u64();
                    match expires {
                        _ if grant["schema"] != 1 || grant["kind"] != "start" => {
                            Err("grant_refused")
                        }
                        _ if grant["attempt_id"] != attempt_id => Err("grant_refused"),
                        Some(expires)
                            if expires > received
                                && expires - received
                                    <= lamp_observer::playback::MAX_GRANT_HORIZON_NS =>
                        {
                            Ok(expires)
                        }
                        _ => Err("grant_refused"),
                    }
                }
            };
            let expires = match decision {
                Ok(expires) => expires,
                Err(status) => {
                    report["status"] = json!(status);
                    return report;
                }
            };
            report["prepared_start"]["grant"] = json!({"decision": "start",
                "expires_monotonic_ns": expires, "received_monotonic_ns": received});
            if let Some(FakePlayerFault::LateFirstCopy { delay_ms }) = fault
                && let Err(error) = wait.delay_copy(&cancel_file, delay_ms)
            {
                report["status"] = json!("failed");
                report["error"] = json!(error.to_string());
                return report;
            }
            // The fresh sample at the first copy decides; equality refuses.
            let copy_ns = match now_ns("first copy") {
                Ok(now) => now,
                Err(error) => {
                    report["status"] = json!("failed");
                    report["error"] = json!(error.to_string());
                    return report;
                }
            };
            if copy_ns >= expires {
                report["status"] = json!("start_expired");
                report["first_copy_attempt_host_ns"] = json!(copy_ns);
                return report;
            }
            if cancel_file.exists() {
                report["status"] = json!("cancelled_before_first_copy");
                return report;
            }
            let sent = UnixDatagram::unbound().and_then(|socket| socket.send_to(&bytes, &room));
            report["counters"]["source_started_host_ns"] = json!(copy_ns);
            match sent {
                Ok(_) => {
                    if let Err(error) = wait.delivered() {
                        report["status"] = json!("failed");
                        report["error"] = json!(error.to_string());
                        return report;
                    }
                    report["valid"] = json!(true);
                    report["status"] = json!("fake_room_delivery");
                }
                Err(error) => {
                    report["status"] = json!("failed");
                    report["error"] = json!(error.to_string());
                }
            }
            report
        });
        Ok(Some(Preparing {
            ready,
            grant,
            handle,
        }))
    }
}

impl Player for FakePreparedRoomPlayer {
    fn start(
        &mut self,
        _: &Path,
        _: &SceneTiming,
        _: &Path,
        _: &Path,
    ) -> Result<JoinHandle<Value>> {
        Err(invalid("the prepared fake player starts only on a grant"))
    }
    fn describe(&self) -> Value {
        json!({"kind": "fake prepared room datagrams",
            "note": "loopback test; mirrors the prepared grant contract; no audio",
            "faults": self.faults.iter().map(|(n, f)| json!({"prepare": n, "fault": format!("{f:?}")})).collect::<Vec<_>>()})
    }
    fn prepare(
        &mut self,
        wav: &Path,
        timing: &SceneTiming,
        _: &Path,
        cancel_file: &Path,
        max_wait_ms: u32,
    ) -> Result<Option<Preparing>> {
        self.prepare_with_ns_clock(
            wav,
            timing,
            cancel_file,
            max_wait_ms,
            |_| Ok(monotonic_ns()),
        )
    }
}

struct ClockedFakePreparedRoomPlayer<C> {
    player: FakePreparedRoomPlayer,
    now: C,
}

impl<C: Fn() -> MonoTime + Clone + Send + Sync + 'static> Player
    for ClockedFakePreparedRoomPlayer<C>
{
    fn start(
        &mut self,
        wav: &Path,
        timing: &SceneTiming,
        report_directory: &Path,
        cancel_file: &Path,
    ) -> Result<JoinHandle<Value>> {
        self.player
            .start(wav, timing, report_directory, cancel_file)
    }

    fn describe(&self) -> Value {
        self.player.describe()
    }

    fn prepare(
        &mut self,
        wav: &Path,
        timing: &SceneTiming,
        _: &Path,
        cancel_file: &Path,
        max_wait_ms: u32,
    ) -> Result<Option<Preparing>> {
        let now = self.now.clone();
        self.player
            .prepare_with_ns_clock(wav, timing, cancel_file, max_wait_ms, move |phase| {
                now().as_micros().checked_mul(1000).ok_or_else(|| {
                    invalid(&format!("fake prepared {phase} time overflows nanoseconds"))
                })
            })
    }
}

#[derive(Clone, Debug)]
pub enum LampMode {
    Fixture {
        reply: String,
        sha256: String,
    },
    Directed {
        provider_config: String,
    },
    /// One `lamp-voice serve` runtime per attempt, stopped by the runner.
    Serve {
        provider_config: String,
    },
}

pub struct PhysicalConfig {
    /// Argument vector that starts `lamp-voice-eval lamp-session ... --runtime
    /// LAMP_LIVE` on the Lamp, for example through the operator's SSH command.
    /// The runner appends `--work`, `--seconds` and mode arguments.
    pub lamp_command: Vec<String>,
    /// Absolute directory on the Lamp under which per-attempt work dirs are made.
    pub lamp_work_root: String,
    pub mode: LampMode,
    pub noise_suppression: String,
    pub diagnostics: bool,
    /// Optional recorder argv with `{out}` and `{seconds}` placeholders, e.g.
    /// the signed observer bundle. It must write `metadata.json` in `{out}`.
    pub room_recorder: Option<Vec<String>>,
    pub room_independent: bool,
    pub allowance_seconds: u16,
    /// Forwarded to lamp-voice so ring cues are traced and checked.
    pub ring_channel_ceiling: Option<u16>,
    /// lamp-voice runtime policy for `directed` and `serve`; empty passes
    /// nothing, so the runtime keeps its defaults.
    pub runtime_flags: crate::physical::session::RuntimeFlags,
    /// Loudspeaker replay or direct human speech; never pooled.
    pub stimulus_source: StimulusSource,
}

/// Relay lines taken in one final dispatch drain before the backend asks the
/// runner to revalidate instead of starting.
const MAX_DISPATCH_DRAIN_LINES: usize = 1024;

/// Keep refining the clock map during the session, within a bound.
const PING_INTERVAL_US: u64 = 250_000;
const MAX_PINGS: u64 = 400;
/// A controlled finite fake may flush its whole trace before acknowledging a
/// poll. Reserve its trace, one cue per trace record, one duplicated fault cue,
/// every pong, runtime readiness, SessionStart and SessionEnd. The reader also
/// reserves one separate terminal-error slot; the session byte cap still holds.
pub(crate) const FIXTURE_RELAY_QUEUE_CAPACITY: usize =
    2 * crate::physical::protocol::MAX_TRACE_LINES + MAX_PINGS as usize + 4;

/// Scheduling identity only; this never changes Lamp conversation authority.
#[derive(Default)]
struct CueOrder {
    boot: Option<[u8; 16]>,
    sequence: u64,
    sent_us: u64,
    owners: BTreeMap<u64, CueOwner>,
    ended: bool,
}
struct CueOwner {
    generation: u64,
    endpoint: bool,
    playback: PlaybackHistory,
    terminal: bool,
}
impl CueOrder {
    fn accept(&mut self, cue: &Cue, lamp_received_us: u64) -> Result<()> {
        if lamp_received_us < cue.sent_us || lamp_received_us >= cue.expires_us {
            return Err(invalid("stale/future cue at Lamp relay"));
        }
        // A new boot ends this conversation even with a contiguous sequence:
        // cues alone do not verify a service restart, and its context is lost.
        if !self.ended
            && self.boot.is_some_and(|boot| boot != cue.boot)
            && self.sequence.checked_add(1) == Some(cue.sequence)
        {
            return Err(invalid(
                "cue boot changed (contiguous sequence): runtime context continuity lost",
            ));
        }
        if self.ended
            || self.sequence.checked_add(1) != Some(cue.sequence)
            || self.boot.is_some_and(|boot| boot != cue.boot)
            || cue.sent_us < self.sent_us
        {
            return Err(invalid("cue boot/sequence/time order changed"));
        }
        if let (Some(turn), Some(generation)) = (cue.turn, cue.generation) {
            if cue.kind == "input_admitted" {
                if self
                    .owners
                    .last_key_value()
                    .is_some_and(|(&old, state)| turn <= old || generation <= state.generation)
                {
                    return Err(invalid("cue admission did not advance its owner"));
                }
                self.owners.insert(
                    turn,
                    CueOwner {
                        generation,
                        endpoint: false,
                        playback: PlaybackHistory::default(),
                        terminal: false,
                    },
                );
            } else {
                let owner = self
                    .owners
                    .get_mut(&turn)
                    .ok_or_else(|| invalid("cue owner has no admitted input"))?;
                if owner.generation != generation || owner.terminal {
                    return Err(invalid("cue relabeled or revived a terminal owner"));
                }
                match cue.kind.as_str() {
                    "local_endpoint" if !owner.endpoint && !owner.playback.started() => {
                        owner.endpoint = true
                    }
                    "speaker_first_write" if owner.endpoint => {
                        let sequence =
                            cue.playback_sequence.filter(|n| *n > 0).ok_or_else(|| {
                                invalid("live playback cue has no occurrence identity")
                            })?;
                        owner
                            .playback
                            .start(Some(sequence), cue.event_us)
                            .map_err(invalid)?;
                    }
                    "speech_retired" => {
                        let sequence =
                            cue.playback_sequence.filter(|n| *n > 0).ok_or_else(|| {
                                invalid("live retirement cue has no occurrence identity")
                            })?;
                        owner
                            .playback
                            .retire(Some(sequence), cue.event_us)
                            .map_err(invalid)?;
                    }
                    // Retiring all current PCM is not whole-turn completion.
                    "cancelled" => owner.terminal = true,
                    // A terminal provider failure needs no normal endpoint (an
                    // abandoned request ends on the drain path) and may follow
                    // a partial answer, whose audio must already be retired.
                    "turn_completed"
                        if cue.outcome.as_deref() == Some(crate::events::PROVIDER_FAILED)
                            && !owner.playback.active() =>
                    {
                        if owner
                            .playback
                            .occurrences
                            .last()
                            .and_then(|p| p.retired_us)
                            .is_some_and(|end| cue.event_us < end)
                        {
                            return Err(invalid("provider failure predates playback retirement"));
                        }
                        owner.terminal = true;
                    }
                    "turn_completed" if owner.endpoint && !owner.playback.active() => {
                        let audio = cue.outcome.as_deref() != Some("no_audio_answer");
                        if (audio && !owner.playback.complete())
                            || (!audio && owner.playback.started())
                            || owner
                                .playback
                                .occurrences
                                .last()
                                .and_then(|p| p.retired_us)
                                .is_some_and(|end| cue.event_us < end)
                        {
                            return Err(invalid(
                                "completion disagrees with playback occurrence history",
                            ));
                        }
                        owner.terminal = true;
                    }
                    _ => return Err(invalid("cue phase out of order or duplicated")),
                }
            }
        }
        self.boot = Some(cue.boot);
        self.sequence = cue.sequence;
        self.sent_us = cue.sent_us;
        self.ended = cue.kind == "run_end";
        Ok(())
    }
}

struct Session {
    child: Child,
    next_ping_us: u64,
    stdin: Option<ChildStdin>,
    lines: Receiver<std::result::Result<SessionLine, String>>,
    pings: Vec<(u64, u64)>,
    best: Option<ClockMapping>,
    trace: Vec<Value>,
    end: Option<SessionLine>,
    /// The transport closed without a relayed `session_end`.
    end_synthesized: bool,
    ready_seen: bool,
    protocol_errors: Vec<String>,
    start: Option<Value>,
    cues: Vec<Value>,
    cue_order: CueOrder,
    cue_failed: bool,
    /// Cues observed under a new boot, kept for provenance only.
    boot_changes: Vec<Value>,
    /// Serve only: the runner's stop line was sent.
    stop_sent: bool,
    /// The runtime's own `run_end` cue was accepted: it is ending by itself.
    runtime_ended: bool,
    /// Serve: relay receipt times of stdout readiness lines (diagnostic only).
    stdout_readiness_us: Vec<u64>,
    /// Every ping/pong sample, bounded, so each freshness decision's mapping
    /// can be reviewed later.
    clock_samples: Vec<Value>,
    /// When the relay's session_start was taken. Only pings written after it
    /// measure the live transport and may define the clock map.
    started_runner_us: Option<u64>,
    /// Cues taken before a live clock map, in order, with their original
    /// runner receipt and relay receipt times. Nothing is published from them
    /// until a live map proves each unexpired at its decision.
    held: std::collections::VecDeque<(u64, u64, Value)>,
    /// Events of held cues just decided, for `take` to latch and queue.
    released: std::collections::VecDeque<RuntimeEvent>,
}

struct Playback {
    step: String,
    requested_us: u64,
    cancel_file: PathBuf,
    handle: Option<JoinHandle<Value>>,
    /// Collected during the session when the player ended early.
    report: Option<Value>,
    /// Started by a prepared grant: its start is the first source copy.
    prepared: bool,
    /// The latest start the room recording covers for this prompt.
    latest_start_us: Option<u64>,
}

/// The armed prepared player for the next step.
struct Armed {
    step: String,
    prepared: PreparedStart,
    cancel_file: PathBuf,
    armed_us: u64,
    /// End of the prompt's speech after its start, for room coverage.
    speech_end_us: u64,
}

/// The original deadline's margin when the grant was sent, signed. A pause
/// after the pre-send check can put the send at or past expiry; the player
/// then refuses at its first copy, and this keeps that evidence instead of
/// inventing a positive headroom.
fn grant_margin(deadline_us: u64, sent_us: u64) -> Value {
    let headroom = i128::from(deadline_us) - i128::from(sent_us);
    json!({"headroom_at_grant_us": i64::try_from(headroom).unwrap_or(if headroom < 0 { i64::MIN } else { i64::MAX }),
        "expired_at_send": sent_us >= deadline_us})
}

/// The longest a prepared player may wait for its grant (the observer's
/// own bound).
const MAX_PREPARED_WAIT_MS: u32 = 120_000;
/// How long the session transport may take to exit once its input closed.
const TRANSPORT_EXIT_BOUND_US: u64 = 5_000_000;

/// How long a serve runtime that sent its own `run_end` may take to exit
/// before the runner stops it.
const SELF_END_BOUND_US: u64 = 10_000_000;

/// Coordinates a finite local fixture before a logical wait. The driver owns
/// time advancement and must acknowledge real participant/reader progress.
/// `sent_frames` counts flushed runner control frames; `enqueued_frames`
/// counts complete relay frames already queued for this backend to consume.
/// Closing/dropping a driver must release its owned control peers so normal
/// backend cleanup can stop and reap a participant blocked on a fixture step.
pub trait FixtureDriver {
    /// Only an explicit driver with prepared-player and signal coordination
    /// can opt into serve. The finite driver keeps rejecting it.
    fn supports_serve(&self) -> bool {
        false
    }

    /// Only a driver that configured an explicit local recorder participant
    /// can coordinate its ready file, full wait and actual exit barrier.
    fn supports_recorder(&self) -> bool {
        false
    }

    /// Register and start the recorder immediately after its real spawn,
    /// sharing the original ready-file host deadline.
    fn start_recorder(&mut self, _host_deadline: Instant) -> Result<()> {
        Err(invalid("fixture driver does not coordinate a recorder"))
    }

    /// Terminal progress is pending until the backend observes actual exit.
    fn recorder_exit_pending(&self) -> bool {
        false
    }

    /// Called only after the backend-owned Child returned a real exit status.
    fn recorder_exit_observed(&mut self) -> Result<()> {
        Ok(())
    }

    /// After relay completion, drive the recorder and pending players only;
    /// return at recorder terminal progress without claiming actual exit.
    fn finish_recorder(&mut self, _host_deadline: Instant) -> Result<()> {
        Ok(())
    }

    /// Called after a real Start or Closed input was successfully sent. It
    /// acknowledges channel consumption, not acceptance or audible output.
    fn grant_sent(&mut self, _attempt_id: &str) -> Result<()> {
        Ok(())
    }

    /// A completed report is not an exited thread. The backend separately
    /// waits for the actual JoinHandle before advancing or inspecting it.
    fn player_report_ready(&self, _attempt_id: &str) -> bool {
        false
    }

    /// After terminal relay output, finish only outstanding fixture-player
    /// work within the original join budget. Report markers still require
    /// the real owned-thread joins below. Errors must close the participants.
    fn finish_players(&mut self, _host_deadline: Instant) -> Result<()> {
        Ok(())
    }

    /// Preparation keeps its original ten-second host watchdog even when
    /// the driver's ordinary control watchdog is longer. Serve-capable
    /// drivers must propagate this deadline into every participant wait.
    fn prepare_wait(
        &mut self,
        _until: MonoTime,
        _sent_frames: u64,
        _enqueued_frames: &AtomicU64,
        _queue_drained: bool,
        _host_deadline: Instant,
    ) -> Result<()> {
        Err(invalid(
            "fixture driver does not coordinate prepared readiness",
        ))
    }

    fn wait(
        &mut self,
        until: MonoTime,
        sent_frames: u64,
        enqueued_frames: &AtomicU64,
        queue_drained: bool,
    ) -> Result<()>;
}

struct FixtureProgress<'a> {
    driver: Box<dyn FixtureDriver + 'a>,
    enqueued: Arc<AtomicU64>,
    sent: u64,
    started: bool,
    recorder_failure: Option<String>,
}

/// Keep a failed explicit fixture nonblocking while dropping its original
/// driver closes all peers. A frozen fixture must never fall back to host
/// channel waits when the runner calls finish after a control error.
struct FailedRecorderFixture(String);

impl FixtureDriver for FailedRecorderFixture {
    fn supports_serve(&self) -> bool {
        true
    }
    fn supports_recorder(&self) -> bool {
        true
    }
    fn start_recorder(&mut self, _: Instant) -> Result<()> {
        Err(invalid(&self.0))
    }
    fn grant_sent(&mut self, _: &str) -> Result<()> {
        Err(invalid(&self.0))
    }
    fn recorder_exit_observed(&mut self) -> Result<()> {
        Err(invalid(&self.0))
    }
    fn finish_recorder(&mut self, _: Instant) -> Result<()> {
        Err(invalid(&self.0))
    }
    fn finish_players(&mut self, _: Instant) -> Result<()> {
        Err(invalid(&self.0))
    }
    fn prepare_wait(
        &mut self,
        _: MonoTime,
        _: u64,
        _: &AtomicU64,
        _: bool,
        _: Instant,
    ) -> Result<()> {
        Err(invalid(&self.0))
    }
    fn wait(&mut self, _: MonoTime, _: u64, _: &AtomicU64, _: bool) -> Result<()> {
        Err(invalid(&self.0))
    }
}

pub struct PhysicalBackend<'a> {
    config: PhysicalConfig,
    assets: &'a AssetIndex,
    player: Box<dyn Player + 'a>,
    now: Box<dyn Fn() -> MonoTime + 'a>,
    fixture: Option<FixtureProgress<'a>>,
    session: Option<Session>,
    room: Option<(Child, PathBuf, u64)>,
    playbacks: Vec<Playback>,
    attempt_directory: Option<PathBuf>,
    seconds: u16,
    /// Events drained by the final dispatch check, returned by `next_event`.
    pending: std::collections::VecDeque<RuntimeEvent>,
    /// The first terminal cause seen on this attempt's session; no stimulus
    /// starts after it.
    unavailable: Option<Withheld>,
    /// Serve: the prepared player for the next step, or why there is none.
    armed: Option<Armed>,
    arm_failure: Option<(String, String)>,
    /// Prepared players that never received a grant, with their reports.
    unused: Vec<(String, Value, JoinHandle<Value>)>,
    /// The trigger and configured delay the next `inject` serves.
    trigger: Option<(TriggerObservation, u64)>,
    /// Serve: runner evidence loss already reported as an event.
    evidence_lost: bool,
    /// Room coverage for this attempt.
    room_cover: RoomCover,
    /// Fixture-only mapping; the original Playback representation stays intact.
    fixture_player_attempts: BTreeMap<String, String>,
    #[cfg(test)]
    transport_exit_clock: Option<transport_exit_clock_tests::Clock>,
}

/// How long the room recording covers the session. A prompt is granted only
/// while the recorder runs and, when it declares its capture end, only if the
/// prompt's speech and the scenario's observation window end before it.
#[derive(Default)]
struct RoomCover {
    /// Declared capture end on this host's monotonic clock (ready receipt).
    end_us: Option<u64>,
    /// Bounded ready receipt, retained with the room evidence.
    ready: Value,
    observe_us: u64,
    /// When and why coverage ended during the session.
    lost: Option<(u64, &'static str, String)>,
}

/// Coverage-loss kinds retained with the room evidence. Only an exit before
/// the declared end (or with none declared) can leave a granted prompt or its
/// reply unrecorded.
pub const RECORDER_EXITED_EARLY: &str = "recorder_exited_early";
const DECLARED_END_REACHED: &str = "declared_end_reached";
const PROMPT_NOT_COVERED: &str = "prompt_not_covered";
/// How far a prompt started at `start_us`, through its speech and the
/// observation window, would run past the recording's declared end.
fn coverage_shortfall(
    end_us: Option<u64>,
    start_us: u64,
    timing: &SceneTiming,
    observe_us: u64,
) -> Option<u64> {
    let speech_end_ms = timing
        .speech_span_ms
        .map_or(timing.duration_ms, |(_, end)| end);
    let needed = start_us + u64::from(speech_end_ms) * 1000 + observe_us;
    end_us.filter(|end| needed > *end).map(|end| needed - end)
}

/// Speech end after the prompt's start; a clip without a span is all speech.
fn speech_end_us(timing: &SceneTiming) -> u64 {
    u64::from(
        timing
            .speech_span_ms
            .map_or(timing.duration_ms, |(_, end)| end),
    ) * 1000
}

/// The latest start whose speech and observation window the recording's
/// declared end still covers.
fn latest_covered_start(end_us: Option<u64>, speech_end_us: u64, observe_us: u64) -> Option<u64> {
    end_us.map(|end| end.saturating_sub(speech_end_us + observe_us))
}

/// Why room evidence cannot certify the attempt's speech: its recorder
/// exited before any declared end, or a prompt actually started after the
/// latest start the recording covered. `None` while coverage holds.
pub fn coverage_refusal(room: &Value) -> Option<&'static str> {
    if room["coverage_lost"]["kind"] == RECORDER_EXITED_EARLY {
        Some("ended_during_session")
    } else if room["coverage_violations"]
        .as_array()
        .is_some_and(|violations| !violations.is_empty())
    {
        Some("prompt_outside_recording")
    } else {
        None
    }
}

/// Bound on the recorder's ready receipt read by the runner.
const MAX_READY_BYTES: u64 = 64 * 1024;

/// The recorder's ready receipt, once it parses (it may still be written),
/// within a short bound. Returns it bounded, with any declared capture end.
fn ready_receipt(path: &Path) -> (Value, Option<u64>) {
    let until = monotonic_us() + 1_000_000;
    loop {
        let parsed = fs::metadata(path)
            .ok()
            .filter(|m| m.len() <= MAX_READY_BYTES)
            .and_then(|_| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .filter(Value::is_object);
        if let Some(receipt) = parsed {
            let end = receipt["planned_capture_end_host_monotonic_ns"]
                .as_u64()
                .map(|ns| ns / 1000);
            let keep = |key: &str| receipt.get(key).cloned().unwrap_or(Value::Null);
            return (
                json!({"status": keep("status"),
                    "first_callback_host_monotonic_ns": keep("first_callback_host_monotonic_ns"),
                    "requested_frames": keep("requested_frames"), "sample_rate": keep("sample_rate"),
                    "planned_capture_end_host_monotonic_ns": keep("planned_capture_end_host_monotonic_ns")}),
                end,
            );
        }
        if monotonic_us() >= until {
            return (json!({"unreadable": true}), None);
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// Run-end status of a runner-generated protocol stop.
const CUE_PROTOCOL_INVALID: &str = "cue_protocol_invalid";

/// End scheduling immediately, retaining the actual trace at finish. This is a
/// runner-generated protocol stop, not a Lamp failure or an acoustic event.
fn cue_failure(session: &mut Session, reason: String, received_us: u64) -> RuntimeEvent {
    session.cue_failed = true;
    session.protocol_errors.push(reason.clone());
    RuntimeEvent::new(
        EventKind::RunEnd {
            status: Some(CUE_PROTOCOL_INVALID.into()),
            error: Some(reason),
        },
        None,
        received_us,
        ClockDomain::RunnerMonotonic,
        EventSource::Cue,
    )
}

/// Most cues held before a live clock map. Startup sends a handful.
const MAX_HELD_CUES: usize = 64;

/// Whether a live mapping proves a cue unexpired at `decided_us` on the
/// runner clock: its upper Lamp-time bound must be before the original
/// expiry. Equality refuses. Returns the upper bound and the decision.
fn proves_unexpired(map: &ClockMapping, decided_us: u64, expires_us: u64) -> (i128, bool) {
    let upper_lamp_us =
        i128::from(decided_us) + i128::from(map.offset_us) + i128::from(map.uncertainty_us);
    (upper_lamp_us, upper_lamp_us < i128::from(expires_us))
}

/// Decide one cue: identity, order and freshness, with a fresh clock sample
/// after parsing. Its original runner receipt (`received_us`) and relay
/// receipt stay its times; nothing is restamped or extended. Freshness needs
/// a live clock map: relay receipt is not proof at the runner's decision.
fn decide_cue(
    session: &mut Session,
    received_us: u64,
    lamp_received_us: u64,
    cue: Value,
    now_us: impl FnOnce() -> u64,
) -> Option<RuntimeEvent> {
    if session.cue_failed {
        return None;
    }
    // Pre-validation failures retain receipt time; freshness samples anew.
    let mut decided_us = received_us;
    // The mapping this freshness decision used, retained with it.
    let mut freshness = json!({"mapping": null});
    let result = (|| -> Result<Cue> {
        if session.start.is_none() {
            return Err(invalid("cue arrived without relay capability receipt"));
        }
        if session.cues.len() >= crate::physical::protocol::MAX_TRACE_LINES {
            return Err(invalid("cue receipt limit exceeded"));
        }
        let parsed = parse_cue(&serde_json::to_vec(&cue)?)?;
        // The Lamp relay validates local receipt; the runner's live clock
        // map must also prove the cue unexpired when it is decided.
        let map = session
            .best
            .as_ref()
            .ok_or_else(|| invalid("no live clock map proves the cue unexpired"))?;
        decided_us = now_us();
        let (upper_lamp_us, fresh) = proves_unexpired(map, decided_us, parsed.expires_us);
        freshness = json!({"offset_us": map.offset_us,
            "uncertainty_us": map.uncertainty_us, "method": map.method,
            "runner_received_us": received_us, "decided_runner_us": decided_us,
            "held_us": decided_us.saturating_sub(received_us),
            "upper_lamp_us": i64::try_from(upper_lamp_us).ok(),
            "expires_us": parsed.expires_us,
            "margin_us": i64::try_from(i128::from(parsed.expires_us) - upper_lamp_us).ok()});
        if !fresh {
            return Err(invalid("stale cue at runner (including clock uncertainty)"));
        }
        if session
            .cue_order
            .boot
            .is_some_and(|boot| boot != parsed.boot)
        {
            session
                .boot_changes
                .push(json!({"sequence": parsed.sequence,
                "kind": parsed.kind, "event_lamp_us": parsed.event_us,
                "runner_received_us": received_us}));
        }
        session.cue_order.accept(&parsed, lamp_received_us)?;
        // A terminal provider link ends a serve run as well.
        session.runtime_ended |= parsed.kind == "run_end"
            || (parsed.kind == "provider_link" && parsed.link.as_deref() == Some("terminal"));
        Ok(parsed)
    })();
    if session.cues.len() < crate::physical::protocol::MAX_TRACE_LINES {
        session
            .cues
            .push(json!({"cue": cue, "lamp_received_us": lamp_received_us,
            "runner_received_us": received_us, "accepted": result.is_ok(),
            "error": result.as_ref().err().map(ToString::to_string),
            "freshness": freshness}));
    }
    let mut event = match result {
        Ok(cue) => cue.event()?,
        Err(error) => return Some(cue_failure(session, error.to_string(), decided_us)),
    };
    // Both modes report readiness on stdout and the cue socket.
    if matches!(event.kind, EventKind::ListeningReady)
        && std::mem::replace(&mut session.ready_seen, true)
    {
        return None;
    }
    event.received_us = Some(received_us);
    Some(event)
}

/// Decide every held cue, in order, once a live clock map exists.
fn release_held(session: &mut Session, mut now_us: impl FnMut() -> u64) {
    while let Some((received_us, lamp_received_us, cue)) = session.held.pop_front() {
        if let Some(event) = decide_cue(session, received_us, lamp_received_us, cue, &mut now_us) {
            session.released.push_back(event);
        }
    }
}

impl<'a> PhysicalBackend<'a> {
    pub fn new(
        config: PhysicalConfig,
        assets: &'a AssetIndex,
        player: Box<dyn Player + 'a>,
    ) -> Self {
        Self::new_with_clock(config, assets, player, || {
            MonoTime::from_micros(monotonic_us())
        })
    }

    /// Use an explicit clock for runner timestamps and logical deadlines.
    /// Reads must be pure and all participating fixtures must share this
    /// clock domain. This entry point does not coordinate or advance them:
    /// channel waits and sleeps still wait on the host. Resource readiness,
    /// post-end draining and process/thread cleanup retain their host bounds.
    pub fn new_with_clock(
        config: PhysicalConfig,
        assets: &'a AssetIndex,
        player: Box<dyn Player + 'a>,
        now: impl Fn() -> MonoTime + 'a,
    ) -> Self {
        Self {
            config,
            assets,
            player,
            now: Box::new(now),
            fixture: None,
            session: None,
            room: None,
            playbacks: Vec::new(),
            attempt_directory: None,
            seconds: 0,
            pending: std::collections::VecDeque::new(),
            unavailable: None,
            armed: None,
            arm_failure: None,
            unused: Vec::new(),
            trigger: None,
            evidence_lost: false,
            room_cover: RoomCover::default(),
            fixture_player_attempts: BTreeMap::new(),
            #[cfg(test)]
            transport_exit_clock: None,
        }
    }

    /// Install a driver for one explicitly controlled local attempt.
    /// Serve requires its driver's prepared-player/signal capability. Room
    /// recorders require their driver's separate recorder capability.
    pub fn with_fixture_driver(mut self, driver: impl FixtureDriver + 'a) -> Result<Self> {
        if (self.serve() && !driver.supports_serve())
            || (self.config.room_recorder.is_some() && !driver.supports_recorder())
            || self.session.is_some()
        {
            return Err(invalid(
                "fixture stepping requires a fresh finite backend without a recorder",
            ));
        }
        if self.fixture.is_some() {
            return Err(invalid("fixture driver already installed"));
        }
        self.fixture = Some(FixtureProgress {
            driver: Box::new(driver),
            enqueued: Arc::new(AtomicU64::new(0)),
            sent: 0,
            started: false,
            recorder_failure: None,
        });
        Ok(self)
    }

    fn pump_fixture(&mut self, until_us: u64, queue_drained: bool) -> Result<()> {
        let recorder_deadline = self
            .fixture_recorder()
            .then(|| Instant::now() + crate::physical::fixture_control::DEFAULT_WATCHDOG);
        let result: Result<()> = (|| {
            if let Some(fixture) = &mut self.fixture {
                fixture.driver.wait(
                    MonoTime::from_micros(until_us),
                    fixture.sent,
                    &fixture.enqueued,
                    queue_drained,
                )?;
            }
            if let Some(deadline) = recorder_deadline {
                self.settle_fixture_recorder_exit(deadline)?;
            }
            self.settle_fixture_reports(Instant::now() + Duration::from_secs(25))?;
            Ok(())
        })();
        if recorder_deadline.is_some()
            && let Err(error) = &result
        {
            self.fail_fixture_recorder(error.to_string());
        }
        result
    }

    fn pump_preparation_fixture(&mut self, until_us: u64, host_deadline: Instant) -> Result<()> {
        let recorder = self.fixture_recorder();
        let result: Result<()> = (|| {
            if let Some(fixture) = &mut self.fixture {
                fixture.driver.prepare_wait(
                    MonoTime::from_micros(until_us),
                    fixture.sent,
                    &fixture.enqueued,
                    true,
                    host_deadline,
                )?;
            }
            if recorder {
                self.settle_fixture_recorder_exit(host_deadline)?;
            }
            self.settle_fixture_reports(host_deadline)?;
            Ok(())
        })();
        if recorder && let Err(error) = &result {
            self.fail_fixture_recorder(error.to_string());
        }
        result
    }

    fn fixture_recorder(&self) -> bool {
        self.config.room_recorder.is_some()
            && self
                .fixture
                .as_ref()
                .is_some_and(|fixture| fixture.driver.supports_recorder())
    }

    fn fail_fixture_recorder(&mut self, error: String) {
        let Some(fixture) = &mut self.fixture else {
            return;
        };
        if fixture.recorder_failure.is_some() {
            return;
        }
        fixture.recorder_failure = Some(error.clone());
        let old = std::mem::replace(&mut fixture.driver, Box::new(FailedRecorderFixture(error)));
        drop(old);
        if let Some((child, _, _)) = &mut self.room {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// A terminal control Ack permits a bounded real exit wait, not an exit
    /// claim. Keep logical time fixed and leave coverage decisions untouched.
    fn settle_fixture_recorder_exit(&mut self, host_deadline: Instant) -> Result<()> {
        let result = self.observe_fixture_recorder_exit(host_deadline);
        if let Err(error) = &result {
            self.fail_fixture_recorder(error.to_string());
        }
        result
    }

    fn observe_fixture_recorder_exit(&mut self, host_deadline: Instant) -> Result<()> {
        let Some(fixture) = &self.fixture else {
            return Ok(());
        };
        if let Some(error) = &fixture.recorder_failure {
            return Err(invalid(error));
        }
        if !fixture.driver.recorder_exit_pending() {
            return Ok(());
        }
        let at = (self.now)();
        loop {
            if (self.now)() != at {
                return Err(invalid(
                    "fixture clock advanced before actual recorder exit",
                ));
            }
            let child = &mut self
                .room
                .as_mut()
                .ok_or_else(|| invalid("fixture recorder exit has no owned child"))?
                .0;
            if child.try_wait()?.is_some() {
                break;
            }
            let remaining = host_deadline
                .checked_duration_since(Instant::now())
                .filter(|remaining| !remaining.is_zero())
                .ok_or_else(|| {
                    invalid("fixture recorder did not actually exit within the host bound")
                })?;
            // The existing owned-child wait polls every 20 ms on host time.
            thread::sleep(remaining.min(Duration::from_millis(20)));
        }
        self.fixture
            .as_mut()
            .unwrap()
            .driver
            .recorder_exit_observed()?;
        if (self.now)() != at {
            return Err(invalid(
                "fixture clock advanced while acknowledging actual recorder exit",
            ));
        }
        Ok(())
    }

    fn finish_fixture_recorder(&mut self, host_deadline: Instant) -> Result<()> {
        if !self.fixture_recorder() {
            return Ok(());
        }
        let result: Result<()> = (|| {
            self.fixture
                .as_mut()
                .unwrap()
                .driver
                .finish_recorder(host_deadline)?;
            self.settle_fixture_recorder_exit(host_deadline)?;
            if self
                .room
                .as_mut()
                .ok_or_else(|| invalid("fixture recorder finish has no owned child"))?
                .0
                .try_wait()?
                .is_none()
            {
                return Err(invalid(
                    "fixture recorder finish returned before actual exit",
                ));
            }
            Ok(())
        })();
        if let Err(error) = &result {
            self.fail_fixture_recorder(error.to_string());
        }
        result
    }

    fn settle_fixture_reports(&self, deadline: Instant) -> Result<()> {
        let Some(fixture) = &self.fixture else {
            return Ok(());
        };
        for playback in &self.playbacks {
            if let Some(attempt) = self.fixture_player_attempts.get(&playback.step)
                && fixture.driver.player_report_ready(attempt)
                && let Some(handle) = &playback.handle
            {
                wait_fixture_thread(handle, deadline)?;
            }
        }
        for (_, ready, handle) in &self.unused {
            if ready["attempt_id"]
                .as_str()
                .is_some_and(|attempt| fixture.driver.player_report_ready(attempt))
            {
                wait_fixture_thread(handle, deadline)?;
            }
        }
        Ok(())
    }

    fn fixture_player_error(&mut self, error: String) {
        self.unavailable
            .get_or_insert_with(|| Withheld::ProtocolFailed(error.clone()));
        if self.evidence_lost
            && self
                .fixture
                .as_ref()
                .is_some_and(|fixture| fixture.recorder_failure.is_some())
        {
            return;
        }
        let event = self.lose_evidence("fixture_player".into(), error);
        self.pending.push_back(event);
    }

    fn notify_prepared_input(&mut self, ready: &Value) -> Result<()> {
        let result: Result<()> = (|| {
            if let Some(fixture) = &mut self.fixture {
                let attempt = ready["attempt_id"]
                    .as_str()
                    .ok_or_else(|| invalid("fixture prepared receipt has no attempt ID"))?;
                fixture.driver.grant_sent(attempt)?;
            }
            Ok(())
        })();
        if self.fixture_recorder()
            && let Err(error) = &result
        {
            self.fail_fixture_recorder(error.to_string());
        }
        result
    }

    fn serve(&self) -> bool {
        matches!(self.config.mode, LampMode::Serve { .. })
    }

    /// Stop an armed player that will never be granted; its report is kept.
    fn release_armed(&mut self) {
        if let Some(armed) = self.armed.take() {
            if armed.prepared.grant.try_send(GrantInput::Closed).is_ok()
                && let Err(error) = self.notify_prepared_input(&armed.prepared.ready)
            {
                self.fixture_player_error(error.to_string());
            }
            self.unused
                .push((armed.step, armed.prepared.ready, armed.prepared.handle));
        }
    }

    fn send_stop(&mut self, reason: &str) -> Result<()> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        let Some(stdin) = session.stdin.as_mut() else {
            return Ok(());
        };
        serde_json::to_writer(
            &mut *stdin,
            &RunnerLine::Stop {
                reason: reason.into(),
            },
        )?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        session.stop_sent = true;
        if let Some(fixture) = &mut self.fixture {
            fixture.sent = fixture
                .sent
                .checked_add(1)
                .ok_or_else(|| invalid("fixture runner input count overflow"))?;
        }
        Ok(())
    }

    /// A cue held for 100 ms of runner time without a live clock map has
    /// certainly expired: the relay showed it arrived no earlier than its
    /// event, so its Lamp age is at least the hold. It is refused, never
    /// published; later held cues stay undecided in the evidence.
    fn expire_held(&mut self, now_us: u64) -> Option<RuntimeEvent> {
        let session = self.session.as_mut()?;
        let &(received, _, _) = session.held.front()?;
        if session.cue_failed || now_us < received.saturating_add(crate::events::CUE_FRESH_US) {
            return None;
        }
        let (received, lamp_received_us, cue) = session.held.pop_front()?;
        if session.cues.len() < crate::physical::protocol::MAX_TRACE_LINES {
            session
                .cues
                .push(json!({"cue": cue, "lamp_received_us": lamp_received_us,
                "runner_received_us": received, "accepted": false,
                "error": "held for its whole 100 ms lease without a live clock map",
                "freshness": {"mapping": null, "decided_runner_us": now_us,
                    "held_us": now_us - received}}));
        }
        let event = cue_failure(
            session,
            "no live clock map proved a held cue unexpired within its 100 ms lease".into(),
            now_us,
        );
        self.note(&event);
        Some(event)
    }

    /// When the oldest held cue's lease ends on the runner clock.
    fn held_due(&self) -> Option<u64> {
        let session = self.session.as_ref()?;
        let &(received, _, _) = session.held.front()?;
        Some(received.saturating_add(crate::events::CUE_FRESH_US))
    }

    /// Take every relayed line that has already arrived, keeping its events
    /// in order for `next_event`, and keep the clock map's pings on time.
    /// Used while a prepared player loads, so a cue is judged on arrival.
    fn consume_transport(&mut self) {
        let (due, sent) = self.session.as_ref().map_or((u64::MAX, MAX_PINGS), |s| {
            (s.next_ping_us, s.pings.len() as u64)
        });
        if sent < MAX_PINGS && (self.now)().as_micros() >= due {
            // A failed write means the transport closed; session_end reports it.
            let _ = self.send_ping(sent + 1);
        }
        while let Some(line) = self.wait_line((self.now)().as_micros()) {
            if let Some(event) = self.take(line) {
                self.pending.push_back(event);
            }
        }
        if let Some(event) = self.expire_held((self.now)().as_micros()) {
            self.pending.push_back(event);
        }
    }

    /// A player that ended without delivering (serve), or a room recording
    /// that stopped covering the session, becomes one runner event; every
    /// later stimulus is then withheld. Reports are kept for `finish`.
    fn poll_evidence(&mut self) -> Option<RuntimeEvent> {
        if self.evidence_lost {
            return None;
        }
        let mut lost = None;
        let serve = self.serve();
        for playback in self.playbacks.iter_mut().filter(|_| serve) {
            if playback.report.is_some()
                || !playback
                    .handle
                    .as_ref()
                    .is_some_and(JoinHandle::is_finished)
            {
                continue;
            }
            let report = playback
                .handle
                .take()
                .and_then(|handle| handle.join().ok())
                .unwrap_or_else(|| json!({"valid": false, "status": "player thread panicked"}));
            if report["valid"] != true && lost.is_none() {
                lost = Some((
                    format!("player:{}", playback.step),
                    format!(
                        "stimulus not delivered: {} {}",
                        report["status"], report["error"]
                    ),
                ));
            }
            playback.report = Some(report);
        }
        if lost.is_none()
            && let Some(reason) = self.room_ended()
        {
            lost = Some(("room_recorder".into(), reason));
        }
        let (source, reason) = lost?;
        Some(self.lose_evidence(source, reason))
    }

    /// Whether the room recording no longer covers the session: its recorder
    /// exited, or its declared capture end passed. Latched with its kind.
    fn room_ended(&mut self) -> Option<String> {
        let (child, _, _) = self.room.as_mut()?;
        let now = (self.now)().as_micros();
        let declared_end = self.room_cover.end_us.filter(|end| now >= *end);
        let (kind, reason) = match child.try_wait() {
            Ok(Some(status)) if declared_end.is_none() => (
                RECORDER_EXITED_EARLY,
                format!(
                    "the room recorder ended during the session ({status}) before any declared capture end"
                ),
            ),
            _ if declared_end.is_some() => (
                DECLARED_END_REACHED,
                "the room recording reached its declared capture end during the session".into(),
            ),
            _ => return None,
        };
        self.room_cover
            .lost
            .get_or_insert((now, kind, reason.clone()));
        Some(reason)
    }

    fn lose_evidence(&mut self, source: String, reason: String) -> RuntimeEvent {
        self.evidence_lost = true;
        let mut event = RuntimeEvent::new(
            EventKind::EvidenceLost { source, reason },
            None,
            (self.now)().as_micros(),
            ClockDomain::RunnerMonotonic,
            EventSource::Runner,
        );
        event.received_us = event.at_us;
        event
    }

    /// Why the room recording cannot cover a prompt started now: the
    /// recorder ended, or its declared capture end comes before the prompt's
    /// speech and the scenario's observation window after it end.
    fn room_uncovered(&mut self, start_us: u64, timing: &SceneTiming) -> Option<String> {
        self.room.as_ref()?;
        if let Some(reason) = self.room_ended() {
            return Some(reason);
        }
        let shortfall_us = coverage_shortfall(
            self.room_cover.end_us,
            start_us,
            timing,
            self.room_cover.observe_us,
        )?;
        let reason = format!(
            "the room recording's declared capture end is {} ms before this prompt and its {} ms observation window would end; no unrecorded prompt is played",
            shortfall_us / 1000,
            self.room_cover.observe_us / 1000
        );
        self.room_cover
            .lost
            .get_or_insert((start_us, PROMPT_NOT_COVERED, reason.clone()));
        Some(reason)
    }

    /// Send the armed player's one original start grant. Its expiry is the
    /// planned start's original lateness bound on this host's
    /// CLOCK_MONOTONIC, placed from the Lamp event time at the earliest
    /// runner time the clock map allows. Transport and receipt add nothing.
    fn grant(&mut self, step: &Step) -> std::result::Result<(Playback, Value), Withheld> {
        let (observation, delay_us) = self
            .trigger
            .clone()
            .ok_or_else(|| Withheld::Stale("no trigger observation for this grant".into()))?;
        let mapping = self
            .session
            .as_ref()
            .and_then(|s| s.best.clone())
            .ok_or_else(|| {
                Withheld::Stale("no clock map places the original deadline on this host".into())
            })?;
        let event_at = observation
            .event_at_us
            .filter(|_| observation.event_domain == ClockDomain::LampMonotonic)
            .ok_or_else(|| {
                Withheld::Stale(
                    "the trigger has no Lamp event time; its original deadline cannot be placed"
                        .into(),
                )
            })?;
        let earliest_us = mapping
            .to_runner(event_at)
            .saturating_sub(mapping.uncertainty_us);
        let original_deadline_us = earliest_us
            .checked_add(delay_us)
            .and_then(|at| at.checked_add(MAX_TRIGGER_LATENESS_US))
            .ok_or_else(|| Withheld::Stale("the original deadline overflows the clock".into()))?;
        let armed = match self.armed.take() {
            Some(armed) if armed.step == step.id => armed,
            other => {
                self.armed = other;
                let reason = match &self.arm_failure {
                    Some((failed, reason)) if *failed == step.id => reason.clone(),
                    _ => "no prepared player is armed for this step".into(),
                };
                return Err(Withheld::PlayerFailed(reason));
            }
        };
        // The first copy must also start no later than the recording still
        // covers: the earlier of the two bounds, never any added slack.
        let latest_start_us = latest_covered_start(
            self.room_cover.end_us,
            armed.speech_end_us,
            self.room_cover.observe_us,
        );
        let deadline_us = latest_start_us.map_or(original_deadline_us, |latest| {
            original_deadline_us.min(latest)
        });
        let capped = deadline_us < original_deadline_us;
        let Some(expires_ns) = deadline_us.checked_mul(1000) else {
            self.armed = Some(armed);
            return Err(Withheld::Stale(
                "the original deadline overflows the clock".into(),
            ));
        };
        let now = grant_clock_us(&self.now);
        if now >= deadline_us {
            let late = now - deadline_us;
            self.armed = Some(armed);
            if capped {
                let reason = format!(
                    "the room recording's latest covered start passed {late} us before the grant; no unrecorded prompt is played"
                );
                self.room_cover
                    .lost
                    .get_or_insert((now, PROMPT_NOT_COVERED, reason.clone()));
                return Err(Withheld::Stale(reason));
            }
            return Err(Withheld::Stale(format!(
                "the original deadline passed {late} us before the grant"
            )));
        }
        let line = json!({"schema": 1, "kind": "start",
            "attempt_id": armed.prepared.ready["attempt_id"], "expires_monotonic_ns": expires_ns})
        .to_string();
        if armed
            .prepared
            .grant
            .try_send(GrantInput::Line(line))
            .is_err()
        {
            let report = armed.prepared.handle.join().unwrap_or(Value::Null);
            return Err(Withheld::PlayerFailed(format!(
                "the prepared player ended before its grant: {} {}",
                report["status"], report["error"]
            )));
        }
        let sent_us = grant_clock_us(&self.now);
        if self.fixture.is_some() {
            if let Some(attempt) = armed.prepared.ready["attempt_id"].as_str() {
                self.fixture_player_attempts
                    .insert(step.id.clone(), attempt.to_owned());
            }
            if let Err(error) = self.notify_prepared_input(&armed.prepared.ready) {
                // The original grant was already sent. Retain its delivery
                // evidence and invalidate the fixture, never claim no dispatch.
                self.fixture_player_error(error.to_string());
            }
            if let Some(fixture) = &self.fixture
                && armed.prepared.ready["attempt_id"]
                    .as_str()
                    .is_some_and(|attempt| fixture.driver.player_report_ready(attempt))
                && let Err(error) = wait_fixture_thread(
                    &armed.prepared.handle,
                    Instant::now() + Duration::from_secs(25),
                )
            {
                self.fixture_player_error(error.to_string());
            }
        }
        let receipt = json!({"ready": armed.prepared.ready,
            "armed_runner_us": armed.armed_us, "grant_sent_runner_us": sent_us,
            "expires_monotonic_ns": expires_ns,
            "deadline": {"event_at_lamp_us": event_at, "mapped_runner_us": mapping.to_runner(event_at),
                "clock_uncertainty_us": mapping.uncertainty_us, "earliest_event_runner_us": earliest_us,
                "delay_us": delay_us, "lateness_bound_us": MAX_TRIGGER_LATENESS_US,
                "original_deadline_runner_us": original_deadline_us},
            "coverage": {"declared_end_runner_us": self.room_cover.end_us,
                "speech_end_us": armed.speech_end_us, "observe_us": self.room_cover.observe_us,
                "latest_start_runner_us": latest_start_us, "capped": capped},
            "margin": grant_margin(deadline_us, sent_us)});
        Ok((
            Playback {
                step: step.id.clone(),
                requested_us: sent_us,
                cancel_file: armed.cancel_file,
                handle: Some(armed.prepared.handle),
                report: None,
                prepared: true,
                latest_start_us,
            },
            receipt,
        ))
    }

    /// Latch the first terminal cause from a live event: a provider failure,
    /// the session's end, or a cue/relay protocol failure.
    fn note(&mut self, event: &RuntimeEvent) {
        if self.unavailable.is_some() {
            return;
        }
        self.unavailable = match &event.kind {
            EventKind::TurnCompleted { outcome, .. }
                if outcome == crate::events::PROVIDER_FAILED =>
            {
                Some(Withheld::ProviderFailed(format!(
                    "turn {} ended with a provider failure before dispatch; no stimulus while the provider is unavailable",
                    event.turn.unwrap_or(0)
                )))
            }
            EventKind::RunEnd { status, error }
                if status.as_deref() == Some(CUE_PROTOCOL_INVALID) =>
            {
                Some(Withheld::ProtocolFailed(format!(
                    "cue or relay protocol failed before dispatch: {}",
                    error.as_deref().unwrap_or("no detail")
                )))
            }
            EventKind::RunEnd { .. } if event.source == EventSource::Cue => Some(
                Withheld::SessionEnded("the runtime sent its run_end cue before dispatch".into()),
            ),
            EventKind::RunEnd { .. } => Some(Withheld::SessionEnded(
                match self.session.as_ref().and_then(|s| s.end.as_ref()) {
                    Some(SessionLine::SessionEnd {
                        exit_code,
                        killed,
                        error,
                        ..
                    }) => format!(
                        "the relay ended the session before dispatch (exit status {}{}{})",
                        exit_code.map_or("unknown".into(), |c| c.to_string()),
                        if *killed { ", killed" } else { "" },
                        error.as_deref().map_or(String::new(), |e| format!(": {e}"))
                    ),
                    _ => "the runtime session ended before dispatch".into(),
                },
            )),
            _ => None,
        };
    }

    /// Convert one relayed line, live or in the final dispatch drain, and
    /// latch any terminal cause. A relay framing error is a protocol failure:
    /// the lost line may have been a cue or the session's end.
    fn take(&mut self, line: std::result::Result<SessionLine, String>) -> Option<RuntimeEvent> {
        // The relay streams the final trace only after the runtime exited or
        // was killed: live admission closes at the first record (MAIN-030).
        if matches!(line, Ok(SessionLine::Trace { .. })) && self.unavailable.is_none() {
            self.unavailable = Some(Withheld::SessionEnded(
                "the runtime has exited; the relay is streaming its final trace".into(),
            ));
        }
        let event = match line {
            Ok(line) => self.handle(line),
            Err(error) => {
                let session = self.session.as_mut()?;
                if session.cue_failed {
                    session.protocol_errors.push(error);
                    return None;
                }
                Some(cue_failure(
                    session,
                    format!("relay protocol failure: {error}"),
                    (self.now)().as_micros(),
                ))
            }
        };
        // Held cues a live clock sample has just decided, in their order.
        let released: Vec<RuntimeEvent> = self
            .session
            .as_mut()
            .map(|session| session.released.drain(..).collect())
            .unwrap_or_default();
        for released in released {
            self.note(&released);
            self.pending.push_back(released);
        }
        let event = event?;
        self.note(&event);
        Some(event)
    }

    fn send_ping(&mut self, seq: u64) -> Result<()> {
        let Some(session) = self.session.as_mut() else {
            return Ok(());
        };
        let Some(stdin) = session.stdin.as_mut() else {
            return Ok(());
        };
        let sent = (self.now)().as_micros();
        serde_json::to_writer(&mut *stdin, &RunnerLine::Ping { seq })?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        if let Some(fixture) = &mut self.fixture {
            fixture.sent = fixture
                .sent
                .checked_add(1)
                .ok_or_else(|| invalid("fixture control frame count overflow"))?;
        }
        session.pings.push((seq, sent));
        session.next_ping_us = sent + PING_INTERVAL_US;
        Ok(())
    }

    /// Convert one relay line into a runtime event, updating the clock map.
    fn handle(&mut self, line: SessionLine) -> Option<RuntimeEvent> {
        let received = (self.now)().as_micros();
        let serve = self.serve();
        // Human speech has no inferred onset window. Its timing must be
        // reviewed; timed acceptance is withheld by the evaluator.
        let path_allowance = match self.config.stimulus_source {
            StimulusSource::DirectHuman => 0,
            _ => 250_000,
        };
        let session = self.session.as_mut()?;
        let mut event = match line {
            SessionLine::Pong { seq, lamp_us } => {
                if let Some(&(_, sent)) = session.pings.iter().find(|(s, _)| *s == seq) {
                    let rtt = received.saturating_sub(sent);
                    let midpoint = sent + rtt / 2;
                    let mapping = ClockMapping {
                        from: ClockDomain::RunnerMonotonic,
                        to: ClockDomain::LampMonotonic,
                        offset_us: lamp_us as i64 - midpoint as i64,
                        uncertainty_us: rtt / 2 + 1_000,
                        path_allowance_us: path_allowance,
                        method: format!(
                            "ping/pong over the session transport, best round trip {rtt} us"
                        ),
                    };
                    // A ping written before session_start was taken waited
                    // while the transport connected: its round trip is setup,
                    // not the live path, and never defines the clock map.
                    let usable = session.started_runner_us.is_some_and(|start| sent >= start);
                    let better = usable
                        && session
                            .best
                            .as_ref()
                            .is_none_or(|best| mapping.uncertainty_us < best.uncertainty_us);
                    if session.clock_samples.len() < MAX_PINGS as usize {
                        session.clock_samples.push(json!({"seq": seq, "sent_runner_us": sent,
                            "received_runner_us": received, "rtt_us": rtt, "lamp_us": lamp_us,
                            "offset_us": mapping.offset_us, "uncertainty_us": mapping.uncertainty_us,
                            "usable": usable, "became_best": better}));
                    }
                    if better {
                        session.best = Some(mapping);
                        release_held(session, || (self.now)().as_micros());
                    }
                }
                return None;
            }
            SessionLine::Cue {
                lamp_received_us,
                cue,
            } => {
                if session.cue_failed {
                    return None;
                }
                // Before a live clock map (or behind cues already waiting), a
                // cue waits in order with its original receipt times; nothing
                // is published from it until a map can decide it.
                if session.start.is_some() && (session.best.is_none() || !session.held.is_empty()) {
                    if session.held.len() >= MAX_HELD_CUES {
                        return Some(cue_failure(
                            session,
                            format!(
                                "more than {MAX_HELD_CUES} cues arrived before a live clock map"
                            ),
                            received,
                        ));
                    }
                    session.held.push_back((received, lamp_received_us, cue));
                    return None;
                }
                return decide_cue(session, received, lamp_received_us, cue, || {
                    (self.now)().as_micros()
                });
            }
            SessionLine::CueInvalid { error, .. } => {
                if session.cue_failed {
                    return None;
                }
                cue_failure(session, format!("relay rejected a cue: {error}"), received)
            }
            SessionLine::Stdout {
                lamp_received_us,
                line,
            } => {
                let status: Value = serde_json::from_str(&line).ok()?;
                if session.cue_failed
                    || session.start.is_none()
                    || status["status"] != "listening_ready"
                {
                    return None;
                }
                // Serve readiness comes only from the accepted schema-1 cue,
                // which carries the runtime's own event time. A stdout line
                // has only the relay's receipt time and may be buffered: it
                // is kept as a diagnostic, never as a readiness event.
                if serve {
                    if session.stdout_readiness_us.len() < 16 {
                        session.stdout_readiness_us.push(lamp_received_us);
                    }
                    return None;
                }
                if std::mem::replace(&mut session.ready_seen, true) {
                    return None;
                }
                RuntimeEvent::new(
                    EventKind::ListeningReady,
                    None,
                    lamp_received_us,
                    ClockDomain::LampMonotonic,
                    EventSource::Stdout,
                )
            }
            SessionLine::StdoutInvalid { error, .. } => {
                if session.cue_failed {
                    return None;
                }
                // The unreadable line may have been readiness or a failure report.
                cue_failure(
                    session,
                    format!("relay could not deliver a runtime stdout line: {error}"),
                    received,
                )
            }
            SessionLine::Trace { record } => {
                if session.trace.len() < crate::physical::protocol::MAX_TRACE_LINES {
                    session.trace.push(record);
                }
                return None;
            }
            end @ SessionLine::SessionEnd { .. } => {
                let SessionLine::SessionEnd { lamp_us, .. } = end else {
                    unreachable!()
                };
                session.end = Some(end);
                // The authoritative relay end also closes attempts whose end cue was lost.
                RuntimeEvent::new(
                    EventKind::RunEnd {
                        status: None,
                        error: None,
                    },
                    None,
                    lamp_us,
                    ClockDomain::LampMonotonic,
                    EventSource::Stdout,
                )
            }
            start @ SessionLine::SessionStart { .. } => {
                let SessionLine::SessionStart {
                    schema,
                    ref mode,
                    cue_socket,
                    ..
                } = start
                else {
                    unreachable!()
                };
                let expected = match self.config.mode {
                    LampMode::Fixture { .. } => "fixture",
                    LampMode::Directed { .. } => "directed",
                    LampMode::Serve { .. } => "serve",
                };
                if session.start.is_some()
                    || schema != crate::physical::protocol::SCHEMA
                    || mode != expected
                    || !cue_socket
                {
                    cue_failure(
                        session,
                        "relay did not establish the requested cue capability".into(),
                        received,
                    )
                } else {
                    session.start = Some(serde_json::to_value(start).ok()?);
                    // The relay now reads runner input: measure it at once.
                    session.started_runner_us = Some(received);
                    session.next_ping_us = 0;
                    return None;
                }
            }
        };
        // Both modes report readiness on stdout and the cue socket.
        if matches!(event.kind, EventKind::ListeningReady)
            && event.source == EventSource::Cue
            && std::mem::replace(&mut session.ready_seen, true)
        {
            return None;
        }
        event.received_us = Some(received);
        Some(event)
    }

    fn wait_line(&mut self, until_us: u64) -> Option<std::result::Result<SessionLine, String>> {
        let session = self.session.as_mut()?;
        Self::receive_line(
            session,
            until_us,
            || (self.now)().as_micros(),
            self.fixture.is_some(),
        )
    }

    /// After session_end only real transport work remains. Its drain must
    /// finish even if the fixture clock has stopped with the runtime.
    fn wait_line_on_host(
        &mut self,
        until_us: u64,
    ) -> Option<std::result::Result<SessionLine, String>> {
        Self::receive_line(self.session.as_mut()?, until_us, monotonic_us, false)
    }

    fn receive_line(
        session: &mut Session,
        until_us: u64,
        now_us: impl FnOnce() -> u64,
        nonblocking: bool,
    ) -> Option<std::result::Result<SessionLine, String>> {
        let now = now_us();
        // A fixture driver has already acknowledged every announced frame.
        // Its next wait is another scheduler step, never a host-time delay.
        let received = if nonblocking || now >= until_us {
            session.lines.try_recv().map_err(|error| match error {
                TryRecvError::Empty => RecvTimeoutError::Timeout,
                TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
            })
        } else {
            session
                .lines
                .recv_timeout(Duration::from_micros(until_us - now))
        };
        match received {
            Ok(line) => Some(line),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => {
                if session.end.is_none() {
                    session.end_synthesized = true;
                    session.end = Some(SessionLine::SessionEnd {
                        lamp_us: 0,
                        exit_code: None,
                        killed: false,
                        events_sha256: None,
                        events_lines: 0,
                        error: Some("session transport closed without session_end".into()),
                        stray_processes_stopped: false,
                        serve: None,
                    });
                    return Some(Ok(session.end.clone()?));
                }
                None
            }
        }
    }
}

/// An error or panic between `begin` and `finish` must not leave the
/// transport, the room recorder or a player running. Closing the transport's
/// input makes the Lamp relay stop the runtime; the transport itself is
/// killed if it has not exited shortly afterwards.
impl Drop for PhysicalBackend<'_> {
    fn drop(&mut self) {
        // Closing the private control peers unblocks fixture hooks before the
        // original transport/group cleanup waits for real process exit.
        drop(self.fixture.take());
        self.release_armed();
        for playback in &self.playbacks {
            let _ = fs::write(&playback.cancel_file, b"cancel\n");
        }
        if let Some(mut session) = self.session.take() {
            drop(session.stdin.take());
            let _ = wait_child(&mut session.child, monotonic_us() + 2_000_000);
        }
        if let Some((mut child, _, _)) = self.room.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Frame relay lines with the protocol's per-line and per-session bounds on
/// a separate thread. Any framing or parse failure becomes a protocol error.
/// Above the session byte bound the reader stops, so the transport looks
/// closed and the attempt is invalidated rather than buffered without limit.
fn relay_reader<R: std::io::Read + Send + 'static>(
    stdout: R,
) -> Receiver<std::result::Result<SessionLine, String>> {
    relay_reader_with_progress(stdout, None)
}

fn relay_reader_with_progress<R: std::io::Read + Send + 'static>(
    stdout: R,
    enqueued: Option<Arc<AtomicU64>>,
) -> Receiver<std::result::Result<SessionLine, String>> {
    relay_reader_with_progress_capacity(stdout, enqueued, FIXTURE_RELAY_QUEUE_CAPACITY)
}

fn relay_reader_with_progress_capacity<R: std::io::Read + Send + 'static>(
    stdout: R,
    enqueued: Option<Arc<AtomicU64>>,
    fixture_capacity: usize,
) -> Receiver<std::result::Result<SessionLine, String>> {
    let controlled = enqueued.is_some();
    // Controlled output is bounded over the whole transcript, not just its
    // current backlog. One extra slot always remains for a terminal error,
    // even when the driver is waiting for an Ack and consumes no output yet.
    let capacity = if controlled {
        fixture_capacity + 1
    } else {
        4096
    };
    let (sender, lines) = mpsc::sync_channel(capacity);
    thread::spawn(move || {
        let terminal = |error| {
            if controlled {
                let _ = sender.try_send(Err(error));
            } else {
                let _ = sender.send(Err(error));
            }
        };
        let mut forwarded = 0;
        // Raw bytes are counted before framing, so even one endless line
        // without a newline ends the session at the bound.
        let mut framer = LineFramer::with_total_limit(stdout, MAX_LINE_BYTES, MAX_SESSION_BYTES);
        loop {
            let frame = match framer.next_frame() {
                Ok(Some(frame)) => frame,
                Ok(None) => break,
                Err(error) if error.to_string() == STREAM_LIMIT => {
                    terminal(format!(
                        "relay exceeded {MAX_SESSION_BYTES} bytes in one session; reading stopped"
                    ));
                    break;
                }
                Err(error) => {
                    terminal(format!("relay transport read failed: {error}"));
                    break;
                }
            };
            if controlled && forwarded == fixture_capacity {
                terminal(format!(
                    "fixture relay exceeded {fixture_capacity} framed records in one session; reading stopped"
                ));
                // Dropping the framer closes stdout, so a relay blocked while
                // writing cannot remain stuck waiting for the driver to drain.
                break;
            }
            let complete = matches!(
                &frame,
                Frame::Line(_)
                    | Frame::InvalidUtf8 { .. }
                    | Frame::Oversized {
                        terminated: true,
                        ..
                    }
            );
            let parsed = match frame {
                Frame::Line(line) => serde_json::from_str::<SessionLine>(&line)
                    .map_err(|e| format!("{e}: {line:.200}")),
                other => Err(other
                    .failure("relay transport", MAX_LINE_BYTES)
                    .unwrap_or_default()),
            };
            if controlled {
                // This is the sole producer and fewer than fixture_capacity
                // records have been sent. Full is impossible with the extra
                // error slot; a disconnected consumer ends the reader too.
                if sender.try_send(parsed).is_err() {
                    break;
                }
            } else if sender.send(parsed).is_err() {
                break;
            }
            forwarded += 1;
            if complete && let Some(enqueued) = &enqueued {
                enqueued.fetch_add(1, Ordering::Release);
            }
        }
    });
    lines
}

fn wait_child(child: &mut Child, deadline_us: u64) -> Option<std::process::ExitStatus> {
    wait_child_with(child, deadline_us, monotonic_us, thread::sleep)
}

fn wait_child_with(
    child: &mut Child,
    deadline_us: u64,
    mut now_us: impl FnMut() -> u64,
    mut wait: impl FnMut(Duration),
) -> Option<std::process::ExitStatus> {
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status);
        }
        if now_us() >= deadline_us {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        wait(Duration::from_millis(20));
    }
}

fn join_bounded(handle: JoinHandle<Value>, cancel_file: &Path, deadline_us: u64) -> Value {
    while !handle.is_finished() && monotonic_us() < deadline_us {
        thread::sleep(Duration::from_millis(20));
    }
    if !handle.is_finished() {
        let _ = fs::write(cancel_file, b"cancel\n");
        let grace = monotonic_us() + 2_000_000;
        while !handle.is_finished() && monotonic_us() < grace {
            thread::sleep(Duration::from_millis(20));
        }
    }
    if handle.is_finished() {
        handle
            .join()
            .unwrap_or_else(|_| json!({"valid": false, "status": "player thread panicked"}))
    } else {
        json!({"valid": false, "status": "playback did not finish within its bound; detached"})
    }
}

impl Backend for PhysicalBackend<'_> {
    fn describe(&self) -> Value {
        json!({
            "kind": "physical",
            "mode": match &self.config.mode {
                LampMode::Fixture { sha256, .. } => json!({"provider": "fixed_reply", "reply_sha256": sha256}),
                LampMode::Directed { .. } => json!({"provider": "gemini", "cues": "schema-1 socket required; software scheduling only"}),
                LampMode::Serve { .. } => json!({"provider": "gemini", "runtime": "lamp-voice serve, one per attempt",
                    "cues": "schema-1 socket required; software scheduling only",
                    "start": "prepared player, one original expiring grant enforced at its first source copy",
                    "stop": "relay SIGTERM to the runtime, bounded grace, forced group kill"}),
            },
            "stimulus_source": self.config.stimulus_source,
            "noise_suppression": self.config.noise_suppression,
            "runtime_flags": self.config.runtime_flags.relay_argv(),
            "lamp_diagnostics": self.config.diagnostics,
            "player": self.player.describe(),
            "room_recorder": self.config.room_recorder.as_ref().map(|argv| argv.first().cloned()),
            "room_recorder_independent_of_lamp": self.config.room_independent,
            "assets": self.assets.scenes().map(|a| json!({"scene": a.scene, "wav_sha256": a.wav_sha256})).collect::<Vec<_>>(),
        })
    }
    fn stratum(&self, _: &Scenario) -> Stratum {
        match self.config.mode {
            LampMode::Fixture { .. } => Stratum::PhysicalFixture,
            LampMode::Directed { .. } => Stratum::PhysicalGemini,
            LampMode::Serve { .. } => Stratum::PhysicalServe,
        }
    }
    fn domain(&self) -> ClockDomain {
        ClockDomain::RunnerMonotonic
    }
    fn source(&self) -> StimulusSource {
        self.config.stimulus_source
    }
    fn unsupported(&self, scenario: &Scenario) -> Option<String> {
        if !scenario.physical {
            return Some("needs provider fault injection, which only the fake runtime has".into());
        }
        match (&self.config.mode, scenario.provider) {
            (LampMode::Fixture { .. }, ProviderKind::Gemini) => {
                return Some("Gemini scenario cannot run on the fixed-reply provider".into());
            }
            (LampMode::Directed { .. } | LampMode::Serve { .. }, ProviderKind::FixedReply) => {
                return Some("fixed-reply scenario needs lamp-voice directed-fixture".into());
            }
            _ => {}
        }
        scenario
            .steps
            .iter()
            .filter_map(|s| s.scene.as_deref())
            .find(|scene| self.assets.get(scene).is_none())
            .map(|scene| format!("no verified cached asset for scene {scene}; cached speech is not regenerated by the runner"))
    }
    fn begin(&mut self, context: &AttemptContext) -> Result<Begin> {
        if let Some(fixture) = &mut self.fixture
            && std::mem::replace(&mut fixture.started, true)
        {
            return Err(invalid("a fixture driver owns exactly one attempt"));
        }
        let directory = context.run_directory.join(&context.attempt_id);
        crate::ledger::create_run_directory(&directory)?;
        self.attempt_directory = Some(directory.clone());
        self.seconds = context.scenario.session_seconds;
        self.playbacks.clear();
        self.pending.clear();
        self.unavailable = None;
        self.release_armed();
        self.arm_failure = None;
        self.trigger = None;
        self.evidence_lost = false;
        self.room_cover = RoomCover {
            observe_us: u64::from(context.scenario.observe_ms) * 1000,
            ..RoomCover::default()
        };
        let total_seconds = u64::from(self.seconds) + u64::from(self.config.allowance_seconds) + 10;
        if let Some(template) = &self.config.room_recorder {
            let out = directory.join("room");
            let argv: Vec<String> = template
                .iter()
                .map(|a| {
                    a.replace("{out}", &out.display().to_string())
                        .replace("{seconds}", &total_seconds.to_string())
                })
                .collect();
            let requested = (self.now)().as_micros();
            let child = Command::new(&argv[0])
                .args(&argv[1..])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()?;
            // Stimuli start only after the recorder reports readiness.
            let ready_deadline = monotonic_us() + 15_000_000;
            if self.fixture_recorder() {
                let remaining = ready_deadline.saturating_sub(monotonic_us());
                let result = self
                    .fixture
                    .as_mut()
                    .unwrap()
                    .driver
                    .start_recorder(Instant::now() + Duration::from_micros(remaining));
                if let Err(error) = result {
                    let reason = format!("fixture recorder startup failed: {error}");
                    self.fail_fixture_recorder(reason.clone());
                    let mut child = child;
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(Begin::Withheld(reason));
                }
            }
            while !out.join("ready.json").exists() && monotonic_us() < ready_deadline {
                thread::sleep(Duration::from_millis(50));
            }
            if !out.join("ready.json").exists() {
                if self.fixture_recorder() {
                    self.fail_fixture_recorder(
                        "room recorder did not report ready.json within 15 s".into(),
                    );
                }
                let mut child = child;
                let _ = child.kill();
                let _ = child.wait();
                return Ok(Begin::Withheld(
                    "room recorder did not report ready.json within 15 s".into(),
                ));
            }
            let (ready, end_us) = ready_receipt(&out.join("ready.json"));
            self.room_cover.ready = ready;
            self.room_cover.end_us = end_us;
            self.room = Some((child, out, requested));
            if self.fixture_recorder() {
                let remaining = ready_deadline.saturating_sub(monotonic_us());
                if let Err(error) = self
                    .settle_fixture_recorder_exit(Instant::now() + Duration::from_micros(remaining))
                {
                    return Ok(Begin::Withheld(format!(
                        "fixture recorder startup failed: {error}"
                    )));
                }
            }
        }
        let mut argv = self.config.lamp_command.clone();
        let work = format!(
            "{}/{}",
            self.config.lamp_work_root.trim_end_matches('/'),
            context.attempt_id
        );
        argv.extend([
            "--work".into(),
            work,
            "--seconds".into(),
            self.seconds.to_string(),
            "--noise-suppression".into(),
            self.config.noise_suppression.clone(),
            "--allowance-seconds".into(),
            self.config.allowance_seconds.to_string(),
        ]);
        match &self.config.mode {
            LampMode::Fixture { reply, sha256 } => {
                argv.extend([
                    "--fixture-reply".into(),
                    reply.clone(),
                    "--fixture-sha256".into(),
                    sha256.clone(),
                ]);
            }
            LampMode::Directed { provider_config } => {
                argv.extend(["--provider-config".into(), provider_config.clone()]);
                if self.config.diagnostics {
                    argv.push("--diagnostics".into());
                }
            }
            LampMode::Serve { provider_config } => {
                argv.extend(["--serve-config".into(), provider_config.clone()]);
            }
        }
        if let Some(ceiling) = self.config.ring_channel_ceiling {
            argv.extend(["--ring-channel-ceiling".into(), ceiling.to_string()]);
        }
        // The relay validates these again before it starts lamp-voice, and
        // gives lamp-voice the route and settings in its environment.
        argv.extend(self.config.runtime_flags.relay_argv());
        let mut child = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| invalid("session stdout unavailable"))?;
        let lines = match &self.fixture {
            Some(fixture) => relay_reader_with_progress(stdout, Some(fixture.enqueued.clone())),
            None => relay_reader(stdout),
        };
        self.session = Some(Session {
            stdin: child.stdin.take(),
            next_ping_us: 0,
            child,
            lines,
            pings: Vec::new(),
            best: None,
            trace: Vec::new(),
            end: None,
            end_synthesized: false,
            ready_seen: false,
            protocol_errors: Vec::new(),
            start: None,
            cues: Vec::new(),
            cue_order: CueOrder::default(),
            cue_failed: false,
            boot_changes: Vec::new(),
            stop_sent: false,
            runtime_ended: false,
            stdout_readiness_us: Vec::new(),
            clock_samples: Vec::new(),
            started_runner_us: None,
            held: std::collections::VecDeque::new(),
            released: std::collections::VecDeque::new(),
        });
        self.send_ping(1)?;
        fs::write(
            directory.join("lamp-command.json"),
            serde_json::to_vec_pretty(&json!({"argv": argv}))?,
        )?;
        Ok(Begin::Ready)
    }
    fn now_us(&mut self) -> u64 {
        (self.now)().as_micros()
    }
    fn next_event(&mut self, until_us: u64) -> Result<Option<RuntimeEvent>> {
        if let Some(event) = self.pending.pop_front() {
            return Ok(Some(event));
        }
        loop {
            if let Some(event) = self.poll_evidence() {
                return Ok(Some(event));
            }
            if let Some(event) = self.expire_held((self.now)().as_micros()) {
                return Ok(Some(event));
            }
            let (due, sent) = self.session.as_ref().map_or((u64::MAX, MAX_PINGS), |s| {
                (s.next_ping_us, s.pings.len() as u64)
            });
            if sent < MAX_PINGS && (self.now)().as_micros() >= due {
                // A failed write means the transport closed; session_end reports it.
                let _ = self.send_ping(sent + 1);
            }
            let mut wait_until = until_us.min(self.session.as_ref().map_or(until_us, |s| {
                s.next_ping_us.max((self.now)().as_micros() + 1_000)
            }));
            if self.serve() {
                // Player and recorder ends are polled, not signalled.
                wait_until = wait_until.min((self.now)().as_micros() + 20_000);
            }
            if let Some(due) = self.held_due() {
                wait_until = wait_until.min(due);
            }
            let mut line = self.wait_line(wait_until);
            if line.is_none() && self.fixture.is_some() {
                self.pump_fixture(wait_until, true)?;
                line = self.wait_line(wait_until);
            }
            let Some(line) = line else {
                if (self.now)().as_micros() >= until_us {
                    return Ok(None);
                }
                continue;
            };
            if let Some(event) = self.take(line) {
                return Ok(Some(event));
            }
            // Held cues a clock sample has just decided, in their order.
            if let Some(event) = self.pending.pop_front() {
                return Ok(Some(event));
            }
            if (self.now)().as_micros() >= until_us {
                return Ok(None);
            }
        }
    }
    fn runner_time(&self, event: &RuntimeEvent) -> (u64, String) {
        let mapping = self.session.as_ref().and_then(|s| s.best.as_ref());
        match (mapping, event.at_us, event.domain) {
            (Some(mapping), Some(at), ClockDomain::LampMonotonic) => (
                mapping.to_runner(at),
                format!(
                    "Lamp event time mapped to the runner clock (+/- {} us)",
                    mapping.uncertainty_us
                ),
            ),
            _ => (
                event
                    .received_us
                    .unwrap_or_else(|| (self.now)().as_micros()),
                "runner receipt time".into(),
            ),
        }
    }
    fn timing(&mut self, catalog: &StimulusCatalog, scene: &str) -> Result<SceneTiming> {
        let mut timing = catalog.estimate(scene)?;
        timing.measure(&self.assets.pcm(scene)?);
        Ok(timing)
    }
    /// Serve: prepare this step's stimulus now, before its trigger wait.
    /// A failure is kept and withholds the step at dispatch.
    fn arm(&mut self, step: &Step, scene: &str, timing: &SceneTiming) -> Result<()> {
        if !self.serve() {
            return Ok(());
        }
        self.release_armed();
        self.arm_failure = None;
        let asset = self
            .assets
            .get(scene)
            .ok_or_else(|| invalid("asset disappeared"))?;
        let directory = self
            .attempt_directory
            .clone()
            .ok_or_else(|| invalid("no attempt directory"))?;
        let cancel_file = directory.join(format!("cancel-{}", step.id));
        let max_wait_ms =
            (step.trigger.deadline_ms + step.trigger.delay_ms + 5_000).min(MAX_PREPARED_WAIT_MS);
        let armed_us = (self.now)().as_micros();
        let prepared = match self.player.prepare(
            &asset.wav,
            timing,
            &directory.join(format!("play-{}", step.id)),
            &cancel_file,
            max_wait_ms,
        ) {
            Ok(Some(preparing)) if self.fixture.is_some() => {
                await_ready_fixture(preparing, |deadline| {
                    self.consume_transport();
                    let until = (self.now)()
                        .as_micros()
                        .checked_add(2_000)
                        .ok_or_else(|| invalid("fixture preparation clock overflow"))?;
                    self.pump_preparation_fixture(until, deadline)?;
                    self.consume_transport();
                    Ok(())
                })
                .map(Some)
            }
            Ok(Some(preparing)) => await_ready(preparing, || self.consume_transport()).map(Some),
            other => other.map(|_| None),
        };
        let failure = match prepared {
            Ok(Some(prepared)) if prepared.ready["source_sha256"] == asset.wav_sha256.as_str() => {
                self.armed = Some(Armed {
                    step: step.id.clone(),
                    prepared,
                    cancel_file,
                    armed_us,
                    speech_end_us: speech_end_us(timing),
                });
                return Ok(());
            }
            Ok(Some(prepared)) => {
                let reason = format!(
                    "prepared source {} is not the cached asset {}",
                    prepared.ready["source_sha256"], asset.wav_sha256
                );
                if prepared.grant.try_send(GrantInput::Closed).is_ok()
                    && let Err(error) = self.notify_prepared_input(&prepared.ready)
                {
                    self.fixture_player_error(error.to_string());
                }
                self.unused
                    .push((step.id.clone(), prepared.ready, prepared.handle));
                reason
            }
            Ok(None) => "the player has no prepared start".into(),
            Err(error) => error.to_string(),
        };
        self.arm_failure = Some((step.id.clone(), failure));
        Ok(())
    }
    fn trigger_observed(&mut self, observation: &TriggerObservation, delay_us: u64) {
        self.trigger = Some((observation.clone(), delay_us));
    }
    fn inject(
        &mut self,
        step: &Step,
        scene: &str,
        timing: &SceneTiming,
        at_us: u64,
    ) -> Result<Injection> {
        let asset = self
            .assets
            .get(scene)
            .ok_or_else(|| invalid("asset disappeared"))?;
        let directory = self
            .attempt_directory
            .clone()
            .ok_or_else(|| invalid("no attempt directory"))?;
        while (self.now)().as_micros() < at_us {
            let remaining = at_us.saturating_sub((self.now)().as_micros());
            if self.fixture.is_some() {
                let until = (self.now)()
                    .as_micros()
                    .saturating_add(remaining.min(1_000));
                self.pump_fixture(until, false)?;
            } else {
                thread::sleep(Duration::from_micros(remaining.min(1_000)));
            }
        }
        // Final dispatch check: take the lines already relayed, within a
        // budget (keeping the events for the runner), and refuse to start
        // after any terminal cause latched on this attempt, including one
        // first seen in this drain.
        let mut exhausted = true;
        for _ in 0..MAX_DISPATCH_DRAIN_LINES {
            let Some(line) = self.wait_line((self.now)().as_micros()) else {
                exhausted = false;
                break;
            };
            if let Some(event) = self.take(line) {
                self.pending.push_back(event);
            }
        }
        if let Some(event) = self.expire_held((self.now)().as_micros()) {
            self.pending.push_back(event);
        }
        if self.session.is_none() {
            self.unavailable
                .get_or_insert_with(|| Withheld::SessionEnded("no runtime session is open".into()));
        }
        let refuse = |withheld: Withheld, pending: usize, exhausted: bool, now: u64| Injection {
            started_us: now,
            receipt: json!({"wav_sha256": asset.wav_sha256, "dispatched": false,
                    "withheld_cause": withheld.cause(), "reason": withheld.reason(),
                    "unobserved_events": pending, "drain_exhausted": exhausted,
                    "late_us": now.saturating_sub(at_us)}),
            withheld: Some(withheld),
        };
        // Never start while the runner has not observed every consumed
        // event, or while lines remain undrained: one may end the overlap or
        // the session (MAIN-025, MAIN-030).
        // Cues still waiting for a live clock map are unobserved evidence too.
        let held = self.session.as_ref().is_some_and(|s| !s.held.is_empty());
        let withheld = self.unavailable.clone().or_else(|| {
            (exhausted || held || !self.pending.is_empty()).then_some(Withheld::Revalidate)
        });
        if let Some(withheld) = withheld {
            return Ok(refuse(
                withheld,
                self.pending.len(),
                exhausted,
                (self.now)().as_micros(),
            ));
        }
        let cancel_file = directory.join(format!("cancel-{}", step.id));
        // The original target's lateness bound holds at the side effect
        // itself, even when no new event surfaced (MAIN-030).
        let requested_us = (self.now)().as_micros();
        if requested_us > at_us.saturating_add(MAX_TRIGGER_LATENESS_US) {
            let stale = Withheld::Stale(format!(
                "planned start passed {} ms before the player could start",
                (requested_us - at_us) / 1000
            ));
            return Ok(refuse(stale, 0, false, (self.now)().as_micros()));
        }
        // A prompt starts only while the room recording covers it and the
        // scenario's observation window. Otherwise the loss becomes a runner
        // event and this and every later step is withheld.
        if !self.evidence_lost
            && let Some(reason) = self.room_uncovered(requested_us, timing)
        {
            let event = self.lose_evidence("room_recorder".into(), reason);
            self.pending.push_back(event);
            return Ok(refuse(
                Withheld::Revalidate,
                self.pending.len(),
                false,
                (self.now)().as_micros(),
            ));
        }
        if self.serve() {
            return Ok(match self.grant(step) {
                Ok((playback, prepared)) => {
                    let started_us = playback.requested_us;
                    self.playbacks.push(playback);
                    Injection {
                        withheld: None,
                        started_us,
                        receipt: json!({"wav_sha256": asset.wav_sha256, "dispatched": true,
                            "prepared": prepared, "late_us": started_us.saturating_sub(at_us),
                            "boundary": "grant sent; the player checks the expiry with a fresh CLOCK_MONOTONIC sample before its first source copy; the acoustic onset is later"}),
                    }
                }
                Err(withheld) => refuse(withheld, 0, false, (self.now)().as_micros()),
            });
        }
        let handle = self.player.start(
            &asset.wav,
            timing,
            &directory.join(format!("play-{}", step.id)),
            &cancel_file,
        )?;
        self.playbacks.push(Playback {
            step: step.id.clone(),
            requested_us,
            cancel_file,
            handle: Some(handle),
            report: None,
            prepared: false,
            latest_start_us: latest_covered_start(
                self.room_cover.end_us,
                speech_end_us(timing),
                self.room_cover.observe_us,
            ),
        });
        Ok(Injection {
            withheld: None,
            started_us: requested_us,
            receipt: json!({"wav_sha256": asset.wav_sha256, "requested_runner_us": requested_us,
                "late_us": requested_us.saturating_sub(at_us),
                "boundary": "runner start request; the player's own start request and the acoustic onset are later"}),
        })
    }
    fn finish(&mut self, context: &AttemptContext) -> Result<Finished> {
        if self.serve() {
            // Nothing more is granted; the runtime is stopped through the
            // relay's one stop path. A failed write means the transport is
            // already closed, which the relay treats as a stop.
            self.release_armed();
            // A runtime that reported its own end is not stopped by the
            // runner, so its end keeps its own provenance; one that does not
            // exit within the bound is then stopped like any other.
            if self
                .session
                .as_ref()
                .is_some_and(|s| s.runtime_ended && s.end.is_none())
            {
                let until = (self.now)().as_micros() + SELF_END_BOUND_US;
                while self.session.as_ref().is_some_and(|s| s.end.is_none())
                    && (self.now)().as_micros() < until
                {
                    if self.next_event(until)?.is_none() {
                        break;
                    }
                }
            }
            if self.session.as_ref().is_some_and(|s| s.end.is_none()) {
                let reason = if self.session.as_ref().is_some_and(|s| s.runtime_ended) {
                    "runtime_reported_end_but_did_not_exit"
                } else {
                    "runner_finished"
                };
                let _ = self.send_stop(reason);
            }
        }
        let bound = (self.now)().as_micros()
            + (u64::from(self.seconds) + u64::from(self.config.allowance_seconds) + 30) * 1_000_000;
        while self.session.as_ref().is_some_and(|s| s.end.is_none())
            && (self.now)().as_micros() < bound
        {
            if self.next_event(bound)?.is_none() {
                break;
            }
        }
        // Keep the transport open until session_end has relayed the trace.
        while self.session.as_ref().is_some_and(|s| s.end.is_some()) {
            match self.wait_line_on_host(monotonic_us() + 300_000) {
                Some(Ok(line)) => {
                    let _ = self.handle(line);
                }
                _ => break,
            }
        }
        let Some(mut session) = self.session.take() else {
            // Session start failed: release the recorder and players, keep nothing.
            // Closing explicit control wakes parked workers before host joins.
            drop(self.fixture.take());
            if let Some((mut child, _, _)) = self.room.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            for playback in self.playbacks.drain(..) {
                if let Some(handle) = playback.handle {
                    let _ = join_bounded(handle, &playback.cancel_file, monotonic_us());
                }
            }
            for (_, _, handle) in self.unused.drain(..) {
                let _ = join_bounded(handle, Path::new("/nonexistent"), monotonic_us());
            }
            return Ok(Finished {
                events: Vec::new(),
                clock: None,
                evidence: json!({"room_audio": null}),
                aborted: Some("no lamp session was running".into()),
                unmapped: 0,
                spoken_failure_notice: None,
                corrected_starts: Vec::new(),
                failed_deliveries: Vec::new(),
            });
        };
        drop(session.stdin.take());
        #[cfg(test)]
        let exited = match &self.transport_exit_clock {
            Some(clock) => clock.wait(&mut session.child, TRANSPORT_EXIT_BOUND_US),
            None => wait_child(&mut session.child, monotonic_us() + TRANSPORT_EXIT_BOUND_US),
        };
        #[cfg(not(test))]
        let exited = wait_child(&mut session.child, monotonic_us() + TRANSPORT_EXIT_BOUND_US);
        // A serve attempt is certified only with its outer transport (the
        // relay, normally over SSH) reaped after a successful exit. A nominal
        // runtime end inside a failed or killed transport is not enough. The
        // runtime's and the transport's outcomes are kept separately.
        let transport_failure = match exited {
            _ if !self.serve() => None,
            None => Some(format!(
                "the session transport did not exit within {} s and was killed",
                TRANSPORT_EXIT_BOUND_US / 1_000_000
            )),
            Some(status) if !status.success() => Some(format!(
                "the session transport exited unsuccessfully ({status})"
            )),
            Some(_) => None,
        };
        // A partial trace must never be scored as a complete one.
        let aborted = match &session.end {
            _ if !session.protocol_errors.is_empty() => Some(format!(
                "invalid scheduling/transport evidence: {}",
                session.protocol_errors.join("; ")
            )),
            _ if session.start.is_none() => Some("no relay cue capability receipt".into()),
            None => Some(
                "lamp session did not report its end within the bound; transport killed".to_owned(),
            ),
            Some(_) if session.end_synthesized => {
                Some("session transport closed before session_end; the trace may be partial".into())
            }
            Some(SessionLine::SessionEnd {
                killed: true,
                error,
                ..
            }) => Some(match error {
                Some(error) => format!("the relay stopped the runtime: {error}"),
                None => "the relay killed the runtime at its hard deadline".into(),
            }),
            // A serve runtime that exited nonzero or by signal cannot have
            // completed its session, whatever its metadata says.
            Some(SessionLine::SessionEnd { exit_code, .. })
                if self.serve() && *exit_code != Some(0) =>
            {
                Some(format!(
                    "the serve runtime exited with status {}; not a completed serve session",
                    exit_code.map_or("none (signal or unknown)".into(), |code| code.to_string())
                ))
            }
            Some(SessionLine::SessionEnd {
                error: Some(error), ..
            }) => Some(format!("relay: {error}")),
            Some(SessionLine::SessionEnd { events_lines, .. })
                if *events_lines != session.trace.len() =>
            {
                Some(format!(
                    "received {} of {events_lines} trace lines",
                    session.trace.len()
                ))
            }
            _ => None,
        };
        let aborted = match (aborted, transport_failure) {
            (Some(reason), Some(transport)) => Some(format!("{reason}; {transport}")),
            (reason, transport) => reason.or(transport),
        };
        // Finish the recorder before the player's internal terminal drain can
        // advance shared time. Its actual exit is still observed by this owner.
        let recorder_finish_failure = if self.fixture_recorder() {
            self.finish_fixture_recorder(
                Instant::now() + Duration::from_secs(u64::from(self.seconds) + 60),
            )
            .err()
            .map(|error| format!("fixture recorder did not finish: {error}"))
        } else {
            None
        };
        let aborted = match (aborted, recorder_finish_failure) {
            (Some(reason), Some(recorder)) => Some(format!("{reason}; {recorder}")),
            (reason, recorder) => reason.or(recorder),
        };
        let playback_bound = monotonic_us() + 25_000_000;
        let player_finish_failure = if let Some(fixture) = &mut self.fixture
            && fixture.recorder_failure.is_none()
        {
            let remaining = playback_bound.saturating_sub(monotonic_us());
            fixture
                .driver
                .finish_players(Instant::now() + Duration::from_micros(remaining))
                .err()
                .map(|error| format!("fixture players did not finish: {error}"))
        } else {
            None
        };
        if self.fixture_recorder()
            && let Some(error) = &player_finish_failure
        {
            self.fail_fixture_recorder(error.clone());
        }
        let aborted = match (aborted, player_finish_failure) {
            (Some(reason), Some(players)) => Some(format!("{reason}; {players}")),
            (reason, players) => reason.or(players),
        };
        let mut playbacks = Vec::new();
        let mut corrected = Vec::new();
        let mut failed = Vec::new();
        // Prompts whose actual start passed the latest start the recording
        // covered: the grant bounds this, and the actual start is rechecked.
        let mut coverage_violations = Vec::new();
        for playback in self.playbacks.drain(..) {
            let report = match (playback.report, playback.handle) {
                (Some(report), _) => report,
                (None, Some(handle)) => join_bounded(handle, &playback.cancel_file, playback_bound),
                (None, None) => json!({"valid": false, "status": "no player report"}),
            };
            if report["valid"] != true {
                // Nothing reliable reached the room; the step tested nothing.
                failed.push((
                    playback.step.clone(),
                    format!(
                        "stimulus not delivered: {} {}",
                        report["status"], report["error"]
                    ),
                ));
            } else {
                // A prepared stream started before readiness; its stimulus
                // starts at the first source copy.
                let started_ns = if playback.prepared {
                    report["counters"]["source_started_host_ns"]
                        .as_u64()
                        .filter(|ns| *ns > 0)
                } else {
                    report["start_requested_host_monotonic_ns"].as_u64()
                };
                if let Some(ns) = started_ns {
                    let started_us = ns / 1000;
                    corrected.push((playback.step.clone(), started_us));
                    if let Some(latest) = playback.latest_start_us.filter(|l| started_us > *l) {
                        coverage_violations.push(json!({"step": playback.step,
                            "actual_start_runner_us": started_us, "latest_start_runner_us": latest,
                            "overrun_us": started_us - latest}));
                    }
                }
            }
            playbacks.push(
                json!({"step": playback.step, "runner_requested_us": playback.requested_us,
                "prepared": playback.prepared, "report": report}),
            );
        }
        let unused: Vec<Value> = self
            .unused
            .drain(..)
            .map(|(step, ready, handle)| {
                let report = join_bounded(handle, Path::new("/nonexistent"), monotonic_us() + 5_000_000);
                json!({"step": step, "ready": ready, "report": report,
                    "note": "prepared, never granted; no source frame was submitted unless the report says otherwise"})
            })
            .collect();
        let room = match self.room.take() {
            Some((mut child, out, requested)) => {
                let status = wait_child(
                    &mut child,
                    monotonic_us() + (u64::from(self.seconds) + 60) * 1_000_000,
                );
                let mut evidence = load_room_evidence(
                    &out.join("metadata.json"),
                    self.config.room_independent,
                    Some(status.is_some_and(|s| s.success())),
                );
                evidence["recorder_spawn_runner_us"] = json!(requested);
                evidence["recorder_exit"] = json!(status.and_then(|s| s.code()));
                evidence["recorder_ready_receipt"] = self.room_cover.ready.clone();
                evidence["declared_capture_end_runner_us"] = json!(self.room_cover.end_us);
                evidence["coverage_lost"] = self.room_cover.lost.as_ref().map_or(
                    Value::Null,
                    |(at, kind, reason)| json!({"runner_us": at, "kind": kind, "reason": reason}),
                );
                evidence["coverage_violations"] = json!(coverage_violations);
                evidence
            }
            None => Value::Null,
        };
        // Serve metadata can span service cycles. Only the first cycle is
        // the conversation; later cycles are counted, never scored with it.
        let cycle_starts: Vec<usize> = session
            .trace
            .iter()
            .enumerate()
            .filter(|(_, record)| record["kind"] == "service_cycle_start")
            .map(|(index, _)| index)
            .collect();
        let scored_end = cycle_starts.get(1).copied().unwrap_or(session.trace.len());
        let mut unmapped = 0;
        let mut events = Vec::new();
        for record in &session.trace[..scored_end] {
            match from_trace(record) {
                Some(event) => events.push(event),
                None => unmapped += 1,
            }
        }
        let continuity = self.serve().then(|| {
            let cycles: Vec<&Value> = session
                .trace
                .iter()
                .filter(|record| {
                    matches!(
                        record["kind"].as_str(),
                        Some("service_cycle_start" | "service_cycle_muted" | "service_stop_requested")
                    )
                })
                .take(64)
                .collect();
            // A restart is verified only by the runtime's own metadata: a
            // muted first cycle, then a second cycle started no later than
            // the first cue seen under the new boot.
            let first_new_boot = session.boot_changes.first().and_then(|c| c["event_lamp_us"].as_u64());
            let muted = session.trace.iter().any(|r| r["kind"] == "service_cycle_muted" && r["cycle"] == 1);
            let restarted_at = session
                .trace
                .iter()
                .find(|r| r["kind"] == "service_cycle_start" && r["cycle"] == 2)
                .and_then(|r| r["at_us"].as_u64());
            let verified = muted
                && restarted_at.zip(first_new_boot).is_some_and(|(start, cue)| start <= cue);
            json!({"service_cycles": cycle_starts.len(),
                "context_continuity_lost": cycle_starts.len() > 1 || !session.boot_changes.is_empty(),
                "boot_changes_observed": session.boot_changes,
                "verified_service_restart": verified,
                "cycle_records": cycles,
                "unscored_records_after_first_cycle": session.trace.len() - scored_end,
                "note": "a new boot ends the conversation; cues alone never qualify a restart"})
        });
        // Terminal provider failures and drain-path endpoints are kept
        // verbatim (bounded) so provider session and outcome survive scoring.
        let provider_records: Vec<&Value> = session
            .trace
            .iter()
            .filter(|record| {
                record["kind"] == "input_discarded_endpoint"
                    || (record["kind"] == "turn_finished"
                        && record["outcome"] == crate::events::PROVIDER_FAILED)
            })
            .take(32)
            .collect();
        // Cues never decided (no live clock map before the end) are kept
        // as refused receipts, never as scheduling evidence.
        while let Some((received, lamp_received_us, cue)) = session.held.pop_front() {
            if session.cues.len() < crate::physical::protocol::MAX_TRACE_LINES {
                session
                    .cues
                    .push(json!({"cue": cue, "lamp_received_us": lamp_received_us,
                    "runner_received_us": received, "accepted": false,
                    "error": "undecided: no live clock map before the session ended",
                    "freshness": {"mapping": null}}));
            }
        }
        let evidence = json!({
            "provider_failure_records": provider_records,
            "session_start": session.start,
            "session_end": session.end,
            "cue_receipts": session.cues,
            "clock_samples": session.clock_samples,
            "transport_exit": exited.map(|s| s.code()),
            "transport_status": exited.map(|s| s.to_string()),
            "transport_forced_kill": exited.is_none(),
            "protocol_errors": session.protocol_errors,
            "playbacks": playbacks,
            "unused_prepared_players": unused,
            "stdout_readiness_relay_us": session.stdout_readiness_us,
            "serve_continuity": continuity,
            "room_audio": room,
            "attempt_directory": self.attempt_directory.as_ref().map(|d| d.display().to_string()),
        });
        let _ = context;
        Ok(Finished {
            events,
            clock: session.best.clone(),
            evidence,
            aborted,
            unmapped,
            spoken_failure_notice: None,
            corrected_starts: corrected,
            failed_deliveries: failed,
        })
    }
    fn reproduce(&self, context: &AttemptContext) -> String {
        format!(
            "cargo run -p lamp-voice-eval -- physical-run --scenario {} --attempt-seed {} (with the same physical options as run {})",
            context.scenario.id, context.seed, context.run_id
        )
    }
}

#[cfg(test)]
mod cue_tests {
    use super::*;

    #[test]
    fn a_prompt_is_covered_only_through_its_speech_and_observation_window() {
        let timing = crate::stimulus::StimulusCatalog::load()
            .unwrap()
            .estimate("fast-followup")
            .unwrap();
        let speech_end_us = u64::from(timing.speech_span_ms.unwrap().1) * 1000;
        let (start, observe) = (10_000_000, 8_000_000);
        let needed = start + speech_end_us + observe;
        // No declared end: only the recorder's exit guards coverage.
        assert_eq!(coverage_shortfall(None, start, &timing, observe), None);
        assert_eq!(
            coverage_shortfall(Some(needed), start, &timing, observe),
            None
        );
        assert_eq!(
            coverage_shortfall(Some(needed - 1), start, &timing, observe),
            Some(1)
        );
        assert_eq!(
            coverage_shortfall(Some(start), start, &timing, observe),
            Some(speech_end_us + observe)
        );
    }

    #[test]
    fn a_grant_sent_after_its_expiry_keeps_a_negative_margin_without_overflow() {
        assert_eq!(
            grant_margin(1_000_100, 1_000_000),
            json!({"headroom_at_grant_us": 100, "expired_at_send": false})
        );
        // A scheduler pause after the pre-send check: the margin is negative
        // and the send is marked expired; nothing wraps or panics.
        assert_eq!(
            grant_margin(1_000_000, 1_000_150),
            json!({"headroom_at_grant_us": -150, "expired_at_send": true})
        );
        assert_eq!(grant_margin(5, 5)["expired_at_send"], true);
        assert_eq!(grant_margin(0, u64::MAX)["headroom_at_grant_us"], i64::MIN);
        assert_eq!(grant_margin(u64::MAX, 0)["headroom_at_grant_us"], i64::MAX);
    }

    fn cue(sequence: u64, kind: &str, turn: u64) -> Cue {
        Cue {
            schema: 1,
            sequence,
            kind: kind.into(),
            boot: [7; 16],
            turn: Some(turn),
            generation: Some(turn + 1),
            capture_epoch: Some(1),
            reference_epoch_context: Some(1),
            event_us: sequence * 10,
            sent_us: sequence * 10 + 1,
            expires_us: sequence * 10 + 100_000,
            reason: None,
            playback_sequence: matches!(kind, "speaker_first_write" | "speech_retired")
                .then_some(1),
            outcome: (kind == "turn_completed").then(|| "audio_written_unscored".into()),
            playback_gaps: (kind == "turn_completed").then_some(0),
            available: None,
            link: None,
        }
    }
    #[test]
    fn readiness_cues_are_unowned_but_still_sequenced() {
        let unowned = |sequence: u64, kind: &str| {
            let mut value = cue(sequence, kind, 1);
            value.turn = None;
            value.generation = None;
            value.available = (kind == "input_availability").then_some(true);
            value.link = (kind == "provider_link").then(|| "ready".into());
            value
        };
        let mut order = CueOrder::default();
        order.accept(&unowned(1, "provider_link"), 12).unwrap();
        order.accept(&unowned(2, "input_availability"), 22).unwrap();
        order.accept(&unowned(3, "input_candidate"), 32).unwrap();
        order.accept(&cue(4, "input_admitted", 1), 42).unwrap();
        // A gap in the sequence is still a protocol failure.
        assert!(order.accept(&unowned(6, "input_availability"), 62).is_err());
    }

    #[test]
    fn cue_order_rejects_missing_duplicate_relabelled_and_out_of_order_owners() {
        let mut order = CueOrder::default();
        order.accept(&cue(1, "input_admitted", 1), 12).unwrap();
        for bad in [
            cue(1, "input_admitted", 1),
            cue(3, "local_endpoint", 1),
            cue(2, "local_endpoint", 2),
            cue(2, "speech_retired", 1),
        ] {
            assert!(order.accept(&bad, 40).is_err(), "{bad:?}");
        }
        let mut bad_boot = cue(2, "local_endpoint", 1);
        bad_boot.boot = [8; 16];
        assert!(order.accept(&bad_boot, 25).is_err());
        let mut relabel = cue(2, "local_endpoint", 1);
        relabel.generation = Some(9);
        assert!(order.accept(&relabel, 25).is_err());
        order.accept(&cue(2, "local_endpoint", 1), 25).unwrap();
        order.accept(&cue(3, "speaker_first_write", 1), 35).unwrap();
        order.accept(&cue(4, "cancelled", 1), 45).unwrap();
        order.accept(&cue(5, "input_admitted", 2), 55).unwrap();
        assert!(order.accept(&cue(6, "speech_retired", 1), 65).is_err());
    }
    #[test]
    fn cue_receipt_at_expiry_or_before_send_is_not_fresh() {
        for received in [10, 100010] {
            assert!(
                CueOrder::default()
                    .accept(&cue(1, "input_admitted", 1), received)
                    .is_err()
            );
        }
    }
    #[test]
    fn retired_audio_can_be_cancelled_without_reviving_playback() {
        let mut order = CueOrder::default();
        for (index, kind) in [
            "input_admitted",
            "local_endpoint",
            "speaker_first_write",
            "speech_retired",
        ]
        .iter()
        .enumerate()
        {
            let c = cue(index as u64 + 1, kind, 1);
            order.accept(&c, c.sent_us + 1).unwrap();
        }
        assert!(order.accept(&cue(5, "speech_retired", 1), 55).is_err());
        assert!(order.accept(&cue(5, "speaker_first_write", 1), 55).is_err());
        order.accept(&cue(5, "cancelled", 1), 55).unwrap();
        assert!(order.accept(&cue(6, "cancelled", 1), 65).is_err());
        order.accept(&cue(6, "input_admitted", 2), 65).unwrap();
        assert!(order.accept(&cue(7, "speaker_first_write", 1), 75).is_err());
    }

    #[test]
    fn fresh_delayed_child_observation_keeps_its_earlier_event_time() {
        let mut order = CueOrder::default();
        order.accept(&cue(1, "input_admitted", 1), 12).unwrap();
        order.accept(&cue(2, "local_endpoint", 1), 22).unwrap();
        let mut observed = cue(3, "speaker_first_write", 1);
        // Child observation and coordinator event times are not a global
        // ordered clock: only the producer's send/sequence order is promised.
        observed.event_us = 19;
        observed.expires_us = 100019;
        order.accept(&observed, 32).unwrap();
        assert_eq!(observed.event().unwrap().at_us, Some(19));
        let mut regressed_send = cue(4, "speech_retired", 1);
        regressed_send.sent_us = 30;
        assert!(order.accept(&regressed_send, 42).is_err());
    }
    #[test]
    fn each_occurrence_is_matched_and_only_whole_completion_is_terminal() {
        let mut order = CueOrder::default();
        for (index, kind) in [
            "input_admitted",
            "local_endpoint",
            "speaker_first_write",
            "speech_retired",
        ]
        .iter()
        .enumerate()
        {
            let c = cue(index as u64 + 1, kind, 1);
            order.accept(&c, c.sent_us + 1).unwrap();
        }
        let mut start = cue(5, "speaker_first_write", 1);
        start.playback_sequence = Some(2);
        order.accept(&start, 52).unwrap();
        // A stale retirement cannot close occurrence two, nor authorize completion.
        assert!(order.accept(&cue(6, "speech_retired", 1), 62).is_err());
        assert!(order.owners[&1].playback.active());
        assert!(order.accept(&cue(6, "turn_completed", 1), 62).is_err());
        let mut retire = cue(6, "speech_retired", 1);
        retire.playback_sequence = Some(2);
        order.accept(&retire, 62).unwrap();
        // A rejected cue invalidates the physical stream; a fresh order tests clean completion.
        let mut clean = CueOrder::default();
        for (index, kind) in [
            "input_admitted",
            "local_endpoint",
            "speaker_first_write",
            "speech_retired",
            "speaker_first_write",
            "speech_retired",
            "turn_completed",
        ]
        .iter()
        .enumerate()
        {
            let mut c = cue(index as u64 + 1, kind, 1);
            if index == 4 || index == 5 {
                c.playback_sequence = Some(2);
            }
            clean.accept(&c, c.sent_us + 1).unwrap();
        }
        assert!(clean.accept(&cue(8, "cancelled", 1), 82).is_err());
        assert!(clean.accept(&cue(8, "speaker_first_write", 1), 82).is_err());
    }
    #[test]
    fn legacy_missing_occurrence_live_cue_is_withheld() {
        let mut order = CueOrder::default();
        order.accept(&cue(1, "input_admitted", 1), 12).unwrap();
        order.accept(&cue(2, "local_endpoint", 1), 22).unwrap();
        let mut old = cue(3, "speaker_first_write", 1);
        old.playback_sequence = None;
        assert!(order.accept(&old, 32).is_err());
    }
}

#[cfg(test)]
mod relay_reader_tests {
    use super::*;

    fn read_all(bytes: Vec<u8>) -> Vec<std::result::Result<SessionLine, String>> {
        relay_reader(std::io::Cursor::new(bytes)).iter().collect()
    }

    #[test]
    fn oversized_non_utf8_and_cut_off_lines_are_protocol_errors() {
        let pong = br#"{"type":"pong","seq":1,"lamp_us":5}"#;
        let mut bytes = pong.to_vec();
        bytes.push(b'\n');
        bytes.extend(vec![b'x'; MAX_LINE_BYTES + 1]);
        bytes.push(b'\n');
        bytes.extend(b"\xff\n");
        bytes.extend(pong);
        bytes.push(b'\n');
        bytes.extend(br#"{"type":"pong","#);
        let items = read_all(bytes);
        assert_eq!(items.len(), 5, "{items:?}");
        assert_eq!(items[0], Ok(SessionLine::Pong { seq: 1, lamp_us: 5 }));
        assert!(
            items[1]
                .as_ref()
                .unwrap_err()
                .contains("exceeds the 65536-byte limit")
        );
        assert!(items[2].as_ref().unwrap_err().contains("not valid UTF-8"));
        assert!(items[3].is_ok());
        assert!(
            items[4]
                .as_ref()
                .unwrap_err()
                .contains("closed inside a line")
        );
    }

    /// Endless lines of `width` bytes, generated without storing them.
    struct Lines {
        width: usize,
        position: usize,
    }
    impl std::io::Read for Lines {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            for byte in buffer.iter_mut() {
                *byte = if self.position % (self.width + 1) == self.width {
                    b'\n'
                } else {
                    b'y'
                };
                self.position += 1;
            }
            Ok(buffer.len())
        }
    }

    /// An endless stream with no newline at all.
    struct NoNewline;
    impl std::io::Read for NoNewline {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            buffer.fill(b'z');
            Ok(buffer.len())
        }
    }

    #[test]
    fn a_never_terminated_line_still_ends_at_the_session_byte_bound() {
        let lines = relay_reader(NoNewline);
        let item = lines
            .recv_timeout(Duration::from_secs(60))
            .expect("the reader kept consuming an unterminated line without limit");
        let error = item.unwrap_err();
        assert!(error.contains("reading stopped"), "{error}");
        assert!(lines.recv_timeout(Duration::from_secs(5)).is_err());
    }

    #[test]
    fn reading_stops_with_an_error_above_the_session_byte_bound() {
        // Rejected oversized lines still count, so an endless stream ends.
        let width = MAX_LINE_BYTES * 4;
        let items: Vec<_> = relay_reader(Lines { width, position: 0 }).iter().collect();
        let last = items.last().unwrap().as_ref().unwrap_err();
        assert!(last.contains("reading stopped"), "{last}");
        // Every complete line before the bound, then the stop.
        assert_eq!(
            items.len() as u64,
            MAX_SESSION_BYTES / (width as u64 + 1) + 1
        );
    }
}

// Test-only clock seam from the independent serve-boundary review
// (coordination/reviews/serve-boundaries-20261012): tests can model a
// scheduler pause between the grant check and its send.
fn grant_clock_us(now: impl FnOnce() -> MonoTime) -> u64 {
    #[cfg(test)]
    if let Some(now) = audit_grant_tests::CLOCK.with(|times| times.borrow_mut().pop_front()) {
        return now;
    }
    now().as_micros()
}

#[cfg(test)]
/// The independent review's production-path reproductions, adapted.
mod audit_grant_tests {
    use super::*;
    use crate::plan::{Expectation, Trigger, TriggerEvent};
    use std::{
        cell::RefCell,
        collections::VecDeque,
        panic::{AssertUnwindSafe, catch_unwind},
    };
    thread_local! {
        pub(super) static CLOCK: RefCell<VecDeque<u64>> = const { RefCell::new(VecDeque::new()) };
    }
    struct NoAudio;
    impl Player for NoAudio {
        fn start(
            &mut self,
            _: &Path,
            _: &SceneTiming,
            _: &Path,
            _: &Path,
        ) -> Result<JoinHandle<Value>> {
            panic!("no audio may start in this audit test")
        }
        fn describe(&self) -> Value {
            json!({"audit": true})
        }
    }

    fn exercise(sent: u64) -> (bool, bool, bool) {
        let root =
            std::env::temp_dir().join(format!("lve-grant-audit-{}-{sent}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("objects")).unwrap();
        let assets = AssetIndex::load(&root, &[], &StimulusCatalog::load().unwrap()).unwrap();
        let config = PhysicalConfig {
            lamp_command: Vec::new(),
            lamp_work_root: root.display().to_string(),
            mode: LampMode::Serve {
                provider_config: "/private/unused.json".into(),
            },
            noise_suppression: "off".into(),
            diagnostics: false,
            room_recorder: None,
            room_independent: false,
            allowance_seconds: 5,
            ring_channel_ceiling: None,
            stimulus_source: StimulusSource::LoudspeakerSynthetic,
            runtime_flags: Default::default(),
        };
        let mut backend = PhysicalBackend::new(config, &assets, Box::new(NoAudio));
        // An already-reaped inert child supplies the Session's OS identity only.
        let mut child = Command::new("/usr/bin/true").spawn().unwrap();
        child.wait().unwrap();
        let (_tx, lines) = mpsc::channel();
        backend.session = Some(Session {
            child,
            next_ping_us: 0,
            stdin: None,
            lines,
            pings: Vec::new(),
            best: Some(ClockMapping::identity(ClockDomain::LampMonotonic)),
            trace: Vec::new(),
            end: None,
            end_synthesized: false,
            ready_seen: true,
            protocol_errors: Vec::new(),
            start: None,
            cues: Vec::new(),
            cue_order: CueOrder::default(),
            cue_failed: false,
            boot_changes: Vec::new(),
            stop_sent: false,
            runtime_ended: false,
            stdout_readiness_us: Vec::new(),
            clock_samples: Vec::new(),
            started_runner_us: None,
            held: std::collections::VecDeque::new(),
            released: std::collections::VecDeque::new(),
        });
        backend.trigger = Some((
            TriggerObservation {
                event: TriggerEvent::ListeningReady,
                turn: None,
                event_at_us: Some(1_000),
                event_domain: ClockDomain::LampMonotonic,
                runner_us: 1_000,
                runner_time_method: "injected same clock".into(),
            },
            0,
        ));
        let (grant, rx) = mpsc::sync_channel(1);
        let handle = thread::spawn(|| Value::Null);
        backend.armed = Some(Armed {
            step: "one".into(),
            prepared: PreparedStart {
                ready: json!({"attempt_id": "original-attempt"}),
                grant,
                handle,
            },
            cancel_file: root.join("cancel"),
            armed_us: 500,
            speech_end_us: 0,
        });
        let step = Step {
            id: "one".into(),
            scene: None,
            trigger: Trigger {
                after: TriggerEvent::ListeningReady,
                delay_ms: 0,
                deadline_ms: 1000,
                require_lamp_speaking: false,
            },
            expect: Expectation::Answer,
            reference: String::new(),
            addressed: Vec::new(),
        };
        // The first sample is valid. The second sample models descheduling
        // after try_send. Native expiry is unchanged at 101,000 microseconds.
        CLOCK.with(|times| *times.borrow_mut() = [100_999, sent].into());
        let outcome = catch_unwind(AssertUnwindSafe(|| backend.grant(&step)));
        let sent_original = matches!(rx.try_recv(), Ok(GrantInput::Line(line))
            if serde_json::from_str::<Value>(&line).unwrap()["expires_monotonic_ns"] == 101_000_000_u64);
        let tracked = backend.armed.is_some() || !backend.playbacks.is_empty();
        let did_not_panic = outcome.is_ok();
        if let Ok(Ok((mut playback, _receipt))) = outcome {
            playback.handle.take().unwrap().join().unwrap();
        }
        // No remote/audio subprocess existed; the inert child is already reaped.
        backend.session = None;
        drop(backend);
        fs::remove_dir_all(root).unwrap();
        (did_not_panic, sent_original, tracked)
    }
    /// R2 (Main review of 3c8e3d3b5), on the production grant: the scenario's
    /// original deadline is 101,000 us, but the recording declares its end at
    /// 140,020 us and the scenario observes 40,000 us after the prompt's
    /// speech (none here), so the latest covered start is 100,020 us.
    fn exercise_room(declared_end_us: u64, observe_us: u64) -> Option<u64> {
        let root = std::env::temp_dir().join(format!(
            "lve-grant-room-{}-{declared_end_us}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("objects")).unwrap();
        let assets = AssetIndex::load(&root, &[], &StimulusCatalog::load().unwrap()).unwrap();
        let config = PhysicalConfig {
            lamp_command: Vec::new(),
            lamp_work_root: root.display().to_string(),
            mode: LampMode::Serve {
                provider_config: "/private/unused.json".into(),
            },
            noise_suppression: "off".into(),
            diagnostics: false,
            room_recorder: None,
            room_independent: false,
            allowance_seconds: 5,
            ring_channel_ceiling: None,
            stimulus_source: StimulusSource::LoudspeakerSynthetic,
            runtime_flags: Default::default(),
        };
        let mut backend = PhysicalBackend::new(config, &assets, Box::new(NoAudio));
        let mut child = Command::new("/usr/bin/true").spawn().unwrap();
        child.wait().unwrap();
        let (_tx, lines) = mpsc::channel();
        backend.session = Some(Session {
            child,
            next_ping_us: 0,
            stdin: None,
            lines,
            pings: Vec::new(),
            best: Some(ClockMapping::identity(ClockDomain::LampMonotonic)),
            trace: Vec::new(),
            end: None,
            end_synthesized: false,
            ready_seen: true,
            protocol_errors: Vec::new(),
            start: None,
            cues: Vec::new(),
            cue_order: CueOrder::default(),
            cue_failed: false,
            boot_changes: Vec::new(),
            stop_sent: false,
            runtime_ended: false,
            stdout_readiness_us: Vec::new(),
            clock_samples: Vec::new(),
            started_runner_us: None,
            held: std::collections::VecDeque::new(),
            released: std::collections::VecDeque::new(),
        });
        backend.room_cover.end_us = Some(declared_end_us);
        backend.room_cover.observe_us = observe_us;
        backend.trigger = Some((
            TriggerObservation {
                event: TriggerEvent::ListeningReady,
                turn: None,
                event_at_us: Some(1_000),
                event_domain: ClockDomain::LampMonotonic,
                runner_us: 1_000,
                runner_time_method: "injected same clock".into(),
            },
            0,
        ));
        let (grant, rx) = mpsc::sync_channel(1);
        let handle = thread::spawn(|| Value::Null);
        backend.armed = Some(Armed {
            step: "one".into(),
            prepared: PreparedStart {
                ready: json!({"attempt_id": "original-attempt"}),
                grant,
                handle,
            },
            cancel_file: root.join("cancel"),
            armed_us: 500,
            speech_end_us: 0,
        });
        let step = Step {
            id: "one".into(),
            scene: None,
            trigger: Trigger {
                after: TriggerEvent::ListeningReady,
                delay_ms: 0,
                deadline_ms: 1000,
                require_lamp_speaking: false,
            },
            expect: Expectation::Answer,
            reference: String::new(),
            addressed: Vec::new(),
        };
        // Granted at 10,000 us: well inside both bounds.
        CLOCK.with(|times| *times.borrow_mut() = [10_000, 10_001].into());
        let outcome = backend.grant(&step);
        let sent = match rx.try_recv() {
            Ok(GrantInput::Line(line)) => {
                serde_json::from_str::<Value>(&line).unwrap()["expires_monotonic_ns"].as_u64()
            }
            _ => None,
        };
        if let Ok((mut playback, _)) = outcome {
            playback.handle.take().unwrap().join().unwrap();
        }
        backend.session = None;
        drop(backend);
        fs::remove_dir_all(root).unwrap();
        sent
    }

    #[test]
    fn r2_a_grant_never_outlives_the_recordings_latest_covered_start() {
        let expires_ns = exercise_room(140_020, 40_000).expect("granted");
        assert!(
            expires_ns <= 100_020_000,
            "the grant lets the first copy start after the recording's latest covered start: expires {expires_ns} ns"
        );
        // A recording that covers past the original deadline leaves it alone.
        assert_eq!(exercise_room(10_000_000, 40_000), Some(101_000_000));
    }

    #[test]
    fn audit_grant_within_deadline_control() {
        let (did_not_panic, original, _) = exercise(100_999);
        assert!(did_not_panic && original);
    }
    #[test]
    fn audit_grant_deschedule_after_send_must_not_panic_or_lose_tracking() {
        let (did_not_panic, original, tracked) = exercise(101_001);
        assert!(original, "the original, unextended grant must be preserved");
        assert!(
            did_not_panic,
            "grant panicked after sending original expiry; player tracked={tracked}"
        );
    }

    fn exit_report(code: i32) -> Finished {
        exit_report_with(code, &["/usr/bin/true"], true, "reaped")
    }

    /// A serve attempt whose runtime ended with `code`, finished through the
    /// production `finish()` while the outer transport (the relay, normally
    /// over SSH) is the real process `transport`, reaped first or not.
    fn exit_report_with(code: i32, transport: &[&str], reaped: bool, label: &str) -> Finished {
        exit_report_prepared(code, transport, reaped, label, |_, _| {})
    }

    /// As `exit_report_with`, after `prepare` adds attempt state (for
    /// example a room recorder and delivered playbacks) to the backend.
    pub(super) fn exit_report_prepared(
        code: i32,
        transport: &[&str],
        reaped: bool,
        label: &str,
        prepare: impl FnOnce(&mut PhysicalBackend<'_>, &Path),
    ) -> Finished {
        let root = std::env::temp_dir().join(format!(
            "lve-exit-audit-{}-{code}-{label}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("objects")).unwrap();
        let assets = AssetIndex::load(&root, &[], &StimulusCatalog::load().unwrap()).unwrap();
        let config = PhysicalConfig {
            lamp_command: Vec::new(),
            lamp_work_root: root.display().to_string(),
            mode: LampMode::Serve {
                provider_config: "/private/unused.json".into(),
            },
            noise_suppression: "off".into(),
            diagnostics: false,
            room_recorder: None,
            room_independent: false,
            allowance_seconds: 5,
            ring_channel_ceiling: None,
            stimulus_source: StimulusSource::LoudspeakerSynthetic,
            runtime_flags: Default::default(),
        };
        let mut backend = PhysicalBackend::new(config, &assets, Box::new(NoAudio));
        // An inert local process stands in for the relay transport; no
        // Lamp, SSH, audio or network is involved.
        let mut child = Command::new(transport[0])
            .args(&transport[1..])
            .spawn()
            .unwrap();
        if reaped {
            child.wait().unwrap();
        }
        let (tx, lines) = mpsc::channel();
        backend.session = Some(Session {
            child,
            next_ping_us: 0,
            stdin: None,
            lines,
            pings: Vec::new(),
            best: Some(ClockMapping::identity(ClockDomain::LampMonotonic)),
            trace: Vec::new(),
            end: None,
            end_synthesized: false,
            ready_seen: true,
            protocol_errors: Vec::new(),
            start: None,
            cues: Vec::new(),
            cue_order: CueOrder::default(),
            cue_failed: false,
            boot_changes: Vec::new(),
            stop_sent: false,
            runtime_ended: false,
            stdout_readiness_us: Vec::new(),
            clock_samples: Vec::new(),
            started_runner_us: None,
            held: std::collections::VecDeque::new(),
            released: std::collections::VecDeque::new(),
        });

        drop(tx);
        let session = backend.session.as_mut().unwrap();
        session.start = Some(json!({"schema": 1, "mode": "serve", "cue_socket": true}));
        session.end = Some(SessionLine::SessionEnd {
            lamp_us: 10,
            exit_code: Some(code),
            killed: false,
            events_sha256: None,
            events_lines: 0,
            error: None,
            stray_processes_stopped: false,
            serve: Some(Box::new(crate::physical::protocol::ServeEnd {
                stop_cause: "runtime_exited".into(),
                stop_reason: None,
                stop_requested_us: None,
                signal_sent_us: None,
                signal_error: None,
                exited_us: Some(10),
                forced_kill: false,
                group_reaped: true,
                metadata: crate::physical::protocol::ServeMetadata::default(),
            })),
        });
        prepare(&mut backend, &root);
        let catalog = StimulusCatalog::load().unwrap();
        let plan = crate::plan::LoadedPlan::parse(crate::plan::SERVE_PLAN, &catalog).unwrap();
        let context = AttemptContext {
            attempt_id: "exit-audit".into(),
            run_id: "audit".into(),
            scenario: &plan.plan.scenarios[0],
            repetition: 0,
            order: 0,
            seed: 1,
            run_directory: root.clone(),
        };
        let finished = backend.finish(&context).unwrap();
        drop(backend);
        fs::remove_dir_all(root).unwrap();
        finished
    }
    /// R2 at finish, through production `finish()`: Main's witness. A prompt
    /// requested at 10.000 s with 2 s of speech and an 8 s window was covered
    /// by a recording ending at 20.020 s (latest start 10.020 s); its first
    /// copy at 10.050 s, inside the scenario's 10.100 s expiry, overruns it.
    fn finish_with_first_copy(label: &str, first_copy_ns: u64) -> Finished {
        exit_report_prepared(0, &["/usr/bin/true"], true, label, |backend, root| {
            let mut recorder = Command::new("/usr/bin/true").spawn().unwrap();
            recorder.wait().unwrap();
            backend.room = Some((recorder, root.join("room"), 0));
            backend.room_cover.end_us = Some(20_020_000);
            backend.playbacks.push(Playback {
                step: "sky".into(),
                requested_us: 10_000_000,
                cancel_file: root.join("cancel-sky"),
                handle: None,
                report: Some(json!({"valid": true, "status": "delivered",
                    "counters": {"source_started_host_ns": first_copy_ns}})),
                prepared: true,
                latest_start_us: latest_covered_start(Some(20_020_000), 2_000_000, 8_000_000),
            });
        })
    }

    #[test]
    fn r2_an_actual_first_copy_after_the_covered_start_is_retained_as_a_violation() {
        let late = finish_with_first_copy("late-copy", 10_050_000_000);
        let room = &late.evidence["room_audio"];
        let violations = room["coverage_violations"].as_array().unwrap();
        assert_eq!(violations.len(), 1, "{room}");
        assert_eq!(violations[0]["step"], "sky");
        assert_eq!(violations[0]["latest_start_runner_us"], 10_020_000);
        assert_eq!(violations[0]["overrun_us"], 30_000);
        assert_eq!(coverage_refusal(room), Some("prompt_outside_recording"));
        let inside = finish_with_first_copy("inside-copy", 10_010_000_000);
        let room = &inside.evidence["room_audio"];
        assert_eq!(room["coverage_violations"], json!([]), "{room}");
        assert_eq!(coverage_refusal(room), None);
    }

    #[test]
    fn audit_failed_outer_transport_cannot_certify_nominal_runtime_end() {
        // Nominal runtime end (exit 0, complete session_end), but the outer
        // relay/SSH transport process itself exits nonzero.
        let result = exit_report_with(0, &["/usr/bin/false"], false, "false");
        assert_eq!(result.evidence["session_end"]["exit_code"], 0);
        assert_eq!(result.evidence["transport_exit"], 1);
        assert!(
            result
                .aborted
                .as_deref()
                .is_some_and(|r| r.contains("transport")),
            "{:?}",
            result.aborted
        );
    }

    #[test]
    fn audit_reaped_successful_transport_certifies_nominal_runtime_end() {
        let result = exit_report_with(0, &["/usr/bin/true"], false, "true");
        assert_eq!(result.evidence["transport_exit"], 0);
        assert_eq!(result.evidence["transport_forced_kill"], false);
        assert!(result.aborted.is_none(), "{:?}", result.aborted);
    }

    #[test]
    fn audit_transport_that_outlives_its_bound_is_killed_and_never_certified() {
        let result = super::transport_exit_clock_tests::expired_transport_report();
        assert_eq!(result.evidence["session_end"]["exit_code"], 0);
        assert_eq!(result.evidence["transport_forced_kill"], true);
        assert!(result.evidence["transport_exit"].is_null());
        assert!(
            result
                .aborted
                .as_deref()
                .is_some_and(|r| r.contains("transport")),
            "{:?}",
            result.aborted
        );
    }

    #[test]
    fn audit_successful_runtime_exit_control() {
        assert!(exit_report(0).aborted.is_none());
    }
    #[test]
    fn audit_nonzero_runtime_exit_must_invalidate_attempt() {
        let result = exit_report(1);
        assert_eq!(result.evidence["session_end"]["exit_code"], 1);
        assert!(
            result.aborted.is_some(),
            "runtime exit 1 was retained but did not invalidate attempt"
        );
    }

    #[test]
    fn audit_serve_stdout_readiness_cannot_mint_an_original_cue() {
        let root = std::env::temp_dir().join(format!("lve-stdout-audit-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("objects")).unwrap();
        let assets = AssetIndex::load(&root, &[], &StimulusCatalog::load().unwrap()).unwrap();
        let config = PhysicalConfig {
            lamp_command: Vec::new(),
            lamp_work_root: root.display().to_string(),
            mode: LampMode::Serve {
                provider_config: "/private/unused.json".into(),
            },
            noise_suppression: "off".into(),
            diagnostics: false,
            room_recorder: None,
            room_independent: false,
            allowance_seconds: 5,
            ring_channel_ceiling: None,
            stimulus_source: StimulusSource::LoudspeakerSynthetic,
            runtime_flags: Default::default(),
        };
        let mut backend = PhysicalBackend::new(config, &assets, Box::new(NoAudio));
        // An already-reaped inert child supplies the Session's OS identity only.
        let mut child = Command::new("/usr/bin/true").spawn().unwrap();
        child.wait().unwrap();
        let (tx, lines) = mpsc::channel();
        backend.session = Some(Session {
            child,
            next_ping_us: 0,
            stdin: None,
            lines,
            pings: Vec::new(),
            best: Some(ClockMapping::identity(ClockDomain::LampMonotonic)),
            trace: Vec::new(),
            end: None,
            end_synthesized: false,
            ready_seen: true,
            protocol_errors: Vec::new(),
            start: None,
            cues: Vec::new(),
            cue_order: CueOrder::default(),
            cue_failed: false,
            boot_changes: Vec::new(),
            stop_sent: false,
            runtime_ended: false,
            stdout_readiness_us: Vec::new(),
            clock_samples: Vec::new(),
            started_runner_us: None,
            held: std::collections::VecDeque::new(),
            released: std::collections::VecDeque::new(),
        });

        drop(tx);
        let session = backend.session.as_mut().unwrap();
        session.start = Some(json!({"schema":1,"mode":"serve","cue_socket":true}));
        session.ready_seen = false;
        let received = monotonic_us();
        let observed = backend.handle(SessionLine::Stdout {
            lamp_received_us: received,
            // The line has no source observation, owner, boot or expiry.
            line: r#"{"status":"listening_ready","seconds":60}"#.into(),
        });
        backend.session = None;
        drop(backend);
        fs::remove_dir_all(root).unwrap();
        assert!(
            observed.is_none(),
            "stdout receipt became scheduling authority with a newly minted time: {observed:?}"
        );
    }

    #[test]
    fn audit_startup_unmapped_expired_readiness_cannot_be_accepted() {
        let root =
            std::env::temp_dir().join(format!("lve-unmapped-startup-audit-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("objects")).unwrap();
        let assets = AssetIndex::load(&root, &[], &StimulusCatalog::load().unwrap()).unwrap();
        let config = PhysicalConfig {
            lamp_command: Vec::new(),
            lamp_work_root: root.display().to_string(),
            mode: LampMode::Serve {
                provider_config: "/private/unused.json".into(),
            },
            noise_suppression: "off".into(),
            diagnostics: false,
            room_recorder: None,
            room_independent: false,
            allowance_seconds: 5,
            ring_channel_ceiling: None,
            stimulus_source: StimulusSource::LoudspeakerSynthetic,
            runtime_flags: Default::default(),
        };
        let mut backend = PhysicalBackend::new(config, &assets, Box::new(NoAudio));
        // An already-reaped inert child supplies the Session's OS identity only.
        let mut child = Command::new("/usr/bin/true").spawn().unwrap();
        child.wait().unwrap();
        let (tx, lines) = mpsc::channel();
        backend.session = Some(Session {
            child,
            next_ping_us: 0,
            stdin: None,
            lines,
            pings: Vec::new(),
            best: None,
            trace: Vec::new(),
            end: None,
            end_synthesized: false,
            ready_seen: true,
            protocol_errors: Vec::new(),
            start: None,
            cues: Vec::new(),
            cue_order: CueOrder::default(),
            cue_failed: false,
            boot_changes: Vec::new(),
            stop_sent: false,
            runtime_ended: false,
            stdout_readiness_us: Vec::new(),
            clock_samples: Vec::new(),
            started_runner_us: None,
            held: std::collections::VecDeque::new(),
            released: std::collections::VecDeque::new(),
        });

        drop(tx);
        let session = backend.session.as_mut().unwrap();
        session.start = Some(json!({"schema":1,"mode":"serve","cue_socket":true}));
        session.ready_seen = false;
        // Original schema-1 time/expiry remain intact. The relay took the cue
        // while fresh; a host-side stall delivered it after its 100 ms lease.
        // This is the post-session-start, pre-first-usable-pong state newly
        // made normal by discarding the pre-session ping.
        let event_us = 1;
        let expires_us = event_us + crate::events::CUE_FRESH_US;
        assert!(monotonic_us() > expires_us);
        let observed = backend.handle(SessionLine::Cue {
            lamp_received_us: event_us + 2,
            cue: json!({"schema":1,"sequence":1,"kind":"listening_ready",
                "boot":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],
                "turn":null,"generation":null,"capture_epoch":1,
                "reference_epoch_context":1,"event_us":event_us,
                "sent_us":event_us + 1,"expires_us":expires_us}),
        });
        // Held, unpublished, until no live map has appeared for its whole
        // lease (A2 adaptation: the decision is now at the hold's end).
        assert!(observed.is_none());
        let expired = backend.expire_held(monotonic_us() + crate::events::CUE_FRESH_US);
        assert!(matches!(
            expired.as_ref().map(|e| &e.kind),
            Some(EventKind::RunEnd { status: Some(s), .. }) if s == CUE_PROTOCOL_INVALID
        ));
        let receipt = backend
            .session
            .as_ref()
            .unwrap()
            .cues
            .last()
            .cloned()
            .unwrap();
        backend.session = None;
        drop(backend);
        fs::remove_dir_all(root).unwrap();
        assert!(
            !observed.is_some_and(|event| matches!(event.kind, EventKind::ListeningReady))
                && receipt["accepted"] != true,
            "expired readiness accepted without any clock proof: {receipt}"
        );
    }

    /// A serve backend whose session took `session_start` at `started_us`
    /// and wrote ping 1 at `sent_us`; no clock map yet.
    fn unmapped_session(root: &Path, started_us: u64, sent_us: u64) -> PhysicalBackend<'static> {
        fs::create_dir_all(root.join("objects")).unwrap();
        let assets: &'static AssetIndex = Box::leak(Box::new(
            AssetIndex::load(root, &[], &StimulusCatalog::load().unwrap()).unwrap(),
        ));
        let config = PhysicalConfig {
            lamp_command: Vec::new(),
            lamp_work_root: root.display().to_string(),
            mode: LampMode::Serve {
                provider_config: "/private/unused.json".into(),
            },
            noise_suppression: "off".into(),
            diagnostics: false,
            room_recorder: None,
            room_independent: false,
            allowance_seconds: 5,
            ring_channel_ceiling: None,
            stimulus_source: StimulusSource::LoudspeakerSynthetic,
            runtime_flags: Default::default(),
        };
        let mut backend = PhysicalBackend::new(config, assets, Box::new(NoAudio));
        let mut child = Command::new("/usr/bin/true").spawn().unwrap();
        child.wait().unwrap();
        let (_, lines) = mpsc::channel();
        backend.session = Some(Session {
            child,
            next_ping_us: 0,
            stdin: None,
            lines,
            pings: vec![(1, sent_us)],
            best: None,
            trace: Vec::new(),
            end: None,
            end_synthesized: false,
            ready_seen: false,
            protocol_errors: Vec::new(),
            start: Some(json!({"schema":1,"mode":"serve","cue_socket":true})),
            cues: Vec::new(),
            cue_order: CueOrder::default(),
            cue_failed: false,
            boot_changes: Vec::new(),
            stop_sent: false,
            runtime_ended: false,
            stdout_readiness_us: Vec::new(),
            clock_samples: Vec::new(),
            started_runner_us: Some(started_us),
            held: std::collections::VecDeque::new(),
            released: std::collections::VecDeque::new(),
        });
        backend
    }

    /// Lamp time is the runner's plus this offset in these controls.
    const LAMP_AHEAD_US: u64 = 1_000_000_000;

    fn startup_cue(sequence: u64, kind: &str, age_us: u64, extra: Value) -> SessionLine {
        let event_us = monotonic_us() + LAMP_AHEAD_US - age_us;
        let mut cue = json!({"schema":1,"sequence":sequence,"kind":kind,
            "boot":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],
            "turn":null,"generation":null,"capture_epoch":1,"reference_epoch_context":1,
            "event_us":event_us,"sent_us":event_us + 1,
            "expires_us":event_us + crate::events::CUE_FRESH_US});
        for (key, value) in extra.as_object().unwrap() {
            cue[key] = value.clone();
        }
        SessionLine::Cue {
            lamp_received_us: event_us + 2,
            cue,
        }
    }

    fn live_pong() -> SessionLine {
        // Ping 1 was written 1 ms ago, after session_start; the relay's
        // clock reads LAMP_AHEAD_US ahead at the round trip's midpoint.
        SessionLine::Pong {
            seq: 1,
            lamp_us: monotonic_us() + LAMP_AHEAD_US - 500,
        }
    }

    #[test]
    fn fast_startup_cues_before_the_first_live_pong_are_published_in_order_with_original_times() {
        let root = std::env::temp_dir().join(format!("lve-held-fast-{}", std::process::id()));
        let now = monotonic_us();
        let mut backend = unmapped_session(&root, now - 5_000, now - 1_000);
        let availability = backend.take(Ok(startup_cue(
            1,
            "input_availability",
            10_000,
            json!({"available": true}),
        )));
        let ready = backend.take(Ok(startup_cue(2, "listening_ready", 9_000, json!({}))));
        // Nothing is published before a live map exists.
        assert!(availability.is_none() && ready.is_none());
        assert!(backend.pending.is_empty());
        let received: Vec<u64> = backend
            .session
            .as_ref()
            .unwrap()
            .held
            .iter()
            .map(|h| h.0)
            .collect();
        assert!(backend.take(Ok(live_pong())).is_none());
        let published: Vec<RuntimeEvent> = backend.pending.drain(..).collect();
        assert!(matches!(
            published[0].kind,
            EventKind::InputAvailability { available: true }
        ));
        assert!(matches!(published[1].kind, EventKind::ListeningReady));
        // Original runner receipt times, never the decision time.
        assert_eq!(
            published
                .iter()
                .map(|e| e.received_us.unwrap())
                .collect::<Vec<_>>(),
            received
        );
        let cues = backend.session.as_ref().unwrap().cues.clone();
        backend.session = None;
        drop(backend);
        let _ = fs::remove_dir_all(&root);
        for receipt in &cues {
            assert_eq!(receipt["accepted"], true, "{receipt}");
            assert!(
                receipt["freshness"]["margin_us"].as_i64().unwrap() > 0,
                "{receipt}"
            );
            assert!(
                receipt["freshness"]["held_us"].as_u64().is_some(),
                "{receipt}"
            );
        }
    }

    #[test]
    fn a_held_startup_cue_that_expired_before_the_live_map_is_refused() {
        let root = std::env::temp_dir().join(format!("lve-held-late-{}", std::process::id()));
        let now = monotonic_us();
        let mut backend = unmapped_session(&root, now - 5_000, now - 1_000);
        // Fresh at the relay, but 150 ms old when the live map decides it.
        assert!(
            backend
                .take(Ok(startup_cue(1, "listening_ready", 150_000, json!({}))))
                .is_none()
        );
        assert!(backend.take(Ok(live_pong())).is_none());
        let published: Vec<RuntimeEvent> = backend.pending.drain(..).collect();
        let cues = backend.session.as_ref().unwrap().cues.clone();
        backend.session = None;
        drop(backend);
        let _ = fs::remove_dir_all(&root);
        assert!(
            !published
                .iter()
                .any(|e| matches!(e.kind, EventKind::ListeningReady))
        );
        assert!(matches!(
            &published[0].kind,
            EventKind::RunEnd { status: Some(s), .. } if s == CUE_PROTOCOL_INVALID
        ));
        assert_eq!(cues[0]["accepted"], false);
        assert_eq!(
            cues[0]["error"],
            "stale cue at runner (including clock uncertainty)"
        );
    }

    #[test]
    fn held_batch_resamples_after_parse_and_refuses_a_later_expired_cue() {
        let root = std::env::temp_dir().join(format!("lve-held-clock-{}", std::process::id()));
        let mut backend = unmapped_session(&root, 0, 0);
        let session = backend.session.as_mut().unwrap();
        session.best = Some(ClockMapping {
            from: ClockDomain::RunnerMonotonic,
            to: ClockDomain::LampMonotonic,
            offset_us: 0,
            uncertainty_us: 2_000,
            path_allowance_us: 0,
            method: "deterministic held-batch clock".into(),
        });
        for (sequence, kind) in [(1, "input_availability"), (2, "listening_ready")] {
            let event_us = 1_000_000 + (sequence - 1) * 1_000;
            let mut cue = json!({"schema":1,"sequence":sequence,"kind":kind,
                "boot":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],
                "turn":null,"generation":null,"capture_epoch":1,"reference_epoch_context":1,
                "event_us":event_us,"sent_us":event_us + 1,
                "expires_us":event_us + crate::events::CUE_FRESH_US});
            if sequence == 1 {
                cue["available"] = json!(true);
            }
            session
                .held
                .push_back((event_us + 10_000, event_us + 2, cue));
        }
        let original = session.held.clone();
        // Model a 20 ms pause between cue decisions, without sleeping. The
        // first upper bound is 1,092,000; the second must use 1,112,000.
        let mut decisions = [1_090_000, 1_110_000].into_iter();
        release_held(session, || {
            decisions.next().expect("one clock sample per cue")
        });
        assert!(decisions.next().is_none());
        assert_eq!(session.released.len(), 2);
        assert!(matches!(
            session.released[0].kind,
            EventKind::InputAvailability { available: true }
        ));
        assert_eq!(session.released[0].at_us, Some(1_000_000));
        assert_eq!(session.released[0].received_us, Some(1_010_000));
        assert!(matches!(
            &session.released[1].kind,
            EventKind::RunEnd { status: Some(s), .. } if s == CUE_PROTOCOL_INVALID
        ));
        assert_eq!(session.released[1].at_us, Some(1_110_000));
        assert!(!session.ready_seen);
        assert!(session.cue_failed);
        assert_eq!(session.cues.len(), 2);
        assert_eq!(session.cues[0]["accepted"], true);
        assert_eq!(session.cues[1]["accepted"], false);
        assert_eq!(session.cues[0]["freshness"]["decided_runner_us"], 1_090_000);
        assert_eq!(session.cues[1]["freshness"]["decided_runner_us"], 1_110_000);
        assert_eq!(session.cues[0]["freshness"]["margin_us"], 8_000);
        assert_eq!(session.cues[1]["freshness"]["margin_us"], -11_000);
        for (receipt, (received_us, lamp_received_us, cue)) in session.cues.iter().zip(original) {
            assert_eq!(receipt["cue"], cue);
            assert_eq!(receipt["runner_received_us"], received_us);
            assert_eq!(receipt["lamp_received_us"], lamp_received_us);
        }
        backend.session = None;
        drop(backend);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_original_expiry_refuses_at_equality() {
        let map = ClockMapping {
            from: ClockDomain::RunnerMonotonic,
            to: ClockDomain::LampMonotonic,
            offset_us: 1_000,
            uncertainty_us: 2_000,
            path_allowance_us: 0,
            method: "test".into(),
        };
        // Upper bound 10_000 + 1_000 + 2_000 = 13_000.
        assert!(!proves_unexpired(&map, 10_000, 13_000).1);
        assert!(proves_unexpired(&map, 10_000, 13_001).1);
        assert!(!proves_unexpired(&map, 10_001, 13_000).1);
    }
}

#[cfg(test)]
#[path = "backend_clock_tests.rs"]
mod backend_clock_tests;

#[cfg(test)]
#[path = "backend_progress_tests.rs"]
mod backend_progress_tests;

#[cfg(test)]
#[path = "prepared_player_clock_tests.rs"]
mod prepared_player_clock_tests;

#[cfg(test)]
#[path = "prepared_player_progress_tests.rs"]
mod prepared_player_progress_tests;

#[cfg(test)]
#[path = "recorder_backend_tests.rs"]
mod recorder_backend_tests;

#[cfg(test)]
#[path = "transport_exit_clock_tests.rs"]
mod transport_exit_clock_tests;
