use crate::{
    Error, INPUT_QUEUE_CAPACITY, MAX_HANDLE_BYTES, MAX_INPUT_QUEUE, MAX_INSTRUCTION_BYTES,
    MAX_TOOLS, OUTPUT_BUFFER_BYTES, OUTPUT_RATE, Result, Tool,
};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr, sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    handshake::client::Request,
    http::{HeaderName, HeaderValue, Uri},
};

pub const GOOGLE_ENDPOINT: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";

#[derive(Clone)]
pub struct Credential {
    name: HeaderName,
    value: HeaderValue,
}
impl Credential {
    pub fn api_key(secret: &str) -> Result<Self> {
        Self::new("x-goog-api-key", "", secret)
    }
    pub fn bearer(secret: &str) -> Result<Self> {
        Self::new("authorization", "Bearer ", secret)
    }
    pub fn ephemeral(secret: &str) -> Result<Self> {
        Self::new("authorization", "Token ", secret)
    }
    fn new(name: &'static str, prefix: &str, secret: &str) -> Result<Self> {
        if secret.is_empty() || secret.len() > 4096 || secret.trim() != secret || !secret.is_ascii()
        {
            return Err(Error::InvalidConfiguration);
        }
        let mut value = HeaderValue::from_str(&format!("{prefix}{secret}"))
            .map_err(|_| Error::InvalidConfiguration)?;
        value.set_sensitive(true);
        Ok(Self {
            name: HeaderName::from_static(name),
            value,
        })
    }
}
impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credential([REDACTED])")
    }
}

/// Every wait is finite. These are failure bounds, not latency measurements.
#[derive(Clone, Copy, Debug)]
pub struct Timeouts {
    pub connect: Duration,
    pub setup: Duration,
    pub write: Duration,
    /// Added to a write's bound for each KiB of the message, so a big
    /// message (the look's picture, a camera frame) has time to go out on a
    /// slow link. Zero by default: each write has `write`, a camera frame its
    /// own 400 ms. A block of audio never gets it: it keeps its own bound,
    /// which its age caps.
    pub write_per_kib: Duration,
    pub read: Duration,
    /// Longest silence of an earlier response, after it was asked to stop,
    /// before its idle completion. Only the next request's activityEnd waits
    /// for it. Audio is held only by the explicit `SuccessorStart` policy.
    pub barrier: Duration,
    /// Ended input to the first output of its response.
    pub response: Duration,
    /// Ended input to the moment a response that has produced nothing is
    /// taken as the service having chosen not to answer. The request then
    /// ends with [`crate::Event::Unanswered`] and the connection stays. A
    /// service told that it may stay silent sends nothing at all in that
    /// case, not an empty completion. `None`: only `response` bounds the
    /// wait, and reaching it fails the connection.
    pub unanswered: Option<Duration>,
    /// Longest silence between outputs before generation completes.
    pub stall: Duration,
    /// Allowance beyond the received audio's real-time duration for the idle
    /// completion. The service defers it while it assumes playback.
    pub completion: Duration,
    /// Ended input to idle completion, whatever progress is observed.
    pub turn: Duration,
    /// How long the consumer may accept no event at all while output waits.
    /// A consumer paced by real-time playback takes one event per event's
    /// worth of audio, so this must exceed the longest event (2 s) plus any
    /// pause in playback that should be survived.
    pub deliver: Duration,
    pub keepalive: Duration,
}
impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(10),
            setup: Duration::from_secs(10),
            write: Duration::from_millis(200),
            write_per_kib: Duration::ZERO,
            read: Duration::from_secs(60),
            barrier: Duration::from_secs(2),
            response: Duration::from_secs(30),
            unanswered: None,
            stall: Duration::from_secs(10),
            completion: Duration::from_secs(15),
            turn: Duration::from_secs(300),
            deliver: Duration::from_secs(30),
            keepalive: Duration::from_secs(15),
        }
    }
}
impl Timeouts {
    pub(crate) fn validate(self) -> Result<()> {
        for (duration, max) in [
            (self.connect, Duration::from_secs(30)),
            (self.setup, Duration::from_secs(30)),
            (self.write, Duration::from_secs(1)),
            (self.read, Duration::from_secs(300)),
            (self.barrier, Duration::from_secs(5)),
            (self.response, Duration::from_secs(300)),
            (self.stall, Duration::from_secs(60)),
            (self.completion, Duration::from_secs(60)),
            (self.turn, Duration::from_secs(600)),
            (self.deliver, Duration::from_secs(120)),
            (self.keepalive, Duration::from_secs(60)),
        ] {
            if duration < Duration::from_millis(1) || duration > max {
                return Err(Error::InvalidConfiguration);
            }
        }
        if self.keepalive >= self.read {
            return Err(Error::InvalidConfiguration);
        }
        if self.write_per_kib > Duration::from_millis(50) {
            return Err(Error::InvalidConfiguration);
        }
        // Not a failure bound: it has to end before the one for a response.
        if self
            .unanswered
            .is_some_and(|wait| wait < Duration::from_millis(1) || wait >= self.response)
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok(())
    }
}

