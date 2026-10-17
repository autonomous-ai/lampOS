//! Explicit private configuration. Neither the HAL nor os-server configuration
//! is read by this runtime. Provisioning may migrate values once outside source.
use lamp_gemini::{
    BarrierPolicy, Credential, RecoveryPolicy, Resumption, SessionConfig, ThinkingLevel, Timeouts,
};
use serde::Deserialize;
use std::{
    fs::OpenOptions,
    io::{self, Read},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
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
    silent_response: SilentResponse,
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

pub const CHAT_INSTRUCTION: &str = "You are Lamp, a considerate desk companion having a live conversation with the person beside you. Always speak English, whatever language you hear, and respond to their meaning. Give short answers when appropriate; expand when asked. Do not use spoken waiting fillers or repetitive acknowledgments. Do not pretend to see, remember, browse, control devices or finish tasks you cannot access. This session supports voice chat only. If something is unclear, ask a brief relevant question. A human may pause mid-thought; do not finish their thought for them.";

/// Availability is not sight: only an attached frame supplies visual evidence.
pub const CAMERA_CHAT_INSTRUCTION: &str = "You are Lamp, a considerate desk companion having a live conversation with the person beside you. Always speak English, whatever language you hear, and respond to their meaning. Give short answers when appropriate; expand when asked. Do not use spoken waiting fillers or repetitive acknowledgments. A fresh camera image may accompany a question. Describe only what an attached image supports; if there is no image or details are unclear, say so. Do not pretend to remember, browse, control devices or finish tasks you cannot access. A human may pause mid-thought; do not finish their thought for them.";

impl ProviderConfig {
    pub fn load(path: &Path) -> io::Result<Self> {
        let data = read_private(path, 16_384)?;
        let config: Self = serde_json::from_slice(&data).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid private provider configuration",
            )
        })?;
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
        self.silent_response.into()
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
    /// after it ([`crate::person`], #806).
    pub fn instruction(&self, camera: bool) -> io::Result<String> {
        let character = self.character(camera)?;
        Ok(match crate::person::from_environment() {
            Some(name) => format!("{character}\n\n{}", crate::person::told(&name)),
            None => character,
        })
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
    fn an_unknown_reconnect_choice_or_field_is_refused() {
        assert!(parsed(r#","resumption":"request""#).is_err());
        assert!(parsed(r#","resumption":true"#).is_err());
        assert!(parsed(r#","resume":"off""#).is_err());
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
