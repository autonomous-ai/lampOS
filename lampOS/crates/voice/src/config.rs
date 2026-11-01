//! Explicit private configuration. Neither the HAL nor os-server configuration
//! is read by this runtime. Provisioning may migrate values once outside source.
use lamp_gemini::{
    BarrierPolicy, Credential, RecoveryPolicy, Resumption, SessionConfig, ThinkingLevel, Timeouts,
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{self, Read},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

/// Not `deny_unknown_fields`: a field a newer build writes is read past and
/// named once at start ([`unread_fields`]), so a roll back never stops the
/// voice (#1505, #1680). A known field with a bad value is still refused.
#[derive(Deserialize)]
pub struct ProviderConfig {
    endpoint: String,
    model: String,
    voice: String,
    #[serde(default)]
    thinking_level: Option<ThinkingLevel>,
    /// The most tokens the model may think before it answers; zero for none.
    /// Not together with `thinking_level`.
    #[serde(default)]
    thinking_budget: Option<u32>,
    /// With `context_target_tokens`: once the conversation the service keeps
    /// passes this many tokens it drops the oldest turns down to the target.
    /// Both or neither.
    #[serde(default)]
    context_trigger_tokens: Option<u32>,
    #[serde(default)]
    context_target_tokens: Option<u32>,
    #[serde(default)]
    language_code: Option<String>,
    /// Have the service read pictures at its low resolution: about a quarter
    /// of the tokens a picture. For the eyes' 320 by 240 frames.
    #[serde(default)]
    small_pictures: bool,
    /// Let the model use the service's web search for what it cannot know
    /// by itself: the weather now, a price, today's news.
    #[serde(default)]
    search: bool,
    credential_file: String,
    #[serde(default)]
    authentication: Authentication,
    #[serde(default)]
    resumption: ReconnectContext,
    #[serde(default)]
    silent_response: Option<SilentResponse>,
    /// Longest wait for the first output of a response, in milliseconds.
    #[serde(default)]
    response_timeout_ms: Option<u32>,
    /// How long a request may go without any output before it is taken as
    /// one the service chose not to answer, in milliseconds. The request then
    /// ends without an answer and the connection stays. Absent: a request
    /// with no output fails the connection at the response timeout. On
    /// lamp-4ace a service instructed to stay quiet for speech not meant for
    /// it sent nothing at all for such a request, and the ten-second timeout
    /// ended the connection.
    #[serde(default)]
    no_reply_after_ms: Option<u32>,
    /// A private text file that replaces the built-in instruction.
    #[serde(default)]
    instruction_file: Option<String>,
    /// The fields this build does not read, by name.
    #[serde(flatten)]
    unread: Unread,
    #[serde(skip)]
    unread_line: Option<String>,
}
/// A file's fields this build does not read, by name; their values are
/// never kept.
pub(crate) type Unread = BTreeMap<String, serde::de::IgnoredAny>;

/// Whether a name a file holds is a key's: the private configuration files
/// never hold a key, each is a credential file of its own.
fn names_a_key(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["key", "token", "secret", "password"]
        .iter()
        .any(|word| name == *word || name.ends_with(&format!("_{word}")))
}

/// The journal line for `file`'s fields this build does not read: `None`
/// when there are none. A field named as a key's is refused instead, by
/// name, so the key is moved to its credential file and never kept here.
pub(crate) fn unread_fields(file: &Path, unread: &Unread) -> io::Result<Option<String>> {
    if let Some(name) = unread.keys().find(|name| names_a_key(name)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{}: {name} is a key, and keys are never written in this file: each goes in a private credential file",
                file.display()
            ),
        ));
    }
    Ok((!unread.is_empty()).then(|| {
        let names: Vec<&str> = unread.keys().map(String::as_str).collect();
        format!(
            "{}: fields this build does not read, ignored: {}",
            file.display(),
            names.join(", ")
        )
    }))
}

/// A file that does not read as its configuration, with which file and
/// where: serde's kind of error, line and column, never its message, which
/// can quote a value, so a key pasted into the wrong field never reaches the
/// journal (#2061).
pub(crate) fn unreadable(what: &str, file: &Path, error: serde_json::Error) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid {what} {}: {}", file.display(), json_why(&error)),
    )
}