/// Explicit provider thinking depth; absence retains the model's own default.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ThinkingLevel {
    Minimal,
    Low,
    Medium,
    High,
}
impl FromStr for ThinkingLevel {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "MINIMAL" => Ok(Self::Minimal),
            "LOW" => Ok(Self::Low),
            "MEDIUM" => Ok(Self::Medium),
            "HIGH" => Ok(Self::High),
            _ => Err(Error::InvalidConfiguration),
        }
    }
}

/// Whether the opaque server-issued session handle is kept for a reconnect.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Resumption {
    /// Parse and discard every handle. Setup is unchanged.
    #[default]
    Off,
    /// Keep the latest handle the service volunteers. Initial setup is
    /// unchanged; only a reconnect that presents a handle differs.
    Retain,
    /// Also ask for handles in the initial setup.
    Request,
}

/// What to do when the service, asked to interrupt a response, sends neither
/// further output nor an idle completion for it within `Timeouts::barrier`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BarrierPolicy {
    /// Fail the connection rather than guess which request owns later output.
    #[default]
    Require,
    /// Treat the silent response as over and let the next request proceed,
    /// reporting [`crate::Event::BarrierAssumed`]. Output of the earlier
    /// response that arrives before the next request is committed re-arms the
    /// barrier. Unverified against the live service.
    AssumeAfterQuiet,
}

/// The longest a next request's `activityStart` may be kept back.
pub const MAX_SUCCESSOR_PATIENCE: Duration = Duration::from_secs(4);
/// The most of a next request's audio kept back behind its `activityStart`:
/// six seconds. More than the longest patience, so that audio arriving at
/// the rate it is spoken cannot fill it first.
pub const MAX_RETAINED_SAMPLES: usize = 16_000 * 6;
/// The most requests in a row whose audio one open activity is accounted
/// for: requests that each sent audio into an activity that was neither
/// ended nor abandoned in between. A bound on what is remembered and
/// nothing else: the account lets a late abandonment report exactly its own
/// audio, and an entry leaves it only when the activity ends or when the
/// abandonment that reports it is applied. One more request than this fails
/// the connection with [`Error::OpenActivityOverrun`] before any of its
/// audio is sent, and that failure is not retried. Only where a start can be
/// kept at all; otherwise there is no account and no such bound.
pub const MAX_OPEN_ACTIVITY_REQUESTS: usize = 64;

