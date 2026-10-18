//! Explicit local experiment settings. AEC is mandatory. Turn-taking changes
//! prospective admission only, not provider settings, gains, or hard authority.
use crate::provider_worker::Continuation;
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf, str::FromStr};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoiseSuppression {
    #[default]
    On,
    Off,
}
impl NoiseSuppression {
    pub fn enabled(self) -> bool {
        self == Self::On
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
        }
    }
    pub fn software_processing(self) -> SoftwareProcessing {
        SoftwareProcessing {
            aec: SoftwareAec::SonoraAec3,
            noise_suppression: self.enabled(),
        }
    }
}
impl FromStr for NoiseSuppression {
    type Err = io::Error;
    fn from_str(value: &str) -> io::Result<Self> {
        match value {
            "on" => Ok(Self::On),
            "off" => Ok(Self::Off),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "noise suppression must be exactly on or off; AEC remains enabled",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SoftwareAec {
    SonoraAec3,
}

/// Immutable provenance for the actual capture pipeline. Missing provenance in
/// a generic recorder or historical file does not imply either mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SoftwareProcessing {
    aec: SoftwareAec,
    noise_suppression: bool,
}

/// Explicit Lamp input role. Selecting a source changes only the capture PCM;
/// it does not change mixer gains, processing, authority or the speaker route.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Microphone {
    Mic1,
    #[default]
    Mic2,
}
impl Microphone {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mic1 => "mic1",
            Self::Mic2 => "mic2",
        }
    }
    pub fn device(self) -> &'static str {
        match self {
            Self::Mic1 => "plug:device_micro1",
            Self::Mic2 => "plug:device_micro2",
        }
    }
}
impl FromStr for Microphone {
    type Err = io::Error;
    fn from_str(value: &str) -> io::Result<Self> {
        match value {
            "mic1" => Ok(Self::Mic1),
            "mic2" => Ok(Self::Mic2),
            _ => Err(invalid_options()),
        }
    }
}

/// Admission experiment only; neither mode identifies a person or addressee.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TurnTaking {
    #[default]
    VadOnly,
    V1Baseline,
    /// The conservative baseline with echo evidence: a person heard over the
    /// lamp makes it yield, and speech that outlasts the lamp's own sound
    /// interrupts. Without evidence for a block it is the baseline.
    Duplex,
}
impl FromStr for TurnTaking {
    type Err = io::Error;
    fn from_str(value: &str) -> io::Result<Self> {
        match value {
            "vad-only" => Ok(Self::VadOnly),
            "v1-baseline" => Ok(Self::V1Baseline),
            "duplex" => Ok(Self::Duplex),
            _ => Err(invalid_options()),
        }
    }
}

/// How the capture worker keeps the echo reference beside the microphone.
/// `Capture` follows the difference between the two USB clocks; `Fixed` is
/// the margin that a long session uses up.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReferenceClock {
    #[default]
    Fixed,
    Capture,
}
impl ReferenceClock {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Capture => "capture",
        }
    }
}
impl FromStr for ReferenceClock {
    type Err = io::Error;
    fn from_str(value: &str) -> io::Result<Self> {
        match value {
            "fixed" => Ok(Self::Fixed),
            "capture" => Ok(Self::Capture),
            _ => Err(invalid_options()),
        }
    }
}

/// Endpoint experiment. `smart-turn` confirms the baseline detector's pauses
/// (V1-baseline or duplex, which share it) with the
/// separate `lamp-turn-end` process and its pinned model, both
/// installed beside this executable, and keeps the silence window as the
/// fallback for every pause without a usable verdict. It is never a default.
///
/// `smart-turn-fast` is the same worker and model run fast (8-bit weights
/// under ONNX Runtime, whose pinned library must sit beside the worker):
/// 148 ms a verdict on lamp-4ace where the exact one takes 382 ms, which is
/// what lets a verdict arrive before the silence window has ended the turn
/// by itself (docs/endpoint-pilot.md, "The fast model"). Without the
/// library the worker fails at startup and every pause is the window's.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Endpoint {
    #[default]
    Silence,
    SmartTurn,
    SmartTurnFast,
}
impl Endpoint {
    /// Whether pauses are put to the end-of-turn model at all.
    pub fn confirms(self) -> bool {
        self != Self::Silence
    }
}
impl FromStr for Endpoint {
    type Err = io::Error;
    fn from_str(value: &str) -> io::Result<Self> {
        match value {
            "silence" => Ok(Self::Silence),
            "smart-turn" => Ok(Self::SmartTurn),
            "smart-turn-fast" => Ok(Self::SmartTurnFast),
            _ => Err(invalid_options()),
        }
    }
}

/// Experimental coordinator horizon; zero preserves immediate dispatch.
/// This is not an ALSA queue size or a promise of gap-free playback.
///
/// The continuity start is the other choice, not an addition to a horizon:
/// its own cover takes the horizon's place. Neither is selected by default.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlayoutStart {
    horizon_ms: u16,
    continuity: bool,
}
impl PlayoutStart {
    /// The longest plain horizon accepted. On lamp-4ace a 200 ms horizon ran
    /// dry in the first quarter second of 10 of 24 replies: after its first
    /// burst the service was up to 153 ms short of real time.
    pub const LONGEST_MS: u16 = 500;

    pub fn new(milliseconds: u16) -> io::Result<Self> {
        if milliseconds > Self::LONGEST_MS {
            return Err(invalid_options());
        }
        Ok(Self {
            horizon_ms: milliseconds,
            continuity: false,
        })
    }
    /// The offline-evaluated continuity start. Not a measured device setting.
    pub const fn continuity() -> Self {
        Self {
            horizon_ms: 0,
            continuity: true,
        }
    }
    pub fn policy(self) -> Option<lamp_audio::playout::Continuity> {
        self.continuity
            .then_some(lamp_audio::playout::Continuity::EVALUATED)
    }
    /// The plain horizon, or the continuity start's cover.
    pub fn as_micros(self) -> u64 {
        self.gate().horizon_us()
    }
    pub fn gate(self) -> lamp_audio::playout::StartGate {
        match self.policy() {
            Some(policy) => lamp_audio::playout::StartGate::continuous(policy),
            None => lamp_audio::playout::StartGate::new(u64::from(self.horizon_ms) * 1_000),
        }
    }
}
impl FromStr for PlayoutStart {
    type Err = io::Error;
    fn from_str(value: &str) -> io::Result<Self> {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid_options());
        }
        Self::new(value.parse().map_err(|_| invalid_options())?)
    }
}

/// What the capture worker does when it can no longer say which reference
/// belongs to the capture it holds. `Off` ends the session with the fault,
/// as always. `Bounded` recovers within a budget and says so. With the
/// fixed reference that is one thing: a margin used up by the difference
/// between the two clocks is re-centred. With the capture-clocked reference
/// it is also loss, overflow and a speaker that paused: the canceller is
/// not run until fresh reference returns.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReferenceRecovery {
    #[default]
    Off,
    Bounded,
}
impl ReferenceRecovery {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Bounded => "bounded",
        }
    }
}
impl FromStr for ReferenceRecovery {
    type Err = io::Error;
    fn from_str(value: &str) -> io::Result<Self> {
        match value {
            "off" => Ok(Self::Off),
            "bounded" => Ok(Self::Bounded),
            _ => Err(invalid_options()),
        }
    }
}

