//! Lamp-side relay for one lamp-voice session: a finite `directed` or
//! `directed-fixture` run, or one long-lived `serve` runtime.
//!
//! It creates a fresh private work directory, binds the cue socket that
//! lamp-voice's `--cue-socket` connects to, starts the runtime, and relays
//! cues, stdout lines and ping replies as JSON lines. After a finite runtime
//! exits (or is killed at the hard deadline) it streams the final
//! `events.jsonl`. A serve runtime runs until the runner's `stop` line, the
//! runner transport closing or the hard deadline; each takes the same path
//! (SIGTERM, a bounded grace, then a forced group kill), and the relay then
//! streams the rolling `metadata/events-*.jsonl` in generation order. It
//! never stops or starts services and never touches mixers or motors;
//! lamp-voice itself refuses to run while legacy owners are active.
//!
//! Every relayed stream is framed with a fixed per-line bound; oversized,
//! non-UTF-8 or cut-off lines are explicit failures in `session_end`, never
//! truncated or lossily repaired. The runtime runs in its own process group so
//! that stopping it also stops its workers. Closing the runner transport
//! before the runtime ends stops the runtime: an unattended session must not
//! keep the microphone and speaker.
use crate::{
    Result,
    events::parse_cue,
    invalid,
    physical::{
        framing::{Frame, HashingReader, LineFramer, STREAM_LIMIT},
        protocol::{
            MAX_LINE_BYTES, MAX_RUNNER_LINE_BYTES, MAX_RUNNER_TOTAL_BYTES, MAX_STDOUT_LINE_BYTES,
            MAX_STDOUT_TOTAL_BYTES, MAX_TRACE_BYTES, MAX_TRACE_LINES, MAX_TRACE_RECORD_BYTES,
            RunnerLine, SCHEMA, ServeEnd, ServeMetadata, ServeSegment, SessionLine,
        },
    },
};
use lamp_ipc::monotonic_us;
use lamp_turn_owner::MonoTime;
use lamp_voice::{
    options::{
        DirectedOptions, Endpoint, Microphone, PlayoutStart, ReferenceClock, ReferenceRecovery,
        TurnTaking,
    },
    provider_worker::Continuation,
};
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    os::unix::{fs::DirBuilderExt, net::UnixDatagram, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub enum SessionMode {
    Fixture {
        reply: PathBuf,
        sha256: String,
    },
    Directed {
        provider_config: PathBuf,
    },
    /// One long-lived `lamp-voice serve` runtime; `seconds` bounds it.
    Serve {
        provider_config: PathBuf,
    },
}

/// Which voice route a `directed` or `serve` runtime starts on, as
/// `LAMP_VOICE_ROUTE` in its environment. The second route (Deepgram,
/// DeepSeek, ElevenLabs) unless a run names Gemini Live: the owner, 19 Oct
/// 19:3x, every voice test runs on the second route, and Live only for a
/// comparison the first builder names on #1712.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VoiceRoute {
    #[default]
    Second,
    Live,
}

impl VoiceRoute {
    pub const FLAG: &str = "--voice-route";

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Second => "second",
            Self::Live => "live",
        }
    }

    /// The value lamp-voice reads (`coordinator/second.rs`, `ROUTE_ENV`).
    pub fn environment_value(self) -> &'static str {
        match self {
            Self::Second => "second",
            Self::Live => "gemini",
        }
    }
}

impl std::str::FromStr for VoiceRoute {
    type Err = ();
    fn from_str(value: &str) -> std::result::Result<Self, ()> {
        match value {
            "second" => Ok(Self::Second),
            "live" => Ok(Self::Live),
            _ => Err(()),
        }
    }
}

/// The environment variable lamp-voice reads its route from.
pub const ROUTE_ENVIRONMENT: &str = "LAMP_VOICE_ROUTE";
/// The second route's configuration, which lamp-voice needs with
/// `LAMP_VOICE_ROUTE=second`.
pub const SECOND_CONFIG_ENVIRONMENT: &str = "LAMP_VOICE_SECOND_CONFIG";

/// lamp-voice runtime policy the relay forwards to `directed` and `serve`,
/// typed with lamp-voice's own option types. An omitted flag is not passed,
/// so the runtime keeps its own default and the child argv is unchanged.
/// The route and the `LAMP_*` settings go in the runtime's environment, not
/// its argv.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeFlags {
    pub microphone: Option<Microphone>,
    pub turn_taking: Option<TurnTaking>,
    pub reference_clock: Option<ReferenceClock>,
    pub reference_recovery: Option<ReferenceRecovery>,
    pub playout_start_ms: Option<u16>,
    pub provider_continuation: Option<Continuation>,
    /// Opt-in end-of-turn experiment; absent keeps lamp-voice's default.
    pub endpoint: Option<Endpoint>,
    /// The voice route; absent is the second route.
    pub voice_route: Option<VoiceRoute>,
    /// `LAMP_*` settings for the runtime's environment, as the lamp's voice
    /// unit sets them (the second route's configuration and its switches),
    /// in the order given, each name once.
    pub environment: Vec<(String, String)>,
}