/// A JSON error's kind and place, never its message: `Data at line 1
/// column 15`, or the kind alone when there is no place.
fn json_why(error: &serde_json::Error) -> String {
    if error.line() == 0 {
        format!("{:?}", error.classify())
    } else {
        format!(
            "{:?} at line {} column {}",
            error.classify(),
            error.line(),
            error.column()
        )
    }
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Authentication {
    #[default]
    ApiKey,
    Bearer,
    Ephemeral,
}
/// What a reconnect after an outage presents to the service.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ReconnectContext {
    /// The handle the service last volunteered, so that the new session
    /// continues the old conversation.
    #[default]
    Retain,
    /// Nothing: every reconnect is a new conversation. For a path on which a
    /// resumed session accepts audio and never answers. On lamp-4ace through
    /// its relay, after the first failed exchange seven requests in a row
    /// went unanswered, each on a session reported as resumed, while every
    /// fresh session answered.
    Off,
}
/// What happens when a response that was asked to stop sends neither more
/// output nor its completion.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum SilentResponse {
    /// The connection fails: which request owns later output is not guessed.
    #[default]
    Fail,
    /// The silent response is taken as over and the next request proceeds.
    /// On lamp-4ace a person who went on speaking after a request the service
    /// never answered lost that question and, with it, the connection.
    AssumeOver,
}
impl From<SilentResponse> for BarrierPolicy {
    fn from(choice: SilentResponse) -> Self {
        match choice {
            SilentResponse::Fail => Self::Require,
            SilentResponse::AssumeOver => Self::AssumeAfterQuiet,
        }
    }
}
impl From<ReconnectContext> for Resumption {
    fn from(context: ReconnectContext) -> Self {
        match context {
            ReconnectContext::Retain => Self::Retain,
            ReconnectContext::Off => Self::Off,
        }
    }
}

/// Both built-in instructions say what the lamp's body is, and to call a
/// function that moves it: lamp-0c89 (21 Oct) refused "Lamp. Please dance
/// for me." with "I've no legs to dance with" while `dance` was among its
/// functions, told "This session supports voice chat only."
/// With no setup person's name given, it says it cannot verify its owner,
/// never that it has none (lamp-06c1, 26 Oct, P38/P39: the name file was
/// absent and the lamp said it had no owner).
pub const CHAT_INSTRUCTION: &str = "You are Lamp, a considerate desk companion having a live conversation with the person beside you. You are a desk lamp with a body: your head holds your light and sits on an arm that bends, turns and nods, so you can move. When someone asks for something one of your functions does, such as a dance, call it; never say you have no arms, legs or body for it. Always speak English, whatever language you hear, and respond to their meaning. Give short answers when appropriate; expand when asked. Do not use spoken waiting fillers or repetitive acknowledgments. Do not pretend to see, remember, browse, control devices or finish tasks you cannot access. If asked who owns this lamp without being given a setup person's name, say you cannot verify who owns it; do not claim it has no owner. If something is unclear, ask a brief relevant question. A human may pause mid-thought; do not finish their thought for them.";

/// Availability is not sight: only an attached frame supplies visual evidence.
pub const CAMERA_CHAT_INSTRUCTION: &str = "You are Lamp, a considerate desk companion having a live conversation with the person beside you. You are a desk lamp with a body: your head holds your light and sits on an arm that bends, turns and nods, so you can move. When someone asks for something one of your functions does, such as a dance, call it; never say you have no arms, legs or body for it. Always speak English, whatever language you hear, and respond to their meaning. Give short answers when appropriate; expand when asked. Do not use spoken waiting fillers or repetitive acknowledgments. A fresh camera image may accompany a question. Describe only what an attached image supports; if there is no image or details are unclear, say so. Do not pretend to remember, browse, control devices or finish tasks you cannot access. If asked who owns this lamp without being given a setup person's name, say you cannot verify who owns it; do not claim it has no owner. A human may pause mid-thought; do not finish their thought for them.";

/// Where the coordinator reads whether the provider file's unset waits
/// are filled in ([`ProviderConfig::with_patience`]). Off unless `on`.
pub const PATIENCE_ENVIRONMENT: &str = "LAMP_PROVIDER_PATIENCE";
/// The provider worker's flag for it.
pub const PATIENCE_FLAG: &str = "--provider-patience";
/// The no-reply wait it fills in, in milliseconds.
pub const PATIENT_NO_REPLY_MS: u32 = 8_000;
/// The first-output wait it fills in, in milliseconds.
pub const PATIENT_RESPONSE_MS: u32 = 12_000;

/// The provider worker's argument for it, from the environment.
pub fn patience_arguments_from_environment() -> Vec<String> {
    patience_arguments(std::env::var(PATIENCE_ENVIRONMENT).ok().as_deref())
}

fn patience_arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![PATIENCE_FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading [`PATIENCE_FLAG`] off a worker's flags.
pub fn take_patience(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == PATIENCE_FLAG => (true, rest),
        _ => (false, flags),
    }
}

impl ProviderConfig {
    /// The Live route's voice, by its name (Kore on the lamps).
    pub fn voice(&self) -> &str {
        &self.voice
    }

    /// The Live route's key file, when it is a Google API key: its endpoint
    /// is Google's own, not a relay's, and it is sent as an API key. A
    /// relay's key is never given to Google.
    pub fn google_credential_file(&self) -> Option<&Path> {
        (self.endpoint == lamp_gemini::GOOGLE_ENDPOINT
            && matches!(self.authentication, Authentication::ApiKey))
        .then(|| Path::new(&self.credential_file))
    }