/// What happens to the next request when it starts while a committed
/// request's response has produced nothing yet.
///
/// Interrupting a response before it has produced anything got no
/// confirmation in the one real-service run that tried it, and the
/// service's documentation defines none for that case. Interrupting one
/// that is producing was confirmed in the one run that tried that.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SuccessorStart {
    /// Its `activityStart` and its audio are written at once, as they have
    /// always been.
    #[default]
    AtOnce,
    /// Its `activityStart`, and its audio behind it, are kept here until
    /// the earlier response produces its first output, which is then
    /// interrupted, or completes idle, after which this request starts
    /// normally. Nothing of it is lost, reordered or sent twice.
    ///
    /// Kept for at most `patience`, and at most
    /// [`MAX_RETAINED_SAMPLES`]: past either, the start and the kept audio
    /// are written and the default behavior applies from there. Audio kept
    /// here is written up to `patience` later than
    /// [`crate::MAX_INPUT_AGE`] otherwise allows, with its own capture time;
    /// it is dropped unsent by a stop or any failure.
    AfterFirstOutput { patience: Duration },
}

/// From when a barrier's quiet bound, `Timeouts::barrier`, can end the
/// connection under [`BarrierPolicy::Require`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BarrierWait {
    /// From the moment the earlier response was asked to stop, whether or
    /// not the next request is still being spoken. As it has always been:
    /// a person who goes on for longer than the bound is cut off.
    #[default]
    FromInterruption,
    /// Only once the next request has ended its input and its `activityEnd`
    /// is being withheld. While its input is open nothing waits on the
    /// earlier response but the absolute bound, `Timeouts::response` from
    /// the interruption. The strict wait is the same; it starts where
    /// something is actually waiting.
    AtCommit,
}

/// Opaque server token for resuming conversation state on a new connection.
/// Held in memory only: no serialization, no value in Debug output.
#[derive(Clone)]
pub struct ResumptionHandle(Arc<str>);
impl ResumptionHandle {
    pub(crate) fn new(value: &str) -> Option<Self> {
        (!value.is_empty() && value.len() <= MAX_HANDLE_BYTES).then(|| Self(value.into()))
    }
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for ResumptionHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ResumptionHandle([PRIVATE])")
    }
}