/// How the echo canceller is given the speaker's sound. `steady` puts a
/// stage before it that follows where the lamp's echo is in the
/// microphone's sound and delays the reference to match, so that the
/// canceller finds the echo in one place (docs/steady-reference.md): on
/// lamp-4ace's voice microphone the echo slides by tens of milliseconds
/// while the lamp speaks. An experiment, and never a default.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EchoReference {
    #[default]
    AsGiven,
    Steady,
}
impl EchoReference {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AsGiven => "as-given",
            Self::Steady => "steady",
        }
    }
    /// What the canceller is made with.
    pub fn reference(self) -> lamp_audio::Reference {
        match self {
            Self::AsGiven => lamp_audio::Reference::AsGiven,
            Self::Steady => lamp_audio::Reference::Steady,
        }
    }
}
impl FromStr for EchoReference {
    type Err = io::Error;
    fn from_str(value: &str) -> io::Result<Self> {
        match value {
            "as-given" => Ok(Self::AsGiven),
            "steady" => Ok(Self::Steady),
            _ => Err(invalid_options()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DirectedOptions {
    pub microphone: Microphone,
    pub playout_start: PlayoutStart,
    pub turn_taking: TurnTaking,
    pub reference_clock: ReferenceClock,
    pub reference_recovery: ReferenceRecovery,
    pub echo_reference: EchoReference,
    /// Explicit opt-in for a person who goes on before the first answer has
    /// begun. Off leaves the provider worker's arguments as they were.
    pub provider_continuation: Continuation,
    pub endpoint: Endpoint,
    pub diagnostics: bool,
    pub noise_suppression: NoiseSuppression,
    /// Explicit qualification opt-in. None leaves the ring device unopened.
    pub ring_channel_ceiling: Option<u16>,
    /// Explicit end-of-speech wait in 10 ms blocks. None keeps each
    /// turn-taking policy's own window.
    pub end_silence_blocks: Option<u16>,
    /// The end sent early (`--early-end-ms`), in 10 ms blocks of quiet:
    /// the request is ended to the provider after this much quiet, and its
    /// answer is held until the end-of-speech wait (or the end-of-turn
    /// model) confirms that the person has finished. None, the default,
    /// sends the end at that confirming moment, as always
    /// (docs/faster-answers.md, way 2).
    pub early_end_blocks: Option<u16>,
    /// How long a pause must last before a confident end-of-turn verdict
    /// ends the turn (`--endpoint-commit-ms`), in 10 ms blocks (#782). None,
    /// the default, keeps the profile's own: 400 ms under a window of a
    /// second or less, a second above it. It must be shorter than the
    /// end-of-speech wait in force, and needs an end-of-turn model.
    pub endpoint_commit_blocks: Option<u16>,
    /// How long an utterance that is only the lamp's name ("Hey Lamp,")
    /// waits for the rest before it ends (`--call-wait-ms`), in 10 ms
    /// blocks (#782). None, the default: it ends as any other. Longer than
    /// the end-of-speech wait, 3 s at most; it needs the bus, where the
    /// lamp's name is heard (`LAMP_BUS=on` and `lamp-name`).
    pub call_wait_blocks: Option<u16>,
    /// A capture block quieter than this, on the face's scale of loudness
    /// after cancellation (`ring_face::level_of`), is not speech whatever
    /// the detector scores it. None leaves every block to the detector.
    pub faint_level: Option<u8>,
}
impl DirectedOptions {
    /// Used directly by the real capture-worker spawn path. The default keeps
    /// its exact previous device and arguments; no extra process or I/O.
    /// The capture worker's argv. `raw_blocks`: each frame also carries
    /// the microphone's block before the canceller, for the stop words
    /// (`LAMP_STOP_WORDS_SOURCE=raw`, #1047).
    pub fn capture_arguments(
        self,
        diagnostic_directory: Option<PathBuf>,
        raw_blocks: bool,
    ) -> io::Result<Vec<String>> {
        AudioWorkerOptions {
            diagnostic_directory,
            noise_suppression: self.noise_suppression,
            reference_clock: self.reference_clock,
            reference_recovery: self.reference_recovery,
            echo_reference: self.echo_reference,
            raw_blocks,
        }
        .child_arguments(self.microphone.device(), true)
    }

    pub fn parse(arguments: &[String]) -> io::Result<Self> {
        let mut options = Self::default();
        let mut noise_seen = false;
        let mut turn_seen = false;
        let mut endpoint_seen = false;
        let mut playout_seen = false;
        let mut reference_seen = false;
        let mut recovery_seen = false;
        let mut echo_seen = false;
        let mut continuation_seen = false;
        let mut microphone_seen = false;
        let mut arguments = arguments.iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--microphone" if !microphone_seen => {
                    options.microphone = required_value(&mut arguments)?.parse()?;
                    microphone_seen = true;
                }
                "--diagnostics" if !options.diagnostics => options.diagnostics = true,
                "--playout-start-ms" if !playout_seen => {
                    options.playout_start = required_value(&mut arguments)?.parse()?;
                    playout_seen = true;
                }
                "--playout-continuity" if !playout_seen => {
                    options.playout_start = PlayoutStart::continuity();
                    playout_seen = true;
                }
                "--turn-taking" if !turn_seen => {
                    options.turn_taking = required_value(&mut arguments)?.parse()?;
                    turn_seen = true;
                }
                "--endpoint" if !endpoint_seen => {
                    options.endpoint = required_value(&mut arguments)?.parse()?;
                    endpoint_seen = true;
                }
                "--noise-suppression" if !noise_seen => {
                    options.noise_suppression = required_value(&mut arguments)?.parse()?;
                    noise_seen = true;
                }
                "--reference-clock" if !reference_seen => {
                    options.reference_clock = required_value(&mut arguments)?.parse()?;
                    reference_seen = true;
                }
                "--reference-recovery" if !recovery_seen => {
                    options.reference_recovery = required_value(&mut arguments)?.parse()?;
                    recovery_seen = true;
                }
                "--echo-reference" if !echo_seen => {
                    options.echo_reference = required_value(&mut arguments)?.parse()?;
                    echo_seen = true;
                }
                Continuation::FLAG if !continuation_seen => {
                    options.provider_continuation = required_value(&mut arguments)?
                        .parse()
                        .map_err(|_| invalid_options())?;
                    continuation_seen = true;
                }
                "--ring-channel-ceiling" if options.ring_channel_ceiling.is_none() => {
                    let value = required_value(&mut arguments)?
                        .parse::<u16>()
                        .map_err(|_| invalid_options())?;
                    lamp_ring::ChannelCeiling::new(value).map_err(io::Error::other)?;
                    options.ring_channel_ceiling = Some(value);
                }
                "--end-silence-ms" if options.end_silence_blocks.is_none() => {
                    let value = required_value(&mut arguments)?
                        .parse::<u16>()
                        .map_err(|_| invalid_options())?;
                    if value % 10 != 0
                        || !(crate::activity::MIN_END_SILENCE_BLOCKS
                            ..=crate::activity::MAX_END_SILENCE_BLOCKS)
                            .contains(&(value / 10))
                    {
                        return Err(invalid_options());
                    }
                    options.end_silence_blocks = Some(value / 10);
                }
                "--endpoint-commit-ms" if options.endpoint_commit_blocks.is_none() => {
                    let value = required_value(&mut arguments)?
                        .parse::<u16>()
                        .map_err(|_| invalid_options())?;
                    if value == 0 || value % 10 != 0 {
                        return Err(invalid_options());
                    }
                    options.endpoint_commit_blocks = Some(value / 10);
                }
                "--call-wait-ms" if options.call_wait_blocks.is_none() => {
                    let value = required_value(&mut arguments)?
                        .parse::<u16>()
                        .map_err(|_| invalid_options())?;
                    if value == 0 || value % 10 != 0 {
                        return Err(invalid_options());
                    }
                    options.call_wait_blocks = Some(value / 10);
                }
                EARLY_END_FLAG if options.early_end_blocks.is_none() => {
                    let value = required_value(&mut arguments)?
                        .parse::<u16>()
                        .map_err(|_| invalid_options())?;
                    if value % 10 != 0 || !(EARLY_END_LEAST_MS..=EARLY_END_MOST_MS).contains(&value)
                    {
                        return Err(invalid_options());
                    }
                    options.early_end_blocks = Some(value / 10);
                }
                "--faint-floor-db" if options.faint_level.is_none() => {
                    // In dBFS, -47 to -20: the face's scale runs from -48
                    // to -14, 255 steps.
                    let value = required_value(&mut arguments)?
                        .parse::<i16>()
                        .map_err(|_| invalid_options())?;
                    if !(-47..=-20).contains(&value) {
                        return Err(invalid_options());
                    }
                    options.faint_level = Some(((value + 48) * 255 / 34) as u8);
                }
                _ => return Err(invalid_options()),
            }
        }
        // A recovered canceller starts cold, and an untrusted reference
        // leaves the lamp's own voice in the microphone uncancelled. A policy
        // that masks own playback has to be in force: `vad-only` masks nothing.
        if options.reference_recovery == ReferenceRecovery::Bounded
            && options.turn_taking == TurnTaking::VadOnly
        {
            return Err(invalid_options());
        }
        // Confirmation is defined against the baseline detector's silence
        // fallback, which V1-baseline and duplex share; `vad-only` has none.
        if options.endpoint.confirms() && options.turn_taking == TurnTaking::VadOnly {
            return Err(invalid_options());
        }
        // The end sent early comes before the wait that confirms it, and a
        // person who goes on is a request given up and continued. Without
        // the kept start a continuation written at once behind an answer
        // that has not begun times out (docs/gemini-session-reliability.md).
        if let Some(early) = options.early_end_blocks
            && (early >= options.end_silence_window_blocks()
                || options.provider_continuation != Continuation::KeptStart)
        {
            return Err(invalid_options());
        }
        // A call waits longer than any other utterance, and not for long.
        if let Some(wait) = options.call_wait_blocks
            && (wait <= options.end_silence_window_blocks()
                || wait > crate::activity::MAX_CALL_WAIT_BLOCKS)
        {
            return Err(invalid_options());
        }
        // A commit is a model's verdict acted on, and only within the wait.
        if let Some(commit) = options.endpoint_commit_blocks
            && (!options.endpoint.confirms() || commit >= options.end_silence_window_blocks())
        {
            return Err(invalid_options());
        }
        Ok(options)
    }

    /// The end-of-speech wait in force, in 10 ms blocks: the explicit one,
    /// or the turn-taking policy's own.
    pub fn end_silence_window_blocks(self) -> u16 {
        self.end_silence_blocks.unwrap_or(match self.turn_taking {
            TurnTaking::V1Baseline | TurnTaking::Duplex => crate::activity::V1_END_SILENCE_BLOCKS,
            TurnTaking::VadOnly => crate::activity::END_SILENCE_BLOCKS,
        })
    }
}

/// The option that sends the end early, and the quiet it may be set to, in
/// milliseconds and a multiple of ten. It must also be shorter than the
/// end-of-speech wait in force.
pub const EARLY_END_FLAG: &str = "--early-end-ms";
pub const EARLY_END_LEAST_MS: u16 = 100;
pub const EARLY_END_MOST_MS: u16 = 1_000;

/// Optional local failure speech. Both values are explicit; this grants no
/// output authority and never creates or downloads recordings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NoticeOptions {
    directory: PathBuf,
    language: String,
}
impl NoticeOptions {
    pub fn new(directory: PathBuf, language: String) -> io::Result<Self> {
        if !directory.is_absolute()
            || language.is_empty()
            || language.len() > 35
            || !language
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || language.split('-').any(str::is_empty)
        {
            return Err(invalid_options());
        }
        Ok(Self {
            directory,
            language,
        })
    }
    pub(crate) fn directory(&self) -> &std::path::Path {
        &self.directory
    }
    pub(crate) fn language(&self) -> &str {
        &self.language
    }
}