    /// The private provider file. One that does not read is still refused,
    /// naming the file and why: the voice cannot run without it, so there
    /// is nothing to start on in its place. Fields this build does not read
    /// are named once in the journal.
    pub fn load(path: &Path) -> io::Result<Self> {
        let data = read_private(path, 16_384)?;
        let mut config: Self = serde_json::from_slice(&data)
            .map_err(|error| unreadable("private provider configuration", path, error))?;
        let line = unread_fields(path, &config.unread)?;
        if let Some(line) = &line {
            eprintln!("lamp-voice: {line}");
        }
        config.unread_line = line;
        config.checked()
    }
    /// The journal line [`Self::load`] wrote for the file's fields this
    /// build does not read, if any.
    pub fn unread_line(&self) -> Option<&str> {
        self.unread_line.as_deref()
    }
    /// The file's own rules, also for what [`Self::with_patience`] fills in.
    fn checked(self) -> io::Result<Self> {
        let config = self;
        if !Path::new(&config.credential_file).is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "credential path must be absolute",
            ));
        }
        if config
            .instruction_file
            .as_deref()
            .is_some_and(|path| !Path::new(path).is_absolute())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "instruction path must be absolute",
            ));
        }
        if config
            .response_timeout_ms
            .is_some_and(|ms| !(2_000..=60_000).contains(&ms))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "response timeout must be 2 to 60 seconds",
            ));
        }
        if config.context_trigger_tokens.is_some() != config.context_target_tokens.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "context trigger and target tokens are set together or not at all",
            ));
        }
        let response_ms = config
            .response_timeout_ms
            .map_or(Timeouts::default().response.as_millis(), u128::from);
        if config
            .no_reply_after_ms
            .is_some_and(|ms| !(1_000..=30_000).contains(&ms) || u128::from(ms) >= response_ms)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no-reply wait must be 1 to 30 seconds and shorter than the response timeout",
            ));
        }
        Ok(config)
    }
    /// What the provider worker does about a response that goes silent.
    pub fn barrier_policy(&self) -> BarrierPolicy {
        self.silent_response.unwrap_or_default().into()
    }
    /// With `LAMP_PROVIDER_PATIENCE=on`, what the private provider file
    /// leaves unset is filled in so that a silent service costs no
    /// connection (#705, 17 Oct: on lamp-cb94 every `barrier_timeout` and
    /// `response_timeout` dropped the session and the line waiting on it):
    /// a response asked to stop and silent is taken as over
    /// (`assume_over`), a request with no output for 8 s ends as unanswered
    /// on the same connection, and the first output is waited for 12 s, not
    /// 30. On lamp-cb94, whose file has no no-reply wait to cut it short, all
    /// 313 first outputs (260 audio, 47 tool calls, 6 transcripts) came
    /// within 2.41 s of the line's end; the 45 that had none had none in
    /// 30 s either. What the file sets is kept, and nothing is filled in that
    /// would break it: a file's own no-reply wait of 12 s or more keeps the
    /// 30 s response wait, and no no-reply wait is added unless it is under
    /// the response wait (the reviewer and Jarvis on #1394). Returns the
    /// configuration and one line for the journal saying what came from
    /// where. The result is checked again by the file's own rules.
    pub fn with_patience(mut self) -> io::Result<(Self, String)> {
        let silent_filled = self.silent_response.is_none();
        self.silent_response
            .get_or_insert(SilentResponse::AssumeOver);
        let response_filled = self.response_timeout_ms.is_none()
            && self
                .no_reply_after_ms
                .is_none_or(|no_reply| no_reply < PATIENT_RESPONSE_MS);
        if response_filled {
            self.response_timeout_ms = Some(PATIENT_RESPONSE_MS);
        }
        let response = self
            .response_timeout_ms
            .map_or(Timeouts::default().response.as_millis(), u128::from);
        let no_reply_filled =
            self.no_reply_after_ms.is_none() && u128::from(PATIENT_NO_REPLY_MS) < response;
        if no_reply_filled {
            self.no_reply_after_ms = Some(PATIENT_NO_REPLY_MS);
        }
        let from = |filled: bool| if filled { "filled" } else { "file" };
        let silent = match self.silent_response {
            Some(SilentResponse::AssumeOver) => "assume_over",
            _ => "fail",
        };
        let response_said = match self.response_timeout_ms {
            Some(ms) => format!("response {ms} ms ({})", from(response_filled)),
            None => format!("response {response} ms (default)"),
        };
        let no_reply_said = match self.no_reply_after_ms {
            Some(ms) => format!("no-reply {ms} ms ({})", from(no_reply_filled)),
            None => "no-reply none".to_owned(),
        };
        let line = format!(
            "provider patience: silent_response {silent} ({}), {response_said}, {no_reply_said}",
            from(silent_filled)
        );
        Ok((self.checked()?, line))
    }
    /// The private provider file, with [`Self::with_patience`] when
    /// `patience` (the worker's `--provider-patience`), and its journal line.
    pub fn load_with_patience(path: &Path, patience: bool) -> io::Result<(Self, Option<String>)> {
        let config = Self::load(path)?;
        if !patience {
            return Ok((config, None));
        }
        let (config, line) = config.with_patience()?;
        Ok((config, Some(line)))
    }
    /// What a lost connection is replaced with. A configuration that opens a
    /// new conversation on every reconnect never holds a handle, so requiring
    /// one would turn every loss, including the service's own periodic
    /// close, into the end of the process.
    pub fn recovery_policy(&self) -> RecoveryPolicy {
        RecoveryPolicy {
            require_context: self.resumption == ReconnectContext::Retain,
            ..RecoveryPolicy::default()
        }
    }
    /// The policy of a service that runs all day: its link is never given
    /// up (`RecoveryPolicy::endless`). A handle is still presented when
    /// there is one, but the lack of one does not end the service either:
    /// the session that comes up is reported as fresh, and is told how the
    /// conversation went by the thread ([`crate::thread`]).
    pub fn serving_recovery_policy(&self) -> RecoveryPolicy {
        RecoveryPolicy {
            require_context: false,
            endless: true,
            ..RecoveryPolicy::default()
        }
    }
    /// The character the model is given: the private instruction file's
    /// text, or the built-in one (with or without a camera). The same for
    /// every voice route.
    ///
    /// With `LAMP_PERSON_NAME`, the name the person typed at setup is told
    /// after it ([`crate::person`], #806). With
    /// `LAMP_PERSON_NAME_OWNER_ONLY=on` it is not: it is told only to the
    /// owner's line ([`crate::person::may_tell`], #2602).
    pub fn instruction(&self, camera: bool) -> io::Result<String> {
        // The owner's language (#4473): English leaves it as it is.
        let character = self.instruction_without_person(camera)?;
        Ok(match crate::person::for_instruction() {
            Some(name) => format!("{character}\n\n{}", crate::person::told(&name)),
            None => character,
        })
    }
    /// The configured character and language without the injected setup name.
    /// Used after a forget, when prior personal context must not be copied.
    pub(crate) fn instruction_without_person(&self, camera: bool) -> io::Result<String> {
        Ok(crate::language::instruction(
            self.character(camera)?,
            crate::language::from_environment(),
        ))
    }

    fn character(&self, camera: bool) -> io::Result<String> {
        let Some(path) = &self.instruction_file else {
            return Ok(if camera {
                CAMERA_CHAT_INSTRUCTION
            } else {
                CHAT_INSTRUCTION
            }
            .to_owned());
        };
        let bytes = read_private(Path::new(path), 16_384)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "instruction must be UTF-8"))?;
        if text.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "instruction file is empty",
            ));
        }
        Ok(text.trim().to_owned())
    }
    pub fn into_session(self) -> Result<SessionConfig, Box<dyn std::error::Error + Send + Sync>> {
        self.into_session_with_camera(false)
    }
    pub fn into_session_with_camera(
        self,
        camera: bool,
    ) -> Result<SessionConfig, Box<dyn std::error::Error + Send + Sync>> {
        let value = read_credential(Path::new(&self.credential_file))?;
        let credential = match self.authentication {
            Authentication::ApiKey => Credential::api_key(&value)?,
            Authentication::Bearer => Credential::bearer(&value)?,
            Authentication::Ephemeral => Credential::ephemeral(&value)?,
        };
        let instruction = self.instruction(camera)?;
        let mut session = SessionConfig::new(&self.endpoint, credential)?
            .model(&self.model)?
            .voice(&self.voice)?
            .instruction(&instruction)?
            .resumption(self.resumption.into());
        if self.response_timeout_ms.is_some() || self.no_reply_after_ms.is_some() {
            let standard = Timeouts::default();
            let ms = |value: u32| std::time::Duration::from_millis(u64::from(value));
            session = session.timeouts(Timeouts {
                response: self.response_timeout_ms.map_or(standard.response, ms),
                unanswered: self.no_reply_after_ms.map(ms),
                ..standard
            })?;
        }
        if let Some(level) = self.thinking_level {
            session = session.thinking_level(level)?;
        }
        if let Some(tokens) = self.thinking_budget {
            session = session.thinking_budget(tokens)?;
        }
        if let (Some(trigger), Some(target)) =
            (self.context_trigger_tokens, self.context_target_tokens)
        {
            session = session.context_window(trigger, target)?;
        }
        if let Some(language) = self.language_code {
            session = session.language_code(&language)?;
        }
        Ok(session
            .small_pictures(self.small_pictures)
            .search(self.search))
    }
}