#[derive(Clone)]
pub struct SessionConfig {
    pub(crate) endpoint: String,
    pub(crate) credential: Credential,
    pub(crate) model: String,
    pub(crate) voice: String,
    pub(crate) thinking_level: Option<ThinkingLevel>,
    pub(crate) thinking_budget: Option<u32>,
    pub(crate) context_window: Option<(u32, u32)>,
    pub(crate) language_code: Option<String>,
    pub(crate) transcription_language: Option<String>,
    pub(crate) small_pictures: bool,
    pub(crate) search: bool,
    pub(crate) tools: Vec<Tool>,
    pub(crate) instruction: String,
    pub(crate) timeouts: Timeouts,
    pub(crate) resumption: Resumption,
    pub(crate) resume: Option<ResumptionHandle>,
    pub(crate) barrier: BarrierPolicy,
    pub(crate) barrier_wait: BarrierWait,
    pub(crate) barrier_reports: bool,
    pub(crate) successor_start: SuccessorStart,
    pub(crate) output_buffer: usize,
    /// Commands the input queue holds ([`Self::writes`]).
    pub(crate) input_queue: usize,
    /// A write that held the session owes a read before any quiet bound is
    /// judged ([`Self::writes`]). Off by default: the read loop is as
    /// before (the reviewer on lampOS#1397).
    pub(crate) read_after_long_writes: bool,
}
impl SessionConfig {
    /// Custom proxies are explicit WSS endpoints. Authentication belongs only in
    /// a private header; userinfo, queries and fragments are rejected entirely.
    pub fn new(endpoint: &str, credential: Credential) -> Result<Self> {
        validate_endpoint(endpoint)?;
        Ok(Self {
            endpoint: endpoint.into(),
            credential,
            model: "gemini-3.8-live".into(),
            voice: "Kore".into(),
            thinking_level: None,
            thinking_budget: None,
            context_window: None,
            language_code: None,
            transcription_language: None,
            small_pictures: false,
            search: false,
            tools: Vec::new(),
            instruction: String::new(),
            timeouts: Timeouts::default(),
            resumption: Resumption::Off,
            resume: None,
            barrier: BarrierPolicy::Require,
            barrier_wait: BarrierWait::FromInterruption,
            barrier_reports: false,
            successor_start: SuccessorStart::AtOnce,
            output_buffer: OUTPUT_BUFFER_BYTES,
            input_queue: INPUT_QUEUE_CAPACITY,
            read_after_long_writes: false,
        })
    }
    pub fn google(credential: Credential) -> Result<Self> {
        Self::new(GOOGLE_ENDPOINT, credential)
    }
    pub fn model(mut self, model: &str) -> Result<Self> {
        if !identifier(model, 128) {
            return Err(Error::InvalidConfiguration);
        }
        validate_thinking_level(model, self.thinking_level)?;
        self.model = model.into();
        Ok(self)
    }
    pub fn voice(mut self, voice: &str) -> Result<Self> {
        if !identifier(voice, 64) {
            return Err(Error::InvalidConfiguration);
        }
        self.voice = voice.into();
        Ok(self)
    }
    /// Select the model first. Known incompatible explicit requests fail;
    /// unknown proxy model capabilities remain the provider's responsibility.
    pub fn thinking_level(mut self, level: ThinkingLevel) -> Result<Self> {
        validate_thinking_level(&self.model, Some(level))?;
        if self.thinking_budget.is_some() {
            return Err(Error::InvalidConfiguration);
        }
        self.thinking_level = Some(level);
        Ok(self)
    }
    /// The most tokens the model may spend thinking before it answers; zero
    /// asks for none. Sent as `thinkingConfig.thinkingBudget`, and never
    /// together with a thinking level. `gemini-3.8-live` refuses a level and
    /// accepted a budget of zero on lamp-4ace on 2026-10-12: five answers
    /// then reported no thinking tokens, against 48 to 387 without it.
    pub fn thinking_budget(mut self, tokens: u32) -> Result<Self> {
        if self.thinking_level.is_some() || tokens > MAX_THINKING_BUDGET {
            return Err(Error::InvalidConfiguration);
        }
        self.thinking_budget = Some(tokens);
        Ok(self)
    }
    /// Bound the conversation the service keeps, and bills again on every
    /// turn: once it holds more than `trigger` tokens the service drops the
    /// oldest turns down to `target`. Sent as `contextWindowCompression` with
    /// a sliding window. Without it a session's history grows until the
    /// session ends: on lamp-4ace the prompt was 1,327 tokens on the first
    /// turn of a session and 4,234 on the fourteenth.
    pub fn context_window(mut self, trigger: u32, target: u32) -> Result<Self> {
        if target == 0 || target >= trigger || trigger > MAX_CONTEXT_TOKENS {
            return Err(Error::InvalidConfiguration);
        }
        self.context_window = Some((trigger, target));
        Ok(self)
    }
    /// Speech language tag, preserved exactly rather than inferred or expanded.
    /// This is not an input-transcription language-hints setting.
    pub fn language_code(mut self, language_code: &str) -> Result<Self> {
        if !valid_language_code(language_code) {
            return Err(Error::InvalidConfiguration);
        }
        self.language_code = Some(language_code.into());
        Ok(self)
    }
    /// A language hint for the service's transcript of what it hears, sent
    /// as `inputAudioTranscription.languageCodes`: set by the voice with
    /// `LAMP_TRANSCRIPTION_LANGUAGE`, and by the transcript probe (#926:
    /// English said with a Vietnamese accent transcribed in Devanagari).
    pub fn transcription_language(mut self, language_code: &str) -> Result<Self> {
        if !valid_language_code(language_code) {
            return Err(Error::InvalidConfiguration);
        }
        self.transcription_language = Some(language_code.into());
        Ok(self)
    }
    /// Ask the service to read each picture at its low resolution, which
    /// costs about a quarter of the tokens. For a camera that gives small
    /// frames anyway.
    pub fn small_pictures(mut self, small: bool) -> Self {
        self.small_pictures = small;
        self
    }
    /// Let the model look things up with the service's own web search when
    /// a question needs it (today's weather, a price, the news). The search
    /// runs at the service; nothing is called on this side.
    pub fn search(mut self, search: bool) -> Self {
        self.search = search;
        self
    }
    /// Functions the model may ask this side to run ([`Tool`]). None by
    /// default. Each is declared to the service at setup; a call arrives as
    /// [`crate::Event::ToolCall`].
    pub fn tools(mut self, tools: Vec<Tool>) -> Result<Self> {
        if tools.len() > MAX_TOOLS {
            return Err(Error::InvalidConfiguration);
        }
        for (index, tool) in tools.iter().enumerate() {
            tool.validate()?;
            if tools[..index]
                .iter()
                .any(|earlier| earlier.name == tool.name)
            {
                return Err(Error::InvalidConfiguration);
            }
        }
        self.tools = tools;
        Ok(self)
    }
    /// The instruction as it will be sent.
    pub fn instruction_text(&self) -> &str {
        &self.instruction
    }
    pub fn instruction(mut self, instruction: &str) -> Result<Self> {
        if instruction.len() > MAX_INSTRUCTION_BYTES || instruction.contains('\0') {
            return Err(Error::InvalidConfiguration);
        }
        self.instruction = instruction.into();
        Ok(self)
    }
    pub fn timeouts(mut self, timeouts: Timeouts) -> Result<Self> {
        timeouts.validate()?;
        self.timeouts = timeouts;
        Ok(self)
    }
    pub fn resumption(mut self, resumption: Resumption) -> Self {
        self.resumption = resumption;
        self
    }
    /// Present a retained handle in setup. A handle is only ever obtained from
    /// a connection whose configuration allowed retention.
    pub fn resume(mut self, handle: Option<ResumptionHandle>) -> Self {
        self.resume = handle;
        self
    }
    pub fn barrier_policy(mut self, policy: BarrierPolicy) -> Self {
        self.barrier = policy;
        self
    }
    pub fn barrier_wait(mut self, wait: BarrierWait) -> Self {
        self.barrier_wait = wait;
        self
    }
    /// Report how each barrier ended as [`crate::Event::Barrier`]. Off by
    /// default, so that a caller's event stream is what it was.
    pub fn barrier_reports(mut self, reports: bool) -> Self {
        self.barrier_reports = reports;
        self
    }
    /// When the next request's `activityStart` is written if it starts
    /// while a committed request's response has produced nothing yet.
    pub fn successor_start(mut self, start: SuccessorStart) -> Result<Self> {
        if let SuccessorStart::AfterFirstOutput { patience } = start
            && (patience < Duration::from_millis(1) || patience > MAX_SUCCESSOR_PATIENCE)
        {
            return Err(Error::InvalidConfiguration);
        }
        self.successor_start = start;
        Ok(self)
    }
    /// Seconds of generated audio held for a consumer slower than the network
    /// before the socket goes unread. The default holds 300 s.
    pub fn output_buffer_seconds(mut self, seconds: u32) -> Result<Self> {
        if !(1..=600).contains(&seconds) {
            return Err(Error::InvalidConfiguration);
        }
        self.output_buffer = seconds as usize * 2 * OUTPUT_RATE as usize;
        Ok(self)
    }
    /// How long a write to the service may wait, and how much input waits
    /// behind it. `write` is every write's bound, `per_kib` is added for each
    /// KiB of the message ([`Timeouts::write_per_kib`]), and
    /// `input_queue` is how many commands the input queue holds. A block of
    /// audio is one command of 20 ms, and a write that waits takes no input
    /// meanwhile. So the queue bounds how long a write may wait while someone
    /// speaks before the connection ends as [`Error::Backpressure`]:
    /// [`INPUT_QUEUE_CAPACITY`] is 640 ms, and [`MAX_INPUT_QUEUE`] reaches the
    /// audio's own [`crate::MAX_INPUT_AGE`] (lampOS#705).
    ///
    /// It also makes a write that held the session 100 ms or more owe a read:
    /// what the service sent meanwhile is read before the barrier's quiet or a
    /// response bound is judged. Without it the read loop is as before.
    pub fn writes(
        mut self,
        write: Duration,
        per_kib: Duration,
        input_queue: usize,
    ) -> Result<Self> {
        if !(INPUT_QUEUE_CAPACITY..=MAX_INPUT_QUEUE).contains(&input_queue) {
            return Err(Error::InvalidConfiguration);
        }
        let timeouts = Timeouts {
            write,
            write_per_kib: per_kib,
            ..self.timeouts
        };
        timeouts.validate()?;
        self.timeouts = timeouts;
        self.input_queue = input_queue;
        self.read_after_long_writes = true;
        Ok(self)
    }
    pub(crate) fn request(&self) -> Result<Request> {
        let mut request = self
            .endpoint
            .as_str()
            .into_client_request()
            .map_err(|_| Error::InvalidConfiguration)?;
        request
            .headers_mut()
            .insert(self.credential.name.clone(), self.credential.value.clone());
        Ok(request)
    }
}
impl fmt::Debug for SessionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionConfig([PRIVATE])")
    }
}
/// A bound on what is accepted here, not a limit of the service.
pub const MAX_THINKING_BUDGET: u32 = 32_768;
/// The same, for the conversation the service is asked to keep.
pub const MAX_CONTEXT_TOKENS: u32 = 1_048_576;