/// Optional observation of a finite conversation. Cues contain lifecycle
/// metadata only; enabling them does not enable microphone PCM diagnostics.
/// Explicit dedicated motor executable and qualification; no defaults.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadOptions {
    pub plan: PathBuf,
    pub channel: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MotorOptions {
    pub worker: PathBuf,
    pub config: PathBuf,
    pub head: Option<HeadOptions>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionOptions {
    pub directed: DirectedOptions,
    pub cue_socket: Option<PathBuf>,
    /// Opt-in direct camera context; absent means no camera process or device.
    pub camera_config: Option<PathBuf>,
    pub failure_notices: Option<NoticeOptions>,
    pub motor: Option<MotorOptions>,
    /// Independent local hand-control experiments, both off unless selected.
    pub controls: crate::hand_controls::ControlsOptions,
}
impl SessionOptions {
    pub fn validate_serve(&self) -> io::Result<()> {
        self.controls.validate()?;
        if self.directed.diagnostics || self.motor.as_ref().is_some_and(|m| m.head.is_some()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "serve rejects raw audio diagnostics/head qualification; use a finite directed session",
            ));
        }
        Ok(())
    }

    pub fn parse(arguments: &[String]) -> io::Result<Self> {
        let mut remaining = Vec::new();
        let mut cue_socket = None;
        let mut camera_config = None;
        let mut motor_worker = None;
        let mut motor_config = None;
        let mut head_plan = None;
        let mut head_channel = None;
        let mut notice_directory = None;
        let mut notice_language = None;
        let mut controls = crate::hand_controls::ControlsOptions {
            head_touch: crate::hand_controls::head_touch_from_environment(),
            wait: crate::hand_controls::wait_from_environment(),
            ..Default::default()
        };
        // The head tap's "hm?" level, tuned by ear (#711): refused at start
        // when wrong, never found wrong at the first tap.
        if controls.head_touch {
            crate::head_cue::level_from_environment()
                .map_err(|why| io::Error::new(io::ErrorKind::InvalidInput, why))?;
        }
        let mut arguments = arguments.iter();
        while let Some(argument) = arguments.next() {
            if matches!(
                argument.as_str(),
                "--cue-socket"
                    | "--camera-config"
                    | "--failure-notice-dir"
                    | "--motor-worker"
                    | "--motor-config"
                    | "--head-qualification-plan"
                    | "--head-qualification-channel"
                    | "--controls-mute"
                    | "--controls-bus-root"
            ) {
                let slot = if argument == "--cue-socket" {
                    &mut cue_socket
                } else if argument == "--camera-config" {
                    &mut camera_config
                } else if argument == "--motor-worker" {
                    &mut motor_worker
                } else if argument == "--head-qualification-plan" {
                    &mut head_plan
                } else if argument == "--head-qualification-channel" {
                    &mut head_channel
                } else if argument == "--motor-config" {
                    &mut motor_config
                } else if argument == "--controls-mute" {
                    &mut controls.mute_record
                } else if argument == "--controls-bus-root" {
                    &mut controls.bus_root
                } else {
                    &mut notice_directory
                };
                if slot.is_some() {
                    return Err(invalid_options());
                }
                let path = PathBuf::from(required_value(&mut arguments)?);
                if !path.is_absolute() {
                    return Err(invalid_options());
                }
                *slot = Some(path);
            } else if argument == "--controls-stop" && !controls.stop {
                controls.stop = true;
            } else if argument == "--failure-notice-language" && notice_language.is_none() {
                notice_language = Some(required_value(&mut arguments)?.to_owned());
            } else {
                remaining.push(argument.clone());
            }
        }
        let failure_notices = match (notice_directory, notice_language) {
            (None, None) => None,
            (Some(directory), Some(language)) => Some(NoticeOptions::new(directory, language)?),
            _ => return Err(invalid_options()),
        };
        let head = match (head_plan, head_channel) {
            (None, None) => None,
            (Some(plan), Some(channel))
                if motor_worker.is_some() && motor_config.is_some() && camera_config.is_some() =>
            {
                Some(HeadOptions { plan, channel })
            }
            _ => return Err(invalid_options()),
        };
        controls.validate()?;
        Ok(Self {
            directed: DirectedOptions::parse(&remaining)?,
            controls,
            cue_socket,
            camera_config,
            failure_notices,
            motor: match (motor_worker, motor_config) {
                (None, None) => None,
                (Some(worker), Some(config)) => Some(MotorOptions {
                    worker,
                    config,
                    head,
                }),
                _ => return Err(invalid_options()),
            },
        })
    }
}

/// Internal child CLI: only named options can identify mode or diagnostic path.
/// Parsing finishes before IPC binding, recorder creation or device opening.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AudioWorkerOptions {
    pub diagnostic_directory: Option<PathBuf>,
    pub noise_suppression: NoiseSuppression,
    pub reference_clock: ReferenceClock,
    pub reference_recovery: ReferenceRecovery,
    pub echo_reference: EchoReference,
    /// Capture only: each frame also carries the block as the microphone
    /// recorded it, before the canceller (`--raw-blocks`; the stop words'
    /// `LAMP_STOP_WORDS_SOURCE=raw`, #1047).
    pub raw_blocks: bool,
}
impl AudioWorkerOptions {
    /// The exact argv tail used by the supervisor. Values remain individual
    /// arguments (including paths with spaces); no shell quoting is involved.
    pub fn child_arguments(&self, device: &str, is_capture: bool) -> io::Result<Vec<String>> {
        if device.is_empty() || (!is_capture && self.noise_suppression != NoiseSuppression::On) {
            return Err(invalid_options());
        }
        let mut arguments = vec![device.to_owned()];
        if is_capture {
            arguments.extend([
                "--noise-suppression".to_owned(),
                self.noise_suppression.as_str().to_owned(),
            ]);
            // Only when chosen, so that the default argv is what it was.
            if self.reference_clock != ReferenceClock::Fixed {
                arguments.extend([
                    "--reference-clock".to_owned(),
                    self.reference_clock.as_str().to_owned(),
                ]);
            }
            if self.reference_recovery != ReferenceRecovery::Off {
                arguments.extend([
                    "--reference-recovery".to_owned(),
                    self.reference_recovery.as_str().to_owned(),
                ]);
            }
            if self.echo_reference != EchoReference::AsGiven {
                arguments.extend([
                    "--echo-reference".to_owned(),
                    self.echo_reference.as_str().to_owned(),
                ]);
            }
            if self.raw_blocks {
                arguments.push("--raw-blocks".to_owned());
            }
        } else if self.reference_clock != ReferenceClock::Fixed
            || self.reference_recovery != ReferenceRecovery::Off
            || self.echo_reference != EchoReference::AsGiven
            || self.raw_blocks
        {
            return Err(invalid_options());
        }
        if let Some(path) = &self.diagnostic_directory {
            if !path.is_absolute() {
                return Err(invalid_options());
            }
            arguments.extend([
                "--diagnostics".to_owned(),
                path.to_str().ok_or_else(invalid_options)?.to_owned(),
            ]);
        }
        Ok(arguments)
    }