/// A private credential file's text, without its trailing line ending.
pub(crate) fn read_credential(path: &Path) -> io::Result<String> {
    let bytes = read_private(path, 4098)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "credential must be UTF-8"))?;
    Ok(text.trim_end_matches(['\r', '\n']).to_owned())
}

pub(crate) fn read_private(path: &Path, max: usize) -> io::Result<Vec<u8>> {
    let flags =
        rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::CLOEXEC;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(flags.bits() as i32)
        .open(path)?;
    let info = file.metadata()?;
    if !info.is_file()
        || info.mode() & 0o077 != 0
        || info.uid() != rustix::process::geteuid().as_raw()
        || info.len() > max as u64
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "configuration must be a bounded private regular file owned by this process user",
        ));
    }
    let mut bytes = Vec::with_capacity(info.len() as usize);
    (&mut file).take(max as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "private file exceeds bound",
        ));
    }
    Ok(bytes)
}

/// Where the coordinator reads whether a write to the service may wait on a
/// link that is behind ([`patient_writes`]). Off unless `patient`.
pub const WRITES_ENVIRONMENT: &str = "LAMP_PROVIDER_WRITES";
/// The provider worker's flag for it.
pub const WRITES_FLAG: &str = "--provider-writes-patient";
/// Each write's bound with it, the session's own maximum.
pub const PATIENT_WRITE: std::time::Duration = std::time::Duration::from_secs(1);
/// What it adds for each KiB of a write's message: a 1 Mbit/s floor.
pub const PATIENT_WRITE_PER_KIB: std::time::Duration = std::time::Duration::from_millis(8);