impl RuntimeFlags {
    /// Take one runtime flag and its value; false when `flag` is not one.
    /// Values use lamp-voice's own parsers; a repeated flag is refused.
    pub fn take(&mut self, flag: &str, value: &str) -> Result<bool> {
        fn set<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<()> {
            if slot.replace(value).is_some() {
                return Err(invalid(&format!("{flag} given more than once")));
            }
            Ok(())
        }
        let bad = || invalid(&format!("invalid {flag} value {value:?}"));
        match flag {
            "--microphone" => set(
                &mut self.microphone,
                value.parse().map_err(|_| bad())?,
                flag,
            )?,
            "--turn-taking" => set(
                &mut self.turn_taking,
                value.parse().map_err(|_| bad())?,
                flag,
            )?,
            "--reference-clock" => set(
                &mut self.reference_clock,
                value.parse().map_err(|_| bad())?,
                flag,
            )?,
            "--reference-recovery" => set(
                &mut self.reference_recovery,
                value.parse().map_err(|_| bad())?,
                flag,
            )?,
            "--playout-start-ms" => {
                value.parse::<PlayoutStart>().map_err(|_| bad())?;
                set(
                    &mut self.playout_start_ms,
                    value.parse().map_err(|_| bad())?,
                    flag,
                )?
            }
            Continuation::FLAG => set(
                &mut self.provider_continuation,
                value.parse().map_err(|_| bad())?,
                flag,
            )?,
            "--endpoint" => set(&mut self.endpoint, value.parse().map_err(|_| bad())?, flag)?,
            VoiceRoute::FLAG => set(
                &mut self.voice_route,
                value
                    .parse()
                    .map_err(|_| invalid(&format!("{flag} is second or live, not {value:?}")))?,
                flag,
            )?,
            "--runtime-env" => {
                let (name, setting) = value
                    .split_once('=')
                    .ok_or_else(|| invalid(&format!("{flag} is NAME=VALUE, not {value:?}")))?;
                let named = name.len() > "LAMP_".len()
                    && name.starts_with("LAMP_")
                    && name.bytes().all(|byte| {
                        byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'
                    });
                if !named {
                    return Err(invalid(&format!(
                        "{flag} sets only LAMP_ settings in capitals, not {name:?}"
                    )));
                }
                // The values travel on the relay's command line, which a
                // process list shows: nothing secret goes there. Keys come
                // by `LoadCredential` (the first builder on #1916).
                if ["_KEY", "_TOKEN", "_SECRET", "_PASSWORD"]
                    .iter()
                    .any(|secret| name.ends_with(secret))
                {
                    return Err(invalid(&format!(
                        "{flag} {name} names a secret: keys come by LoadCredential, never on a command line"
                    )));
                }
                if name == ROUTE_ENVIRONMENT {
                    return Err(invalid(&format!(
                        "{ROUTE_ENVIRONMENT} is set by {}, not {flag}",
                        VoiceRoute::FLAG
                    )));
                }
                if setting.contains(['\0', '\n']) {
                    return Err(invalid(&format!(
                        "{flag} {name}'s value has a NUL or newline"
                    )));
                }
                if self.environment.iter().any(|(given, _)| given == name) {
                    return Err(invalid(&format!("{flag} {name} given more than once")));
                }
                self.environment.push((name.to_owned(), setting.to_owned()));
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The route the runtime starts on: the second unless Live is named.
    pub fn route(&self) -> VoiceRoute {
        self.voice_route.unwrap_or_default()
    }

    /// The runtime's environment from these: its route, then the settings.
    pub fn runtime_environment(&self) -> Vec<(String, String)> {
        let mut environment = vec![(
            ROUTE_ENVIRONMENT.to_owned(),
            self.route().environment_value().to_owned(),
        )];
        environment.extend(self.environment.iter().cloned());
        environment
    }

    /// These as `lamp-session`'s own options, for the runner to pass to the
    /// relay: lamp-voice's flags, then the route and the settings.
    pub fn relay_argv(&self) -> Vec<String> {
        let mut argv = self.argv();
        if let Some(route) = self.voice_route {
            argv.extend([VoiceRoute::FLAG.to_owned(), route.as_str().to_owned()]);
        }
        for (name, value) in &self.environment {
            argv.extend(["--runtime-env".to_owned(), format!("{name}={value}")]);
        }
        argv
    }

    /// The second route without its configuration: lamp-voice would refuse
    /// to start (`LAMP_VOICE_ROUTE=second needs LAMP_VOICE_SECOND_CONFIG`),
    /// so a run's config says so before anything starts on the lamp.
    pub fn validate_route(&self) -> Result<()> {
        let configured = self
            .environment
            .iter()
            .any(|(name, value)| name == SECOND_CONFIG_ENVIRONMENT && !value.is_empty());
        if self.route() == VoiceRoute::Second && !configured {
            return Err(invalid(&format!(
                "the voice runs on the second route unless a run names live, and the second \
                 route needs its configuration: give runtime_env {SECOND_CONFIG_ENVIRONMENT} \
                 (and the lamp voice unit's LAMP_SECOND_ settings), or voice_route \"live\" \
                 for a comparison named on #1712"
            )));
        }
        Ok(())
    }

    /// The set flags, in the installed pilot template's order.
    pub fn argv(&self) -> Vec<String> {
        let mut argv = Vec::new();
        let mut push = |flag: &str, value: &str| argv.extend([flag.to_owned(), value.to_owned()]);
        if let Some(microphone) = self.microphone {
            push("--microphone", microphone.as_str());
        }
        if let Some(policy) = self.turn_taking {
            push(
                "--turn-taking",
                match policy {
                    TurnTaking::VadOnly => "vad-only",
                    TurnTaking::V1Baseline => "v1-baseline",
                    TurnTaking::Duplex => "duplex",
                },
            );
        }
        if let Some(clock) = self.reference_clock {
            push("--reference-clock", clock.as_str());
        }
        if let Some(recovery) = self.reference_recovery {
            push("--reference-recovery", recovery.as_str());
        }
        if let Some(milliseconds) = self.playout_start_ms {
            push("--playout-start-ms", &milliseconds.to_string());
        }
        if let Some(continuation) = self.provider_continuation {
            push(Continuation::FLAG, continuation.as_str());
        }
        if let Some(endpoint) = self.endpoint {
            push(
                "--endpoint",
                match endpoint {
                    Endpoint::Silence => "silence",
                    Endpoint::SmartTurn => "smart-turn",
                    Endpoint::SmartTurnFast => "smart-turn-fast",
                },
            );
        }
        argv
    }

    /// Refuse what lamp-voice would refuse, with its own parser and rules
    /// (for example bounded reference recovery without a masking policy).
    pub fn validate(
        &self,
        noise_suppression: &str,
        ring_channel_ceiling: Option<u16>,
        diagnostics: bool,
    ) -> Result<()> {
        let mut flags = vec![
            "--noise-suppression".to_owned(),
            noise_suppression.to_owned(),
        ];
        flags.extend(self.argv());
        if let Some(ceiling) = ring_channel_ceiling {
            flags.extend(["--ring-channel-ceiling".to_owned(), ceiling.to_string()]);
        }
        if diagnostics {
            flags.push("--diagnostics".into());
        }
        DirectedOptions::parse(&flags).map_err(|error| {
            invalid(&format!(
                "lamp-voice would refuse runtime options {flags:?}: {error}"
            ))
        })?;
        Ok(())
    }
}

/// Grace between a serve runtime's SIGTERM and the forced kill of its group.
/// lamp-voice records its end and finishes its bounded metadata wait well
/// inside it; a runtime that outlives it is a failure, never a retry.
pub const SERVE_STOP_GRACE_US: u64 = 10_000_000;
/// lamp-voice serve's rolling metadata files and its health record bound.
const SERVE_SEGMENTS: usize = 4;
const MAX_HEALTH_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug)]
pub struct SessionOptions {
    pub runtime: PathBuf,
    /// Extra leading arguments (used to substitute the fake runtime in tests).
    pub runtime_prefix: Vec<String>,
    pub work: PathBuf,
    pub seconds: u16,
    pub mode: SessionMode,
    pub noise_suppression: String,
    pub diagnostics: bool,
    /// Startup before readiness plus shutdown; added to `seconds` for the
    /// hard kill deadline. lamp-voice's own startup bound is 25 s.
    pub allowance_seconds: u16,
    /// Passed as `--ring-channel-ceiling`; without it no ring device opens.
    pub ring_channel_ceiling: Option<u16>,
    /// Runtime policy for `directed` and `serve`; empty passes nothing.
    pub runtime_flags: RuntimeFlags,
}

impl SessionOptions {
    pub fn parse(arguments: &[String]) -> Result<Self> {
        let mut runtime = None;
        let mut runtime_prefix = Vec::new();
        let mut work = None;
        let mut seconds = None;
        let mut reply = None;
        let mut sha256 = None;
        let mut provider_config = None;
        let mut serve_config = None;
        let mut noise_suppression = "on".to_owned();
        let mut diagnostics = false;
        let mut allowance_seconds = 40;
        let mut ring_channel_ceiling = None;
        let mut runtime_flags = RuntimeFlags::default();
        let mut iter = arguments.iter();
        while let Some(flag) = iter.next() {
            let mut value = || {
                iter.next()
                    .cloned()
                    .ok_or_else(|| invalid(&format!("{flag} needs a value")))
            };
            match flag.as_str() {
                "--runtime" => runtime = Some(PathBuf::from(value()?)),
                "--runtime-prefix-arg" => runtime_prefix.push(value()?),
                "--work" => work = Some(PathBuf::from(value()?)),
                "--seconds" => seconds = Some(value()?.parse()?),
                "--fixture-reply" => reply = Some(PathBuf::from(value()?)),
                "--fixture-sha256" => sha256 = Some(value()?),
                "--provider-config" => provider_config = Some(PathBuf::from(value()?)),
                "--serve-config" => serve_config = Some(PathBuf::from(value()?)),
                "--noise-suppression" => noise_suppression = value()?,
                "--allowance-seconds" => allowance_seconds = value()?.parse()?,
                "--diagnostics" => diagnostics = true,
                "--ring-channel-ceiling" => ring_channel_ceiling = Some(value()?.parse()?),
                _ if runtime_flags.take(flag, &value()?)? => {}
                _ => return Err(invalid(&format!("unknown lamp-session option {flag}"))),
            }
        }
        let mode = match (reply, sha256, provider_config, serve_config) {
            (Some(reply), Some(sha256), None, None) => SessionMode::Fixture { reply, sha256 },
            (None, None, Some(provider_config), None) => SessionMode::Directed { provider_config },
            (None, None, None, Some(provider_config)) => SessionMode::Serve { provider_config },
            _ => {
                return Err(invalid(
                    "give one of --fixture-reply with --fixture-sha256, --provider-config or --serve-config",
                ));
            }
        };
        if diagnostics && matches!(mode, SessionMode::Serve { .. }) {
            // lamp-voice serve refuses unbounded raw audio diagnostics.
            return Err(invalid("--diagnostics needs a finite session, not serve"));
        }
        if !matches!(noise_suppression.as_str(), "on" | "off") {
            return Err(invalid("--noise-suppression must be on or off"));
        }
        if !runtime_flags.is_empty() {
            if matches!(mode, SessionMode::Fixture { .. }) {
                return Err(invalid(
                    "runtime policy flags need --provider-config or --serve-config",
                ));
            }
            runtime_flags.validate(&noise_suppression, ring_channel_ceiling, diagnostics)?;
        }
        let options = Self {
            runtime: runtime.ok_or_else(|| invalid("--runtime is required"))?,
            runtime_prefix,
            work: work.ok_or_else(|| invalid("--work is required"))?,
            seconds: seconds.ok_or_else(|| invalid("--seconds is required"))?,
            mode,
            noise_suppression,
            diagnostics,
            allowance_seconds,
            ring_channel_ceiling,
            runtime_flags,
        };
        if !(1..=600).contains(&options.seconds) || !(5..=120).contains(&options.allowance_seconds)
        {
            return Err(invalid("seconds must be 1..600 and allowance 5..120"));
        }
        if !options.work.is_absolute() {
            return Err(invalid("--work must be absolute"));
        }
        Ok(options)
    }

    pub fn runtime_output(&self) -> PathBuf {
        self.work.join("runtime")
    }
    pub fn cue_path(&self) -> PathBuf {
        self.work.join("cue.sock")
    }