    pub fn parse(arguments: &[String], is_capture: bool) -> io::Result<Self> {
        let mut options = Self::default();
        let mut noise_seen = false;
        let mut reference_seen = false;
        let mut arguments = arguments.iter();
        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--diagnostics" if options.diagnostic_directory.is_none() => {
                    let path = PathBuf::from(required_value(&mut arguments)?);
                    if !path.is_absolute() {
                        return Err(invalid_options());
                    }
                    options.diagnostic_directory = Some(path);
                }
                "--noise-suppression" if is_capture && !noise_seen => {
                    options.noise_suppression = required_value(&mut arguments)?.parse()?;
                    noise_seen = true;
                }
                "--reference-clock" if is_capture && !reference_seen => {
                    options.reference_clock = required_value(&mut arguments)?.parse()?;
                    reference_seen = true;
                }
                "--reference-recovery"
                    if is_capture && options.reference_recovery == ReferenceRecovery::Off =>
                {
                    options.reference_recovery = required_value(&mut arguments)?.parse()?;
                    if options.reference_recovery == ReferenceRecovery::Off {
                        // Never sent: the default argv has no such flag.
                        return Err(invalid_options());
                    }
                }
                "--echo-reference"
                    if is_capture && options.echo_reference == EchoReference::AsGiven =>
                {
                    options.echo_reference = required_value(&mut arguments)?.parse()?;
                    if options.echo_reference == EchoReference::AsGiven {
                        // Never sent: the default argv has no such flag.
                        return Err(invalid_options());
                    }
                }
                "--raw-blocks" if is_capture && !options.raw_blocks => options.raw_blocks = true,
                _ => return Err(invalid_options()),
            }
        }
        Ok(options)
    }
}

fn required_value<'a>(arguments: &mut impl Iterator<Item = &'a String>) -> io::Result<&'a str> {
    arguments
        .next()
        .map(String::as_str)
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(invalid_options)
}
fn invalid_options() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "missing, duplicate or unknown audio experiment option",
    )
}