fn validate_thinking_level(model: &str, level: Option<ThinkingLevel>) -> Result<()> {
    // These exact combinations are evidenced by the pinned V1 setup. Do not
    // infer capabilities of unknown aliases from substrings or clamp silently.
    if level.is_some() && model == "gemini-3.8-live"
        || level == Some(ThinkingLevel::Minimal) && model == "gemini-3.8-live-extended-thinking"
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}
/// Whether `value` is a language tag this crate takes: the voice checks a
/// switch's value with it before passing it on, so the two never differ.
pub fn valid_language_code(value: &str) -> bool {
    // Bounded language/script/region/variant form, not a registry allowlist or
    // full BCP-47 extension/private-use parser. The provider checks support.
    if value.len() > 63 {
        return false;
    }
    let mut subtags = value.split('-');
    subtags.next().is_some_and(|language| {
        (2..=8).contains(&language.len()) && language.bytes().all(|b| b.is_ascii_alphabetic())
    }) && subtags.all(|subtag| {
        (2..=8).contains(&subtag.len()) && subtag.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}
fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|x| x.is_ascii_alphanumeric() || b"._-".contains(&x))
}
fn validate_endpoint(value: &str) -> Result<()> {
    if value.len() > 2048 || value.contains(['?', '#', '@']) {
        return Err(Error::InvalidConfiguration);
    }
    let uri: Uri = value.parse().map_err(|_| Error::InvalidConfiguration)?;
    if uri.scheme_str() != Some("wss")
        || uri.host().is_none_or(str::is_empty)
        || uri.authority().is_none()
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authentication_is_sensitive_header_only() {
        let config =
            SessionConfig::google(Credential::api_key("private-only-test-key").unwrap()).unwrap();
        let request = config.request().unwrap();
        assert!(request.headers()["x-goog-api-key"].is_sensitive());
        assert!(!request.uri().to_string().contains("private-only-test-key"));
        assert!(!format!("{request:?}").contains("private-only-test-key"));
        assert_eq!(request.headers()["x-goog-api-key"], "private-only-test-key");
    }
}