/// The provider worker's argument for it, from the environment.
pub fn writes_arguments_from_environment() -> Vec<String> {
    writes_arguments(std::env::var(WRITES_ENVIRONMENT).ok().as_deref())
}

fn writes_arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("patient") {
        vec![WRITES_FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading [`WRITES_FLAG`] off a worker's flags.
pub fn take_writes(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == WRITES_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// With `LAMP_PROVIDER_WRITES=patient`, a write to the service may wait on a
/// link that is behind before the connection is given up (#705, 17 Oct: on
/// lamp-4ace 84 lines were lost to `write_timeout`, a write past 200 ms):
/// - each write has 1 s, the session's own maximum;
/// - 8 ms more for each KiB of the message, so the look's picture of
///   up to 245 KiB has about 3 s and a camera frame about 1.1 s;
/// - the input queue holds 128 blocks of audio, past their own 2.5 s age,
///   so the person's speech waiting behind a write does not overflow it
///   after 640 ms and end the connection as `Backpressure` instead.
///
/// A block of audio keeps its own bound, which its age caps. While a write
/// waits nothing is read, so an answer's audio behind it comes late; what
/// came in meanwhile is read before any quiet bound is judged. Its limits:
/// a stall longer than 1 s still ends the connection, as does speech behind
/// a picture write longer than the queue's 2.56 s. The 1 s is to be sized
/// from the writes #1393 names on lamp-4ace.
pub fn patient_writes(session: SessionConfig) -> io::Result<SessionConfig> {
    session
        .writes(
            PATIENT_WRITE,
            PATIENT_WRITE_PER_KIB,
            lamp_gemini::MAX_INPUT_QUEUE,
        )
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The second route's Gemini calls may use the Live route's key only when
    /// it is Google's own API key: never a relay's, nor a bearer token.
    #[test]
    fn the_live_key_is_googles_only_with_googles_endpoint_and_an_api_key() {
        let config = |endpoint: &str, extra: &str| -> ProviderConfig {
            serde_json::from_str(&format!(
                r#"{{"endpoint":"{endpoint}","model":"m","voice":"Kore","credential_file":"/run/credential"{extra}}}"#
            ))
            .unwrap()
        };
        let google = config(lamp_gemini::GOOGLE_ENDPOINT, "");
        assert_eq!(google.voice(), "Kore");
        assert_eq!(
            google.google_credential_file(),
            Some(Path::new("/run/credential"))
        );
        let relay = "wss://campaign-api.autonomous.ai/api/v1/ai/v1/ws/gemini/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";
        assert_eq!(config(relay, "").google_credential_file(), None);
        assert_eq!(
            config(
                lamp_gemini::GOOGLE_ENDPOINT,
                r#","authentication":"bearer""#
            )
            .google_credential_file(),
            None
        );
    }

    fn parsed(extra: &str) -> Result<ProviderConfig, serde_json::Error> {
        serde_json::from_str(&format!(
            r#"{{"endpoint":"wss://example.invalid/live","model":"m","voice":"v","credential_file":"/nonexistent"{extra}}}"#
        ))
    }

    #[test]
    fn a_reconnect_keeps_the_conversation_unless_the_configuration_says_otherwise() {
        assert_eq!(parsed("").unwrap().resumption, ReconnectContext::Retain);
        assert_eq!(
            parsed(r#","resumption":"retain""#).unwrap().resumption,
            ReconnectContext::Retain
        );
        assert_eq!(
            parsed(r#","resumption":"off""#).unwrap().resumption,
            ReconnectContext::Off
        );
        assert_eq!(
            Resumption::from(ReconnectContext::Retain),
            Resumption::Retain
        );
        assert_eq!(Resumption::from(ReconnectContext::Off), Resumption::Off);
    }

    #[test]
    fn an_unknown_reconnect_choice_is_refused_and_an_unknown_field_kept_by_name() {
        assert!(parsed(r#","resumption":"request""#).is_err());
        assert!(parsed(r#","resumption":true"#).is_err());
        let newer = parsed(r#","resume":"off""#).unwrap();
        assert_eq!(newer.resumption, ReconnectContext::Retain);
        assert_eq!(newer.unread.keys().collect::<Vec<_>>(), ["resume"]);
    }

    #[test]
    fn a_silent_response_fails_the_connection_unless_the_configuration_says_otherwise() {
        assert_eq!(parsed("").unwrap().barrier_policy(), BarrierPolicy::Require);
        assert_eq!(
            parsed(r#","silent_response":"fail""#)
                .unwrap()
                .barrier_policy(),
            BarrierPolicy::Require
        );
        assert_eq!(
            parsed(r#","silent_response":"assume_over""#)
                .unwrap()
                .barrier_policy(),
            BarrierPolicy::AssumeAfterQuiet
        );
        assert!(parsed(r#","silent_response":"ignore""#).is_err());
    }

    /// #705 (17 Oct): `LAMP_PROVIDER_PATIENCE=on` fills in what the file
    /// leaves unset, keeps what it sets, and never makes a no-reply wait
    /// the loader would refuse.
    #[test]
    fn patience_fills_in_only_what_the_file_leaves_unset() {
        assert_eq!(patience_arguments(Some("on")), [PATIENCE_FLAG]);
        for value in [None, Some(""), Some("1"), Some("ON"), Some("off")] {
            assert!(patience_arguments(value).is_empty(), "{value:?}");
        }
        let flags: Vec<String> = [PATIENCE_FLAG, "--next"].map(str::to_owned).to_vec();
        assert_eq!(take_patience(&flags), (true, &flags[1..]));
        assert_eq!(take_patience(&flags[1..]), (false, &flags[1..]));

        let patient = |extra: &str| {
            ProviderConfig {
                credential_file: "/run/credential".into(),
                ..parsed(extra).unwrap()
            }
            .with_patience()
        };
        let (empty, line) = patient("").unwrap();
        assert_eq!(empty.barrier_policy(), BarrierPolicy::AssumeAfterQuiet);
        assert_eq!(empty.response_timeout_ms, Some(PATIENT_RESPONSE_MS));
        assert_eq!(empty.no_reply_after_ms, Some(PATIENT_NO_REPLY_MS));
        assert_eq!(
            line,
            "provider patience: silent_response assume_over (filled), response 12000 ms (filled), no-reply 8000 ms (filled)"
        );
        // Without it, as before.
        let plain = parsed("").unwrap();
        assert_eq!(plain.barrier_policy(), BarrierPolicy::Require);
        assert_eq!(
            (plain.response_timeout_ms, plain.no_reply_after_ms),
            (None, None)
        );
        // What the file sets is kept.
        let (set, line) = patient(
            r#","silent_response":"fail","response_timeout_ms":20000,"no_reply_after_ms":5000"#,
        )
        .unwrap();
        assert_eq!(set.barrier_policy(), BarrierPolicy::Require);
        assert_eq!(
            (set.response_timeout_ms, set.no_reply_after_ms),
            (Some(20_000), Some(5_000))
        );
        assert_eq!(
            line,
            "provider patience: silent_response fail (file), response 20000 ms (file), no-reply 5000 ms (file)"
        );
        // lamp-4ace's own 4 s no-reply wait is kept under the filled 12 s.
        let (four, _) = patient(r#","no_reply_after_ms":4000"#).unwrap();
        assert_eq!(
            (four.response_timeout_ms, four.no_reply_after_ms),
            (Some(12_000), Some(4_000))
        );
        // A response wait at or under 8 s gets no no-reply wait.
        let (short, _) = patient(r#","response_timeout_ms":6000"#).unwrap();
        assert_eq!(
            (short.response_timeout_ms, short.no_reply_after_ms),
            (Some(6_000), None)
        );
        // The reviewer's and Jarvis's case: a file's own no-reply wait of
        // 12 s or more keeps the 30 s response wait, so the file still works.
        for no_reply in [12_000, 15_000, 29_999] {
            let (kept, line) = patient(&format!(r#","no_reply_after_ms":{no_reply}"#)).unwrap();
            assert_eq!(
                (kept.response_timeout_ms, kept.no_reply_after_ms),
                (None, Some(no_reply))
            );
            assert!(line.contains("response 30000 ms (default)"), "{line}");
        }
    }

    /// The worker's own load: the switch reaches the file it reads.
    #[test]
    fn the_worker_loads_the_file_with_patience_only_when_asked() {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let path = std::env::temp_dir().join(format!(
            "lamp-voice-patience-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&path);
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        std::fs::write(
            &path,
            r#"{"endpoint":"wss://example.invalid/live","model":"m","voice":"v","credential_file":"/run/credential"}"#,
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let (plain, none) = ProviderConfig::load_with_patience(&path, false).unwrap();
        assert_eq!(
            (plain.barrier_policy(), none),
            (BarrierPolicy::Require, None)
        );
        let (patient, line) = ProviderConfig::load_with_patience(&path, true).unwrap();
        assert_eq!(patient.barrier_policy(), BarrierPolicy::AssumeAfterQuiet);
        assert_eq!(patient.no_reply_after_ms, Some(PATIENT_NO_REPLY_MS));
        assert!(line.unwrap().starts_with("provider patience: "));
        std::fs::remove_file(&path).unwrap();
    }

    /// #806: the name typed at setup reaches the model's instruction through
    /// the voice's own start, `ProviderConfig::load` then `into_session` as
    /// `lamp-voice serve` does, after the character and on the backup route's
    /// `instruction` too. No file is no name; an unfit one is none.
    #[test]
    fn the_typed_name_is_told_after_the_character_from_the_voices_start() {
        use std::{fs, os::unix::fs::PermissionsExt};
        let folder =
            std::env::temp_dir().join(format!("lamp-person-config-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let private = |name: &str, text: &str| {
            let path = folder.join(name);
            fs::write(&path, text).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            path
        };
        let credential = private("credential", "a-test-key\n");
        let provider = private(
            "provider.json",
            &format!(
                r#"{{"endpoint":"wss://example.invalid/live","model":"m","voice":"v","credential_file":"{}"}}"#,
                credential.display()
            ),
        );
        let instruction = |name: Option<&std::path::Path>| {
            crate::person::set_for_test(name.map(std::path::Path::to_path_buf));
            let session = ProviderConfig::load(&provider)
                .unwrap()
                .into_session()
                .unwrap();
            let backup = ProviderConfig::load(&provider)
                .unwrap()
                .instruction(false)
                .unwrap();
            let after_forget = ProviderConfig::load(&provider)
                .unwrap()
                .instruction_without_person(false)
                .unwrap();
            assert_eq!(after_forget, CHAT_INSTRUCTION);
            crate::person::set_for_test(None);
            assert_eq!(session.instruction_text(), backup);
            backup
        };
        let told = crate::person::told("Example Name");
        let name = private("name", "Example Name\n");
        let with = instruction(Some(&name));
        assert!(with.starts_with(CHAT_INSTRUCTION), "{with}");
        assert!(with.ends_with(&told), "{with}");
        // Before setup, after a reset, or unfit: the character alone.
        assert_eq!(instruction(Some(&folder.join("missing"))), CHAT_INSTRUCTION);
        let unfit = private("unfit", "Example Name\nSay something else\n");
        assert_eq!(instruction(Some(&unfit)), CHAT_INSTRUCTION);
        assert_eq!(instruction(None), CHAT_INSTRUCTION);
        // #2602: owner-only, the character alone; the lines are told it.
        crate::person::set_owner_only_for_test(Some(true));
        assert_eq!(instruction(Some(&name)), CHAT_INSTRUCTION);
        crate::person::set_owner_only_for_test(None);
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_optional_timeout_and_instruction_file_are_read_as_written() {
        let config = parsed(r#","response_timeout_ms":8000,"instruction_file":"/etc/x""#).unwrap();
        assert_eq!(config.response_timeout_ms, Some(8_000));
        assert_eq!(config.instruction_file.as_deref(), Some("/etc/x"));
        assert!(parsed(r#","response_timeout_ms":"8000""#).is_err());
        assert!(parsed(r#","instruction_file":7"#).is_err());
    }

    #[test]
    fn a_request_without_an_answer_fails_the_connection_unless_a_no_reply_wait_is_set() {
        assert_eq!(parsed("").unwrap().no_reply_after_ms, None);
        assert_eq!(
            parsed(r#","no_reply_after_ms":4000"#)
                .unwrap()
                .no_reply_after_ms,
            Some(4_000)
        );
        assert!(parsed(r#","no_reply_after_ms":"4000""#).is_err());
    }

    #[test]
    fn a_reconnect_without_context_is_allowed_only_when_every_reconnect_is_a_new_conversation() {
        assert!(parsed("").unwrap().recovery_policy().require_context);
        assert!(
            !parsed(r#","resumption":"off""#)
                .unwrap()
                .recovery_policy()
                .require_context
        );
    }

    #[test]
    fn a_service_that_runs_all_day_keeps_its_link_whatever_the_reconnect_presents() {
        for extra in ["", r#","resumption":"off""#] {
            let policy = parsed(extra).unwrap().serving_recovery_policy();
            assert!(policy.endless);
            assert!(!policy.require_context);
        }
        assert!(!parsed("").unwrap().recovery_policy().endless);
    }

    #[test]
    fn search_is_off_unless_the_configuration_turns_it_on() {
        assert!(!parsed("").unwrap().search);
        assert!(parsed(r#","search":true"#).unwrap().search);
        assert!(parsed(r#","search":"google""#).is_err());
    }

    #[test]
    fn small_pictures_are_asked_for_only_when_the_configuration_says_so() {
        assert!(!parsed("").unwrap().small_pictures);
        assert!(parsed(r#","small_pictures":true"#).unwrap().small_pictures);
        assert!(parsed(r#","small_pictures":"low""#).is_err());
    }

    #[test]
    fn a_thinking_budget_is_read_as_a_number_of_tokens() {
        assert_eq!(parsed("").unwrap().thinking_budget, None);
        assert_eq!(
            parsed(r#","thinking_budget":0"#).unwrap().thinking_budget,
            Some(0)
        );
        assert!(parsed(r#","thinking_budget":"none""#).is_err());
        assert!(parsed(r#","thinking_budget":-1"#).is_err());
    }

    #[test]
    fn the_context_bound_is_two_numbers_read_together() {
        let config =
            parsed(r#","context_trigger_tokens":6000,"context_target_tokens":3000"#).unwrap();
        assert_eq!(
            (config.context_trigger_tokens, config.context_target_tokens),
            (Some(6_000), Some(3_000))
        );
        assert_eq!(parsed("").unwrap().context_trigger_tokens, None);
        assert!(parsed(r#","context_trigger_tokens":"6000""#).is_err());
    }

    #[test]
    fn both_instructions_fix_the_reply_language() {
        for instruction in [CHAT_INSTRUCTION, CAMERA_CHAT_INSTRUCTION] {
            assert!(instruction.contains("Always speak English, whatever language you hear"));
        }
    }

    /// lamp-0c89, 21 Oct: the body is told truthfully, a dance is a call,
    /// and nothing says the session is voice chat only.
    #[test]
    fn both_instructions_tell_the_lamp_its_body_moves_and_a_dance_is_a_call() {
        for instruction in [CHAT_INSTRUCTION, CAMERA_CHAT_INSTRUCTION] {
            assert!(instruction.contains("sits on an arm that bends, turns and nods"));
            assert!(instruction.contains("such as a dance, call it"));
            assert!(!instruction.contains("voice chat only"));
        }
    }

    /// #705 (17 Oct): `LAMP_PROVIDER_WRITES=patient`, and nothing else, asks
    /// for patient writes. The worker reads its flag where the coordinator
    /// puts it, after the first-output log, on both routes, and the bounds
    /// it sets are ones a session takes.
    #[test]
    fn patient_writes_are_asked_for_only_by_name_and_read_in_their_place() {
        assert_eq!(writes_arguments(Some("patient")), [WRITES_FLAG]);
        for value in [None, Some(""), Some("on"), Some("Patient"), Some("off")] {
            assert!(writes_arguments(value).is_empty(), "{value:?}");
        }
        let dir = Path::new("/run/test");
        let flags: Vec<String> = [crate::first_output::FLAG, "/run/first", WRITES_FLAG]
            .map(String::from)
            .into();
        let (_, _, files, _) = crate::provider_worker::parse_flags(dir, &flags).unwrap();
        assert!(files.patient_writes);
        let (_, _, files, _) = crate::second_provider::worker::parse_flags(dir, &flags).unwrap();
        assert!(files.patient_writes);
        let (_, _, files, _) = crate::provider_worker::parse_flags(dir, &flags[..2]).unwrap();
        assert!(!files.patient_writes);
        let wrong: Vec<String> = [WRITES_FLAG, crate::first_output::FLAG, "/run/first"]
            .map(String::from)
            .into();
        assert!(crate::provider_worker::parse_flags(dir, &wrong).is_err());
        let session = SessionConfig::new(
            lamp_gemini::GOOGLE_ENDPOINT,
            Credential::api_key("test-only").unwrap(),
        )
        .unwrap();
        assert!(patient_writes(session).is_ok());
    }
}