    pub fn argv(&self) -> Vec<String> {
        let text = |path: &PathBuf| path.display().to_string();
        let mut argv = vec![text(&self.runtime)];
        argv.extend(self.runtime_prefix.iter().cloned());
        let output = text(&self.runtime_output());
        match &self.mode {
            SessionMode::Fixture { reply, sha256 } => {
                // lamp-voice requires --diagnostics for fixture experiments.
                argv.extend([
                    "directed-fixture".into(),
                    text(reply),
                    sha256.clone(),
                    self.seconds.to_string(),
                    output,
                    "--diagnostics".into(),
                    "--noise-suppression".into(),
                    self.noise_suppression.clone(),
                ]);
            }
            SessionMode::Directed { provider_config } => {
                argv.extend([
                    "directed".into(),
                    text(provider_config),
                    self.seconds.to_string(),
                    output,
                    "--noise-suppression".into(),
                    self.noise_suppression.clone(),
                ]);
                argv.extend(self.runtime_flags.argv());
                if self.diagnostics {
                    argv.push("--diagnostics".into());
                }
            }
            // serve has no length: the relay bounds it and stops it.
            SessionMode::Serve { provider_config } => {
                argv.extend([
                    "serve".into(),
                    text(provider_config),
                    output,
                    "--noise-suppression".into(),
                    self.noise_suppression.clone(),
                ]);
                argv.extend(self.runtime_flags.argv());
            }
        }
        // Every mode explicitly requests the scheduling capability. An older
        // runtime must fail its CLI, never fall back to guessed delays.
        argv.extend(["--cue-socket".into(), text(&self.cue_path())]);
        if let Some(ceiling) = self.ring_channel_ceiling {
            argv.extend(["--ring-channel-ceiling".into(), ceiling.to_string()]);
        }
        argv
    }
}

/// The runtime's command: its argv, and for `directed` and `serve` its route
/// and the run's `LAMP_*` settings in its environment, over the relay's own.
/// A fixture has no provider, so no route.
fn runtime_command(options: &SessionOptions, argv: &[String]) -> Command {
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        // Workers inherit this group, so one signal reaches all of them.
        .process_group(0);
    if !matches!(options.mode, SessionMode::Fixture { .. }) {
        command.envs(options.runtime_flags.runtime_environment());
    }
    command
}

/// Encode one relay line, refusing anything the runner's framing would reject.
fn encode(line: &SessionLine) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(line)?;
    if bytes.len() > MAX_LINE_BYTES {
        return Err(invalid(&format!(
            "encoded relay line of {} bytes exceeds the {MAX_LINE_BYTES}-byte limit",
            bytes.len()
        )));
    }
    bytes.push(b'\n');
    Ok(bytes)
}

fn emit(out: &mut impl Write, line: &SessionLine) -> Result<()> {
    out.write_all(&encode(line)?)?;
    out.flush()?;
    Ok(())
}

/// Bounded failure text retained for `session_end`.
#[derive(Default)]
struct RelayErrors {
    first: Vec<String>,
    omitted: usize,
}
impl RelayErrors {
    const KEPT: usize = 16;
    fn push(&mut self, error: String) {
        if self.first.len() < Self::KEPT {
            self.first.push(error);
        } else {
            self.omitted += 1;
        }
    }
    fn summary(&self) -> Option<String> {
        if self.first.is_empty() {
            return None;
        }
        let mut text = self.first.join("; ");
        if self.omitted > 0 {
            text.push_str(&format!("; {} further relay errors omitted", self.omitted));
        }
        Some(text)
    }
}

type Framed = std::result::Result<String, String>;

const READER_QUEUE_CAPACITY: usize = 256;
const READER_PROGRESS_GUARD: Duration = Duration::from_secs(30);

struct QueuedFrame {
    item: Framed,
    /// Only an actual newline completes a producer's announced frame.
    complete: bool,
}

#[derive(Default)]
struct EnqueuedState {
    frames: u64,
    closed: bool,
}

#[derive(Default)]
struct EnqueuedFrames {
    state: Mutex<EnqueuedState>,
    changed: Condvar,
}

impl EnqueuedFrames {
    fn sent(&self, complete: bool) {
        if complete {
            let mut state = self.state.lock().expect("reader progress lock");
            state.frames += 1;
            self.changed.notify_all();
        }
    }

    fn close(&self) {
        self.state.lock().expect("reader progress lock").closed = true;
        self.changed.notify_all();
    }

    fn wait_until(&self, target: u64, until: Instant, what: &str) -> Result<()> {
        let mut state = self.state.lock().expect("reader progress lock");
        loop {
            if state.frames >= target {
                return Ok(());
            }
            if state.closed {
                return Err(invalid(&format!(
                    "{what} ended after {} complete frames, before fixture target {target}",
                    state.frames
                )));
            }
            let Some(remaining) = until.checked_duration_since(Instant::now()) else {
                return Err(invalid(&format!(
                    "{what} enqueued {} complete frames, below fixture target {target} at the host reader deadline",
                    state.frames
                )));
            };
            (state, _) = self
                .changed
                .wait_timeout(state, remaining)
                .expect("reader progress lock");
        }
    }
}

struct FramedReader {
    receiver: Receiver<QueuedFrame>,
    /// System-clock sessions keep the original queue without progress locks.
    enqueued: Option<Arc<EnqueuedFrames>>,
}

impl FramedReader {
    fn wait_until(&self, target: u64, consumed: u64, until: Instant, what: &str) -> Result<()> {
        if target.saturating_sub(consumed) > READER_QUEUE_CAPACITY as u64 {
            return Err(invalid(&format!(
                "{what} fixture target {target} exceeds the {READER_QUEUE_CAPACITY}-frame queue after {consumed} consumed frames"
            )));
        }
        self.enqueued
            .as_ref()
            .ok_or_else(|| invalid("fixture reader progress was not enabled"))?
            .wait_until(target, until, what)
    }
}

/// Frame a byte stream on its own thread. Invalid frames arrive as `Err`
/// text; the thread ends at end of input, after a read error, or when the
/// receiver is dropped.
fn line_reader<R: Read + Send + 'static>(
    source: R,
    max: usize,
    total: u64,
    what: &'static str,
    controlled: bool,
) -> FramedReader {
    let (sender, receiver) = mpsc::sync_channel(READER_QUEUE_CAPACITY);
    let enqueued = controlled.then(|| Arc::new(EnqueuedFrames::default()));
    let progress = enqueued.clone();
    thread::spawn(move || {
        let mut framer = LineFramer::with_total_limit(source, max, total);
        loop {
            let (item, complete, last) = match framer.next_frame() {
                Ok(None) => break,
                Ok(Some(Frame::Line(line))) => (Ok(line), true, false),
                Ok(Some(frame)) => {
                    let complete = matches!(
                        frame,
                        Frame::Oversized {
                            terminated: true,
                            ..
                        } | Frame::InvalidUtf8 { .. }
                    );
                    (
                        Err(frame.failure(what, max).unwrap_or_default()),
                        complete,
                        false,
                    )
                }
                Err(error) if error.to_string() == STREAM_LIMIT => (
                    Err(format!("{what} exceeded {total} bytes; reading stopped")),
                    false,
                    true,
                ),
                Err(error) => (Err(format!("{what} read failed: {error}")), false, true),
            };
            if sender.send(QueuedFrame { item, complete }).is_err() {
                break;
            }
            if let Some(progress) = &progress {
                // A read or a blocked send is not an enqueued frame.
                progress.sent(complete);
            }
            if last {
                break;
            }
        }
        if let Some(progress) = progress {
            progress.close();
        }
    });
    FramedReader { receiver, enqueued }
}

/// The relay line for one framed runtime stdout item.
fn stdout_line(lamp_received_us: u64, item: Framed, errors: &mut RelayErrors) -> SessionLine {
    let error = match item {
        Ok(line) => {
            let relayed = SessionLine::Stdout {
                lamp_received_us,
                line,
            };
            match encode(&relayed) {
                Ok(_) => return relayed,
                Err(error) => format!("runtime stdout: {error}"),
            }
        }
        Err(error) => error,
    };
    errors.push(error.clone());
    SessionLine::StdoutInvalid {
        lamp_received_us,
        error,
    }
}

fn cue_line(lamp_received_us: u64, datagram: &[u8]) -> Result<SessionLine> {
    Ok(match parse_cue(datagram) {
        Ok(_) => SessionLine::Cue {
            lamp_received_us,
            cue: serde_json::from_slice(datagram)?,
        },
        Err(error) => SessionLine::CueInvalid {
            lamp_received_us,
            error: error.to_string(),
        },
    })
}