#[cfg(test)]
mod hand_option_tests {
    use super::*;
    fn parse(arguments: &[&str]) -> io::Result<SessionOptions> {
        SessionOptions::parse(
            &arguments
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
        )
    }
    #[test]
    fn hand_actions_require_independent_explicit_switches() {
        assert!(!parse(&[]).unwrap().controls.enabled());
        let stop = parse(&["--controls-stop"]).unwrap().controls;
        assert!(stop.stop && stop.mute_record.is_none());
        let mute = parse(&["--controls-mute", "/private/voice/mute.json"])
            .unwrap()
            .controls;
        assert!(!mute.stop && mute.mute_record.is_some());
        let both = parse(&[
            "--controls-stop",
            "--controls-mute",
            "/private/voice/mute.json",
            "--controls-bus-root",
            "/run/lampos-bus",
        ])
        .unwrap();
        both.validate_serve().unwrap();
        assert!(both.controls.stop && both.controls.mute_record.is_some());
        // A root is configuration, never permission for either action.
        assert!(
            !parse(&["--controls-bus-root", "/tmp/unused"])
                .unwrap()
                .controls
                .enabled()
        );
    }
    #[test]
    fn hand_switches_reject_missing_duplicate_and_relative_values() {
        for arguments in [
            vec!["--controls-stop", "--controls-stop"],
            vec!["--controls-mute"],
            vec!["--controls-mute", "relative.json"],
            vec!["--controls-mute", "/a", "--controls-mute", "/b"],
            vec!["--controls-bus-root", "relative"],
            vec!["--controls-bus-root", "/a", "--controls-bus-root", "/b"],
        ] {
            assert!(parse(&arguments).is_err(), "{arguments:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }
    #[test]
    fn microphone_selection_reaches_the_capture_spawn_without_changing_processing() {
        let default = DirectedOptions::default()
            .capture_arguments(None, false)
            .unwrap();
        assert_eq!(default, ["plug:device_micro2", "--noise-suppression", "on"]);
        for (name, device) in [
            ("mic1", "plug:device_micro1"),
            ("mic2", "plug:device_micro2"),
        ] {
            let chosen = SessionOptions::parse(&args(&[
                "--microphone",
                name,
                "--turn-taking",
                "duplex",
                "--noise-suppression",
                "off",
                "--reference-clock",
                "capture",
                "--reference-recovery",
                "bounded",
            ]))
            .unwrap();
            chosen.validate_serve().unwrap();
            assert_eq!(
                chosen
                    .directed
                    .capture_arguments(Some(PathBuf::from("/tmp/mic diagnostic")), false)
                    .unwrap(),
                [
                    device,
                    "--noise-suppression",
                    "off",
                    "--reference-clock",
                    "capture",
                    "--reference-recovery",
                    "bounded",
                    "--diagnostics",
                    "/tmp/mic diagnostic"
                ]
            );
            assert_eq!(
                serde_json::to_value(chosen.directed.microphone).unwrap(),
                name
            );
        }
        for flags in [
            vec!["--microphone"],
            vec!["--microphone", "mic3"],
            vec!["--microphone", "MIC1"],
            vec!["--microphone", "hw:0"],
            vec!["--microphone", "mic1", "--microphone", "mic2"],
            vec!["--microphone", "mic2", "--microphone", "mic2"],
        ] {
            assert!(SessionOptions::parse(&args(&flags)).is_err(), "{flags:?}");
        }
    }

    #[test]
    fn service_rejects_raw_recording_but_preserves_explicit_audio_camera_ring_options() {
        assert!(SessionOptions::default().validate_serve().is_ok());
        assert!(
            SessionOptions::parse(&args(&["--diagnostics"]))
                .unwrap()
                .validate_serve()
                .is_err()
        );
        let options = SessionOptions::parse(&args(&[
            "--camera-config",
            "/tmp/camera.json",
            "--ring-channel-ceiling",
            "24",
            "--turn-taking",
            "v1-baseline",
            "--playout-start-ms",
            "200",
        ]))
        .unwrap();
        assert!(options.validate_serve().is_ok());
        assert_eq!(options.directed.ring_channel_ceiling, Some(24));
    }
    #[test]
    fn startup_buffer_defaults_to_zero_and_bounds_cli_and_programmatic_callers() {
        assert_eq!(
            SessionOptions::default().directed.playout_start.as_micros(),
            0
        );
        for ms in [0_u16, 1, 120, 160, 200, 350, 500] {
            let parsed = SessionOptions::parse(&args(&[
                "--playout-start-ms",
                &ms.to_string(),
                "--turn-taking",
                "v1-baseline",
                "--noise-suppression",
                "off",
            ]))
            .unwrap();
            assert_eq!(
                parsed.directed.playout_start.as_micros(),
                u64::from(ms) * 1_000
            );
            assert_eq!(parsed.directed.turn_taking, TurnTaking::V1Baseline);
            assert!(!parsed.directed.diagnostics);
        }
        assert!(PlayoutStart::new(501).is_err());
        assert!(PlayoutStart::new(u16::MAX).is_err());
        for flags in [
            vec!["--playout-start-ms"],
            vec!["--playout-start-ms", "501"],
            vec!["--playout-start-ms", "65536"],
            vec!["--playout-start-ms", "-1"],
            vec!["--playout-start-ms", "+1"],
            vec!["--playout-start-ms", "0.2"],
            vec!["--playout-start-ms", "--diagnostics"],
            vec!["--playout-start-ms", "0", "--playout-start-ms", "200"],
        ] {
            assert!(SessionOptions::parse(&args(&flags)).is_err(), "{flags:?}");
        }
        assert!(AudioWorkerOptions::parse(&args(&["--playout-start-ms", "200"]), true).is_err());
        assert!(AudioWorkerOptions::parse(&args(&["--playout-start-ms", "200"]), false).is_err());
    }

    #[test]
    fn the_endpoint_commit_is_chosen_only_with_a_model_and_within_the_wait() {
        let parse = |extra: &[&str]| {
            let mut all = vec!["--turn-taking", "duplex", "--end-silence-ms", "600"];
            all.extend_from_slice(extra);
            SessionOptions::parse(&args(&all))
        };
        assert_eq!(
            parse(&["--endpoint", "smart-turn-fast"])
                .unwrap()
                .directed
                .endpoint_commit_blocks,
            None
        );
        assert_eq!(
            parse(&[
                "--endpoint",
                "smart-turn-fast",
                "--endpoint-commit-ms",
                "550"
            ])
            .unwrap()
            .directed
            .endpoint_commit_blocks,
            Some(55)
        );
        for wrong in [
            vec!["--endpoint-commit-ms", "550"],
            vec![
                "--endpoint",
                "smart-turn-fast",
                "--endpoint-commit-ms",
                "555",
            ],
            vec!["--endpoint", "smart-turn-fast", "--endpoint-commit-ms", "0"],
            vec![
                "--endpoint",
                "smart-turn-fast",
                "--endpoint-commit-ms",
                "600",
            ],
            vec!["--endpoint", "smart-turn-fast", "--endpoint-commit-ms"],
        ] {
            assert!(parse(&wrong).is_err(), "{wrong:?}");
        }
    }

    #[test]
    fn a_call_waits_only_when_chosen_and_longer_than_the_end_of_speech_wait() {
        let parse = |extra: &[&str]| {
            let mut all = vec!["--turn-taking", "duplex", "--end-silence-ms", "600"];
            all.extend_from_slice(extra);
            SessionOptions::parse(&args(&all))
        };
        assert_eq!(parse(&[]).unwrap().directed.call_wait_blocks, None);
        assert_eq!(
            parse(&["--call-wait-ms", "2000"])
                .unwrap()
                .directed
                .call_wait_blocks,
            Some(200)
        );
        for wrong in [
            vec!["--call-wait-ms", "600"],
            vec!["--call-wait-ms", "3010"],
            vec!["--call-wait-ms", "2005"],
            vec!["--call-wait-ms", "0"],
            vec!["--call-wait-ms"],
            vec!["--call-wait-ms", "2000", "--call-wait-ms", "2000"],
        ] {
            assert!(parse(&wrong).is_err(), "{wrong:?}");
        }
    }

    #[test]
    fn smart_turn_endpoint_is_explicit_and_requires_the_baseline_detector() {
        assert_eq!(
            SessionOptions::default().directed.endpoint,
            Endpoint::Silence
        );
        let options = SessionOptions::parse(&args(&[
            "--turn-taking",
            "v1-baseline",
            "--endpoint",
            "smart-turn",
        ]))
        .unwrap();
        assert_eq!(options.directed.endpoint, Endpoint::SmartTurn);
        assert_eq!(
            serde_json::to_value(options.directed.endpoint).unwrap(),
            "smart-turn"
        );
        // Duplex gates the same baseline detector, so the trial can keep the
        // cached-speech cohorts' duplex policy.
        let duplex = SessionOptions::parse(&args(&[
            "--turn-taking",
            "duplex",
            "--endpoint",
            "smart-turn",
        ]))
        .unwrap();
        assert_eq!(duplex.directed.endpoint, Endpoint::SmartTurn);
        // The fast form is a value of its own and no default either.
        let fast = SessionOptions::parse(&args(&[
            "--turn-taking",
            "duplex",
            "--endpoint",
            "smart-turn-fast",
        ]))
        .unwrap();
        assert_eq!(fast.directed.endpoint, Endpoint::SmartTurnFast);
        assert_eq!(
            serde_json::to_value(fast.directed.endpoint).unwrap(),
            "smart-turn-fast"
        );
        assert!(Endpoint::SmartTurnFast.confirms() && Endpoint::SmartTurn.confirms());
        assert!(!Endpoint::Silence.confirms());
        for values in [
            vec!["--endpoint", "smart-turn"],
            vec!["--endpoint", "smart-turn-fast"],
            vec!["--turn-taking", "vad-only", "--endpoint", "smart-turn-fast"],
            vec!["--turn-taking", "vad-only", "--endpoint", "smart-turn"],
            vec!["--turn-taking", "v1-baseline", "--endpoint"],
            vec!["--turn-taking", "v1-baseline", "--endpoint", "Smart-Turn"],
            vec![
                "--turn-taking",
                "v1-baseline",
                "--endpoint",
                "silence",
                "--endpoint",
                "smart-turn",
            ],
        ] {
            assert!(SessionOptions::parse(&args(&values)).is_err(), "{values:?}");
        }
    }

    #[test]
    fn continuity_is_explicit_and_takes_the_place_of_a_horizon() {
        use lamp_audio::playout::Continuity;
        let default = SessionOptions::default().directed.playout_start;
        assert_eq!(
            (default.policy(), default.gate().continuity()),
            (None, None)
        );
        for ms in [0_u16, 200] {
            let plain = PlayoutStart::new(ms).unwrap();
            assert_eq!(plain.policy(), None);
            assert_eq!(plain.gate().horizon_us(), u64::from(ms) * 1_000);
        }
        let parsed = SessionOptions::parse(&args(&[
            "--playout-continuity",
            "--turn-taking",
            "v1-baseline",
        ]))
        .unwrap()
        .directed;
        assert_eq!(parsed.playout_start, PlayoutStart::continuity());
        assert_eq!(parsed.playout_start.policy(), Some(Continuity::EVALUATED));
        assert_eq!(
            parsed.playout_start.gate().continuity(),
            Some(Continuity::EVALUATED)
        );
        // The trace's horizon field reports the cover that replaced it.
        assert_eq!(
            parsed.playout_start.as_micros(),
            Continuity::EVALUATED.cover_us
        );
        assert_eq!(parsed.turn_taking, TurnTaking::V1Baseline);
        for flags in [
            vec!["--playout-continuity", "--playout-continuity"],
            vec!["--playout-continuity", "--playout-start-ms", "200"],
            vec!["--playout-start-ms", "0", "--playout-continuity"],
            vec!["--playout-continuity", "on"],
            vec!["--playout-continuity=on"],
        ] {
            assert!(SessionOptions::parse(&args(&flags)).is_err(), "{flags:?}");
        }
        assert!(AudioWorkerOptions::parse(&args(&["--playout-continuity"]), true).is_err());
        assert!(AudioWorkerOptions::parse(&args(&["--playout-continuity"]), false).is_err());
    }

    #[test]
    fn duplex_and_the_capture_clocked_reference_are_explicit_and_separate() {
        let defaults = SessionOptions::default().directed;
        assert_eq!(defaults.turn_taking, TurnTaking::VadOnly);
        assert_eq!(defaults.reference_clock, ReferenceClock::Fixed);
        let options = SessionOptions::parse(&args(&[
            "--turn-taking",
            "duplex",
            "--reference-clock",
            "capture",
        ]))
        .unwrap()
        .directed;
        assert_eq!(options.turn_taking, TurnTaking::Duplex);
        assert_eq!(options.reference_clock, ReferenceClock::Capture);
        assert_eq!(
            serde_json::to_value((options.turn_taking, options.reference_clock)).unwrap(),
            serde_json::json!(["duplex", "capture"])
        );
        // Either without the other.
        let only = DirectedOptions::parse(&args(&["--turn-taking", "duplex"])).unwrap();
        assert_eq!(only.reference_clock, ReferenceClock::Fixed);
        let only = DirectedOptions::parse(&args(&["--reference-clock", "capture"])).unwrap();
        assert_eq!(only.turn_taking, TurnTaking::VadOnly);
        for values in [
            vec!["--reference-clock"],
            vec!["--reference-clock", "Capture"],
            vec!["--reference-clock", "speaker"],
            vec!["--reference-clock", "fixed", "--reference-clock", "capture"],
            vec!["--turn-taking", "Duplex"],
        ] {
            assert!(SessionOptions::parse(&args(&values)).is_err());
        }
    }

    #[test]
    fn the_reference_clock_reaches_only_the_capture_worker_and_only_when_chosen() {
        let fixed = AudioWorkerOptions::default();
        assert_eq!(
            fixed.child_arguments("plug:device_micro2", true).unwrap(),
            ["plug:device_micro2", "--noise-suppression", "on"],
            "the default argv is what it was"
        );
        let clocked = AudioWorkerOptions {
            reference_clock: ReferenceClock::Capture,
            ..AudioWorkerOptions::default()
        };
        let arguments = clocked.child_arguments("plug:device_micro2", true).unwrap();
        assert_eq!(
            arguments,
            [
                "plug:device_micro2",
                "--noise-suppression",
                "on",
                "--reference-clock",
                "capture"
            ]
        );
        assert_eq!(
            AudioWorkerOptions::parse(&arguments[1..], true).unwrap(),
            clocked
        );
        // The speaker worker has no reference to clock.
        assert!(
            clocked
                .child_arguments("plug:device_speaker", false)
                .is_err()
        );
        assert!(
            AudioWorkerOptions::parse(&args(&["--reference-clock", "capture"]), false).is_err()
        );
        for values in [
            vec!["--reference-clock"],
            vec!["--reference-clock", "sometimes"],
            vec![
                "--reference-clock",
                "capture",
                "--reference-clock",
                "capture",
            ],
        ] {
            assert!(AudioWorkerOptions::parse(&args(&values), true).is_err());
        }
    }

    #[test]
    fn provider_continuation_is_explicit_and_reaches_only_the_provider_worker() {
        let default = SessionOptions::default().directed;
        assert_eq!(default.provider_continuation, Continuation::Off);
        // Off adds nothing to the worker's command line.
        assert!(default.provider_continuation.arguments().is_empty());
        let chosen = DirectedOptions::parse(&args(&["--provider-continuation", "kept-start"]))
            .unwrap()
            .provider_continuation;
        assert_eq!(chosen, Continuation::KeptStart);
        let worker = chosen.arguments();
        assert_eq!(worker, ["--provider-continuation", "kept-start"]);
        // The worker takes exactly that off the front of its flags and
        // leaves the rest, such as the camera context, as they were.
        let mut flags = worker.clone();
        flags.push("--camera-context".to_owned());
        let (taken, rest) = Continuation::take(&flags).unwrap();
        assert_eq!((taken, rest.len()), (Continuation::KeptStart, 1));
        let (taken, rest) = Continuation::take(&flags[2..]).unwrap();
        assert_eq!((taken, rest.len()), (Continuation::Off, 1));
        assert_eq!(
            DirectedOptions::parse(&args(&["--provider-continuation", "off"]))
                .unwrap()
                .provider_continuation,
            Continuation::Off
        );
        for bad in [
            vec!["--provider-continuation"],
            vec!["--provider-continuation", "on"],
            vec!["--provider-continuation=kept-start"],
            vec![
                "--provider-continuation",
                "kept-start",
                "--provider-continuation",
                "off",
            ],
        ] {
            assert!(DirectedOptions::parse(&args(&bad)).is_err(), "{bad:?}");
        }
        assert!(Continuation::take(&args(&["--provider-continuation"])).is_err());
        assert!(Continuation::take(&args(&["--provider-continuation", "on"])).is_err());
    }

    #[test]
    fn a_reply_fixture_refuses_the_kept_start_and_gemini_chat_accepts_it() {
        use crate::fixture_provider::FixtureOptions;
        // The fixture's provider worker takes exactly its WAV and digest:
        // an extra argument would be read as something else. So the kept
        // start is refused before any worker is started.
        assert!(
            FixtureOptions::parse(&args(&[
                "--diagnostics",
                "--provider-continuation",
                "kept-start",
            ]))
            .is_err()
        );
        // Off, said or unsaid, is the fixture as it always was, and adds
        // nothing to its worker's arguments.
        for flags in [
            vec!["--diagnostics"],
            vec!["--diagnostics", "--provider-continuation", "off"],
        ] {
            let fixture = FixtureOptions::parse(&args(&flags)).unwrap();
            assert_eq!(fixture.directed.provider_continuation, Continuation::Off);
            assert!(
                fixture
                    .directed
                    .provider_continuation
                    .arguments()
                    .is_empty()
            );
        }
        // Directed Gemini chat takes it, and is off by default.
        let chat =
            SessionOptions::parse(&args(&["--provider-continuation", "kept-start"])).unwrap();
        assert_eq!(chat.directed.provider_continuation, Continuation::KeptStart);
        assert_eq!(
            SessionOptions::parse(&[])
                .unwrap()
                .directed
                .provider_continuation,
            Continuation::Off
        );
    }

    #[test]
    fn reference_recovery_is_explicit_and_needs_a_policy_that_masks_own_playback() {
        assert_eq!(
            SessionOptions::default().directed.reference_recovery,
            ReferenceRecovery::Off
        );
        // With either reference, under either masking policy.
        for (policy, clock) in [
            ("v1-baseline", "fixed"),
            ("v1-baseline", "capture"),
            ("duplex", "fixed"),
            ("duplex", "capture"),
        ] {
            let options = DirectedOptions::parse(&args(&[
                "--turn-taking",
                policy,
                "--reference-clock",
                clock,
                "--reference-recovery",
                "bounded",
            ]))
            .unwrap();
            assert_eq!(options.reference_recovery, ReferenceRecovery::Bounded);
        }
        assert_eq!(
            serde_json::to_value(ReferenceRecovery::Bounded).unwrap(),
            "bounded"
        );
        for values in [
            // `vad-only`, named or by default, masks nothing.
            vec!["--reference-recovery", "bounded"],
            vec![
                "--turn-taking",
                "vad-only",
                "--reference-recovery",
                "bounded",
            ],
            vec!["--turn-taking", "v1-baseline", "--reference-recovery"],
            vec!["--turn-taking", "v1-baseline", "--reference-recovery", "on"],
            vec![
                "--turn-taking",
                "v1-baseline",
                "--reference-recovery",
                "off",
                "--reference-recovery",
                "bounded",
            ],
        ] {
            assert!(
                DirectedOptions::parse(&args(&values)).is_err(),
                "{values:?}"
            );
        }
        // Off, said aloud, is the default and needs nothing.
        assert_eq!(
            DirectedOptions::parse(&args(&["--reference-recovery", "off"])).unwrap(),
            DirectedOptions::default()
        );
    }

    /// #1047: the stop words' raw blocks are asked of the capture worker
    /// only, by one flag that is never in the default argv.
    #[test]
    fn raw_blocks_reach_only_the_capture_worker_and_only_when_asked() {
        let plain = DirectedOptions::default()
            .capture_arguments(None, false)
            .unwrap();
        assert!(!plain.iter().any(|argument| argument == "--raw-blocks"));
        let raw = DirectedOptions::default()
            .capture_arguments(None, true)
            .unwrap();
        assert_eq!(
            raw.iter()
                .filter(|argument| *argument == "--raw-blocks")
                .count(),
            1
        );
        let parsed = AudioWorkerOptions::parse(&raw[1..], true).unwrap();
        assert!(parsed.raw_blocks);
        assert!(
            !AudioWorkerOptions::parse(&plain[1..], true)
                .unwrap()
                .raw_blocks
        );
        let flag = vec!["--raw-blocks".to_owned()];
        assert!(AudioWorkerOptions::parse(&flag, false).is_err());
        assert!(AudioWorkerOptions::parse(&[flag.clone(), flag].concat(), true).is_err());
        let render = AudioWorkerOptions {
            raw_blocks: true,
            ..AudioWorkerOptions::default()
        };
        assert!(render.child_arguments("plug:speaker", false).is_err());
    }

    /// The steady echo reference is a value of its own, reaches only the
    /// capture worker, and is never sent unless chosen.
    #[test]
    fn a_steady_echo_reference_is_explicit_and_reaches_only_the_capture_worker() {
        assert_eq!(
            SessionOptions::default().directed.echo_reference,
            EchoReference::AsGiven
        );
        let chosen = DirectedOptions::parse(&args(&["--echo-reference", "steady"])).unwrap();
        assert_eq!(chosen.echo_reference, EchoReference::Steady);
        assert_eq!(
            chosen.echo_reference.reference(),
            lamp_audio::Reference::Steady
        );
        assert_eq!(
            serde_json::to_value(EchoReference::Steady).unwrap(),
            "steady"
        );
        let capture = chosen.capture_arguments(None, false).unwrap();
        assert!(
            capture
                .windows(2)
                .any(|pair| pair == ["--echo-reference", "steady"]),
            "{capture:?}"
        );
        let parsed = AudioWorkerOptions::parse(&capture[1..], true).unwrap();
        assert_eq!(parsed.echo_reference, EchoReference::Steady);
        // The default argv has no such flag, and a worker told "as-given"
        // was started by nobody this program knows.
        let plain = DirectedOptions::default()
            .capture_arguments(None, false)
            .unwrap();
        assert!(!plain.iter().any(|argument| argument == "--echo-reference"));
        for wrong in [
            vec!["--echo-reference"],
            vec!["--echo-reference", "Steady"],
            vec!["--echo-reference", "steady", "--echo-reference", "steady"],
        ] {
            assert!(DirectedOptions::parse(&args(&wrong)).is_err(), "{wrong:?}");
        }
        let to_owned = |values: &[&str]| values.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>();
        assert!(
            AudioWorkerOptions::parse(&to_owned(&["--echo-reference", "as-given"]), true).is_err()
        );
        assert!(
            AudioWorkerOptions::parse(&to_owned(&["--echo-reference", "steady"]), false).is_err()
        );
        // The speaker's worker is never given it.
        let render = AudioWorkerOptions {
            echo_reference: EchoReference::Steady,
            noise_suppression: NoiseSuppression::On,
            ..AudioWorkerOptions::default()
        };
        assert!(render.child_arguments("plug:speaker", false).is_err());
    }

    #[test]
    fn reference_recovery_reaches_only_the_capture_worker_and_only_when_chosen() {
        for clock in [ReferenceClock::Fixed, ReferenceClock::Capture] {
            let chosen = AudioWorkerOptions {
                reference_clock: clock,
                reference_recovery: ReferenceRecovery::Bounded,
                echo_reference: EchoReference::AsGiven,
                ..AudioWorkerOptions::default()
            };
            let arguments = chosen.child_arguments("plug:device_micro2", true).unwrap();
            assert_eq!(
                arguments[arguments.len() - 2..],
                ["--reference-recovery", "bounded"]
            );
            assert_eq!(
                AudioWorkerOptions::parse(&arguments[1..], true).unwrap(),
                chosen
            );
            assert!(
                chosen
                    .child_arguments("plug:device_speaker", false)
                    .is_err()
            );
        }
        // The default argv has no such flag, and `off` is never sent.
        assert!(
            !AudioWorkerOptions::default()
                .child_arguments("plug:device_micro2", true)
                .unwrap()
                .iter()
                .any(|argument| argument.contains("recovery"))
        );
        for values in [
            vec!["--reference-recovery"],
            vec!["--reference-recovery", "off"],
            vec![
                "--reference-recovery",
                "bounded",
                "--reference-recovery",
                "bounded",
            ],
        ] {
            assert!(AudioWorkerOptions::parse(&args(&values), true).is_err());
        }
        assert!(
            AudioWorkerOptions::parse(&args(&["--reference-recovery", "bounded"]), false).is_err()
        );
    }

    #[test]
    fn turn_taking_is_explicit_and_default_remains_vad_only() {
        assert_eq!(
            SessionOptions::default().directed.turn_taking,
            TurnTaking::VadOnly
        );
        let options = SessionOptions::parse(&args(&[
            "--turn-taking",
            "v1-baseline",
            "--noise-suppression",
            "off",
            "--cue-socket",
            "/tmp/test-only.sock",
        ]))
        .unwrap();
        assert_eq!(options.directed.turn_taking, TurnTaking::V1Baseline);
        assert_eq!(options.directed.noise_suppression, NoiseSuppression::Off);
        assert!(!options.directed.diagnostics);
        assert_eq!(options.directed.ring_channel_ceiling, None);
        assert_eq!(
            serde_json::to_value(options.directed.turn_taking).unwrap(),
            "v1-baseline"
        );
        for values in [
            vec!["--turn-taking"],
            vec!["--turn-taking", "unknown"],
            vec!["--turn-taking", "V1-baseline"],
            vec!["--turn-taking", "vad-only", "--turn-taking", "v1-baseline"],
        ] {
            assert!(SessionOptions::parse(&args(&values)).is_err());
        }
    }
    #[test]
    fn default_is_the_existing_aec_plus_noise_suppression_pipeline() {
        let options = DirectedOptions::parse(&[]).unwrap();
        assert_eq!(options, DirectedOptions::default());
        assert!(options.noise_suppression.enabled());
        assert!(!options.diagnostics);
        assert_eq!(options.ring_channel_ceiling, None);
        assert_eq!(
            serde_json::to_value(options.noise_suppression.software_processing()).unwrap(),
            serde_json::json!({"aec":"sonora_aec3","noise_suppression":true})
        );
    }
    #[test]
    fn session_cues_are_optional_metadata_without_enabling_audio_recording() {
        assert_eq!(
            SessionOptions::parse(&[]).unwrap(),
            SessionOptions::default()
        );
        let options = SessionOptions::parse(&args(&[
            "--cue-socket",
            "/tmp/private session/cue.sock",
            "--noise-suppression",
            "off",
            "--ring-channel-ceiling",
            "24",
        ]))
        .unwrap();
        assert_eq!(
            options.cue_socket,
            Some(PathBuf::from("/tmp/private session/cue.sock"))
        );
        assert_eq!(options.directed.noise_suppression, NoiseSuppression::Off);
        assert_eq!(options.directed.ring_channel_ceiling, Some(24));
        assert!(!options.directed.diagnostics);
    }
    #[test]
    fn session_cues_reject_missing_duplicate_relative_and_flag_values() {
        for values in [
            vec!["--cue-socket"],
            vec!["--cue-socket", ""],
            vec!["--cue-socket", "relative.sock"],
            vec!["--cue-socket", "--diagnostics"],
            vec!["--cue-socket", "/tmp/a", "--cue-socket", "/tmp/b"],
            vec!["--cue-socket", "/tmp/a", "--unknown"],
            vec!["--cue-socket", "/tmp/a", "--noise-suppression", "yes"],
        ] {
            assert!(SessionOptions::parse(&args(&values)).is_err(), "{values:?}");
        }
    }
    #[test]
    fn ring_is_explicit_bounded_and_independent_of_audio_treatment() {
        for ceiling in [0, 24, 120] {
            let options = DirectedOptions::parse(&args(&[
                "--ring-channel-ceiling",
                &ceiling.to_string(),
                "--noise-suppression",
                "off",
                "--diagnostics",
            ]))
            .unwrap();
            assert_eq!(options.ring_channel_ceiling, Some(ceiling));
            assert_eq!(options.noise_suppression, NoiseSuppression::Off);
            assert!(options.diagnostics);
        }
        for flags in [
            vec!["--ring-channel-ceiling"],
            vec!["--ring-channel-ceiling", "121"],
            vec!["--ring-channel-ceiling", "65536"],
            vec!["--ring-channel-ceiling", "-1"],
            vec![
                "--ring-channel-ceiling",
                "24",
                "--ring-channel-ceiling",
                "24",
            ],
            vec!["--ring-channel-ceiling", "--diagnostics"],
        ] {
            assert!(DirectedOptions::parse(&args(&flags)).is_err());
        }
    }
    #[test]
    fn both_modes_are_orthogonal_to_diagnostics_in_either_order() {
        for mode in [NoiseSuppression::On, NoiseSuppression::Off] {
            for flags in [
                vec!["--noise-suppression", mode.as_str(), "--diagnostics"],
                vec!["--diagnostics", "--noise-suppression", mode.as_str()],
            ] {
                assert_eq!(
                    DirectedOptions::parse(&args(&flags)).unwrap(),
                    DirectedOptions {
                        end_silence_blocks: None,
                        early_end_blocks: None,
                        endpoint_commit_blocks: None,
                        call_wait_blocks: None,
                        faint_level: None,
                        microphone: Microphone::Mic2,
                        playout_start: PlayoutStart::default(),
                        turn_taking: TurnTaking::VadOnly,
                        reference_clock: ReferenceClock::Fixed,
                        reference_recovery: ReferenceRecovery::Off,
                        echo_reference: EchoReference::AsGiven,
                        provider_continuation: Continuation::Off,
                        endpoint: Endpoint::Silence,
                        diagnostics: true,
                        noise_suppression: mode,
                        ring_channel_ceiling: None,
                    }
                );
            }
            let only =
                DirectedOptions::parse(&args(&["--noise-suppression", mode.as_str()])).unwrap();
            assert!(!only.diagnostics);
            assert_eq!(only.noise_suppression, mode);
            assert_eq!(
                serde_json::to_value(mode.software_processing()).unwrap(),
                serde_json::json!({"aec":"sonora_aec3","noise_suppression":mode.enabled()})
            );
        }
    }
    #[test]
    fn invalid_or_ambiguous_flags_fail_before_any_startup_work() {
        for flags in [
            vec!["--noise-suppression"],
            vec!["--noise-suppression", "false"],
            vec!["--noise-suppression", "OFF"],
            vec!["--noise-suppression", "--diagnostics"],
            vec!["--noise-suppression", "off", "--noise-suppression", "on"],
            vec!["--diagnostics", "--diagnostics"],
            vec!["--aec", "off"],
            vec!["off"],
            vec!["/tmp/audio"],
            vec!["--diagnostics", "/tmp/audio"],
        ] {
            assert!(DirectedOptions::parse(&args(&flags)).is_err(), "{flags:?}");
        }
    }
    #[test]
    fn child_options_are_explicit_and_render_cannot_claim_capture_processing() {
        for mode in [NoiseSuppression::On, NoiseSuppression::Off] {
            let parsed = AudioWorkerOptions::parse(
                &args(&[
                    "--noise-suppression",
                    mode.as_str(),
                    "--diagnostics",
                    "/tmp/new-capture",
                ]),
                true,
            )
            .unwrap();
            assert_eq!(parsed.noise_suppression, mode);
            assert_eq!(
                parsed.diagnostic_directory,
                Some(PathBuf::from("/tmp/new-capture"))
            );
        }
        assert_eq!(
            AudioWorkerOptions::parse(&[], true)
                .unwrap()
                .noise_suppression,
            NoiseSuppression::On
        );
        assert!(AudioWorkerOptions::parse(&args(&["--noise-suppression", "off"]), false).is_err());
        for flags in [
            vec!["off"],
            vec!["/tmp/new-capture"],
            vec!["--diagnostics"],
            vec!["--diagnostics", "relative"],
            vec!["--diagnostics", "--noise-suppression", "off"],
            vec!["--diagnostics", "/tmp/a", "--diagnostics", "/tmp/b"],
        ] {
            assert!(
                AudioWorkerOptions::parse(&args(&flags), true).is_err(),
                "{flags:?}"
            );
        }
    }

    #[test]
    fn supervisor_argv_preserves_requested_capture_mode_and_separate_render_path() {
        for mode in [NoiseSuppression::On, NoiseSuppression::Off] {
            for directory in [None, Some(PathBuf::from("/tmp/private run/capture-audio"))] {
                let requested = AudioWorkerOptions {
                    diagnostic_directory: directory,
                    noise_suppression: mode,
                    reference_clock: ReferenceClock::Fixed,
                    reference_recovery: ReferenceRecovery::Off,
                    echo_reference: EchoReference::AsGiven,
                    raw_blocks: false,
                };
                let arguments = requested
                    .child_arguments("plug:device_micro2", true)
                    .unwrap();
                assert_eq!(arguments[0], "plug:device_micro2");
                let actual = AudioWorkerOptions::parse(&arguments[1..], true).unwrap();
                assert_eq!(actual, requested);
            }
        }
        let render = AudioWorkerOptions {
            diagnostic_directory: Some(PathBuf::from("/tmp/private run/render-audio")),
            ..AudioWorkerOptions::default()
        };
        let arguments = render
            .child_arguments("plug:device_speaker", false)
            .unwrap();
        assert_eq!(
            arguments,
            args(&[
                "plug:device_speaker",
                "--diagnostics",
                "/tmp/private run/render-audio"
            ])
        );
        assert_eq!(
            AudioWorkerOptions::parse(&arguments[1..], false).unwrap(),
            render
        );
    }
}

#[cfg(test)]
mod notice_tests {
    use super::*;
    fn parse(args: &[&str]) -> io::Result<SessionOptions> {
        SessionOptions::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }
    #[test]
    fn failure_notice_is_explicit_paired_bounded_and_not_forwarded_to_workers() {
        assert!(SessionOptions::default().failure_notices.is_none());
        let options = parse(&[
            "--failure-notice-dir",
            "/tmp/local notices",
            "--failure-notice-language",
            "en-US",
        ])
        .unwrap();
        let notices = options.failure_notices.unwrap();
        assert_eq!(
            notices.directory(),
            std::path::Path::new("/tmp/local notices")
        );
        assert_eq!(notices.language(), "en-US");
        for args in [
            vec!["--failure-notice-dir", "/tmp/local"],
            vec!["--failure-notice-language", "en-US"],
            vec![
                "--failure-notice-dir",
                "relative",
                "--failure-notice-language",
                "en-US",
            ],
            vec![
                "--failure-notice-dir",
                "/tmp/local",
                "--failure-notice-language",
                "en--US",
            ],
            vec![
                "--failure-notice-dir",
                "/tmp/local",
                "--failure-notice-language",
                "en-US",
                "--failure-notice-language",
                "en-US",
            ],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
        assert!(NoticeOptions::new("/tmp/local".into(), "a".repeat(36)).is_err());
        let args = [
            "--failure-notice-dir",
            "/tmp/local",
            "--failure-notice-language",
            "en-US",
        ]
        .map(String::from);
        assert!(AudioWorkerOptions::parse(&args, true).is_err());
        assert!(crate::fixture_provider::FixtureOptions::parse(&args).is_err());
    }
    #[test]
    fn head_qualification_options_are_paired_explicit_and_finite() {
        let args = [
            "--motor-worker",
            "/tmp/motor",
            "--motor-config",
            "/tmp/config",
            "--camera-config",
            "/tmp/camera",
            "--head-qualification-plan",
            "/tmp/plan",
            "--head-qualification-channel",
            "/tmp/channel",
        ]
        .map(String::from);
        let s = SessionOptions::parse(&args).unwrap();
        assert!(s.motor.unwrap().head.is_some());
        assert!(
            SessionOptions::parse(&args)
                .unwrap()
                .validate_serve()
                .is_err()
        );
        assert!(SessionOptions::parse(&args[..8]).is_err());
        assert!(SessionOptions::parse(&args[4..]).is_err());
        let plain = SessionOptions::parse(&[]).unwrap();
        assert!(plain.motor.is_none());
        assert!(plain.validate_serve().is_ok());
    }
}

#[cfg(test)]
mod end_silence_tests {
    use super::*;

    fn parse(args: &[&str]) -> io::Result<DirectedOptions> {
        DirectedOptions::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn the_end_of_speech_wait_is_explicit_bounded_and_given_once() {
        assert_eq!(parse(&[]).unwrap().end_silence_blocks, None);
        assert_eq!(
            parse(&["--end-silence-ms", "700"])
                .unwrap()
                .end_silence_blocks,
            Some(70)
        );
        assert_eq!(
            parse(&["--end-silence-ms", "300", "--turn-taking", "duplex"])
                .unwrap()
                .end_silence_blocks,
            Some(30)
        );
        let refused: [&[&str]; 6] = [
            &["--end-silence-ms"],
            &["--end-silence-ms", "290"],
            &["--end-silence-ms", "2010"],
            &["--end-silence-ms", "705"],
            &["--end-silence-ms", "soon"],
            &["--end-silence-ms", "700", "--end-silence-ms", "700"],
        ];
        for arguments in refused {
            assert!(parse(arguments).is_err(), "{arguments:?}");
        }
    }
}

#[cfg(test)]
mod early_end_tests {
    use super::*;

    fn parse(args: &[&str]) -> io::Result<DirectedOptions> {
        DirectedOptions::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    /// What lamp-4ace's unit passes today (`deploy/lamp-4ace/lamp-voice.service`),
    /// less the provider configuration and the output directory.
    const LAMP: [&str; 18] = [
        "--noise-suppression",
        "off",
        "--turn-taking",
        "duplex",
        "--faint-floor-db",
        "-28",
        "--microphone",
        "mic2",
        "--reference-clock",
        "capture",
        "--reference-recovery",
        "off",
        "--provider-continuation",
        "kept-start",
        "--playout-start-ms",
        "350",
        "--end-silence-ms",
        "600",
    ];

    #[test]
    fn without_the_option_nothing_is_sent_early_and_the_lamps_flags_parse_as_before() {
        assert_eq!(parse(&[]).unwrap().early_end_blocks, None);
        let lamp = parse(&LAMP).unwrap();
        assert_eq!(lamp.early_end_blocks, None);
        // Everything else the lamp sets is what it was.
        assert_eq!(
            DirectedOptions {
                early_end_blocks: None,
                ..lamp
            },
            lamp
        );
        assert_eq!(lamp.end_silence_window_blocks(), 60);
        // The option belongs to the coordinator alone: the capture worker
        // is started with the same arguments either way.
        let mut on = LAMP.to_vec();
        on.extend(["--early-end-ms", "200"]);
        let on = parse(&on).unwrap();
        assert_eq!(on.early_end_blocks, Some(20));
        assert_eq!(
            DirectedOptions {
                early_end_blocks: None,
                ..on
            },
            lamp
        );
        assert_eq!(
            on.capture_arguments(None, false).unwrap(),
            lamp.capture_arguments(None, false).unwrap()
        );
    }

    #[test]
    fn the_early_end_is_explicit_bounded_shorter_than_the_wait_and_needs_the_kept_start() {
        let kept = ["--provider-continuation", "kept-start"];
        let with = |extra: &[&str]| {
            let mut all = kept.to_vec();
            all.extend(extra);
            parse(&all)
        };
        assert_eq!(
            with(&["--early-end-ms", "200"]).unwrap().early_end_blocks,
            Some(20)
        );
        // `vad-only` waits 600 ms by itself, the baseline and duplex 1,200.
        assert_eq!(
            with(&["--early-end-ms", "590"]).unwrap().early_end_blocks,
            Some(59)
        );
        assert_eq!(
            with(&["--turn-taking", "duplex", "--early-end-ms", "1000"])
                .unwrap()
                .early_end_blocks,
            Some(100)
        );
        assert_eq!(
            with(&["--early-end-ms", "100", "--end-silence-ms", "300"])
                .unwrap()
                .early_end_blocks,
            Some(10)
        );
        let refused: [&[&str]; 10] = [
            &["--early-end-ms"],
            &["--early-end-ms", "90"],
            &["--early-end-ms", "1010"],
            &["--early-end-ms", "205"],
            &["--early-end-ms", "soon"],
            &["--early-end-ms", "200", "--early-end-ms", "200"],
            // Not shorter than the wait that confirms it.
            &["--early-end-ms", "600"],
            &["--early-end-ms", "300", "--end-silence-ms", "300"],
            &[
                "--turn-taking",
                "duplex",
                "--end-silence-ms",
                "600",
                "--early-end-ms",
                "700",
            ],
            &[
                "--early-end-ms",
                "1000",
                "--turn-taking",
                "v1-baseline",
                "--end-silence-ms",
                "1000",
            ],
        ];
        for arguments in refused {
            assert!(with(arguments).is_err(), "{arguments:?}");
        }
        // And never without the kept start.
        assert!(parse(&["--early-end-ms", "200"]).is_err());
        assert!(parse(&["--early-end-ms", "200", "--provider-continuation", "off"]).is_err());
    }
}

#[cfg(test)]
mod faint_floor_tests {
    use super::*;

    fn parse(args: &[&str]) -> io::Result<DirectedOptions> {
        DirectedOptions::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn the_floor_for_speech_is_explicit_bounded_and_given_once() {
        assert_eq!(parse(&[]).unwrap().faint_level, None);
        // -40 dBFS is 60 on the face's scale, which runs from -48 to -14.
        assert_eq!(
            parse(&["--faint-floor-db", "-40"]).unwrap().faint_level,
            Some(60)
        );
        assert_eq!(
            parse(&["--faint-floor-db", "-20", "--turn-taking", "duplex"])
                .unwrap()
                .faint_level,
            Some(210)
        );
        assert_eq!(
            parse(&["--faint-floor-db", "-47"]).unwrap().faint_level,
            Some(7)
        );
        let refused: [&[&str]; 6] = [
            &["--faint-floor-db"],
            &["--faint-floor-db", "-48"],
            &["--faint-floor-db", "-19"],
            &["--faint-floor-db", "40"],
            &["--faint-floor-db", "quiet"],
            &["--faint-floor-db", "-40", "--faint-floor-db", "-40"],
        ];
        for arguments in refused {
            assert!(parse(arguments).is_err(), "{arguments:?}");
        }
    }
}