/// Stop the runtime and every process still in its group, then reap it.
/// Safe after the runtime has exited: the group outlives its leader while any
/// worker remains, and reaping an already reaped child is a no-op error.
fn kill(child: &mut Child) {
    let _ = rustix::process::kill_process_group(
        rustix::process::Pid::from_child(child),
        rustix::process::Signal::KILL,
    );
    let _ = child.kill();
    let _ = child.wait();
}

/// Ensures an early error (for example a closed relay transport) never leaves
/// the runtime, any of its workers, or the cue socket behind. The process
/// group is killed even when the runtime itself has already exited: its
/// workers may still be running in that group.
struct Cleanup {
    child: Option<Child>,
    socket: Option<PathBuf>,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            kill(child);
        }
        if let Some(path) = &self.socket {
            let _ = fs::remove_file(path);
        }
    }
}

/// The streamed trace: its identity, record count and any failure.
struct TraceStream {
    sha256: Option<String>,
    lines: usize,
    error: Option<String>,
}

/// Stream `events.jsonl` one bounded record per line. The digest always
/// covers the whole file, including anything after a failure. A cut-off final
/// record, an oversized or non-UTF-8 record, malformed JSON or too many
/// records stops streaming with an explicit error.
fn stream_trace(path: &Path, out: &mut impl Write) -> Result<TraceStream> {
    let mut stream = TraceStream {
        sha256: None,
        lines: 0,
        error: None,
    };
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            stream.error = Some(format!("no runtime trace: {error}"));
            return Ok(stream);
        }
    };
    let length = file.metadata()?.len();
    if length > MAX_TRACE_BYTES {
        stream.error = Some(format!(
            "trace of {length} bytes exceeds the {MAX_TRACE_BYTES}-byte relay bound"
        ));
        return Ok(stream);
    }
    // The runtime has exited, but never read past the checked length.
    let mut framer = LineFramer::new(
        HashingReader::new(file.take(MAX_TRACE_BYTES)),
        MAX_TRACE_RECORD_BYTES,
    );
    while stream.error.is_none() {
        let line = match framer.next_frame()? {
            None => break,
            Some(Frame::Line(line)) => line,
            Some(frame) => {
                stream.error = frame.failure("trace", MAX_TRACE_RECORD_BYTES);
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        if stream.lines >= MAX_TRACE_LINES {
            stream.error = Some(format!(
                "trace exceeds the {MAX_TRACE_LINES}-record relay bound; later records were not relayed"
            ));
            break;
        }
        let record = match serde_json::from_str(&line) {
            Ok(record) => record,
            Err(error) => {
                stream.error = Some(format!(
                    "malformed trace record {}: {error}",
                    stream.lines + 1
                ));
                break;
            }
        };
        match encode(&SessionLine::Trace { record }) {
            Ok(bytes) => {
                out.write_all(&bytes)?;
                out.flush()?;
            }
            Err(error) => {
                stream.error = Some(format!("trace record {}: {error}", stream.lines + 1));
                break;
            }
        }
        stream.lines += 1;
    }
    let (sha256, bytes) = framer.finish_hash()?;
    if bytes != length {
        stream.error.get_or_insert(format!(
            "trace changed while relaying ({length} bytes before, {bytes} read)"
        ));
    }
    stream.sha256 = Some(sha256);
    Ok(stream)
}

/// A requested stop of a serve runtime and how its SIGTERM went.
struct StopRequest {
    cause: &'static str,
    reason: Option<String>,
    requested_us: u64,
    signal_sent_us: Option<u64>,
    signal_error: Option<String>,
}

/// Coordinator-first stop, like the pilot unit's `KillMode=mixed`: SIGTERM
/// goes only to the runtime this relay started (the leader of the process
/// group it created). Workers are stopped by the coordinator itself; a
/// group-wide SIGTERM would bypass its ordered cleanup.
fn request_stop(
    child: &Child,
    cause: &'static str,
    reason: Option<String>,
    now: &impl Fn() -> MonoTime,
) -> StopRequest {
    let requested_us = now().as_micros();
    let signal = rustix::process::kill_process(
        rustix::process::Pid::from_child(child),
        rustix::process::Signal::TERM,
    );
    StopRequest {
        cause,
        reason,
        requested_us,
        signal_sent_us: signal.is_ok().then(|| now().as_micros()),
        signal_error: signal.err().map(|error| error.to_string()),
    }
}

/// Whether the relay's own process group is gone, polled within a bound
/// after its leader was reaped (orphaned members are reaped by init).
fn group_gone(child: &Child, within_us: u64) -> bool {
    let group = rustix::process::Pid::from_child(child);
    let until = monotonic_us() + within_us;
    loop {
        if rustix::process::test_kill_process_group(group).is_err() {
            return true;
        }
        if monotonic_us() >= until {
            return false;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// Stream lamp-voice serve's rolling metadata as `trace` lines, oldest
/// generation first. Headers are checked and summarized, not relayed. A
/// missing, overwritten, malformed or gapped history is reported, never
/// repaired: the relay pushes it as an error so the attempt is not scored.
fn stream_metadata(directory: &Path, out: &mut impl Write) -> Result<ServeMetadata> {
    let mut metadata = ServeMetadata::default();
    let mut segments = Vec::new();
    let mut total = 0_u64;
    for slot in 0..SERVE_SEGMENTS {
        let file = match fs::File::open(directory.join(format!("events-{slot}.jsonl"))) {
            Ok(file) => file,
            Err(error) => {
                metadata.error = Some(format!("metadata segment {slot}: {error}"));
                return Ok(metadata);
            }
        };
        let length = file.metadata()?.len();
        if length == 0 {
            continue;
        }
        total += length;
        if total > MAX_TRACE_BYTES {
            metadata.error = Some(format!(
                "metadata exceeds the {MAX_TRACE_BYTES}-byte relay bound"
            ));
            return Ok(metadata);
        }
        let mut framer = LineFramer::new(
            HashingReader::new(file.take(length)),
            MAX_TRACE_RECORD_BYTES,
        );
        let header = match framer.next_frame()? {
            Some(Frame::Line(line)) => serde_json::from_str::<Value>(&line).ok(),
            _ => None,
        };
        let identity = header.as_ref().and_then(|header| {
            (header["kind"] == "service_segment").then_some((
                header["generation"].as_u64()?,
                header["first_writer_event"].as_u64()?,
            ))
        });
        let Some((generation, first_writer_event)) = identity else {
            metadata.error = Some(format!("metadata segment {slot} has no valid header"));
            return Ok(metadata);
        };
        segments.push((generation, slot, first_writer_event, length, framer));
    }
    segments.sort_by_key(|segment| segment.0);
    if segments.is_empty() {
        metadata.error = Some("no serve metadata was written".into());
        return Ok(metadata);
    }
    if segments.windows(2).any(|pair| pair[1].0 != pair[0].0 + 1) {
        metadata.error = Some("metadata segment generations are not contiguous".into());
        return Ok(metadata);
    }
    // Rotation reuses the oldest file: generation 0 gone means lost history.
    metadata.history_truncated = segments[0].0 > 0;
    let mut previous: Option<u64> = None;
    for (generation, slot, first_writer_event, length, mut framer) in segments {
        while metadata.error.is_none() {
            let line = match framer.next_frame()? {
                None => break,
                Some(Frame::Line(line)) => line,
                Some(frame) => {
                    metadata.error = frame.failure("metadata", MAX_TRACE_RECORD_BYTES);
                    break;
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            if metadata.records >= MAX_TRACE_LINES {
                metadata.error = Some(format!(
                    "metadata exceeds the {MAX_TRACE_LINES}-record relay bound"
                ));
                break;
            }
            let record: Value = match serde_json::from_str(&line) {
                Ok(record) => record,
                Err(error) => {
                    metadata.error = Some(format!(
                        "malformed metadata record {}: {error}",
                        metadata.records + 1
                    ));
                    break;
                }
            };
            match record["service_sequence"].as_u64() {
                Some(sequence) => {
                    if previous.is_some_and(|last| last.checked_add(1) != Some(sequence)) {
                        metadata.sequence_gaps += 1;
                    }
                    metadata.first_service_sequence.get_or_insert(sequence);
                    metadata.last_service_sequence = Some(sequence);
                    previous = Some(sequence);
                }
                None => metadata.sequence_gaps += 1,
            }
            match encode(&SessionLine::Trace { record }) {
                Ok(bytes) => {
                    out.write_all(&bytes)?;
                    out.flush()?;
                }
                Err(error) => {
                    metadata.error =
                        Some(format!("metadata record {}: {error}", metadata.records + 1));
                    break;
                }
            }
            metadata.records += 1;
        }
        if metadata.error.is_some() {
            break;
        }
        let (sha256, bytes) = framer.finish_hash()?;
        if bytes != length {
            metadata.error = Some(format!("metadata segment {slot} changed while relaying"));
            break;
        }
        metadata.segments.push(ServeSegment {
            slot,
            generation,
            first_writer_event,
            bytes,
            sha256,
        });
    }
    let mut health = Vec::new();
    if let Ok(file) = fs::File::open(directory.join("health.json")) {
        file.take(MAX_HEALTH_BYTES + 1).read_to_end(&mut health)?;
    }
    metadata.health = (health.len() as u64 <= MAX_HEALTH_BYTES)
        .then(|| serde_json::from_slice(&health).ok())
        .flatten();
    Ok(metadata)
}

/// Every reason a serve session's metadata is missing evidence: the history
/// is overwritten, gapped or incomplete, or the runtime never confirmed its
/// final metadata flush.
fn metadata_problems(metadata: &ServeMetadata, finished: Option<&Value>) -> Vec<String> {
    let mut problems = Vec::new();
    if let Some(error) = &metadata.error {
        problems.push(format!("serve metadata: {error}"));
    }
    if metadata.history_truncated {
        problems.push("serve metadata history was overwritten by rotation".into());
    }
    if metadata.sequence_gaps > 0 {
        problems.push(format!(
            "serve metadata has {} service_sequence gaps",
            metadata.sequence_gaps
        ));
    }
    if metadata
        .health
        .as_ref()
        .is_none_or(|health| health["writer_finished"] != true)
    {
        problems.push("serve metadata health does not report a finished writer".into());
    }
    let report = finished.map(|line| &line["health"]);
    let acknowledged = report.is_some_and(|health| {
        health["acknowledged"] == true
            && health["degraded"] == false
            && health["writer_failed"] == false
            && health["dropped_events"] == 0
            && health["oversized_events"] == 0
            && health["unconfirmed_events"] == 0
    });
    if !acknowledged {
        problems.push(match report {
            None => "the runtime printed no service_metadata_finished acknowledgment".into(),
            Some(health) => {
                format!("the runtime's final metadata acknowledgment is incomplete: {health}")
            }
        });
    }
    problems
}

/// Run one session; returns the process exit code to use.
pub fn run<R: Read + Send + 'static>(
    options: &SessionOptions,
    input: R,
    out: &mut impl Write,
) -> Result<i32> {
    run_with_clock(
        options,
        input,
        out,
        || MonoTime::from_micros(monotonic_us()),
    )
}

/// Run with an explicit source for protocol timestamps and session/stop deadlines.
/// The source must only read time, remain monotonic, and share the runtime's
/// timestamp domain. A body `Clock` adapts as `|| clock.now()`.
///
/// Process-group cleanup and the late-stdout drain remain bounded on the host
/// clock, and every polling sleep is unchanged. This seam does not drive a
/// stepped clock or synchronize children; callers must provide that separately.
/// The CLI continues to call `run` with the system clock.
pub fn run_with_clock<R: Read + Send + 'static>(
    options: &SessionOptions,
    input: R,
    out: &mut impl Write,
    now: impl Fn() -> MonoTime,
) -> Result<i32> {
    run_engine(options, input, out, now, None)
}

/// Complete frames already sent by the parked fixture participants. The relay
/// waits for its reader threads to enqueue these targets before polling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RelayPollTargets {
    pub stdout_frames: u64,
    pub runner_inputs: u64,
}

/// Work consumed by completed relay polls, never inferred from a Pong or a
/// clock sample. `terminal` marks the last poll; final trace/SessionEnd output
/// and process cleanup still follow it before the run returns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RelayPollProgress {
    pub poll_seq: u64,
    pub stdout_frames: u64,
    pub runner_inputs: u64,
    /// The existing fresh sample after a successful serve SIGTERM send. It
    /// does not prove that the runtime observed the signal or exited.
    pub stop_signal_sent_us: Option<u64>,
    pub terminal: bool,
}

struct PollControl<'a> {
    before: &'a mut dyn FnMut() -> Result<RelayPollTargets>,
    completed: &'a mut dyn FnMut(RelayPollProgress) -> Result<()>,
}

/// Run an explicitly coordinated local fixture with real relay I/O and child
/// exit checks. `now` remains a pure read. `before_poll` supplies the current
/// step's producer frame targets; a separate 30-second host guard bounds their
/// enqueue barrier without advancing logical time.
///
/// `completed_poll` runs after cue/stdout/input drains and exit/deadline/grace
/// checks. For a continuing poll it acknowledges work and parks until the next
/// step, replacing only the normal 2 ms sleep. For a terminal poll it must only
/// record progress and return promptly: the caller can acknowledge final output
/// only after this function returns. Neither hook proves the relay's OS exit.
pub fn run_with_controlled_clock<R: Read + Send + 'static>(
    options: &SessionOptions,
    input: R,
    out: &mut impl Write,
    now: impl Fn() -> MonoTime,
    mut before_poll: impl FnMut() -> Result<RelayPollTargets>,
    mut completed_poll: impl FnMut(RelayPollProgress) -> Result<()>,
) -> Result<i32> {
    run_engine(
        options,
        input,
        out,
        now,
        Some(PollControl {
            before: &mut before_poll,
            completed: &mut completed_poll,
        }),
    )
}

fn complete_poll(
    control: &mut Option<PollControl<'_>>,
    progress: &mut RelayPollProgress,
    stop: Option<&StopRequest>,
    terminal: bool,
) -> Result<()> {
    if let Some(control) = control {
        progress.poll_seq = progress
            .poll_seq
            .checked_add(1)
            .ok_or_else(|| invalid("relay fixture poll sequence overflow"))?;
        progress.stop_signal_sent_us = stop.and_then(|request| request.signal_sent_us);
        progress.terminal = terminal;
        (control.completed)(*progress)?;
    } else if !terminal {
        thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

fn run_engine<R: Read + Send + 'static>(
    options: &SessionOptions,
    input: R,
    out: &mut impl Write,
    now: impl Fn() -> MonoTime,
    mut control: Option<PollControl<'_>>,
) -> Result<i32> {
    fs::DirBuilder::new().mode(0o700).create(&options.work)?;
    let mut cleanup = Cleanup {
        child: None,
        socket: None,
    };
    let socket = UnixDatagram::bind(options.cue_path())?;
    cleanup.socket = Some(options.cue_path());
    socket.set_nonblocking(true)?;
    let argv = options.argv();
    let serve = matches!(options.mode, SessionMode::Serve { .. });
    emit(
        out,
        &SessionLine::SessionStart {
            schema: SCHEMA,
            lamp_us: now().as_micros(),
            mode: match options.mode {
                SessionMode::Fixture { .. } => "fixture",
                SessionMode::Directed { .. } => "directed",
                SessionMode::Serve { .. } => "serve",
            }
            .into(),
            cue_socket: true,
            runtime_argv: argv.clone(),
        },
    )?;
    let child = cleanup
        .child
        .insert(runtime_command(options, &argv).spawn()?);
    let stdout = line_reader(
        child
            .stdout
            .take()
            .ok_or_else(|| invalid("runtime stdout unavailable"))?,
        MAX_STDOUT_LINE_BYTES,
        MAX_STDOUT_TOTAL_BYTES,
        "runtime stdout",
        control.is_some(),
    );
    let input = line_reader(
        input,
        MAX_RUNNER_LINE_BYTES,
        MAX_RUNNER_TOTAL_BYTES,
        "runner transport",
        control.is_some(),
    );
    let deadline = now().as_micros()
        + (u64::from(options.seconds) + u64::from(options.allowance_seconds)) * 1_000_000;
    let mut buffer = [0_u8; 1024];
    let mut errors = RelayErrors::default();
    let mut killed = false;
    let mut stop: Option<StopRequest> = None;
    let mut forced_kill = false;
    let mut finished_line: Option<Value> = None;
    let mut progress = RelayPollProgress::default();
    let relay_stdout = |out: &mut dyn Write,
                        item: Framed,
                        errors: &mut RelayErrors,
                        finished_line: &mut Option<Value>|
     -> Result<()> {
        let line = stdout_line(now().as_micros(), item, errors);
        if serve
            && let SessionLine::Stdout { line, .. } = &line
            && let Ok(value) = serde_json::from_str::<Value>(line)
            && value["kind"] == "service_metadata_finished"
        {
            *finished_line = Some(value);
        }
        let bytes = encode(&line)?;
        out.write_all(&bytes)?;
        out.flush()?;
        Ok(())
    };
    let (status, exited_us) = loop {
        if let Some(control) = &mut control {
            let targets = (control.before)()?;
            let until = Instant::now() + READER_PROGRESS_GUARD;
            stdout.wait_until(
                targets.stdout_frames,
                progress.stdout_frames,
                until,
                "runtime stdout",
            )?;
            input.wait_until(
                targets.runner_inputs,
                progress.runner_inputs,
                until,
                "runner transport",
            )?;
        }
        for _ in 0..64 {
            match socket.recv(&mut buffer) {
                Ok(count) => emit(out, &cue_line(now().as_micros(), &buffer[..count])?)?,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error.into()),
            }
        }
        while let Ok(frame) = stdout.receiver.try_recv() {
            relay_stdout(out, frame.item, &mut errors, &mut finished_line)?;
            if frame.complete {
                progress.stdout_frames += 1;
            }
        }
        let mut runner_closed = false;
        loop {
            match input.receiver.try_recv() {
                Ok(frame) => {
                    if frame.complete {
                        progress.runner_inputs += 1;
                    }
                    match frame.item {
                        Ok(line) => match serde_json::from_str(&line) {
                            Ok(RunnerLine::Ping { seq }) => emit(
                                out,
                                &SessionLine::Pong {
                                    seq,
                                    lamp_us: now().as_micros(),
                                },
                            )?,
                            Ok(RunnerLine::Stop { reason }) if serve => {
                                // A repeated stop changes nothing: one signal, one grace.
                                if stop.is_none() {
                                    stop = Some(request_stop(
                                        child,
                                        "runner_stop",
                                        Some(reason),
                                        &now,
                                    ));
                                }
                            }
                            Ok(RunnerLine::Stop { .. }) => {
                                errors.push("stop is only defined for serve sessions".into())
                            }
                            Err(error) => errors.push(format!("invalid runner line: {error}")),
                        },
                        Err(error) => errors.push(error),
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    runner_closed = true;
                    break;
                }
            }
        }
        if let Some(status) = child.try_wait()? {
            let exited_us = now().as_micros();
            complete_poll(&mut control, &mut progress, stop.as_ref(), true)?;
            break (Some(status), Some(exited_us));
        }
        if runner_closed && stop.is_none() {
            errors.push(
                "runner transport closed before the runtime ended; the relay stopped the runtime"
                    .into(),
            );
            if !serve {
                kill(child);
                killed = true;
                complete_poll(&mut control, &mut progress, stop.as_ref(), true)?;
                break (None, None);
            }
            stop = Some(request_stop(child, "transport_closed", None, &now));
        }
        if now().as_micros() >= deadline && stop.is_none() {
            if !serve {
                kill(child);
                killed = true;
                complete_poll(&mut control, &mut progress, stop.as_ref(), true)?;
                break (None, None);
            }
            errors.push("the serve hard deadline passed; the relay stopped the runtime".into());
            stop = Some(request_stop(child, "deadline", None, &now));
        }
        if let Some(request) = &stop
            && now().as_micros() >= request.requested_us + SERVE_STOP_GRACE_US
        {
            // Only the group this relay created is killed.
            kill(child);
            killed = true;
            forced_kill = true;
            errors.push(format!(
                "the runtime outlived the {} ms stop grace; its process group was killed",
                SERVE_STOP_GRACE_US / 1000
            ));
            complete_poll(&mut control, &mut progress, stop.as_ref(), true)?;
            break (None, None);
        }
        complete_poll(&mut control, &mut progress, stop.as_ref(), false)?;
    };
    // A normal runtime exit must not leave workers behind in its group. Any
    // stopped straggler is reported as evidence; the trace itself is unaffected.
    let mut stray_processes_stopped = false;
    if status.is_some() {
        stray_processes_stopped = rustix::process::kill_process_group(
            rustix::process::Pid::from_child(child),
            rustix::process::Signal::KILL,
        )
        .is_ok();
        if serve && stray_processes_stopped {
            // A serve coordinator reaps its own workers; a forced cleanup is
            // never a clean conversation end.
            errors.push("processes outlived the serve runtime and were killed".into());
        }
    }
    // A serve runtime that exits nonzero or by signal never ended a session
    // cleanly, even when its metadata was finished and acknowledged.
    if serve
        && let Some(status) = status
        && !status.success()
    {
        errors.push(match status.code() {
            Some(code) => format!("the serve runtime exited with status {code}"),
            None => format!("the serve runtime ended by signal ({status})"),
        });
    }
    let group_reaped = !serve || group_gone(child, 2_000_000);
    if !group_reaped {
        errors.push("the runtime's process group was still present 2 s after its end".into());
    }
    // Late stdout/cues after exit are still relayed, within a short bound.
    let drain_until = monotonic_us() + 200_000;
    while monotonic_us() < drain_until {
        match stdout.receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(frame) => relay_stdout(out, frame.item, &mut errors, &mut finished_line)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    for _ in 0..64 {
        let count = match socket.recv(&mut buffer) {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(error.into()),
        };
        emit(out, &cue_line(now().as_micros(), &buffer[..count])?)?;
    }
    let (trace, serve_end) = if serve {
        let metadata = stream_metadata(&options.runtime_output().join("metadata"), out)?;
        for problem in metadata_problems(&metadata, finished_line.as_ref()) {
            errors.push(problem);
        }
        if let Some(request) = &stop
            && let Some(error) = &request.signal_error
        {
            errors.push(format!("the stop signal failed: {error}"));
        }
        let trace = TraceStream {
            sha256: None,
            lines: metadata.records,
            error: None,
        };
        let end = ServeEnd {
            stop_cause: stop.as_ref().map_or("runtime_exited", |s| s.cause).into(),
            stop_reason: stop.as_ref().and_then(|s| s.reason.clone()),
            stop_requested_us: stop.as_ref().map(|s| s.requested_us),
            signal_sent_us: stop.as_ref().and_then(|s| s.signal_sent_us),
            signal_error: stop.as_ref().and_then(|s| s.signal_error.clone()),
            exited_us,
            forced_kill,
            group_reaped,
            metadata,
        };
        (trace, Some(Box::new(end)))
    } else {
        (
            stream_trace(&options.runtime_output().join("events.jsonl"), out)?,
            None,
        )
    };
    if let Some(error) = trace.error {
        errors.push(error);
    }
    let exit_code = status.and_then(|s| s.code());
    emit(
        out,
        &SessionLine::SessionEnd {
            lamp_us: now().as_micros(),
            exit_code,
            killed,
            events_sha256: trace.sha256,
            events_lines: trace.lines,
            error: errors.summary(),
            stray_processes_stopped,
            serve: serve_end,
        },
    )?;
    drop(socket);
    drop(cleanup);
    Ok(if killed { 3 } else { 0 })
}

#[cfg(test)]
#[path = "relay_progress_tests.rs"]
mod relay_progress_tests;

#[cfg(test)]
#[path = "serve_relay_progress_tests.rs"]
mod serve_relay_progress_tests;

#[cfg(test)]
mod clock_tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn successful_stop_samples_the_clock_again_after_sending_the_signal() {
        let mut cleanup = Cleanup {
            child: Some(
                Command::new("/bin/sh")
                    .args(["-c", "IFS= read -r line"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .process_group(0)
                    .spawn()
                    .unwrap(),
            ),
            socket: None,
        };
        let reads = Cell::new(0);
        let now = || {
            reads.set(reads.get() + 1);
            MonoTime::from_micros(123_456)
        };
        let stop = request_stop(cleanup.child.as_ref().unwrap(), "test", None, &now);
        assert_eq!(stop.requested_us, 123_456);
        assert_eq!(stop.signal_sent_us, Some(123_456));
        assert_eq!(stop.signal_error, None);
        assert_eq!(reads.get(), 2, "a successful signal needs a fresh sample");
        let child = cleanup.child.as_mut().unwrap();
        assert!(!child.wait().unwrap().success());
    }
}

#[cfg(test)]
mod serve_metadata_tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A metadata directory: `segments` as (slot, generation, first event,
    /// service sequences), plus `health.json` when `finished` is set.
    fn directory(segments: &[(usize, u64, u64, &[u64])], finished: Option<bool>) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lve-meta-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        for slot in 0..SERVE_SEGMENTS {
            let mut text = String::new();
            if let Some((_, generation, first, sequences)) =
                segments.iter().find(|(s, ..)| *s == slot)
            {
                text.push_str(&format!(
                    "{}\n",
                    json!({"kind":"service_segment","generation":generation,
                        "first_writer_event":first,"evidence":"rolling_best_effort_not_benchmark"})
                ));
                for sequence in *sequences {
                    text.push_str(&format!(
                        "{}\n",
                        json!({"kind":"listening_ready","at_us":sequence,"service_sequence":sequence})
                    ));
                }
            }
            fs::write(path.join(format!("events-{slot}.jsonl")), text).unwrap();
        }
        if let Some(finished) = finished {
            fs::write(
                path.join("health.json"),
                json!({"writer_finished":finished}).to_string(),
            )
            .unwrap();
        }
        path
    }

    fn acknowledged(dropped: u64) -> Value {
        json!({"kind":"service_metadata_finished","health":{"acknowledged":true,
            "degraded":false,"writer_failed":false,"dropped_events":dropped,
            "oversized_events":0,"unconfirmed_events":0}})
    }

    fn relayed(out: &[u8]) -> Vec<u64> {
        String::from_utf8(out.to_vec())
            .unwrap()
            .lines()
            .map(|line| match serde_json::from_str(line).unwrap() {
                SessionLine::Trace { record } => record["service_sequence"].as_u64().unwrap(),
                other => panic!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn rotated_segments_relay_in_generation_order_without_headers() {
        // Slot 1 holds the older generation after one rotation.
        let path = directory(&[(1, 0, 0, &[1, 2, 3]), (0, 1, 3, &[4, 5])], Some(true));
        let mut out = Vec::new();
        let metadata = stream_metadata(&path, &mut out).unwrap();
        assert_eq!(relayed(&out), [1, 2, 3, 4, 5]);
        assert_eq!(metadata.records, 5);
        assert_eq!(
            metadata
                .segments
                .iter()
                .map(|s| (s.slot, s.generation))
                .collect::<Vec<_>>(),
            [(1, 0), (0, 1)]
        );
        assert_eq!(
            (
                metadata.first_service_sequence,
                metadata.last_service_sequence
            ),
            (Some(1), Some(5))
        );
        assert!(metadata_problems(&metadata, Some(&acknowledged(0))).is_empty());
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn gapped_overwritten_or_unacknowledged_history_is_missing_evidence() {
        let check = |segments: &[(usize, u64, u64, &[u64])],
                     finished: Option<bool>,
                     line: Option<Value>,
                     expected: &str| {
            let path = directory(segments, finished);
            let metadata = stream_metadata(&path, &mut Vec::new()).unwrap();
            let problems = metadata_problems(&metadata, line.as_ref()).join("; ");
            assert!(problems.contains(expected), "{expected}: {problems}");
            fs::remove_dir_all(path).unwrap();
        };
        check(
            &[(0, 0, 0, &[1, 2, 4])],
            Some(true),
            Some(acknowledged(0)),
            "1 service_sequence gaps",
        );
        check(
            &[(0, 1, 9, &[10]), (1, 2, 10, &[11])],
            Some(true),
            Some(acknowledged(0)),
            "overwritten",
        );
        check(
            &[(0, 0, 0, &[1]), (1, 2, 5, &[6])],
            Some(true),
            Some(acknowledged(0)),
            "not contiguous",
        );
        check(
            &[(0, 0, 0, &[1])],
            Some(false),
            Some(acknowledged(0)),
            "finished writer",
        );
        check(
            &[(0, 0, 0, &[1])],
            None,
            Some(acknowledged(0)),
            "finished writer",
        );
        check(
            &[(0, 0, 0, &[1])],
            Some(true),
            None,
            "no service_metadata_finished",
        );
        check(
            &[(0, 0, 0, &[1])],
            Some(true),
            Some(acknowledged(2)),
            "incomplete",
        );
        check(&[], Some(true), Some(acknowledged(0)), "no serve metadata");
    }

    #[test]
    fn a_segment_without_a_valid_header_is_rejected() {
        let path = directory(&[(0, 0, 0, &[1])], Some(true));
        fs::write(
            path.join("events-0.jsonl"),
            "{\"kind\":\"listening_ready\",\"service_sequence\":1}\n",
        )
        .unwrap();
        let metadata = stream_metadata(&path, &mut Vec::new()).unwrap();
        assert!(
            metadata
                .error
                .as_deref()
                .unwrap()
                .contains("no valid header")
        );
        fs::remove_dir_all(path).unwrap();
    }
}

#[cfg(test)]
mod runtime_flag_tests {
    use super::*;

    fn parse(extra: &[&str]) -> Result<SessionOptions> {
        let mut arguments: Vec<String> = [
            "--runtime",
            "/opt/lamp-voice",
            "--work",
            "/w/a1",
            "--seconds",
            "300",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        arguments.extend(extra.iter().map(|s| s.to_string()));
        SessionOptions::parse(&arguments)
    }

    fn argv(extra: &[&str]) -> Vec<String> {
        parse(extra).unwrap().argv()
    }

    const PILOT: [&str; 12] = [
        "--turn-taking",
        "duplex",
        "--reference-clock",
        "capture",
        "--reference-recovery",
        "off",
        "--playout-start-ms",
        "200",
        "--provider-continuation",
        "kept-start",
        "--noise-suppression",
        "off",
    ];

    #[test]
    fn microphone_flag_is_typed_forwarded_and_rejects_duplicates() {
        for name in ["mic1", "mic2"] {
            let mut flags = RuntimeFlags::default();
            assert!(flags.take("--microphone", name).unwrap());
            flags.validate("off", Some(12), false).unwrap();
            let argv = flags.argv();
            assert_eq!(argv, ["--microphone", name]);
            let parsed = DirectedOptions::parse(&argv).unwrap();
            assert_eq!(parsed.microphone.as_str(), name);
            assert!(flags.take("--microphone", name).is_err());
        }
        let mut flags = RuntimeFlags::default();
        assert!(flags.take("--microphone", "hw:0").is_err());
        assert!(flags.is_empty());
        assert!(flags.argv().is_empty());
    }

    #[test]
    fn omitted_runtime_flags_leave_every_child_argv_unchanged() {
        assert_eq!(
            argv(&["--serve-config", "/p.json", "--ring-channel-ceiling", "12"]),
            [
                "/opt/lamp-voice",
                "serve",
                "/p.json",
                "/w/a1/runtime",
                "--noise-suppression",
                "on",
                "--cue-socket",
                "/w/a1/cue.sock",
                "--ring-channel-ceiling",
                "12",
            ]
        );
        assert_eq!(
            argv(&["--provider-config", "/p.json", "--diagnostics"]),
            [
                "/opt/lamp-voice",
                "directed",
                "/p.json",
                "300",
                "/w/a1/runtime",
                "--noise-suppression",
                "on",
                "--diagnostics",
                "--cue-socket",
                "/w/a1/cue.sock",
            ]
        );
        assert_eq!(
            argv(&["--fixture-reply", "/r.wav", "--fixture-sha256", "ab"]),
            [
                "/opt/lamp-voice",
                "directed-fixture",
                "/r.wav",
                "ab",
                "300",
                "/w/a1/runtime",
                "--diagnostics",
                "--noise-suppression",
                "on",
                "--cue-socket",
                "/w/a1/cue.sock",
            ]
        );
    }

    #[test]
    fn the_pilot_settings_reach_serve_and_directed_exactly() {
        let mut serve = vec!["--serve-config", "/p.json", "--ring-channel-ceiling", "12"];
        serve.extend(PILOT);
        assert_eq!(
            argv(&serve),
            [
                "/opt/lamp-voice",
                "serve",
                "/p.json",
                "/w/a1/runtime",
                "--noise-suppression",
                "off",
                "--turn-taking",
                "duplex",
                "--reference-clock",
                "capture",
                "--reference-recovery",
                "off",
                "--playout-start-ms",
                "200",
                "--provider-continuation",
                "kept-start",
                "--cue-socket",
                "/w/a1/cue.sock",
                "--ring-channel-ceiling",
                "12",
            ]
        );
        let directed = argv(&[
            "--provider-config",
            "/p.json",
            "--turn-taking",
            "v1-baseline",
            "--reference-recovery",
            "bounded",
        ]);
        assert_eq!(
            directed,
            [
                "/opt/lamp-voice",
                "directed",
                "/p.json",
                "300",
                "/w/a1/runtime",
                "--noise-suppression",
                "on",
                "--turn-taking",
                "v1-baseline",
                "--reference-recovery",
                "bounded",
                "--cue-socket",
                "/w/a1/cue.sock",
            ]
        );
        // What the relay forwards is exactly what lamp-voice accepts.
        let forwarded = &directed[5..directed.len() - 2];
        assert!(lamp_voice::options::DirectedOptions::parse(forwarded).is_ok());
    }

    /// The opt-in endpoint trial: the pilot's settings plus `--endpoint
    /// smart-turn`, forwarded last and accepted by lamp-voice's own parser.
    #[test]
    fn the_endpoint_trial_reaches_serve_after_the_pilot_settings() {
        let mut serve = vec!["--serve-config", "/p.json"];
        serve.extend(PILOT);
        serve.extend(["--endpoint", "smart-turn"]);
        let argv = argv(&serve);
        let at = argv.iter().position(|a| a == "--endpoint").unwrap();
        assert_eq!(argv[at + 1], "smart-turn");
        assert_eq!(argv[at - 2], "--provider-continuation");
        let forwarded = &argv[4..argv.len() - 2];
        let options = lamp_voice::options::DirectedOptions::parse(forwarded).unwrap();
        assert_eq!(options.endpoint, Endpoint::SmartTurn);
        assert_eq!(options.turn_taking, TurnTaking::Duplex);
    }

    #[test]
    fn values_and_combinations_lamp_voice_refuses_are_refused_before_any_child() {
        for (extra, expected) in [
            (
                vec![
                    "--serve-config",
                    "/p",
                    "--turn-taking",
                    "vad-only",
                    "--endpoint",
                    "smart-turn",
                ],
                "would refuse",
            ),
            (
                vec!["--serve-config", "/p", "--endpoint", "smart-turn-early"],
                "invalid --endpoint",
            ),
            (
                vec!["--serve-config", "/p", "--turn-taking", "full-duplex"],
                "invalid --turn-taking",
            ),
            (
                vec!["--serve-config", "/p", "--playout-start-ms", "501"],
                "invalid --playout-start-ms",
            ),
            (
                vec!["--serve-config", "/p", "--playout-start-ms", "-1"],
                "invalid --playout-start-ms",
            ),
            (
                vec!["--serve-config", "/p", "--provider-continuation", "maybe"],
                "invalid --provider-continuation",
            ),
            (
                vec![
                    "--serve-config",
                    "/p",
                    "--reference-clock",
                    "capture",
                    "--reference-clock",
                    "fixed",
                ],
                "more than once",
            ),
            // Bounded recovery needs a policy that masks Lamp's own playback.
            (
                vec!["--serve-config", "/p", "--reference-recovery", "bounded"],
                "would refuse",
            ),
            (
                vec![
                    "--serve-config",
                    "/p",
                    "--turn-taking",
                    "vad-only",
                    "--reference-recovery",
                    "bounded",
                ],
                "would refuse",
            ),
            (
                vec![
                    "--fixture-reply",
                    "/r",
                    "--fixture-sha256",
                    "ab",
                    "--turn-taking",
                    "duplex",
                ],
                "need --provider-config",
            ),
        ] {
            let error = parse(&extra).unwrap_err().to_string();
            assert!(error.contains(expected), "{extra:?}: {error}");
        }
    }

    /// The runtime's environment as the relay starts it.
    fn environment(extra: &[&str]) -> Vec<(String, String)> {
        let options = parse(extra).unwrap();
        let argv = options.argv();
        runtime_command(&options, &argv)
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.unwrap_or_default().to_string_lossy().into_owned(),
                )
            })
            .collect()
    }

    /// The owner, 19 Oct 19:3x: every voice test runs on the second route,
    /// and Live only for a comparison named on #1712. So a `serve` or
    /// `directed` runtime starts with `LAMP_VOICE_ROUTE=second` unless the
    /// run names live, with the run's `LAMP_*` settings beside it; a fixture
    /// has no provider and no route.
    #[test]
    fn the_runtime_starts_on_the_second_route_unless_live_is_named() {
        let route = |pairs: &[(String, String)]| {
            pairs
                .iter()
                .find(|(name, _)| name == ROUTE_ENVIRONMENT)
                .map(|(_, value)| value.clone())
        };
        let serve = ["--serve-config", "/p.json"];
        assert_eq!(route(&environment(&serve)).as_deref(), Some("second"));
        let directed = ["--provider-config", "/p.json"];
        assert_eq!(route(&environment(&directed)).as_deref(), Some("second"));
        let live = ["--serve-config", "/p.json", "--voice-route", "live"];
        assert_eq!(route(&environment(&live)).as_deref(), Some("gemini"));
        let configured = [
            "--serve-config",
            "/p.json",
            "--runtime-env",
            "LAMP_VOICE_SECOND_CONFIG=/etc/lampOS/voice/second.json",
            "--runtime-env",
            "LAMP_SECOND_CHAT=deepseek",
        ];
        // `Command` keeps its environment sorted by name.
        assert_eq!(
            environment(&configured),
            [
                ("LAMP_SECOND_CHAT".to_owned(), "deepseek".to_owned()),
                (ROUTE_ENVIRONMENT.to_owned(), "second".to_owned()),
                (
                    SECOND_CONFIG_ENVIRONMENT.to_owned(),
                    "/etc/lampOS/voice/second.json".to_owned()
                ),
            ]
        );
        // Not lamp-voice's options: its argv is the same either way.
        assert_eq!(argv(&configured), argv(&serve));
        let fixture = ["--fixture-reply", "/r.wav", "--fixture-sha256", "abc"];
        assert!(environment(&fixture).is_empty());
    }

    #[test]
    fn a_route_or_setting_lamp_voice_would_not_take_is_refused() {
        for (extra, expected) in [
            (&["--voice-route", "gemini"][..], "second or live"),
            (&["--runtime-env", "PATH=/tmp"][..], "only LAMP_ settings"),
            (
                &["--runtime-env", "lamp_second_chat=deepseek"][..],
                "only LAMP_ settings",
            ),
            (&["--runtime-env", "LAMP_="][..], "only LAMP_ settings"),
            (&["--runtime-env", "LAMP_SECOND_CHAT"][..], "NAME=VALUE"),
            (
                &["--runtime-env", "LAMP_VOICE_ROUTE=second"][..],
                "set by --voice-route",
            ),
            (
                &[
                    "--runtime-env",
                    "LAMP_SECOND_CHAT=a",
                    "--runtime-env",
                    "LAMP_SECOND_CHAT=b",
                ][..],
                "more than once",
            ),
            (
                &["--voice-route", "live", "--voice-route", "live"][..],
                "more than once",
            ),
            (
                &["--runtime-env", "LAMP_DEEPSEEK_KEY=x"][..],
                "names a secret",
            ),
            (
                &["--runtime-env", "LAMP_SECOND_TOKEN=x"][..],
                "names a secret",
            ),
            (
                &["--runtime-env", "LAMP_RELAY_SECRET=x"][..],
                "names a secret",
            ),
            (
                &["--runtime-env", "LAMP_WIFI_PASSWORD=x"][..],
                "names a secret",
            ),
        ] {
            let mut arguments = vec!["--serve-config", "/p.json"];
            arguments.extend(extra);
            let error = parse(&arguments).unwrap_err().to_string();
            assert!(error.contains(expected), "{extra:?}: {error}");
        }
        // A name that only has KEY inside it is a setting, not a secret.
        parse(&[
            "--serve-config",
            "/p.json",
            "--runtime-env",
            "LAMP_SECOND_KEY_TERMS=on",
        ])
        .unwrap();
        // A fixture has no provider: a route or setting for it is refused.
        let error = parse(&[
            "--fixture-reply",
            "/r.wav",
            "--fixture-sha256",
            "abc",
            "--voice-route",
            "live",
        ])
        .unwrap_err()
        .to_string();
        assert!(error.contains("need --provider-config"), "{error}");
    }

    /// What the runner passes to the relay is what the relay takes back:
    /// the route and the settings survive the hop, and lamp-voice's own
    /// argv carries neither.
    #[test]
    fn the_route_and_settings_cross_from_runner_to_relay_unchanged() {
        let mut flags = RuntimeFlags::default();
        for (flag, value) in [
            ("--turn-taking", "duplex"),
            (VoiceRoute::FLAG, "live"),
            (
                "--runtime-env",
                "LAMP_VOICE_SECOND_CONFIG=/etc/lampOS/voice/second.json",
            ),
            ("--runtime-env", "LAMP_SECOND_SPEECH=elevenlabs"),
        ] {
            assert!(flags.take(flag, value).unwrap());
        }
        let relay = flags.relay_argv();
        let mut taken = RuntimeFlags::default();
        for pair in relay.chunks(2) {
            assert!(taken.take(&pair[0], &pair[1]).unwrap(), "{pair:?}");
        }
        assert_eq!(taken, flags);
        assert_eq!(flags.argv(), ["--turn-taking", "duplex"]);
        assert_eq!(flags.route(), VoiceRoute::Live);
    }

    #[test]
    fn the_second_route_without_its_configuration_is_refused_before_a_run() {
        let mut flags = RuntimeFlags::default();
        let error = flags.validate_route().unwrap_err().to_string();
        assert!(error.contains(SECOND_CONFIG_ENVIRONMENT), "{error}");
        flags
            .take(
                "--runtime-env",
                "LAMP_VOICE_SECOND_CONFIG=/etc/lampOS/voice/second.json",
            )
            .unwrap();
        flags.validate_route().unwrap();
        let mut live = RuntimeFlags::default();
        live.take(VoiceRoute::FLAG, "live").unwrap();
        live.validate_route().unwrap();
    }
}
