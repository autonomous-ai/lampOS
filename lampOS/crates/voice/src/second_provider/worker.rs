//! The second route as a provider worker, on the ordinary provider IPC:
//! the fixture's startup readiness, authority guard and credited output
//! (`crate::fixture_provider`), around [`Engine`] instead of a recording.
//!
//! [`serve`] is the process (`lamp-voice worker second`): the character of
//! the Live route's configuration, told the same time, memory note and
//! conversation thread as a Live session is, with the same tools, and each
//! turn kept in that thread and the memory's journal as the Live route
//! keeps its own.
use super::{
    Engine, Joined, Net, Told, ToolRunner,
    net::{Message, search::Search},
    output_request,
};
#[cfg(test)]
use crate::scenario_clock::monotonic_us;
use crate::{
    provider_flow::OutputSender,
    provider_worker::{ProviderInput, ProviderOutput},
    transport::{Channel, WorkerChannels},
    wire::{Control, WorkerEvent},
};
#[cfg(not(test))]
use lamp_ipc::monotonic_us;
use lamp_turn_owner::{BootId, BoundaryGuard, MonoTime, Snapshot};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

mod carry_window;
mod held_colleagues;
#[cfg(test)]
mod memory_note_retries;

/// The coordinator renews authority at least this often.
const CONTROL_LEASE_US: u64 = 250_000;
/// The longest a backlog left by a stall may hold the fresh authority back
/// before the controller counts as gone, as the Live worker (#784).
const STALLED_CONTROL_US: u64 = 1_500_000;
/// Control messages taken in one tick before input and output wait.
const CONTROL_SLICE: usize = 16;
/// Input commands taken in one tick, as the Live provider worker does.
const INPUT_SLICE: usize = 8;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// The fixture's authority bookkeeping: who owns the turn, which requests
/// are over, and whether the controller is still there.
struct Gate {
    guard: BoundaryGuard,
    flow: OutputSender,
    last_control_us: u64,
    highest_seen: u64,
    retired_through: u64,
    /// Authority snapshots that lapsed in the queue while this worker was
    /// held up, since the last one installed.
    skipped_expired: u32,
}

enum Drain {
    Empty,
    Deferred,
    Stopped,
}

impl Gate {
    fn new(boot: BootId, now_us: u64) -> Self {
        Self {
            guard: BoundaryGuard::new(boot, MonoTime::from_micros(now_us)),
            flow: OutputSender::default(),
            last_control_us: now_us,
            highest_seen: 0,
            retired_through: 0,
            skipped_expired: 0,
        }
    }

    /// As the fixture's `install_inner`: a newer owner retires every
    /// earlier request, no owner retires all seen, and an old owner coming
    /// back is a fault. The engine drops the retired requests' work.
    fn install(&mut self, now_us: u64, state: Snapshot, engine: &mut Engine) -> io::Result<()> {
        match self.guard.install(MonoTime::from_micros(now_us), state) {
            Ok(()) => {}
            // Queued while this worker was held up (on lamp-4ace by the
            // kernel re-tuning its SD/MMC hosts, #784; it ended the voice
            // there on 2026-10-16, #960): it lapsed in the queue. Fresher
            // ones follow it; the heartbeat in `drain` ends the route if
            // none comes.
            Err(lamp_turn_owner::Error::ExpiredState) => {
                self.skipped_expired = self.skipped_expired.saturating_add(1);
                return Ok(());
            }
            Err(error) => return Err(io::Error::other(error)),
        }
        if self.skipped_expired > 0 {
            eprintln!(
                "lamp-voice second: held up {} ms; {} lapsed authority snapshots skipped, going on",
                now_us.saturating_sub(self.last_control_us) / 1_000,
                self.skipped_expired
            );
            self.skipped_expired = 0;
        }
        engine.memory_camera(
            state.camera_allowed(MonoTime::from_micros(now_us)),
            state.camera_generation(),
        );
        engine.privacy((
            state.camera_permission() == lamp_turn_owner::Permission::Allowed,
            state.camera_generation(),
            state.microphone_permission() == lamp_turn_owner::Permission::Allowed,
            state.microphone_generation(),
        ));
        self.last_control_us = now_us;
        if let Some(owner) = state.owner() {
            if owner.turn() <= self.retired_through || owner.turn() < self.highest_seen {
                return Err(invalid(
                    "second route authority resurrected a retired owner",
                ));
            }
            self.highest_seen = owner.turn();
            self.retired_through = self.retired_through.max(owner.turn() - 1);
            engine.retire_through(self.retired_through);
        } else {
            // No owner: stopped, muted, cut by a gap or over, never cut by
            // a newer line, so nothing of it is kept for the next one.
            self.retired_through = self.retired_through.max(self.highest_seen);
            engine.stop_through(self.retired_through);
        }
        // A listener kept for the next line goes when listening stops (the
        // privacy switch, a mute) or its generation changes (#1712).
        if let Some(state) = self.guard.state() {
            engine.listening(
                state.listening_ready(MonoTime::from_micros(now_us)),
                state.microphone_generation(),
            );
        }
        Ok(())
    }

    /// As the fixture's `fresh`, less its finite-run clock rules: judged
    /// once the control queue is empty, so on the newest authority sent.
    fn fresh(&self, now_us: u64) -> io::Result<()> {
        if let Some(state) = self.guard.state()
            && (now_us >= state.expires_at().as_micros()
                || (state.owner().is_some()
                    && !state.listening_ready(MonoTime::from_micros(now_us))))
        {
            return Err(invalid("second route privacy or input lease expired"));
        }
        Ok(())
    }

    /// Input is taken only for the admitted owner, under the current
    /// microphone generation. Input for a retired request is dropped.
    fn admits(&self, now_us: u64, command: &ProviderInput) -> io::Result<bool> {
        let (request, privacy) = match command {
            ProviderInput::Start {
                request,
                privacy_generation,
                ..
            }
            | ProviderInput::Audio {
                request,
                privacy_generation,
                ..
            }
            | ProviderInput::End {
                request,
                privacy_generation,
                ..
            }
            | ProviderInput::Called {
                request,
                privacy_generation,
            }
            | ProviderInput::Paused {
                request,
                privacy_generation,
            }
            | ProviderInput::CountOnly {
                request,
                privacy_generation,
            }
            | ProviderInput::StepCall {
                request,
                privacy_generation,
            }
            | ProviderInput::Written {
                request,
                privacy_generation,
                ..
            }
            | ProviderInput::Reminder {
                request,
                privacy_generation,
                ..
            } => (*request, *privacy_generation),
        };
        let state = self
            .guard
            .state()
            .ok_or_else(|| invalid("second route has no authority"))?;
        if request == 0
            || !state.listening_ready(MonoTime::from_micros(now_us))
            || privacy != state.microphone_generation()
        {
            return Err(invalid(
                "second route input lost privacy or retention authority",
            ));
        }
        if request <= self.retired_through {
            return Ok(false);
        }
        if state.owner().map(|owner| owner.turn()) != Some(request) {
            return Err(invalid("second route input has no matching admitted owner"));
        }
        Ok(true)
    }

    /// Output of a request goes only to its owner while input is retained.
    fn may_send(&self, now_us: u64, event: &ProviderOutput) -> io::Result<()> {
        let Some(request) = output_request(event) else {
            return Ok(());
        };
        let state = self
            .guard
            .state()
            .ok_or_else(|| invalid("second route output has no authority"))?;
        if !state.listening_ready(MonoTime::from_micros(now_us))
            || state.owner().map(|owner| owner.turn()) != Some(request)
        {
            return Err(invalid("second route output lost original owner"));
        }
        Ok(())
    }

    /// Taking an event from Engine does not authorize a later IPC retry. A
    /// camera change can revoke its scope, or its original start deadline
    /// can expire, while the microphone owner stays.
    /// `true` consumes the pending entry. A discard spends no output credit.
    fn send_or_discard(
        &mut self,
        engine: &Engine,
        now_us: u64,
        event: &ProviderOutput,
        send: impl FnOnce(&ProviderOutput) -> io::Result<()>,
    ) -> io::Result<bool> {
        if !engine.current_output(event) {
            return Ok(true);
        }
        if let ProviderOutput::RequestedAudio {
            request,
            session,
            offer,
        } = event
            && (offer.check().is_err()
                || offer.context.request != *request
                || offer.context.provider_session != *session
                || now_us < offer.issued_us
                || now_us >= offer.start_before_us)
        {
            return Ok(true);
        }
        self.may_send(now_us, event)?;
        self.flow.try_send(|| send(event))
    }

    fn drain(&mut self, control: &mut Channel, engine: &mut Engine) -> io::Result<Drain> {
        let mut drained = Drain::Deferred;
        for _ in 0..CONTROL_SLICE {
            match control.receive()? {
                Some(Control::Stop) => {
                    // Its Days session closed, so a restart leaves none (#1722, D).
                    crate::remember::close_at_exit();
                    return Ok(Drain::Stopped);
                }
                Some(Control::Authority { snapshot }) => {
                    self.install(monotonic_us(), snapshot, engine)?
                }
                Some(Control::ProviderOutputCapacity { through }) => self.flow.grant(through)?,
                Some(Control::StartCapture | Control::ConnectReference { .. }) => {
                    return Err(invalid("invalid second route provider control"));
                }
                None => {
                    drained = Drain::Empty;
                    break;
                }
            }
        }
        // While a backlog left by a stall is still being read, the fresh
        // authority may be further down it: the heartbeat is judged once
        // the queue is empty, or after the longest stall a gap may be.
        let silent = monotonic_us().saturating_sub(self.last_control_us);
        if silent > STALLED_CONTROL_US
            || (matches!(drained, Drain::Empty) && silent >= CONTROL_LEASE_US)
        {
            return Err(invalid("second route controller heartbeat expired"));
        }
        Ok(drained)
    }
}

/// A request's answer sound as it goes out: when its first samples went and
/// how many, so its end is when they have all been played (the first sound
/// plus their length at [`super::net::TTS_SAMPLE_RATE`]).
#[derive(Debug, Default)]
struct AnswerSound {
    request: Option<u64>,
    first: Option<Instant>,
    samples: u64,
}

impl AnswerSound {
    fn heard(&mut self, event: &ProviderOutput, at: Instant) {
        if let ProviderOutput::Audio {
            request, samples, ..
        } = event
        {
            if self.request != Some(*request) {
                *self = Self {
                    request: Some(*request),
                    first: Some(at),
                    samples: 0,
                };
            }
            self.samples += samples.len() as u64;
        }
    }

    /// When `request`'s answer ends playing; now for one with no sound.
    fn ends(&self, request: u64) -> Instant {
        match (self.request == Some(request), self.first) {
            (true, Some(first)) => {
                first
                    + Duration::from_micros(
                        self.samples * 1_000_000 / u64::from(super::net::TTS_SAMPLE_RATE),
                    )
            }
            _ => Instant::now(),
        }
    }
}

/// Whether a turn is told and kept for its admitted memory scope (#1675):
/// its recall, history, Days note and answer that person's or the room's,
/// never the sticky session owner's. `on` only; otherwise as before.
pub const SCOPED_CONTEXT_ENVIRONMENT: &str = "LAMP_SECOND_SCOPED_CONTEXT";

/// `on`: the same person's conversation quiet for ten minutes
/// ([`crate::thread::REMEMBERED_FOR`]) is over, and the next turn asks the
/// memory's note again, a new session, so an open thread due by then
/// (`LAMP_MEMORY_THREADS`) reaches them (#1693 (1)). Off, the note is asked
/// again only when the person changes or the worker restarts.
pub const REOPEN_QUIET_ENVIRONMENT: &str = "LAMP_MEMORY_REOPEN_QUIET";

/// Whether the service's environment says `LAMP_SECOND_SCOPED_CONTEXT=on`.
pub fn scoped_context_from_environment() -> bool {
    std::env::var(SCOPED_CONTEXT_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Where the coordinator reads whether a slow first round makes a sound
/// (sixth-P3, [`super::CUE_AFTER`]). `on` only.
pub const WAIT_CUE_ENVIRONMENT: &str = "LAMP_SECOND_WAIT_CUE";
/// The worker's flag for it, right after the second route's configuration.
pub const WAIT_CUE_FLAG: &str = "--wait-cue";

/// Where the coordinator reads whether a turn the chat never began says
/// [`super::DOWN_LINE`]. `on` only.
pub const DOWN_LINE_ENVIRONMENT: &str = "LAMP_SECOND_DOWN_LINE";
/// The worker's flag for it.
pub const DOWN_LINE_FLAG: &str = "--down-line";

/// The worker's flag when this route is the lamp's only voice
/// (`LAMP_VOICE_ROUTE=second`): it says no [`super::ANNOUNCEMENT`], as there
/// is no other voice to differ from, says due reminders, and takes its
/// input late as the Live worker does.
pub const ONLY_VOICE_FLAG: &str = "--only-voice";

/// Where the coordinator reads which text model this route asks in place of
/// the configured chat: `gemini` (Google's API, directly), `openrouter`
/// (OpenRouter) or `deepseek` (DeepSeek's own API). Off unset or empty; any
/// other value is refused at start.
pub const CHAT_ENVIRONMENT: &str = "LAMP_SECOND_CHAT";
/// Where the coordinator reads whether this route's speech is Gemini's own
/// speech model (`gemini`), the configured voice when it fails. As
/// [`CHAT_ENVIRONMENT`].
pub const SPEECH_ENVIRONMENT: &str = "LAMP_SECOND_SPEECH";
pub const GEMINI_SPEECH_FLAG: &str = "--gemini-speech";
/// Where the coordinator reads which web search the model may use:
/// `deepseek` (DeepSeek's Search & Fetch, as functions), `openrouter`
/// (OpenRouter's web plugin, with `LAMP_SECOND_CHAT=openrouter` only) or
/// `exa` (`second.json`'s `search_url`). As [`CHAT_ENVIRONMENT`].
pub const SEARCH_ENVIRONMENT: &str = "LAMP_SECOND_SEARCH";

/// The text model in place of the configured chat (`LAMP_SECOND_CHAT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatRoute {
    Gemini,
    OpenRouter,
    DeepSeek,
}

/// The lamp's one voice (`LAMP_SECOND_SPEECH`): every sentence it says, its
/// wait's sounds, its reminders and the skills' say, and never another's
/// (the owner, 19 Oct: "it should be one voice per lamp").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeechRoute {
    Gemini,
    ElevenLabs,
    OpenAi,
}

/// The web search the model may use (`LAMP_SECOND_SEARCH`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchRoute {
    DeepSeek,
    OpenRouter,
    Exa,
}

/// The second route's own flags, given right after its configuration, each
/// only when on, in the order of the fields.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RouteFlags {
    /// `LAMP_SECOND_WAIT_CUE=on`.
    pub wait_cue: bool,
    /// `LAMP_SECOND_DOWN_LINE=on`.
    pub down_line: bool,
    /// `LAMP_VOICE_ROUTE=second`, set by the coordinator's routes.
    pub only_voice: bool,
    /// `LAMP_SECOND_CHAT`.
    pub chat: Option<ChatRoute>,
    /// `LAMP_SECOND_SPEECH`: the lamp's one voice; `None`, the configured
    /// voice (`second.json`'s `tts_provider`).
    pub speech: Option<SpeechRoute>,
    /// `LAMP_SECOND_SEARCH`.
    pub search: Option<SearchRoute>,
}

/// Each choice's word in the environment, and its worker flag.
const CHATS: [(ChatRoute, &str, &str); 3] = [
    (ChatRoute::Gemini, "gemini", "--gemini-chat"),
    (ChatRoute::OpenRouter, "openrouter", "--openrouter-chat"),
    (ChatRoute::DeepSeek, "deepseek", "--deepseek-chat"),
];
const SPEECHES: [(SpeechRoute, &str, &str); 3] = [
    (SpeechRoute::Gemini, "gemini", GEMINI_SPEECH_FLAG),
    (SpeechRoute::ElevenLabs, "elevenlabs", "--elevenlabs-speech"),
    (SpeechRoute::OpenAi, "openai", "--openai-speech"),
];
const SEARCHES: [(SearchRoute, &str, &str); 3] = [
    (SearchRoute::DeepSeek, "deepseek", "--deepseek-search"),
    (SearchRoute::OpenRouter, "openrouter", "--openrouter-search"),
    (SearchRoute::Exa, "exa", "--exa-search"),
];

/// One of `table`'s words for `value`; nothing unset or empty; why not.
fn choice<T: Copy>(
    name: &str,
    value: Option<std::ffi::OsString>,
    table: &[(T, &str, &str)],
) -> Result<Option<T>, String> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    table
        .iter()
        .find(|(_, word, _)| value == *word)
        .map(|(route, ..)| Some(*route))
        .ok_or_else(|| {
            let words: Vec<&str> = table.iter().map(|(_, word, _)| *word).collect();
            format!(
                "{name} is {} or nothing, not {value:?}: the voice does not start",
                words.join(", ")
            )
        })
}

impl RouteFlags {
    /// Whether the lamp's one voice is Gemini's.
    pub fn gemini_speech(&self) -> bool {
        self.speech == Some(SpeechRoute::Gemini)
    }

    /// The provider the configured voice is asked of: the one chosen, or
    /// `second.json`'s; `None` when the voice is Gemini's.
    pub fn voice_provider(
        &self,
        configured: crate::second_config::TtsProvider,
    ) -> Option<crate::second_config::TtsProvider> {
        use crate::second_config::TtsProvider;
        match self.speech {
            Some(SpeechRoute::Gemini) => None,
            Some(SpeechRoute::ElevenLabs) => Some(TtsProvider::ElevenLabs),
            Some(SpeechRoute::OpenAi) => Some(TtsProvider::OpenAi),
            None => Some(configured),
        }
    }

    /// The switches (all but `only_voice`) as `var` gives them from the
    /// service's environment (`coordinator::second`). The two `on` switches
    /// are on with `on` only, as before; the others with one of their words,
    /// off unset or empty, and refused with anything else, as is OpenRouter's
    /// search without OpenRouter's chat, which it rides on.
    pub fn from_values(var: impl Fn(&str) -> Option<std::ffi::OsString>) -> Result<Self, String> {
        let on = |name| var(name).is_some_and(|value| value == "on");
        let route = Self {
            wait_cue: on(WAIT_CUE_ENVIRONMENT),
            down_line: on(DOWN_LINE_ENVIRONMENT),
            only_voice: false,
            chat: choice(CHAT_ENVIRONMENT, var(CHAT_ENVIRONMENT), &CHATS)?,
            speech: choice(SPEECH_ENVIRONMENT, var(SPEECH_ENVIRONMENT), &SPEECHES)?,
            search: choice(SEARCH_ENVIRONMENT, var(SEARCH_ENVIRONMENT), &SEARCHES)?,
        };
        if route.search == Some(SearchRoute::OpenRouter)
            && route.chat != Some(ChatRoute::OpenRouter)
        {
            return Err(format!(
                "{SEARCH_ENVIRONMENT}=openrouter is OpenRouter's web plugin and needs {CHAT_ENVIRONMENT}=openrouter: the voice does not start"
            ));
        }
        Ok(route)
    }

    /// The worker's flags for these switches, in order.
    pub fn flags(&self) -> Vec<&'static str> {
        let mut flags = Vec::new();
        for (on, flag) in [
            (self.wait_cue, WAIT_CUE_FLAG),
            (self.down_line, DOWN_LINE_FLAG),
            (self.only_voice, ONLY_VOICE_FLAG),
        ] {
            if on {
                flags.push(flag);
            }
        }
        flags.extend(
            CHATS
                .iter()
                .filter(|(route, ..)| self.chat == Some(*route))
                .map(|(.., flag)| *flag),
        );
        flags.extend(
            SPEECHES
                .iter()
                .filter(|(route, ..)| self.speech == Some(*route))
                .map(|(.., flag)| *flag),
        );
        flags.extend(
            SEARCHES
                .iter()
                .filter(|(route, ..)| self.search == Some(*route))
                .map(|(.., flag)| *flag),
        );
        flags
    }

    /// Take the leading flags off the worker's, in order.
    pub fn take(mut flags: &[String]) -> (Self, &[String]) {
        let mut next = |flag: &str| match flags {
            [first, rest @ ..] if first == flag => {
                flags = rest;
                true
            }
            _ => false,
        };
        let wait_cue = next(WAIT_CUE_FLAG);
        let down_line = next(DOWN_LINE_FLAG);
        let only_voice = next(ONLY_VOICE_FLAG);
        let chat = CHATS
            .iter()
            .find(|(.., flag)| next(flag))
            .map(|(chat, ..)| *chat);
        let speech = SPEECHES
            .iter()
            .find(|(.., flag)| next(flag))
            .map(|(speech, ..)| *speech);
        let search = SEARCHES
            .iter()
            .find(|(.., flag)| next(flag))
            .map(|(search, ..)| *search);
        (
            Self {
                wait_cue,
                down_line,
                only_voice,
                chat,
                speech,
                search,
            },
            flags,
        )
    }
}

/// Whether this route checks short sounds: as the Live route does, only
/// with `--sounds-checked` and `--attention` together.
pub fn checks_sounds(files: &crate::provider_worker::Files) -> bool {
    files.attention && files.sounds_checked
}

/// The `worker second` flags: the Live provider worker's, as
/// `coordinator::provider_arguments` passes them, parsed by the same code,
/// but for `--keep-link`: this route has no link to keep.
pub fn parse_flags(dir: &Path, flags: &[String]) -> io::Result<crate::provider_worker::Parsed> {
    let parsed = crate::provider_worker::parse_flags(dir, flags)?;
    if parsed.2.keep_link {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the second voice route has no link to keep",
        ));
    }
    Ok(parsed)
}

/// The key for Gemini's API: the voice unit's credential `gemini.key`
/// (`LoadCredential=gemini.key:<file>`, in `credentials`, its
/// `CREDENTIALS_DIRECTORY`), as OpenRouter's and DeepSeek's are; without
/// it, the Live route's own when that is a Google API key. A credential
/// there but unreadable (not private, a link) is refused, never passed
/// over. A relay's key is never sent to Google: without a Google key the
/// worker does not start.
pub fn gemini_key(
    provider: &crate::config::ProviderConfig,
    credentials: Option<&std::ffi::OsStr>,
) -> io::Result<crate::second_config::Key> {
    use crate::second_config::{GEMINI_CREDENTIAL, unit_credential};
    match unit_credential(credentials, GEMINI_CREDENTIAL) {
        Ok(key) => return Ok(key),
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        Err(_) => {}
    }
    match provider.google_credential_file() {
        Some(file) => crate::second_config::credential(file),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LAMP_SECOND_CHAT=gemini and LAMP_SECOND_SPEECH=gemini call Google directly and need a Google API key: the voice unit's credential gemini.key (LoadCredential=gemini.key:<file>); the Live route's key goes to a relay and is not given to Google",
        )),
    }
}

/// DeepSeek's key for its chat and search: the person's own when the
/// route has it (`LAMP_OWN_KEYS`, #2094), else the relay's credential
/// (`llm_credential_file`) when they go through the relay
/// (`deepseek_base_url`, #1934), else the voice unit's `deepseek.key`.
pub fn deepseek_key(
    config: &crate::second_config::SecondConfig,
    credentials: Option<&std::ffi::OsStr>,
) -> io::Result<crate::second_config::Key> {
    use crate::second_config::{DEEPSEEK_CREDENTIAL, unit_credential};
    if let Some(own) = &config.hosted.own_deepseek {
        return Ok(own.clone());
    }
    match config.hosted.deepseek_base_url {
        Some(_) => Ok(config.llm_key.clone()),
        // Another relay than the company's gives no address of its own
        // (#2978): say what else would do (El's P3).
        None => unit_credential(credentials, DEEPSEEK_CREDENTIAL).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("{error}; or, on a relay, its DeepSeek address as second.json's deepseek_base_url"),
            )
        }),
    }
}

/// The memory's observation pass (#1693 (2), `LAMP_MEMORY_OBSERVE`): this
/// route's DeepSeek model, thinking off, on the relay or DeepSeek's own API
/// as the route has it, so a person's conversations go to one outside model
/// (the first builder on #1693). Asked once per conversation, on the
/// memory's observation thread with a runtime of its own, within
/// [`crate::remember::OBSERVE_DEADLINE`]; never on a turn's path.
fn memory_observer(
    config: &crate::second_config::SecondConfig,
    credentials: Option<&std::ffi::OsStr>,
) -> io::Result<crate::remember::Observer> {
    let key = deepseek_key(config, credentials)?;
    let base = config
        .hosted
        .deepseek_base_url
        .clone()
        .unwrap_or_else(|| crate::second_config::DEEPSEEK_BASE_URL.to_owned());
    observer_at(&base, &config.hosted.deepseek_model, key.expose())
}

/// [`memory_observer`]'s model at `base` (DeepSeek's API, or the relay's),
/// for the pass and for its dry run (`lamp-voice-eval observe-dry`, #1693
/// (4)), so the dry run asks exactly as the lamp will.
pub fn observer_at(base: &str, model: &str, key: &str) -> io::Result<crate::remember::Observer> {
    let chat =
        crate::second_provider::net::Chat::deepseek_at(base, model, key, Arc::new(Vec::new), false)
            .map_err(|error| io::Error::other(error.to_string()))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    Ok(Box::new(move |input| {
        runtime.block_on(async {
            tokio::time::timeout(
                crate::remember::OBSERVE_DEADLINE,
                ask_observations(&chat, input),
            )
            .await
            .ok()
            .flatten()
        })
    }))
}

/// One observation pass: the prompt, the job as quoted data, and the
/// model's whole answer read as [`crate::remember::observe_output`]. A job
/// given its day (`"today"`, `LAMP_MEMORY_EVENTS`) is asked for an event's
/// day too ([`crate::remember::observe_prompt`]); any other as before.
async fn ask_observations(
    chat: &crate::second_provider::net::Chat,
    input: &serde_json::Value,
) -> Option<serde_json::Value> {
    let mut stream = chat
        .send(&[
            Message::system(crate::remember::observe_prompt(
                input.get("today").is_some(),
            )),
            Message::user(input.to_string()),
        ])
        .await
        .ok()?;
    let mut text = String::new();
    while let Some(event) = stream.next().await.ok()? {
        match event {
            crate::second_provider::net::ChatEvent::Text(piece) => text.push_str(&piece),
            crate::second_provider::net::ChatEvent::Done => break,
            _ => {}
        }
    }
    crate::remember::observe_output(&text)
}

/// The status the person's own DeepSeek key was last refused with, 0 while
/// it never was in this run (#2094): what `voice-keys` tells lamp-health.
static OWN_KEY_REFUSED: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);

/// The health part that says whether the person's own key still works.
pub const OWN_KEY_HEALTH: &str = "voice-keys";
/// Its publisher's name on the bus, which takes `[a-z][a-z0-9_]{0,31}`
/// only: `voice-keys` itself is refused there, so the part never reported
/// (found by the test of the reviewer's P2 on #2167).
pub const OWN_KEY_PUBLISHER: &str = "voice_keys";

/// The warning lamp-health shows once the person's key was refused.
pub const OWN_KEY_REFUSED_WORDS: &str = "your DeepSeek key was refused, using the lamp's own";

/// A request the person's own key was refused for: one journal line, and
/// the health warning from then on. Never the key.
fn own_key_refused(status: u16) {
    OWN_KEY_REFUSED.store(status, std::sync::atomic::Ordering::Relaxed);
    eprintln!(
        "lamp-voice second: DeepSeek: the person's own key was refused (HTTP {status}); this request goes through the relay (LAMP_OWN_KEYS)"
    );
}

/// `voice-keys`' report: well, with whose DeepSeek key it uses (`uses`:
/// `own` or `lamp`), or the warning once the person's own key (`own`) was
/// refused. Without the person's key it is well, `uses=lamp`, so a part
/// that reported once never goes quiet when the key is removed (the
/// reviewer's P2 on #2167).
pub fn own_key_health(own: bool, refused: u16) -> lamp_msgs::Diagnostic {
    match (own, refused) {
        (false, _) => lamp_msgs::Diagnostic::ok(OWN_KEY_HEALTH).with("uses", "lamp"),
        (true, 0) => lamp_msgs::Diagnostic::ok(OWN_KEY_HEALTH).with("uses", "own"),
        (true, status) => lamp_msgs::Diagnostic::new(
            lamp_msgs::Level::Warn,
            OWN_KEY_HEALTH,
            OWN_KEY_REFUSED_WORDS,
        )
        .with("status", status),
    }
}

/// `voice-keys`' report at this worker's start, if it reports at all:
/// whenever the switch is on, with the person's DeepSeek key (`own`) or
/// without it (the reviewer's P2 on #2167). Off, none.
pub fn own_key_report(switch_on: bool, own: bool) -> Option<lamp_msgs::Diagnostic> {
    switch_on.then(|| own_key_health(own, 0))
}

/// `voice-keys` stopping on purpose (#2415's P3), said at this worker's
/// start with the switch off: own keys turned off at runtime restart the
/// voice, and the part a run before reported would otherwise stay quiet
/// until lamp-health restarts, counted as a part that stopped working (Not
/// feeling well's droop). lamp-health forgets a part that never reported,
/// so on a lamp that never had the switch on it changes nothing. On, none:
/// the part reports.
pub fn own_key_stopping_at_start(switch_on: bool) -> Option<lamp_msgs::Diagnostic> {
    (!switch_on).then(|| lamp_msgs::Diagnostic::stopping(OWN_KEY_HEALTH))
}

/// `voice-keys` stopping on purpose when the second route's worker is
/// dropped (#960): the voice goes on with Gemini alone, and the part that
/// worker reported is gone with it, not failing. With the switch off it
/// never reported, so none.
pub fn own_key_stopping_on_drop(switch_on: bool) -> Option<lamp_msgs::Diagnostic> {
    switch_on.then(|| lamp_msgs::Diagnostic::stopping(OWN_KEY_HEALTH))
}

/// `report` said once under `voice-keys`' publisher, if there is a bus;
/// nothing, and no error, if not.
pub fn say_own_key_once(bus_root: Option<&Path>, report: &lamp_msgs::Diagnostic) {
    let Some(mut out) = bus_root
        .and_then(|root| lamp_bus::Bus::new(root).ok())
        .and_then(|bus| {
            bus.publisher_declared(
                lamp_msgs::Diagnostic::TOPIC,
                &lamp_msgs::Diagnostic::declaration(),
                OWN_KEY_PUBLISHER,
            )
            .ok()
        })
    else {
        return;
    };
    let _ = out.publish_json(lamp_ipc::monotonic_us(), report);
}

/// The voice's own side of [`own_key_stopping_on_drop`]: with the bus on
/// (`LAMP_BUS=on`) and `LAMP_OWN_KEYS=on` in its environment, `voice-keys`
/// is said to be stopping once the second worker was dropped.
pub fn own_key_dropped_from_environment() {
    let switch_on =
        std::env::var_os(crate::second_config::OWN_KEYS_ENV).is_some_and(|value| value == "on");
    let Some(report) = own_key_stopping_on_drop(switch_on) else {
        return;
    };
    let flags = crate::bus_out::worker_arguments_from_environment();
    if let Ok((root, _)) = crate::bus_out::take_root(&flags) {
        say_own_key_once(root.as_deref(), &report);
    }
}

/// With `LAMP_OWN_KEYS=on` and a bus: `voice-keys` reported to lamp-health
/// every second, as a running part reports, for as long as this worker
/// runs, with the person's DeepSeek key (`own`) or without it. A key kept
/// and then removed restarts the voice without one, and the part goes on
/// reporting, so it is never stale for good. Without a bus, nothing.
fn report_own_key(bus_root: Option<&Path>, own: bool) {
    let Some(mut health) = bus_root
        .and_then(|root| lamp_bus::Bus::new(root).ok())
        .and_then(|bus| {
            bus.publisher_declared(
                lamp_msgs::Diagnostic::TOPIC,
                &lamp_msgs::Diagnostic::declaration(),
                OWN_KEY_PUBLISHER,
            )
            .ok()
        })
    else {
        return;
    };
    std::thread::spawn(move || {
        loop {
            let refused = OWN_KEY_REFUSED.load(std::sync::atomic::Ordering::Relaxed);
            let _ = health.publish_json(lamp_ipc::monotonic_us(), &own_key_health(own, refused));
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

/// How the journal names where DeepSeek is asked: never a URL. With the
/// person's own key, its host, so a changed endpoint shows in every start's
/// line, not only the day it was set (Third's P3 on #2167).
pub(crate) fn deepseek_where(hosted: &crate::second_config::Hosted) -> String {
    if hosted.own_deepseek.is_some() {
        format!(
            "with the person's own key, at {} (LAMP_OWN_KEYS)",
            crate::second_config::host(
                hosted
                    .deepseek_base_url
                    .as_deref()
                    .unwrap_or(crate::second_config::DEEPSEEK_BASE_URL)
            )
        )
    } else if hosted.deepseek_base_url.is_some() {
        "through the relay with its credential (deepseek_base_url, LAMP_SECOND_DEEPSEEK_BASE_URL)"
            .to_owned()
    } else {
        "on DeepSeek's own API".to_owned()
    }
}

/// The real services as `route` sets them: the configured chat and voice,
/// or Gemini's, OpenRouter's or DeepSeek's in their place, each saying so in
/// the journal once. OpenRouter's and DeepSeek's keys are the voice unit's
/// credentials, in `credentials` (its `CREDENTIALS_DIRECTORY`).
pub fn services(
    mut config: crate::second_config::SecondConfig,
    provider: &crate::config::ProviderConfig,
    route: RouteFlags,
    credentials: Option<&std::ffi::OsStr>,
    declare: super::net::Declare,
) -> io::Result<Net> {
    use crate::second_config::{OPENROUTER_CREDENTIAL, unit_credential};
    let gemini = config.gemini.clone();
    let deepseek = (route.chat == Some(ChatRoute::DeepSeek))
        .then(|| deepseek_key(&config, credentials))
        .transpose()?;
    let hosted = config.hosted.clone();
    // The lamp's one voice (#1712). A provider chosen over the configured
    // one takes its own default voice: a configured voice's name is that
    // provider's.
    if let Some(provider) = route.voice_provider(config.tts_provider)
        && provider != config.tts_provider
    {
        config.tts_provider = provider;
        config.tts_voice = None;
    }
    let fallback = config.tts_provider.name();
    let configured_model = config
        .tts_model
        .clone()
        .unwrap_or_else(|| super::net::default_model(config.tts_provider).to_owned());
    let key = (route.chat == Some(ChatRoute::Gemini) || route.gemini_speech())
        .then(|| gemini_key(provider, credentials))
        .transpose()?;
    let routable = relay_routable(&config, route, key.as_ref());
    let mut net = Net::new(config, declare).map_err(io::Error::other)?;
    let call = |model| {
        key.as_ref().map(|key| super::services::GeminiCall {
            base_url: &gemini.base_url,
            key: key.expose(),
            model,
        })
    };
    match route.chat {
        None => {}
        Some(ChatRoute::Gemini) => {
            if let Some(call) = call(&gemini.chat_model) {
                net = net.chatting_with_gemini(&call).map_err(io::Error::other)?;
            }
            eprintln!(
                "lamp-voice second: the text model is Gemini ({}, thinking {}), called directly (LAMP_SECOND_CHAT=gemini)",
                gemini.chat_model,
                super::net::GEMINI_REASONING
            );
        }
        Some(ChatRoute::OpenRouter) => {
            let key = unit_credential(credentials, OPENROUTER_CREDENTIAL)?;
            let search = route.search == Some(SearchRoute::OpenRouter);
            net = net
                .chatting_with_openrouter(key.expose(), search)
                .map_err(io::Error::other)?;
            eprintln!(
                "lamp-voice second: the text model is {} through OpenRouter, the fastest provider first, reasoning {}{} (LAMP_SECOND_CHAT=openrouter)",
                hosted.openrouter_model,
                hosted.openrouter_effort,
                if search {
                    ", searching the web on Exa before every answer (LAMP_SECOND_SEARCH=openrouter)"
                } else {
                    ""
                }
            );
        }
        Some(ChatRoute::DeepSeek) => {
            if let Some(key) = &deepseek {
                net = net
                    .chatting_with_deepseek(key.expose())
                    .map_err(io::Error::other)?;
            }
            // The person's own key refused later (a rotation, #2094): that
            // request goes through the relay it went over, never a failed
            // answer, said in the journal and to lamp-health.
            if hosted.own_deepseek.is_some()
                && let Some(relay) = &hosted.relay_deepseek_base_url
            {
                net = net
                    .falling_back_to_the_relay(relay, Arc::new(own_key_refused))
                    .map_err(io::Error::other)?;
            }
            eprintln!(
                "lamp-voice second: the text model is DeepSeek's {} {}, thinking off (LAMP_SECOND_CHAT=deepseek)",
                hosted.deepseek_model,
                deepseek_where(&hosted)
            );
        }
    }
    if !route.gemini_speech() {
        eprintln!(
            "lamp-voice second: the voice is {fallback}, model {configured_model}, the lamp's one voice"
        );
    }
    if route.gemini_speech()
        && let Some(call) = call(&gemini.speech_model)
    {
        net = net
            .speaking_with_gemini(&call, provider.voice())
            .map_err(io::Error::other)?;
        eprintln!(
            "lamp-voice second: the voice is Gemini ({}, voice {}), the lamp's one voice (LAMP_SECOND_SPEECH=gemini)",
            gemini.speech_model,
            provider.voice()
        );
    }
    // The chat's connection opened as a line begins (#1712).
    let warm = std::env::var(super::services::CHAT_WARM_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    if warm {
        eprintln!(
            "lamp-voice second: the chat's connection is opened as a line begins (LAMP_SECOND_CHAT_WARM)"
        );
    }
    // The chat's idle connections kept briefly
    // (`LAMP_SECOND_CHAT_KEPT=on`; the first builder's 02:14 on #2602).
    if std::env::var(super::net::CHAT_KEPT_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on")
    {
        eprintln!(
            "lamp-voice second: the chat keeps an idle connection {} s at most (LAMP_SECOND_CHAT_KEPT)",
            super::net::CHAT_POOL_IDLE.as_secs()
        );
    }
    // The configured voice's connection opened as a line begins (#1712).
    let warm_speech = std::env::var(super::services::SPEECH_WARM_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    if warm_speech {
        eprintln!(
            "lamp-voice second: the configured voice's connection is opened as a line begins (LAMP_SECOND_SPEECH_WARM)"
        );
    }
    // A speech socket opened as each answer begins (#2135's row).
    let speech_socket = std::env::var(super::services::SPEECH_SOCKET_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    if speech_socket {
        eprintln!(
            "lamp-voice second: a speech socket is opened as each answer begins, for its first piece (LAMP_SECOND_SPEECH_SOCKET)"
        );
    }
    // The relay's near addresses, asked in the background (#2882).
    let relay_routes = std::env::var(super::services::RELAY_ROUTES_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    if relay_routes {
        let may = |on: bool| {
            if on {
                "may be near"
            } else {
                "stays at its own"
            }
        };
        eprintln!(
            "lamp-voice second: the relay is asked for near addresses, never on a turn's way: chat {}, speech {}, listening {} (LAMP_RELAY_ROUTES)",
            may(routable.chat),
            may(routable.speech),
            may(routable.listening)
        );
        net = net.routing_through_the_relay(routable);
    }
    Ok(net
        .warming_the_chat(warm)
        .warming_the_speech(warm_speech)
        .speaking_on_a_socket(speech_socket))
}

/// What the relay's near addresses may take (`LAMP_RELAY_ROUTES`, #2882):
/// the chat when it is DeepSeek through the relay (a `deepseek_base_url`,
/// not DeepSeek's own API) with its key, speech when
/// the configured voice speaks, listening when it is the relay's; speech and
/// listening only with the relay's key, so a key of another service's in
/// `second.json` never goes to the relay's near address; the route's own
/// Gemini, chat or voice, only when it already goes through the relay with
/// the relay's key (`gemini_key`, never a Google key); for none whose address
/// the voice unit's environment gave.
pub(crate) fn relay_routable(
    config: &crate::second_config::SecondConfig,
    route: RouteFlags,
    gemini_key: Option<&crate::second_config::Key>,
) -> super::services::Routable {
    super::services::Routable {
        chat: route.chat == Some(ChatRoute::DeepSeek)
            && config.hosted.own_deepseek.is_none()
            && config.hosted.deepseek_base_url.is_some()
            && !config.pinned.chat,
        speech: !route.gemini_speech() && config.tts_key == config.llm_key && !config.pinned.speech,
        listening: matches!(&config.stt, crate::second_config::Stt::Autonomous { key, .. } if *key == config.llm_key)
            && !config.pinned.listening,
        gemini: (route.chat == Some(ChatRoute::Gemini) || route.gemini_speech())
            && gemini_key == Some(&config.llm_key)
            && config.gemini.base_url != crate::second_config::GEMINI_BASE_URL
            && !config.pinned.gemini,
    }
}

/// The web search the model may call as its own functions, as `route` sets
/// it: DeepSeek's Search & Fetch (its key the unit's `deepseek.key`), or the
/// search service `second.json` names. OpenRouter's rides on its chat.
pub fn search_for(
    config: &crate::second_config::SecondConfig,
    route: RouteFlags,
    credentials: Option<&std::ffi::OsStr>,
) -> io::Result<Option<Search>> {
    let search = match route.search {
        None | Some(SearchRoute::OpenRouter) => return Ok(None),
        Some(SearchRoute::DeepSeek) => {
            let key = deepseek_key(config, credentials)?;
            let base = config
                .hosted
                .deepseek_base_url
                .as_deref()
                .unwrap_or(crate::second_config::DEEPSEEK_BASE_URL);
            eprintln!(
                "lamp-voice second: DeepSeek's search {}",
                deepseek_where(&config.hosted)
            );
            Search::deepseek_at(base, key.expose())
        }
        Some(SearchRoute::Exa) => {
            let service = config.search.as_ref().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "LAMP_SECOND_SEARCH=exa needs search_url and search_credential_file in the second route's configuration",
                )
            })?;
            Search::exa(&service.url, service.key.expose(), service.extra.clone())
        }
    }
    .map_err(io::Error::other)?;
    let names: Vec<String> = search
        .declarations()
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    eprintln!(
        "lamp-voice second: the model may search the web ({}; LAMP_SECOND_SEARCH)",
        names.join(", ")
    );
    Ok(Some(search))
}

/// The functions each chat request of this route is given, before a spoken
/// turn's name signal: the lamp's ([`crate::tools::uncut_within`] this route's
/// bound, so none of the lamp's own actions is left out for Live's),
/// [`crate::tools::THE_TIME`] only for elsewhere (the time here is told in
/// every request), then the search's, and with `look` (a spoken turn with a
/// full look) the look's ([`look_tool`]), within this route's own bound of
/// [`crate::second_provider::MAX_TOOLS`] ([`crate::tools::fit_within`]): cut by
/// [`chat_cut`], never a core skill's function nor the look, which is one
/// of the core (the first builder on #2602, 22 Oct 12:3x: added after the
/// bound, a list already at it left the look out, and nothing said so).
pub fn chat_tools(
    tools: &crate::tools::Settings,
    search: &[lamp_gemini::Tool],
    look: bool,
) -> Vec<lamp_gemini::Tool> {
    let places = tools.clock && tools.places;
    let (lamp, skills, mut core) =
        crate::tools::uncut_within(tools, crate::second_provider::MAX_TOOLS);
    let mut told = time_elsewhere(lamp, places);
    told.extend(search.iter().cloned());
    if look {
        told.push(look_tool());
        core.insert(crate::second_provider::LOOK_CALL.to_owned());
    }
    crate::tools::fit_within(
        told,
        &chat_cut(skills),
        &core,
        crate::second_provider::MAX_TOOLS,
    )
}

/// The order the bound cuts this route's functions in (the first builder
/// on #2614 and #2644): the search's fetch, then the time elsewhere, then
/// `skills` ([`crate::tools::uncut`]'s cut: `reports_state` with
/// `LAMP_REPORTS=on`, then those not a core skill's), then the weather
/// (`LAMP_SECOND_WEATHER=on`), then the search. A core skill's function,
/// `reports_stop` and the lamp's own actions are never in it.
pub fn chat_cut(skills: Vec<String>) -> Vec<String> {
    let first = [super::net::search::FETCH, crate::tools::THE_TIME];
    first
        .into_iter()
        .map(str::to_owned)
        .chain(skills)
        .chain([super::net::weather::NAME, super::net::search::NAME].map(str::to_owned))
        .collect()
}

/// What [`crate::tools::THE_TIME`] is told on this route (#1712): each
/// request is told the time here as it is asked
/// ([`crate::local_time::told_asked_here`]), so the function is only for
/// the time in another place.
pub(crate) const TIME_ELSEWHERE: &str = "Call this only for the time somewhere else, such as \"what time is it in London\": give the place as its time zone. The time here is in what you were told, read as you were asked: answer the time here, the date and how long until something from that, never with this. Then say it as it gives it \"said aloud\", with its hour and the place it names, and never say a time unless you were asked for it.";

/// The lamp's functions with [`crate::tools::THE_TIME`] only for another
/// place's time: told [`TIME_ELSEWHERE`], its place needed, with `places`
/// (`LAMP_TIME_PLACES`), and not given without, when it could say only the
/// time here that every request is told.
pub(crate) fn time_elsewhere(
    mut tools: Vec<lamp_gemini::Tool>,
    places: bool,
) -> Vec<lamp_gemini::Tool> {
    tools.retain_mut(|tool| {
        if tool.name != crate::tools::THE_TIME {
            return true;
        }
        tool.description = TIME_ELSEWHERE.into();
        tool.parameters = Some(serde_json::json!({
            "type": "object",
            "properties": {
                "place": {
                    "type": "string",
                    "description": "The time zone of the place asked about, as its IANA name: Europe/London, America/New_York, Asia/Tokyo."
                }
            },
            "required": ["place"]
        }));
        places
    });
    tools
}

/// Told after the character in spoken turns on this route, whose model reads
/// what was heard as words (#1712, the first builder's scoring of 19 Oct:
/// "what is the weather like in Hanoi" heard as "Hannover" or answered with
/// "You asking what the weather's like here?"; "Night!" at 1 pm, the
/// character's example; a poem of two lines).
pub(crate) const SPOKEN_NOTE: &str = "\n\nYou read what people say as words, and your ears turn some words into others, names and places most of all. When a question is plainly about the weather, a price, a time or a place, answer it for the likeliest meaning, here first, and search if you need to; ask back only when you cannot guess at all. Greet and say goodbye for the time of day it is now. A poem you are asked for has at least four lines.";

/// Why a line of a word or two is held ([`lone_words`]).
pub(crate) const LONE_WORDS: &str = "a word or two in a conversation, no question and no short word said to the lamp, and nothing shows it was said to the lamp: no name, nobody turned to it";

/// Whether a line let out only as the conversation going on is one or two
/// words, no question, and no short word said to the lamp
/// (`not_speech::short_word`: "louder", "thanks", "stop"): the room's,
/// not a request. On lamp-4ace on 19 Oct "What's the price of gold?" was
/// heard as "Surprise" and answered "Oh! I do like a surprise." (#1712). "And
/// Paris?" is a question, and goes on.
fn lone_words(heard: &str) -> bool {
    let words = heard
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .count();
    (1..=2).contains(&words) && !heard.contains('?') && !crate::not_speech::short_word(heard)
}

/// Told after [`SPOKEN_NOTE`] when the model looks by a call (#1712).
pub(crate) const LOOK_CALL_NOTE: &str = " When you would look, call take_a_look: it is how you look, and you will then be shown what you see.";

/// Told after [`SPOKEN_NOTE`] when the voice has a memory (#1712, the first
/// builder at 00:34 on #1722: on lamp-4ace, "remember my violet umbrella is
/// in the car" and "forget that" were answered in words, nothing kept or
/// forgotten; the memory note names only recall). Live called both.
/// "Forget it" about the talk itself is not a forget: it would delete what
/// they asked the lamp to keep (the reviewer and the first builder on
/// #2007, 01:17 and 01:18). But "please forget that" about something kept
/// is: worded only against "forget it", it was answered in words on
/// lamp-4ace, nothing forgotten (the first builder at 02:05 on #2007). So
/// both, as examples, in one line.
pub(crate) const MEMORY_CALLS_NOTE: &str = " When someone asks you to remember something, call remember: saying you will is not keeping it. When they ask you to forget something they told you, such as \"please forget that\" or \"forget what I just told you\", call forget; \"forget it, it doesn't matter\" or \"never mind\" about the conversation is not that. For something from an earlier conversation, call recall.";

/// Whether a hearing of the name at `at_us` with `score` is one since
/// `from_us`, as the rule takes a hearing for a line.
fn name_counts(score: f32, at_us: u64, from_us: u64) -> bool {
    score >= crate::called::LEAST_SCORE && at_us.saturating_add(crate::called::BEFORE_US) >= from_us
}

/// Told beside a spoken line that its words or the lamp's name heard in its
/// sound called the lamp for: answer it (the first builder on #1712, 18:09:
/// at 17:56 DeepSeek declined "Lamp, what time is it?" and "Hey Lamp, tell
/// me a short joke").
pub(crate) const BY_NAME_NOTE: &str =
    "(For you to know, not to answer: this was said to you by name: answer it.)";
/// Told beside every other spoken line: it may not be for the lamp, and
/// then it is left alone (17:26: DeepSeek answered what the rule let out,
/// where Gemini declined).
pub(crate) const DECLINE_NOTE: &str = "(For you to know, not to answer: much of what reaches you is not said to you. When this line is plainly people talking to each other, a remark to the room, someone on a call, a video or a noise, call not_for_me and say nothing; leaving it alone is right, not rude, and a person who wants you says so.)";

/// The note for a spoken line: [`BY_NAME_NOTE`] when it called the lamp by
/// name, else [`DECLINE_NOTE`].
pub(crate) fn call_note(by_name: bool) -> &'static str {
    if by_name { BY_NAME_NOTE } else { DECLINE_NOTE }
}

/// The character with [`SPOKEN_NOTE`] after it, and [`LOOK_CALL_NOTE`]
/// when the model looks by a call.
pub(crate) fn spoken_character(character: &str, look_by_call: bool, memory: bool) -> String {
    let look = if look_by_call { LOOK_CALL_NOTE } else { "" };
    let memory = if memory { MEMORY_CALLS_NOTE } else { "" };
    format!("{character}{SPOKEN_NOTE}{look}{memory}")
}

/// The look's function (#1712): a spoken turn with a full look looks by it.
pub(crate) fn look_tool() -> lamp_gemini::Tool {
    lamp_gemini::Tool {
        name: crate::second_provider::LOOK_CALL.into(),
        description: "Look with your eye at what is in front of you: when someone asks what you see, shows you something, or asks how they or something looks. Call it instead of answering; you will then be shown what you see, and answer from that. Never for looking something up on the web.".into(),
        parameters: None,
        silent: false,
    }
}

/// `tools`, already fitted, with the look's after them (the probe's list):
/// within this route's bound of [`crate::second_provider::MAX_TOOLS`], the last that isn't
/// the lamp's own leaves for it, never the look, and it is said.
pub(crate) fn with_look(tools: Vec<lamp_gemini::Tool>) -> Vec<lamp_gemini::Tool> {
    let mut tools = tools;
    tools.push(look_tool());
    let core = [crate::second_provider::LOOK_CALL.to_owned()].into();
    crate::tools::fit_within(tools, &[], &core, crate::second_provider::MAX_TOOLS)
}

/// A search begun from a request's words before the model asks (#1712):
/// the last request's, until its first `web_search` takes it, if that asks
/// nothing the words did not ([`early_covers`]).
#[derive(Default)]
pub(crate) struct Early {
    begun: Mutex<Option<EarlySearch>>,
}

/// One search begun early: its request, its query, when, and its answer to
/// come.
type EarlySearch = (
    u64,
    String,
    Instant,
    futures_util::future::Shared<futures_util::future::BoxFuture<'static, serde_json::Value>>,
);

impl Early {
    /// Begin `search` for `request`, in place of any earlier one. Driven on
    /// its own task, so it runs while the model's first round does.
    pub(crate) fn begin(
        &self,
        request: u64,
        query: String,
        search: futures_util::future::BoxFuture<'static, serde_json::Value>,
    ) {
        use futures_util::FutureExt;
        let shared = search.shared();
        tokio::spawn(shared.clone());
        if let Ok(mut begun) = self.begun.lock() {
            *begun = Some((request, query, Instant::now(), shared));
        }
    }

    /// The search begun for `request`, once: when it began, and its answer,
    /// when the model's `asked` query adds nothing to it. One asking more
    /// takes it too, and is told `Err` with the words it added, so its own
    /// query is searched: a "what's the weather this weekend" searched as
    /// heard found the forecasts of American towns, while the model had
    /// asked for the lamp's own city (lamp-4ace, 20 Oct).
    pub(crate) fn take(&self, request: u64, asked: &str) -> Option<Result<EarlyAnswer, usize>> {
        let mut begun = self.begun.lock().ok()?;
        match begun.take() {
            Some((of, query, at, answer)) if of == request => {
                Some(match early_adds(&query, asked) {
                    0 => Ok((at, answer)),
                    added => Err(added),
                })
            }
            other => {
                *begun = other;
                None
            }
        }
    }
}

/// A search begun early, as its request takes it: when it began, and its
/// answer.
pub(crate) type EarlyAnswer = (
    Instant,
    futures_util::future::Shared<futures_util::future::BoxFuture<'static, serde_json::Value>>,
);

/// How many words the model's `asked` query has that the `early` query
/// begun from the words heard has not, small words aside: 0 when the
/// early search answers what the model asked, in any order and with words
/// left out.
pub(crate) fn early_adds(early: &str, asked: &str) -> usize {
    const SMALL: [&str; 24] = [
        "a", "an", "the", "in", "at", "on", "of", "for", "to", "and", "is", "are", "what",
        "what's", "whats", "how", "me", "please", "tell", "check", "look", "up", "find", "out",
    ];
    let words = |text: &str| -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !(c.is_alphanumeric() || c == '\''))
            .filter(|word| !word.is_empty() && !SMALL.contains(word))
            .map(str::to_owned)
            .collect()
    };
    let had = words(early);
    words(asked)
        .iter()
        .filter(|word| !had.contains(word))
        .count()
}

/// The words of a plain ask for something new (news, a score or result, a
/// price or rate, opening hours, the weather) as a search's query, with the
/// lamp's name left off and the month and year after them; `None` for
/// anything else. A question, or a request to tell, check or look up.
pub(crate) fn early_query(heard: &str, month_and_year: Option<String>) -> Option<String> {
    let lower = heard.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .collect();
    let has = |word: &str| words.contains(&word);
    let phrase = |phrase: &str| lower.contains(phrase);
    let new = [
        "news",
        "headline",
        "headlines",
        "score",
        "scores",
        "result",
        "results",
        "price",
        "prices",
        "weather",
        "forecast",
        "temperature",
        "bitcoin",
        "stock",
        "stocks",
    ]
    .iter()
    .any(|word| has(word))
        || [
            "who won",
            "how much is",
            "exchange rate",
            "opening hours",
            "open today",
            "is it open",
            "latest",
            "newest",
        ]
        .iter()
        .any(|said| phrase(said));
    let asks = heard.contains('?')
        || ["tell me", "check", "look up", "find out", "search"]
            .iter()
            .any(|said| {
                lower
                    .trim_start_matches(|c: char| !c.is_alphanumeric())
                    .starts_with(said)
                    || phrase(&format!("lamp, {said}"))
            });
    if !(new && asks) {
        return None;
    }
    // The name the person called it by is no part of the query.
    let mut rest = heard.trim();
    for name in ["hey lamp", "hi lamp", "ok lamp", "okay lamp", "lamp"] {
        if rest.to_lowercase().starts_with(name) {
            rest = rest[name.len()..].trim_start_matches(|c: char| !c.is_alphanumeric());
            break;
        }
    }
    let query = match month_and_year {
        Some(when) => format!("{} {when}", rest.trim_end_matches(['?', '.', '!'])),
        None => rest.to_owned(),
    };
    (!query.is_empty() && query.chars().count() <= 128).then_some(query)
}

/// The weather the model may ask for, with `switch` (`LAMP_SECOND_WEATHER`)
/// exactly `on` (the owner on 22 Oct): looked up with `fetch` only when a
/// call asks, for the place and the day asked, the lamp's own place
/// (`here`) for a call that names none, its days told from that place's
/// own date by `clock`. Nothing is fetched here and no task is started;
/// with the switch off there is no weather at all.
fn weather_asked(
    switch: Option<&str>,
    here: Option<String>,
    fetch: crate::second_provider::net::weather::Fetch,
    clock: crate::second_provider::net::weather::Clock,
) -> Option<Arc<crate::second_provider::net::weather::Weather>> {
    use crate::second_provider::net::weather;
    weather::on(switch).then(|| Arc::new(weather::Weather::new(here, fetch, clock)))
}

/// The search's functions, then the weather's when it is on: what a spoken
/// turn's chat is given beside the lamp's ([`chat_tools`]).
fn with_weather_tool(searched: &[lamp_gemini::Tool], weather: bool) -> Vec<lamp_gemini::Tool> {
    let mut tools = searched.to_vec();
    if weather {
        tools.push(crate::second_provider::net::weather::declaration());
    }
    tools
}

/// One call of the weather's, answered as the search's are: within its
/// bound ([`crate::second_provider::net::weather::WITHIN`]), and journalled
/// with what it asked and its time, never a word the person said.
async fn weather_answer(
    weather: &crate::second_provider::net::weather::Weather,
    request: u64,
    args: &serde_json::Value,
) -> serde_json::Value {
    let began = Instant::now();
    let answered = weather.answer(args).await;
    eprintln!(
        "lamp-voice second: request {request} asked for weather {}; answered in {} ms, {}",
        answered.asked,
        began.elapsed().as_millis(),
        answered.how
    );
    answered.told
}

/// `runner` with the weather's call answered here first, by `weather`,
/// when there is one; every other call goes to `runner` as before.
pub(crate) fn answering_weather(
    runner: ToolRunner,
    weather: Option<Arc<crate::second_provider::net::weather::Weather>>,
) -> ToolRunner {
    let Some(weather) = weather else {
        return runner;
    };
    Arc::new(move |request, heard, name, args| {
        if name != crate::second_provider::net::weather::NAME {
            return runner(request, heard, name, args);
        }
        let weather = weather.clone();
        Box::pin(async move { weather_answer(&weather, request, &args).await })
    })
}

/// [`answering_weather`] for the runner with the admitted request's memory
/// authority: the weather's call needs none.
pub(crate) fn scoped_answering_weather(
    runner: crate::second_provider::ScopedToolRunner,
    weather: Option<Arc<crate::second_provider::net::weather::Weather>>,
) -> crate::second_provider::ScopedToolRunner {
    let Some(weather) = weather else {
        return runner;
    };
    Arc::new(move |request, heard, name, args, scope| {
        if name != crate::second_provider::net::weather::NAME {
            return runner(request, heard, name, args, scope);
        }
        let weather = weather.clone();
        Box::pin(async move { weather_answer(&weather, request, &args).await })
    })
}

/// [`answering_weather`] for a spoken request's private audio runner: the
/// weather's call offers no audio.
pub(crate) fn audio_answering_weather(
    runner: super::RequestedAudioRunner,
    weather: Option<Arc<crate::second_provider::net::weather::Weather>>,
) -> super::RequestedAudioRunner {
    let Some(weather) = weather else {
        return runner;
    };
    Arc::new(move |context, call, scope| {
        if call.name != crate::second_provider::net::weather::NAME {
            return runner(context, call, scope);
        }
        let weather = weather.clone();
        Box::pin(async move {
            crate::tools::ToolAnswer::plain(
                weather_answer(&weather, context.request, &call.args).await,
            )
        })
    })
}

/// [`answered_here`] with the weather's call too when it is on: a lookup,
/// never the lamp's action, so the Policy never holds it.
pub(crate) fn answered_here_with_weather(
    here: crate::second_provider::policy::AnsweredHere,
    weather: bool,
) -> crate::second_provider::policy::AnsweredHere {
    if !weather {
        return here;
    }
    Arc::new(move |name| name == crate::second_provider::net::weather::NAME || here(name))
}

/// The air as `air/state` says it, read every five seconds while the
/// voice runs (`LAMP_AIR_FACTS=on`): a fresh reading feeling stuffy or very
/// stuffy keeps the count going, anything else starts it again, as the Live
/// worker reads it.
async fn air_task(bus: lamp_bus::Bus, air: Arc<Mutex<crate::air_told::AirTold>>) {
    let mut every = tokio::time::interval(Duration::from_secs(5));
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        every.tick().await;
        let fresh_ms = crate::air_told::FRESH_FOR.as_millis() as u64;
        let now_ms = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        let reading = bus
            .latest(lamp_msgs::AirState::TOPIC)
            .ok()
            .flatten()
            .filter(|message| now_ms.saturating_sub(message.header.at_unix_ms) < fresh_ms)
            .and_then(|message| message.parse::<lamp_msgs::AirState>().ok());
        let stuffy = reading.is_some_and(|reading| {
            matches!(
                reading.feel,
                lamp_msgs::AirFeel::Stuffy | lamp_msgs::AirFeel::VeryStuffy
            )
        });
        if let Ok(mut air) = air.lock() {
            air.seen(
                stuffy,
                reading.and_then(|reading| reading.co2_ppm),
                monotonic_us(),
            );
        }
    }
}

/// `humans/seems` as the eyes say it, read every second while the voice runs
/// (`LAMP_SECOND_SEEMS=on`): a fresh read (within `HumanSeems::LASTS`)
/// with the time it was said; a FER+ read in `latest`, while both readers
/// run, breaks the run of Blendshape V2's (`crate::seems_told`).
async fn seems_task(bus: lamp_bus::Bus, seems: Arc<Mutex<crate::seems_told::SeemsTold>>) {
    let mut every = tokio::time::interval(Duration::from_secs(1));
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        every.tick().await;
        let fresh_ms = lamp_msgs::HumanSeems::LASTS.as_millis() as u64;
        let now_ms = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        let read = bus
            .latest(lamp_msgs::HumanSeems::TOPIC)
            .ok()
            .flatten()
            .filter(|message| now_ms.saturating_sub(message.header.at_unix_ms) < fresh_ms)
            .and_then(|message| {
                let said_ms = message.header.at_unix_ms;
                Some((message.parse::<lamp_msgs::HumanSeems>().ok()?, said_ms))
            });
        let faces = crate::known_faces::faces_in_view(&bus);
        if let Ok(mut seems) = seems.lock() {
            seems.seen_in_view(read, faces, monotonic_us());
        }
    }
}

/// The most requests remembered as given the air's note and not yet heard.
const AIR_GIVEN_MOST: usize = 4;

/// The air's note for a spoken request: due (stuffy ten minutes, not told
/// in three hours) and the room not `busy` at that moment. It is counted as
/// told only once that request's answer begins to be heard (the second
/// part), so a line cut short or retired before any word keeps it for the
/// next (the reviewer's P3 on #1960). In a busy room it is neither said nor
/// counted. `clock` is the monotonic clock in µs: [`monotonic_us`], or a
/// test's, which can't place ten minutes before a runner up for less (#1569
/// row 64).
fn air_noting(
    air: Arc<Mutex<crate::air_told::AirTold>>,
    busy: impl Fn(u64) -> bool + Send + Sync + 'static,
    clock: fn() -> u64,
) -> (
    crate::second_provider::TaskNote,
    crate::second_provider::AirHeard,
) {
    let spent = air.clone();
    noting_once(
        move |now| air.lock().ok()?.due(now),
        move |request, now| {
            if let Ok(mut air) = spent.lock() {
                air.told(now);
                eprintln!("lamp-voice second: request {request} is told the air is stuffy");
            }
        },
        busy,
        clock,
    )
}

/// How a known person seems, from their face (`LAMP_SECOND_SEEMS=on`,
/// `crate::seems_told`): given and spent as the air's note is
/// ([`air_noting`]), not in a busy room. Which requests may be given it (a
/// known person in view, spoken) is the engine's to say.
fn seems_noting(
    seems: Arc<Mutex<crate::seems_told::SeemsTold>>,
    busy: impl Fn(u64) -> bool + Send + Sync + 'static,
    clock: fn() -> u64,
) -> (
    crate::second_provider::TaskNote,
    crate::second_provider::AirHeard,
) {
    let spent = seems.clone();
    noting_once(
        move |now| seems.lock().ok()?.due(now),
        move |request, now| {
            if let Ok(mut seems) = spent.lock() {
                seems.told(now);
                eprintln!("lamp-voice second: request {request} is told how the person seems");
            }
        },
        busy,
        clock,
    )
}

/// A note `due` says, given to a request unless the room is `busy`, and
/// `told` (request, now) once any request given it begins to be heard.
fn noting_once(
    due: impl Fn(u64) -> Option<String> + Send + Sync + 'static,
    told: impl Fn(u64, u64) + Send + Sync + 'static,
    busy: impl Fn(u64) -> bool + Send + Sync + 'static,
    clock: fn() -> u64,
) -> (
    crate::second_provider::TaskNote,
    crate::second_provider::AirHeard,
) {
    // The requests given the note and not yet heard, the newest last: any of
    // their answers spends it (two can be given it before the first's answer
    // plays; the reviewer's P3 on #2083). A few at most.
    let given: Arc<Mutex<std::collections::VecDeque<u64>>> = Arc::default();
    let giving = given.clone();
    let note: crate::second_provider::TaskNote = Arc::new(move |request| {
        let now = clock();
        if busy(now) {
            return None;
        }
        let note = due(now)?;
        let mut giving = giving.lock().ok()?;
        if giving.len() == AIR_GIVEN_MOST {
            giving.pop_front();
        }
        giving.push_back(request);
        Some(note)
    });
    let heard: crate::second_provider::AirHeard = Arc::new(move |request| {
        let Ok(mut given) = given.lock() else { return };
        if !given.contains(&request) {
            return;
        }
        given.clear();
        told(request, clock());
    });
    (note, heard)
}

/// What the model is told of the time, with the lamp's place after it
/// ([`crate::local_time::place_told`]); the place alone when the time is not
/// known.
fn with_place(time: Option<String>, place: Option<String>) -> Option<String> {
    match (time, place) {
        (Some(time), Some(place)) => Some(time + &place),
        (time, place) => time.or_else(|| place.map(|place| place.trim_start().to_owned())),
    }
}

/// Who work can go to, read from the tasks part as the Live route reads
/// them (`provider_worker`, `agent_tasks::start`; #1722). Without it, on
/// lamp-4ace on 20 Oct with two targets (claude and codex) and
/// `LAMP_TASKS_WHO=on`, this route never read them: `start_agent_task` had
/// no `who`, "Ask codex to write a haiku about coffee." went to the target
/// the voice was given, as Claude Code, and the journal had no "work can go
/// to" line.
fn start_tasks(tools: &crate::tools::Settings) {
    if let Some(place) = &tools.tasks {
        crate::agent_tasks::start(place);
    }
}

/// The two boots a second worker runs under, named so neither can stand in
/// for the other: the coordinator's, whose authority snapshots its gate
/// takes (`run`, `Gate::new`), and its own, a fresh one per spawn
/// (`process.rs`, `spawn_with_tick`), which stamps its requested-audio
/// offers and keys its memory scope, so the coordinator, which knows the
/// worker by that boot (`rig.provider.boot`), takes them. With the
/// coordinator's on both, every offer on the second route was refused as
/// "from another worker boot" (lamp-4ace, 24 Oct, #2793); with the worker's
/// on both, the gate would refuse the coordinator's authority (reviewer-two's
/// P1 on #3319).
#[derive(Clone, Copy, Debug)]
pub struct WorkerBoots {
    pub coordinator: BootId,
    pub own: BootId,
}

/// The second route's worker process until Stop. `files` are the Live
/// provider worker's flags; this route uses the thread's file and the
/// tools (the memory among them), and has no use yet for the rest. With
/// `route.only_voice` (`--only-voice`) it is the lamp's only voice.
pub fn serve(
    channels: WorkerChannels,
    boots: WorkerBoots,
    provider_config: &Path,
    second_config: &Path,
    files: crate::provider_worker::Files,
    route: RouteFlags,
) -> io::Result<()> {
    let provider = crate::config::ProviderConfig::load(provider_config)?;
    let character = provider.instruction(false)?;
    // The unit's Gemini endpoint and voice over the file's (#1712).
    let config = crate::second_config::load_from_path(second_config)?
        .with_environment(|name| std::env::var_os(name))?;
    // The person's own keys over the relay, with LAMP_OWN_KEYS=on (#2094).
    let own_keys_on =
        std::env::var_os(crate::second_config::OWN_KEYS_ENV).is_some_and(|value| value == "on");
    let (config, lines) = config.with_own_keys(
        own_keys_on,
        std::env::var_os(crate::second_config::CREDENTIALS_ENV).as_deref(),
    );
    for line in lines {
        eprintln!("lamp-voice second: {line}");
    }
    if own_key_report(own_keys_on, config.hosted.own_deepseek.is_some()).is_some() {
        report_own_key(
            files.bus_root.as_deref(),
            config.hosted.own_deepseek.is_some(),
        );
    } else if let Some(stopping) = own_key_stopping_at_start(own_keys_on) {
        say_own_key_once(files.bus_root.as_deref(), &stopping);
    }
    // As the lamp's only voice, its link follows whether the services'
    // hosts answer (#1844, `link.rs`): the hosts checked.
    let probe_targets = route.only_voice.then(|| probe_targets(&config));
    if config.gemini.base_url != crate::second_config::GEMINI_BASE_URL {
        eprintln!(
            "lamp-voice second: Gemini at {}, voice {}",
            config.gemini.base_url, config.gemini.speech_model
        );
    }
    if !config.stt_key_terms.is_empty() {
        eprintln!(
            "lamp-voice second: speech to text expects {} words beside the name ({})",
            config.stt_key_terms.len(),
            crate::second_config::KEY_TERMS_ENV
        );
    }
    // The names the lamp knows, listened for beside the unit's words, with
    // `LAMP_SECOND_KNOWN_NAMES=on` (#753): read beside the turns.
    crate::second_provider::net::key_terms::start_from_environment(files.tools.tasks.is_some());
    let checked = checks_sounds(&files);
    let tools = files.tools;
    start_tasks(&tools);
    // As the Live route says them, when this route is the only voice.
    let reminders = route.only_voice && tools.schedule_speak;
    if reminders {
        eprintln!("lamp-voice second: due reminders are said on this route, the lamp's only voice");
    }
    if let Some(socket) = &tools.memory {
        // As the Live route starts it: each known person's memory apart.
        crate::remember::start(
            socket.clone(),
            tools.who.as_deref().map(crate::known_faces::known_file_of),
        );
    }
    // The voice unit's credentials (`LoadCredential=`), passed to this
    // worker alone (`process.rs`).
    let credentials = std::env::var_os(crate::second_config::CREDENTIALS_ENV);
    // The memory's observation pass (#1693 (2)), with this route's model.
    if crate::remember::observe_on() && tools.memory.is_some() {
        match memory_observer(&config, credentials.as_deref()) {
            Ok(observer) => {
                crate::remember::observe_with(observer);
                eprintln!(
                    "lamp-voice second: at each conversation's end the memory's observation pass asks {} {} (LAMP_MEMORY_OBSERVE)",
                    config.hosted.deepseek_model,
                    deepseek_where(&config.hosted)
                );
            }
            Err(error) => eprintln!(
                "lamp-voice second: no observation pass, the memory keeps only what it is asked to: {error}"
            ),
        }
    }
    // A web search as functions of its own, with LAMP_SECOND_SEARCH:
    // declared beside the lamp's tools and answered here.
    let search = search_for(&config, route, credentials.as_deref())?.map(Arc::new);
    let searched = search
        .as_ref()
        .map(|search| search.declarations())
        .unwrap_or_default();
    // The weather for any place and day, with LAMP_SECOND_WEATHER=on (the
    // owner on 22 Oct): a function of its own, looked up only when it is
    // called, for the place asked or else the lamp's own (LAMP_PLACE, or
    // the place its zone names).
    let weather = weather_asked(
        std::env::var(crate::second_provider::net::weather::ENVIRONMENT)
            .ok()
            .as_deref(),
        crate::local_time::place_here().or_else(crate::local_time::zone_place_here),
        crate::second_provider::net::weather::open_meteo(),
        crate::second_provider::net::weather::wall_clock(),
    );
    if let Some(weather) = &weather {
        eprintln!(
            "lamp-voice second: the model may ask for the weather of any place and day, looked up at Open-Meteo only when it asks and kept {} minutes a place; here is {} ({})",
            crate::second_provider::net::weather::KEPT_FOR.as_secs() / 60,
            weather
                .here()
                .unwrap_or("not known, so a call must name its place"),
            crate::second_provider::net::weather::ENVIRONMENT
        );
    }
    let declared = tools.clone();
    // The time here is told in every request; the_time is for elsewhere.
    let clock = tools.clock;
    let places = clock && tools.places;
    // With `use_skill` the skills' lines are told after the character, as
    // the Live route tells them (#1668).
    let skills_told = tools.clone();
    let declare: crate::second_provider::net::Declare = Arc::new({
        let (declared, searched) = (declared.clone(), searched.clone());
        move || chat_tools(&declared, &searched, false)
    });
    // Texts from the Autonomous app (#1673): a client of their own, on the
    // same model, beside the spoken turns.
    let texts = match (&files.bus_root, files.messages) {
        (Some(root), true) => Some((
            crate::second_provider::net::Chat::new(&config, declare.clone())
                .map_err(io::Error::other)?,
            root.clone(),
        )),
        _ => None,
    };
    // With a full look, spoken turns look by a call of their own (#1712);
    // texts, which see nothing, are not offered it.
    let look_by_call = files.look.is_some() && files.look_full;
    // Spoken turns alone are given the weather's function: a text was never
    // told the weather (#1712), and its runner answers none.
    let spoken_searched = with_weather_tool(&searched, weather.is_some());
    let spoken_declare: crate::second_provider::net::Declare =
        Arc::new(move || chat_tools(&declared, &spoken_searched, look_by_call));
    // With `LAMP_NAME_SIGNAL=on`, spoken turns are offered `heard_my_name`
    // as the Live route's sessions are, and the journal says how long
    // before a line's first sound the model said it heard its name. Texts,
    // which nobody said, are not offered it.
    let spoken_declare = if files.name_signal {
        with_name_signal(spoken_declare)
    } else {
        spoken_declare
    };
    // A forget called unasked is held for their yes (`LAMP_FORGET_ASKED`):
    // the forget functions say so to spoken turns.
    let spoken_declare = with_held_forgets(spoken_declare);
    // A few emotion tags, given to the voice only when it reads them
    // (ElevenLabs, with a model that reads them, the speech socket's too),
    // with `LAMP_SECOND_SPEECH_TAGS=on` (#2901).
    let speech_tags = (std::env::var(crate::second_provider::SPEECH_TAGS_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on"))
    .then(|| {
        let provider = crate::second_config::TtsProvider::ElevenLabs;
        let socket = (std::env::var(crate::second_provider::SPEECH_SOCKET_ENVIRONMENT)
            .ok()
            .as_deref()
            == Some("on"))
        .then(|| {
            std::env::var(crate::second_provider::SPEECH_SOCKET_MODEL_ENVIRONMENT)
                .ok()
                .filter(|model| !model.trim().is_empty())
                .unwrap_or_else(|| super::net::default_model(provider).to_owned())
        });
        route.voice_provider(config.tts_provider) == Some(provider)
            && crate::second_provider::speech_tags::read_by(
                config
                    .tts_model
                    .as_deref()
                    .unwrap_or(super::net::default_model(provider)),
                socket.as_deref(),
            )
    });
    let net = services(
        config,
        &provider,
        route,
        credentials.as_deref(),
        spoken_declare,
    )?;
    // A spoken turn's few functions, the rest behind one (#1712).
    let net = if std::env::var(crate::second_provider::FEW_TOOLS_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on")
    {
        eprintln!(
            "lamp-voice second: a spoken turn's chat is given its few functions, and the rest once it calls {MORE} (LAMP_SECOND_FEW_TOOLS)"
        );
        net.narrowing(Arc::new(few_tools))
    } else {
        net
    };
    // A line whose own words ask one of the staff by name to do something
    // is handed over: its first round must call start_agent_task (#1722).
    let named = tools
        .tasks
        .clone()
        .filter(|_| crate::agent_tasks::named_ask_on())
        .map(|place| {
            eprintln!(
                "lamp-voice second: a line that asks one of the staff by name to do something is handed over: its first round must call {} ({})",
                crate::agent_tasks::START,
                crate::agent_tasks::NAMED_ASK_ENVIRONMENT
            );
            named_asks(place)
        });
    // A line that asks the lamp to remember the person's face, with their
    // name, must call remember_my_face: only with the eyes' faces and names
    // checked, so a name not in their words is still refused.
    let remember = (crate::known_faces::asked_on() && tools.who.is_some() && tools.names_checked)
        .then(|| {
            eprintln!(
                "lamp-voice second: a line that plainly asks the lamp to remember their face and gives their name must call {} in its first round ({})",
                crate::known_faces::REMEMBER,
                crate::known_faces::ASKED_ENVIRONMENT
            );
            remember_asks()
        });
    // A line that plainly asks the lamp to keep something must call
    // remember, so the lamp never says it kept what it didn't
    // (`LAMP_MEMORY_KEEP_ASKED`).
    let keep = crate::remember::asked::on().then(|| {
        eprintln!(
            "lamp-voice second: a line that plainly asks the lamp to keep something must call {} in its first round ({})",
            crate::remember::REMEMBER,
            crate::remember::asked::ENVIRONMENT
        );
        crate::remember::asked::force()
    });
    // A line that plainly asks for a timer or a reminder must call the
    // function that sets it (`LAMP_SCHEDULE_ASKED`).
    let set = crate::tools::schedule_asked::on().then(|| {
        eprintln!(
            "lamp-voice second: a line that plainly asks for a timer or a reminder must call {} or {} in its first round ({})",
            crate::tools::schedule_asked::TIMER,
            crate::tools::schedule_asked::REMINDER,
            crate::tools::schedule_asked::ENVIRONMENT
        );
        crate::tools::schedule_asked::force()
    });
    let net = match first_force(named, first_force(remember, first_force(keep, set))) {
        Some(force) => net.forcing(force),
        None => net,
    };
    let thread = files.thread;
    let file = thread.clone();
    // Each turn is one person's, as a Live session is (#764): whose it is,
    // counted from the eyes' known file as the Live worker counts it, opens
    // the thread for them and asks for their kept things. A Days note is
    // frozen for a session; here a session is the run of turns for one
    // person, so it is asked again only when the person changes, or, with
    // LAMP_MEMORY_REOPEN_QUIET, after ten quiet minutes (#1693 (1)).
    let socket = tools.memory.clone();
    let who = tools
        .who
        .as_deref()
        .map(crate::known_faces::known_file_of)
        .filter(|_| socket.is_some());
    let scope_file = who.clone();
    // With LAMP_FACE_GAP_HOLD, a known face lost for a few seconds with no
    // face in view keeps its memory scope (#2176 row 5b).
    // The eyes' faces in view, read beside the known file for that hold;
    // without them nothing can be held, so the hold is off.
    let faces_bus = crate::memory_scope::gap_from_environment()
        .and(files.bus_root.clone())
        .and_then(|root| lamp_bus::Bus::new(root).ok())
        .map(Arc::new);
    let face_gap = faces_bus
        .as_ref()
        .and(crate::memory_scope::gap_from_environment());
    match (crate::memory_scope::gap_from_environment(), &faces_bus) {
        (Some(_), Some(_)) => eprintln!(
            "lamp-voice second: a known face lost for a few seconds, with no face in view and while the lamp answers them, keeps its memory scope (LAMP_FACE_GAP_HOLD)"
        ),
        (Some(_), None) => eprintln!(
            "lamp-voice second: LAMP_FACE_GAP_HOLD is on but there is no bus to read the eyes' faces from, so no face is held"
        ),
        _ => {}
    }
    let face_cache = Arc::new(crate::memory_scope::FaceCache::holding_gaps(face_gap));
    // Cold from the voice's start (#2807): a room's line looks later for
    // its keeper until a face is named or `COLD_FOR` passes.
    face_cache.went_cold(Instant::now());
    let scoped_settings = tools.clone();
    let skill_settings = tools.clone();
    let days = crate::remember::days_from_environment();
    // For what memory holds that fits a turn's words (#2176 row 5c).
    let (hits_socket, hits_who) = (socket.clone(), who.clone());
    // Told after the character with `LAMP_TASKS_ASKED=on`, as the Live route is (#854).
    let tasks = tools.tasks.clone();
    // What a Live session is told of the moment, with `--session-fact` and
    // `--room-facts`: the room as Gemini's worker publishes it.
    let session_fact = files.session_fact;
    let room_bus = files
        .bus_root
        .as_deref()
        .filter(|_| files.room_facts)
        .and_then(|root| lamp_bus::Bus::new(root).ok());
    // With `--health-facts`, an unwell part, kept up from the bus by a
    // thread of its own; a turn reads only what it keeps (#1681).
    let health = files
        .bus_root
        .as_deref()
        .filter(|_| files.health_facts)
        .and_then(|root| lamp_bus::Bus::new(root).ok())
        .map(|bus| crate::health_told::watch(bus, crate::health_told::READ_EVERY));
    // What the texts' task is built from, kept before the spoken turns take
    // their own (`texts_task`).
    let texts_from = (character.clone(), tools.clone());
    // Spoken turns only: a text is typed, not heard (#1712).
    let character = spoken_character(&character, look_by_call, tools.memory.is_some());
    let character = if speech_tags == Some(true) {
        format!("{character}{}", crate::second_provider::speech_tags::NOTE)
    } else {
        character
    };
    let reopen_quiet =
        days && std::env::var(REOPEN_QUIET_ENVIRONMENT).ok().as_deref() == Some("on");
    if reopen_quiet {
        eprintln!(
            "lamp-voice second: after ten quiet minutes the next turn asks the memory's note again (LAMP_MEMORY_REOPEN_QUIET)"
        );
    }
    // One for the turn's notes and memory's hits alike, so both see the
    // session the turn is opened for (the reviewer's P2 on #2268).
    let sessions = Arc::new(Mutex::new(Sessions::reopening_after_quiet(reopen_quiet)));
    let hits_sessions = sessions.clone();
    // Whose a greeting turn is, for a dated follow-up's line (#2805).
    let (event_sessions, event_who) = (sessions.clone(), who.clone());
    // The Days note asked as each spoken line begins, off the loop (lamp-4ace,
    // 22 Oct 17:45).
    let note_ahead = socket
        .clone()
        .filter(|_| days)
        .map(|socket| Ahead::reading(sessions.clone(), who.clone(), socket));
    let place = crate::local_time::place_here();
    if let Some(place) = &place {
        eprintln!("lamp-voice second: the lamp's place is {place} (LAMP_PLACE)");
    }
    // What stays first, and the rest after the exchanges (#1712).
    let steady_first = std::env::var(crate::second_provider::STEADY_FIRST_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    if steady_first {
        eprintln!(
            "lamp-voice second: a spoken turn's request begins with the character and the skills, and the memory, the time, the room and the thread go after the exchanges (LAMP_SECOND_STEADY_FIRST)"
        );
    }
    // The known person's name, as the Live route tells it (#2539).
    let known_name = std::env::var(crate::second_provider::FACE_NAME_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    if known_name {
        eprintln!(
            "lamp-voice second: a turn that is the known face's own person is told their name, as the Live route tells it (LAMP_SECOND_FACE_NAME)"
        );
    }
    // The typed name, told only to the owner's line, with
    // LAMP_PERSON_NAME_OWNER_ONLY=on (#2602): out of the character, and
    // judged each turn on its own person and the faces in view.
    let setup_name = crate::person::for_lines();
    let setup_bus = setup_name
        .as_ref()
        .and(files.bus_root.as_deref())
        .and_then(|root| lamp_bus::Bus::new(root).ok());
    match (&setup_name, &setup_bus) {
        (Some(_), Some(_)) => eprintln!(
            "lamp-voice second: the typed name is told only to the owner's own known face (LAMP_PERSON_NAME_OWNER_ONLY)"
        ),
        (Some(_), None) => eprintln!(
            "lamp-voice second: LAMP_PERSON_NAME_OWNER_ONLY is on but there is no bus to read the faces from, so the typed name is never told"
        ),
        _ => {}
    }
    let told_for = memory_context(
        sessions,
        socket,
        who.clone(),
        days,
        move |now, person, line: Option<Option<String>>, memory| {
            let going_on = crate::thread::current(file.as_deref(), |thread| thread.going_on(now))
                .unwrap_or_default();
            let room = room_bus.as_ref().map(crate::bus_out::room_now);
            let skills = if skills_told.use_skill {
                crate::use_skill::section(crate::use_skill::offered(&crate::tools::skill_tools(
                    &skills_told,
                )))
            } else {
                String::new()
            };
            // The lamp's own place, with `LAMP_PLACE`, beside its time
            // (#1712). The weather is never told here: it is the weather's
            // function's to say, when asked (LAMP_SECOND_WEATHER).
            let time = with_place(
                crate::local_time::told_asked_here(clock, places),
                crate::local_time::place_told(place.as_deref()),
            );
            let time = crate::agent_tasks::noted(time, tasks.as_ref());
            let facts = facts(session_fact && !going_on, room)
                + &health_fact(health.as_deref())
                + &known_name_told(known_name, who.as_deref(), person.as_deref());
            // Beside the character, where the instruction had it: told after
            // the exchanges, the model used it for the owner a third as
            // often (EN, the #2602 probe).
            // The typed name follows this line's own lease only, never the
            // person the session is counted for (the reviewer's P2 on #3050:
            // the owner's sticky presence for 30 s after they go), and no
            // lease is no name.
            let told = |character: String| {
                with_typed_name(
                    character,
                    setup_name.as_deref(),
                    setup_bus.as_ref(),
                    line.as_ref().map(Option::as_deref),
                )
            };
            if steady_first {
                steady_first_message(
                    told(format!("{character}{skills}")),
                    &format!("{memory}{}{facts}", time.as_deref().unwrap_or_default()),
                    file.as_deref(),
                    person.as_deref(),
                )
            } else {
                system_message(
                    &told(character.clone()),
                    time.as_deref(),
                    &(memory + &facts + &skills),
                    file.as_deref(),
                    person.as_deref(),
                )
            }
        },
    );
    let told_for = Arc::new(told_for);
    let scoped = scoped_context_from_environment();
    let scoped_system = scoped.then(|| {
        let told_for = told_for.clone();
        Arc::new(move |lease: &crate::memory_scope::ScopeLease| {
            told_for(Some(lease), Chosen::ByLease)
        }) as crate::second_provider::ScopedSystem
    });
    // Without scoped context a turn is still told for the person counted
    // at the lamp, its lease read only for the typed name (#2602).
    let lease_system = (!scoped).then(|| {
        let told_for = told_for.clone();
        Arc::new(move |lease: &crate::memory_scope::ScopeLease| {
            told_for(Some(lease), Chosen::ByPresence)
        }) as crate::second_provider::ScopedSystem
    });
    let system = move || told_for(None, Chosen::ByPresence);
    // When the wait sound comes, with LAMP_SECOND_CUE_AFTER_MS (#2602): said
    // once here, and a value it can't take is said and left at 1.5 s.
    let cue_after = match crate::second_provider::cue_after(
        std::env::var(crate::second_provider::CUE_AFTER_ENVIRONMENT)
            .ok()
            .as_deref(),
    ) {
        Ok(Some(after)) => {
            if route.wait_cue {
                eprintln!(
                    "lamp-voice second: the wait sound comes {} ms after the words ({})",
                    after.as_millis(),
                    crate::second_provider::CUE_AFTER_ENVIRONMENT
                );
            }
            after
        }
        Ok(None) => crate::second_provider::CUE_AFTER,
        Err(why) => {
            eprintln!("lamp-voice second: {why}; the wait sound comes at 1500 ms");
            crate::second_provider::CUE_AFTER
        }
    };
    let staff = tools
        .tasks
        .clone()
        .filter(|_| files.staff_note)
        .map(staff_noting);
    let has_tasks = tools.tasks.is_some();
    // Whom a line's words ask by name, with `LAMP_TASKS_NAMED_ASK=on`: its
    // forced call goes to that one, and the journal says so (#1722).
    let named_ask = tools
        .tasks
        .clone()
        .filter(|_| crate::agent_tasks::named_ask_on());
    let making_skills = tools.tasks.as_ref().is_some_and(|place| place.make_skill);
    let declines = tools.decline;
    // A plain ask for news, a score, a price, hours or the weather starts
    // its search from the words heard, while the model's first round runs
    // (#1712).
    let early = search.as_ref().map(|_| Arc::new(Early::default()));
    // The lamp's actions wait for the Policy's decision (#800, #1712): with
    // the bus, and the route's rule, which knows each turn's name and
    // conversation. The Live route's battery line ("run after 0 ms") is
    // then the gate's own.
    let verdicts = policy_verdicts(
        files.policy_holds,
        files.attention,
        files.bus_root.as_deref(),
    );
    let policed = verdicts.is_some();
    let policy_settings = tools.clone();
    let policy_here = answered_here_with_weather(answered_here(search.clone()), weather.is_some());
    // A held line's `reports_stop` is done, its answer kept held, as on the
    // Live route (`crate::attention::DONE_THOUGH_HELD`), with LAMP_REPORTS=on.
    let though_held = tools.reports_stop;
    let remember_speaker = tools.remember_speaker;
    // The weather's call is answered before any of these, as the search's
    // is inside them, with LAMP_SECOND_WEATHER=on.
    let audio_runner = tools.requested_audio.then(|| {
        audio_answering_weather(
            requested_audio_runner(tools.clone(), search.clone(), early.clone(), policed),
            weather.clone(),
        )
    });
    let runner = answering_weather(
        tool_runner(tools, search.clone(), early.clone(), policed),
        weather.clone(),
    );
    // The same calls with the admitted request's memory authority (#1794).
    let scoped_runner = scoped_answering_weather(
        scoped_tool_runner(scoped_settings, search.clone(), early.clone(), policed),
        weather,
    );
    let system: crate::second_provider::System = Arc::new(system);
    // The relay's routes, asked on the runtime below (#2882).
    let routes_task = net.routes_task();
    let mut engine = Engine::new(
        Arc::new(net),
        {
            let system = system.clone();
            move || system()
        },
        runner.clone(),
    )
    .with_memory_scope(boots.own, face_cache.clone(), scoped_runner, scoped_system)
    .reading_the_lease_for_the_name(lease_system)
    .checking_sounds(checked)
    .telling_cut_ins(files.cut_in_told)
    .holding_hand_on(files.said_as_heard)
    .waiting_with_a_sound(route.wait_cue)
    .sounding_after(cue_after)
    .saying_when_down(route.down_line)
    .doing_though_held(though_held)
    .announcing(!route.only_voice)
    .saying_reminders(reminders);
    if let Some(ahead) = note_ahead {
        engine = engine.asking_the_note_ahead(ahead.told_as());
    }
    // The air, with `LAMP_AIR_FACTS=on` and the bus (#1712's map, item 6).
    let air = files
        .air_facts
        .then(|| {
            files
                .bus_root
                .as_deref()
                .and_then(|root| lamp_bus::Bus::new(root).ok())
        })
        .flatten()
        .map(|bus| {
            (
                bus,
                Arc::new(Mutex::new(crate::air_told::AirTold::default())),
            )
        });
    if let Some((_, told)) = &air {
        // The room this process keeps, as the Live worker reads its own.
        let busy = |now| {
            crate::room_told::room()
                .lock()
                .is_ok_and(|mut room| room.at(now) == crate::room_told::Room::Busy)
        };
        let (note, heard) = air_noting(told.clone(), busy, monotonic_us);
        engine = engine.noting_the_air(note, heard);
    }
    // How a known person seems, with `LAMP_SECOND_SEEMS=on` and the bus
    // (#1671): a note on tone only, never said.
    let seems = files
        .second_seems
        .then(|| {
            files
                .bus_root
                .as_deref()
                .and_then(|root| lamp_bus::Bus::new(root).ok())
        })
        .flatten()
        .map(|bus| {
            (
                bus,
                Arc::new(Mutex::new(crate::seems_told::SeemsTold::default())),
            )
        });
    if let Some((_, told)) = &seems {
        let busy = |now| {
            crate::room_told::room()
                .lock()
                .is_ok_and(|mut room| room.at(now) == crate::room_told::Room::Busy)
        };
        let (note, heard) = seems_noting(told.clone(), busy, monotonic_us);
        engine = engine.noting_how_they_seem(note, heard);
    }
    if let Some(runner) = audio_runner {
        engine = engine.requesting_audio(boots.own, runner);
    }
    if let Some(staff) = staff {
        engine = engine.noting_staff(staff);
    }
    // What memory holds that may fit a spoken turn's words (#2176 row 5c,
    // `LAMP_SECOND_MEMORY_HITS=on`, Days only): searched in the scope the
    // turn's note is told for, the known face's or the room's, within
    // `HITS_WITHIN`, and told as notes, never as sources. With
    // `LAMP_SECOND_SCOPED_CONTEXT`, in the request's own admitted scope (its
    // lease, narrowed to the room when another face spoke it), as its context
    // is told: never the sticky session's person.
    match (crate::remember::hits_on(), hits_socket) {
        (true, Some(socket)) if days => {
            eprintln!(
                "lamp-voice second: each spoken turn is told what memory holds that may fit its words ({})",
                crate::remember::HITS_ENVIRONMENT
            );
            let who = hits_who;
            engine = engine.remembering(Arc::new(move |request, heard, lease| {
                let started = Instant::now();
                let person = hits_whose(request, lease, &hits_sessions, who.as_deref(), started)?;
                let now_unix_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |since| since.as_millis() as u64);
                // The lamp's own zone, as memory's other notes read the day
                // (#820): each moment at its own offset.
                let note = crate::remember::hits_note_counted(
                    &socket,
                    person.as_deref(),
                    heard,
                    now_unix_ms,
                    crate::local_time::system(),
                );
                // What was told, counted, never said (#2510 take 1 c).
                eprintln!(
                    "lamp-voice second: {}",
                    crate::remember::hits_journal_line(
                        request,
                        note.as_ref().map(|(_, count)| *count),
                        started.elapsed().as_millis(),
                        person.is_some(),
                    )
                );
                note.map(|(note, _)| note)
            }));
        }
        (true, _) => eprintln!(
            "lamp-voice second: memory isn't told on each turn: {} needs the Days memory",
            crate::remember::HITS_ENVIRONMENT
        ),
        (false, _) => {}
    }
    // A dated follow-up the session was offered, asked about in its first
    // turn's own instructions when that line only greets
    // (`LAMP_MEMORY_EVENTS`, the first builder on #2805, 22 Oct 20:28, and
    // on #2602, 22 Oct 22:17): whose the turn is, as memory's hits read it.
    if crate::remember::events_on() {
        eprintln!(
            "lamp-voice second: a session that opens with only a greeting from someone offered a dated follow-up is told to ask about it now; one that opens with a request takes the offer back ({})",
            crate::remember::EVENTS_ENVIRONMENT
        );
        engine = engine.asking_after_events(
            Arc::new(move |request, heard, lease| {
                let person = hits_whose(
                    request,
                    lease,
                    &event_sessions,
                    event_who.as_deref(),
                    Instant::now(),
                )?;
                crate::remember::event_turn_note(person.as_deref(), heard)
            }),
            Arc::new(crate::remember::event_withheld),
        );
    }
    // Seeing someone speak (`LAMP_SECOND_SPEAKING_FACE=on`, the first
    // builder on #1722, 00:08): the eyes' faces, read every 100 ms on their
    // own thread, say whose a line is when another face spoke it. Only with
    // the memory's known file, the bus and the scoped context, which tells
    // and files each turn for its lease (Second on #2008); the reader stops
    // when this worker returns. Its faces are kept for a held forget's
    // sight below, so one thread reads the topic.
    let (engine_seeing, speaking_eyes, _speaking_reader) = see_who_speaks(
        engine,
        scope_file.as_ref(),
        files.bus_root.as_deref(),
        crate::speaking_faces::from_environment(),
        std::env::var(crate::second_provider::PERSON_SCOPE_SEEN_ENVIRONMENT)
            .is_ok_and(|value| value == "on"),
    );
    engine = engine_seeing;
    // Where the eyes read speaking (`LAMP_EYES_SPEAKING=on`, #2377): a held
    // forget's yes counts only from the held person's face seen saying it.
    // The faces seeing who speaks reads, when it is on, or else its own
    // reader, which reads the eyes' faces every 100 ms while this worker
    // runs; the judging happens only at a held forget's yes. Without the
    // memory's known file and the bus, a held forget runs on a yes as before,
    // and the journal says so once.
    let mut enrollment_eyes = speaking_eyes.clone();
    let _forget_sight_reader = match (&scope_file, files.bus_root.as_deref()) {
        (Some(known), Some(root)) if crate::speaking_faces::eyes_from_environment() => {
            let (eyes, reader) = match speaking_eyes {
                Some(eyes) => (Some(eyes), None),
                None => {
                    let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
                    let reader = crate::speaking_faces::read_from(eyes.clone(), root);
                    (reader.is_some().then_some(eyes), reader)
                }
            };
            match eyes {
                Some(eyes) => {
                    enrollment_eyes = Some(eyes.clone());
                    eprintln!(
                        "lamp-voice second: a held forget's yes counts only from their face seen saying it (LAMP_EYES_SPEAKING)"
                    );
                    engine = engine.confirming_forgets_by_sight(eyes, known.clone());
                }
                None => eprintln!(
                    "lamp-voice second: held forgets: the speaking check is off (the bus can't be opened); a plain yes runs as before"
                ),
            }
            reader
        }
        (known, root) => {
            if crate::speaking_faces::eyes_from_environment() {
                eprintln!(
                    "lamp-voice second: held forgets: the speaking check is off ({}); a plain yes runs as before",
                    match (known.is_some(), root.is_some()) {
                        (false, false) => "no bus, no known faces file",
                        (false, true) => "no known faces file",
                        _ => "no bus",
                    }
                );
            }
            None
        }
    };
    // Enrollment does not need a previously known face, but does need the
    // speaking topic. Reuse its reader when one is already running.
    let _enrollment_reader = if remember_speaker
        && crate::speaking_faces::eyes_from_environment()
        && enrollment_eyes.is_none()
    {
        files.bus_root.as_deref().and_then(|root| {
            let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
            let reader = crate::speaking_faces::read_from(eyes.clone(), root)?;
            enrollment_eyes = Some(eyes);
            Some(reader)
        })
    } else {
        None
    };
    engine = engine.remembering_speaker(remember_speaker, enrollment_eyes);
    if let (Some(early), Some(search)) = (early, search) {
        engine = engine.hearing_words(Arc::new(move |request, heard| {
            let Some(query) = early_query(heard, crate::local_time::month_and_year_here()) else {
                return;
            };
            eprintln!(
                "lamp-voice second: request {request}: a search begins from the words heard ({} characters)",
                query.chars().count()
            );
            let search = search.clone();
            early.begin(
                request,
                query.clone(),
                Box::pin(async move {
                    search
                        .answer(
                            crate::second_provider::net::search::NAME,
                            &serde_json::json!({ "query": query }),
                        )
                        .await
                }),
            );
        }));
    }
    // The answer's first piece goes after a few words, not the whole first
    // clause or sentence: Gemini's speech is asked a piece at a time, and the
    // configured voice waited for a 9-word clause on lamp-4ace (#1712).
    if let Some(words) = crate::second_provider::first_words_for(
        std::env::var(crate::second_provider::FIRST_WORDS_ENVIRONMENT)
            .ok()
            .as_deref(),
        route.gemini_speech(),
    ) {
        engine = engine.cutting_first_words(words);
    }
    if look_by_call {
        engine = engine.looking_by_call();
    }
    // A skill's answer that asks for a look takes the turn's full look, as
    // on the Live route (`LAMP_SECOND_SKILL_LOOK=on`, #941): only where the
    // model can look at all.
    let skill_look = std::env::var(crate::second_provider::SKILL_LOOK_ENVIRONMENT).ok();
    let skill_look = skill_look.as_deref();
    // A push on the body ends a skill's search here too, as lamp-skills
    // ends it (`body/hand`, #2904): only with the bus (`LAMP_BUS=on`), read
    // while this worker runs.
    let hand_reader = takes_skill_looks(skill_look, look_by_call)
        .then_some(files.bus_root.as_deref())
        .flatten()
        .and_then(|root| {
            let pushed = Arc::new(Mutex::new(crate::search_pushed::SearchPushed::default()));
            crate::search_pushed::read_from(pushed.clone(), root).map(|reader| (pushed, reader))
        });
    if takes_skill_looks(skill_look, look_by_call) {
        eprintln!(
            "lamp-voice second: a skill's answer that asks for a look takes the turn's full look (LAMP_SECOND_SKILL_LOOK)"
        );
        engine = engine.looking_for_skills(skill_call(skill_settings));
        if let Some((pushed, _)) = &hand_reader {
            engine = engine.stopping_searches_by_hand(pushed.clone());
        }
        // A look's turn whose answer called nothing is told so, so that a
        // skill's search the model left after one look gets one note to go
        // on (`LAMP_SECOND_LOOK_NOTE=on`).
        if std::env::var(crate::second_provider::LOOK_NOTE_ENVIRONMENT)
            .ok()
            .as_deref()
            == Some("on")
        {
            eprintln!(
                "lamp-voice second: a look's answer that calls nothing is told to the coordinator (LAMP_SECOND_LOOK_NOTE)"
            );
            engine = engine.noting_looks(true);
            // And a search's note turn left so, so the coordinator walks
            // the search on (`LAMP_SECOND_LOOK_WALK=on`).
            if std::env::var(crate::second_provider::LOOK_WALK_ENVIRONMENT)
                .ok()
                .as_deref()
                == Some("on")
            {
                eprintln!(
                    "lamp-voice second: a search's note answered with nothing called is told to the coordinator, which walks it on (LAMP_SECOND_LOOK_WALK)"
                );
                engine = engine.walking_looks(true);
            }
        }
    } else if skill_look == Some("on") {
        eprintln!(
            "lamp-voice second: LAMP_SECOND_SKILL_LOOK is on, but there is no full look (LAMP_LOOK_FULL and LAMP_EYES_LOOK_FILE): a skill's look is not taken"
        );
    }
    // The next line's speech to text kept connected (#1712), and the first
    // line's opened before it (#2135).
    if std::env::var(crate::second_provider::STT_WARM_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on")
    {
        eprintln!(
            "lamp-voice second: speech to text's socket for the next line is kept open, the first line's opened before it (LAMP_SECOND_STT_WARM)"
        );
        engine = engine.keeping_a_listener_ready(true);
    }
    // A line whose words end before a name waits for it (#2135).
    if std::env::var(crate::second_provider::OPEN_NAME_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on")
    {
        eprintln!(
            "lamp-voice second: a line whose words end before a name is answered no sooner than 2.5 s after its end (LAMP_SECOND_OPEN_NAME)"
        );
        engine = engine.waiting_for_a_name(true);
    }
    // A line that is only the lamp's name waits for the line after it, which
    // is then said by it.
    if std::env::var(crate::second_provider::NAME_CARRIES_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on")
    {
        eprintln!(
            "lamp-voice second: a line that is only the lamp's name, or short and begun with it, is answered no sooner than 2.5 s after its end, and a line begun before then is said by it when one person said both (LAMP_SECOND_NAME_CARRIES)"
        );
        engine = engine.carrying_the_name(true);
    }
    // The lamp's own words heard back are held as its echo, or taken out
    // of a mixed line's start or end (#2997).
    if std::env::var(crate::second_provider::OWN_ECHO_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on")
    {
        eprintln!(
            "lamp-voice second: a line begun over the lamp's answer or within 1.5 s after it, nothing but its own words in their order, is held as its echo; its own words at a line's start or end are taken out (LAMP_SECOND_OWN_ECHO)"
        );
        engine = engine.holding_own_echo(true);
    }
    // A spoken turn's chat begun on the line's early words (#1712).
    if std::env::var(crate::second_provider::PREPARED_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on")
    {
        eprintln!(
            "lamp-voice second: a spoken turn's chat begins on the line's early words (LAMP_SECOND_PREPARED)"
        );
        engine = engine.preparing(true);
    }
    // Whether a line's words so far end unfinished, told to the
    // coordinator, which holds its end a little longer then (#3026).
    if crate::unfinished_words::on() {
        eprintln!(
            "lamp-voice second: a line whose words so far end unfinished is told, and its end waits up to {} ms more ({})",
            u64::from(crate::unfinished_words::WAIT_BLOCKS) * 10,
            crate::unfinished_words::ENVIRONMENT
        );
        engine = engine.telling_unfinished_words(true);
    }
    // Each turn's tools in the journal, names and counts only (#2510).
    if crate::remember::days_from_environment()
        && std::env::var(crate::remember::REPLACES_ENVIRONMENT)
            .ok()
            .as_deref()
            == Some("on")
    {
        engine = engine.noting_turn_tools(true);
    }
    if let Some(place) = named_ask {
        engine = engine.asking_by_name(Arc::new(move |words: &str| {
            crate::agent_tasks::named_ask(&place, words)
        }));
    }
    // A hand-over waiting for its yes, with the tasks part (#1712).
    // And a made skill waiting for its yes to be put on (`crate::made_skill`).
    if has_tasks {
        engine = engine.noting_a_task_waiting(Arc::new(move |request| {
            crate::agent_tasks::pending_note(request, Instant::now()).or_else(|| {
                making_skills
                    .then(|| {
                        let now_unix_ms = SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |since| since.as_millis() as u64);
                        crate::made_skill::waiting_note(
                            std::path::Path::new(crate::made_skill::DIRECTORY),
                            now_unix_ms,
                        )
                    })
                    .flatten()
            })
        }));
    }
    // A look reads the eyes' look file with the Live route's bounds: a full
    // look's still may be older and larger (#836, #941).
    if let Some(path) = files.look.clone() {
        let (within, most) = if files.look_full {
            (crate::sight::FULL_FRESH, lamp_gemini::MAX_LOOK_IMAGE_BYTES)
        } else {
            (Duration::from_secs(1), lamp_gemini::MAX_IMAGE_BYTES)
        };
        engine = engine.looking(Arc::new(move || {
            crate::sight::fresh_up_to(&path, SystemTime::now(), within, most).map(|(jpeg, _)| jpeg)
        }));
    }
    // A holding-only answer is asked to go on, as on Live
    // (`LAMP_HOLDING_ANSWER=on`; the first builder's 22:54 on #1722).
    engine = engine.holding_answers(files.holding_answer);
    // Words said before a call are not said again after it
    // (`LAMP_SECOND_SAID_ONCE=on`; the first builder at 06:56 on #1722).
    let said_once = std::env::var(crate::second_provider::SAID_ONCE_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    if said_once {
        eprintln!(
            "lamp-voice second: a round's words said before its calls are told to the next round, which adds only what is new (LAMP_SECOND_SAID_ONCE)"
        );
    }
    engine = engine.saying_once(said_once);
    // "Not for me" written as words on a line let out only as the
    // conversation going on is the decline itself
    // (`LAMP_NOT_FOR_ME_WORDS=on`; lamp-4ace, r06, request 131).
    let declines_in_words = std::env::var(crate::second_provider::NOT_FOR_ME_WORDS_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    if declines_in_words {
        eprintln!(
            "lamp-voice second: a reply opening with \"not for me\" to a line let out only as the conversation going on is taken as not_for_me, its first piece held until its opening is known (LAMP_NOT_FOR_ME_WORDS)"
        );
    }
    engine = engine.declining_in_words(declines_in_words);
    if let Some(voiced) = speech_tags {
        eprintln!(
            "lamp-voice second: the chat is told a few speech tags; the voice is given {} (LAMP_SECOND_SPEECH_TAGS)",
            if voiced {
                "one listed tag a reply at most"
            } else {
                "none, it reads none"
            }
        );
    }
    engine = engine.speaking_tags(speech_tags);
    // A light or volume turn answered with its calls' own line when the next
    // round adds nothing (`LAMP_SHORT_DONE=on`, #2877).
    let short_done = std::env::var(crate::second_provider::SHORT_DONE_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    if short_done {
        eprintln!(
            "lamp-voice second: a turn whose every call is a light or volume change answered short is said with the calls' own line when the next round adds nothing (LAMP_SHORT_DONE)"
        );
    }
    engine = engine.answering_short(short_done);
    // A line said to the lamp by name never ends in silence from an empty
    // answer (`LAMP_NO_WORDS_AGAIN=on`, #3704).
    let no_words_again = std::env::var(crate::second_provider::NO_WORDS_AGAIN_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    if no_words_again {
        eprintln!(
            "lamp-voice second: a line begun with the lamp's name answered with no words and no call is asked once more, and said a short sorry if it has none again (LAMP_NO_WORDS_AGAIN)"
        );
    }
    engine = engine.asking_again_for_no_words(no_words_again);
    // The lamp's own written turns, news above all, may begin later than a
    // spoken one (`LAMP_SECOND_WRITTEN_WAIT=on`; #1854's follow-up).
    let written_wait = std::env::var(crate::second_provider::WRITTEN_WAIT_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on");
    if written_wait {
        eprintln!(
            "lamp-voice second: a turn the lamp wrote itself may begin as late as {} ms (LAMP_SECOND_WRITTEN_WAIT)",
            crate::second_provider::WRITTEN_BEGIN_WITHIN.as_millis()
        );
    }
    engine = engine.waiting_for_written(written_wait);
    // A line lost on the network is asked once more when a service host
    // answers again, the link's own check (`LAMP_RESEND_LOST_LINE=on`; the
    // first builder's 00:02 on #1722). As the only voice, with its hosts.
    if files.resend_lost_line
        && let Some(targets) = probe_targets.clone()
    {
        engine = engine.resending(Arc::new(move || {
            Box::pin(super::link::any_answers(targets.clone()))
        }));
    }
    // As the lamp's only voice this route keeps the room itself (#1712).
    if route.only_voice {
        engine = engine.keeping_room(keeping_room(
            files.attention && files.spoken_to,
            files.cut_ins_not_talk,
            std::env::var(crate::second_provider::BUSY_ENDS_SOONER_ENVIRONMENT)
                .is_ok_and(|value| value == "on"),
            files.room_file.as_deref(),
            files.bus_root.as_deref(),
        ));
    }
    if files.attention {
        let heeding = Arc::new(Mutex::new(Heeding::new(
            files.attention_busy,
            files.name_in_passing_ignored,
            files.bus_root.as_deref(),
            thread.clone(),
        )));
        let quiet_same_one =
            std::env::var(crate::second_provider::QUIET_GOING_ON_SAME_ONE_ENVIRONMENT)
                .is_ok_and(|value| value == "on");
        let not_own = std::env::var(crate::second_provider::GOING_ON_NOT_OWN_ENVIRONMENT)
            .is_ok_and(|value| value == "on");
        // The same-voice check (#829): its child loads its model now. Only
        // as the lamp's only voice: a standby beside Live loads none.
        let hears = checking_the_voice_for(
            &heeding,
            files.same_voice.clone().filter(|_| route.only_voice),
            files.spoken_to || quiet_same_one || not_own || crate::follows_face::on(),
            |model| {
                crate::same_voice::Prints::beside_voice(model)
                    .map(|prints| Box::new(prints) as Box<dyn crate::same_voice::Printer>)
            },
        );
        // The name heard only in the room's sound opens nothing alone
        // (`LAMP_NAME_SECOND_SIGN=on`, the first builder at 21:03 on
        // #1722).
        if std::env::var(crate::second_provider::NAME_SECOND_SIGN_ENVIRONMENT)
            .is_ok_and(|value| value == "on")
        {
            eprintln!(
                "lamp-voice second: the lamp's name heard only in the room's sound counts only with a second sign: a face turned to it, said close to it, or its words calling it (LAMP_NAME_SECOND_SIGN)"
            );
            if let Ok(mut heeding) = heeding.lock() {
                heeding.attention.ask_a_second_sign_of_the_name(true);
            }
        }
        // A hearing of the name inside one line's sound counts for that line
        // only (`LAMP_NAME_STAYS_IN_ITS_LINE=on`).
        if std::env::var(crate::second_provider::NAME_STAYS_ENVIRONMENT)
            .is_ok_and(|value| value == "on")
        {
            eprintln!(
                "lamp-voice second: a hearing of the lamp's name inside one line's sound counts for that line only, never for the line begun after it (LAMP_NAME_STAYS_IN_ITS_LINE)"
            );
            if let Ok(mut heeding) = heeding.lock() {
                heeding.keeping_the_name_in_its_line();
            }
        }
        // At a desk, a face turned to the lamp is no sign, and a
        // conversation goes on only shortly after its words
        // (`LAMP_NAME_OR_FOLLOW_UP=on`).
        if files.name_or_follow_up
            && let Ok(mut heeding) = heeding.lock()
        {
            eprintln!(
                "lamp-voice second: in a busy room, a face turned to the lamp lets no line out alone, and a conversation goes on only {} s after the lamp's own words; held talk and two faces make the room busy (LAMP_NAME_OR_FOLLOW_UP)",
                crate::name_or_follow_up::seconds(crate::name_or_follow_up::follow_up_for())
            );
            heeding.needing_name_or_follow_up();
            if let Ok(mut room) = crate::room_told::room().lock() {
                room.count_held_talk(true);
            }
            if std::env::var(crate::second_provider::TWO_FACES_HELD_ENVIRONMENT)
                .is_ok_and(|value| value == "on")
            {
                eprintln!(
                    "lamp-voice second: two faces make the room busy only when the newest look and {TWO_FACES_LOOKS} or more of the eyes' looks in the last {FACES_KEPT_MS} ms saw two, spanning {TWO_FACES_FOR_MS} ms or more, not on one look (LAMP_TWO_FACES_HELD)"
                );
                heeding.holding_two_faces(files.bus_root.as_deref());
            }
        }
        // Someone else's answer left alone (`LAMP_OTHERS_ANSWER=on`,
        // #2136): the rule learns which line began over the lamp's answer.
        if files.others_answer
            && let Ok(mut heeding) = heeding.lock()
        {
            heeding.leaving_others_answers();
        }
        // A conversation going on holds only for the one the lamp answered
        // (`LAMP_GOING_ON_SAME_ONE=on`, #2191).
        if files.going_on_same_one
            && let Ok(mut heeding) = heeding.lock()
        {
            heeding.going_on_only_for_the_same_one();
        }
        if let Some(hook) = hearing_hook(&heeding, hears, files.others_answer || declines) {
            engine = engine.hearing(hook);
        }
        // The name note, as the Live route gives it (`LAMP_NAME_NOTE=on`).
        if files.name_note
            && let Ok(mut heeding) = heeding.lock()
        {
            heeding.telling_the_name(
                files.name_note_busy_face,
                files.name_note && files.name_note_heard_as,
            );
        }
        if files.name_note {
            let heeding = heeding.clone();
            engine = engine.noting_the_name(Arc::new(move |request| {
                heeding.lock().ok()?.name_note_for(request)
            }));
        }
        // The name heard in a wordless piece counts for the line going on from
        // it without its sound sent again (#1712).
        {
            let heeding = heeding.clone();
            engine = engine.knowing_the_name_heard(Arc::new(move |from_us| {
                heeding
                    .lock()
                    .is_ok_and(|heeding| heeding.name_heard_since(from_us))
            }));
        }
        // Said by name, or perhaps not for the lamp: its note (#1712).
        {
            let heeding = heeding.clone();
            engine = engine.noting_whether_called(Arc::new(move |request, _| {
                heeding
                    .lock()
                    .ok()
                    .map(|heeding| heeding.call_note_for(request))
            }));
        }
        // In such a line, nothing set to come from people's talk (#1712).
        {
            let heeding = heeding.clone();
            engine = engine.holding_calls(Arc::new(move |request, name| {
                heeding
                    .lock()
                    .is_ok_and(|heeding| heeding.holds_call(request, name))
            }));
        }
        // In a quiet room, a conversation going on holds only for the one the
        // lamp answered (`LAMP_QUIET_GOING_ON_SAME_ONE=on`).
        if quiet_same_one {
            eprintln!(
                "lamp-voice second: in a quiet room, a line let out only as the conversation going on is held when another voice or another face than the caller's is heard or seen speaking it (LAMP_QUIET_GOING_ON_SAME_ONE)"
            );
            if let Ok(mut heeding) = heeding.lock() {
                heeding.going_on_quiet_only_for_the_same_one();
                // The eyes' speaking faces (`LAMP_EYES_SPEAKING` on
                // lamp-eyes): another face than the answered caller's seen
                // speaking a line holds it. Without them nothing reads
                // speaking, and only another voice holds.
                heeding.opening_the_eyes(files.bus_root.as_deref());
            }
        }
        // In a busy room, the one the lamp answered goes on by their own
        // voice or face (`LAMP_SAME_SPEAKER_CARRIES=on`).
        if std::env::var(crate::second_provider::SAME_SPEAKER_CARRIES_ENVIRONMENT)
            .is_ok_and(|value| value == "on")
        {
            eprintln!(
                "lamp-voice second: in a busy room, the one the lamp answered goes on for {} s after each answer by their own voice or face, and another voice or face is held, ending it at the second in a row (LAMP_SAME_SPEAKER_CARRIES)",
                SAME_SPEAKER_CARRIES_FOR.as_secs()
            );
            if let Ok(mut heeding) = heeding.lock() {
                heeding.carrying_the_same_speaker();
                // The eyes' speaking faces, the same reader as the switches
                // above; without them only the voice carries.
                heeding.opening_the_eyes(files.bus_root.as_deref());
            }
        }
        // A camera close or a mute: no face id outlives it.
        let forgets = heeding.clone();
        engine = engine.forgetting_on_privacy(Box::new(move || {
            if let Ok(mut heeding) = forgets.lock() {
                heeding.privacy_changed();
            }
        }));
        // A line let out only as the conversation going on: the model, which
        // reads only words, is told it may be people's talk, with the tool
        // it leaves such talk alone with (#1712).
        if declines {
            let heeding = heeding.clone();
            engine = engine.noting_the_conversation_alone(Arc::new(move |request| {
                heeding.lock().ok()?.conversation_note_for(request)
            }));
        }
        // A line that may follow on from the lamp is not told so in a busy
        // room, as the rule judged it (#1712).
        {
            let heeding = heeding.clone();
            engine = engine.judging_busy(Arc::new(move |request| {
                heeding
                    .lock()
                    .is_ok_and(|heeding| heeding.judged_busy(request))
            }));
        }
        engine = heeding_switches(
            engine,
            &heeding,
            std::env::var(crate::second_provider::NAMED_ALWAYS_ENVIRONMENT)
                .is_ok_and(|value| value == "on"),
            std::env::var(crate::second_provider::BUSY_CALLED_STRICT_ENVIRONMENT)
                .is_ok_and(|value| value == "on"),
        );
        // A face turned to the lamp but seen silent is nobody turned to it
        // (`LAMP_TURNED_SPEAKING=on`).
        if std::env::var(crate::second_provider::TURNED_SPEAKING_ENVIRONMENT)
            .is_ok_and(|value| value == "on")
        {
            eprintln!(
                "lamp-voice second: a face turned to the lamp is a sign only when the eyes don't see the only ones turned to it silent through the line (LAMP_TURNED_SPEAKING)"
            );
            if let Ok(mut heeding) = heeding.lock() {
                heeding.counting_only_turned_speakers();
                // The eyes' speaking faces (`LAMP_EYES_SPEAKING` on
                // lamp-eyes); without them nothing reads silent, and the
                // sign is as before.
                heeding.opening_the_eyes(files.bus_root.as_deref());
            }
        }
        // A face turned to the lamp lets a line out only when it is seen
        // speaking it, or the line is in the conversation's voice
        // (`LAMP_TURNED_FACE_SPEAKS=on`).
        if std::env::var(crate::second_provider::TURNED_FACE_SPEAKS_ENVIRONMENT)
            .is_ok_and(|value| value == "on")
        {
            eprintln!(
                "lamp-voice second: a face turned to the lamp lets a line out only when it is seen speaking it or the line is in the conversation's voice; one read and not seen speaking is nobody turned to it, and the line held is talk for the room; with no mouths read, as before (LAMP_TURNED_FACE_SPEAKS)"
            );
            if let Ok(mut heeding) = heeding.lock() {
                heeding.needing_the_turned_face_to_speak();
                heeding.opening_the_eyes(files.bus_root.as_deref());
            }
        }
        // What the caller said before their face was known follows them
        // (`LAMP_MEMORY_FOLLOWS_FACE=on`, #2176): each line's voice and the
        // faces seen speaking it tell whether it was theirs.
        if crate::follows_face::on() && days {
            eprintln!(
                "lamp-voice second: what the caller said before their face was known follows them once it is, by their voice or their face (LAMP_MEMORY_FOLLOWS_FACE)"
            );
            if let Ok(mut heeding) = heeding.lock() {
                heeding.opening_the_eyes(files.bus_root.as_deref());
            }
            // A saving line's print come after its verdict is waited for
            // (`follows_face::PRINT_WAIT`), filled in by the writer.
            let filling = heeding.clone();
            crate::follows_face::filling_prints(move |request| {
                filling
                    .lock()
                    .is_ok_and(|mut heeding| heeding.prints_filled(request))
            });
        }
        // The lamp's own answers to lines let out only as the conversation
        // going on keep no conversation going (`LAMP_GOING_ON_NOT_OWN=on`).
        if not_own {
            eprintln!(
                "lamp-voice second: the lamp's answers to lines let out only as the conversation going on keep no conversation going (LAMP_GOING_ON_NOT_OWN)"
            );
            if let Ok(mut heeding) = heeding.lock() {
                heeding.not_counting_own_answers();
                // Another face than the caller's seen speaking a line tells
                // it is another person's; the same reader as above.
                heeding.opening_the_eyes(files.bus_root.as_deref());
            }
        }
        // A line that cuts the one before it short is asked with that one's
        // words when one person said both (`LAMP_JOIN_CUT_WORDS`): by the
        // prints, when the same-voice check runs for a rule above, or by
        // one face seen speaking both, the same reader as above.
        if std::env::var(crate::second_provider::JOIN_CUT_WORDS_ENVIRONMENT)
            .is_ok_and(|value| value == "on")
        {
            eprintln!(
                "lamp-voice second: a line that cuts the one before it short, that one unanswered, is asked with its words when the same voice or one face said both, or, let out, when nothing shows a second person (LAMP_JOIN_CUT_WORDS)"
            );
            if let Ok(mut heeding) = heeding.lock() {
                heeding.opening_the_eyes(files.bus_root.as_deref());
                heeding.keeping_line_rooms();
            }
            let same = heeding.clone();
            engine = engine
                .joining_cut_words(Arc::new(move |cut, request| {
                    same.lock().map_or(
                        Joined::Not("the rule is not readable".to_owned()),
                        |mut heeding| heeding.same_speaker(cut, request),
                    )
                }))
                // A joined line's answer was never played: not history.
                .unsaying_joined(Arc::new(crate::thread::unsay));
        }
        // The lamp's name carries to the line after it only when one person
        // said both, as the join tells it (`LAMP_SECOND_NAME_CARRIES`): the
        // same reader of the eyes, the rooms kept for each line.
        if std::env::var(crate::second_provider::NAME_CARRIES_ENVIRONMENT)
            .is_ok_and(|value| value == "on")
        {
            if let Ok(mut heeding) = heeding.lock() {
                heeding.opening_the_eyes(files.bus_root.as_deref());
                heeding.keeping_line_rooms();
            }
            let (same, kept) = (heeding.clone(), heeding.clone());
            engine = engine.telling_one_speaker(
                Arc::new(move |name, request| {
                    same.lock().map_or(
                        Joined::Not("the rule is not readable".to_owned()),
                        |mut heeding| heeding.one_speaker(name, request),
                    )
                }),
                Arc::new(move |request, said| {
                    if let Ok(mut heeding) = kept.lock() {
                        heeding.keep_unjudged(request, said);
                    }
                }),
            );
        }
        // The rule keeps the route's one reader of the Policy's decisions,
        // which a line going on reads too (`LAMP_GOING_ON_ASKS`, below).
        if let Some(verdicts) = verdicts {
            if let Ok(mut heeding) = heeding.lock() {
                heeding.reading_the_policy(verdicts);
            }
            let (decided, facts) = (heeding.clone(), heeding.clone());
            let desk = crate::second_provider::policy::Desk::new(
                Arc::new(move |request| decided.lock().ok()?.policy_verdict(request)),
                policy_settings,
                files.policy_holds && files.follows_run,
                Arc::new(move |request| {
                    facts
                        .lock()
                        .map(|heeding| heeding.policy_turn(request))
                        .unwrap_or_default()
                }),
                policy_here,
            );
            eprintln!(
                "lamp-voice second: the lamp's actions wait for the Policy's decision about their turn, at most {} ms (LAMP_POLICY_HOLDS_ACTIONS)",
                crate::policy_holds::WAIT.as_millis()
            );
            engine = engine.holding_for_the_policy(Arc::new(desk).gate());
        }
        {
            let heeding = heeding.clone();
            engine = engine.telling_what_it_did(Arc::new(move |did| {
                if let Ok(mut heeding) = heeding.lock() {
                    heeding.lamp_did(did);
                }
            }));
        }
        // Before the levels' sampling begins, which reads the eyes beside
        // them for it. Only as the lamp's only voice, so a standby beside
        // Live adds no second `attention/addressed` publisher.
        if files.addressed
            && route.only_voice
            && let (Ok(mut heeding), Some(root)) = (heeding.lock(), files.bus_root.as_deref())
        {
            heeding.judging_addressed(root);
        }
        if files.attention_near
            && let (Ok(mut heeding), Some(root)) = (heeding.lock(), files.bus_root.as_deref())
        {
            heeding.hear_levels(root, true);
        }
        // Right after a start, the ears' quiet room shows the room quiet for
        // the name's join, never just unknown (`LAMP_FIRST_SECONDS_QUIET`,
        // #3711). After `--addressed`'s looks are set up, so the levels'
        // thread reads the eyes beside them (the reviewer-lead's P3 on #3801).
        if std::env::var(crate::second_provider::FIRST_SECONDS_QUIET_ENVIRONMENT)
            .is_ok_and(|value| value == "on")
        {
            eprintln!(
                "lamp-voice second: right after a start, a line in a room not yet known is kept as said in a quiet room for a join when the ears' stream stayed near its own quiet outside the lines, no talk was left alone, no line was held and no other voice heard since (LAMP_FIRST_SECONDS_QUIET)"
            );
            if let Ok(mut heeding) = heeding.lock() {
                heeding.showing_the_first_seconds_quiet();
                if let Some(root) = files.bus_root.as_deref() {
                    heeding.hear_levels(root, false);
                }
            }
        }
        // A line's first word, set off and sounding like the lamp's name, is
        // the name when lamp-name heard it near at the line's start
        // (`LAMP_NAME_NEAR`): both signs, never one. After `--addressed`'s
        // own looks are set up, so one reader of the eyes keeps them for
        // both (Sixth's P3 on c0890e8db).
        if std::env::var(lamp_msgs::NameHeard::NEAR_ENVIRONMENT).is_ok_and(|value| value == "on") {
            if heeding
                .lock()
                .is_ok_and(|mut heeding| heeding.hearing_near())
            {
                eprintln!(
                    "lamp-voice second: a line's first word, set off and sounding like the lamp's name, is the name when lamp-name heard it near (0.5 or more) within 1.3 s of the line's start (LAMP_NAME_NEAR)"
                );
                // Whose face is turned is read from the eyes' speaking faces
                // when they read speaking (`LAMP_EYES_SPEAKING`).
                if let Ok(mut heeding) = heeding.lock() {
                    heeding.opening_the_eyes(files.bus_root.as_deref());
                    heeding.turned_near_speaking(crate::speaking_faces::eyes_from_environment());
                    if let Some(root) = files.bus_root.as_deref() {
                        heeding.looking_for_near(root);
                    }
                }
                let near = heeding.clone();
                engine = engine.hearing_the_name_near(Arc::new(move |window: (u64, u64)| {
                    near.lock().map_or((None, false), |mut heeding| {
                        (
                            heeding.near_name(window.0),
                            heeding.turned_to_the_lamp(window),
                        )
                    })
                }));
            } else {
                eprintln!(
                    "lamp-voice second: a first word like the lamp's name is never taken as it: no bus to read lamp-name's near hearings from (LAMP_NAME_NEAR)"
                );
            }
        }
        // In a busy room, a line going on waits for the Policy (#1522), as
        // on the Live route: with the rule and the bus.
        if files.going_on_asks
            && let Some(root) = files.bus_root.as_deref()
        {
            if let Ok(mut held) = heeding.lock() {
                held.asking_the_policy(root);
            }
            let heeding = heeding.clone();
            engine = engine.asking_the_policy(Arc::new(move |request| {
                heeding.lock().ok()?.policy_now(request)
            }));
        }
        if route.only_voice
            && let Ok(mut heeding) = heeding.lock()
        {
            heeding.as_the_only_voice(
                files.remarks,
                files.attention && files.spoken_to,
                files.spoken_to_by_day,
            );
        }
        // Asked again, or hummed for, as on the Live route
        // (`LAMP_OTHER_LANGUAGE=sorry`, `LAMP_MISHEARD_HUM`; the first
        // builder's 21:51 on #1722).
        if files.ask_unclear {
            if let Ok(mut heeding) = heeding.lock() {
                heeding.asking_again(files.misheard_hum);
            }
            let told = heeding.clone();
            let asked = heeding.clone();
            engine = engine.asking_again(
                Arc::new(move |request| {
                    if let Ok(mut heeding) = told.lock() {
                        heeding.ended_as_cut_in(request);
                    }
                }),
                Arc::new(move |request| asked.lock().ok()?.unclear(request)),
            );
        }
        engine = engine.heeding(Arc::new(move |request, said, heard, from| {
            heeding
                .lock()
                .ok()
                .and_then(|mut heeding| heeding.heed(request, said, heard, from))
        }));
    }
    // Without the rule, a line's own words say whether it called the lamp.
    if !files.attention {
        engine = engine.noting_whether_called(Arc::new(|_, words| {
            Some(call_note(crate::attention::called_by_name_on(words, true)))
        }));
    }
    // Each judged turn's verdict on the bus, for the Policy's hello (#1155).
    if let Some(root) = files.bus_root.as_deref() {
        match crate::bus_out::TurnOut::on(root) {
            Ok(out) => {
                let out = Mutex::new(out);
                engine = engine.telling_verdicts(Arc::new(move |request, verdict| {
                    if let Ok(mut out) = out.lock() {
                        out.tell(request, verdict, lamp_ipc::monotonic_us());
                    }
                }));
            }
            Err(error) => eprintln!(
                "lamp-voice second: turns' verdicts are not published on the bus: {error}"
            ),
        }
    }
    if checked {
        let file = thread.clone();
        engine = engine.answering_the_lamp(Arc::new(move |request| {
            crate::thread::current(file.as_deref(), |thread| {
                lamp_just_asked(thread, request, Instant::now())
            })
            .unwrap_or_default()
        }));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    if let Some(task) = routes_task {
        runtime.spawn(task);
    }
    if let Some((bus, told)) = air {
        eprintln!(
            "lamp-voice second: a stuffy room is told beside what a person says (LAMP_AIR_FACTS)"
        );
        runtime.spawn(air_task(bus, told));
    }
    if let Some((bus, told)) = seems {
        eprintln!(
            "lamp-voice second: how a known person seems is told beside what they say (LAMP_SECOND_SEEMS)"
        );
        runtime.spawn(seems_task(bus, told));
    }
    if let Some((chat, root)) = texts {
        let (character, tools) = texts_from;
        runtime.spawn(texts_task(chat, character, tools, root));
    }
    let result = runtime.block_on(async move {
        // At most one filesystem read is outstanding; admission only reads the
        // nonblocking cache. Aborting the refresh prevents any later poll.
        let refresh = scope_file.map(|path| {
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(crate::memory_scope::REFRESH_EVERY);
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    interval.tick().await;
                    let mut token = face_cache.begin_refresh();
                    let path = path.clone();
                    let bus = faces_bus.clone();
                    // The known file's mark first: a change read here, by
                    // whoever wrote it, makes the cache cold (#2807), and
                    // this refresh's observation, read after, may end it.
                    let read = tokio::task::spawn_blocking(move || {
                        let mark = crate::known_faces::KnownMark::read(&path);
                        let at = Instant::now();
                        (
                            (mark, at),
                            crate::known_faces::FaceObservation::read(&path),
                            bus.as_deref().and_then(crate::known_faces::faces_in_view),
                        )
                    })
                    .await;
                    let Ok(((mark, at), observation, in_view)) = read else {
                        face_cache.replace_seen(token, None, None);
                        continue;
                    };
                    if face_cache.known_file_read(&mut token, mark, at) {
                        eprintln!(
                            "lamp-voice second: the eyes' known file changed: the face cache is cold"
                        );
                    }
                    face_cache.replace_seen(token, observation, in_view);
                }
            })
        });
        // The first turn after a start finds the chat's connection open
        // (#3754).
        engine.warm_ahead();
        let result = run(channels, boots.coordinator, engine, thread, probe_targets).await;
        if let Some(refresh) = refresh {
            refresh.abort();
        }
        result
    });
    finish(runtime);
    result
}

/// Ends the worker's runtime once `run` has returned on Stop (#1620). A
/// plain drop waits, with no bound, for every `spawn_blocking` still
/// running: a tool call (up to `tools::ASK_DEADLINE`, 1.5 s), a text's tool
/// call or a DNS lookup would hold the worker past its stop deadline, so it
/// is killed and the run's cleanup fails. The process exits right after, so
/// blocking work left detached here dies with it.
pub(crate) fn finish(runtime: tokio::runtime::Runtime) {
    runtime.shutdown_background();
}

/// The second route's tool calls: a web search's functions answered by the
/// search, every other call run as the Live route runs it
/// (`crate::tools::answer`: the lamp's light and volume, its memory, the
/// schedule, the tasks part's `start_agent_task` and the rest), whose
/// journal line for each run is worded as the Live route's.
/// With `policed`, the Policy's gate says what it runs
/// ([`crate::second_provider::policy`]).
pub(crate) fn tool_runner(
    tools: crate::tools::Settings,
    search: Option<Arc<Search>>,
    early: Option<Arc<Early>>,
    policed: bool,
) -> ToolRunner {
    Arc::new(move |request, heard, name, args| {
        let answer = run_call(
            tools.clone(),
            search.clone(),
            early.clone(),
            (request, heard, policed),
            lamp_gemini::ToolCall {
                id: String::new(),
                name,
                args,
            },
            None,
            None,
        );
        Box::pin(async move { answer.await.told })
    })
}

/// [`tool_runner`] with the admitted request's memory authority (#1794):
/// the same search and journal lines, and the lamp's own call answered
/// within that request's person or room (`crate::tools::answer_scoped`).
pub(crate) fn scoped_tool_runner(
    tools: crate::tools::Settings,
    search: Option<Arc<Search>>,
    early: Option<Arc<Early>>,
    policed: bool,
) -> crate::second_provider::ScopedToolRunner {
    Arc::new(move |request, heard, name, args, scope| {
        let answer = run_call(
            tools.clone(),
            search.clone(),
            early.clone(),
            (request, heard, policed),
            lamp_gemini::ToolCall {
                id: String::new(),
                name,
                args,
            },
            Some(scope),
            None,
        );
        Box::pin(async move { answer.await.told })
    })
}

/// The same dispatcher with a spoken request's private audio capability.
/// Its complete call ID and immutable scope survive every local tool hop.
pub(crate) fn requested_audio_runner(
    tools: crate::tools::Settings,
    search: Option<Arc<Search>>,
    early: Option<Arc<Early>>,
    policed: bool,
) -> super::RequestedAudioRunner {
    Arc::new(move |context, call, scope| {
        Box::pin(run_call(
            tools.clone(),
            search.clone(),
            early.clone(),
            (context.request, true, policed),
            call,
            Some(scope),
            Some(context),
        ))
    })
}

/// The function a spoken turn calls for the rest of its functions, with
/// `LAMP_SECOND_FEW_TOOLS=on` ([`few_tools`]).
pub(crate) const MORE: &str = "more_functions";

/// What a call of [`MORE`] is answered: the next round has them all.
pub(crate) const MORE_GIVEN: &str =
    "Every function is given to you now. Call the one this needs, as you would have.";

/// Whether a spoken turn is given `name` at once with
/// `LAMP_SECOND_FEW_TOOLS=on` (#1712, the first builder at 18:56: "light,
/// volume, timer, search, look, tasks"): the light's and the volume's, the
/// timer's and the reminders', the tasks', the web search's and its fetch,
/// the weather's, the look's, and the decline, the skills' one function and
/// setup's walk, which a busy room, the skills' lines and a setup talk need
/// at every turn.
pub(crate) fn given_at_once(name: &str) -> bool {
    name.starts_with("light_")
        || name.starts_with("volume_")
        || crate::tools::is_schedule_tool(name)
        || crate::agent_tasks::is_tool(name)
        || [
            crate::tools::NOT_FOR_ME,
            crate::attention::HEARD_MY_NAME,
            crate::use_skill::NAME,
            crate::onboarding::STEP,
            crate::onboarding::REDO,
            crate::second_provider::net::search::NAME,
            crate::second_provider::net::search::FETCH,
            crate::second_provider::net::weather::NAME,
            crate::second_provider::LOOK_CALL,
            // Asked of a lamp all day; behind `more_functions` DeepSeek
            // answered "remember …" and "forget that" in words (#1712, the
            // first builder at 00:34 on #1722).
            crate::remember::REMEMBER,
            crate::remember::RECALL,
            crate::remember::FORGET,
        ]
        .contains(&name)
}

/// The functions a spoken turn's request is given with
/// `LAMP_SECOND_FEW_TOOLS=on` ([`crate::second_provider::net::Narrow`]):
/// those [`given_at_once`], and [`MORE`], which names the rest; once the
/// model has called [`MORE`] in the turn, every one. The first request is
/// then smaller, and the same from turn to turn for a prompt cache; what
/// it costs is a round for the rest (the memory's, the faces', the time
/// elsewhere, a mark, a stop of reports).
pub(crate) fn few_tools(
    declared: Vec<lamp_gemini::Tool>,
    messages: &[Message],
) -> Vec<lamp_gemini::Tool> {
    let asked = messages.iter().any(|message| {
        message.tool_calls.iter().any(|call| {
            call.pointer("/function/name")
                .and_then(|name| name.as_str())
                == Some(MORE)
        })
    });
    if asked {
        return declared;
    }
    let (mut given, rest): (Vec<_>, Vec<_>) = declared
        .into_iter()
        .partition(|tool| given_at_once(&tool.name) || a_note_names(messages, &tool.name));
    if !rest.is_empty() {
        let names: Vec<&str> = rest.iter().map(|tool| tool.name.as_str()).collect();
        given.push(lamp_gemini::Tool {
            name: MORE.into(),
            description: format!(
                "Your other functions, not given to you yet: {}. Call this first when what someone asks needs one of them, such as remembering or forgetting something; you are then given every function, and call the one it needs. Never for what the functions you have do.",
                names.join(", ")
            ),
            parameters: None,
            silent: false,
        });
    }
    given
}

/// What makes a spoken turn's first round call `start_agent_task`, with
/// `LAMP_TASKS_NAMED_ASK=on` ([`crate::agent_tasks::named_ask`]): the last
/// message is the person's, and its words ask one of the staff by name to
/// do something. A round after a call ends with the call's result, never
/// the person's words, and is asked as before. On lamp-4ace on 21 Oct, two
/// of the staff read, the played asks of a poem by name were answered by
/// the lamp itself or with `not_for_me` (#1722).
///
/// When the words ask exactly one of the staff, its `who` is told only
/// that one, and must be given: on lamp-4ace in b0701 (#1722, 07:19),
/// forced, "can you ask Claude Code on my office computer to …" went to
/// Codex, the model's choice. When they ask two or more, it is still
/// forced, and `who` is told as declared, so each task can go to its own
/// (the reviewer on #2410).
pub(crate) fn named_asks(place: crate::agent_tasks::Place) -> crate::second_provider::net::Force {
    Arc::new(move |messages: &[Message]| {
        let last = messages.last().filter(|message| message.role == "user")?;
        match crate::agent_tasks::named_ask(&place, last.words()?)[..] {
            [] => None,
            [agent] => Some(crate::second_provider::net::Forced {
                name: crate::agent_tasks::START,
                only: Some((crate::agent_tasks::WHO, agent)),
            }),
            _ => Some(crate::second_provider::net::Forced {
                name: crate::agent_tasks::START,
                only: None,
            }),
        }
    })
}

/// What makes a spoken turn's first round call `remember_my_face`, with
/// `LAMP_SECOND_REMEMBER_ASKED=on` and names checked: the last message is
/// the person's, and its own words give their name
/// ([`crate::known_faces::gives_a_name`]) and plainly ask the lamp to
/// remember their face or who they are
/// ([`crate::known_faces::asks_to_remember`]). A round after a call ends
/// with the call's result, and is asked as before.
///
/// On lamp-4ace on 23 Oct at 18:30 (the owner's own try), "Lamp, remember my
/// face, I'm …" was given every function, and its round 1 called
/// take_a_look; when the name was spelled after it, the lamp said it knew
/// the face now, and the eyes held no one. The name is still checked
/// (`LAMP_NAMES=checked`): one not in their words is refused, and the model
/// is told to ask them to spell it. A name in a later line is the model's,
/// as before: the thread's messages don't say whether the line before was
/// the same person's, and in a room another's ask would file this one's
/// name on the face at the lamp (reviewer-three's P2 on #3298).
pub(crate) fn remember_asks() -> crate::second_provider::net::Force {
    use crate::known_faces::{asks_to_remember, gives_a_name};
    Arc::new(|messages: &[Message]| {
        let last = messages.last().filter(|message| message.role == "user")?;
        let words = last.words()?;
        (gives_a_name(words) && asks_to_remember(words)).then_some(
            crate::second_provider::net::Forced {
                name: crate::known_faces::REMEMBER,
                only: None,
            },
        )
    })
}

/// One force of two, the first that names a function for the messages: a
/// chat takes only one ([`crate::second_provider::net::Chat::forcing`]).
pub(crate) fn first_force(
    first: Option<crate::second_provider::net::Force>,
    second: Option<crate::second_provider::net::Force>,
) -> Option<crate::second_provider::net::Force> {
    match (first, second) {
        (Some(first), Some(second)) => Some(Arc::new(move |messages: &[Message]| {
            first(messages).or_else(|| second(messages))
        })),
        (first, second) => first.or(second),
    }
}

/// Whether a system or user message the turn carries names the function
/// `name` as a whole word: a note that says "call remember_my_face" is
/// given it at once, not behind [`MORE`], where DeepSeek reached for a
/// plain `remember` instead and never closed the step (row 10 on
/// lamp-52e6).
fn a_note_names(messages: &[Message], name: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let names = |text: &str| {
        text.match_indices(name).any(|(at, _)| {
            !text[..at].chars().next_back().is_some_and(word)
                && !text[at + name.len()..].chars().next().is_some_and(word)
        })
    };
    messages
        .iter()
        .filter(|message| matches!(message.role, "system" | "user"))
        .filter_map(|message| message.content.as_ref())
        .any(|content| match content {
            serde_json::Value::String(text) => names(text.as_str()),
            serde_json::Value::Array(parts) => parts
                .iter()
                .filter_map(|part| part.get("text").and_then(|text| text.as_str()))
                .any(names),
            _ => false,
        })
}

/// The calls [`run_call`] answers itself before any is routed: [`MORE`],
/// and the web search's functions when there is a search.
pub(crate) fn answered_here(
    search: Option<Arc<Search>>,
) -> crate::second_provider::policy::AnsweredHere {
    Arc::new(move |name| name == MORE || search.as_ref().is_some_and(|search| search.answers(name)))
}

/// Whether a skill's look is taken on this route: `LAMP_SECOND_SKILL_LOOK`
/// exactly `on`, and a full look to take it with.
fn takes_skill_looks(switch: Option<&str>, look_by_call: bool) -> bool {
    switch == Some("on") && look_by_call
}

/// Whether a call of a name goes to the skills, in either of their folders
/// or through `use_skill`, as the Live worker decides which answers may ask
/// for a look (#941, #1668, #1656).
pub(crate) fn skill_call(settings: crate::tools::Settings) -> crate::second_provider::SkillCall {
    Arc::new(move |name| {
        let route = crate::tools::route(&settings, name);
        matches!(route, crate::tools::Route::UseSkill(_))
            || crate::tools::is_skills(&settings, &route)
    })
}

/// One call of the model's, as all spoken runners run it.
async fn run_call(
    settings: crate::tools::Settings,
    search: Option<Arc<Search>>,
    early: Option<Arc<Early>>,
    (request, heard, policed): (u64, bool, bool),
    call: lamp_gemini::ToolCall,
    scope: Option<crate::memory_scope::ScopeLease>,
    context: Option<lamp_msgs::RequestedAudioContext>,
) -> crate::tools::ToolAnswer {
    use crate::tools::ToolAnswer;
    if context.as_ref().is_some_and(|context| {
        context.check().is_err()
            || context.call_id != call.id
            || !scope
                .as_ref()
                .is_some_and(|scope| super::audio_scope_current(scope, context))
    }) {
        return ToolAnswer::plain(crate::tools::unavailable());
    }
    if call.name == crate::attention::HEARD_MY_NAME {
        named(request);
        return ToolAnswer::plain(crate::tools::declined());
    }
    if call.name == MORE {
        eprintln!(
            "lamp-voice second: request {request} called {MORE}: its next round is given every function"
        );
        return ToolAnswer::plain(serde_json::json!({"ok": true, "result": MORE_GIVEN}));
    }
    if let Some(search) = search.filter(|search| search.answers(&call.name)) {
        // The request's first search takes the one begun from its
        // words, if any (#1712).
        if call.name == crate::second_provider::net::search::NAME
            && let Some(taken) = early.and_then(|early| {
                early.take(request, call.args["query"].as_str().unwrap_or_default())
            })
        {
            match taken {
                Ok((begun, answer)) => {
                    eprintln!(
                        "lamp-voice second: request {request} web_search answered by the search begun {} ms before the call",
                        begun.elapsed().as_millis()
                    );
                    return ToolAnswer::plain(answer.await);
                }
                Err(added) => eprintln!(
                    "lamp-voice second: request {request} web_search asks {added} words the search begun from its words did not: searched as asked"
                ),
            }
        }
        return ToolAnswer::plain(search.answer(&call.name, &call.args).await);
    }
    tokio::task::spawn_blocking(move || {
        use crate::tools::Route;
        let route = crate::tools::route(&settings, &call.name);
        // The Live route's line for a call the Policy held and then
        // let run, which the battery counts (`followups.sh`'s "; run
        // after N ms"): said for each call that route would hold, at
        // 0 ms, when nothing here holds it (#1712: 0 tool calls were
        // counted on it at 09:00). With the Policy holding, its gate
        // says it ([`crate::second_provider::policy`]).
        if let Some(line) = run_after(policed, &route, &call.name, request) {
            eprintln!("{line}");
        }
        // Something done is the lamp answering, words or none, as the
        // Live worker counts it for the room (#829): not a lookup, and
        // only for a line heard in the room, never a text from the
        // app or the lamp's own turn.
        if heard
            && matches!(
                route,
                Route::Executive(_)
                    | Route::UseSkill(_)
                    | Route::Schedule(_)
                    | Route::Tasks(_)
                    | Route::Faces(..)
                    | Route::Mark(_)
            )
            && let Ok(mut room) = crate::room_told::room().lock()
        {
            room.answered_by_a_call(request, monotonic_us());
        }
        // A news key stays in the answer: the turn takes it out before
        // its model reads it, and tells the coordinator (#1390).
        match (context.as_ref(), scope.as_ref()) {
            (Some(context), Some(scope)) => {
                crate::tools::answer_spoken_scoped_keeping_news(&settings, &call, context, scope)
            }
            _ => ToolAnswer::plain(crate::tools::answer_keeping_news(
                &settings,
                &call,
                request,
                heard,
                scope.as_ref(),
            )),
        }
    })
    .await
    .unwrap_or_else(|_| ToolAnswer::plain(crate::tools::unavailable()))
}

/// The Policy's decisions for the lamp's actions on this route, when they
/// wait for them (`--policy-holds-actions`): with the bus, and with the
/// route's rule (`--attention`), which tells the gate each turn's name and
/// conversation. Without either the actions run at once, said once here.
pub(crate) fn policy_verdicts(
    holds: bool,
    attention: bool,
    bus_root: Option<&Path>,
) -> Option<crate::policy_holds::Verdicts> {
    if !holds {
        return None;
    }
    let Some(root) = bus_root else {
        eprintln!(
            "lamp-voice second: the Policy cannot hold the lamp's actions without the bus (LAMP_BUS=on): they run at once"
        );
        return None;
    };
    if !attention {
        eprintln!(
            "lamp-voice second: the Policy cannot hold the lamp's actions on this route without its rule (LAMP_ATTENTION=on), which knows each turn's name and conversation: they run at once"
        );
        return None;
    }
    crate::policy_holds::Verdicts::on(root)
        .inspect_err(|why| {
            eprintln!(
                "lamp-voice second: the Policy's decisions cannot be read from the bus, so the lamp's actions run at once: {why}"
            );
        })
        .ok()
}

/// Whether spoken turns are offered `heard_my_name` (`LAMP_NAME_SIGNAL=on`).
static NAME_SIGNAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The request whose model said last that it heard its name, and when, by
/// this process's clock.
static NAMED: Mutex<Option<(u64, u64)>> = Mutex::new(None);

/// `declare`'s functions and `heard_my_name` (`LAMP_NAME_SIGNAL=on`), whose
/// calls are journaled from now on.
fn with_name_signal(
    declare: crate::second_provider::net::Declare,
) -> crate::second_provider::net::Declare {
    NAME_SIGNAL.store(true, std::sync::atomic::Ordering::Relaxed);
    Arc::new(move || {
        let mut declared = declare();
        declared.push(crate::attention::name_tool());
        declared
    })
}

/// Spoken turns' forget functions say that one called unasked is held for
/// the person's yes (`crate::remember::held_forgets_described`).
fn with_held_forgets(
    declare: crate::second_provider::net::Declare,
) -> crate::second_provider::net::Declare {
    Arc::new(move || crate::remember::held_forgets_described(declare()))
}

/// The model said it heard its name in `request`: answered, nothing done,
/// and journaled as the Live route journals it. On this route the call
/// comes in a chat round before the answer's, so before its first sound.
fn named(request: u64) {
    if let Ok(mut named) = NAMED.lock() {
        *named = Some((request, monotonic_us()));
    }
    eprintln!("lamp-voice second: request {request}: the model said it heard its name");
}

/// The first sound of `request`'s answer, with `LAMP_NAME_SIGNAL=on`: how
/// long after the model said it heard its name, if it did, as the Live
/// route says it (`provider_worker`'s `sound`), the count that decides
/// whether the sign can be used. Once per request, from the line that says
/// the first answer audio.
pub(crate) fn name_sounded(request: u64) {
    if !NAME_SIGNAL.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    match NAMED.lock().ok().and_then(|named| *named) {
        Some((named, at)) if named == request => eprintln!(
            "lamp-voice second: request {request}: first sound {} ms after the model said it heard its name",
            monotonic_us().saturating_sub(at) / 1_000
        ),
        _ => eprintln!(
            "lamp-voice second: request {request}: first sound, and the model has not said it heard its name"
        ),
    }
}

/// The journal's line for a call the Live route would hold for the Policy
/// (`policy_holds::holds`), as that route says it once it lets one run,
/// for the battery's counters: at 0 ms, when nothing here holds it. With
/// the Policy holding (`policed`), none: the gate says its own.
pub(crate) fn run_after(
    policed: bool,
    route: &crate::tools::Route,
    name: &str,
    request: u64,
) -> Option<String> {
    (!policed && crate::policy_holds::holds(route, name)).then(|| {
        format!(
            "lamp-voice second: request {request} asked for {name}; run after 0 ms: this route holds no call for the Policy"
        )
    })
}

/// The room this route keeps as the lamp's only voice, as the Live worker
/// keeps its own: taken back from its file at the start, told on the bus
/// (`room/state`) at each tick, and kept in its file about once a second.
/// With `spoken_to` (`--spoken-to` and `--attention`) only talk between
/// people counts, and with `cut_ins_not_talk` a line said over the lamp is
/// left out, as on the Live worker. With `ends_sooner`
/// (`LAMP_BUSY_ENDS_SOONER`) a busy room ends a minute after the last talk
/// when nothing since was answered, and a minute after the lamp's first
/// answer since the talk once it was, as Live's does with
/// `LAMP_GOING_ON_ASKS`.
pub(crate) fn keeping_room(
    spoken_to: bool,
    cut_ins_not_talk: bool,
    ends_sooner: bool,
    room_file: Option<&Path>,
    bus_root: Option<&Path>,
) -> Box<dyn FnMut(u64) + Send> {
    let mut kept_boot = None;
    if let Ok(mut room) = crate::room_told::room().lock() {
        room.leave_cut_ins_out(cut_ins_not_talk);
        room.count_only_talk(spoken_to);
        room.end_busy_sooner(ends_sooner);
        room.end_busy_once_answered(ends_sooner);
        if let Some(path) = room_file {
            let (line, boot) = crate::room_told::start_from(
                path,
                Path::new(crate::room_told::MACHINE_BOOT_FILE),
                &mut room,
                monotonic_us(),
            );
            if let Some(line) = line {
                eprintln!("lamp-voice second: {line}");
            }
            kept_boot = boot.map(|boot| (path.to_path_buf(), boot));
        }
    }
    let mut out = bus_root.and_then(|root| {
        crate::bus_out::RoomOut::on(root)
            .inspect_err(|error| {
                eprintln!("lamp-voice second: the room is not published on the bus: {error}");
            })
            .ok()
    });
    let mut kept = None;
    let mut saved_at_us = 0;
    let mut unwritten_told = false;
    Box::new(move |now_us| {
        let Ok(mut room) = crate::room_told::room().lock() else {
            return;
        };
        let now = room.at(now_us);
        if let Some(out) = out.as_mut() {
            out.tell(now, now_us);
        }
        if let Some((path, boot)) = &kept_boot
            && now_us.saturating_sub(saved_at_us) >= 1_000_000
        {
            saved_at_us = now_us;
            if let Err(error) = crate::room_told::keep_in(path, &room, boot, now_us, &mut kept)
                && !unwritten_told
            {
                unwritten_told = true;
                eprintln!(
                    "lamp-voice second: the room's file could not be written ({error}): a restart will not know the room"
                );
            }
        }
    })
}

/// With `LAMP_STAFF_NOTE=on` (`--staff-note`) and the tasks part: the note
/// the Live route gives beside a request whose words name one of the staff
/// (`crate::staff_note`), for this route to give beside the words. Here the
/// words are whole before the chat is asked, so it is never late. On
/// lamp-4ace on 16 Oct the Live route's model did the work itself until
/// that note named who the work was for.
pub(crate) fn staff_noting(place: crate::agent_tasks::Place) -> super::StaffNote {
    Arc::new(move |request, words| {
        let who = crate::agent_tasks::named_in_words(&place, words);
        if who.is_empty() {
            return None;
        }
        eprintln!(
            "lamp-voice second: request {request}: named {}; the model is told",
            who.join(" and ")
        );
        Some(crate::staff_note::note(&who))
    })
}

/// Whether a checked sound heard for `request` may be the person's answer
/// to the lamp: a conversation is going on and the lamp's own last line
/// before it was a question, as the Live route's rule reads the same thread
/// (`crate::attention`, 17 October).
pub(crate) fn lamp_just_asked(thread: &crate::thread::Thread, request: u64, now: Instant) -> bool {
    thread.going_on(now) && thread.lamp_asked(request)
}

/// The known person's name for a turn (#2539, `LAMP_SECOND_FACE_NAME=on`),
/// as the Live route tells it (`known_faces::note`): when the face the eyes
/// know now, in `who`, is this turn's own person. A room's turn, a line
/// narrowed to the room, or another person's has none; nor does a face gone.
pub(crate) fn known_name_told(on: bool, who: Option<&Path>, person: Option<&str>) -> String {
    if !on {
        return String::new();
    }
    let Some(name) = who.and_then(crate::known_faces::known_now) else {
        return String::new();
    };
    if person != Some(crate::known_faces::memory_person(&name).as_str()) {
        return String::new();
    }
    format!("\n\n{}", crate::known_faces::note(&name))
}

/// What a Live session is told of the moment beside what it hears, told
/// here in each turn's system message, since each chat starts with nothing:
/// the session fact ([`crate::session_fact::FACT`]) when `session` (no
/// conversation going on), and how the room is when it is known
/// ([`crate::room_told::BUSY`], [`crate::room_told::QUIET`]).
pub(crate) fn facts(session: bool, room: Option<crate::room_told::Room>) -> String {
    let room = match room {
        Some(crate::room_told::Room::Busy) => Some(crate::room_told::BUSY),
        Some(crate::room_told::Room::Quiet) => Some(crate::room_told::QUIET),
        _ => None,
    };
    session
        .then_some(crate::session_fact::FACT)
        .into_iter()
        .chain(room)
        .map(|fact| format!("\n\n{fact}"))
        .collect()
}

/// Which parts are unwell, with `--health-facts`, for this turn's system
/// message (`crate::health_told`): from what the watch keeps, every turn
/// while it lasts, since each chat starts with nothing; empty when all is
/// well, nothing fresh is known, or the switch is off.
pub(crate) fn health_fact(
    health: Option<&std::sync::Mutex<crate::health_told::HealthTold<()>>>,
) -> String {
    health
        .and_then(|health| health.lock().ok()?.now())
        .map_or(String::new(), |line| format!("\n\n{line}"))
}

/// How long [`FacesHeld`] keeps the eyes' looks. The eyes look every
/// 250 ms with a face to follow (`LOOK_EVERY` in `lamp-eyes`), 500 ms with
/// one face sat still and 600 ms with nobody, so this holds 4 to 10 looks.
const FACES_KEPT_MS: u64 = 2_500;
/// And at most this many, whatever the eyes' pace.
const FACES_LOOKS_KEPT: usize = 16;
/// A look over this old when taken is refused; a gap between two looks
/// over this, a look dated before the one kept last or ahead of now, or a
/// verdict this long after the newest look, is a break: what was kept is
/// dropped (reviewer-two's HOLDs on #3355). The eyes' slowest pace is
/// 600 ms.
const LOOK_GAP_MS: u64 = 1_000;
/// How many kept looks must see two faces or more for the room to be busy.
const TWO_FACES_LOOKS: usize = 3;
/// How long, from the first such look to the last, two faces must stay.
const TWO_FACES_FOR_MS: u64 = 1_500;

/// One look the eyes published, as [`FacesHeld`] keeps it.
#[derive(Clone, Copy)]
struct FacesLook {
    /// When the eyes looked (`header.at_us`): their `lamp_ipc::monotonic_us`,
    /// the system-wide clock the voice reads too (reviewer-two on #3355), so
    /// every age, expiry and span is on one clock no wall step can move.
    at_us: u64,
    faces: usize,
    /// The header's `expires_ms`, from when the eyes looked.
    expires_ms: Option<u64>,
}

/// Every look the eyes published (`humans/faces/tracked`), read on a
/// thread of its own as they come (`LAMP_TWO_FACES_HELD=on`): two faces
/// make the room busy only when the newest fresh look sees two and at
/// least [`TWO_FACES_LOOKS`] of those in the last [`FACES_KEPT_MS`] did,
/// spanning [`TWO_FACES_FOR_MS`] or more, so a face passing by, a misread
/// or a photo frame seen once does not. Sampling once per line instead
/// made it 3 lines of 4 (the coordinator's finding on #3355). A privacy
/// change drops what was kept and refuses every look dated at or before
/// it, and every look the reader took across it (`epoch`), so looks
/// queued before it never come back.
#[derive(Default)]
struct FacesHeld {
    looks: std::collections::VecDeque<FacesLook>,
    epoch: u64,
    cutoff_us: Option<u64>,
}

impl FacesHeld {
    /// Keep a look the reader took at `now_us`, `epoch` read before it
    /// began to wait for it.
    fn look(&mut self, look: FacesLook, epoch: u64, now_us: u64) {
        if epoch != self.epoch || self.cutoff_us.is_some_and(|cutoff| look.at_us <= cutoff) {
            return;
        }
        let ahead = look.at_us > now_us + lamp_bus::EXPIRY_GRACE_MS * 1_000;
        if ahead
            || self.looks.back().is_some_and(|last| {
                look.at_us < last.at_us || look.at_us - last.at_us > LOOK_GAP_MS * 1_000
            })
        {
            self.looks.clear();
        }
        if ahead || Self::stale(&look, now_us) {
            return;
        }
        if self.looks.len() == FACES_LOOKS_KEPT {
            self.looks.pop_front();
        }
        self.looks.push_back(look);
    }

    /// Whether a look is over [`LOOK_GAP_MS`] old at `now_us`, or expired
    /// as [`lamp_bus::Header::expired_at`] has it.
    fn stale(look: &FacesLook, now_us: u64) -> bool {
        let age_ms = now_us.saturating_sub(look.at_us) / 1_000;
        age_ms > LOOK_GAP_MS
            || look
                .expires_ms
                .is_some_and(|lasts| age_ms > lasts + lamp_bus::EXPIRY_GRACE_MS)
    }

    /// Privacy closed or reopened at `now_us`: nothing before it counts.
    fn privacy_changed(&mut self, now_us: u64) {
        self.looks.clear();
        self.epoch += 1;
        self.cutoff_us = Some(self.cutoff_us.map_or(now_us, |cutoff| cutoff.max(now_us)));
    }

    /// Whether two faces have stayed at `now_us`. A missing or expired
    /// newest look ends what was kept.
    fn two_stayed(&mut self, now_us: u64) -> bool {
        while self
            .looks
            .front()
            .is_some_and(|look| now_us.saturating_sub(look.at_us) > FACES_KEPT_MS * 1_000)
        {
            self.looks.pop_front();
        }
        let Some(newest) = self.looks.back().copied() else {
            return false;
        };
        if Self::stale(&newest, now_us) {
            self.looks.clear();
            return false;
        }
        if newest.faces < 2 {
            return false;
        }
        let mut two = self.looks.iter().filter(|look| look.faces >= 2);
        let count = two.clone().count();
        let first = two.next().map_or(0, |look| look.at_us);
        let last = two.next_back().map_or(first, |look| look.at_us);
        count >= TWO_FACES_LOOKS && last.saturating_sub(first) >= TWO_FACES_FOR_MS * 1_000
    }
}

/// The Live route's rule for whom it answers (`crate::attention`, given
/// with `--attention`), run on this route's spoken turns with what this
/// route can give it: how the room is as Gemini's worker publishes it
/// (`room/state`), the lamp's name as the part that listens for it hears it
/// (`name/heard`), whether the eyes see someone turned to the lamp, and the
/// thread (a conversation going on, the lamp's last line a question). On
/// lamp-4ace on 2026-10-16 this route answered two remarks in a busy room
/// that Gemini's session left alone, nobody facing and no name (#1014).
///
/// With `--attention-near`, also how far the room microphone rose over its
/// quiet during the line (`room/sound`), sampled as the Live worker samples
/// it. With `--addressed` (`LAMP_ADDRESSED=on`), whether the line was said
/// to the lamp from close to it ([`crate::addressed`]): the room
/// microphone's rise and the eyes' close faces turned to the lamp over the
/// line's sound, judged once for each request and published on
/// `attention/addressed`, as on the Live route. Not given: a tap on the
/// head, so a line in a busy room that only that would let through on the
/// Live route is held here. With `LAMP_SAME_VOICE_MODEL`, the line's voice
/// against the conversation's, as on Live ([`Self::checking_the_voice`]).
/// Each turn is judged as a session's first: this
/// route keeps no session that has heard enough to be trusted.
pub(crate) struct Heeding {
    attention: crate::attention::Attention,
    strict: bool,
    /// Whether it is night here, read before each verdict with the day's
    /// own bars (`LAMP_SPOKEN_TO=day-and-night`), as the Live worker reads
    /// it: [`crate::local_time::night_here`], or a test's.
    night: fn() -> bool,
    /// The bus, and the name's hearings on it since the last turn.
    bus: Option<(lamp_bus::Bus, lamp_bus::Subscriber)>,
    thread: Option<PathBuf>,
    /// The room microphone's rise over its quiet, by when it was sampled,
    /// with `--attention-near` or `--addressed`: the last [`LEVELS_KEPT_US`].
    levels: Option<Levels>,
    /// Whether the rise is the rule's own sign of a line said near
    /// (`--attention-near`), as on the Live route: with `--addressed` alone
    /// it is sampled for that judgement only.
    near_sign: bool,
    /// Where the levels' thread finds `--addressed`'s looks to sample beside
    /// them, whichever was set up first: started before them
    /// (`LAMP_FIRST_SECONDS_QUIET`), it still reads the eyes once they are
    /// (the reviewer-lead's P3 on #3801).
    levels_looks: Arc<std::sync::OnceLock<std::sync::Weak<Mutex<crate::addressed::Looks>>>>,
    /// With `--addressed`: the eyes' looks, sampled beside the levels, and
    /// where each request's judgement is published, if the bus can be used.
    addressed: Option<(Looked, Option<crate::addressed::AddressedOut>)>,
    /// The request the rule judged last, the session it is judged in, which
    /// a request going on from it keeps, the words it was judged with,
    /// which are stripped from the front of that request's, and the whole
    /// chain's words, the first half's included, which name what it asks
    /// for (Eighth on #2054: "Remind me" | "in two minutes to stretch").
    judged: Option<(u64, u64, String, String)>,
    /// This route is the lamp's only voice ([`Self::as_the_only_voice`]):
    /// the room is the one it keeps itself, not Gemini's worker's.
    keeps_room: bool,
    /// The name note (`LAMP_NAME_NOTE=on`, [`Self::telling_the_name`]):
    /// whether a busy room waits for someone turned to the lamp, and
    /// whether the note says the name may have sounded like another word.
    name_note: Option<(bool, bool)>,
    /// The request judged last, and the note it earned, if any.
    noted: Option<(u64, Option<&'static str>)>,
    /// The request last let out only as the conversation going on.
    alone: Option<u64>,
    /// The request last judged in a busy room ([`Self::judged_busy`]).
    busy: Option<u64>,
    /// The Policy's attention decisions, with `LAMP_GOING_ON_ASKS`
    /// ([`Self::asking_the_policy`]).
    policy: Option<crate::policy_holds::Verdicts>,
    /// The line that waits for the Policy's decision, and how it was seen.
    waiting: Option<Waiting>,
    /// The request last let out because its words or the lamp's name heard
    /// in its sound called the lamp.
    by_name: Option<u64>,
    /// The rule's session, as a Live session is one (#1712, the first
    /// builder at 16:43): the route's turns are judged as one session's, so
    /// one past its first [`crate::attention::FRESH_REQUESTS`] is trusted as
    /// on Live. A new one begins as Live renews its own: for a new person at
    /// the lamp, and after [`RENEW_AFTER_HELD`] lines held in a row, at most
    /// every [`RENEW_AT_MOST_EVERY`].
    session: u64,
    session_for: Option<Option<String>>,
    held_run: u32,
    renewed_at: Option<Instant>,
    /// When a line that called the lamp for itself (its name, its words,
    /// a face turned to it) was let out: the conversation goes on from it
    /// for [`CALL_CARRIES_FOR`], whether or not its answer was said before
    /// the person spoke again.
    called_at: Option<Instant>,
    /// When the lamp's words sent will have been played
    /// ([`crate::second_provider::Did::Said`]), on the monotonic clock.
    said_until_us: Option<u64>,
    /// The request judged last, and what the Policy's gate is told of it
    /// ([`Self::policy_turn`]).
    policed: Option<(u64, crate::second_provider::policy::Turn)>,
    /// The lamp hums for a misheard cut-in (`LAMP_MISHEARD_HUM`,
    /// [`Self::asking_again`]).
    misheard_hum: bool,
    /// In a quiet room, a conversation going on holds only for the one the
    /// lamp answered (`LAMP_QUIET_GOING_ON_SAME_ONE=on`,
    /// [`Self::going_on_quiet_only_for_the_same_one`]).
    quiet_same_one: bool,
    /// With it, the eyes' speaking faces and their reader
    /// ([`Self::seeing_speaking_faces`]), and the passing ids seen speaking
    /// through the last call let out, each with the eyes' boot: the
    /// answered caller's face, gone when its conversation is over or the
    /// privacy changes ([`Self::privacy_changed`]).
    eyes: Option<(
        Arc<crate::speaking_faces::SpeakingFaces>,
        Option<crate::speaking_faces::Reader>,
    )>,
    caller_faces: crate::speaking_faces::Speakers,
    /// The request last ended as a cut-in over the lamp's answer.
    cut_in: Option<u64>,
    /// Leave alone a reply that answers nothing the lamp asked, said over its
    /// answer ([`crate::others_answer`]).
    others_answer: bool,
    /// In a busy room, a conversation going on holds only for the one the
    /// lamp answered ([`crate::going_on_same_one`]).
    going_on_same_one: bool,
    /// `LAMP_SAME_SPEAKER_CARRIES` ([`Self::carrying_the_same_speaker`]):
    /// whether it is on; whether a conversation is open for it, from the
    /// lamp's last answer until a stop, a privacy change, or two lines in a
    /// row from someone else end it; and the request begun while it stands, for its check
    /// at the verdict, with whether it began within a follow-up's 8 s.
    carries: bool,
    carrying: bool,
    carry_window: carry_window::CarryWindow,
    carried: Option<(u64, bool)>,
    /// The newest line carried by a sure sign of the same speaker: a print
    /// come late of a line before it ends nothing (reviewer-two at 03:07).
    carried_last: Option<u64>,
    /// Lines in a row from someone else held while the carry stands, each
    /// request once ([`Self::colleague_held`]): the second ends it.
    carry_others: Vec<u64>,
    /// Bounded constraints from held colleagues in this call. The owner's
    /// voice alone must be nearer theirs than every retained held print;
    /// missing evidence or capacity exhaustion requires their face.
    held_colleagues: held_colleagues::HeldColleagues,
    /// How much speech a print under `same_voice::DIFFERENT_BELOW` needs to
    /// be a sure other voice under a carry: [`CARRY_SURE_OTHER_SPEECH_SAMPLES`],
    /// lowered only by tests for a synthetic classification control.
    sure_other_samples: usize,
    /// The newest request, if it began over the lamp's own answer (its
    /// Start's `over_the_lamps_voice`), as [`Self::hear`] is told.
    began_over: Option<u64>,
    /// The same-voice check (`LAMP_SAME_VOICE_MODEL`, #829,
    /// [`Self::checking_the_voice`]), as the Live worker keeps it.
    voices: Option<crate::same_voice::Voices<Box<dyn crate::same_voice::Printer>>>,
    /// #829's conversation check reads [`Self::voices`] only beside
    /// `LAMP_SPOKEN_TO` ([`Self::as_the_only_voice`]); without it the prints
    /// feed only [`Self::someone_else`] (#2428).
    voice_ends_going_on: bool,
    /// `LAMP_NAMED_ALWAYS=on` (#2191, [`Self::always_answering_named`]).
    named_always: bool,
    /// The request judged last, if it began with the lamp's name said to a
    /// face turned to the lamp ([`named_to_a_face`]).
    named_faced: Option<u64>,
    /// `LAMP_GOING_ON_NOT_OWN=on` ([`Self::not_counting_own_answers`]).
    not_own: bool,
    /// With it: the lines let out only as the conversation going on since
    /// the lamp last answered a line said to it. Its answers to them carry
    /// no conversation; its other lines do, as before.
    chain: Vec<u64>,
    /// [`Self::said_until_us`] but for the words of answers to `chain`.
    said_until_free_us: Option<u64>,
    /// `LAMP_BUSY_CALLED_STRICT=on` ([`crate::attention::Attention::narrow_the_call`]).
    busy_called_strict: bool,
    /// `LAMP_TURNED_SPEAKING=on` ([`Self::counting_only_turned_speakers`]).
    turned_speaking: bool,
    /// `LAMP_TURNED_FACE_SPEAKS=on` ([`Self::needing_the_turned_face_to_speak`]),
    /// and the request last judged whose turned face was so read as no
    /// sign, for the room ([`crate::room_told::RoomTold::face_not_speaking`]).
    turned_face_speaks: bool,
    face_not_speaking: Option<u64>,
    /// `LAMP_NAME_OR_FOLLOW_UP=on` ([`Self::needing_name_or_follow_up`]).
    name_or_follow_up: bool,
    /// `LAMP_TWO_FACES_HELD=on` ([`Self::holding_two_faces`]): the eyes'
    /// last looks, so two faces make the room busy only when they stay.
    faces_held: Option<Arc<Mutex<FacesHeld>>>,
    /// `LAMP_NAME_STAYS_IN_ITS_LINE=on` ([`Self::keeping_the_name_in_its_line`]),
    /// and the last [`SAID_KEPT`] requests judged, newest last, each with its
    /// line's sound and whether its words called the lamp by name, a call
    /// whose hearing goes on.
    name_stays: bool,
    said_lately: std::collections::VecDeque<(u64, (u64, u64), bool)>,
    /// The faces the eyes saw speaking each of the last lines judged, newest
    /// last, at most [`SPOKE_KEPT`] ([`Self::same_speaker`]).
    spoke: std::collections::VecDeque<(u64, crate::speaking_faces::Speakers)>,
    /// With `LAMP_NAME_NEAR=on` ([`Self::hearing_near`]): lamp-name's near
    /// hearings (`name/near`), and those read so far, when each reached its
    /// bar by this process's clock and its score, the last
    /// [`NEAR_KEPT_US`].
    near: Option<(lamp_bus::Subscriber, std::collections::VecDeque<(u64, f32)>)>,
    /// With it and `LAMP_EYES_SPEAKING`: the face turned to the lamp for a
    /// first word must be seen speaking the line ([`Self::turned_to_the_lamp`]).
    turned_near_speaks: bool,
    /// With it, the eyes' looks of the last 30 s, for whether a close face
    /// was turned to the lamp over a line ([`Self::looking_for_near`]):
    /// `--addressed`'s own when it runs, else read here.
    near_looks: Option<Looked>,
    /// With `LAMP_JOIN_CUT_WORDS` ([`Self::keeping_line_rooms`]): how the
    /// room was as each of the last lines was judged, newest last, at most
    /// [`SPOKE_KEPT`]; none without it.
    rooms: Option<std::collections::VecDeque<(u64, LineRoom)>>,
    /// With it, the faces in the eyes' newest look as a request's signs were
    /// read ([`Self::signs`]), for its judgement.
    in_view: Option<(u64, Option<usize>)>,
    /// With it, the conversation's last line that showed another person:
    /// heard in another voice than the conversation's (#829), printed under
    /// [`crate::same_voice::DIFFERENT_BELOW`] against it, or seen spoken by
    /// two faces. Gone when a line is judged with no conversation going on.
    another_person: Option<u64>,
    /// `LAMP_FIRST_SECONDS_QUIET` ([`Self::showing_the_first_seconds_quiet`]),
    /// and whether another person was heard or seen speaking in any line
    /// since the start: never cleared, unlike [`Self::another_person`].
    first_seconds_quiet: bool,
    another_since_start: bool,
    /// With it, whether the rule held a line judged since the start: talk
    /// the lamp left alone, which the room told counts as no talk when held
    /// (`RoomTold::held`), never cleared.
    held_since_start: bool,
    /// With it, each of the last lines kept and when it was said, so the
    /// ears' stream is read outside them ([`Self::ears_show_quiet`]).
    said: std::collections::VecDeque<(u64, (u64, u64))>,
}

/// How the room was as a line was judged, for a join on no sign of a
/// second person ([`Heeding::same_speaker`], `LAMP_JOIN_CUT_WORDS`).
#[derive(Clone, Copy, Debug)]
struct LineRoom {
    /// The faces in the eyes' newest fresh look, turned to the lamp or not
    /// ([`crate::bus_out::faces_in_view`]); none when the eyes said nothing
    /// fresh (not running, or the camera closed).
    in_view: Option<usize>,
    /// The room it was judged in: unknown is not known to be quiet.
    room: crate::room_told::Room,
}

/// Lines whose speaking faces and rooms [`Heeding`] keeps: the line cut
/// short is the one just before the line that cut it, and a chain of lines
/// cut short (`LAMP_JOIN_CUT_WORDS`) is judged link by link, so a request
/// that broke into 4 pieces and the line after keep theirs (lamp-4ace on
/// 22 Oct).
const SPOKE_KEPT: usize = 8;
/// Why one person said two lines, by their voices
/// (`LAMP_JOIN_CUT_WORDS`, [`crate::same_voice::SAME_FROM`]).
pub(crate) const SAME_VOICE_JOINS: &str = "the same voice said both";
/// Why, by the eyes: one face, the same, seen speaking each, and no other.
pub(crate) const SAME_FACE_JOINS: &str = "one face was seen speaking both";

/// The same-voice check (#829) on `heeding`, its child made by `start` with
/// `model`, when a rule reads its prints (`read`): `LAMP_SPOKEN_TO`'s
/// conversation check, or `LAMP_QUIET_GOING_ON_SAME_ONE`'s and
/// `LAMP_GOING_ON_NOT_OWN`'s voice sign of someone else
/// ([`Heeding::someone_else`]), which lamp-52e6 runs without
/// `LAMP_SPOKEN_TO` (#2341). #829's own conversation check still reads them
/// only beside `LAMP_SPOKEN_TO` ([`Heeding::voice_going_on`]). Whether the
/// voice is heard.
fn checking_the_voice_for(
    heeding: &Mutex<Heeding>,
    model: Option<PathBuf>,
    read: bool,
    start: impl FnOnce(PathBuf) -> std::io::Result<Box<dyn crate::same_voice::Printer>>,
) -> bool {
    match (read, model) {
        (true, Some(model)) => match start(model) {
            Ok(printer) => {
                if let Ok(mut heeding) = heeding.lock() {
                    heeding.checking_the_voice(printer);
                }
                true
            }
            Err(error) => {
                eprintln!("lamp-voice second: the same voice is not judged: {error}");
                false
            }
        },
        (false, Some(_)) => {
            eprintln!(
                "lamp-voice second: the same voice is judged only with LAMP_SPOKEN_TO, LAMP_QUIET_GOING_ON_SAME_ONE, LAMP_GOING_ON_NOT_OWN or LAMP_MEMORY_FOLLOWS_FACE on: not judged"
            );
            false
        }
        (_, None) => false,
    }
}

/// The rule's two switches on `engine` and `heeding`, each on its own.
/// `named_always` (`LAMP_NAMED_ALWAYS=on`, #2191): a line that begins with
/// the lamp's name, said to a face turned to the lamp, is answered in a busy
/// room, the model asked once more on its `not_for_me`. `busy_called_strict`
/// (`LAMP_BUSY_CALLED_STRICT=on`, #1712): in a busy room, a stand-in or the
/// name not set off as a call is no call without the name heard or a face
/// turned to the lamp.
fn heeding_switches(
    mut engine: Engine,
    heeding: &Arc<Mutex<Heeding>>,
    named_always: bool,
    busy_called_strict: bool,
) -> Engine {
    if named_always {
        eprintln!(
            "lamp-voice second: a line that begins with the lamp's name, said to a face turned to the lamp, is answered in a busy room (LAMP_NAMED_ALWAYS)"
        );
        if let Ok(mut heeding) = heeding.lock() {
            heeding.always_answering_named();
        }
        let heeding = heeding.clone();
        engine = engine.answering_named_to_a_face(Arc::new(move |request| {
            heeding
                .lock()
                .is_ok_and(|heeding| heeding.named_to_a_face(request))
        }));
    }
    if busy_called_strict {
        eprintln!(
            "lamp-voice second: in a busy room, a stand-in for the lamp's name or the name not set off as a call counts only with the name heard or a face turned to the lamp (LAMP_BUSY_CALLED_STRICT)"
        );
        if let Ok(mut heeding) = heeding.lock() {
            heeding.narrowing_busy_calls();
        }
    }
    engine
}

/// A line counts as begun with the lamp's name if a hearing of it began no
/// later than this into the line (#2191): "Lamp, …" or "Hey Lamp, …".
pub(crate) const NAME_STARTS_WITHIN_US: u64 = 1_000_000;

/// Held in a busy room, then let out: the line began with the lamp's name,
/// said to a face turned to the lamp (`LAMP_NAMED_ALWAYS=on`, #2191).
pub(crate) const NAMED_TO_A_FACE: &str =
    "it began with the lamp's name, said to a face turned to the lamp";

/// Whether a line begun at `began_us` with the words `heard` began with
/// the lamp's name, said to a face turned to the lamp (#2191): the name
/// heard in the sound (at [`crate::called::LEAST_SCORE`] or more, from
/// [`crate::called::BEFORE_US`] before the line to [`NAME_STARTS_WITHIN_US`]
/// into it), or its words opening with it
/// ([`crate::attention::begins_with_the_name`]: on lamp-52e6 on 20 Oct the
/// part that listens for the name heard none of three such lines); and
/// `turned`, the eyes seeing a face turned to the lamp, not any face in
/// view, which in a busy room is nearly always there (the reviewer's P2 on
/// #2210). The face is what tells the name from office talk heard as it
/// at 0.9 to 0.997, so the model's `not_for_me` is overruled only with the
/// speaker facing the lamp. On a production floor on 19 Oct the name was
/// heard at 0.976 to 0.978 and a face seen in 36 to 40 of 40 looks, and
/// every such line was held as a busy room or declined by the model
/// (#1722, hw-first-P1 finding 4).
pub(crate) fn named_to_a_face(
    names: &[(u64, f32)],
    began_us: u64,
    heard: &str,
    turned: Option<bool>,
) -> bool {
    let heard_named = names.iter().any(|(at, score)| {
        *score >= crate::called::LEAST_SCORE
            && at.saturating_add(crate::called::BEFORE_US) >= began_us
            && *at <= began_us.saturating_add(NAME_STARTS_WITHIN_US)
    });
    turned == Some(true) && (heard_named || crate::attention::begins_with_the_name(heard))
}

/// What let a line out, as the journal says it: the sign itself where the
/// rule's first reason is one that lets nothing out alone with
/// `--spoken-to` (the session trusted, the room not known to be busy). On
/// lamp-4ace on 19 Oct (the first builder at 17:49) lines were journaled as
/// let out by a trusted session, and the sign that did was not told.
fn shown(why: &'static str, sign: Option<&'static str>) -> &'static str {
    let weak = [crate::attention::TRUSTED, crate::attention::ROOM_NOT_BUSY].contains(&why);
    match sign {
        Some(sign) if weak => sign,
        _ => why,
    }
}

/// In a busy room a conversation goes on, on this route, only this long
/// from the lamp's own last words (#1712, the first builder at 18:57: in
/// ten minutes of a busy office on lamp-4ace, 15 lines were let out as the
/// conversation going on, and about 10 of the 15 answers went to office
/// talk; DeepSeek declines far less than Gemini). In a quiet room it goes
/// on [`crate::thread::GOING_ON_FOR`], as on Live.
/// The faces `eyes` saw speaking from `began_us` to `ended_us`, by this
/// process's clock, each with the eyes' boot (`boot/f3`), and of those the
/// ones turned to the lamp; none without them.
fn speakers_between(
    eyes: Option<&(
        Arc<crate::speaking_faces::SpeakingFaces>,
        Option<crate::speaking_faces::Reader>,
    )>,
    began_us: u64,
    ended_us: u64,
) -> (
    crate::speaking_faces::Speakers,
    crate::speaking_faces::Speakers,
) {
    let Some((eyes, _)) = eyes else {
        return Default::default();
    };
    let (now, now_us) = (Instant::now(), monotonic_us());
    let at = |us: u64| {
        if us >= now_us {
            now.checked_add(Duration::from_micros(us - now_us))
        } else {
            now.checked_sub(Duration::from_micros(now_us - us))
        }
    };
    match (at(began_us), at(ended_us)) {
        (Some(from), Some(to)) => (
            eyes.speakers_with_boot(from, to),
            eyes.turned_speakers_with_boot(from, to),
        ),
        _ => Default::default(),
    }
}

/// Whether `eyes` saw only silent faces turned to the lamp from `began_us`
/// to `ended_us`, by this process's clock; never without them. With
/// `unread_spoils`, a turned mouth unread in any look says no
/// ([`crate::speaking_faces::SpeakingFaces::turned_only_silent`]); without,
/// it doesn't (`turned_not_seen_speaking`).
fn turned_only_silent(
    eyes: Option<&(
        Arc<crate::speaking_faces::SpeakingFaces>,
        Option<crate::speaking_faces::Reader>,
    )>,
    began_us: u64,
    ended_us: u64,
    unread_spoils: bool,
) -> bool {
    let Some((eyes, _)) = eyes else {
        return false;
    };
    let (now, now_us) = (Instant::now(), monotonic_us());
    let at = |us: u64| {
        if us >= now_us {
            now.checked_add(Duration::from_micros(us - now_us))
        } else {
            now.checked_sub(Duration::from_micros(now_us - us))
        }
    };
    match (at(began_us), at(ended_us)) {
        (Some(from), Some(to)) if unread_spoils => eyes.turned_only_silent(from, to),
        (Some(from), Some(to)) => eyes.turned_not_seen_speaking(from, to),
        _ => false,
    }
}

pub(crate) const BUSY_GOING_ON_FOR: Duration = Duration::from_secs(15);
/// Held in a busy room: let out only as the conversation going on, and
/// neither a question, nor three words, nor an answer to the lamp's own
/// question ("yes" to "Shall I start it?").
pub(crate) const BUSY_SHORT: &str = "a busy room: a conversation going on carries only a question, three words or more, or an answer to the lamp's own question";
/// Held in a busy room: let out only by someone turned to the lamp, and
/// asking it nothing.
pub(crate) const BUSY_TURNED: &str =
    "a busy room: someone is turned to the lamp, but asks it nothing";

/// Why a line let out in a busy room for `why` is held after all, if it is
/// (#1712, the first builder at 18:57): one let out only as the
/// conversation going on needs to be a question, or three words or more,
/// or an answer to the lamp's own question (`lamp_asked`); one let out only
/// by a face turned to the lamp needs to be a question. A line called by its
/// name, its words, a tap, or said close to the lamp is let out as before,
/// and so is one whose words call the lamp, whatever else let it out: on
/// lamp-4ace on 24 Oct the owner's "Lamp.", answered as a call once no line
/// came after it, was let out as the conversation going on too, and held
/// here as a short line.
fn busy_bar(why: &str, heard: &str, lamp_asked: bool) -> Option<&'static str> {
    if crate::attention::called_by_real_name(heard) {
        return None;
    }
    let question = heard.trim_end().ends_with('?');
    if why == crate::attention::GOING_ON {
        (!(question || heard.split_whitespace().count() >= 3 || lamp_asked)).then_some(BUSY_SHORT)
    } else if why == crate::attention::TURNED {
        (!question).then_some(BUSY_TURNED)
    } else {
        None
    }
}

/// How long before `now_us` words to be played until `until_us` ended, zero
/// while they still play.
fn ended_ago(until_us: Option<u64>, now_us: u64) -> Option<Duration> {
    until_us.map(|until| Duration::from_micros(now_us.saturating_sub(until)))
}

/// How long ago the lamp last said words, counted to their end: the
/// thread's `said_ago` is from when its answer began, `ended_ago` from when
/// the words it sent were played to their end, if it sent any. Nothing
/// when the thread has no words of it, as after it forgot them.
fn to_the_end(said_ago: Option<Duration>, ended_ago: Option<Duration>) -> Option<Duration> {
    said_ago.map(|ago| ended_ago.map_or(ago, |ended| ended.min(ago)))
}

/// `signs` as the turn is judged with them: in a busy room a conversation
/// goes on only by the thread's window or the person's call
/// (`thread_or_call`), and then only [`BUSY_GOING_ON_FOR`] from the end of
/// the lamp's last words (`said_ago`) or [`CALL_CARRIES_FOR`] from the
/// call (`called_ago`). The quiet room's [`AFTER_THE_LAMPS_WORDS`]
/// ([`going_on_from`]) never counts there: after an answer of 30 s or more
/// it let office talk out in the 15 s after the words (the first builder's
/// agent on #2032, 03:09). When none is going on, the journal says why,
/// without words (#1712, the first builder at 16:16: which of these had
/// held the owner's lines was not told).
fn went_on_told(
    request: u64,
    mut signs: Signs,
    said_ago: Option<Duration>,
    called_ago: Option<Duration>,
    left_alone: Option<usize>,
    thread_or_call: bool,
) -> Signs {
    let busy = signs.room == crate::room_told::Room::Busy;
    if busy {
        signs.going_on &= thread_or_call
            && (said_ago.is_some_and(|ago| ago <= BUSY_GOING_ON_FOR)
                || called_ago.is_some_and(|ago| ago <= CALL_CARRIES_FOR));
    }
    if !signs.going_on {
        let ago = |ago: Option<Duration>, what: &str| {
            ago.map_or(format!("it was never {what}"), |ago| {
                format!("it was last {what} {} s ago", ago.as_secs())
            })
        };
        eprintln!(
            "lamp-voice second: request {request}: no conversation going on: {}, {}{}{}",
            ago(said_ago, "heard saying words, to their end,"),
            ago(called_ago, "called"),
            left_alone.map_or(String::new(), |left| format!(
                ", {left} lines left alone since it last spoke"
            )),
            if busy {
                ", in a busy room, where it goes on 15 s"
            } else {
                ""
            }
        );
    }
    signs
}

/// How long after the end of the lamp's own words a line is in a
/// conversation going on, whenever its answer began: as long as one may
/// follow on from them ([`crate::follows_on::WITHIN_US`]).
pub(crate) const AFTER_THE_LAMPS_WORDS: Duration =
    Duration::from_micros(crate::follows_on::WITHIN_US);

/// Whether a conversation with the lamp is going on: the thread's, 45 s from
/// an answer's start ([`crate::thread::GOING_ON_FOR`]); or within
/// [`AFTER_THE_LAMPS_WORDS`] of the end of the lamp's words (`said_ago`); or
/// [`CALL_CARRIES_FOR`] from the person's call (`called_ago`). A story told
/// for 40 s or more outlives the thread's window while it is told: on
/// lamp-4ace on 20 Oct at 01:47 and 01:49, two hand-overs said as stories
/// ended ("Hey Lamp, ask Claude Code to write a two-line poem…", "… ask
/// Codex to write a haiku…") lost "Hey Lamp" to the lamp's own voice and
/// were held as no conversation, "it was last heard saying words, to their
/// end, 0 s ago" (the first builder at 01:50 and 02:04 on #1712).
fn going_on_from(
    said_going_on: bool,
    said_ago: Option<Duration>,
    called_ago: Option<Duration>,
) -> bool {
    said_going_on
        || said_ago.is_some_and(|ago| ago <= AFTER_THE_LAMPS_WORDS)
        || called_ago.is_some_and(|ago| ago <= CALL_CARRIES_FOR)
}

/// With `LAMP_NAME_OR_FOLLOW_UP`, whether a line begun `began_ago` ago is
/// a follow-up: begun within [`crate::name_or_follow_up::follow_up_for`] of
/// the end of the lamp's own last words (`said_ago`, now) or of a call let
/// out (`called_ago`, now, which carries no longer than the words do), or
/// while they still played, however long the line then went on. Only then
/// does a conversation go on. In a busy room, in the check's `ended`, it
/// alone decides, so [`went_on_told`]'s 15 s doesn't bound it there.
fn follows_up(
    said_ago: Option<Duration>,
    called_ago: Option<Duration>,
    began_ago: Duration,
) -> bool {
    let as_it_began = |ago: Option<Duration>| ago.map(|ago| ago.saturating_sub(began_ago));
    crate::name_or_follow_up::follows_up(as_it_began(said_ago))
        || crate::name_or_follow_up::follows_up(as_it_began(called_ago))
}

/// How long a call let out carries the person's next lines when the lamp
/// has said no word since (`Heeding::called_at`): the owner's lines on
/// lamp-4ace came every 6 s. The lamp's own words carry them for
/// [`crate::thread::GOING_ON_FOR`], as on Live (the first builder at 17:26:
/// noises after a call the lamp never answered were carried and answered).
pub(crate) const CALL_CARRIES_FOR: Duration = Duration::from_secs(15);

/// With `LAMP_SAME_SPEAKER_CARRIES`, in a busy room, how long after the end
/// of the lamp's last words a line begun by the one it answered goes on
/// ([`Heeding::carrying_the_same_speaker`]). Each answer renews it; one
/// held colleague line extends from its audio end, at most forty seconds
/// from the answer. Past the
/// 8 s a follow-up has (`crate::name_or_follow_up::FOLLOW_UP_FOR`): on
/// lamp-4ace on 23 Oct at 18:51 the owner's third line, in a room kept busy
/// by their own held lines, was held, and every line after it.
pub(crate) const SAME_SPEAKER_CARRIES_FOR: Duration = Duration::from_secs(20);

/// With `LAMP_SAME_SPEAKER_CARRIES`, a voice print under
/// `same_voice::DIFFERENT_BELOW` against the conversation's is surely
/// another voice, and ends the carry, only when made from this much speech
/// or more, the pre-roll left out (`same_voice::Voices::usable_speech`):
/// 2.5 s, provisional. The existing score rows of
/// `docs/verification/same-voice-early-20261018` (lamp-4ace's mic2, one
/// synthetic voice), selected by their clips' original durations, with no
/// audio re-sliced and no CAM++ run again: three clips of 0.93 to 1.45 s
/// against seven references, 21 correlated pairs, read 0.11 to 0.50 against
/// their own voice, 38% under 0.30, and four clips of another voice (1.43 to
/// 1.45 s) 0.09 to 0.26. That supports caution about a
/// short print's "another voice" but calibrates no bar; on lamp-4ace on
/// 23 Oct 1.6–1.8 s prints read 0.25 and 0.27 (the first builder at 03:03).
/// Every print is asked at `same_voice::ASKED_AT`, 1.5 s of speech, so none
/// reaches it today: a voice ends no carry until longer prints of real
/// voices are measured, and this is set from that measurement.
pub(crate) const CARRY_SURE_OTHER_SPEECH_SAMPLES: usize = crate::same_voice::RATE * 5 / 2;

/// With `LAMP_SAME_SPEAKER_CARRIES`, once a line has been held as someone
/// else's in a carry, how much nearer the conversation's voice a line's
/// print must be than to that held line's, beside `same_voice::SAME_FROM`,
/// to be carried by voice alone (reviewer-three's P2 on #3331: two voices
/// have read 0.69 on 18 Oct's set, so owner and colleague lines could
/// alternate, a colleague's at 0.70 carried as the owner's).
// ponytail: provisional margin, not calibrated; set it from CAM++ pairs of
// two real voices at a desk.
pub(crate) const CARRY_NEARER_BY: f32 = 0.10;

/// `LAMP_SAME_SPEAKER_CARRIES`: `request` was someone else's while the
/// carry stood: counted once among `others`, the lines in a row from
/// someone else; at [`crate::room_told::TALK_ENDS_CONVERSATION`] of them
/// the carry ends. Whether it still stands.
fn count_colleague(others: &mut Vec<u64>, carrying: &mut bool, request: u64) -> bool {
    if !others.contains(&request) {
        others.push(request);
    }
    if others.len() >= crate::room_told::TALK_ENDS_CONVERSATION {
        *carrying = false;
    }
    *carrying
}

/// What [`Heeding::carried_by_the_same_speaker`] found of a line begun
/// while a carry stands.
struct Carried {
    /// Someone else's line: another face seen speaking it, or a sure other
    /// voice ([`Heeding::colleague_held`] counts it).
    other: bool,
    /// A sure sign: the same speaker to the lamp (carried, it goes on as the
    /// conversation), or not (held). `None`: no sure sign either way.
    goes_on: Option<bool>,
    /// Its voice, when sure: the conversation's, or another's.
    voice: Option<bool>,
}

/// Lines held in a row before the rule's session is renewed, as Live renews
/// its own after eight unanswered (`provider_worker`).
pub(crate) const RENEW_AFTER_HELD: u32 = 8;
/// At most this often.
pub(crate) const RENEW_AT_MOST_EVERY: Duration = Duration::from_secs(900);

/// The room microphone's rise over its quiet, dB, by when it was sampled.
type Levels = Arc<Mutex<std::collections::VecDeque<(u64, f32)>>>;
/// The eyes' looks of the last 30 s ([`crate::addressed::Looks`]).
type Looked = Arc<Mutex<crate::addressed::Looks>>;

/// Told beside the lamp's name when it is answered in the place of the line
/// after it, held because nothing showed the one who said the name said it
/// (`LAMP_SECOND_NAME_CARRIES`, `one_speaker`): that line's words are never
/// told, as they may be someone else's, only that words came. On lamp-4ace on
/// 26 Oct, the first line after a start, "Lamp." and then "Remember that the
/// plant by the window needs water on Fridays." in a room not yet known to be
/// quiet, got "Mm? I'm here.", and the request was lost with nothing asked.
pub(crate) const NAME_THEN_HELD: &str = "(For you to know, not to answer: right after your name someone went on talking, but you could not tell it was the one who called you, so you did not take in what they said. Say, briefly, that you didn't catch it, and ask them to say it again.)";

/// Told beside a line the rule let out only as the conversation going on
/// (`attention::Attention::played_on_the_conversation_alone`), with
/// `not_for_me` offered. Gemini Live hears who is talking to whom; this
/// route's model reads one line of words, and on lamp-4ace on 19 Oct it
/// answered colleagues' talk after a named request six times ("Sounds like
/// a big saving."), where Live answered it once (#1712).
pub(crate) const CONVERSATION_ALONE: &str = "(For you to know, not to answer: nobody said your name or turned to you. This reached you only because you were just talking with someone, and people near you may be talking to each other. If it is plainly said to someone else, or a remark to the room, call not_for_me and say nothing. A follow-up to what you were just talking about is for you.)";

/// How the rule saw a line when it judged it, kept for a verdict given
/// once the Policy decides ([`Heeding::policy_now`]).
struct Seen {
    room: crate::room_told::Room,
    going_on: bool,
    turned: Option<bool>,
    busy: bool,
    lamp_asked: bool,
    /// The faces the eyes saw speaking through it, with
    /// [`Heeding::seeing_speaking_faces`].
    speakers: crate::speaking_faces::Speakers,
    /// Of those, the ones turned to the lamp.
    turned_speakers: crate::speaking_faces::Speakers,
    heard: String,
    /// Begun with the lamp's name said to a face turned to the lamp, with
    /// the switch ([`named_to_a_face`]).
    named_faced: bool,
    /// Held as another voice than the conversation's (#829), which the
    /// room counts as talk past the lamp.
    other_voice: bool,
    /// Found in the conversation's own voice by the same-voice check.
    same_voice: bool,
    /// Its print's likeness to the conversation's voice, if judged: none
    /// begun over the lamp's voice or too short for a print.
    likeness: Option<f32>,
}

/// A line that waits for the Policy's decision: its request, since when,
/// and how the rule saw it.
struct Waiting {
    request: u64,
    since: Instant,
    seen: Seen,
}

/// Words that ask the lamp to remember or forget something: with the
/// schedule's words, a line let out only as the conversation going on is
/// then not told it may be people's talk (`Heeding::asks_its_work`).
const MEMORY_WORDS: [&str; 2] = ["remember", "forget"];

/// Seeing someone speak (`LAMP_SECOND_SPEAKING_FACE`, `speaking`) for
/// `engine`, with `LAMP_PERSON_SCOPE_SEEN` (`scope_seen`): the policy is
/// installed whether or not the eyes' reader starts, so with no reader no
/// face is seen and every line is the room's (reviewer-two's P2 on #3437).
/// The eyes and their reader, when it started.
pub(super) fn see_who_speaks(
    mut engine: Engine,
    known: Option<&std::path::PathBuf>,
    root: Option<&std::path::Path>,
    speaking: bool,
    scope_seen: bool,
) -> (
    Engine,
    Option<Arc<crate::speaking_faces::SpeakingFaces>>,
    Option<crate::speaking_faces::Reader>,
) {
    // A line is a known person's only when their own face was seen
    // speaking it (`LAMP_PERSON_SCOPE_SEEN=on`).
    let seen = speaking && scope_seen;
    if seen {
        eprintln!(
            "lamp-voice second: a line is a known person's only when their own face was seen speaking it, else the room's (LAMP_PERSON_SCOPE_SEEN)"
        );
    }
    engine = engine.scoping_by_sight(seen);
    match (known, root) {
        (Some(known), Some(root)) if speaking && engine.scopes_context() => {
            let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
            let reader = crate::speaking_faces::read_from(eyes.clone(), root);
            if reader.is_some() {
                engine = engine.seeing_who_speaks(eyes.clone(), known.clone());
                return (engine, Some(eyes), reader);
            }
            (engine, None, None)
        }
        _ => {
            if speaking && !engine.scopes_context() {
                eprintln!(
                    "lamp-voice second: seeing who speaks is not on: it needs LAMP_SECOND_SCOPED_CONTEXT=on"
                );
            }
            (engine, None, None)
        }
    }
}

/// The rule's hook for each spoken line's sound ([`Engine::hearing`]), when
/// it needs one: for the same-voice check (`voice_judged`), or to know which
/// line began over the lamp's answer (`over_the_lamp`: `LAMP_OTHERS_ANSWER`,
/// #2136, and the conversation's note, [`Heeding::conversation_note_for`]).
/// Neither, none.
fn hearing_hook(
    heeding: &Arc<Mutex<Heeding>>,
    voice_judged: bool,
    over_the_lamp: bool,
) -> Option<crate::second_provider::Hearing> {
    (voice_judged || over_the_lamp).then(|| {
        let heeding = heeding.clone();
        let hook: crate::second_provider::Hearing = Arc::new(move |request, heard| {
            if let Ok(mut heeding) = heeding.lock() {
                heeding.hear(request, heard);
            }
        });
        hook
    })
}

/// What the bus and the thread say as a spoken turn is judged.
#[derive(Clone)]
pub(crate) struct Signs {
    pub room: crate::room_told::Room,
    pub going_on: bool,
    pub lamp_asked: bool,
    pub turned: Option<bool>,
    /// The name's hearings: when each began, by this process's clock, and
    /// its score.
    pub names: Vec<(u64, f32)>,
    /// The room microphone rose [`crate::attention::NEAR_RISE_DB`] or more
    /// over its quiet during the line.
    pub near: bool,
    /// The request this one goes on from, the same utterance after the lamp
    /// ended it early (`ProviderInput::Start::continues`,
    /// `LAMP_CUT_KEEPS_NAME=on`): the one begun just before, same session.
    pub continues_from: Option<u64>,
}

/// The lines judged lately whose sound a later-come hearing of the name is
/// laid against (`LAMP_NAME_STAYS_IN_ITS_LINE`): a hearing counts for a
/// line at most [`crate::attention::NAME_BEFORE_US`] before it begins, which
/// three short lines can fill.
const SAID_KEPT: usize = 4;

/// Hearings of the name read in one turn's judgement; more wait for the next.
const NAMES_READ: usize = 64;
/// `LAMP_NAME_NEAR`: a near hearing counts for a line's first word from
/// this long before the line's first sound, which holds the sound read
/// before the line was taken ([`Heeding::near_name`]).
const NEAR_BEFORE_US: u64 = 300_000;
/// Near hearings are kept this long, and at most this many.
const NEAR_KEPT_US: u64 = 30_000_000;
const NEAR_HEARINGS_KEPT: usize = 32;
/// The room microphone's level is sampled this often, as the Live worker does.
const LEVEL_EVERY: Duration = Duration::from_millis(100);
/// A level older than this is let go: longer than a line replayed to this
/// route takes to be heard.
const LEVELS_KEPT_US: u64 = 30_000_000;
/// `LAMP_FIRST_SECONDS_QUIET` ([`Heeding::ears_show_quiet`]): the ears'
/// quiet over which a room is a loud one, whatever its stream does, in
/// dBFS. Each lamp's quiet sits where its capture level puts it (lamp-4ace
/// -51.5, lamp-c0a4 -45 on 27 Oct, lamp-75c9 at an unpinned level -37.6), so
/// only a quiet louder than any of them decides alone. A first choice.
const LOUD_ROOM_FLOOR_DB: f32 = -30.0;
/// The most the room microphone may rise over its own quiet outside the
/// lines said for the room to be quiet, in dB: lamp-c0a4's quiet room read
/// within 2 dB of it, colleagues talking across lamp-4ace's office rose 2
/// to 8 dB (docs/ears.md). A first choice.
const FIRST_SECONDS_RISE_DB: f32 = 6.0;
/// At least this many of the ears' 100 ms readings outside the lines, a
/// second's worth, before the stream shows anything.
const FIRST_SECONDS_READINGS: usize = 10;
/// A line's own sound, for the stream: from this long before its first
/// sound to this long after its end.
const SAID_BEFORE_US: u64 = 300_000;
const SAID_AFTER_US: u64 = 700_000;

impl Heeding {
    pub(crate) fn new(
        strict: bool,
        name_in_passing_ignored: bool,
        bus_root: Option<&Path>,
        thread: Option<PathBuf>,
    ) -> Self {
        let mut attention = crate::attention::Attention::default();
        attention.ignore_name_in_passing(name_in_passing_ignored);
        // Deepgram's "Lynn" and "Lin" for "Lamp" call it here, never on Live.
        attention.count_stand_alone_ins(true);
        let bus = bus_root.and_then(|root| {
            let bus = lamp_bus::Bus::new(root).ok()?;
            let names = bus
                .subscribe_typed(
                    lamp_msgs::NameHeard::TOPIC,
                    "second",
                    lamp_msgs::NameHeard::TYPE,
                )
                .inspect_err(|why| {
                    eprintln!(
                        "lamp-voice second: the lamp's name cannot be taken from the bus, so whom it answers goes without it: {why}"
                    );
                })
                .ok()?;
            Some((bus, names))
        });
        Self {
            attention,
            strict,
            night: crate::local_time::night_here,
            bus,
            thread,
            levels: None,
            levels_looks: Arc::default(),
            near_sign: false,
            addressed: None,
            judged: None,
            keeps_room: false,
            name_note: None,
            noted: None,
            alone: None,
            busy: None,
            policy: None,
            waiting: None,
            by_name: None,
            session: 1,
            session_for: None,
            held_run: 0,
            renewed_at: None,
            called_at: None,
            said_until_us: None,
            policed: None,
            misheard_hum: false,
            quiet_same_one: false,
            eyes: None,
            caller_faces: Vec::new(),
            cut_in: None,
            others_answer: false,
            going_on_same_one: false,
            carries: false,
            carrying: false,
            carry_window: carry_window::CarryWindow::default(),
            carried: None,
            carried_last: None,
            carry_others: Vec::new(),
            held_colleagues: held_colleagues::HeldColleagues::default(),
            sure_other_samples: CARRY_SURE_OTHER_SPEECH_SAMPLES,
            began_over: None,
            voices: None,
            voice_ends_going_on: false,
            named_always: false,
            named_faced: None,
            not_own: false,
            chain: Vec::new(),
            said_until_free_us: None,
            busy_called_strict: false,
            turned_speaking: false,
            turned_face_speaks: false,
            face_not_speaking: None,
            name_or_follow_up: false,
            faces_held: None,
            name_stays: false,
            said_lately: std::collections::VecDeque::new(),
            spoke: std::collections::VecDeque::new(),
            near: None,
            turned_near_speaks: false,
            near_looks: None,
            rooms: None,
            in_view: None,
            another_person: None,
            first_seconds_quiet: false,
            another_since_start: false,
            held_since_start: false,
            said: std::collections::VecDeque::new(),
        }
    }

    /// `LAMP_JOIN_CUT_WORDS`: how the room was as each line was judged is
    /// kept (the faces in the eyes' newest look, a busy room), and another
    /// person heard or seen speaking in the conversation, so that a line
    /// let out may be joined with the one it cut short on no sign of a
    /// second person ([`Self::same_speaker`]).
    pub(crate) fn keeping_line_rooms(&mut self) {
        self.rooms = Some(std::collections::VecDeque::new());
    }

    /// `LAMP_FIRST_SECONDS_QUIET` (#3711): a line kept for a join
    /// ([`Self::keep_line_room`]) in a room the route keeps itself, unknown
    /// only for want of history right after a start
    /// ([`crate::room_told::RoomTold::unknown_for_want_of_history`]), is kept
    /// as said in a quiet room when the ears' own stream shows one
    /// ([`Self::ears_show_quiet`]), no other person was heard or seen
    /// speaking in any line since the start, and the rule held no line since
    /// the start. With no voice check (`LAMP_SAME_VOICE_MODEL` unset) a
    /// line in another voice with no face seen speaking it shows no other
    /// person; held as said to nobody, it is no talk left alone for the room
    /// told either (`RoomTold::held`), so the hold itself keeps the join off
    /// (#3801's P3). On lamp-4ace on 26 Oct, a bare
    /// "Lamp.", a pause, then the request, the first line after a start
    /// with nobody in view, was held and had to be said twice: no eyes and
    /// an unknown room never joined. Only the join reads it: the rule's
    /// verdicts and the room told are as before.
    pub(crate) fn showing_the_first_seconds_quiet(&mut self) {
        self.first_seconds_quiet = true;
    }

    /// `request` was said over `window`, for the ears' stream
    /// ([`Self::ears_show_quiet`]), with `LAMP_FIRST_SECONDS_QUIET`.
    fn said_over(&mut self, request: u64, window: (u64, u64)) {
        if !self.first_seconds_quiet {
            return;
        }
        if self.said.len() == SPOKE_KEPT {
            self.said.pop_front();
        }
        self.said.push_back((request, window));
    }

    /// Whether the ears show a quiet room up to `request`'s end, by their
    /// own stream rather than one level for every lamp: the readings kept
    /// ([`Self::hear_levels`], the last 30 s) outside the lines said, at
    /// least [`FIRST_SECONDS_READINGS`] of them, all under
    /// [`FIRST_SECONDS_RISE_DB`] over the room's own quiet, and that quiet
    /// (`ears/level`'s, fresh within a second) under a loud room's
    /// ([`LOUD_ROOM_FLOOR_DB`]). The quiet and the most it rose; none
    /// without the bus, a fresh quiet, `request`'s window or enough
    /// readings.
    fn ears_show_quiet(&self, request: u64) -> Option<(f32, f32)> {
        let (bus, _) = self.bus.as_ref()?;
        let message = bus.latest(lamp_msgs::RoomSound::TOPIC).ok()??;
        if unix_ms(SystemTime::now()).saturating_sub(message.header.at_unix_ms) >= 1_000 {
            return None;
        }
        let floor = message
            .parse::<lamp_msgs::RoomSound>()
            .ok()?
            .floor_db
            .filter(|floor| *floor <= LOUD_ROOM_FLOOR_DB)?;
        let (_, (_, ended)) = self.said.iter().rev().find(|(of, _)| *of == request)?;
        let until = ended.saturating_add(SAID_AFTER_US);
        let levels = self.levels.as_ref()?.lock().ok()?;
        let outside: Vec<f32> = levels
            .iter()
            .filter(|(at, _)| {
                *at <= until
                    && !self.said.iter().any(|(_, (began, ended))| {
                        (began.saturating_sub(SAID_BEFORE_US)..=ended.saturating_add(SAID_AFTER_US))
                            .contains(at)
                    })
            })
            .map(|(_, rise)| *rise)
            .collect();
        if outside.len() < FIRST_SECONDS_READINGS {
            return None;
        }
        let most = outside.into_iter().reduce(f32::max)?;
        (most < FIRST_SECONDS_RISE_DB).then_some((floor, most))
    }

    /// A line that begins with the lamp's name, said to a face turned to the
    /// lamp, is answered in a busy room (`LAMP_NAMED_ALWAYS=on`, #2191): the
    /// strict busy rule's and `busy_bar`'s holds are not kept for it
    /// ([`NAMED_TO_A_FACE`]), and the model's `not_for_me` on it is not
    /// taken ([`Self::named_to_a_face`]).
    pub(crate) fn always_answering_named(&mut self) {
        self.named_always = true;
    }

    /// The lamp's own answers to lines let out only as the conversation
    /// going on keep no conversation going (`LAMP_GOING_ON_NOT_OWN=on`, the
    /// first builder's 17:2x task on Queue #1722): on lamp-4ace at 16:34, after
    /// one question to the lamp, two people's own talk was answered line
    /// after line, each answer opening the window for the next. The window
    /// then runs from the lamp's words before the first such line, as they
    /// stood, its answers to such lines left out, until a line said to it (a
    /// call, a face, a reply to its question) is answered. Its other lines
    /// (a reminder, news, a skill's say, an answer to a line said to it)
    /// open their window as before, and a reply to the lamp's own question
    /// is judged as before. Only a line surely another person's joins the
    /// chain: another face than the caller's seen speaking it (the first
    /// builder at 01:02 on Queue #1722: on lamp-4ace at 00:32 one person's
    /// follow-ups without the name were held, day lines 12/12 to 9/12).
    /// With nothing to tell the speakers apart the line is the same
    /// person's, and its answer renews the window as without the switch.
    pub(crate) fn not_counting_own_answers(&mut self) {
        self.not_own = true;
    }

    /// In a busy room with nobody turned to the lamp, the text shows a call
    /// only as the name set off as one, or with the name heard
    /// (`LAMP_BUSY_CALLED_STRICT=on`).
    pub(crate) fn narrowing_busy_calls(&mut self) {
        self.busy_called_strict = true;
    }

    /// A face turned to the lamp counts as nobody turned to it when the eyes
    /// saw the only faces turned to it silent through the line
    /// (`LAMP_TURNED_SPEAKING=on`,
    /// [`crate::speaking_faces::SpeakingFaces::turned_only_silent`]).
    pub(crate) fn counting_only_turned_speakers(&mut self) {
        self.turned_speaking = true;
    }

    /// A face turned to the lamp lets a line out only when it is seen
    /// speaking the line, or the line is in the conversation's voice
    /// (`LAMP_TURNED_FACE_SPEAKS=on`, [`Self::silence_turned`]).
    pub(crate) fn needing_the_turned_face_to_speak(&mut self) {
        self.turned_face_speaks = true;
    }

    /// With people talking at a desk, a line is the lamp's by its name, a
    /// tap, its words calling it, or a follow-up said soon after it spoke
    /// (`LAMP_NAME_OR_FOLLOW_UP=on`, [`crate::name_or_follow_up`]): in a busy
    /// room a face turned to the lamp lets no line out alone, nor makes a
    /// call that carries the next lines
    /// ([`crate::attention::Attention::face_shows`]); and a conversation goes
    /// on only for a line begun within
    /// [`crate::name_or_follow_up::follow_up_for`] of the end of the lamp's
    /// own last words, or of a call let out ([`follows_up`]). Two faces in
    /// view make the room busy ([`Self::signs_begun`]).
    pub(crate) fn needing_name_or_follow_up(&mut self) {
        self.name_or_follow_up = true;
        self.attention.need_name_or_follow_up(true);
    }

    /// With [`Self::needing_name_or_follow_up`], two faces make the room
    /// busy only when they stay ([`FacesHeld`], `LAMP_TWO_FACES_HELD=on`),
    /// not on one look.
    pub(crate) fn holding_two_faces(&mut self, root: Option<&Path>) {
        let held: Arc<Mutex<FacesHeld>> = Arc::default();
        // Weak: the thread ends once this `Heeding` is gone.
        let kept = Arc::downgrade(&held);
        let faces = root.and_then(|root| {
            lamp_bus::Bus::new(root)
                .and_then(|bus| {
                    bus.subscribe_typed(
                        lamp_msgs::Faces::TOPIC,
                        "second_faces",
                        lamp_msgs::Faces::TYPE,
                    )
                })
                .inspect_err(|why| {
                    eprintln!(
                        "lamp-voice second: the eyes' looks cannot be taken from the bus, so two faces make no room busy: {why} (LAMP_TWO_FACES_HELD)"
                    );
                })
                .ok()
        });
        if let Some(mut faces) = faces {
            let _ = std::thread::Builder::new()
                .name("second-faces".into())
                .spawn(move || {
                    loop {
                        // Read before waiting: a look taken across a privacy
                        // change is refused.
                        let Some(epoch) = kept
                            .upgrade()
                            .and_then(|held| held.lock().ok().map(|held| held.epoch))
                        else {
                            return;
                        };
                        let message = faces.recv(Duration::from_secs(1));
                        let Some(held) = kept.upgrade() else { return };
                        match message {
                            Ok(Some(message)) => {
                                if let Ok(seen) = message.parse::<lamp_msgs::Faces>()
                                    && let Ok(mut held) = held.lock()
                                {
                                    let look = FacesLook {
                                        at_us: message.header.at_us,
                                        faces: seen.faces.len(),
                                        expires_ms: message.header.expires_ms,
                                    };
                                    held.look(look, epoch, monotonic_us());
                                }
                            }
                            Ok(None) => {}
                            Err(_) => std::thread::sleep(Duration::from_secs(1)),
                        }
                    }
                });
        }
        self.faces_held = Some(held);
    }

    /// From now on, a hearing of the lamp's name that began inside the
    /// sound of a line judged lately, before the line in hand began, is that
    /// line's, and is not among the line in hand's signs ([`Self::heed_signs`]):
    /// not its name heard, its name's note, nor its name said to a face. A
    /// line going on from the one before keeps it, so does the line after one
    /// whose words called the lamp by name ("Lamp."), and so does a hearing
    /// at the line before's start ([`crate::attention::NAME_AT_THE_START_US`]),
    /// a call however its words were written; a hearing in its own window is
    /// its own. `lamp-name` tells a hearing once its score falls,
    /// up to 0.8 s after it began, so one from a line's last words can come
    /// after that line is judged. On lamp-4ace on 2026-10-26 at 06:21 (build
    /// 6919, the busy scene played) a hearing at 0.92 in "Can you pass me
    /// the charger? The white one.", which has no name in it, counted for
    /// "Here you go. Is your phone dying again?", begun 0.7 s after it, and
    /// that was answered (`LAMP_NAME_STAYS_IN_ITS_LINE=on`).
    pub(crate) fn keeping_the_name_in_its_line(&mut self) {
        self.name_stays = true;
        self.attention.keep_the_name_in_its_line(true);
    }

    /// With [`Self::needing_name_or_follow_up`], whether the line in hand,
    /// by when its window began ([`crate::attention::Attention::window`]),
    /// follows up the lamp's words, its answers to `chain` left out as
    /// [`Self::voice_going_on`] leaves them, or its call; without it,
    /// always.
    fn follows_up_from_its_start(&self) -> bool {
        if !self.name_or_follow_up {
            return true;
        }
        let now_us = monotonic_us();
        let began_ago = self
            .attention
            .window()
            .map_or(Duration::ZERO, |(began, _)| {
                Duration::from_micros(now_us.saturating_sub(began))
            });
        let said_ago = crate::thread::current(self.thread.as_deref(), |thread| {
            thread.said_last_ago_but(Instant::now(), &self.chain)
        })
        .flatten();
        let until_us = if self.chain.is_empty() {
            self.said_until_us
        } else {
            self.said_until_free_us
        };
        follows_up(
            to_the_end(said_ago, ended_ago(until_us, now_us)),
            self.called_at.map(|at| at.elapsed()),
            began_ago,
        )
    }

    /// Whether one person said `cut`, a line cut short before any of its
    /// answer was heard, and `request`, the line that cut it
    /// (`LAMP_JOIN_CUT_WORDS`, `Engine::joining_cut_words`):
    /// - the eyes first: more than one face seen speaking either line, or
    ///   each line's face another, is two people, whatever the voices say;
    /// - their prints, when both have come: under
    ///   [`crate::same_voice::DIFFERENT_BELOW`] two people, whatever the eyes
    ///   saw; at or over [`crate::same_voice::SAME_FROM`] the same one;
    /// - else the eyes: one face, the same passing id, seen speaking each
    ///   line and no other face speaking either;
    /// - else no sign of a second person ([`Self::no_second_person`]),
    ///   which joins only a line let out ([`Joined::NoSecond`]);
    /// - a print still to come (said with the lamp quiet, long enough to
    ///   know) is waited for, with what is known without it for when the
    ///   wait is over.
    ///
    /// Otherwise two people, or nothing to tell: the cut words go, as
    /// without it.
    pub(crate) fn same_speaker(&mut self, cut: u64, request: u64) -> Joined {
        let spoke = |of: u64| {
            self.spoke
                .iter()
                .rev()
                .find(|(line, _)| *line == of)
                .map(|(_, faces)| faces.clone())
        };
        let (cut_faces, faces) = (spoke(cut), spoke(request));
        // Two people seen, whatever the voices say: more than one face
        // speaking a line, or each line's face another (the review of #2620:
        // a voice at 0.70 is 0.01 over the office set's highest two-person
        // pair, and a family's voices may be closer).
        // Each side on its own: two faces on either line are two people even
        // when the other line's record is gone, a camera change having
        // cleared it while the prints stayed (the Reviewer on #2657). Only
        // comparing one face with the other needs both.
        let several = |seen: &Option<Vec<String>>| seen.as_ref().is_some_and(|seen| seen.len() > 1);
        let another = match (&cut_faces, &faces) {
            (Some(a), Some(b)) => !a.is_empty() && !b.is_empty() && a != b,
            _ => false,
        };
        if several(&cut_faces) || several(&faces) || another {
            return Joined::Not("two faces were seen speaking them".to_owned());
        }
        let likeness = self
            .voices
            .as_mut()
            .and_then(|voices| voices.alike(cut, request));
        match likeness {
            Some(likeness) if likeness < crate::same_voice::DIFFERENT_BELOW => {
                return Joined::Not(format!("two voices (cosine {likeness:.2})"));
            }
            Some(likeness) if likeness >= crate::same_voice::SAME_FROM => {
                return Joined::Same(SAME_VOICE_JOINS);
            }
            _ => {}
        }
        let one_face = match (cut_faces.as_deref(), faces.as_deref()) {
            (Some([a]), Some([b])) => a == b,
            _ => false,
        };
        let coming = likeness.is_none()
            && self
                .voices
                .as_ref()
                .is_some_and(|voices| voices.print_coming(cut) || voices.print_coming(request));
        // What was missing, for the journal: the voices (a print still to
        // come is said once its wait is over), the eyes, then the sign of a
        // second person.
        let voice = match (likeness, self.voices.as_mut()) {
            _ if coming => String::new(),
            (Some(likeness), _) => {
                format!("a likeness between the bars (cosine {likeness:.2}), and ")
            }
            (None, None) => "no voice check, and ".to_owned(),
            (None, Some(voices)) => match (voices.printed(cut), voices.printed(request)) {
                (false, false) => "neither has a voice print, and ".to_owned(),
                (false, true) => format!("request {cut} has no voice print, and "),
                _ => format!(
                    "request {request} has no voice print (too short, or begun over the lamp's sound), and "
                ),
            },
        };
        let eyes = if self.eyes.is_none() {
            "no eyes read"
        } else {
            "no one face was seen speaking both"
        };
        // Without a print: one face speaking both, else no sign of a second
        // person.
        let without = if one_face {
            Joined::Same(SAME_FACE_JOINS)
        } else {
            match self.no_second_person(cut, request) {
                Ok(why) => Joined::NoSecond(why),
                Err(sign) => Joined::Not(format!("{voice}{eyes}; {sign}")),
            }
        };
        if coming {
            Joined::Coming(Box::new(without))
        } else {
            without
        }
    }

    /// How the room was as `request` was judged, with
    /// [`Self::keeping_line_rooms`]: the faces in view read for its signs,
    /// and the room it was judged in. Another person heard or seen speaking
    /// it (`another`) is kept until a line is judged with no conversation
    /// going on.
    fn keep_line_room(
        &mut self,
        request: u64,
        room: crate::room_told::Room,
        going_on: bool,
        another: bool,
    ) {
        if self.rooms.is_none() {
            return;
        }
        if another {
            self.another_since_start = true;
        }
        // `LAMP_FIRST_SECONDS_QUIET`: right after a start, the ears' quiet
        // room, never just an unknown one.
        let room = if self.first_seconds_quiet
            && self.keeps_room
            && room == crate::room_told::Room::Unknown
            && !self.another_since_start
            && !self.held_since_start
            && crate::room_told::room()
                .lock()
                .is_ok_and(|mut told| told.unknown_for_want_of_history(monotonic_us()))
            && let Some((floor, most)) = self.ears_show_quiet(request)
        {
            eprintln!(
                "lamp-voice second: request {request}: the room is not known yet after the start, and the ears' stream stayed within {most:.1} dB of its quiet of {floor:.1} dBFS outside the lines, with no talk left alone, no line held and no other voice since: kept as said in a quiet room for a join (LAMP_FIRST_SECONDS_QUIET)"
            );
            crate::room_told::Room::Quiet
        } else {
            room
        };
        let in_view = self
            .in_view
            .take()
            .filter(|(of, _)| *of == request)
            .and_then(|(_, faces)| faces);
        let Some(rooms) = self.rooms.as_mut() else {
            return;
        };
        if rooms.len() == SPOKE_KEPT {
            rooms.pop_front();
        }
        rooms.push_back((request, LineRoom { in_view, room }));
        if !going_on {
            self.another_person = None;
        }
        if another {
            self.another_person = Some(request);
        }
    }

    /// The faces seen speaking `request` and how the room was, kept for
    /// [`Self::same_speaker`]: another person shown by another voice than
    /// the conversation's, a print under
    /// [`crate::same_voice::DIFFERENT_BELOW`] against it, or two faces
    /// speaking it.
    fn keep_spoken(
        &mut self,
        request: u64,
        speakers: &crate::speaking_faces::Speakers,
        (room, going_on): (crate::room_told::Room, bool),
        other_voice: bool,
        likeness: Option<f32>,
    ) {
        if self.spoke.len() == SPOKE_KEPT {
            self.spoke.pop_front();
        }
        self.spoke.push_back((request, speakers.clone()));
        let another = other_voice
            || likeness.is_some_and(|likeness| likeness < crate::same_voice::DIFFERENT_BELOW)
            || speakers.len() > 1;
        self.keep_line_room(request, room, going_on, another);
    }

    /// `LAMP_SECOND_NAME_CARRIES`: a line said in `began_us..=ended_us` that
    /// the rule has not judged, kept as a judgement keeps it, for
    /// [`Self::one_speaker`]: the faces seen speaking it, the eyes' newest
    /// look, and the room now. The lamp's name waiting for the line after
    /// it, and that line before it is judged with the name or without.
    /// Nothing of the bus's name hearings is read, so they are still there
    /// for the line that is judged; and no conversation's end clears
    /// another person seen in it.
    pub(crate) fn keep_unjudged(&mut self, request: u64, (began_us, ended_us): (u64, u64)) {
        self.said_over(request, (began_us, ended_us));
        let room = if self.keeps_room {
            crate::room_told::room()
                .lock()
                .map_or(crate::room_told::Room::Unknown, |mut room| {
                    room.at(monotonic_us())
                })
        } else {
            self.bus
                .as_ref()
                .map_or(crate::room_told::Room::Unknown, |(bus, _)| {
                    crate::bus_out::room_now(bus)
                })
        };
        if self.rooms.is_some() {
            let in_view = self
                .bus
                .as_ref()
                .and_then(|(bus, _)| crate::bus_out::faces_in_view(bus));
            self.in_view = Some((request, in_view));
        }
        let (speakers, _) = speakers_between(self.eyes.as_ref(), began_us, ended_us);
        let likeness = self
            .voices
            .as_mut()
            .and_then(|voices| voices.likeness(request));
        self.keep_spoken(request, &speakers, (room, true), false, likeness);
    }

    /// `LAMP_NAME_NEAR=on`: lamp-name's near hearings (`name/near`) are read
    /// for a line's first word ([`Self::near_name`]). Whether they can be.
    pub(crate) fn hearing_near(&mut self) -> bool {
        if self.near.is_some() {
            return true;
        }
        let Some((bus, _)) = &self.bus else {
            return false;
        };
        match bus.subscribe_typed(
            lamp_msgs::NameHeard::NEAR_TOPIC,
            "second",
            lamp_msgs::NameHeard::TYPE,
        ) {
            Ok(near) => {
                self.near = Some((near, std::collections::VecDeque::new()));
                true
            }
            Err(why) => {
                eprintln!(
                    "lamp-voice second: lamp-name's near hearings cannot be taken from the bus: {why} (LAMP_NAME_NEAR)"
                );
                false
            }
        }
    }

    /// `LAMP_NAME_NEAR` with the eyes reading speaking (`LAMP_EYES_SPEAKING`):
    /// a face turned to the lamp counts for a first word only when it was
    /// seen speaking the line ([`Self::turned_to_the_lamp`]).
    pub(crate) fn turned_near_speaking(&mut self, on: bool) {
        self.turned_near_speaks = on;
    }

    /// `LAMP_NAME_NEAR`: the eyes' looks are kept for each line's window
    /// ([`Self::turned_to_the_lamp`]): `--addressed`'s own when it was set
    /// up first ([`Self::judging_addressed`], as `serve` does), so one
    /// reader keeps them; else read here from the bus at `root` every
    /// [`LEVEL_EVERY`] on a thread of its own, which ends with this rule.
    pub(crate) fn looking_for_near(&mut self, root: &Path) {
        if self.near_looks.is_some() {
            return;
        }
        if let Some((looks, _)) = &self.addressed {
            self.near_looks = Some(looks.clone());
            return;
        }
        let Ok(bus) = lamp_bus::Bus::new(root) else {
            return;
        };
        let looks = Looked::default();
        let kept = Arc::downgrade(&looks);
        let started = std::thread::Builder::new()
            .name("second-near-looks".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(LEVEL_EVERY);
                    let Some(looks) = kept.upgrade() else { return };
                    if let Ok(Some(message)) = bus.latest(lamp_msgs::Faces::TOPIC)
                        && let Ok(mut looks) = looks.lock()
                    {
                        looks.read(&message, monotonic_us());
                    }
                }
            });
        if started.is_ok() {
            self.near_looks = Some(looks);
        }
    }

    /// Whether a face turned to the lamp said the line in
    /// `began_us..=ended_us`, for a first word of the lamp's name's shape
    /// (`LAMP_NAME_NEAR`), judged over the line's own window as
    /// `--addressed` judges a close face turned to the lamp
    /// ([`crate::addressed::Looks::between`], Sixth's P2s on 1600b0a69):
    /// from 1.5 s before the line to its end, at least two looks, a close
    /// face ([`crate::addressed::CLOSE_FACE_HEIGHT`]) turned to the lamp in
    /// [`crate::addressed::FACING_SHARE`] of them, never a face turning to
    /// it after the line, nor one far off. Then as the rule reads it: with
    /// `LAMP_TURNED_SPEAKING`, a face turned to it but seen silent through
    /// the line is nobody turned to it ([`Self::turned_but_silent`], the P3);
    /// with speaking faces read ([`Self::turned_near_speaking`]), one must
    /// be seen speaking the line too ([`speakers_between`]), so the
    /// listener's face never carries a colleague's "Leon, can you…"
    /// (reviewer-three's P3 on 1600b0a69). No looks kept: no.
    pub(crate) fn turned_to_the_lamp(&self, (began_us, ended_us): (u64, u64)) -> bool {
        let facing = self
            .near_looks
            .as_ref()
            .and_then(|looks| {
                looks
                    .lock()
                    .ok()
                    .map(|looks| looks.between(began_us, ended_us))
            })
            .and_then(|seen| seen.share)
            .is_some_and(|share| share >= crate::addressed::FACING_SHARE);
        let speaking = !self.turned_near_speaks || {
            let (_, turned_speakers) = speakers_between(self.eyes.as_ref(), began_us, ended_us);
            !turned_speakers.is_empty()
        };
        facing && speaking && !self.turned_but_silent((began_us, ended_us))
    }

    /// The best score of lamp-name's near hearings (`name/near`) that
    /// reached their bar from [`NEAR_BEFORE_US`] before `began_us`, a
    /// line's first sound, to [`crate::attention::NAME_AT_THE_START_US`]
    /// (1.3 s) after it: none without [`Self::hearing_near`] or any such.
    /// Hearings are kept [`NEAR_KEPT_US`], so a line asks of its own start
    /// whatever came since.
    pub(crate) fn near_name(&mut self, began_us: u64) -> Option<f32> {
        let (near, kept) = self.near.as_mut()?;
        let now_us = monotonic_us();
        let now_ms = unix_ms(SystemTime::now());
        while let Ok(Some(message)) = near.recv(Duration::ZERO) {
            if let Ok(heard) = message.parse::<lamp_msgs::NameHeard>() {
                let ago_ms =
                    now_ms.saturating_sub(message.header.at_unix_ms) + u64::from(heard.ago_ms);
                if kept.len() == NEAR_HEARINGS_KEPT {
                    kept.pop_front();
                }
                kept.push_back((now_us.saturating_sub(ago_ms * 1_000), heard.score));
            }
        }
        kept.retain(|(at, _)| now_us.saturating_sub(*at) <= NEAR_KEPT_US);
        let from = began_us.saturating_sub(NEAR_BEFORE_US);
        let until = began_us.saturating_add(crate::attention::NAME_AT_THE_START_US);
        kept.iter()
            .filter(|(at, _)| (from..=until).contains(at))
            .map(|(_, score)| *score)
            .reduce(f32::max)
    }

    /// Whether one person said `name`, a line that was the lamp's name (or
    /// short and begun with it), and `request`, the line begun while it
    /// waited (`LAMP_SECOND_NAME_CARRIES`): [`Self::same_speaker`]'s
    /// judgement, the join's own, with its no sign of a second person
    /// ([`Joined::NoSecond`]) only when both were said in a quiet room. One
    /// face in view in a busy room, or in one not known to be quiet, may be
    /// a person turned away from the lamp while another speaks: the name
    /// never carries on it.
    pub(crate) fn one_speaker(&mut self, name: u64, request: u64) -> Joined {
        let quiet = |heeding: &Self| {
            [name, request].iter().all(|of| {
                heeding
                    .rooms
                    .as_ref()
                    .and_then(|rooms| rooms.iter().rev().find(|(line, _)| line == of))
                    .is_some_and(|(_, room)| room.room == crate::room_told::Room::Quiet)
            })
        };
        let in_quiet = |said: Joined, heeding: &Self| match said {
            Joined::NoSecond(why) if !quiet(heeding) => {
                Joined::Not(format!("{why}, but not both in a quiet room"))
            }
            said => said,
        };
        match self.same_speaker(name, request) {
            Joined::Coming(without) => Joined::Coming(Box::new(in_quiet(*without, self))),
            said => in_quiet(said, self),
        }
    }

    /// Whether nothing shows a second person at `cut` and `request` by how
    /// the room was as each was judged ([`Self::keeping_line_rooms`]): no
    /// other voice heard and no two faces seen speaking in the conversation
    /// ([`Self::keep_line_room`]), the eyes' newest look at each
    /// showing one face at most, and with no face in view at either (nobody
    /// in view, or no eyes: not running, or the camera closed) both judged
    /// in a quiet room, the one sign left: never an unknown one, which is
    /// the room after a start or with its facts off, and every two-person
    /// scene's first minute (a reviewer's P2, 22 Oct). Two faces seen speaking are
    /// [`Self::same_speaker`]'s to tell, before this. What was seen, in
    /// counts, or the sign of a second person. Nothing kept of a line (the
    /// switch off, a privacy change since) is no join.
    fn no_second_person(&self, cut: u64, request: u64) -> Result<String, String> {
        let kept = |of: u64| {
            self.rooms
                .as_ref()?
                .iter()
                .rev()
                .find(|(line, _)| *line == of)
                .map(|(_, room)| *room)
        };
        let (before, now) = match (kept(cut), kept(request)) {
            (Some(before), Some(now)) => (before, now),
            (None, _) => return Err(format!("nothing kept of the room at request {cut}")),
            (_, None) => return Err(format!("nothing kept of the room at request {request}")),
        };
        if let Some(other) = self.another_person {
            return Err(format!(
                "another voice or two faces speaking in the conversation (request {other})"
            ));
        }
        let seen = |room: LineRoom| match room.in_view {
            Some(1) => "1 face in view".to_owned(),
            Some(0) => "no face in view".to_owned(),
            Some(faces) => format!("{faces} faces in view"),
            None => "no fresh look from the eyes".to_owned(),
        };
        let looks = format!(
            "{} at request {cut}'s end, {} at request {request}'s",
            seen(before),
            seen(now)
        );
        if [before, now].iter().any(|room| room.in_view > Some(1)) {
            return Err(looks);
        }
        let one_each = before.in_view == Some(1) && now.in_view == Some(1);
        if !one_each {
            use crate::room_told::Room;
            if [before, now].iter().any(|line| line.room == Room::Busy) {
                return Err(format!("{looks}, in a busy room"));
            }
            if [before, now].iter().any(|line| line.room != Room::Quiet) {
                return Err(format!("{looks}, in a room not known to be quiet"));
            }
        }
        Ok(format!(
            "no sign of a second person: {looks}{}, no other voice heard and no two faces seen speaking in the conversation",
            if one_each { "" } else { ", a quiet room" }
        ))
    }

    /// Whether `request` was judged as begun with the lamp's name said to
    /// a face turned to the lamp, with the switch.
    pub(crate) fn named_to_a_face(&self, request: u64) -> bool {
        self.named_faced == Some(request)
    }

    /// A line in another voice than the conversation's is no part of it
    /// (#829), judged by `printer` as the Live worker judges it: with
    /// `LAMP_SAME_VOICE_MODEL`, beside `LAMP_SPOKEN_TO=on`, or here beside
    /// `LAMP_QUIET_GOING_ON_SAME_ONE=on` or `LAMP_GOING_ON_NOT_OWN=on`
    /// ([`checking_the_voice_for`]).
    pub(crate) fn checking_the_voice(&mut self, printer: Box<dyn crate::same_voice::Printer>) {
        self.voices = Some(
            crate::same_voice::Voices::new(printer).callers_only(
                std::env::var(crate::same_voice::CALLER_ONLY_ENVIRONMENT)
                    .is_ok_and(|value| value == "on"),
            ),
        );
    }

    /// A spoken line's sound, for the same-voice check: judged only if it
    /// began with the lamp quiet, as on Live.
    pub(crate) fn hear(&mut self, request: u64, heard: crate::second_provider::Heard<'_>) {
        use crate::second_provider::Heard;
        if let Heard::Began {
            over_the_lamps_voice,
        } = heard
        {
            self.began_over = over_the_lamps_voice.then_some(request);
        }
        if matches!(heard, Heard::BeganOverTheWaitSound) {
            self.began_over = None;
        }
        let Some(voices) = self.voices.as_mut() else {
            return;
        };
        match heard {
            Heard::Began {
                over_the_lamps_voice,
            } => voices.began(request, !over_the_lamps_voice),
            // The wait sound is in its first samples: no print.
            Heard::BeganOverTheWaitSound => voices.began(request, false),
            Heard::Samples(samples) => voices.heard(request, samples),
            Heard::Ended => voices.ended(request),
        }
    }

    /// Whether a conversation goes on for `request`, `going_on` as the rule
    /// read it, with the same-voice check, as the Live worker judges it
    /// (`provider_worker.rs`, #829): a line in another voice than the
    /// conversation's is no part of it; a late "another voice" from the line
    /// before ends it; after another voice's line held, a question in the
    /// conversation's own voice goes on, a statement in it is talk past the
    /// lamp. The second field: held as another voice; the third: found in
    /// the conversation's own voice.
    #[cfg(test)]
    fn voice_going_on(&mut self, request: u64, going_on: bool) -> (bool, bool, bool, Option<f32>) {
        self.voice_going_on_in(request, going_on, false)
    }

    /// [`Self::voice_going_on`] for a line said in a busy room or not.
    fn voice_going_on_in(
        &mut self,
        request: u64,
        going_on: bool,
        busy: bool,
    ) -> (bool, bool, bool, Option<f32>) {
        // `LAMP_NAME_OR_FOLLOW_UP`: in a busy room, a conversation another
        // voice ended goes on again only soon after the lamp's words, as any
        // other does.
        let follows_up = !busy || self.follows_up_from_its_start();
        let Some(voices) = self.voices.as_mut() else {
            return (going_on, false, false, None);
        };
        // Without `LAMP_SPOKEN_TO` (#829's check never ran there) the print
        // is only read, for [`Self::someone_else`] and a trusted session's
        // line, going on or not: a person's own short follow-up that reads
        // low still goes on (the first builder at 09:14 on #2428).
        if !self.voice_ends_going_on {
            return (going_on, false, false, voices.likeness(request));
        }
        let attention = &self.attention;
        let under_the_mark = !going_on
            && crate::room_told::room()
                .lock()
                .is_ok_and(|room| room.ended_only_by_another_voice())
            // `LAMP_GOING_ON_NOT_OWN`: the answers to `chain` left out.
            && crate::thread::current(self.thread.as_deref(), |thread| {
                thread
                    .said_last_ago_but(Instant::now(), &self.chain)
                    .is_some_and(|ago| ago <= crate::thread::GOING_ON_FOR)
            })
            .unwrap_or_default()
            && follows_up;
        let mut at = voices.at_verdict(
            request,
            going_on || under_the_mark,
            attention.called_for_itself(),
        );
        // `LAMP_SAME_SPEAKER_CARRIES`: a print come late of a line older than
        // the newest one carried by a sure sign undoes nothing newer.
        if self.carries
            && let Some(late) = at.late_other
            && self.carried_last.is_some_and(|carried| carried > late)
        {
            eprintln!(
                "lamp-voice second: request {request}: request {late}'s voice, known late as another's, is older than request {:?}, carried since by a sure sign: it ends nothing (LAMP_SAME_SPEAKER_CARRIES)",
                self.carried_last
            );
            at.late_other = None;
        }
        if let Some(late) = at.late_other {
            // Someone else's after all: a line from someone else, as
            // [`Self::colleague_held`] counts them (its fields: `voices` is
            // borrowed). With the carry standing after it, it ends nothing.
            if self.carries {
                let print = voices.print_of(late);
                self.held_colleagues.hold(late, print);
            }
            let stands =
                self.carries && count_colleague(&mut self.carry_others, &mut self.carrying, late);
            if stands {
                self.carry_window.held(late);
                eprintln!(
                    "lamp-voice second: request {request}: request {late}'s voice, known after its answer, was another's: one line from someone else, the carry stands (LAMP_SAME_SPEAKER_CARRIES)"
                );
            } else {
                self.carrying = false;
                self.carry_window.clear();
                self.held_colleagues.clear();
                eprintln!(
                    "lamp-voice second: request {request}: no conversation is going on for it: request {late}'s voice, known after its answer, was another's"
                );
                if let Ok(mut told) = crate::room_told::room().lock() {
                    told.another_voice_held(late);
                }
            }
        }
        if let Some(why) = at
            .unjudged
            .filter(|_| at.late_other.is_none() && (going_on || under_the_mark))
        {
            voices.judged_without(request, why);
        }
        let talked_past = |why: &str| {
            eprintln!("lamp-voice second: request {request}: {why}");
            if let Ok(mut told) = crate::room_told::room().lock() {
                told.own_voice_talked();
            }
            (false, false, false)
        };
        let (going_on, other, same) = match at.likeness {
            _ if at.late_other.is_some() => (false, false, false),
            Some(likeness) if likeness < crate::same_voice::DIFFERENT_BELOW => {
                eprintln!(
                    "lamp-voice second: request {request}: another voice than the conversation's (cosine {likeness:.2}): no conversation is going on for it"
                );
                (false, true, false)
            }
            Some(likeness) if under_the_mark && attention.asks_a_question() => {
                eprintln!(
                    "lamp-voice second: request {request}: the conversation's voice (cosine {likeness:.2}), a question after another voice's line: the conversation goes on"
                );
                (true, false, true)
            }
            Some(likeness) if under_the_mark => talked_past(&format!(
                "the conversation's voice (cosine {likeness:.2}), no question after another voice's line: talk past the lamp, and the conversation is over"
            )),
            None if under_the_mark && !attention.asks_a_question() && at.unjudged.is_some() => {
                talked_past(
                    "no question after another voice's line, its voice not known in time: taken for talk past the lamp, and the conversation is over",
                )
            }
            Some(likeness) => {
                eprintln!(
                    "lamp-voice second: request {request}: the conversation's voice (cosine {likeness:.2})"
                );
                (true, false, true)
            }
            None => (going_on, false, false),
        };
        (going_on, other, same, at.likeness)
    }

    /// Ask again rather than answer a line of a conversation heard in
    /// another script, as a number alone, or as a cut-in with no English
    /// word (`--ask-unclear`, `LAMP_OTHER_LANGUAGE=sorry`, #926), as the Live
    /// worker's rule does; with `misheard_hum` (`LAMP_MISHEARD_HUM`, #828) a
    /// misheard cut-in is hummed for instead.
    pub(crate) fn asking_again(&mut self, misheard_hum: bool) {
        self.attention.ask_again_when_unclear(true);
        self.misheard_hum = misheard_hum;
    }

    /// In a quiet room, a line let out only as the conversation going on is
    /// held when it is surely someone else's ([`Self::someone_else`]) and
    /// answers no question of the lamp's (`LAMP_QUIET_GOING_ON_SAME_ONE=on`,
    /// the first builder at 21:03 and 01:21 on Queue #1722). With no voice
    /// model and no speaking face nothing is held.
    pub(crate) fn going_on_quiet_only_for_the_same_one(&mut self) {
        self.quiet_same_one = true;
    }

    /// The eyes' speaking faces for that rule, kept with their `reader` (none
    /// in tests): a line the answered caller's face is seen speaking
    /// through goes on (the first builder at 23:01 on #2305).
    pub(crate) fn seeing_speaking_faces(
        &mut self,
        eyes: Arc<crate::speaking_faces::SpeakingFaces>,
        reader: Option<crate::speaking_faces::Reader>,
    ) {
        self.eyes = Some((eyes, reader));
    }

    /// The eyes' speaking faces read from the bus at `root`, once for both
    /// switches that read them (`LAMP_QUIET_GOING_ON_SAME_ONE`,
    /// `LAMP_GOING_ON_NOT_OWN`); none without a reader.
    /// ponytail: its own reader beside `LAMP_SECOND_SPEAKING_FACE`'s; one
    /// shared reader if both are ever on together.
    fn opening_the_eyes(&mut self, root: Option<&Path>) {
        if self.eyes.is_some() {
            return;
        }
        let Some(root) = root else {
            return;
        };
        let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
        let reader = crate::speaking_faces::read_from(eyes.clone(), root);
        if reader.is_some() {
            self.seeing_speaking_faces(eyes, reader);
        }
    }

    /// Whether `seen` shows a line surely someone else's than the answered
    /// caller's: its print scored under [`crate::same_voice::DIFFERENT_BELOW`]
    /// (0.30) against the caller's, or the eyes saw a face speaking it not
    /// turned to the lamp, none of them the caller's, whether or not the
    /// caller's face is known. No print (begun over the lamp, too short) is
    /// no voice told. A face turned to the lamp is talking to it, so a
    /// caller back with a new passing id is not someone else; people
    /// talking to each other face each other (the first builder at 01:51 on
    /// #2341). Nothing seen, nothing told: the same person. One check for
    /// `LAMP_GOING_ON_NOT_OWN` and `LAMP_QUIET_GOING_ON_SAME_ONE` (the first
    /// builder at 01:02 and 01:21 on Queue #1722).
    fn someone_else(&self, seen: &Seen) -> bool {
        let callers = |face: &String| self.caller_faces.contains(face);
        seen.likeness
            .is_some_and(|likeness| likeness < crate::same_voice::DIFFERENT_BELOW)
            || (!seen.speakers.iter().any(callers)
                && seen
                    .speakers
                    .iter()
                    .any(|face| !seen.turned_speakers.contains(face)))
    }

    /// The camera closed or reopened, or the microphone was muted or
    /// unmuted ([`Gate::install`]): the caller's face ids and the eyes'
    /// looks go, so no passing id outlives a privacy change.
    pub(crate) fn privacy_changed(&mut self) {
        self.caller_faces.clear();
        // No conversation is carried across it (`LAMP_SAME_SPEAKER_CARRIES`),
        // and no colleague's print is kept past it.
        self.carrying = false;
        self.carry_window.clear();
        self.held_colleagues.clear();
        // The lines' speaking faces too, for a join (`Self::same_speaker`),
        // and how the room was.
        self.spoke.clear();
        if let Some(rooms) = self.rooms.as_mut() {
            rooms.clear();
        }
        self.in_view = None;
        if let Some((eyes, _)) = &self.eyes {
            eyes.forget();
        }
        // The looks before it count for nothing after it (`LAMP_TWO_FACES_HELD`).
        if let Some(held) = &self.faces_held
            && let Ok(mut held) = held.lock()
        {
            held.privacy_changed(monotonic_us());
        }
    }

    /// `LAMP_SAME_SPEAKER_CARRIES`: whether the line `request`, begun while
    /// a carry stands ([`Self::signs_begun`]), within a follow-up's 8 s or
    /// past them, was said to the lamp by the one it answered, the same rule
    /// either way (reviewer-two at 03:46 on #3296): by `likeness`, its print against the
    /// conversation's, and the faces seen speaking it (`speakers`), those
    /// turned to the lamp among them (`turned_speakers`).
    ///
    /// The contract (the first builder at 04:33 on #3296): the first 8 s
    /// after the lamp's words are not identity-gated. Within them an
    /// uncertain, short or missing print is no evidence either way, and the
    /// line goes as an ordinary follow-up; past 8 s only a sure sign of
    /// the same speaker carries it within the bounded window. Always held, inside the 8 s too: another
    /// face seen speaking it, the caller turned away, or a sure other voice,
    /// which today is not reachable: prints carry at most 1.5 s of speech,
    /// under the provisional 2.5 s. A print that was no evidence never
    /// becomes the conversation's voice (`same_voice::Voices::adopt`).
    ///
    /// - A sure other voice (under `same_voice::DIFFERENT_BELOW` from
    ///   [`CARRY_SURE_OTHER_SPEECH_SAMPLES`] of speech or more, which no
    ///   print reaches today), or a face seen speaking it that is not the
    ///   caller's: someone else, held; the second such line in a row ends
    ///   the carry until the lamp answers again ([`Self::colleague_held`]),
    ///   and their identity is not reused after.
    /// - The caller's face seen speaking it, turned away from the lamp: the
    ///   same speaker talking to someone else (reviewer-two at 03:01 on
    ///   Queue #3126: the same speaker tells who spoke, not to whom). Held;
    ///   the carry ends, so turning back needs a fresh call or answer.
    /// - The caller's face alone seen speaking it turned to the lamp, or the
    ///   conversation's voice at `same_voice::SAME_FROM` or over: carried.
    ///   With short lines the sign is mostly the face: one speaker's short
    ///   prints rarely reach 0.70.
    /// - Else, a print not back, under 0.70 from too little speech
    ///   (lamp-4ace on 23 Oct: 1.6–1.8 s prints at 0.25 and 0.27, possibly
    ///   the owner's own), or no face seen speaking: no sign either way
    ///   (`None`), and the carry stands. The line is then judged as if no
    ///   print had come, whether it came at once, late or never: past the
    ///   8 s it is held; within them the ordinary follow-up rule lets it go
    ///   on, the legacy exception ([`Self::heed`]).
    ///
    /// Carried, the line is let out as the conversation going on, before
    /// `busy_bar`, which then still holds a short line that is no question
    /// (`BUSY_SHORT`), as it does a follow-up in the 8 s: "okay, thanks" to
    /// a colleague is no turn with the lamp, and the owner's turns are
    /// questions or three words or more. Carried and left unanswered by the
    /// model, it is no talk (`room_told::RoomTold::carried`).
    fn carried_by_the_same_speaker(
        &mut self,
        request: u64,
        likeness: Option<f32>,
        speakers: &crate::speaking_faces::Speakers,
        turned_speakers: &crate::speaking_faces::Speakers,
    ) -> Carried {
        let speech = self
            .voices
            .as_ref()
            .and_then(|voices| voices.usable_speech(request));
        let enough = speech.is_some_and(|speech| speech >= self.sure_other_samples);
        let voice = likeness.and_then(|likeness| {
            if likeness >= crate::same_voice::SAME_FROM {
                Some(true)
            } else if enough && likeness < crate::same_voice::DIFFERENT_BELOW {
                Some(false)
            } else {
                None
            }
        });
        let callers = |face: &String| self.caller_faces.contains(face);
        let other_face =
            !self.caller_faces.is_empty() && speakers.iter().any(|face| !callers(face));
        let own_face_away = speakers.iter().any(callers) && !turned_speakers.iter().any(callers);
        let own_face = spoken_by_the_caller_alone(speakers, &self.caller_faces)
            && turned_speakers.iter().any(callers);
        let heard = format!(
            "voice {}, from {} ms of speech; faces seen speaking {speakers:?}, turned to the lamp {turned_speakers:?}, the caller's {:?}",
            likeness.map_or("not known".to_owned(), |likeness| format!("{likeness:.2}")),
            speech.map_or(0, |speech| speech * 1_000 / crate::same_voice::RATE),
            self.caller_faces
        );
        let other = voice == Some(false) || other_face;
        let goes_on = if other {
            eprintln!(
                "lamp-voice second: request {request}: someone else ({heard}): held, not answered (LAMP_SAME_SPEAKER_CARRIES)"
            );
            Some(false)
        } else if own_face_away {
            // The first builder at 09:34 on #3296: turned away, the owner
            // has left the conversation, their voice sure or not.
            self.carrying = false;
            self.carry_window.clear();
            self.held_colleagues.clear();
            eprintln!(
                "lamp-voice second: request {request}: the one the lamp answered, turned away from it, talking to someone else ({heard}): not carried, and the conversation is carried no further until the lamp answers again (LAMP_SAME_SPEAKER_CARRIES)"
            );
            Some(false)
        } else if own_face
            || voice == Some(true) && self.nearer_than_the_colleague(request, likeness)
        {
            eprintln!(
                "lamp-voice second: request {request}: the one the lamp answered ({heard}): their conversation goes on (LAMP_SAME_SPEAKER_CARRIES)"
            );
            self.carried_last = Some(request);
            self.carry_others.clear();
            if let Ok(mut room) = crate::room_told::room().lock() {
                room.carried(request);
            }
            Some(true)
        } else {
            eprintln!(
                "lamp-voice second: request {request}: no sure sign who said it ({heard}): not carried; the carry stands (LAMP_SAME_SPEAKER_CARRIES)"
            );
            None
        };
        Carried {
            other,
            goes_on,
            voice,
        }
    }

    /// With held colleagues in this call, the owner's voice alone must be
    /// nearer the owner's than every retained held print by CARRY_NEARER_BY.
    /// A missing held print or exhausted bound requires the owner's face.
    fn nearer_than_the_colleague(&mut self, request: u64, likeness: Option<f32>) -> bool {
        let voices = &mut self.voices;
        let nearer = self.held_colleagues.nearer(request, likeness, |of| {
            voices.as_mut().and_then(|voices| voices.print_of(of))
        });
        if !nearer {
            eprintln!(
                "lamp-voice second: request {request}: its voice ({likeness:?}) is not nearer the conversation's than every held colleague's known print, by {CARRY_NEARER_BY}, or a held print is missing or the bound exhausted: the voice alone carries nothing (LAMP_SAME_SPEAKER_CARRIES)"
            );
        }
        nearer
    }

    /// Retain this held line's constraint without replacing an older one.
    fn hold_the_colleagues_print(&mut self, request: u64) {
        let print = self
            .voices
            .as_mut()
            .and_then(|voices| voices.print_of(request));
        self.held_colleagues.hold(request, print);
    }

    /// `LAMP_SAME_SPEAKER_CARRIES`: `request`, begun while the carry
    /// stands, was someone else's, held (the first builder at 09:34 on
    /// #3296): one such line ends nothing, and the owner goes on; a second
    /// in a row is people talking between them, as two lines left alone
    /// end a conversation (`room_told::TALK_ENDS_CONVERSATION`), and ends
    /// the carry until the lamp answers again. Whether it still stands.
    /// The line, held with the carry standing, is no talk for the room
    /// either (`room_told::RoomTold::colleague_held`).
    fn colleague_held(&mut self, request: u64) -> bool {
        let stands = count_colleague(&mut self.carry_others, &mut self.carrying, request);
        self.hold_the_colleagues_print(request);
        if stands {
            self.carry_window.held(request);
            eprintln!(
                "lamp-voice second: request {request}: one line from someone else: the conversation's carry stands, and the line makes the room neither busy nor talked past (LAMP_SAME_SPEAKER_CARRIES)"
            );
            if let Ok(mut room) = crate::room_told::room().lock() {
                room.colleague_held(request);
            }
        } else {
            self.carry_window.clear();
            self.held_colleagues.clear();
            eprintln!(
                "lamp-voice second: request {request}: {} lines in a row from someone else: the conversation is carried no further until the lamp answers again (LAMP_SAME_SPEAKER_CARRIES)",
                self.carry_others.len()
            );
        }
        stands
    }

    /// In a busy room, the one the lamp answered goes on talking with it
    /// for [`SAME_SPEAKER_CARRIES_FOR`] after each of its answers, by the
    /// line's start, extended once from a held line's actual audio end,
    /// at most forty seconds after the answer; past the follow-up's 8 s
    /// and people talking past it,
    /// by their face, the caller's alone, seen speaking the line turned to
    /// the lamp, or their voice at `same_voice::SAME_FROM` or over
    /// (`LAMP_SAME_SPEAKER_CARRIES=on`, the owner's P1 on Queue #3126;
    /// [`Self::carried_by_the_same_speaker`]). Another face seen speaking a
    /// line, or a sure other voice, is held; two such lines in a row end the
    /// carry, as a stop and a privacy change do ([`Self::colleague_held`]); a line with no sure sign is held and ends nothing. Such a
    /// line the model leaves unanswered is no talk
    /// (`room_told::RoomTold::carried`).
    pub(crate) fn carrying_the_same_speaker(&mut self) {
        self.carries = true;
    }

    /// Leave alone a reply that answers nothing the lamp asked, said over
    /// its answer ([`crate::others_answer`], `LAMP_OTHERS_ANSWER=on`).
    pub(crate) fn leaving_others_answers(&mut self) {
        self.others_answer = true;
    }

    /// In a busy room, a conversation going on holds only for the one the
    /// lamp answered ([`crate::going_on_same_one`],
    /// `LAMP_GOING_ON_SAME_ONE=on`).
    pub(crate) fn going_on_only_for_the_same_one(&mut self) {
        self.going_on_same_one = true;
    }

    /// `request` ended as a cut-in over the lamp's answer: judged as one.
    pub(crate) fn ended_as_cut_in(&mut self, request: u64) {
        self.cut_in = Some(request);
    }

    /// Why `request` was held as unclear, if it was, and whether the lamp
    /// hums for it: a cut-in, with the switch (as `misheard_held` on Live).
    pub(crate) fn unclear(&self, request: u64) -> Option<(&'static str, bool)> {
        let why = self.attention.unclear(request)?;
        Some((why, self.misheard_hum && self.cut_in == Some(request)))
    }

    /// The model is told beside a line that the lamp's name was heard as it
    /// began, as the Live route tells it (`crate::called`, `LAMP_NAME_NOTE`):
    /// with `needs_face` (`LAMP_NAME_NOTE_BUSY=face`) in a busy room only
    /// with someone turned to the lamp or the name heard surely, and with
    /// `heard_as` saying it may have sounded like another word.
    pub(crate) fn telling_the_name(&mut self, needs_face: bool, heard_as: bool) {
        self.name_note = Some((needs_face, heard_as));
    }

    /// What the lamp did, for the rule, as the Live worker tells its own:
    /// its work, for a question about it later, and its own turn heard, for
    /// a reply to it (#1712).
    pub(crate) fn lamp_did(&mut self, did: crate::second_provider::Did) {
        match did {
            crate::second_provider::Did::Worked(at_us) => self.attention.worked(at_us),
            crate::second_provider::Did::Spoke(at_us) => self.attention.news_told(at_us),
            crate::second_provider::Did::Said(request, until_us) => {
                if self.carries && self.carrying && !self.chain.contains(&request) {
                    self.carry_window.answered(request, until_us);
                }
                self.said_until_us = self.said_until_us.max(Some(until_us));
                if !self.chain.contains(&request) {
                    self.said_until_free_us = self.said_until_free_us.max(Some(until_us));
                }
            }
            // `LAMP_SAME_SPEAKER_CARRIES`: a stop ends the conversation
            // carried.
            crate::second_provider::Did::Stopped => {
                self.carrying = false;
                self.carry_window.clear();
                self.held_colleagues.clear();
            }
        }
    }

    /// The call let out `by` longer ago, as time passes in a test. On a
    /// clock booted under `by` ago the call stays where it is, never a
    /// panic (#1569).
    #[cfg(test)]
    pub(crate) fn call_older(&mut self, by: Duration) {
        self.called_at = self.called_at.map(|at| at.checked_sub(by).unwrap_or(at));
    }

    /// How long ago the lamp's words sent were played to their end
    /// ([`crate::second_provider::Did::Said`]), zero while they still play.
    #[cfg(test)]
    pub(crate) fn said_ended_ago(&self, now_us: u64) -> Option<Duration> {
        ended_ago(self.said_until_us, now_us)
    }

    /// The note `request` earned when it was judged, if any.
    pub(crate) fn name_note_for(&self, request: u64) -> Option<&'static str> {
        self.noted
            .filter(|(judged, _)| *judged == request)
            .and_then(|(_, note)| note)
    }

    /// Whether the name was heard as the line began (or since, until it
    /// ended), as the Live route's `called::Called::due` counts it: a
    /// hearing of [`crate::called::LEAST_SCORE`] or more that began no more
    /// than [`crate::called::BEFORE_US`] before the line. The note, or why
    /// none, said in the journal as the Live route says it.
    fn note_the_name(&mut self, request: u64, (began_us, ended_us): (u64, u64), signs: &Signs) {
        let Some((needs_face, heard_as)) = self.name_note else {
            return;
        };
        let heard = signs.names.iter().any(|(at, score)| {
            *score >= crate::called::LEAST_SCORE
                && at.saturating_add(crate::called::BEFORE_US) >= began_us
                && *at <= ended_us
        });
        let note = heard.then(|| {
            let busy = needs_face && signs.room == crate::room_told::Room::Busy;
            let sure = signs
                .names
                .iter()
                .any(|(_, score)| *score >= crate::attention::SURE_SCORE);
            let turned = if busy && sure { Some(true) } else { signs.turned };
            match crate::called::busy_gate(needs_face, busy, turned) {
                Ok(()) => {
                    eprintln!(
                        "lamp-voice second: request {request}: the model is told the lamp's name was heard"
                    );
                    Some(crate::called::note(heard_as))
                }
                Err(why) => {
                    eprintln!(
                        "lamp-voice second: request {request}: the lamp's name was heard in a busy room and {why}; the model is not told"
                    );
                    None
                }
            }
        });
        self.noted = Some((request, note.flatten()));
    }

    /// As the lamp's only voice (`LAMP_VOICE_ROUTE=second`), whom this
    /// route answers is judged as the Live worker judges it: the room it
    /// keeps itself (nothing else publishes `room/state` then), a
    /// conversation over once people have talked past the lamp, remarks
    /// held, and with `--spoken-to` only talk between people counted, by
    /// day too (#1712: this morning the route missed two calls and answered
    /// a bystander, every line judged in an unknown room).
    pub(crate) fn as_the_only_voice(&mut self, remarks: bool, spoken_to: bool, by_day: bool) {
        self.keeps_room = true;
        self.attention.hold_remarks(remarks);
        self.attention.speak_when_spoken_to(spoken_to);
        self.voice_ends_going_on = spoken_to;
        self.attention.judge_by_day(by_day);
    }

    /// Judge whether each spoken line was said to the lamp from close to it
    /// (`--addressed`, [`crate::addressed`]), as the Live worker does, and
    /// publish it on `attention/addressed` under the bus at `root`. In a
    /// busy room it lets a line out ([`crate::attention::ADDRESSED`]).
    pub(crate) fn judging_addressed(&mut self, root: &Path) {
        let out = crate::addressed::AddressedOut::on(root)
            .inspect_err(|error| {
                eprintln!(
                    "lamp-voice second: whether a request was said to the lamp is not published on the bus: {error}"
                );
            })
            .ok();
        let looks = Looked::default();
        let _ = self.levels_looks.set(Arc::downgrade(&looks));
        self.addressed = Some((looks, out));
        self.hear_levels(root, false);
    }

    /// Sample the room microphone's rise over its quiet from now on, on a
    /// thread of its own: the turn's task cannot, since the line is judged
    /// only after it has been heard.
    fn hear_levels(&mut self, root: &Path, near: bool) {
        self.near_sign |= near;
        if self.levels.is_some() {
            return;
        }
        let Ok(bus) = lamp_bus::Bus::new(root) else {
            return;
        };
        let levels: Levels = Arc::default();
        // Weak: the thread ends once this `Heeding` is gone (Second on #1028).
        let kept = Arc::downgrade(&levels);
        // The eyes' looks, read beside the levels as the Live worker reads
        // them, with `--addressed` only, set up before this or after.
        let looked = self.levels_looks.clone();
        let started = std::thread::Builder::new()
            .name("second-levels".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(LEVEL_EVERY);
                    let now_us = monotonic_us();
                    let rise = bus
                        .latest(lamp_msgs::RoomSound::TOPIC)
                        .ok()
                        .flatten()
                        .filter(|message| {
                            unix_ms(SystemTime::now()).saturating_sub(message.header.at_unix_ms)
                                < 1_000
                        })
                        .and_then(|message| message.parse::<lamp_msgs::RoomSound>().ok())
                        .filter(|level| !level.own_voice)
                        .and_then(|level| Some(level.level_db - level.floor_db?));
                    if let Some(looks) = looked.get().and_then(std::sync::Weak::upgrade)
                        && let Ok(Some(message)) = bus.latest(lamp_msgs::Faces::TOPIC)
                        && let Ok(mut looks) = looks.lock()
                    {
                        looks.read(&message, now_us);
                    }
                    let Some(levels) = kept.upgrade() else { return };
                    let Ok(mut levels) = levels.lock() else {
                        return;
                    };
                    if let Some(rise) = rise {
                        levels.push_back((now_us, rise));
                    }
                    while levels
                        .front()
                        .is_some_and(|(at, _)| now_us.saturating_sub(*at) > LEVELS_KEPT_US)
                    {
                        levels.pop_front();
                    }
                }
            });
        if started.is_ok() {
            self.levels = Some(levels);
        }
    }

    pub(crate) fn heed(
        &mut self,
        request: u64,
        said: (u64, u64),
        heard: &str,
        continues_from: Option<u64>,
    ) -> Option<&'static str> {
        // A line going on from the one judged last began with it, as the
        // rule keeps it (`Attention::continued_at`).
        let onset_us = continues_from
            .filter(|from| self.judged.as_ref().is_some_and(|(last, ..)| last == from))
            .and_then(|_| self.attention.window())
            .map_or(said.0, |(began, _)| began.min(said.0));
        let signs = Signs {
            continues_from,
            ..self.signs_begun(request, said, onset_us)
        };
        self.heed_signs(request, said, heard, signs)
    }

    /// [`Self::heed`] once the signs are read: the line judged, then its
    /// name note, which a name dropped in the judging does not earn
    /// (`LAMP_NAME_SECOND_SIGN=on`, the reviewer's P3 on #2313).
    fn heed_signs(
        &mut self,
        request: u64,
        said: (u64, u64),
        heard: &str,
        mut signs: Signs,
    ) -> Option<&'static str> {
        self.keep_the_name_in_its_line(request, said, &mut signs);
        self.silence_turned(request, said, &mut signs);
        let verdict = self.judge(request, said, heard, signs.clone());
        // `LAMP_FIRST_SECONDS_QUIET`: a line held since the start keeps the
        // first seconds' join off. A wait for the Policy is no hold; a bare
        // name waiting for its request is never judged here.
        if self.first_seconds_quiet
            && verdict.is_some_and(|why| why != crate::second_provider::WAITS_FOR_POLICY)
        {
            self.held_since_start = true;
        }
        if self.said_lately.len() == SAID_KEPT {
            self.said_lately.pop_front();
        }
        self.said_lately
            .push_back((request, said, self.attention.called_in_its_words()));
        if self.noted.is_none_or(|(judged, _)| judged != request) {
            self.note_the_name(request, said, &signs);
        }
        verdict
    }

    /// [`Self::keeping_the_name_in_its_line`]: the hearings that began in the
    /// sound of a line judged lately ([`SAID_KEPT`]), past its start, before
    /// this one began, left out of this one's signs, and said in the
    /// journal. Not only the line just before: a hearing from the line two
    /// back may come only now, after the one between was judged
    /// (reviewer-two's P2 on `2110d04a9`).
    fn keep_the_name_in_its_line(&self, request: u64, said: (u64, u64), signs: &mut Signs) {
        if !self.name_stays {
            return;
        }
        // A line going on from another keeps that one's hearings, its own
        // utterance's, but not an older line's (the reviewer-lead's P3 on
        // `db3bc2d59`).
        let from = signs.continues_from;
        let all = signs.names.len();
        let mut theirs = Vec::new();
        signs.names.retain(|(at, _)| {
            // Past a line's start: one at its start is a call however its
            // words were written ("Lamp." heard as "Ramp."), and goes on
            // (the reviewer-lead's P2 on `da9a597de`).
            let owner = self
                .said_lately
                .iter()
                .rev()
                .find(|(before, (began, ended), call)| {
                    *before != request
                        && Some(*before) != from
                        && !call
                        && *at > began.saturating_add(crate::attention::NAME_AT_THE_START_US)
                        && *at <= *ended
                });
            if let Some((before, ..)) = owner.filter(|_| *at < said.0) {
                theirs.push(*before);
                false
            } else {
                true
            }
        });
        if signs.names.len() < all {
            eprintln!(
                "lamp-voice second: request {request}: {} hearing(s) of its name began in the sound of request(s) {theirs:?}, before this line, and stay with that line (LAMP_NAME_STAYS_IN_ITS_LINE)",
                all - signs.names.len()
            );
        }
    }

    /// `LAMP_TURNED_SPEAKING`: a face turned to the lamp but seen silent
    /// through the line, with no face turned to it speaking, is someone at
    /// the lamp not talking to it. The sign goes before the rule and the
    /// name's note read it (the reviewer-lead's P2 on #2546), and nothing
    /// fresh from the eyes (`None`) stays as it is.
    ///
    /// `LAMP_TURNED_FACE_SPEAKS`: the same when the faces turned to it were
    /// read and none was seen speaking the line, a look unread among them
    /// ([`crate::speaking_faces::SpeakingFaces::turned_not_seen_speaking`]),
    /// unless the line is in the conversation's voice
    /// ([`crate::same_voice::SAME_FROM`] or over). The line, held, is then
    /// talk for the room ([`crate::room_told::RoomTold::face_not_speaking`]).
    /// With no speaking read (`LAMP_EYES_SPEAKING` off, or the turned
    /// mouths never read), the face is a sign as before: fail open, so a
    /// person alone at a lamp whose eyes can't read mouths is still answered.
    fn silence_turned(&mut self, request: u64, said: (u64, u64), signs: &mut Signs) {
        if signs.turned != Some(true) {
            return;
        }
        // With `LAMP_TURNED_FACE_SPEAKS` its policy decides alone: it holds
        // whatever `LAMP_TURNED_SPEAKING` would, and more (the reviewer-lead's
        // P2 on #3305: the old branch first skipped the voice and the room).
        if !self.turned_face_speaks {
            if self.turned_but_silent(said) {
                eprintln!(
                    "lamp-voice second: request {request}: the only faces turned to the lamp were seen silent through the line: nobody is turned to it (LAMP_TURNED_SPEAKING)"
                );
                signs.turned = Some(false);
            }
        } else if turned_only_silent(self.eyes.as_ref(), said.0, said.1, false) {
            let likeness = self
                .voices
                .as_mut()
                .and_then(|voices| voices.likeness_so_far(request));
            if likeness.is_some_and(|likeness| likeness >= crate::same_voice::SAME_FROM) {
                return;
            }
            eprintln!(
                "lamp-voice second: request {request}: the face turned to the lamp was not seen speaking the line, nor is it in the conversation's voice: nobody is turned to it (LAMP_TURNED_FACE_SPEAKS)"
            );
            signs.turned = Some(false);
            self.face_not_speaking = Some(request);
        }
    }

    /// `LAMP_TURNED_SPEAKING`: the only faces turned to the lamp were seen
    /// silent through `began_us..=ended_us` ([`turned_only_silent`]).
    fn turned_but_silent(&self, (began_us, ended_us): (u64, u64)) -> bool {
        self.turned_speaking && turned_only_silent(self.eyes.as_ref(), began_us, ended_us, true)
    }

    /// What the Policy's gate is told of `request`, as the Live worker
    /// tells it of its own turns (#800, #1054): the name heard in it, a
    /// line it follows left unanswered, the lamp's words heard lately; all
    /// as its line was judged. Nothing known of a request not judged last.
    pub(crate) fn policy_turn(&self, request: u64) -> crate::second_provider::policy::Turn {
        self.policed
            .filter(|(judged, _)| *judged == request)
            .map(|(_, turn)| turn)
            .unwrap_or_default()
    }

    /// What the bus and the thread say now of a line said in `began..=ended`.
    // The route reads them with the line's start (`Self::heed`).
    #[cfg(test)]
    fn signs(&mut self, request: u64, said: (u64, u64)) -> Signs {
        self.signs_begun(request, said, said.0)
    }

    /// [`Self::signs`] for a line whose first speech began at `onset_us`:
    /// its own sound's start, or that of the line it goes on from.
    fn signs_begun(&mut self, request: u64, (began, ended): (u64, u64), onset_us: u64) -> Signs {
        let now = Instant::now();
        // `LAMP_GOING_ON_NOT_OWN`: the lamp's answers to lines let out only
        // as the conversation going on are left out, unless this line may
        // answer its question.
        let (lamp_asked, said_ago, person, but) =
            crate::thread::current(self.thread.as_deref(), |thread| {
                let lamp_asked = thread.lamp_asked(request);
                let but: &[u64] = if lamp_asked { &[] } else { &self.chain };
                (
                    lamp_asked,
                    thread.said_last_ago_but(now, but),
                    thread.opened_for().map(str::to_owned),
                    !but.is_empty(),
                )
            })
            .unwrap_or_default();
        let said_going_on = said_ago.is_some_and(|ago| ago <= crate::thread::GOING_ON_FOR);
        let said_until_us = if but {
            eprintln!(
                "lamp-voice second: request {request}: the lamp's answers to {} requests (the newest {:?}), let out only as the conversation going on, keep no conversation going (LAMP_GOING_ON_NOT_OWN)",
                self.chain.len(),
                self.chain.last()
            );
            self.said_until_free_us
        } else {
            self.said_until_us
        };
        // A new person at the lamp begins a new session, as on Live.
        if self.session_for.as_ref() != Some(&person) {
            if self.session_for.is_some() {
                self.session += 1;
            }
            self.session_for = Some(person);
        }
        // Going on from the lamp's own words, or from the person's last call
        // let out (#1712: on lamp-4ace on 19 Oct the owner's next lines came
        // before the lamp's answers were said, and were held as no
        // conversation).
        let called_ago = self.called_at.map(|at| now.saturating_duration_since(at));
        // From the end of what the lamp said, not from when its answer began,
        // which the thread keeps (#1712, the first builder at 19:21: "And what
        // about tomorrow?" 18 s after the weather line, a long answer, was
        // held in a busy room).
        let said_ago = to_the_end(said_ago, ended_ago(said_until_us, monotonic_us()));
        let going_on = going_on_from(said_going_on, said_ago, called_ago);
        // `LAMP_NAME_OR_FOLLOW_UP`: whether the line is a follow-up begun
        // while the lamp's words played or soon after them, or its call,
        // however long it went on; in a busy room such a line, and only
        // such a line, goes on (`ended`, below).
        let began_ago = Duration::from_micros(monotonic_us().saturating_sub(onset_us));
        let followed_up = !self.name_or_follow_up || follows_up(said_ago, called_ago, began_ago);
        // What a busy room goes on by: the thread's window or the call,
        // never the quiet room's 20 s after the words (`went_on_told`).
        let thread_or_call = said_going_on || called_ago.is_some_and(|ago| ago <= CALL_CARRIES_FOR);
        let mut left_alone = None;
        let mut signs = Signs {
            room: crate::room_told::Room::Unknown,
            going_on,
            lamp_asked,
            turned: None,
            names: Vec::new(),
            near: self
                .levels
                .as_ref()
                .filter(|_| self.near_sign)
                .and_then(|levels| levels.lock().ok())
                .is_some_and(|levels| {
                    levels.iter().any(|(at, rise)| {
                        (began..=ended).contains(at) && *rise >= crate::attention::near_rise_db()
                    })
                }),
            continues_from: None,
        };
        // The route's own room as the only voice, as the Live worker reads
        // its own (`attention_now`): with people talking past the lamp since
        // it last spoke, no conversation is going on.
        let mut talked_past = false;
        if self.keeps_room
            && let Ok(mut room) = crate::room_told::room().lock()
        {
            signs.room = room.at(monotonic_us());
            left_alone = Some(room.left_alone_since_spoke());
            talked_past = room.talked_past_the_lamp();
            signs.going_on &= !talked_past;
        }
        let switch = self.name_or_follow_up;
        // `LAMP_SAME_SPEAKER_CARRIES`: in a busy room the one the lamp
        // answered goes on for `SAME_SPEAKER_CARRIES_FOR` after the end of
        // its words, or once from a held colleague line's audio end,
        // capped at forty seconds after the answer; by the line's start,
        // past the follow-up's 8 s and
        // people talking past the lamp, if the verdict then finds their
        // voice or face in it ([`Self::judge`]).
        let goes = followed_up && !talked_past;
        // Forgetting removes the thread's words, including any extension.
        // A delayed output receipt must not revive that forgotten scope.
        if self.carries && said_ago.is_none() {
            // The next legitimately admitted line must begin a new scope,
            // even when a recent call still permits ordinary follow-up.
            self.carrying = false;
            self.carry_window.clear();
            self.held_colleagues.clear();
        }
        let carry = self.carries
            && self.carrying
            && (said_ago
                .is_some_and(|ago| ago.saturating_sub(began_ago) <= SAME_SPEAKER_CARRIES_FOR)
                || self.carry_window.extra_allows(began));
        if carry {
            self.carry_window.remember(request, ended, monotonic_us());
        }
        // Every line begun while it stands is read for its signs, within
        // the 8 s too: an unsure print and a sure sign count the same either
        // side of them, and another face there is held too. Two such
        // lines in a row end the carry (the later #3331 contract).
        self.carried = carry.then_some((request, goes));
        let ended = |mut signs: Signs| {
            // `LAMP_NAME_OR_FOLLOW_UP`, in a busy room: a line is in the
            // conversation going on when it began while the lamp's words
            // played, or within `FOLLOW_UP_FOR` of their end or of its call,
            // however long it then went on, and nobody has talked past the
            // lamp since it spoke. The windows read as it is judged (the
            // thread's, a busy room's 15 s) don't decide: on lamp-52e6 on 22
            // Oct, follow-ups begun about 7 s after their asks, as the
            // answers played, went unanswered.
            if switch && signs.room == crate::room_told::Room::Busy {
                signs.going_on = goes || carry;
                if carry && !goes {
                    // Its start decides, as a follow-up's does: a line begun
                    // in the window may be judged after it.
                    let ms = |ago: Duration| ago.as_millis();
                    eprintln!(
                        "lamp-voice second: request {request}: in a busy room, begun {} ms after the lamp's words ended and judged {} ms after, past a follow-up's window or after talk past the lamp: it goes on only as the same speaker's conversation, within the bounded carry window by its start (ordinary {} s), if its voice or face is the one the lamp answered (LAMP_SAME_SPEAKER_CARRIES)",
                        said_ago.map_or(0, |ago| ms(ago.saturating_sub(began_ago))),
                        said_ago.map_or(0, ms),
                        SAME_SPEAKER_CARRIES_FOR.as_secs()
                    );
                } else if !followed_up {
                    let after = |ago: Option<Duration>| {
                        ago.map_or("never".to_owned(), |ago| {
                            format!("{} s", ago.saturating_sub(began_ago).as_secs())
                        })
                    };
                    eprintln!(
                        "lamp-voice second: request {request}: no conversation going on: in a busy room, the line began {} after the lamp's words ended and {} after its call, past the {} s a follow-up has (LAMP_NAME_OR_FOLLOW_UP)",
                        after(said_ago),
                        after(called_ago),
                        crate::name_or_follow_up::seconds(crate::name_or_follow_up::follow_up_for())
                    );
                } else if talked_past {
                    eprintln!(
                        "lamp-voice second: request {request}: no conversation going on: in a busy room, people have talked past the lamp since it last spoke (LAMP_NAME_OR_FOLLOW_UP)"
                    );
                }
                return signs;
            }
            went_on_told(
                request,
                signs,
                said_ago,
                called_ago,
                left_alone,
                thread_or_call,
            )
        };
        let Some((bus, names)) = &mut self.bus else {
            return ended(signs);
        };
        if !self.keeps_room {
            signs.room = crate::bus_out::room_now(bus);
        }
        signs.turned = crate::bus_out::someone_turned_to_lamp(bus);
        // The faces in the newest look, read once for both switches that
        // count them: the line's room, for a join on no sign of a second
        // person (`LAMP_JOIN_CUT_WORDS`, [`Self::keeping_line_rooms`]); and
        // two in view are two people at the lamp, so the room is busy at
        // once (`LAMP_NAME_OR_FOLLOW_UP`).
        let in_view = (self.name_or_follow_up || self.rooms.is_some())
            .then(|| crate::bus_out::faces_in_view(bus))
            .flatten();
        if self.rooms.is_some() {
            self.in_view = Some((request, in_view));
        }
        // With `LAMP_TWO_FACES_HELD`, only two faces that stay.
        let two_faces = match &self.faces_held {
            Some(held) => held
                .lock()
                .is_ok_and(|mut held| held.two_stayed(monotonic_us())),
            None => in_view.is_some_and(|faces| faces >= 2),
        };
        if self.name_or_follow_up && two_faces {
            if self.keeps_room
                && let Ok(mut room) = crate::room_told::room().lock()
                && room.two_people(monotonic_us())
            {
                eprintln!(
                    "lamp-voice second: request {request}: two faces in view: the room is busy (LAMP_NAME_OR_FOLLOW_UP)"
                );
            }
            signs.room = crate::room_told::Room::Busy;
        }
        let now_us = monotonic_us();
        let now_ms = unix_ms(SystemTime::now());
        for _ in 0..NAMES_READ {
            let Ok(Some(message)) = names.recv(Duration::ZERO) else {
                break;
            };
            if let Ok(name) = message.parse::<lamp_msgs::NameHeard>() {
                // Its publisher's monotonic clock is its own: dated by the
                // wall clock both share.
                let ago_ms =
                    now_ms.saturating_sub(message.header.at_unix_ms) + u64::from(name.ago_ms);
                signs
                    .names
                    .push((now_us.saturating_sub(ago_ms * 1_000), name.score));
            }
        }
        ended(signs)
    }

    /// The rule's verdict on a spoken turn: why it is not answered, or
    /// nothing.
    pub(crate) fn judge(
        &mut self,
        request: u64,
        (began_us, ended_us): (u64, u64),
        heard: &str,
        mut signs: Signs,
    ) -> Option<&'static str> {
        self.said_over(request, (began_us, ended_us));
        let attention = &mut self.attention;
        // What the engine gave: a request going on from one cut short has
        // that one's words in front of its own (#1168).
        let whole = heard;
        // Each turn is its own session here. One going on from the request
        // begun just before it, when the rule judged that one last, keeps
        // what the rule knew of it (its name, the words so far, whether it
        // was close), as on the Live route. Anything else, a request never
        // judged among them, is judged fresh (the reviewer on #1226).
        let (session, heard) = match self
            .judged
            .as_ref()
            .filter(|(last, _, _, _)| signs.continues_from == Some(*last))
        {
            Some((_, session, before, _)) => {
                attention.continued_at(request, *session, began_us);
                // The engine put the words going on from in front of these
                // (#1168); the rule keeps them already, so it is given only
                // this request's own (the reviewer on #1226).
                // Its leading space kept: the rule's text goes on from the
                // first half's words (Fourth on #1262: "HeyLamp, …").
                let own = heard.strip_prefix(before.trim()).unwrap_or(heard);
                // ponytail: a chain whose first link was never judged keeps
                // that link's words in `before`, the strip misses, and the
                // rule reads the joined words as before this (Fourth on #1262).
                (*session, own)
            }
            None => {
                attention.began_at(request, self.session, began_us);
                (self.session, heard)
            }
        };
        self.judged = Some((request, session, heard.to_owned(), whole.to_owned()));
        let named_faced =
            self.named_always && named_to_a_face(&signs.names, began_us, heard, signs.turned);
        self.named_faced = named_faced.then_some(request);
        attention.ended(request, ended_us);
        if self.cut_in == Some(request) {
            attention.ended_as_cut_in(request);
        }
        // Where the newest hearing began against the line's sound, for the
        // journal: what `attention::NAME_AT_THE_START_US` is set from.
        let name_into_ms = signs
            .names
            .iter()
            .map(|(at, _)| (*at as i64 - began_us as i64) / 1_000)
            .max();
        for (at_us, score) in signs.names {
            attention.name_heard_scored(at_us, score);
        }
        attention.heard(heard);
        attention.lamp_asked(signs.lamp_asked);
        let (busy, lamp_asked) = (signs.room == crate::room_told::Room::Busy, signs.lamp_asked);
        attention.set_near(signs.near);
        // `LAMP_BUSY_CALLED_STRICT=on`: in a busy room with nobody turned to
        // the lamp, a stand-in or "Lamp is broken, …" is no call in itself.
        attention.narrow_the_call(self.busy_called_strict && busy && signs.turned != Some(true));
        // Said to the lamp from close to it, over the line's sound, judged
        // once as the Live worker judges it as the sound ends.
        if let Some((looks, out)) = self.addressed.as_mut()
            && let Some(in_hand) = attention.in_hand()
        {
            let now = monotonic_us();
            let none = std::collections::VecDeque::new();
            let rises = self.levels.as_ref().and_then(|levels| levels.lock().ok());
            let looks = looks.lock().ok();
            let said = crate::addressed::judge(
                in_hand,
                rises.as_deref().unwrap_or(&none),
                looks
                    .as_deref()
                    .unwrap_or(&crate::addressed::Looks::default()),
                now,
            );
            attention.set_addressed(said.addressed);
            if said.addressed {
                eprintln!(
                    "lamp-voice second: request {request}: said to the lamp from close to it"
                );
            }
            if let Some(out) = out.as_mut()
                && let Err(error) = out.tell(&said, now)
            {
                eprintln!(
                    "lamp-voice second: request {request}: whether it was said to the lamp was not published: {error}"
                );
            }
        }
        // The night's bars, as the Live worker reads them before each
        // verdict (`provider_worker.rs`): only with the day's own bars, and
        // otherwise every hour was judged as day (Tests and the first
        // builder on #1712, 20:22 and 20:29).
        if attention.judges_by_day() {
            attention.set_night((self.night)());
        }
        // `LAMP_NAME_SECOND_SIGN=on`: the name heard only in the room's
        // sound needs a second sign; the eyes are read for it whatever the
        // rule's form.
        if self.attention.weigh_the_name(signs.turned) {
            eprintln!(
                "lamp-voice second: request {request}: its name, heard only in the room's sound, counts for nothing alone: nobody turned to the lamp, not said close to it, its words not calling it, its hearing not at the line's start (began {} ms into the line); the model is not told it was heard (LAMP_NAME_SECOND_SIGN)",
                name_into_ms.map_or_else(|| "?".to_owned(), |ms| ms.to_string())
            );
            // The name note goes with the name (`heed_signs`).
            self.noted = Some((request, None));
        }
        let turned = signs.turned.filter(|_| self.strict);
        // `LAMP_SAME_SPEAKER_CARRIES`: a print come after its line's verdict
        // is another voice's only when it is sure (`Voices::sure_of_another_only`).
        if self.carries
            && let Some(voices) = self.voices.as_mut()
        {
            voices
                .sure_of_another_only(self.sure_other_samples, crate::same_voice::DIFFERENT_BELOW);
        }
        let going_on_unheard = signs.going_on;
        let extended_before =
            self.carries && self.carrying && self.carry_window.extra_allows(began_us);
        let mut voice = self.voice_going_on_in(request, going_on_unheard, busy);
        // That poll may first identify the earlier colleague. Reconsider
        // this line's start against the newly known end, then read this
        // line's own identity through the same check. Neither is a wait or
        // an admission: the classifier below still requires a sure owner.
        let late_window = self.carries
            && self.carrying
            && !extended_before
            && self.carried.is_none()
            && self.carry_window.extra_allows(began_us);
        if late_window {
            self.carried = Some((request, false));
            // An active thread may already have consumed this line's one
            // likeness before reporting the colleague's late print. Keep
            // that evidence for the owner/nearness check below; polling a
            // second time must not replace it with an empty judgement.
            let likeness = voice.3;
            voice = self.voice_going_on_in(request, true, busy);
            voice.3 = likeness.or(voice.3);
            if self.voice_ends_going_on
                && let Some(likeness) = likeness
            {
                // The first lookup consumed this evidence. Keep the same
                // classification as voice_going_on_in for downstream rules.
                voice.1 = likeness < crate::same_voice::DIFFERENT_BELOW;
                voice.2 = !voice.1;
            }
        }
        let (mut going_on, mut other_voice, same_voice, likeness) = voice;
        let (speakers, turned_speakers) = speakers_between(self.eyes.as_ref(), began_us, ended_us);
        // Whether a carry still stands for the caller after this line.
        let mut carry_stands = false;
        // `LAMP_SAME_SPEAKER_CARRIES`: someone else's line held while the
        // carry stands, its first: it does not make the room busy by voice.
        let mut colleague_held = false;
        let carried_now = self
            .carried
            .take()
            .filter(|(carried, _)| *carried == request);
        // In a room not busy, a line held under the carry is held for its
        // own verdict as in a busy room, the room itself unchanged.
        let mut held_here = false;
        // A print come late at this verdict, of a second line in a row from
        // someone else, ended the carry: this line is judged as with none.
        let carried_now = match carried_now {
            Some((_, follow_up)) if !self.carrying => {
                going_on &= follow_up;
                signs.going_on = going_on;
                None
            }
            carried_now => carried_now,
        };
        if carried_now.is_some() {
            carry_stands = self.carrying;
        }
        // In any room (reviewer-two at 10:35 on #3331): the same speaker's
        // identity, another face, the caller turned away.
        if let Some((_, follow_up)) = carried_now {
            let carried =
                self.carried_by_the_same_speaker(request, likeness, &speakers, &turned_speakers);
            // Held past a still turned face (`LAMP_TURNED_FACE_SPEAKS`), and
            // not carried by a sure sign after all (the reviewer-lead's P3).
            let still_face =
                self.face_not_speaking == Some(request) && carried.goes_on != Some(true);
            if carried.other || still_face {
                colleague_held = self.colleague_held(request);
            }
            // An unsure print is no sign either way: the voice check's own
            // "another voice" from it is undone, so the line is judged as if
            // no print had come, as when it comes late or never (reviewer-two
            // at 04:11 on #3296). In a room not busy such a line is still
            // held, as another voice is there without the carry, but it is
            // counted as no one's and changes nothing in the room (the
            // reviewer-lead's P3 on #3331).
            if other_voice && carried.voice.is_none() {
                other_voice = false;
                going_on = going_on_unheard;
                if !busy && carried.goes_on != Some(true) {
                    going_on = false;
                    held_here = true;
                    if let Ok(mut room) = crate::room_told::room().lock() {
                        room.colleague_held(request);
                    }
                }
            }
            // A newly opened late window is only eligibility, in every
            // room. A generic same-voice continuation must not bypass the
            // owner's stricter nearness check. An unsure held line is no
            // talk for the room, just as in the existing quiet-room path.
            // A voice above SAME_FROM whose held-print comparison was
            // refused is not an owner sign outside the ordinary follow-up,
            // even in an active Unknown/Quiet thread. Generic missing or
            // weak current evidence keeps its existing separate policy.
            let constrained_voice =
                !follow_up && carried.voice == Some(true) && carried.goes_on.is_none();
            if (late_window || constrained_voice) && !busy && carried.goes_on != Some(true) {
                held_here = true;
                if carried.goes_on.is_none()
                    && let Ok(mut room) = crate::room_told::room().lock()
                {
                    room.colleague_held(request);
                }
            }
            if busy || late_window || constrained_voice {
                // No sure sign: past the 8 s held; within them the ordinary
                // follow-up rule, the legacy exception, whenever the print
                // comes.
                going_on = carried.goes_on.unwrap_or(follow_up && going_on);
            } else if carried.goes_on == Some(false) {
                going_on = false;
                held_here = true;
            }
            signs.going_on = going_on;
            if carried.goes_on == Some(true) {
                self.attention.carried_past_a_question();
            }
            carry_stands = self.carrying;
        }
        // `LAMP_NAME_OR_FOLLOW_UP`: a line in another voice than the
        // conversation's is a second person at the lamp, so the room is busy
        // now, for this line's verdict as well as the next ones'.
        let room = if self.name_or_follow_up && other_voice {
            if colleague_held {
                eprintln!(
                    "lamp-voice second: request {request}: another voice than the conversation's, held under the carry: the room is not made busy by it (LAMP_SAME_SPEAKER_CARRIES)"
                );
            } else if self.keeps_room
                && let Ok(mut told) = crate::room_told::room().lock()
                && told.two_people(monotonic_us())
            {
                eprintln!(
                    "lamp-voice second: request {request}: another voice than the conversation's: the room is busy (LAMP_NAME_OR_FOLLOW_UP)"
                );
            }
            crate::room_told::Room::Busy
        } else if held_here {
            eprintln!(
                "lamp-voice second: request {request}: held under the carry, judged as in a busy room, the room unchanged (LAMP_SAME_SPEAKER_CARRIES)"
            );
            crate::room_told::Room::Busy
        } else {
            signs.room
        };
        let busy = room == crate::room_told::Room::Busy;
        self.attention
            .narrow_the_call(self.busy_called_strict && busy && signs.turned != Some(true));
        // `LAMP_JOIN_CUT_WORDS`: who spoke it and how the room was, for a
        // join on no sign of a second person.
        self.keep_spoken(
            request,
            &speakers,
            (room, signs.going_on),
            other_voice,
            likeness,
        );
        let attention = &mut self.attention;
        let seen = Seen {
            room,
            going_on,
            other_voice,
            same_voice,
            turned,
            busy,
            lamp_asked,
            speakers,
            turned_speakers,
            heard: heard.to_owned(),
            named_faced,
            likeness,
        };
        // The conversation's window over (none going on, whoever spoke):
        // the caller's faces go with it, before a call sets them again.
        // A line held with the carry standing (no sure sign, or the caller
        // turned away) keeps the caller's face for their next line.
        if !signs.going_on && !carry_stands {
            self.caller_faces.clear();
        }
        // #1522 on this route (`LAMP_GOING_ON_ASKS`): what the Policy said of
        // the line is given before it is judged, and in a busy room a line
        // going on waits for it, as on the Live route.
        if let Some(policy) = self.policy.as_mut() {
            policy.take();
            if let Some(says) = policy.says(request) {
                attention.policy_said(request, Some(says));
            } else if attention.waits_for_policy(request, room, signs.going_on, turned) {
                eprintln!(
                    "lamp-voice second: request {request}: its answer waits for the Policy's decision: a conversation with the lamp is going on, in a busy room"
                );
                self.waiting = Some(Waiting {
                    request,
                    since: Instant::now(),
                    seen,
                });
                return Some(crate::second_provider::WAITS_FOR_POLICY);
            }
        }
        self.verdict(request, &seen)
    }

    /// The Policy's decision for the line that waits for it
    /// ([`crate::second_provider::WAITS_FOR_POLICY`]): `None` while it may
    /// still come, then the line's verdict, judged with it, or as with no
    /// decision once [`crate::policy_holds::WAIT`] has passed, when the
    /// line is played (a Policy that is down must not make the lamp deaf in
    /// a conversation).
    pub(crate) fn policy_now(&mut self, request: u64) -> Option<Option<&'static str>> {
        let since = self
            .waiting
            .as_ref()
            .filter(|waiting| waiting.request == request)?
            .since;
        let policy = self.policy.as_mut()?;
        policy.take();
        let says = policy.says(request);
        let waited = since.elapsed();
        if says.is_none() && waited < crate::policy_holds::WAIT {
            return None;
        }
        let waiting = self.waiting.take()?;
        if says.is_some() {
            eprintln!(
                "lamp-voice second: request {request}: the Policy's decision came after {} ms",
                waited.as_millis()
            );
        } else {
            eprintln!(
                "lamp-voice second: request {request}: no decision of the Policy's came in {} ms",
                waited.as_millis()
            );
        }
        self.attention.policy_said(request, says);
        Some(self.verdict(request, &waiting.seen))
    }

    /// Ask the Policy of a line it would let out only as a conversation
    /// going on, in a busy room (`LAMP_GOING_ON_ASKS`, #1522): its attention
    /// decisions from the bus at `root`.
    /// With the lamp's actions held for the Policy too
    /// (`LAMP_POLICY_HOLDS_ACTIONS`), the reader they read is this one's:
    /// the route keeps one ([`Self::reading_the_policy`]).
    pub(crate) fn asking_the_policy(&mut self, root: &Path) {
        if self.policy.is_none() {
            match crate::policy_holds::Verdicts::on(root) {
                Ok(verdicts) => self.policy = Some(verdicts),
                Err(error) => {
                    eprintln!(
                        "lamp-voice second: the Policy's decisions can't be read ({error}); a line going on is judged without them"
                    );
                    return;
                }
            }
        }
        eprintln!(
            "lamp-voice second: in a busy room, a line let out only as a conversation going on waits for the Policy's decision (LAMP_GOING_ON_ASKS)"
        );
        self.attention.ask_policy_when_going_on(true);
    }

    /// The route's one reader of the Policy's attention decisions, `verdicts`,
    /// for the lamp's actions ([`Self::policy_verdict`],
    /// `crate::second_provider::policy`) and for a line going on
    /// ([`Self::asking_the_policy`], [`Self::policy_now`]), so both read the
    /// same decisions by request (#1712 item 6, on #1969).
    pub(crate) fn reading_the_policy(&mut self, verdicts: crate::policy_holds::Verdicts) {
        self.policy = Some(verdicts);
    }

    /// What the Policy has decided of `request`, if it has yet, by the kind
    /// of its decision, for the lamp's actions: read now, without waiting.
    /// Nothing without a reader.
    pub(crate) fn policy_verdict(&mut self, request: u64) -> Option<crate::policy_holds::Verdict> {
        let policy = self.policy.as_mut()?;
        policy.take();
        policy.verdict(request)
    }

    /// With `LAMP_MEMORY_FOLLOWS_FACE`, the line `request`'s voice print and
    /// the faces seen speaking it, kept for whether it was the caller's
    /// ([`crate::follows_face`]); and the prints come since for lines kept
    /// before. In memory only.
    fn marking_the_line(&mut self, request: u64, seen: &Seen) {
        if !crate::follows_face::on() {
            return;
        }
        let Ok(mut lines) = crate::follows_face::LINES_HEARD.lock() else {
            return;
        };
        self.filling_the_prints(&mut lines);
        let line = crate::follows_face::Line {
            print: self
                .voices
                .as_mut()
                .and_then(|voices| voices.print_of(request)),
            faces: seen.speakers.clone(),
        };
        lines.mark(request, line);
    }

    /// The prints come since for the lines kept without one.
    fn filling_the_prints(&mut self, lines: &mut crate::follows_face::Lines) {
        let Some(voices) = self.voices.as_mut() else {
            return;
        };
        for waiting in lines.unprinted() {
            if let Some(came) = voices.print_of(waiting) {
                lines.printed(waiting, came);
            }
        }
    }

    /// With `LAMP_MEMORY_FOLLOWS_FACE`, for the Days writer before it
    /// weighs a move (`follows_face::fill_prints`): the prints come since
    /// filled in, and whether the line `request`'s is still on its way.
    fn prints_filled(&mut self, request: u64) -> bool {
        let Ok(mut lines) = crate::follows_face::LINES_HEARD.lock() else {
            return false;
        };
        self.filling_the_prints(&mut lines);
        lines.of(request).is_some_and(|line| line.print.is_none())
            && self
                .voices
                .as_ref()
                .is_some_and(|voices| voices.print_coming(request))
    }

    /// The verdict on `request`, seen as `seen`, said and kept.
    fn verdict(&mut self, request: u64, seen: &Seen) -> Option<&'static str> {
        self.marking_the_line(request, seen);
        let someone_else = self.someone_else(seen);
        let attention = &mut self.attention;
        let (heard, busy, lamp_asked, turned) =
            (seen.heard.as_str(), seen.busy, seen.lamp_asked, seen.turned);
        let mut verdict =
            attention.first_output_busy(request, seen.room, seen.going_on, self.strict, turned);
        let alone = attention.played_on_the_conversation_alone();
        if alone && lone_words(heard) {
            verdict = Some(crate::attention::Verdict::Hold(LONE_WORDS));
        }
        // A reply to nothing the lamp asked, said over its answer, is
        // someone else's (#2136): "Yes, give me five minutes." to B, over
        // the story A asked for. `alone` already means no name, no call, no
        // face, and no question of the lamp's that it answers.
        if self.others_answer
            && alone
            && !lamp_asked
            && self.began_over == Some(request)
            && crate::others_answer::begins_as_an_answer(heard)
        {
            verdict = Some(crate::attention::Verdict::Hold(crate::others_answer::HELD));
        }
        // In a busy room, a conversation going on is the one the lamp
        // answered (#2191): a reply to its question, or, with the same-voice
        // check, the conversation's own voice. Office talk after a named
        // answer is neither. Not `alone`, which needs `LAMP_SPOKEN_TO`.
        if self.going_on_same_one
            && busy
            && attention.played_only_as_going_on()
            && matches!(verdict, Some(crate::attention::Verdict::Play(_)))
            && !lamp_asked
            && !seen.same_voice
        {
            verdict = Some(crate::attention::Verdict::Hold(
                crate::going_on_same_one::HELD,
            ));
        }
        if busy && let Some(crate::attention::Verdict::Play(why)) = verdict {
            verdict = busy_bar(why, heard, lamp_asked)
                .map(crate::attention::Verdict::Hold)
                .or(verdict);
        }
        // #2191: begun with the lamp's name, said to a face turned to the
        // lamp, a line is not held by the strict busy rule or `busy_bar`;
        // any other hold (`LONE_WORDS`, an unclear language) stands.
        if busy
            && seen.named_faced
            && let Some(crate::attention::Verdict::Hold(why)) = verdict
            && [
                crate::attention::BUSY_NAMED_UNTURNED,
                crate::attention::BUSY_UNCALLED,
                crate::attention::BUSY_TURNED_ALONE,
                BUSY_SHORT,
                BUSY_TURNED,
            ]
            .contains(&why)
        {
            eprintln!(
                "lamp-voice second: request {request}: held as \"{why}\" no longer: it began with the lamp's name, said to a face turned to the lamp (LAMP_NAMED_ALWAYS)"
            );
            verdict = Some(crate::attention::Verdict::Play(NAMED_TO_A_FACE));
        }
        // In a quiet room, a conversation going on is the one the lamp
        // answered (`LAMP_QUIET_GOING_ON_SAME_ONE`): a line surely someone
        // else's, just after the caller was answered, is not to the lamp.
        // With no sign of someone else it is the caller's follow-up and goes
        // on (the first builder at 01:21 on Queue #1722: lamp-4ace's day
        // lines, one person's, 6/12 when no sign was read as someone else).
        // Let out only as the conversation going on already means no name,
        // no call, no face turned, not said close to it; not `alone`, which
        // needs `LAMP_SPOKEN_TO` (lamp-52e6 runs without it).
        if self.quiet_same_one
            && !busy
            && attention.played_only_as_going_on()
            && matches!(verdict, Some(crate::attention::Verdict::Play(_)))
            && !lamp_asked
            && someone_else
        {
            verdict = Some(crate::attention::Verdict::Hold(
                crate::second_provider::QUIET_GOING_ON_HELD,
            ));
        }
        // A session's trust is the caller's, earned in their voice: it does
        // not carry to a line whose print says someone else (under
        // `DIFFERENT_BELOW`), let out by trust with no sign of its own but
        // the conversation going on (the first builder at 09:40 on #2428:
        // lamp-52e6's request 11, another voice's 16 words at 0.23, played
        // as "the session has heard enough to be trusted"). A line begun
        // over the lamp's sound has no print, so it is never held so.
        if (self.quiet_same_one || self.not_own)
            && matches!(
                verdict,
                Some(crate::attention::Verdict::Play(crate::attention::TRUSTED))
            )
            && matches!(
                attention.sign(turned),
                None | Some(crate::attention::GOING_ON)
            )
            && !lamp_asked
            && seen
                .likeness
                .is_some_and(|likeness| likeness < crate::same_voice::DIFFERENT_BELOW)
        {
            verdict = Some(crate::attention::Verdict::Hold(
                crate::second_provider::TRUSTED_NOT_CARRIED,
            ));
        }
        let played = matches!(verdict, Some(crate::attention::Verdict::Play(_)));
        // `LAMP_SAME_SPEAKER_CARRIES`: the lamp answers someone, whose
        // conversation may carry on ([`Self::carrying_the_same_speaker`]).
        // A face alone is no call with `LAMP_NAME_OR_FOLLOW_UP`.
        let called = played
            && (attention.called_for_itself()
                || attention.face_shows(seen.room, turned)
                || seen.named_faced);
        if played {
            // Admission alone does not renew the once-per-answer window.
            // A new call changes its owner; only actual words renew it.
            let new_caller = called
                && (attention.called_for_itself()
                    || !spoken_by_the_caller_alone(&seen.speakers, &self.caller_faces));
            if self.carries && (!self.carrying || new_caller) {
                self.carry_window.begin(request);
                self.held_colleagues.begin(request);
            }
            self.carrying = true;
            self.carry_others.clear();
        }
        // Let out as the conversation going on, or for itself: a line that
        // called the lamp begins a conversation of its own (Eighth on #829).
        // With the caller's voice only, the same call as the caller's face
        // takes below (`called`): a face turned to the lamp is a new caller,
        // and their voice is the conversation's (the Reviewer's P2 on #2650).
        // Without it, as before.
        if let (Some(voices), Some(_)) = (self.voices.as_mut(), verdict) {
            // Seen spoken by the caller's face and no other: with
            // `LAMP_SAME_VOICE_CALLER_ONLY`, the voice of a conversation whose
            // call was too short to print.
            if spoken_by_the_caller_alone(&seen.speakers, &self.caller_faces) {
                voices.seen_from_the_caller(request);
            }
            let new_call = if voices.is_callers_only() {
                called
            } else {
                attention.called_for_itself()
            };
            voices.decided(request, played, seen.going_on && !new_call);
        }
        // The verdict for the room this route keeps (`super::room`), and a
        // call let out, which ends the count of talk past the lamp; a line
        // held as another voice is talk past it too (#829).
        // `LAMP_NAME_OR_FOLLOW_UP`: a held line is people talking to each
        // other with someone in view and turned away, never a lone voice's.
        let talking_seen = self.name_or_follow_up
            && self
                .bus
                .as_ref()
                .is_some_and(|(bus, _)| crate::bus_out::someone_in_view_turned_away(bus));
        if let Ok(mut room) = crate::room_told::room().lock() {
            if talking_seen {
                room.seen_talking(request);
            }
            if self.face_not_speaking == Some(request) {
                room.face_not_speaking(request);
            }
            room.judged(request, verdict, seen.other_voice);
            if called {
                room.called(request);
            }
        }
        self.alone = (alone && matches!(verdict, Some(crate::attention::Verdict::Play(_))))
            .then_some(request);
        // `LAMP_GOING_ON_NOT_OWN`: the answer to a line let out only as the
        // conversation going on renews nothing when it is surely someone
        // else's ([`Self::someone_else`]). Under #829's own bar another voice
        // (`LAMP_SAME_VOICE_MODEL`) is held at its verdict, or, known after
        // its answer, ends the conversation at the next line
        // ([`Self::voice_going_on`]).
        // With nothing to tell the speakers apart it is the same person's
        // follow-up, and its answer renews the window as without the switch
        // (the first builder at 01:02 on Queue #1722). An answer to a line
        // said to the lamp ends the chain.
        if self.not_own && self.alone.is_some() {
            if someone_else {
                self.chain.push(request);
            }
        } else if played {
            self.chain.clear();
            self.said_until_free_us = self.said_until_us;
        }
        self.busy = busy.then_some(request);
        // Its words or the name heard called the lamp, not a face alone.
        self.by_name =
            (played && (attention.called_for_itself() || seen.named_faced)).then_some(request);
        // A call let out: the conversation goes on from it, with the face
        // seen speaking it.
        if called {
            self.called_at = Some(Instant::now());
            self.caller_faces = seen.speakers.clone();
        } else if played && seen.going_on && self.caller_faces.is_empty() {
            // After a call no face was seen speaking, the first face seen
            // speaking a line going on turned to the lamp is the caller's
            // (the first builder at 01:51 on #2341).
            self.caller_faces = seen.turned_speakers.clone();
        }
        // Held in a row, as Live counts its unanswered: a new session.
        self.held_run = if played {
            0
        } else {
            self.held_run.saturating_add(1)
        };
        if self.held_run >= RENEW_AFTER_HELD
            && self
                .renewed_at
                .is_none_or(|at| at.elapsed() >= RENEW_AT_MOST_EVERY)
        {
            self.session += 1;
            self.renewed_at = Some(Instant::now());
            self.held_run = 0;
        }
        // What the Policy's gate is told of the turn (#800, #1712): before
        // the turn's own words, so whether the lamp spoke lately is of its
        // words before this line.
        self.policed = Some((
            request,
            crate::second_provider::policy::Turn {
                name_heard: self.attention.in_hand().is_some_and(|turn| {
                    turn.request == request && turn.name_score >= crate::policy_holds::NAME_FROM
                }),
                after_unanswered: self.attention.follows_an_unanswered_line(request),
                lamp_spoke_lately: self.said_until_us.is_some_and(|until| {
                    monotonic_us().saturating_sub(until) <= crate::policy_holds::IN_CONVERSATION_US
                }),
            },
        ));
        let attention = &self.attention;
        match verdict {
            Some(crate::attention::Verdict::Hold(why)) => Some(why),
            // Said, as each hold is by the route (#829, 17:21).
            Some(crate::attention::Verdict::Play(why)) => {
                let alone = if self.began_over == Some(request) && self.alone.is_some() {
                    ", only as the conversation going on, begun over the lamp's answer: the model is not told it may be people's talk"
                } else if self.alone.is_some() {
                    ", only as the conversation going on: the model is told so"
                } else {
                    ""
                };
                let why = shown(why, attention.sign(turned));
                eprintln!(
                    "lamp-voice second: request {request}: its answer is played: {why}{alone}"
                );
                None
            }
            None => None,
        }
    }

    /// Whether a call is held in `request`: one that sets something to come
    /// or takes it away (`policy_holds::SCHEDULE_HELD`) in a request let out
    /// only as the conversation going on, as the Live route's busy-room rule
    /// holds the schedule's (#1522, #1712).
    ///
    /// Not when its words name a timer, a reminder or the lamp, as Live's
    /// busy-room rule lets those set it (#1522, the first builder at 19:04):
    /// on lamp-4ace on 20 Oct "Remind me in two minutes to stretch." was
    /// held on this route, the person told to say it again, and set on Live
    /// (the first builder at 04:54 and their yes at 05:01). A line answering
    /// the lamp's own question is never let out only as the conversation
    /// going on, so it sets what it asks for, as on Live.
    pub(crate) fn holds_call(&self, request: u64, name: &str) -> bool {
        self.alone == Some(request)
            && crate::policy_holds::SCHEDULE_HELD.contains(&name)
            && !self.names_the_schedule(request)
    }

    /// Whether `request`'s words name a timer, a reminder or the lamp
    /// (`going_on_asks::names_the_schedule`): the whole chain's, for a
    /// request going on from one cut short.
    fn names_the_schedule(&self, request: u64) -> bool {
        self.judged.as_ref().is_some_and(|(of, _, _, whole)| {
            *of == request && crate::going_on_asks::names_the_schedule(whole)
        })
    }

    /// Whether `request`'s words ask for the lamp's own work: the
    /// schedule's words, or to remember or forget something. The whole
    /// chain's, as [`Self::names_the_schedule`].
    fn asks_its_work(&self, request: u64) -> bool {
        self.names_the_schedule(request)
            || self.judged.as_ref().is_some_and(|(of, _, _, whole)| {
                *of == request
                    && whole
                        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
                        .any(|word| MEMORY_WORDS.contains(&word.to_lowercase().as_str()))
            })
    }

    /// Whether the lamp's name was heard (`name/heard`, at
    /// `called::LEAST_SCORE` or more) since `from_us`, less
    /// `called::BEFORE_US`, by this process's clock. Read from the bus's last
    /// hearing, so the rule's own queue of hearings keeps it.
    pub(crate) fn name_heard_since(&self, from_us: u64) -> bool {
        let Some((bus, _)) = &self.bus else {
            return false;
        };
        let Ok(Some(message)) = bus.latest(lamp_msgs::NameHeard::TOPIC) else {
            return false;
        };
        let Ok(name) = message.parse::<lamp_msgs::NameHeard>() else {
            return false;
        };
        let now_ms = unix_ms(SystemTime::now());
        let ago_ms = now_ms.saturating_sub(message.header.at_unix_ms) + u64::from(name.ago_ms);
        let at_us = monotonic_us().saturating_sub(ago_ms * 1_000);
        name_counts(name.score, at_us, from_us)
    }

    /// The note for `request` judged: [`BY_NAME_NOTE`] if its words or the
    /// lamp's name called the lamp, else [`DECLINE_NOTE`].
    pub(crate) fn call_note_for(&self, request: u64) -> &'static str {
        call_note(self.by_name == Some(request))
    }

    /// The note for `request` if the rule let it out only as the
    /// conversation going on ([`CONVERSATION_ALONE`]): not for a line whose
    /// words ask for the lamp's own work ([`Self::asks_its_work`]), which is
    /// said to the lamp. On lamp-4ace on 20 Oct, told the note, "Remember
    /// that my favourite colour is green." got `not_for_me` and nothing was
    /// kept (the first builder at 04:54 and their yes at 05:01).
    ///
    /// Nor for a line begun over the lamp's own answer ([`Self::began_over`]):
    /// a cut-in over its voice is said to it, and is told what any line
    /// without its name is ([`DECLINE_NOTE`]). On lamp-52e6 on 24 Oct, told
    /// the note, "Sorry, can you say that last part again?", said over the
    /// lamp's story, got `not_for_me` and silence.
    pub(crate) fn conversation_note_for(&self, request: u64) -> Option<&'static str> {
        (self.alone == Some(request)
            && self.began_over != Some(request)
            && !self.asks_its_work(request))
        .then_some(CONVERSATION_ALONE)
    }

    /// Whether `request` was judged in a busy room: a line that may follow
    /// on from the lamp is not told so there (`LAMP_FOLLOWS_ON`), as on the
    /// Live route.
    pub(crate) fn judged_busy(&self, request: u64) -> bool {
        self.busy == Some(request)
    }
}

fn unix_ms(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis().try_into().unwrap_or(u64::MAX))
}

/// Whose kept things a turn's words are matched against (memory's hits,
/// `crate::remember::hits_scope`): the person's only when the eyes' face
/// and the turn's session agree, the room's when neither names anyone, and
/// otherwise none. The session is opened here for the turn, before its
/// notes are, so a person's first turn is theirs and not the last
/// session's (the reviewer's P2 on #2268).
fn hits_person(
    sessions: &Mutex<Sessions>,
    who: Option<&Path>,
    now: Instant,
) -> Option<Option<String>> {
    let (known, session) = sessions
        .lock()
        .map(|mut sessions| sessions.open_ahead(who, now))
        .unwrap_or_default();
    crate::remember::hits_scope(known.as_deref(), session.as_deref())
}

/// Whose kept things a spoken turn's hits are asked in: `Some(None)` the
/// room's, `Some(Some(person))` that person's, `None` nothing. With a
/// request's lease (`LAMP_SECOND_SCOPED_CONTEXT`), its own admitted scope,
/// narrowed to the room when another face spoke it, and nothing for a revoked
/// one. Without, the sticky session's person, and nothing when the face and
/// the session's person differ.
fn hits_whose(
    request: u64,
    lease: Option<&crate::memory_scope::ScopeLease>,
    sessions: &Mutex<Sessions>,
    who: Option<&Path>,
    now: Instant,
) -> Option<Option<String>> {
    if let Some(lease) = lease {
        return Some(lease.scope()?.person().map(str::to_owned));
    }
    let person = hits_person(sessions, who, now);
    if person.is_none() {
        eprintln!(
            "lamp-voice second: request {request}: memory isn't told: the face and the session's person differ"
        );
    }
    person
}

/// Whether the faces seen speaking a line are the caller's one face and no
/// other: no face, two faces or another face is not, and a call seen spoken
/// by two faces names no caller (the first builder's and the reviewer-lead's
/// P2 on #2663).
fn spoken_by_the_caller_alone(
    speakers: &crate::speaking_faces::Speakers,
    caller_faces: &crate::speaking_faces::Speakers,
) -> bool {
    matches!((speakers.as_slice(), caller_faces.as_slice()), ([face], [caller]) if face == caller)
}

/// The worker's memory-bearing context callback, shared with its integration
/// tests. Selection and composition run inside the same post-wait/post-read
/// scope guard as the production callback; retries keep the parent's policy.
fn memory_context(
    sessions: Arc<Mutex<Sessions>>,
    socket: Option<PathBuf>,
    who: Option<PathBuf>,
    days: bool,
    tell: impl Fn(Instant, Option<String>, Option<Option<String>>, String) -> Told + Send + Sync,
) -> impl Fn(Option<&crate::memory_scope::ScopeLease>, Chosen) -> Told + Send + Sync {
    let compose = move |sessions: &mut Sessions,
                        scope: Option<&crate::memory_scope::ScopeSnapshot>,
                        line: Option<Option<String>>| {
        let now = Instant::now();
        // Counted at every ask, before anything can return early: a turn's
        // second ask (its prepared chat, then its rounds) is never quiet.
        let quiet = sessions.quiet_long_enough(now);
        let (opened, person) = sessions.open(who.as_deref(), now);
        let (changed, person) = match scope {
            None => (opened, person),
            Some(scope) => {
                let person = scope.person().map(str::to_owned);
                (sessions.told_for(&person), person)
            }
        };
        // A line held only by a known face's gap is told no one's memory
        // note: it may not be theirs (the reviewer's P2 on #2449). Its
        // exchanges, the conversation as it stood, are told as before.
        let held = scope.is_some_and(crate::memory_scope::ScopeSnapshot::held);
        let memory = match &socket {
            _ if held => String::new(),
            // Every request here is built afresh from the thread and the
            // session's snapshot: a new session is a fresh context (#2176
            // row 1b).
            // After a stop, the next turn opens one, so journalling can go on
            // (Friday's P1 on #2226). After a quiet, or a person change, the
            // note is asked again (`remember::told_for_fresh_session`). After
            // something kept, the same ask, so it's in the run's next turns
            // (#2540). Never read here, on the worker's loop: asked as the
            // line began, or as the turn is told, on a thread of its own
            // ([`Ahead`]), and only taken here.
            Some(_) if days => {
                let kept = crate::remember::kept_since_note_peek();
                let fresh = crate::remember::wants_fresh_session();
                sessions.taken(changed || quiet || kept, fresh, person.as_deref())
            }
            Some(socket) => crate::remember::told_for_session(socket, 0, person.as_deref()),
            None => crate::remember::told(),
        };
        tell(now, person, line, memory)
    };
    move |lease, chosen| match chosen {
        Chosen::ByLease => told_while_scope_holds(&sessions, lease, |sessions, scope| {
            let line = scope.map(|scope| scope.person().map(str::to_owned));
            compose(sessions, scope, line)
        }),
        // Whose the turn is as before; the line's own person, read once
        // here, only for the typed name.
        Chosen::ByPresence => {
            let line = lease
                .and_then(crate::memory_scope::ScopeLease::scope)
                .map(|scope| scope.person().map(str::to_owned));
            told_while_scope_holds(&sessions, None, |sessions, _| compose(sessions, None, line))
        }
    }
}

/// How a second-route turn's context is chosen: by its line's own lease
/// (`LAMP_SECOND_SCOPED_CONTEXT=on`), or by the person counted at the lamp,
/// the line's lease then read only for the typed name (#2602). Either way,
/// the line's own person alone decides the typed name, and no lease is no
/// name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Chosen {
    ByLease,
    ByPresence,
}

/// Do not hand a context to a prepared or ordinary chat if its authority
/// changed during the note read. A revoked lease is never the room.
fn told_while_scope_holds(
    sessions: &Mutex<Sessions>,
    lease: Option<&crate::memory_scope::ScopeLease>,
    tell: impl FnOnce(&mut Sessions, Option<&crate::memory_scope::ScopeSnapshot>) -> Told,
) -> Told {
    let Ok(mut sessions) = sessions.lock() else {
        return Told::default();
    };
    // A prior context's slow read may have held the lock through this
    // request's cancellation. Check only after that wait, before any I/O.
    let scope = lease
        .and_then(crate::memory_scope::ScopeLease::scope)
        .cloned();
    if lease.is_some() && scope.is_none() {
        return Told::default();
    }
    let told = tell(&mut sessions, scope.as_ref());
    if lease.is_some_and(|lease| lease.scope() != scope.as_ref()) {
        Told::default()
    } else {
        told
    }
}

/// Whose the second route's turns are, and the Days note of that person's
/// run of turns.
#[derive(Default)]
struct Sessions {
    presence: crate::known_faces::Presence,
    /// Runs of turns opened so far: the Days session number.
    opened: u64,
    note: Option<String>,
    /// When an incomplete note may be asked again: on a line begun after
    /// it failed, at once after one failure, and after two or more back to
    /// back no sooner than [`crate::remember::FRESH_RETRY`] from the last
    /// attempt's start ([`Sessions::retry_due`]).
    note_retry_at: Option<Instant>,
    /// Notes failed back to back since the last complete one.
    failures: u32,
    /// Spoken lines begun so far ([`Ahead::begin`]), and how many had begun
    /// when the note last failed: it is asked again only on a later line.
    lines: u64,
    failed_on: u64,
    /// The note's one read, asked ahead as a line began or as a turn is
    /// told; its answer stands for its scope until a turn takes it, or a
    /// turn answered for someone else lets it go.
    asked: Option<NoteAsk>,
    /// What the cached note was read under ([`crate::remember::NoteStamp`]):
    /// once the store may have changed since (a forget), it is never told
    /// again, and the note is asked anew whatever the pacing.
    note_stamp: Option<crate::remember::NoteStamp>,
    /// The scope the last answered turn was told in ([`Ahead::ready_now`]):
    /// a line begun is asked ahead only for it, since a line that is not
    /// answered must open and close no Days session.
    answered: Option<Option<String>>,
    /// The admitted scope the Days note was last asked for.
    scoped: Option<Option<String>>,
    /// A change of person seen by [`Sessions::open_ahead`] that the turn's
    /// notes haven't been opened for yet.
    unsaid: bool,
    /// `LAMP_MEMORY_REOPEN_QUIET`: a run of turns ends after
    /// [`crate::thread::REMEMBERED_FOR`] with none told.
    reopen_quiet: bool,
    /// When a turn was last told, for that quiet.
    last_told: Option<Instant>,
}
impl Sessions {
    fn reopening_after_quiet(reopen_quiet: bool) -> Self {
        Self {
            reopen_quiet,
            ..Self::default()
        }
    }

    /// Whether the conversation was quiet long enough before this ask, `now`,
    /// that it is over and the note is asked again: with the switch, ten
    /// minutes since the last turn was told. Each ask is counted, so a
    /// long conversation's turns, and one turn's several asks, are never
    /// quiet.
    fn quiet_long_enough(&mut self, now: Instant) -> bool {
        let last = self.last_told.replace(now);
        self.reopen_quiet
            && last.is_some_and(|at| {
                now.saturating_duration_since(at) >= crate::thread::REMEMBERED_FOR
            })
    }

    /// Open the thread for whoever is at the lamp now ([`Thread::opening`]):
    /// whether that is someone other than the last turn's notes were for,
    /// and whom.
    ///
    /// [`Thread::opening`]: crate::thread::Thread::opening
    fn open(&mut self, who: Option<&Path>, now: Instant) -> (bool, Option<String>) {
        let (changed, _, person) = self.opening(who, now);
        (changed || std::mem::take(&mut self.unsaid), person)
    }

    /// As [`Sessions::open`], for what is worked out before the turn's
    /// notes (memory's hits): the face known now, and whose the session is.
    /// A change it sees is kept for the notes' own `open`, so their Days
    /// note is still asked anew.
    fn open_ahead(&mut self, who: Option<&Path>, now: Instant) -> (Option<String>, Option<String>) {
        let (changed, known, person) = self.opening(who, now);
        self.unsaid |= changed;
        (known, person)
    }

    /// Whether the session changed person, the face known now, and whose
    /// the session is.
    fn opening(
        &mut self,
        who: Option<&Path>,
        now: Instant,
    ) -> (bool, Option<String>, Option<String>) {
        let known = who
            .and_then(crate::known_faces::known_now)
            .map(|name| crate::known_faces::memory_person(&name));
        let whose = who.and(self.presence.seen(known.clone(), now).map(str::to_owned));
        let Ok(mut thread) = crate::thread::THREAD.lock() else {
            return (false, known, None);
        };
        if who.is_some() {
            thread.at_lamp(whose);
            thread.seen_now(known.clone());
        }
        let changed = thread.opening(now);
        (changed, known, thread.opened_for().map(str::to_owned))
    }

    /// Whether `person`'s admitted scope differs from the last turn's.
    fn told_for(&mut self, person: &Option<String>) -> bool {
        let changed = self.scoped.as_ref() != Some(person);
        self.scoped = Some(person.clone());
        if changed {
            // A held-scope turn can skip the read below. Discard the old
            // person's cache now so the next admitted turn cannot use it.
            self.note = None;
            self.note_stamp = None;
            self.note_retry_at = None;
            self.failures = 0;
        }
        changed
    }

    /// Before any note is chosen: a voided read that has ended is let go,
    /// so it gives back the keep it took (one reader still, while it is
    /// read); and a cached note the store may have changed under since it
    /// was read (a forget) is dropped, so the next ask is due whatever the
    /// retry pacing or a journal resume's backoff say (reviewer-two's P2s on
    /// #3016).
    fn tidy(&mut self) {
        if self
            .asked
            .as_ref()
            .is_some_and(|asked| asked.void && !asked.reading())
        {
            self.asked = None;
        }
        if self
            .note_stamp
            .as_ref()
            .is_some_and(|stamp| !stamp.current())
        {
            eprintln!(
                "lamp-voice second: the memory changed since the note was read: it is asked again"
            );
            self.note = None;
            self.note_stamp = None;
        }
    }

    /// A whole turn with its note asked in place, for the tests of when the
    /// note is asked: its line begun, then the Days note for this run of
    /// turns, asked anew when the person changes, something is kept, or a
    /// failed read may be asked again ([`Self::note_due`]), by the rules the
    /// worker's note asked ahead keeps ([`Ahead`]). Successful empty
    /// snapshots are cached exactly like populated ones.
    #[cfg(test)]
    fn frozen(
        &mut self,
        changed: bool,
        now: Instant,
        ask: impl FnOnce(u64) -> crate::remember::SessionNote,
    ) -> String {
        self.lines += 1;
        if self.note_due(changed, now) {
            self.opened += 1;
            let note = ask(self.opened);
            self.settle(note, (now, self.lines), false);
        }
        self.note.clone().unwrap_or_default()
    }

    /// Whether the run's note is to be asked now: `changed` (the person or
    /// the run changed, it was quiet, or something was kept), there is
    /// none, or a failed one may be asked again.
    fn note_due(&self, changed: bool, now: Instant) -> bool {
        changed || self.note.is_none() || self.retry_due(now)
    }

    /// Whether a failed note may be asked again now: on a line begun after
    /// it failed, and, once two have failed back to back, no sooner than
    /// [`crate::remember::FRESH_RETRY`] after the last attempt began. So a
    /// memory slow once costs one turn its note, and one that is down is
    /// asked twice a minute, not on every turn.
    fn retry_due(&self, now: Instant) -> bool {
        self.lines > self.failed_on && self.note_retry_at.is_some_and(|at| now >= at)
    }

    /// An attempt at the note begun at `at`, on line `line`, failed:
    /// incomplete, or `late` (not in by what was left of its 300 ms as the
    /// line ended), for `cause`, which the journal names by its code only
    /// (#1693).
    fn failed(
        &mut self,
        (at, line): (Instant, u64),
        late: bool,
        cause: crate::remember::NoteCause,
    ) {
        self.failures = self.failures.saturating_add(1);
        self.failed_on = line;
        let what = if late { "late" } else { "incomplete" };
        let cause = cause.code();
        if self.failures == 1 {
            self.note_retry_at = Some(at);
            eprintln!(
                "lamp-voice second: memory note {what}; retry on the next turn; cause={cause}"
            );
        } else {
            self.note_retry_at = Some(at + crate::remember::FRESH_RETRY);
            eprintln!(
                "lamp-voice second: memory note {what} again; retry after {} ms; cause={cause}",
                crate::remember::FRESH_RETRY.as_millis()
            );
        }
    }

    /// What an attempt begun at `at` gave, kept for the run. An incomplete
    /// one replaces the note too, so a failed refresh never brings back a
    /// forgotten fact or another person's note; `counted` when its
    /// lateness was already counted as a failure.
    fn settle(&mut self, note: crate::remember::SessionNote, begun: (Instant, u64), counted: bool) {
        if note.complete {
            if self.failures > 0 {
                eprintln!("lamp-voice second: memory note retry complete");
            }
            self.failures = 0;
            self.note_retry_at = None;
        } else if !counted {
            // A note made incomplete with no cause is one no store gave.
            let cause = note.cause.unwrap_or(crate::remember::NoteCause::Invalid);
            self.failed(begun, false, cause);
        }
        self.note = Some(note.text);
        self.note_stamp = None;
    }

    /// Whose note a turn told in `lease` (its admitted scope; without one,
    /// the session's, as memory's hits find it, [`Self::open_ahead`]) would
    /// be, and whether that is someone other than the last turn's: nothing
    /// for a revoked scope or a line held by a gap, which is told no note.
    fn whose_note(
        &mut self,
        lease: Option<&crate::memory_scope::ScopeLease>,
        who: Option<&Path>,
        now: Instant,
    ) -> Option<(Option<String>, bool)> {
        match lease {
            Some(lease) => {
                let scope = lease.scope().filter(|scope| !scope.held())?;
                let person = scope.person().map(str::to_owned);
                let changed = self.scoped.as_ref() != Some(&person);
                Some((person, changed))
            }
            None => {
                let (_, person) = self.open_ahead(who, now);
                Some((person, self.unsaid))
            }
        }
    }

    /// Ask `person`'s note on a thread of its own when it is due and no
    /// read is going on: at most one at a time, so a slow memory is never
    /// asked twice at once. A complete answer already in for them stands,
    /// however late it came, for [`RETAINED_FOR`], unless something was kept
    /// since or the store may have changed under it (a forget); an
    /// incomplete one counts once as a failed attempt and is asked again
    /// when that may be. `ahead` when a line begins.
    fn ask_if_due(
        &mut self,
        (person, changed): (Option<String>, bool),
        now: Instant,
        ask: &AskNote,
        ahead: bool,
    ) {
        self.tidy();
        if self.asked.as_ref().is_some_and(NoteAsk::reading) {
            return;
        }
        let kept = crate::remember::kept_since_note_peek();
        let theirs = self
            .asked
            .as_ref()
            .filter(|asked| !asked.void && asked.person == person)
            .map(|asked| {
                (
                    asked.standing(now),
                    asked.complete(),
                    asked.current(),
                    asked.late,
                    (asked.at, asked.line),
                )
            });
        match theirs {
            Some((true, _, _, _, _)) if !kept => return,
            // Read under a generation since passed (a forget): never told,
            // and the cached note, read before it too, goes with it.
            Some((_, _, false, _, _)) => {
                self.asked = None;
                self.note = None;
                self.note_stamp = None;
            }
            Some((_, false, _, late, begun)) => {
                let cause = self.asked.take().map(|asked| asked.cause());
                if !late {
                    let cause = cause.unwrap_or(crate::remember::NoteCause::Busy);
                    self.failed(begun, false, cause);
                }
            }
            Some(_) => self.asked = None,
            None => {}
        }
        let quiet = self.reopen_quiet
            && self.last_told.is_some_and(|at| {
                now.saturating_duration_since(at) >= crate::thread::REMEMBERED_FOR
            });
        if !self.note_due(opens_anew(changed) || quiet || kept, now) {
            return;
        }
        // The read takes the kept mark as it begins, and gives it back if
        // it is never told (`remember::NoteRead`).
        self.opened += 1;
        let begun = (now, self.lines);
        self.asked = Some(NoteAsk::start(self.opened, person, begun, ask, ahead));
    }

    /// The Days note a turn is told, taken from what was asked for `person`
    /// ([`Ahead`]), never read here: the worker's loop builds it. Only an
    /// answered line's turn, one [`Ahead::ready`] waited for, takes a read
    /// into the memory's context ([`crate::remember::NoteRead::adopt`]);
    /// one the store may have changed under since it began (a forget) is let
    /// go. `due` is the turn's own reason the run's note may not stand: the
    /// person or the run changed, it was quiet, or something was kept that
    /// no read has covered; `fresh` when journalling waits for a fresh
    /// context.
    fn taken(&mut self, due: bool, fresh: bool, person: Option<&str>) -> String {
        self.tidy();
        let unavailable = || crate::remember::SessionNote::unavailable().text;
        let theirs = self
            .asked
            .as_mut()
            .filter(|asked| !asked.void && asked.person.as_deref() == person);
        if let Some(asked) = theirs {
            let begun = (asked.at, asked.line);
            if asked.waited
                && let Some(read) = asked.take()
            {
                let late = asked.late;
                self.asked = None;
                let stamp = read.stamp();
                let Some(note) = read.adopt() else {
                    // Counts only: what it held is never told.
                    eprintln!(
                        "lamp-voice second: memory note let go: the memory changed while it was read; asked again"
                    );
                    self.note = None;
                    self.note_stamp = None;
                    return unavailable();
                };
                self.settle(note, begun, late);
                self.note_stamp = stamp;
                return self.note.clone().unwrap_or_default();
            }
            if asked.waited {
                // Not in by what was left of its 300 ms: this turn is told
                // the note it may (none, when the run's may not stand). The
                // read goes on: a complete answer is the next turn's, and an
                // incomplete one is asked again on the next line.
                if !std::mem::replace(&mut asked.late, true) {
                    self.failed(begun, true, crate::remember::NoteCause::Timeout);
                }
                if due || self.note.is_none() {
                    self.note = Some(unavailable());
                    self.note_stamp = None;
                }
                return self.note.clone().unwrap_or_default();
            }
            // Not taken before the line's end (a chat begun on its early
            // words, for a line that may not be answered): the turn's own
            // ask takes it later.
            if due {
                self.note = None;
            }
            return self.note.clone().unwrap_or_else(unavailable);
        }
        if due || fresh || self.note.is_none() {
            // Nothing asked for them, or what was may hold what was
            // forgotten: told none, and the next ask is theirs.
            self.note = None;
            return unavailable();
        }
        // The run's note, or a failed one until it may be asked again.
        self.note.clone().unwrap_or_default()
    }
}

/// How long a note asked ahead may take to read ([`Ahead`]): a turn waits
/// for no more than what is left of the note's 300 ms from the ask
/// (`remember::FIRST_READ`), but an answer that comes later, before the line
/// ends, is still the line's. Two seconds, as the memory gives a client.
const NOTE_AHEAD_WITHIN: Duration = Duration::from_secs(2);

/// How long a turn waits for a note asked as it is told, not ahead: a run's
/// first line, or the first after the person changed, has no line's time
/// to read in. On lamp-4ace (48 h to 25 Oct 16:2x) all 56 such notes came
/// late against the note's 300 ms, in a median 443 ms and at most 610 ms,
/// so the first thing asked of a fresh session ("when does the plant need
/// water?") was answered without what the lamp keeps. A note asked ahead
/// still waits its 300 ms from the ask, long past by the line's end.
const NOTE_TOLD_WITHIN: Duration = Duration::from_millis(900);

/// How a Days note is read: its session number, whose (a person's memory
/// id, or none for the room's), within how long, and whether its follow-up
/// is read with it, for a turn already answered
/// ([`crate::remember::DaysClient::read_note_within`]).
type AskNote =
    Arc<dyn Fn(u64, Option<&str>, Duration, bool) -> crate::remember::NoteRead + Send + Sync>;

/// An ask's read, held by the ask and, while it reads, by its thread.
type AskedRead = Arc<Mutex<Option<crate::remember::NoteRead>>>;

/// How long an answered turn waits for its note's follow-up, read only once
/// the turn is answered ([`Ahead::ready`]): the note's own 300 ms.
const FOLLOW_UP_WITHIN: Duration = crate::remember::FIRST_READ;
/// What the wait for it allows beyond that, for its thread to start and
/// hand the read back.
const FOLLOW_UP_SLACK: Duration = Duration::from_millis(100);

/// How long a complete answer stands for its person, though no turn has
/// taken it yet ([`Sessions::ask_if_due`]): the ten quiet minutes after
/// which a run of turns is over (`thread::REMEMBERED_FOR`).
const RETAINED_FOR: Duration = crate::thread::REMEMBERED_FOR;

/// One read of the memory's note, on a thread of its own, never the
/// worker's loop.
struct NoteAsk {
    /// Whose note it is: a person's memory id, or none for the room's.
    person: Option<String>,
    /// When it was asked: a turn waits from here, at most `FIRST_READ`.
    at: Instant,
    /// The line count when it was asked ([`Sessions::lines`]).
    line: u64,
    /// The read, once done, not yet told ([`crate::remember::NoteRead`]).
    /// The thread holds the other reference while it reads.
    answer: AskedRead,
    /// Told when the read is done.
    done: tokio::sync::watch::Receiver<bool>,
    /// A turn waited for it ([`Ahead::ready`]): not in then, it is late.
    waited: bool,
    /// Its lateness was counted as a failed note, once.
    late: bool,
    /// Let go when the camera or the microphone closed or opened: never
    /// told, though while it is still read no other is asked.
    void: bool,
    /// Asked as a line began, not as its turn is told.
    ahead: bool,
}

impl NoteAsk {
    fn start(
        session: u64,
        person: Option<String>,
        (now, line): (Instant, u64),
        ask: &AskNote,
        ahead: bool,
    ) -> Self {
        let answer = Arc::new(Mutex::new(None));
        let (said, done) = tokio::sync::watch::channel(false);
        let read = {
            let (ask, answer, person) = (ask.clone(), answer.clone(), person.clone());
            move || {
                let began = Instant::now();
                let read = ask(session, person.as_deref(), NOTE_AHEAD_WITHIN, !ahead);
                // Counts and times only, never the note.
                eprintln!(
                    "lamp-voice second: memory note asked {}; {} in {} ms",
                    if ahead {
                        "ahead as the line began"
                    } else {
                        "as the turn is told"
                    },
                    if read.complete() {
                        "ready"
                    } else {
                        "incomplete"
                    },
                    began.elapsed().as_millis()
                );
                if let Ok(mut answer) = answer.lock() {
                    *answer = Some(read);
                }
                // Its hold let go before it says it is done: once done, the
                // read is the ask's alone, so one let go then is dropped at
                // once and gives back the keep it took before any note is
                // chosen.
                drop(answer);
                let _ = said.send(true);
            }
        };
        // A thread that cannot be started leaves no reader: the note is
        // then incomplete ([`Self::take`]).
        if let Err(error) = std::thread::Builder::new()
            .name("lamp-voice-note".into())
            .spawn(read)
        {
            eprintln!("lamp-voice second: the memory note could not be asked: {error}");
        }
        Self {
            person,
            at: now,
            line,
            answer,
            done,
            waited: false,
            late: false,
            void: false,
            ahead,
        }
    }

    /// How long from the ask a turn waits for it: `first` (the note's 300
    /// ms on the lamp) for one asked ahead, at least [`NOTE_TOLD_WITHIN`]
    /// for one asked as the turn is told.
    fn within(&self, first: Duration) -> Duration {
        if self.ahead {
            first
        } else {
            first.max(NOTE_TOLD_WITHIN)
        }
    }

    /// Whether its read is still going on.
    fn reading(&self) -> bool {
        Arc::strong_count(&self.answer) > 1 && self.answer.lock().is_ok_and(|note| note.is_none())
    }

    /// Whether its read is done with a complete note; still being read is
    /// not.
    fn complete(&self) -> bool {
        !self.reading()
            && self
                .answer
                .lock()
                .is_ok_and(|read| read.as_ref().is_some_and(|read| read.complete()))
    }

    /// Whether its answer stands for its person at `now`: complete, younger
    /// than [`RETAINED_FOR`], and with nothing the store keeps changed
    /// since it began.
    fn standing(&self, now: Instant) -> bool {
        self.complete()
            && now.saturating_duration_since(self.at) < RETAINED_FOR
            && self
                .answer
                .lock()
                .is_ok_and(|read| read.as_ref().is_some_and(|read| read.current()))
    }

    /// Whether its read, once done, was read under the store's generation
    /// now; still being read counts as current.
    fn current(&self) -> bool {
        self.answer
            .lock()
            .is_ok_and(|read| read.as_ref().is_none_or(|read| read.current()))
    }

    /// Its read, once done: incomplete when the read ended without one.
    fn take(&mut self) -> Option<crate::remember::NoteRead> {
        if self.reading() {
            return None;
        }
        Some(
            self.answer
                .lock()
                .ok()
                .and_then(|mut read| read.take())
                .unwrap_or_else(|| crate::remember::NoteRead::failed(Self::NO_READER)),
        )
    }

    /// Why a read with no answer is incomplete: no reader had it (its
    /// thread could not be started).
    const NO_READER: crate::remember::NoteCause = crate::remember::NoteCause::Busy;

    /// Why its read, once done, is incomplete.
    fn cause(&self) -> crate::remember::NoteCause {
        self.answer
            .lock()
            .ok()
            .and_then(|read| read.as_ref().map(crate::remember::NoteRead::cause))
            .flatten()
            .unwrap_or(Self::NO_READER)
    }
}

/// The second route's memory note asked ahead (lamp-4ace, 22 Oct 17:45: asked
/// as the line ended, the note held the worker's one thread 300 ms, came
/// 60 ms after the voice gave up, and wasn't asked again for 30 s). As a
/// spoken line begins its run's note is asked, when due, on a thread of its
/// own, but only for the scope the last answered turn was told in: a line
/// begins before the lamp knows it is for it, and one that is not answered
/// (someone else's talk in the room) must open and close no Days session.
/// An answered turn asks for its own scope if that differs. The turn waits
/// for it off the loop, no longer than what is left of the note's 300 ms
/// from that ask, and its context only takes it ([`Sessions::taken`]).
#[derive(Clone)]
struct Ahead {
    sessions: Arc<Mutex<Sessions>>,
    who: Option<PathBuf>,
    ask: AskNote,
    /// How long a turn waits for its note from the ask, then for its
    /// follow-up: the note's 300 ms each on the lamp (`remember::FIRST_READ`,
    /// [`FOLLOW_UP_WITHIN`]).
    within: (Duration, Duration),
}

impl Ahead {
    /// Asked with the voice's Days client on `socket`.
    fn reading(sessions: Arc<Mutex<Sessions>>, who: Option<PathBuf>, socket: PathBuf) -> Self {
        Self::asking(
            sessions,
            who,
            Arc::new(move |session, person, within, follow_up| {
                crate::remember::fresh_note_read(&socket, session, person, within, follow_up)
            }),
        )
    }

    /// Asked with `ask`, a turn waiting as the lamp's do.
    fn asking(sessions: Arc<Mutex<Sessions>>, who: Option<PathBuf>, ask: AskNote) -> Self {
        Self {
            sessions,
            who,
            ask,
            within: (crate::remember::FIRST_READ, FOLLOW_UP_WITHIN),
        }
    }

    /// A turn waits up to `within` for its note from the ask, and as long
    /// again for its follow-up: for a test on a loaded machine, whose turns
    /// then wait on the read's own done mark, not on the lamp's 300 ms.
    #[cfg(test)]
    fn waiting_up_to(mut self, within: Duration) -> Self {
        self.within = (within, within);
        self
    }

    /// A spoken line begins, its turn told in `lease`: the note asked now
    /// if it is due and the line is in the scope the last answered turn was
    /// told in (without a lease, taken to be). Nothing is let go here, and
    /// nothing is read of who is at the lamp.
    fn begin(&self, lease: Option<&crate::memory_scope::ScopeLease>, now: Instant) {
        let Ok(mut sessions) = self.sessions.lock() else {
            return;
        };
        sessions.lines += 1;
        sessions.tidy();
        // A read kept from an earlier line waits to be taken by this one's
        // turn, once it is answered ([`Self::ready_now`]).
        if let Some(asked) = sessions.asked.as_mut() {
            asked.waited = false;
        }
        let Some(answered) = sessions.answered.clone() else {
            return;
        };
        if let Some(lease) = lease {
            let Some(scope) = lease.scope().filter(|scope| !scope.held()) else {
                return;
            };
            if scope.person() != answered.as_deref() {
                return;
            }
        }
        sessions.ask_if_due((answered, false), now, &self.ask, true);
    }

    /// Just before a turn told in `lease` is built, which only an answered
    /// line is: its scope is the answered one now, the note asked for it if
    /// it is due and none is (someone else's answer let go), and what to
    /// wait on, until `FIRST_READ` after that ask began (`within`), or
    /// [`NOTE_TOLD_WITHIN`] for one asked now ([`NoteAsk::within`]). Reads
    /// nothing itself.
    fn ready_now(
        &self,
        lease: Option<&crate::memory_scope::ScopeLease>,
        now: Instant,
    ) -> Option<(tokio::sync::watch::Receiver<bool>, Instant, AskedRead)> {
        let mut sessions = self.sessions.lock().ok()?;
        sessions.tidy();
        let whose = sessions.whose_note(lease, self.who.as_deref(), now)?;
        sessions.answered = Some(whose.0.clone());
        if sessions
            .asked
            .as_ref()
            .is_some_and(|asked| asked.person != whose.0 && !asked.reading())
        {
            sessions.asked = None;
        }
        let person = whose.0.clone();
        sessions.ask_if_due(whose, now, &self.ask, false);
        let asked = sessions
            .asked
            .as_mut()
            .filter(|asked| !asked.void && asked.person == person)?;
        asked.waited = true;
        Some((
            asked.done.clone(),
            asked.at + asked.within(self.within.0),
            Arc::clone(&asked.answer),
        ))
    }

    /// [`Self::ready_now`], then the wait, on the runtime's timer. A read
    /// asked as the line began has no follow-up yet: the memory counts an
    /// offer when it gives one, so it is read only now that the turn is
    /// answered, off the loop, within [`FOLLOW_UP_WITHIN`] more (reviewer-
    /// two's P2 on #3016). While it is read the ask counts as being read.
    async fn ready(self, lease: Option<crate::memory_scope::ScopeLease>) {
        let Some((mut done, until, answer)) = self.ready_now(lease.as_ref(), Instant::now()) else {
            return;
        };
        let until = tokio::time::Instant::from_std(until);
        let _ = tokio::time::timeout_at(until, done.wait_for(|done| *done)).await;
        // Still this turn's to take: not voided by a privacy change, nor its
        // scope revoked or held, meanwhile. Only then is an offer spent.
        let scoped = lease
            .as_ref()
            .is_none_or(|lease| lease.scope().is_some_and(|scope| !scope.held()));
        let ours = scoped
            && self.sessions.lock().is_ok_and(|sessions| {
                sessions
                    .asked
                    .as_ref()
                    .is_some_and(|asked| !asked.void && Arc::ptr_eq(&asked.answer, &answer))
            });
        if !ours {
            return;
        }
        let wanted = answer.lock().ok().and_then(|mut read| {
            read.as_ref()
                .is_some_and(crate::remember::NoteRead::wants_follow_up)
                .then(|| read.take())
                .flatten()
        });
        let Some(mut read) = wanted else {
            return;
        };
        let within = self.within.1;
        let reading = tokio::task::spawn_blocking(move || {
            read.follow_up(within);
            if let Ok(mut slot) = answer.lock() {
                *slot = Some(read);
            }
        });
        let _ = tokio::time::timeout(within + FOLLOW_UP_SLACK, reading).await;
    }

    /// The camera or the microphone closed or opened: what was asked
    /// before is never told.
    fn forget(&self) {
        if let Ok(mut sessions) = self.sessions.lock()
            && let Some(asked) = sessions.asked.as_mut()
        {
            if asked.reading() {
                asked.void = true;
            } else {
                sessions.asked = None;
            }
        }
    }

    /// As the engine is given it.
    fn told_as(self) -> crate::second_provider::NoteAhead {
        let (begin, ready, forget) = (self.clone(), self.clone(), self);
        crate::second_provider::NoteAhead {
            begin: Arc::new(move |lease| begin.begin(lease, Instant::now())),
            ready: Arc::new(move |lease| Box::pin(ready.clone().ready(lease))),
            forget: Arc::new(move || forget.forget()),
        }
    }
}

/// The note asked ahead ([`Ahead`]) and the scoped context callback
/// ([`memory_context`]) as [`serve`] wires them with the Days client and
/// `LAMP_SECOND_SCOPED_CONTEXT`, read through `client`, a turn waiting up to
/// `within` for its note and its follow-up: for the engine's tests of the
/// note read ahead with `LAMP_MEMORY_EVENTS` (#3016 with #3059).
#[cfg(test)]
pub(super) fn reading_ahead(
    client: Arc<crate::remember::DaysClient>,
    within: Duration,
) -> (
    crate::second_provider::NoteAhead,
    crate::second_provider::ScopedSystem,
) {
    let sessions = Arc::new(Mutex::new(Sessions::default()));
    let ahead = Ahead::asking(
        sessions.clone(),
        None,
        Arc::new(move |session, person, _, follow_up| {
            client.read_note_within(session, person, false, within, (true, follow_up))
        }),
    )
    .waiting_up_to(within);
    let socket = Some(PathBuf::from("/unused/the note is asked ahead"));
    let told = memory_context(sessions, socket, None, true, |_, person, _, note| {
        system_message(
            "Synthetic character.\n",
            None,
            &note,
            None,
            person.as_deref(),
        )
    });
    (
        ahead.told_as(),
        Arc::new(move |lease: &crate::memory_scope::ScopeLease| told(Some(lease), Chosen::ByLease)),
    )
}

/// Whether this turn's session is opened anew: the person changed, or the
/// memory wants a fresh context after journalling stopped or a write was
/// lost (`LAMP_MEMORY_JOURNAL_RESUMES`, Friday's P1 on #2226).
pub(crate) fn opens_anew(changed: bool) -> bool {
    changed || crate::remember::wants_fresh_session()
}

/// The system message for a turn begun now, as a Live session opened now
/// is told its instruction (`crate::thread::Recalling`): the character,
/// the time and place, the memory note, and the thread in the room the
/// rest leave it, taken first from `file` if the Live route wrote it since.
///
/// With it, the person's last exchanges as messages: up to
/// [`HISTORY_EXCHANGES`] of the last [`HISTORY_WITHIN`]. A Live session keeps
/// its own, and "and how many live there?" needs the one before. Taken
/// from the thread under the same lock as the recall, after the thread's
/// forgetting and reload, so a forgotten exchange is never given back.
/// Only `whose` exchanges are given: a person's memory id, or none for the room.
/// The character and, right after it, the typed name when this turn's
/// line may be told it (#2602, `crate::person::told_from_bus`): judged on
/// the line's own person (`line`, from its lease), and nothing when the
/// line has no lease.
fn with_typed_name(
    character: String,
    name: Option<&str>,
    bus: Option<&lamp_bus::Bus>,
    line: Option<Option<&str>>,
) -> String {
    match line {
        Some(person) => character + &crate::person::told_from_bus(name, bus, person),
        None => character,
    }
}

pub(crate) fn system_message(
    character: &str,
    time: Option<&str>,
    memory: &str,
    file: Option<&Path>,
    whose: Option<&str>,
) -> Told {
    let told = format!("{character}{}{memory}", time.unwrap_or_default());
    let (thread, history) = thread_and_history(told.len(), file, whose);
    Told {
        text: told + &thread,
        after: None,
        history,
    }
}

/// [`system_message`] with what stays told first, as
/// `LAMP_SECOND_STEADY_FIRST=on` asks (#1712, the first builder at 18:56):
/// `steady`, the character and the skills, as the system message, and
/// `changing`, the memory note, the time and the room's facts, with the
/// thread, after the exchanges ([`Told::after`]). A service's prompt cache
/// then holds the character, the tools and the exchanges from one request
/// to the next; the time alone changes every minute.
pub(crate) fn steady_first_message(
    steady: String,
    changing: &str,
    file: Option<&Path>,
    whose: Option<&str>,
) -> Told {
    let (thread, history) = thread_and_history(steady.len() + changing.len(), file, whose);
    let after = format!("{changing}{thread}").trim_start().to_owned();
    Told {
        text: steady,
        after: (!after.is_empty()).then_some(after),
        history,
    }
}

/// The thread in the room `used` bytes of the system message leave it,
/// and the exchanges as messages, both `whose` alone (a person's memory id,
/// or none for the room), under the thread's one lock.
fn thread_and_history(
    used: usize,
    file: Option<&Path>,
    whose: Option<&str>,
) -> (String, Vec<Message>) {
    let room = lamp_gemini::MAX_INSTRUCTION_BYTES.saturating_sub(used);
    let now = Instant::now();
    let (thread, recent) = crate::thread::current(file, |thread| {
        (
            thread.recall_for(whose, now, room),
            thread.recent_for(whose, now, HISTORY_WITHIN, HISTORY_EXCHANGES),
        )
    })
    .unwrap_or_default();
    (
        thread.unwrap_or_default(),
        recent.into_iter().flat_map(messages).collect(),
    )
}

/// What a text's answer (#1673) is told, and how its functions run. It is
/// told the character, the time and place, the room's memory and the
/// skills, never the room's spoken conversation or the kept things of the
/// person in view: the person texting may be elsewhere, and need not be
/// them (the reviewer on #1713). Its functions run as nobody's at the lamp
/// (`crate::tools::answer_typed`).
pub(crate) fn texts_ends(
    character: String,
    tools: crate::tools::Settings,
) -> (crate::second_provider::System, ToolRunner) {
    let told = tools.clone();
    let system: crate::second_provider::System = Arc::new(move || {
        let skills = if told.use_skill {
            crate::use_skill::section(&crate::tools::skill_tools(&told))
        } else {
            String::new()
        };
        texts_system_message(
            &character,
            crate::agent_tasks::noted(
                crate::local_time::told_asked_here(told.clock, told.places),
                told.tasks.as_ref(),
            )
            .as_deref(),
            &(crate::remember::told_for_texts() + &skills),
        )
    });
    let runner: ToolRunner = Arc::new(move |request, _heard, name, args| {
        let settings = tools.clone();
        Box::pin(async move {
            let call = lamp_gemini::ToolCall {
                id: String::new(),
                name,
                args,
            };
            tokio::task::spawn_blocking(move || {
                crate::tools::answer_typed(&settings, &call, request)
            })
            .await
            .unwrap_or_else(|_| crate::tools::unavailable())
        })
    });
    (system, runner)
}

/// The worker's texts (#1673) as [`serve`] runs them: `messages::serve` with
/// a text's own system message and functions ([`texts_ends`]). Built here
/// from the character and the settings alone, so the spoken turn's `system`
/// can never be handed to it (the reviewer on #1726). With the tasks part,
/// news not told aloud is given with the answer to the next text
/// (`task_news::ForTexts`).
pub(crate) fn texts_task(
    model: impl crate::messages::Model + Send + Sync + 'static,
    character: String,
    tools: crate::tools::Settings,
    bus_root: PathBuf,
) -> impl std::future::Future<Output = ()> {
    let news: Box<dyn crate::messages::Untold> = match tools
        .tasks
        .as_ref()
        .and_then(crate::task_news::ForTexts::for_tasks)
    {
        Some(news) => Box::new(news),
        None => Box::new(crate::messages::NoNews),
    };
    let (system, runner) = texts_ends(character, tools);
    crate::messages::serve(model, system, runner, news, bus_root)
}

/// The system message for a text's answer (#1673): the character, the time
/// and place, and `memory`, the room's note. No thread and no history: a
/// text is not the room's spoken conversation (the reviewer on #1713).
pub(crate) fn texts_system_message(character: &str, time: Option<&str>, memory: &str) -> Told {
    Told {
        text: format!("{character}{}{memory}", time.unwrap_or_default()),
        after: None,
        history: Vec::new(),
    }
}

/// With `LAMP_SAID_AS_HEARD=on`, a line that cut the lamp's answer, or the
/// first after a Stop that cut one, says how much of it was played: only
/// those words of it are kept as the lamp's ([`crate::said_as_heard`],
/// [`crate::thread::Thread::heard_only`]). The journal says so, in counts.
pub(super) fn keep_heard(
    thread: &mut crate::thread::Thread,
    engine: &Engine,
    command: &ProviderInput,
) {
    let ProviderInput::Start {
        request,
        heard_of_cut: Some(cut),
        ..
    } = command
    else {
        return;
    };
    let Some((said, sent)) = engine.said_as_heard(*cut) else {
        return;
    };
    // The engine's own played words never go round `leaving`: a sign-in's
    // are its one line, and any other's are scrubbed (Jarvis's P2 2 on
    // #2137).
    let said = if crate::sign_in::marked(cut.request) {
        crate::sign_in::SAID_LINE.to_owned()
    } else {
        crate::sign_in::scrub_piece(&said)
    };
    thread.heard_only(cut.request, &said);
    eprintln!(
        "lamp-voice second: request {}: {} of its answer's {sent} words were played, {} ms of it, before it was cut; request {request} is asked with only those as the lamp's",
        cut.request,
        said.split_whitespace().count(),
        cut.samples * 1_000 / u64::from(super::net::TTS_SAMPLE_RATE),
    );
}

/// One exchange as chat messages. A request the lamp was given in writing
/// goes as a system note, never as words a person said; nothing is sent
/// for a person who said nothing.
fn messages(exchange: crate::thread::Recent) -> Vec<Message> {
    let mut messages = Vec::with_capacity(3);
    if let Some(written) = exchange.written {
        messages.push(Message::system(format!(
            "(A note you were given in writing, not words anyone said: {written})"
        )));
    }
    if !exchange.person.is_empty() {
        messages.push(Message::user(exchange.person));
    }
    messages.push(Message::assistant(exchange.lamp));
    messages
}

/// Exchanges given to the chat as messages, and for how long since each.
pub const HISTORY_EXCHANGES: usize = 6;
pub const HISTORY_WITHIN: Duration = Duration::from_secs(300);

/// Keep what a turn's output says in the thread, as the Live route keeps
/// its transcripts: the person's words, the lamp's (not the announcement),
/// and at the turn's end the exchange to the memory's journal and the
/// thread to its file.
#[cfg(test)]
pub(crate) fn keep(event: &ProviderOutput, file: Option<&Path>) {
    keep_scoped(event, file, None, None)
}

/// [`keep`] for a request admitted with `lease`, as the worker keeps it
/// with `LAMP_SECOND_SCOPED_CONTEXT=on`: for other parts' tests.
#[cfg(test)]
pub(crate) fn keep_admitted(event: &ProviderOutput, lease: &crate::memory_scope::ScopeLease) {
    keep_scoped(event, None, Some(lease), None)
}

/// [`keep`] for a request's admitted scope (`lease`). With `held`
/// (`LAMP_SAID_AS_HEARD`), an answer's hand-on to memory at its turn's end is
/// held until it is known how much of it was heard.
fn keep_scoped(
    event: &ProviderOutput,
    file: Option<&Path>,
    lease: Option<&crate::memory_scope::ScopeLease>,
    held: Option<&mut crate::said_as_heard::HeldHandOn>,
) {
    if lease.is_some_and(|lease| !lease.is_current()) {
        return;
    }
    // A line held only by a known face's gap (`LAMP_FACE_GAP_HOLD`) may not
    // be theirs: it is never journalled, as theirs or anyone's, so it is kept
    // out before its words reach the thread or its end hands anything on
    // (the reviewer's P2 on #2616). Its exchange stays in the spoken thread,
    // so the conversation as it stood carries on when they look back.
    if let Some(lease) = lease
        && lease
            .scope()
            .is_some_and(crate::memory_scope::ScopeSnapshot::held)
    {
        crate::remember::held_by_gap(lease.key().request);
    }
    let now = Instant::now();
    match event {
        ProviderOutput::Transcript {
            request: None,
            text,
            finished,
        } => {
            if let Ok(mut thread) = crate::thread::THREAD.lock() {
                thread.person(text, *finished, now);
            }
        }
        ProviderOutput::Transcript {
            request: Some(request),
            text,
            ..
        } => {
            // Sentences come trimmed, those after the first with a space
            // before; the thread joins its pieces as given. The empty last
            // piece says the words are over and adds none.
            if !text.is_empty()
                && let Ok(mut thread) = crate::thread::THREAD.lock()
            {
                let text = format!("{} ", text.trim_start());
                match lease {
                    Some(lease) => {
                        if lease.key().request != *request {
                            return;
                        }
                        let Some(scope) = lease.scope() else {
                            return;
                        };
                        thread.lamp_for(*request, &text, now, scope.person());
                    }
                    None => thread.lamp(*request, &text, now),
                }
            }
        }
        ProviderOutput::TurnComplete { request, .. } => {
            match held {
                Some(held) => {
                    // What the turn's end knows goes with it (Twelve's
                    // conditions on #1722, 10:40).
                    crate::remember::hold(*request);
                    if let Some(older) = held.hold(*request, now) {
                        crate::remember::finished(older);
                    }
                }
                None => crate::remember::finished(*request),
            }
            save(file);
        }
        ProviderOutput::TurnFailed { .. } => save(file),
        _ => {}
    }
}

/// The thread to its file if it changed; one not written loses the thread
/// at the next restart and nothing else.
fn save(file: Option<&Path>) {
    let Some(path) = file else { return };
    let text = crate::thread::THREAD
        .lock()
        .ok()
        .and_then(|mut thread| thread.save(Instant::now(), SystemTime::now()));
    if let Some(text) = text
        && crate::thread::write_file(path, &text).is_ok()
        && let Ok(mut thread) = crate::thread::THREAD.lock()
    {
        thread.wrote(path);
    }
}

/// The hosts a down link checks (#1844): the chat's, the speech's, speech
/// to text's and Gemini's, any answering enough.
fn probe_targets(config: &crate::second_config::SecondConfig) -> Vec<String> {
    let stt = match &config.stt {
        crate::second_config::Stt::Deepgram { .. } => "wss://api.deepgram.com/v1/listen",
        crate::second_config::Stt::Autonomous { base_url, .. } => base_url.as_str(),
    };
    let mut targets: Vec<String> = [
        config.llm_base_url.as_str(),
        config.tts_base_url.as_str(),
        stt,
        config.gemini.base_url.as_str(),
    ]
    .into_iter()
    .filter_map(super::link::target)
    .collect();
    targets.sort();
    targets.dedup();
    targets
}

/// A written request kept for the thread as the note it is, with the
/// answer to it (`Thread::written`). An add-on's quoted line is not: neither
/// it nor what the lamp says of it is kept, so a person's later turn, with
/// its functions, never carries the partner's words (the reviewer-lead on
/// #2675).
fn keep_written(thread: &mut crate::thread::Thread, command: &ProviderInput, scopes_context: bool) {
    match command {
        ProviderInput::Written {
            request,
            quoted: true,
            ..
        } => thread.quoted(*request),
        // A written request is admitted as the room's.
        ProviderInput::Written { request, text, .. } if scopes_context => {
            thread.written_for(*request, text, None);
        }
        ProviderInput::Written { request, text, .. } => thread.written(*request, text),
        _ => {}
    }
}

/// A sign-in's news marks its request, before anything of it is kept
/// (#1676, `crate::sign_in::written`).
fn sign_in_written(command: &ProviderInput) {
    if let ProviderInput::Written { request, text, .. } = command {
        crate::sign_in::written(*request, text);
    }
}

/// The engine's next output, with a sign-in's code and account taken out
/// of its words (#1676, `crate::sign_in::leaving`): before the thread keeps
/// them, and before they are sent.
fn next_output(engine: &mut Engine) -> Option<ProviderOutput> {
    let mut event = engine.try_output();
    if let Some(event) = event.as_mut() {
        crate::sign_in::leaving(event);
    }
    event
}

/// Run the engine as a provider worker until Stop. Readiness is the
/// fixture's: `Ready`, then one `ProviderLink` Ready on the control plane.
/// The services are not probed first: a service that cannot be reached
/// fails that turn. With `probe_targets` (the lamp's only voice, #1844) a
/// turn failed on the network checks whether any of them answers, and none
/// answering is the link Unavailable until a check every 30 s finds one
/// (`link.rs`); without, the link stays Ready, as a standby's always has.
pub async fn run(
    mut channels: WorkerChannels,
    boot: BootId,
    mut engine: Engine,
    thread: Option<PathBuf>,
    probe_targets: Option<Vec<String>>,
) -> io::Result<()> {
    let started_us = monotonic_us();
    channels.control.send(WorkerEvent::Ready)?;
    // The wait sounds and down lines rendered at the start, not at the
    // first line: the first turn after a restart had none (#3755).
    engine.render_lines();
    let (mut link, ready) = super::link::Watch::new(probe_targets, started_us, monotonic_us());
    ready.validate(monotonic_us())?;
    channels
        .control
        .send(WorkerEvent::ProviderLink { link: ready })?;
    let mut gate = Gate::new(boot, monotonic_us());
    let mut ticker = tokio::time::interval(Duration::from_millis(2));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // At most one input read ahead of its authority, and one output waiting
    // for credit or a full socket.
    let mut retained: Option<ProviderInput> = None;
    let mut pending: Option<ProviderOutput> = None;
    // The answer's sound as it goes out, for when the lamp has finished
    // saying it (`AnswerSound::ends`).
    let mut answer_sound = AnswerSound::default();
    // An answer's hand-on to memory, held until it is known how much of it
    // was heard (`LAMP_SAID_AS_HEARD`).
    let mut held = crate::said_as_heard::HeldHandOn::default();
    let holding = engine.holds_hand_on();
    // What is held goes as the worker stops.
    let stopped = |held: &mut crate::said_as_heard::HeldHandOn| {
        if let Some(request) = held.release() {
            crate::remember::finished(request);
        }
    };
    'tick: loop {
        // The turn tasks run while this waits.
        ticker.tick().await;
        engine.tick_room(monotonic_us());
        if let Some(request) = held.due(Instant::now()) {
            crate::remember::finished(request);
        }
        match gate.drain(&mut channels.control, &mut engine)? {
            Drain::Stopped => {
                stopped(&mut held);
                return Ok(());
            }
            Drain::Deferred => continue,
            Drain::Empty => {}
        }
        gate.fresh(monotonic_us())?;
        // As many inputs a tick as the Live worker takes: a line replayed
        // to this route arrives faster than it was spoken, and a packet
        // left waiting past its age bound is refused.
        for _ in 0..INPUT_SLICE {
            if retained.is_none() {
                retained = channels.data.receive()?;
            }
            // The parent sends authority before its dependent input, possibly
            // after the first empty read: drain again before judging the input.
            match gate.drain(&mut channels.control, &mut engine)? {
                Drain::Stopped => {
                    stopped(&mut held);
                    return Ok(());
                }
                Drain::Deferred => continue 'tick,
                Drain::Empty => {}
            }
            let Some(command) = retained.take() else {
                break;
            };
            if gate.admits(monotonic_us(), &command)? {
                // A written request is kept for the thread as the note it
                // is, with the answer to it (`Thread::written`).
                sign_in_written(&command);
                if let Ok(mut thread) = crate::thread::THREAD.lock() {
                    keep_written(&mut thread, &command, engine.scopes_context());
                }
                // What was played of an answer this line, or the Stop
                // before it, cut is the lamp's words of it, before the
                // line is asked (`LAMP_SAID_AS_HEARD`).
                if let ProviderInput::Start {
                    heard_of_cut: Some(_),
                    ..
                } = &command
                    && let Ok(mut thread) = crate::thread::THREAD.lock()
                {
                    keep_heard(&mut thread, &engine, &command);
                }
                // A new line or a written request begins: the answer before
                // it is kept as heard, so its hand-on to memory goes now.
                if matches!(
                    command,
                    ProviderInput::Start { .. } | ProviderInput::Written { .. }
                ) && let Some(request) = held.release()
                {
                    crate::remember::finished(request);
                }
                engine.submit(command);
            }
        }
        // Output kept waiting for a request since retired is dropped.
        if pending
            .as_ref()
            .and_then(output_request)
            .is_some_and(|request| request <= gate.retired_through)
        {
            pending = None;
        }
        if pending.is_none() {
            pending = next_output(&mut engine);
            if let Some(event) = &pending {
                let lease = engine
                    .scopes_context()
                    .then(|| engine.memory_lease())
                    .flatten();
                keep_scoped(
                    event,
                    thread.as_deref(),
                    lease.as_ref(),
                    holding.then_some(&mut held),
                );
                // Whom the lamp answered, for a known face's held gap: as
                // its answer ends playing, not as its turn completes, which
                // is about when its first sound goes out (lamp-c0a4 on
                // #2449: a 3.5 s answer took that much off the window).
                answer_sound.heard(event, Instant::now());
                if let ProviderOutput::TurnComplete { request, .. } = event {
                    let now = Instant::now();
                    engine.memory_answered(*request, answer_sound.ends(*request).max(now));
                }
            }
            if let Some(event) = &pending {
                link.output(monotonic_us(), event);
            }
        }
        // The lamp's only voice tells the coordinator its link (#1844).
        if let Some((changed, was_down)) = link.tick(monotonic_us()).await {
            changed.validate(monotonic_us())?;
            if was_down {
                eprintln!("lamp-voice second: a service host answers again: the link is back");
                // The relay's routes asked again now, not at their time's
                // end (#2882).
                engine.shared.services.link_back();
            } else {
                eprintln!(
                    "lamp-voice second: no service host answers after a turn failed on the network: the link is down, checked every {} s",
                    super::link::PROBE_EVERY_US / 1_000_000
                );
            }
            channels
                .control
                .send(WorkerEvent::ProviderLink { link: changed })?;
        }
        if let Some(event) = pending.take()
            && !gate.send_or_discard(&engine, monotonic_us(), &event, |event| {
                channels.data.send(event)
            })?
        {
            pending = Some(event);
        }
    }
}

#[cfg(test)]
mod route_checks;

#[cfg(test)]
mod tests {
    /// These synchronous fixtures backdate events by up to 400 seconds.
    /// Heeding and its inputs share this epoch, independent of host uptime.
    fn clock_with_history() -> crate::scenario_clock::Scope {
        crate::scenario_clock::Scope::enter_at(Duration::from_secs(10), 600_000_000).unwrap()
    }

    /// The reviewer's P2 on #3050: with scoped context off, the owner's
    /// turn, then the known file gone and one unknown face in view. The
    /// session is still counted for the owner (their sticky presence, 30 s),
    /// but the typed name follows the visitor's own lease, the room's, so
    /// the visitor's request carries none; with scoped context on, the
    /// same. The owner's own lease is told it either way, and a turn with
    /// no lease is told none.
    #[test]
    fn reviewer_second_owner_name_does_not_follow_sticky_person_to_unknown_face() {
        use crate::memory_scope::{RequestKey, RequestScope, ScopeSnapshot};
        use std::sync::{Arc, Mutex};
        let _one = crate::provider_worker::tests::one_thread();
        let root = std::path::PathBuf::from(format!("/tmp/lpk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut eyes = bus
            .publisher_declared(
                lamp_msgs::Faces::TOPIC,
                &lamp_msgs::Faces::declaration(),
                "eyes",
            )
            .unwrap();
        let mut one_face = || {
            eyes.publish_json(
                crate::scenario_clock::monotonic_us(),
                &lamp_msgs::Faces {
                    faces: vec![lamp_msgs::Face {
                        id: "f".into(),
                        xmin: 0.4,
                        ymin: 0.0,
                        xmax: 0.7,
                        ymax: 0.3,
                        confidence: 0.9,
                        yaw: None,
                        facing: None,
                        speaking: None,
                    }],
                },
            )
            .unwrap();
        };
        let name = "Example Name";
        let who = root.join("who.sock");
        let known = crate::known_faces::known_file_of(&who);
        let told_for = super::memory_context(
            Arc::new(Mutex::new(super::Sessions::default())),
            None,
            Some(who),
            false,
            move |_, person, line, memory| {
                super::system_message(
                    &super::with_typed_name(
                        "You are a lamp.".into(),
                        Some(name),
                        Some(&bus),
                        line.as_ref().map(Option::as_deref),
                    ),
                    None,
                    &memory,
                    None,
                    person.as_deref(),
                )
            },
        );
        let scope = |request: u64, snapshot: ScopeSnapshot| {
            RequestScope::new(
                RequestKey {
                    worker: lamp_turn_owner::BootId::new([7; 16]).unwrap(),
                    session: 1,
                    request,
                },
                snapshot,
            )
        };
        let modes = [super::Chosen::ByPresence, super::Chosen::ByLease];
        // The owner's turn: their known face, alone in view.
        std::fs::write(&known, name).unwrap();
        one_face();
        let owner = scope(1, ScopeSnapshot::of(name));
        for mode in modes {
            let told = told_for(Some(&owner.lease()), mode).text;
            assert!(told.contains(name), "the owner, {mode:?}: {told}");
        }
        // They go; a face the lamp doesn't know is in view, within 30 s.
        std::fs::remove_file(&known).unwrap();
        one_face();
        let visitor = scope(2, ScopeSnapshot::room());
        for mode in modes {
            let told = told_for(Some(&visitor.lease()), mode).text;
            assert!(!told.contains(name), "the visitor, {mode:?}: {told}");
        }
        // No lease at all: no name.
        let told = told_for(None, super::Chosen::ByPresence).text;
        assert!(!told.contains(name), "no lease: {told}");
        std::fs::remove_dir_all(root).unwrap();
    }

    /// El's P2 on #3050, the second route's counterpart of the Live one:
    /// an empty quiet room's turn, then an unknown face's on the same room
    /// thread. Neither request carries the typed name, so no reply in the
    /// room's thread can carry it either; the owner's own face, alone in
    /// view, is told it (the control).
    #[test]
    fn an_empty_rooms_turn_then_an_unknown_faces_carry_no_typed_name() {
        let _one = crate::provider_worker::tests::one_thread();
        let root = std::path::PathBuf::from(format!("/tmp/lps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut eyes = bus
            .publisher_declared(
                lamp_msgs::Faces::TOPIC,
                &lamp_msgs::Faces::declaration(),
                "eyes",
            )
            .unwrap();
        let mut room = crate::bus_out::RoomOut::on(&root).unwrap();
        room.tell(crate::room_told::Room::Quiet, 1_000_000);
        let mut look = |faces: usize| {
            eyes.publish_json(
                crate::scenario_clock::monotonic_us(),
                &lamp_msgs::Faces {
                    faces: (0..faces)
                        .map(|_| lamp_msgs::Face {
                            id: "f".into(),
                            xmin: 0.4,
                            ymin: 0.0,
                            xmax: 0.7,
                            ymax: 0.3,
                            confidence: 0.9,
                            yaw: None,
                            facing: None,
                            speaking: None,
                        })
                        .collect(),
                },
            )
            .unwrap();
        };
        let name = "Example Name";
        let owner = crate::known_faces::memory_person(name);
        let turn = |person: Option<&str>| {
            let told = super::system_message(
                &super::with_typed_name(
                    "You are a lamp.".into(),
                    Some(name),
                    Some(&bus),
                    Some(person),
                ),
                None,
                "",
                None,
                person,
            );
            let steady = super::steady_first_message(
                super::with_typed_name(
                    "You are a lamp.".into(),
                    Some(name),
                    Some(&bus),
                    Some(person),
                ),
                "",
                None,
                person,
            );
            format!(
                "{}{:?}{}{:?}",
                told.text, told.history, steady.text, steady.after
            )
        };
        // Nobody in view, the room quiet: the room's turn.
        look(0);
        let empty = turn(None);
        assert!(!empty.contains(name), "{empty}");
        // Then a face the lamp doesn't know, still the room's.
        look(1);
        let visitor = turn(None);
        assert!(!visitor.contains(name), "{visitor}");
        // The owner's own face: told.
        let owners = turn(Some(&owner));
        assert!(owners.contains(name), "{owners}");
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The reviewer on #2218 (P3 2): a skill's look is taken only with the
    /// switch exactly `on` and a full look; nothing else turns it on.
    #[test]
    fn a_skills_look_needs_the_switch_on_and_a_full_look() {
        assert!(super::takes_skill_looks(Some("on"), true));
        for switch in [None, Some(""), Some("On"), Some("1"), Some("off")] {
            assert!(!super::takes_skill_looks(switch, true), "{switch:?}");
        }
        assert!(!super::takes_skill_looks(Some("on"), false));
    }

    /// #1712's map, item 6: the air's note is due after ten minutes of a
    /// stuffy room, never in a busy one, spent only once its request's
    /// answer is heard (a line cut short before any word keeps it; the
    /// reviewer's P3 on #1960), and not again within three hours.
    #[test]
    fn the_airs_note_is_given_once_when_due() {
        // An hour ahead of the monotonic clock, so eleven minutes before now
        // is a time on it even on a runner up for less: from the clock itself,
        // the note was never due there (probe run 39538962883, #1569 row 64).
        fn clock() -> u64 {
            monotonic_us() + 3_600_000_000
        }
        let air = Arc::new(Mutex::new(crate::air_told::AirTold::default()));
        let busy = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let room_busy = busy.clone();
        let (note, heard) = super::air_noting(
            air.clone(),
            move |_| room_busy.load(std::sync::atomic::Ordering::Relaxed),
            clock,
        );
        let now = clock();
        air.lock()
            .unwrap()
            .seen(true, Some(1180), now.saturating_sub(5 * 60_000_000));
        assert_eq!(note(1), None, "stuffy five minutes: not yet");
        air.lock().unwrap().seen(false, None, now);
        air.lock()
            .unwrap()
            .seen(true, Some(1180), now.saturating_sub(11 * 60_000_000));
        // Due, but the room is busy: not said, and not counted as told
        // (Fourth's case, from #1978).
        busy.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(note(2), None, "never in a busy room");
        busy.store(false, std::sync::atomic::Ordering::Relaxed);
        let said = note(3).expect("due once the room is not busy");
        assert!(said.contains("1200"), "{said}");
        // Request 3 cut short before any word: its note isn't spent.
        heard(99);
        assert!(note(4).is_some(), "not heard: still due for the next");
        heard(4);
        assert_eq!(note(5), None, "heard once: told");
        heard(4);
        assert_eq!(note(6), None, "and not again");
        // Two given it before the first's answer plays (the reviewer's P3 on
        // #2083): the earlier one's answer heard still spends it.
        let air = Arc::new(Mutex::new(crate::air_told::AirTold::default()));
        let (note, heard) = super::air_noting(air.clone(), |_| false, clock);
        air.lock()
            .unwrap()
            .seen(true, Some(1180), now.saturating_sub(11 * 60_000_000));
        assert!(note(7).is_some() && note(8).is_some(), "due for both");
        heard(7);
        assert_eq!(note(9), None, "the earlier one's answer spent it");
    }

    /// #1712, the first builder at 16:43: the route's lines are judged as
    /// one session's, as a Live session's, so in a busy room one past the
    /// session's first two is trusted when the rule is not strict. Before,
    /// each line was a session's first. Eight held in a row begin a new one.
    /// The first builder's 21:51 on #1722, the hums on the second route: with
    /// `--ask-unclear` the route's rule holds a line of a conversation heard
    /// in another script, or a cut-in with no English word, as the Live rule
    /// does, and says why; the lamp hums only for the cut-in, and only with
    /// `LAMP_MISHEARD_HUM`. Without the switch neither is held as unclear.
    #[test]
    fn a_line_heard_as_nothing_the_conversation_means_is_held_as_unclear() {
        use crate::not_speech::{CUT_IN_FRAGMENT, OTHER_SCRIPT};
        use crate::room_told::Room;
        let going_on = || Signs {
            room: Room::Quiet,
            going_on: true,
            lamp_asked: false,
            turned: None,
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let said = (10_000_000, 11_000_000);
        let hindi = "नमस्ते, आप कैसे हैं?";
        let spanish = "Venga, nos vemos luego.";
        for hum in [false, true] {
            let mut heeding = Heeding::new(false, true, None, None);
            heeding.asking_again(hum);
            assert_eq!(
                heeding.judge(1, said, hindi, going_on()),
                Some(OTHER_SCRIPT)
            );
            assert_eq!(heeding.unclear(1), Some((OTHER_SCRIPT, false)));
            heeding.ended_as_cut_in(2);
            assert_eq!(
                heeding.judge(2, said, spanish, going_on()),
                Some(CUT_IN_FRAGMENT)
            );
            assert_eq!(heeding.unclear(2), Some((CUT_IN_FRAGMENT, hum)));
            // Not a cut-in: some Spanish is no fault of its own.
            assert_ne!(
                heeding.judge(3, said, spanish, going_on()),
                Some(CUT_IN_FRAGMENT)
            );
            assert_eq!(heeding.unclear(3), None);
        }
        let mut off = Heeding::new(false, true, None, None);
        off.ended_as_cut_in(2);
        assert_ne!(off.judge(1, said, hindi, going_on()), Some(OTHER_SCRIPT));
        assert_ne!(
            off.judge(2, said, spanish, going_on()),
            Some(CUT_IN_FRAGMENT)
        );
        assert_eq!((off.unclear(1), off.unclear(2)), (None, None));
    }

    #[test]
    fn the_routes_lines_are_one_sessions_and_eight_held_renew_it() {
        use crate::room_told::Room;
        let busy = || Signs {
            room: Room::Busy,
            going_on: false,
            lamp_asked: false,
            turned: Some(false),
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let said = (10_000_000, 11_000_000);
        let mut heeding = Heeding::new(false, true, None, None);
        for request in [300, 301] {
            assert!(heeding.judge(request, said, "And then?", busy()).is_some());
        }
        assert_eq!(heeding.judge(302, said, "And then?", busy()), None);
        assert_eq!(heeding.session, 1);
        let mut strict = Heeding::new(true, true, None, None);
        for request in 400..400 + u64::from(RENEW_AFTER_HELD) {
            assert!(strict.judge(request, said, "And then?", busy()).is_some());
        }
        assert_eq!(strict.session, 2);
        // At most every quarter of an hour.
        for request in 500..500 + u64::from(RENEW_AFTER_HELD) {
            assert!(strict.judge(request, said, "And then?", busy()).is_some());
        }
        assert_eq!(strict.session, 2);
    }

    /// #1712, the first builder at 18:57: in a busy room, a line let out
    /// only as the conversation going on is a question, three words or more,
    /// or an answer to the lamp's own question; one let out only by a face
    /// turned to the lamp asks something. Other signs let a line out as
    /// before.
    #[test]
    fn a_busy_room_asks_more_of_a_line_going_on_or_a_turned_face() {
        use crate::attention::{GOING_ON, TURNED};
        assert_eq!(super::busy_bar(GOING_ON, "Yes.", false), Some(BUSY_SHORT));
        assert_eq!(
            super::busy_bar(GOING_ON, "Yes.", true),
            None,
            "the hand-over's yes"
        );
        assert_eq!(super::busy_bar(GOING_ON, "Okay?", false), None);
        assert_eq!(super::busy_bar(GOING_ON, "Book the car.", false), None);
        assert_eq!(
            super::busy_bar(GOING_ON, "Okay sure.", false),
            Some(BUSY_SHORT)
        );
        assert_eq!(
            super::busy_bar(TURNED, "Okay sure, fine.", false),
            Some(BUSY_TURNED)
        );
        assert_eq!(super::busy_bar(TURNED, "Is it raining?", false), None);
        let named = "its name was heard plainly in the room's sound";
        assert_eq!(super::busy_bar(named, "Okay.", false), None);
    }

    /// lamp-4ace on 24 Oct, the owner alone: "Lamp." said, nothing after it,
    /// answered as a call, then held in a busy room as a short line of the
    /// conversation going on. A literal name call is never held for being
    /// short; recognizer substitutes and a mere mention keep the old bar.
    #[test]
    fn a_busy_room_never_holds_a_line_that_calls_the_lamp() {
        use crate::attention::{GOING_ON, TURNED};
        for called in [
            "Lamp.",
            "Lamp",
            "Hey Lamp.",
            "Lamp tell me a joke.",
            "Lamb, please.",
            "Thanks, Lamp.",
        ] {
            assert_eq!(super::busy_bar(GOING_ON, called, false), None, "{called}");
            assert_eq!(super::busy_bar(TURNED, called, false), None, "{called}");
        }
        for alias in ["Plan.", "Man.", "Lynn."] {
            assert_eq!(super::busy_bar(GOING_ON, alias, false), Some(BUSY_SHORT));
            assert_eq!(super::busy_bar(TURNED, alias, false), Some(BUSY_TURNED));
        }
        assert_eq!(
            super::busy_bar(TURNED, "Man, that's crazy.", false),
            Some(BUSY_TURNED)
        );
        // Three words meet the existing GOING_ON threshold.
        assert_eq!(super::busy_bar(GOING_ON, "Man, that's crazy.", false), None);
        assert_eq!(
            super::busy_bar(GOING_ON, "The lamp.", false),
            Some(BUSY_SHORT),
            "the lamp mentioned, not called"
        );
        assert_eq!(super::busy_bar(GOING_ON, "Yes.", false), Some(BUSY_SHORT));
    }

    /// #1712, the first builder at 19:21: "And what about tomorrow?" came
    /// 18 s after the weather line, after a long answer, and was held in a
    /// busy room. The 15 s count from the end of the lamp's words.
    #[test]
    fn a_busy_rooms_fifteen_seconds_count_from_the_end_of_the_lamps_words() {
        let s = Duration::from_secs;
        let mut heeding = super::Heeding::new(false, false, None, None);
        assert_eq!(heeding.said_ended_ago(100_000_000), None);
        heeding.lamp_did(crate::second_provider::Did::Said(1, 105_000_000));
        // Still playing, then over.
        assert_eq!(heeding.said_ended_ago(100_000_000), Some(Duration::ZERO));
        assert_eq!(heeding.said_ended_ago(114_000_000), Some(s(9)));
        // An earlier end told late moves nothing back.
        heeding.lamp_did(crate::second_provider::Did::Said(1, 101_000_000));
        assert_eq!(heeding.said_ended_ago(114_000_000), Some(s(9)));
        // The answer began 18 s ago and ended 9 s ago: it goes on.
        let said_ago = to_the_end(Some(s(18)), heeding.said_ended_ago(114_000_000));
        assert_eq!(said_ago, Some(s(9)));
        let busy = || Signs {
            room: crate::room_told::Room::Busy,
            going_on: true,
            lamp_asked: false,
            turned: None,
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        assert!(went_on_told(5, busy(), said_ago, None, None, true).going_on);
        assert!(!went_on_told(5, busy(), Some(s(18)), None, None, true).going_on);
        // No words in the thread: none, whatever was played.
        assert_eq!(to_the_end(None, Some(s(1))), None);
        assert_eq!(to_the_end(Some(s(30)), None), Some(s(30)));
    }

    /// #1712 (the first builder at 01:50 and 02:04): a line said as a long
    /// story ends is in a conversation going on, though the thread's 45 s
    /// from the story's start have passed; 20 s after the lamp's words it
    /// isn't; a call carries as before.
    #[test]
    fn a_line_just_after_a_long_answer_is_in_a_conversation_going_on() {
        let s = Duration::from_secs;
        // The thread's window passed, the lamp's words ended just now.
        assert!(super::going_on_from(false, Some(s(0)), None));
        assert!(super::going_on_from(false, Some(s(20)), None));
        assert!(!super::going_on_from(false, Some(s(21)), None));
        // No words in the thread, no call: none.
        assert!(!super::going_on_from(false, None, None));
        assert!(super::going_on_from(false, None, Some(s(10))));
        assert!(super::going_on_from(true, Some(s(44)), None));
    }

    /// #1712, the first builder at 18:57: in a busy room a conversation goes
    /// on 15 s from the lamp's last words; in a quiet one, Live's 45 s.
    #[test]
    fn a_busy_rooms_conversation_goes_on_fifteen_seconds() {
        use crate::room_told::Room;
        let signs = |room| Signs {
            room,
            going_on: true,
            lamp_asked: false,
            turned: None,
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let s = Duration::from_secs;
        let went = |room, said: u64, called: Option<u64>| {
            super::went_on_told(1, signs(room), Some(s(said)), called.map(s), None, true).going_on
        };
        assert!(went(Room::Busy, 10, None));
        assert!(!went(Room::Busy, 20, None));
        assert!(!went(Room::Busy, 44, None));
        assert!(
            went(Room::Busy, 30, Some(10)),
            "the person's call, 10 s ago"
        );
        assert!(went(Room::Quiet, 30, None));
        assert!(went(Room::Unknown, 44, None));
        // The first builder's agent on #2032 (03:09): after a 40 s answer
        // the thread's window has passed, and 10 s after the words the
        // quiet room's 20 s count; a busy room's don't.
        let after_words =
            |room| super::went_on_told(1, signs(room), Some(s(10)), None, None, false).going_on;
        assert!(!after_words(Room::Busy));
        assert!(after_words(Room::Quiet));
        assert!(after_words(Room::Unknown));
    }

    /// #1712, the first builder at 17:49: the journal names the sign that
    /// let a line out, never the session's trust or the room alone.
    #[test]
    fn the_journal_names_the_sign_that_let_a_line_out() {
        use crate::attention::{GOING_ON, ROOM_NOT_BUSY, TRUSTED, TURNED};
        assert_eq!(super::shown(TRUSTED, Some(TURNED)), TURNED);
        assert_eq!(super::shown(ROOM_NOT_BUSY, Some(GOING_ON)), GOING_ON);
        // A strong reason stays, and a weak one with no other sign (the
        // rule without `--spoken-to`) is the reason.
        let named = "its name was heard plainly in the room's sound";
        assert_eq!(super::shown(named, Some(TURNED)), named);
        assert_eq!(super::shown(TRUSTED, None), TRUSTED);
        // From the rule: someone turned to the lamp, in a trusted session.
        let busy = || Signs {
            room: crate::room_told::Room::Unknown,
            going_on: false,
            lamp_asked: false,
            turned: Some(true),
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let said = (10_000_000, 11_000_000);
        let mut heeding = Heeding::new(true, true, None, None);
        heeding.as_the_only_voice(false, true, false);
        for request in [1, 2, 3] {
            assert_eq!(
                heeding.judge(request, said, "Okay yeah sure.", busy()),
                None
            );
        }
        assert_eq!(heeding.attention.sign(Some(true)), Some(TURNED));
        assert_eq!(heeding.attention.sign(Some(false)), None);
    }

    /// `LAMP_NAME_OR_FOLLOW_UP` on this route (the owner's desk on lamp-4ace,
    /// 22 October): in a busy room a face turned to the lamp lets no line
    /// out alone, and a line it was in, let out as the conversation going
    /// on, is no call that carries the next lines; with the name heard it is
    /// played. In a room not known to be busy the face lets a line out, in a
    /// session trusted or not, and is a call, as without the switch (the
    /// owner at 18:0x: one person at the desk does not say the lamp's name).
    #[test]
    fn with_name_or_follow_up_a_face_alone_lets_no_line_out_in_a_busy_room_on_this_route() {
        use crate::room_told::Room;
        let signs = |room, going_on, names: Vec<(u64, f32)>| Signs {
            room,
            going_on,
            lamp_asked: false,
            turned: Some(true),
            names,
            near: false,
            continues_from: None,
        };
        let said = (10_000_000, 11_000_000);
        let desk = |on: bool| {
            let mut heeding = Heeding::new(true, true, None, None);
            heeding.as_the_only_voice(false, true, false);
            if on {
                heeding.needing_name_or_follow_up();
            }
            heeding
        };
        let mut heeding = desk(true);
        for request in [1, 2, 3] {
            assert_eq!(
                heeding.judge(
                    request,
                    said,
                    "Okay yeah sure.",
                    signs(Room::Unknown, false, Vec::new())
                ),
                None,
                "{request}"
            );
            assert_eq!(
                heeding.attention.sign(Some(true)),
                Some(crate::attention::TURNED)
            );
        }
        assert_eq!(
            desk(true).judge(
                1,
                said,
                "Okay yeah sure.",
                signs(Room::Busy, false, Vec::new())
            ),
            Some(crate::attention::BUSY_TURNED_ALONE)
        );
        assert_eq!(
            desk(false).judge(
                1,
                said,
                "Okay yeah sure?",
                signs(Room::Busy, false, Vec::new())
            ),
            None
        );
        // With the name heard in the line's sound, the face confirms it.
        let named = vec![(said.0 + 100_000, 0.95)];
        assert_eq!(
            desk(true).judge(1, said, "What time is it?", signs(Room::Busy, false, named)),
            None
        );
        // Let out as the conversation going on: in a busy room a call only
        // without the switch; in a quiet one a call with it too.
        for (on, room, call) in [
            (true, Room::Busy, false),
            (false, Room::Busy, true),
            (true, Room::Quiet, true),
        ] {
            let mut heeding = desk(on);
            assert_eq!(
                heeding.judge(
                    1,
                    said,
                    "And what about tomorrow?",
                    signs(room, true, Vec::new())
                ),
                None
            );
            assert_eq!(heeding.called_at.is_some(), call, "{on} {room:?}");
        }
    }

    /// `LAMP_NAME_OR_FOLLOW_UP` on this route: two people's lines the lamp
    /// holds in a quiet room, said to nobody with someone in view and turned
    /// away, are talk, and the second makes the room busy, where the next
    /// line is held as a busy room's. A lone voice
    /// with nobody in view leaves the room quiet however many of its lines
    /// are held (lamp-4ace, 22 Oct: an unseen person's call, misheard, and the
    /// lines after it turned the room busy, and the scene was lost). Without
    /// the switch they are held lines, no talk with `LAMP_SPOKEN_TO`, and the
    /// room stays quiet.
    #[test]
    fn with_name_or_follow_up_two_peoples_held_lines_make_a_quiet_room_busy() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        let _clock = clock_with_history();
        use crate::room_told::Room;
        // A short root: a Unix socket's address holds 103 bytes.
        let root = std::path::PathBuf::from(format!("/tmp/lnh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut eyes = bus
            .publisher_declared(
                lamp_msgs::Faces::TOPIC,
                &lamp_msgs::Faces::declaration(),
                "eyes",
            )
            .unwrap();
        let mut after = |on: bool, in_view: bool| {
            *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
            let faces = lamp_msgs::Faces {
                faces: if in_view {
                    vec![lamp_msgs::Face {
                        id: "at-the-desk".into(),
                        xmin: 0.4,
                        ymin: 0.0,
                        xmax: 0.7,
                        ymax: 0.3,
                        confidence: 0.9,
                        yaw: None,
                        facing: Some(false),
                        speaking: None,
                    }]
                } else {
                    Vec::new()
                },
            };
            eyes.publish_json(monotonic_us(), &faces).unwrap();
            let now_us = monotonic_us();
            {
                let mut room = crate::room_told::room().lock().unwrap();
                *room = crate::room_told::RoomTold::new();
                room.count_only_talk(true);
                room.count_held_talk(on);
                // Quiet: five minutes with no talk.
                room.at(now_us - 400_000_000);
                assert_eq!(room.at(now_us - 90_000_000), Room::Quiet);
            }
            let mut heeding = Heeding::new(true, true, Some(&root), None);
            heeding.as_the_only_voice(false, true, false);
            if on {
                heeding.needing_name_or_follow_up();
            }
            let lines = [
                "Did you get my message about the budget?",
                "Yes, I'll read it after lunch.",
                "Okay, sounds good.",
            ];
            let mut rooms = Vec::new();
            for (request, line) in (1..).zip(lines) {
                // Heard and closed by the engine, as it tells the room: a held
                // line is left unanswered.
                let said = (
                    now_us + request * 3_000_000,
                    now_us + request * 3_000_000 + 1,
                );
                crate::room_told::room()
                    .lock()
                    .unwrap()
                    .heard(request, said.0);
                let held = heeding.heed(request, said, line, None);
                let mut room = crate::room_told::room().lock().unwrap();
                room.ended(request, said.1);
                room.unanswered(request, said.1);
                rooms.push((held, room.at(said.1 + 1)));
            }
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
            rooms
        };
        use crate::attention::{BUSY_UNCALLED, NOT_SPOKEN_TO};
        assert_eq!(
            after(true, true),
            [
                (Some(NOT_SPOKEN_TO), Room::Quiet),
                (Some(NOT_SPOKEN_TO), Room::Busy),
                (Some(BUSY_UNCALLED), Room::Busy),
            ]
        );
        assert_eq!(after(true, false), [(Some(NOT_SPOKEN_TO), Room::Quiet); 3]);
        assert_eq!(after(false, true), [(Some(NOT_SPOKEN_TO), Room::Quiet); 3]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `LAMP_NAME_OR_FOLLOW_UP` on this route, in a busy room: a follow-up
    /// begun while the lamp's answer is still playing is in the conversation
    /// going on, however long it then goes on, whether the thread has that
    /// answer's words yet or only an earlier one's (a run on lamp-52e6 on
    /// 22 Oct: "Another one?" begun about 7 s after its ask, while the
    /// answer played, went unanswered). One begun 12 s after the words
    /// ended, without the name, is held.
    #[test]
    fn with_name_or_follow_up_a_follow_up_begun_while_the_answer_plays_goes_on() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        let _clock = clock_with_history();
        use crate::room_told::Room;
        let s = Duration::from_secs;
        // The ask's answer began `began_s` s ago and its words play until
        // `until_s` s from now (negative: ended that long ago); the thread
        // has its words, or only an answer of a minute before. The line began
        // `long` s ago and is judged now.
        let judged = |in_thread: bool, began_s: u64, until_s: i64, long: u64| {
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
            {
                let mut room = crate::room_told::room().lock().unwrap();
                for other in 901..=903 {
                    room.unanswered(other, monotonic_us());
                }
                assert_eq!(room.at(monotonic_us()), Room::Busy);
            }
            let mut thread = crate::thread::Thread::new();
            let earlier = Instant::now() - s(60);
            thread.person("Lamp, what's the weather today?", true, earlier);
            thread.lamp(1, "Sunny and warm today.", earlier);
            if in_thread {
                let began = Instant::now() - s(began_s);
                thread.person("Lamp, tell me a joke.", true, began);
                thread.lamp(2, "Why did the lamp go to school?", began);
            }
            *crate::thread::THREAD.lock().unwrap() = thread;
            let mut heeding = Heeding::new(false, false, None, None);
            heeding.as_the_only_voice(false, true, false);
            heeding.needing_name_or_follow_up();
            let now_us = monotonic_us();
            let until = now_us.saturating_add_signed(until_s * 1_000_000);
            heeding.lamp_did(crate::second_provider::Did::Said(2, until));
            let said = (now_us - long * 1_000_000, now_us);
            let held = heeding.heed(3, said, "Another one?", None);
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
            held
        };
        for in_thread in [true, false] {
            assert_eq!(
                judged(in_thread, 6, 3, 1),
                None,
                "still playing, {in_thread}"
            );
            assert_eq!(
                judged(in_thread, 25, -15, 17),
                None,
                "begun 2 s before the words ended, judged 15 s after, {in_thread}"
            );
            assert!(
                judged(in_thread, 25, -12, 0).is_some(),
                "begun 12 s after, {in_thread}"
            );
        }
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
    }

    /// `LAMP_NAME_OR_FOLLOW_UP` on this route: a line the same-voice check
    /// reads in another voice than the conversation's, ready at its verdict,
    /// is a second person at the lamp, and the room is busy for that line's
    /// own verdict, so a face turned to the lamp doesn't let it out (a
    /// reviewer's P2 on #2967: it was played as "someone is turned to the
    /// lamp" in the room it made busy). As (held, room): another voice with a
    /// face, held, busy; with no face, held, busy; with the name, played,
    /// busy; the conversation's own voice with a face, played, unknown; and
    /// without the switch another voice with a face is played and the room
    /// stays unknown.
    #[test]
    fn with_name_or_follow_up_another_voice_makes_the_room_busy_before_its_own_verdict() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        use crate::room_told::Room;
        use crate::second_provider::Heard;
        /// Prints by a line's first sample, answered at once: 1 is A's
        /// voice, -1 B's.
        #[derive(Default)]
        struct Voice(std::collections::VecDeque<(u64, Result<Vec<f32>, String>)>);
        impl crate::same_voice::Printer for Voice {
            fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
                let print = if samples[0] > 0 {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                };
                self.0.push_back((request, Ok(print)));
                true
            }
            fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
                self.0.pop_front()
            }
        }
        // A short root: a Unix socket's address holds 103 bytes.
        let root = std::path::PathBuf::from(format!("/tmp/lnv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut eyes = bus
            .publisher_declared(
                lamp_msgs::Faces::TOPIC,
                &lamp_msgs::Faces::declaration(),
                "eyes",
            )
            .unwrap();
        let mut look = |turned: bool| {
            let faces = lamp_msgs::Faces {
                faces: vec![lamp_msgs::Face {
                    id: "at-the-desk".into(),
                    xmin: 0.4,
                    ymin: 0.0,
                    xmax: 0.7,
                    ymax: 0.3,
                    confidence: 0.9,
                    yaw: None,
                    facing: Some(turned),
                    speaking: None,
                }],
            };
            eyes.publish_json(lamp_ipc::monotonic_us(), &faces).unwrap();
        };
        let said = (10_000_000, 11_000_000);
        let line = |heeding: &mut Heeding, request: u64, voice: i16, words: &str| {
            heeding.hear(
                request,
                Heard::Began {
                    over_the_lamps_voice: false,
                },
            );
            heeding.hear(request, Heard::Samples(&[voice; 48_000]));
            heeding.hear(request, Heard::Ended);
            let held = heeding.heed(request, said, words, None);
            let mut thread = crate::thread::THREAD.lock().unwrap();
            thread.person(words, true, Instant::now());
            if held.is_none() {
                thread.lamp(request, "Sure.", Instant::now());
            }
            held.is_some()
        };
        let mut judged = |on: bool, voice: i16, words: &str, turned: bool| {
            *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
            {
                let mut room = crate::room_told::room().lock().unwrap();
                *room = crate::room_told::RoomTold::new();
                room.count_only_talk(true);
                room.count_held_talk(on);
            }
            let mut heeding = Heeding::new(true, true, Some(&root), None);
            heeding.as_the_only_voice(false, true, false);
            if on {
                heeding.needing_name_or_follow_up();
            }
            heeding.checking_the_voice(Box::new(Voice::default()));
            // A calls the lamp, facing it: A's voice is the conversation's.
            look(true);
            assert!(!line(&mut heeding, 1, 1, "Lamp, what time is it?"));
            look(turned);
            let held = line(&mut heeding, 2, voice, words);
            let room = crate::room_told::room().lock().unwrap().at(monotonic_us());
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
            (held, room)
        };
        let follow_up = "And what about tomorrow?";
        assert_eq!(judged(true, -1, follow_up, true), (true, Room::Busy));
        assert_eq!(judged(true, -1, follow_up, false), (true, Room::Busy));
        assert_eq!(
            judged(true, -1, "Lamp, and what about tomorrow?", true),
            (false, Room::Busy)
        );
        assert_eq!(judged(true, 1, follow_up, true), (false, Room::Unknown));
        assert_eq!(judged(false, -1, follow_up, true), (false, Room::Unknown));
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `LAMP_NAME_OR_FOLLOW_UP`'s known limit on this route, as built: the
    /// person at the desk faces the lamp while a colleague off camera talks
    /// with them, and there is no voice model. One face is in view, turned
    /// to the lamp, in a room not known to be busy, so every line is let out
    /// by the face, nothing is held, and the room never turns busy: the
    /// desk talk is answered, as without the switch, until a line is held. A
    /// run on the lamp must say whether the colleague was in view and
    /// whether the voice model was on.
    #[test]
    fn with_name_or_follow_up_a_colleague_off_camera_is_answered_while_the_person_faces_the_lamp() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        // A short root: a Unix socket's address holds 103 bytes.
        let root = std::path::PathBuf::from(format!("/tmp/lnc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut eyes = bus
            .publisher_declared(
                lamp_msgs::Faces::TOPIC,
                &lamp_msgs::Faces::declaration(),
                "eyes",
            )
            .unwrap();
        let faces = lamp_msgs::Faces {
            faces: vec![lamp_msgs::Face {
                id: "at-the-desk".into(),
                xmin: 0.4,
                ymin: 0.0,
                xmax: 0.7,
                ymax: 0.3,
                confidence: 0.9,
                yaw: None,
                facing: Some(true),
                speaking: None,
            }],
        };
        eyes.publish_json(lamp_ipc::monotonic_us(), &faces).unwrap();
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
        {
            let mut room = crate::room_told::room().lock().unwrap();
            *room = crate::room_told::RoomTold::new();
            room.count_only_talk(true);
            room.count_held_talk(true);
        }
        let mut heeding = Heeding::new(true, true, Some(&root), None);
        heeding.as_the_only_voice(false, true, false);
        heeding.needing_name_or_follow_up();
        let now_us = monotonic_us();
        for (request, line) in (1..).zip([
            "Did you get my message about the budget?",
            "Yes, I'll read it after lunch.",
            "Can we move the meeting to three?",
            "Sure, three works for me.",
        ]) {
            let said = (
                now_us + request * 3_000_000,
                now_us + request * 3_000_000 + 1,
            );
            crate::room_told::room()
                .lock()
                .unwrap()
                .heard(request, said.0);
            assert_eq!(heeding.heed(request, said, line, None), None, "{line}");
            let mut room = crate::room_told::room().lock().unwrap();
            room.ended(request, said.1);
            room.spoke(request, said.1);
        }
        let room = crate::room_told::room()
            .lock()
            .unwrap()
            .at(now_us + 13_000_000);
        assert_ne!(room, crate::room_told::Room::Busy);
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `LAMP_NAME_OR_FOLLOW_UP` on this route: two faces in the eyes' newest
    /// look make the room this route keeps busy at once, and the line in
    /// hand is judged in it: a face turned to the lamp lets nothing out
    /// alone. One face, or the switch off, leaves the room as it was, and
    /// the face lets the line out. On the real clock, a moment.
    #[test]
    fn with_name_or_follow_up_two_faces_in_view_make_the_room_busy() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        use crate::room_told::Room;
        // A short root: a Unix socket's address holds 103 bytes.
        let root = std::path::PathBuf::from(format!("/tmp/lnf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut eyes = bus
            .publisher_declared(
                lamp_msgs::Faces::TOPIC,
                &lamp_msgs::Faces::declaration(),
                "eyes",
            )
            .unwrap();
        let face = |id: &str, facing: bool| lamp_msgs::Face {
            id: id.into(),
            xmin: 0.4,
            ymin: 0.0,
            xmax: 0.7,
            ymax: 0.3,
            confidence: 0.9,
            yaw: None,
            facing: Some(facing),
            speaking: None,
        };
        let mut judged = |on: bool, faces: Vec<lamp_msgs::Face>| {
            *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
            {
                let mut room = crate::room_told::room().lock().unwrap();
                *room = crate::room_told::RoomTold::new();
                room.count_only_talk(true);
                room.count_held_talk(on);
            }
            eyes.publish_json(lamp_ipc::monotonic_us(), &lamp_msgs::Faces { faces })
                .unwrap();
            let mut heeding = Heeding::new(true, true, Some(&root), None);
            heeding.as_the_only_voice(false, true, false);
            if on {
                heeding.needing_name_or_follow_up();
            }
            let now_us = monotonic_us();
            let held = heeding.heed(1, (now_us, now_us + 1_000_000), "Okay yeah sure.", None);
            let room = crate::room_told::room().lock().unwrap().at(now_us);
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
            (room, held)
        };
        let two = || vec![face("a", true), face("b", false)];
        assert_eq!(
            judged(true, two()),
            (Room::Busy, Some(crate::attention::BUSY_TURNED_ALONE))
        );
        assert_eq!(judged(true, vec![face("a", true)]), (Room::Unknown, None));
        assert_eq!(judged(false, two()), (Room::Unknown, None));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `LAMP_TWO_FACES_HELD`: two faces make the room busy only when the
    /// newest look and 3 of the last 2.5 s's looks saw two, spanning 1.5 s.
    /// One look is not enough, nor a second face that has left, nor looks
    /// too close together, nor an expired or old newest look.
    #[test]
    fn with_two_faces_held_two_faces_must_stay() {
        // In these the times are ms on the shared monotonic clock, and each
        // look is taken as it is published.
        let look = |at_ms: u64, faces: usize| FacesLook {
            at_us: at_ms * 1_000,
            faces,
            expires_ms: Some(2_000),
        };
        let held = |looks: &[(u64, usize)], now_ms: u64| {
            let mut held = FacesHeld::default();
            for &(at, faces) in looks {
                held.look(look(at, faces), 0, at * 1_000);
            }
            held.two_stayed(now_ms * 1_000)
        };
        // One look with two faces.
        assert!(!held(&[(10_000, 2)], 10_000));
        // The eyes' 250 ms pace, two faces over 1.5 s: 7 looks.
        let pace = |faces: &[usize]| -> Vec<(u64, usize)> {
            (0..)
                .zip(faces)
                .map(|(k, &f)| (10_000 + k * 250, f))
                .collect()
        };
        assert!(held(&pace(&[2; 7]), 11_500));
        // A stray look among one face.
        assert!(!held(&pace(&[1, 1, 1, 2, 1, 1, 1]), 11_500));
        // Two faces over 1.25 s only.
        assert!(!held(&pace(&[1, 1, 2, 2, 2, 2, 2, 2]), 11_750));
        // The second face has left in the newest look.
        assert!(!held(&pace(&[2, 2, 2, 2, 2, 2, 2, 1]), 11_750));
        // The 600 ms pace with nobody to follow: 4 looks over 1.8 s.
        assert!(held(
            &[(10_000, 2), (10_600, 2), (11_200, 1), (11_800, 2)],
            11_800
        ));
        // The looks all older than the 2.5 s kept, or the newest missing:
        // the verdict over 1 s after it.
        assert!(!held(&pace(&[2; 7]), 11_500 + FACES_KEPT_MS + 1));
        assert!(!held(&pace(&[2; 7]), 11_500 + LOOK_GAP_MS + 1));
        // An expired newest look, its own 500 ms run out.
        let mut short = FacesHeld::default();
        for (at, _) in pace(&[2; 7]) {
            let look = FacesLook {
                expires_ms: Some(500),
                ..look(at, 2)
            };
            short.look(look, 0, at * 1_000);
        }
        assert!(short.two_stayed((11_500 + 500) * 1_000));
        assert!(!short.two_stayed((11_500 + 500 + lamp_bus::EXPIRY_GRACE_MS + 1) * 1_000));
        // After a long gap, a lone face, or two faces seen once again.
        let gap = [(1_000, 2), (1_800, 2), (2_600, 2)];
        assert!(!held(&[&gap[..], &[(602_600, 1)]].concat(), 602_600));
        assert!(!held(&[&gap[..], &[(602_600, 2)]].concat(), 602_600));
        // A look dated before the last one kept is a break too.
        assert!(!held(
            &[(10_000, 2), (10_800, 2), (11_600, 2), (9_000, 2)],
            11_600
        ));
        // And the valid run still holds after such a break, once it stays.
        assert!(held(
            &[(9_000, 1), (12_000, 2), (12_800, 2), (13_600, 2)],
            13_600
        ));
        // A look dated ahead of now, past the grace, is a break, and not kept.
        let mut ahead = FacesHeld::default();
        for at in [100_000, 100_750] {
            ahead.look(look(at, 2), 0, at * 1_000);
        }
        ahead.look(look(101_500, 2), 0, 101_000 * 1_000);
        assert!(!ahead.two_stayed(101_500 * 1_000));
        assert!(ahead.looks.is_empty());
    }

    /// reviewer-two's, the reviewer-lead's and the second builder's HOLDs on
    /// #3355, on the shared monotonic clock FacesHeld alone reads: looks
    /// queued before a privacy change, or taken across it, never make the
    /// room busy; looks delivered late get no time back; and a wall clock
    /// stepped back or forward changes nothing, since no wall time is read.
    #[test]
    fn with_two_faces_held_privacy_late_looks_and_clock_steps_leave_no_busy_room() {
        let look = |at_ms: u64| FacesLook {
            at_us: at_ms * 1_000,
            faces: 2,
            expires_ms: Some(2_000),
        };
        // Queued: the eyes looked at 100 000, 100 750 and 101 500; privacy
        // closed and reopened at 101 550; the reader takes the first across
        // the change and the others after it, from 101 600.
        let mut held = FacesHeld::default();
        let before = held.epoch;
        held.privacy_changed(101_550_000);
        held.privacy_changed(101_550_000);
        held.look(look(100_000), before, 101_600_000);
        for (at, taken) in [(100_750, 101_610), (101_500, 101_620)] {
            held.look(look(at), held.epoch, taken * 1_000);
        }
        assert!(held.looks.is_empty());
        assert!(!held.two_stayed(101_650_000));
        // Fresh looks after it count, once they have stayed.
        for at in [101_700, 102_450, 103_200] {
            held.look(look(at), held.epoch, at * 1_000);
        }
        assert!(held.two_stayed(103_250_000));
        // Delivered late: looked at 100 000, 100 750 and 101 500, taken at
        // 101 900, 102 000 and 103 400, each already over 1 s old: none
        // kept, and at 104 300 no busy room.
        let mut late = FacesHeld::default();
        for (at, taken) in [(100_000, 101_900), (100_750, 102_000), (101_500, 103_400)] {
            late.look(look(at), 0, taken * 1_000);
        }
        assert!(late.looks.is_empty());
        assert!(!late.two_stayed(104_300_000));
        // Taken 400 ms late each: kept, fresh from when the eyes looked, so
        // busy at 102 000 and stale by 102 600.
        let mut slow = FacesHeld::default();
        for at in [100_000, 100_750, 101_500] {
            slow.look(look(at), 0, (at + 400) * 1_000);
        }
        assert!(slow.two_stayed(102_000_000));
        assert!(!slow.two_stayed(102_600_000));
        // The wall clock set back 60 s with no new look: the looks kept go
        // stale 1 s after the newest, as always.
        let mut kept = FacesHeld::default();
        for at in [100_000, 100_750, 101_500] {
            kept.look(look(at), 0, at * 1_000);
        }
        assert!(kept.two_stayed(101_600_000));
        assert!(!kept.two_stayed((101_500 + LOOK_GAP_MS + 1) * 1_000));
        // The wall clock stepped 500 ms forward through 1 s of real looks
        // (the second builder's trace): still 1 s, not busy.
        let mut stepped = FacesHeld::default();
        for at in [100_000, 100_250, 100_500, 100_750, 101_000] {
            stepped.look(look(at), 0, at * 1_000);
        }
        assert!(!stepped.two_stayed(101_000_000));
    }

    /// `LAMP_TWO_FACES_HELD` on this route with `LAMP_NAME_OR_FOLLOW_UP`,
    /// on the real clock: the eyes' looks published between lines are all
    /// read, so two faces held 2.1 s make the first line's room busy, and
    /// the face turned to the lamp lets nothing out alone; one stray look
    /// with two faces, or two faces whose newest look sees one, leave the
    /// room as it was, and so do looks before a privacy close and reopen.
    /// Without the switch one look is enough, as today.
    #[test]
    fn with_two_faces_held_looks_between_lines_make_the_first_line_busy() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        use crate::room_told::Room;
        // A short root: a Unix socket's address holds 103 bytes.
        let root = std::path::PathBuf::from(format!("/tmp/ltf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut eyes = bus
            .publisher_declared(
                lamp_msgs::Faces::TOPIC,
                &lamp_msgs::Faces::declaration(),
                "eyes",
            )
            .unwrap();
        eyes.expires_after(Some(Duration::from_secs(2)));
        let face = |id: &str, facing: bool| lamp_msgs::Face {
            id: id.into(),
            xmin: 0.4,
            ymin: 0.0,
            xmax: 0.7,
            ymax: 0.3,
            confidence: 0.9,
            yaw: None,
            facing: Some(facing),
            speaking: None,
        };
        let busy = (Room::Busy, Some(crate::attention::BUSY_TURNED_ALONE));
        // Each case: the switch, the looks' face counts, 300 ms apart, and
        // whether privacy closes and reopens after them, with one more look
        // with two faces after the reopening.
        for (held_on, looks, privacy, expected) in [
            (true, vec![2; 8], false, busy),
            (true, vec![1, 1, 1, 2], false, (Room::Unknown, None)),
            (
                true,
                vec![2, 2, 2, 2, 2, 2, 1],
                false,
                (Room::Unknown, None),
            ),
            (true, vec![2; 8], true, (Room::Unknown, None)),
            (false, vec![2], false, busy),
        ] {
            *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
            {
                let mut room = crate::room_told::room().lock().unwrap();
                *room = crate::room_told::RoomTold::new();
                room.count_only_talk(true);
                room.count_held_talk(true);
            }
            let mut heeding = Heeding::new(true, true, Some(&root), None);
            heeding.as_the_only_voice(false, true, false);
            heeding.needing_name_or_follow_up();
            if held_on {
                heeding.holding_two_faces(Some(&root));
                // The reader subscribes before the first look.
                std::thread::sleep(Duration::from_millis(100));
            }
            for (k, &count) in looks.iter().enumerate() {
                if k > 0 {
                    std::thread::sleep(Duration::from_millis(300));
                }
                let faces = [face("a", true), face("b", false)][..count].to_vec();
                eyes.publish_json(lamp_ipc::monotonic_us(), &lamp_msgs::Faces { faces })
                    .unwrap();
            }
            std::thread::sleep(Duration::from_millis(100));
            if privacy {
                // Closed, then reopened.
                heeding.privacy_changed();
                heeding.privacy_changed();
                let faces = vec![face("a", true), face("b", false)];
                eyes.publish_json(lamp_ipc::monotonic_us(), &lamp_msgs::Faces { faces })
                    .unwrap();
                std::thread::sleep(Duration::from_millis(100));
            }
            let now_us = monotonic_us();
            let held = heeding.heed(1, (now_us, now_us + 1_000_000), "Okay yeah sure.", None);
            let room = crate::room_told::room().lock().unwrap().at(now_us);
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
            assert_eq!((room, held), expected, "{held_on} {looks:?} {privacy}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `LAMP_NAME_OR_FOLLOW_UP` on this route, through the thread and the
    /// lamp's words as the route keeps them: after "Lamp, what's the weather
    /// today?" was answered, in a busy room, a follow-up said 5 s after the
    /// lamp's words ended is played as the conversation going on, and so is
    /// one begun 5 s after them that went on 7 s, judged 12 s after them
    /// (a reviewer's P2 on #2967: the window runs to the line's start, not
    /// its verdict); one begun 12 s after, without the name, is held; with
    /// the name it is played. Without the switch the 12 s one is played, in
    /// a busy room's 15 s. In a quiet room the windows are as without the
    /// switch: 12 s and 15 s after the words are played.
    #[test]
    fn with_name_or_follow_up_a_follow_up_goes_on_only_eight_seconds() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        let _clock = clock_with_history();
        let s = Duration::from_secs;
        let judged = |on: bool, busy: bool, ago: u64, long: u64, heard: &str| {
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
            if busy {
                let mut room = crate::room_told::room().lock().unwrap();
                for other in 901..=903 {
                    room.unanswered(other, monotonic_us());
                }
                assert_eq!(room.at(monotonic_us()), Room::Busy);
            }
            // The answer began 2 s before its words ended, `ago` s ago.
            let began = Instant::now() - s(ago + 2);
            let mut thread = crate::thread::Thread::new();
            thread.person("Lamp, what's the weather today?", true, began);
            thread.lamp(1, "Sunny and warm today.", began);
            *crate::thread::THREAD.lock().unwrap() = thread;
            let mut heeding = Heeding::new(false, false, None, None);
            heeding.as_the_only_voice(false, true, false);
            if on {
                heeding.needing_name_or_follow_up();
            }
            heeding.lamp_did(crate::second_provider::Did::Said(
                1,
                monotonic_us() - ago * 1_000_000,
            ));
            // Begun `long` s ago, and heard to its end now.
            let now_us = monotonic_us();
            let said = (now_us - long * 1_000_000, now_us + 1_000_000);
            heeding.heed(2, said, heard, None)
        };
        use crate::room_told::Room;
        let follow_up = "And what about tomorrow?";
        assert_eq!(judged(true, true, 5, 0, follow_up), None, "5 s");
        assert_eq!(
            judged(true, true, 12, 7, follow_up),
            None,
            "begun 5 s after, judged 12 s after"
        );
        assert!(judged(true, true, 12, 0, follow_up).is_some(), "12 s");
        assert_eq!(
            judged(true, true, 12, 0, "Lamp, and what about tomorrow?"),
            None,
            "12 s, the name"
        );
        assert_eq!(judged(false, true, 12, 0, follow_up), None, "without");
        // A quiet room: as without the switch.
        for ago in [5, 12, 15] {
            assert_eq!(
                judged(true, false, ago, 0, follow_up),
                None,
                "quiet, {ago} s"
            );
            assert_eq!(
                judged(false, false, ago, 0, follow_up),
                None,
                "without, {ago} s"
            );
        }
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    }

    /// #829 on the second route (#1712, Tests' map item 1): with
    /// `LAMP_SAME_VOICE_MODEL`, a line in another voice than the
    /// conversation's, going on from it, is held as no part of it; after
    /// it, a question in the conversation's own voice goes on, as on Live.
    #[test]
    fn another_voice_going_on_is_held_and_the_conversations_own_goes_on() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        use crate::second_provider::Heard;
        /// Prints by a line's first sample, answered at once: 1 is A's
        /// voice, -1 B's.
        #[derive(Default)]
        struct Voice(std::collections::VecDeque<(u64, Result<Vec<f32>, String>)>);
        impl crate::same_voice::Printer for Voice {
            fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
                let print = if samples[0] > 0 {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                };
                self.0.push_back((request, Ok(print)));
                true
            }
            fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
                self.0.pop_front()
            }
        }
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
        {
            let mut room = crate::room_told::room().lock().unwrap();
            *room = crate::room_told::RoomTold::new();
            room.count_only_talk(true);
        }
        let said = (10_000_000, 11_000_000);
        let mut heeding = Heeding::new(true, true, None, None);
        heeding.as_the_only_voice(false, true, false);
        heeding.checking_the_voice(Box::new(Voice::default()));
        let line = |heeding: &mut Heeding, request: u64, voice: i16, words: &str| {
            heeding.hear(
                request,
                Heard::Began {
                    over_the_lamps_voice: false,
                },
            );
            heeding.hear(request, Heard::Samples(&[voice; 48_000]));
            heeding.hear(request, Heard::Ended);
            let held = heeding.heed(request, said, words, None);
            let mut thread = crate::thread::THREAD.lock().unwrap();
            thread.person(words, true, Instant::now());
            if held.is_none() {
                thread.lamp(request, "Sure.", Instant::now());
            }
            held
        };
        // A calls the lamp: A's voice is the conversation's.
        assert_eq!(line(&mut heeding, 1, 1, "Lamp, what time is it?"), None);
        // B, going on from it in another voice: held.
        assert!(
            line(&mut heeding, 2, -1, "And what about tomorrow?").is_some(),
            "another voice is no part of the conversation"
        );
        // A again, a question: the conversation goes on.
        assert_eq!(line(&mut heeding, 3, 1, "And what about tomorrow?"), None);
        // Without the check, B's line would have gone on.
        let mut unchecked = Heeding::new(true, true, None, None);
        unchecked.as_the_only_voice(false, true, false);
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
        crate::room_told::room()
            .lock()
            .unwrap()
            .count_only_talk(true);
        assert_eq!(line(&mut unchecked, 1, 1, "Lamp, what time is it?"), None);
        assert_eq!(
            line(&mut unchecked, 2, -1, "And what about tomorrow?"),
            None
        );
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    }

    /// Ten's trace on #3126 (05:0x), pinned for the owner's question 4 (the
    /// first builder at 05:28): with `LAMP_SAME_VOICE_MODEL`,
    /// `LAMP_SPOKEN_TO` and `LAMP_NAME_OR_FOLLOW_UP`, an answered ask, then
    /// another line begun 4 s after the answer's words ended, inside
    /// [`crate::name_or_follow_up::FOLLOW_UP_FOR`]. In another voice than
    /// the asker's (a likeness under [`crate::same_voice::DIFFERENT_BELOW`])
    /// it is held: a colleague's line, and as much a second person's real
    /// follow-up. With no likeness at its verdict (its print not back) it
    /// goes on, and the journal says it was judged without the voice check.
    /// The asker's own voice goes on, judged. In a room already busy, with
    /// the call and the answer's start past the 8 s, so that only the end
    /// of the lamp's words lets a line go on; 9 s after it, past the 8 s,
    /// every line is held (reviewer-two's P3 on #3139).
    #[test]
    fn a_line_4_s_after_an_answer_is_held_by_another_voice_or_goes_on_judged_without_it() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        let _clock = clock_with_history();
        use crate::second_provider::{Did, Heard};
        use std::sync::{Arc, Mutex};
        /// Prints by a line's first sample: 1 is the asker's voice, -1
        /// another's. Another's print comes back only with `back`.
        struct Voice {
            prints: std::collections::VecDeque<(u64, Result<Vec<f32>, String>)>,
            back: bool,
        }
        impl crate::same_voice::Printer for Voice {
            fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
                if samples[0] > 0 {
                    self.prints.push_back((request, Ok(vec![1.0, 0.0])));
                } else if self.back {
                    self.prints.push_back((request, Ok(vec![0.0, 1.0])));
                }
                true
            }
            fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
                self.prints.pop_front()
            }
        }
        let hear = |heeding: &mut Heeding, request: u64, voice: i16| {
            heeding.hear(
                request,
                Heard::Began {
                    over_the_lamps_voice: false,
                },
            );
            heeding.hear(request, Heard::Samples(&[voice; 48_000]));
            heeding.hear(request, Heard::Ended);
        };
        // Whether the second line, in `voice`, begun `ended_s` after the
        // answer's words ended, is held, and the journal's lines. The room
        // is already busy, so only `FOLLOW_UP_FOR` from the end of the
        // lamp's words or the call lets a line go on, and the call is
        // backdated past it: the reply's end alone decides (reviewer-two's
        // P3 on #3139).
        let judged = |voice: i16, back: Option<bool>, ended_s: u64| -> (bool, Vec<String>) {
            *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
            {
                let mut room = crate::room_told::room().lock().unwrap();
                *room = crate::room_told::RoomTold::new();
                room.count_only_talk(true);
                room.count_held_talk(true);
            }
            let journal = Arc::new(Mutex::new(Vec::new()));
            let mut heeding = Heeding::new(true, true, None, None);
            heeding.as_the_only_voice(false, true, false);
            heeding.needing_name_or_follow_up();
            let into = Arc::clone(&journal);
            // No `back`: no voice check at all.
            heeding.voices = back.map(|back| {
                crate::same_voice::Voices::journaled(
                    Box::new(Voice {
                        prints: Default::default(),
                        back,
                    }) as Box<dyn crate::same_voice::Printer>,
                    Arc::new(move |line: &str| into.lock().unwrap().push(line.to_owned())),
                )
            });
            // The asker calls the lamp, and is answered for 6 s: the call
            // ended `ended_s` + 8 s ago, the answer began `ended_s` + 6 s ago,
            // past the 8 s from its start, and its words ended `ended_s` ago.
            let now_us = monotonic_us();
            let ago_us = |s: u64| now_us - s * 1_000_000;
            let ago = |s: u64| Instant::now() - Duration::from_secs(s);
            hear(&mut heeding, 1, 1);
            let call = "Lamp, what time is it?";
            assert_eq!(
                heeding.heed(1, (ago_us(ended_s + 9), ago_us(ended_s + 8)), call, None),
                None
            );
            heeding.called_at = Some(ago(ended_s + 8));
            {
                let mut thread = crate::thread::THREAD.lock().unwrap();
                thread.person(call, true, ago(ended_s + 8));
                thread.lamp(1, "It's ten past six.", ago(ended_s + 6));
            }
            heeding.lamp_did(Did::Said(1, ago_us(ended_s)));
            // Two people talking at the desk: the room is busy.
            assert!(crate::room_told::room().lock().unwrap().two_people(now_us));
            hear(&mut heeding, 2, voice);
            let line = "Did you send the slides?";
            let held = heeding
                .heed(2, (now_us, now_us + 1_500_000), line, None)
                .is_some();
            *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
            let lines = journal.lock().unwrap().clone();
            (held, lines)
        };
        let without =
            "request 2: judged without the voice check: its voice print was not back at its answer";
        let said_without = |lines: &[String]| lines.iter().any(|line| line.contains(without));
        // Another voice, its print back: under 0.30, held, judged.
        let (held, lines) = judged(-1, Some(true), 4);
        assert!(held, "another voice 4 s after the answer is held");
        assert!(!said_without(&lines), "{lines:?}");
        // Another voice, its print not back at its verdict: it goes on, and
        // the journal says so.
        let (held, lines) = judged(-1, Some(false), 4);
        assert!(!held, "with no likeness it goes on");
        assert!(said_without(&lines), "{lines:?}");
        // The asker's own voice: it goes on, judged.
        let (held, lines) = judged(1, Some(true), 4);
        assert!(!held, "the asker's own follow-up goes on");
        assert!(!said_without(&lines), "{lines:?}");
        // Without the voice check, the same line in another voice goes on:
        // the hold is the check's.
        let (held, lines) = judged(-1, None, 4);
        assert!(!held, "unchecked, another voice goes on");
        assert!(lines.is_empty(), "{lines:?}");
        // Just past the 8 s: no conversation goes on in the busy room, so
        // even the asker's own line is held, and nothing is judged without
        // the check.
        let (held, lines) = judged(1, Some(true), 9);
        assert!(held, "9 s after the answer, past FOLLOW_UP_FOR, it is held");
        assert!(!said_without(&lines), "{lines:?}");
        let (held, lines) = judged(-1, Some(false), 9);
        assert!(held, "past FOLLOW_UP_FOR, a line with no print is held too");
        assert!(!said_without(&lines), "{lines:?}");
    }

    /// Prints for the carry tests by a line's first sample: 1 is the
    /// owner's voice; 2 the owner's too, a short print reading 0.27 against
    /// it, as on lamp-4ace on 23 Oct; 3 a short print reading 0.50; -1 a colleague's; for 0 none ever
    /// comes back. While `late` is set, prints wait until it is cleared.
    #[derive(Default)]
    struct CarryVoice {
        prints: std::collections::VecDeque<(u64, Result<Vec<f32>, String>)>,
        late: Arc<std::sync::atomic::AtomicBool>,
    }

    impl crate::same_voice::Printer for CarryVoice {
        fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
            let print = match samples[0] {
                1 => vec![1.0, 0.0],
                2 => vec![0.27, 0.962_8],
                3 => vec![0.5, 0.866],
                // A colleague's voice reading 0.75 against the owner's.
                4 => vec![0.75, 0.661_4],
                -1 => vec![0.0, 1.0],
                -2 => vec![-1.0, 0.0],
                _ => return true,
            };
            self.prints.push_back((request, Ok(print)));
            true
        }
        fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
            if self.late.load(std::sync::atomic::Ordering::Relaxed) {
                return None;
            }
            self.prints.pop_front()
        }
    }

    /// The owner at a desk on lamp-4ace on 23 Oct (Queue #3126), on the
    /// second route as the lamp's only voice, with `LAMP_SPOKEN_TO`,
    /// `LAMP_NAME_OR_FOLLOW_UP`, `LAMP_BUSY_ENDS_SOONER`, the same-voice
    /// check and the eyes' speaking faces, and `LAMP_SAME_SPEAKER_CARRIES`
    /// when `carries`. The owner, face "f1", calls the lamp in a room not
    /// yet busy and is answered; then two people make the room busy.
    struct CarryDesk {
        heeding: Heeding,
        eyes: Arc<crate::speaking_faces::SpeakingFaces>,
        late: Arc<std::sync::atomic::AtomicBool>,
        /// The clocks the lines' windows are laid out from: each line 2 s
        /// long, 3 s after the one before.
        start: (Instant, u64),
        request: u64,
        seq: u64,
        /// The next line's face is seen speaking only before it began.
        stale: bool,
        /// The next line's face is seen speaking turned away from the lamp.
        away: bool,
        /// The request the lamp last answered ([`Self::answered_ms`]).
        answered: u64,
    }

    impl CarryDesk {
        fn new(carries: bool) -> Self {
            Self::new_in(carries, true)
        }

        /// [`Self::new`], the room made busy after the call only when `busy`.
        fn new_in(carries: bool, busy: bool) -> Self {
            *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
            {
                let mut room = crate::room_told::room().lock().unwrap();
                *room = crate::room_told::RoomTold::new();
                room.count_only_talk(true);
                room.count_held_talk(true);
                room.end_busy_sooner(true);
                room.end_busy_once_answered(true);
            }
            let mut heeding = Heeding::new(true, true, None, None);
            heeding.as_the_only_voice(false, true, false);
            heeding.needing_name_or_follow_up();
            if carries {
                heeding.carrying_the_same_speaker();
            }
            let voice = CarryVoice::default();
            let late = voice.late.clone();
            heeding.voices = Some(crate::same_voice::Voices::new(
                Box::new(voice) as Box<dyn crate::same_voice::Printer>
            ));
            let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
            heeding.seeing_speaking_faces(eyes.clone(), None);
            let mut desk = Self {
                heeding,
                eyes,
                late,
                start: (Instant::now(), monotonic_us()),
                request: 0,
                seq: 0,
                stale: false,
                away: false,
                answered: 0,
            };
            assert_eq!(
                desk.line(1, "f1", "Lamp, what time is it?"),
                None,
                "the call"
            );
            desk.called_long_ago();
            if busy {
                assert!(
                    crate::room_told::room()
                        .lock()
                        .unwrap()
                        .two_people(monotonic_us())
                );
            }
            desk
        }

        /// The conversation's voice is its caller's alone
        /// (`LAMP_SAME_VOICE_CALLER_ONLY`), from the call on.
        fn caller_only(&mut self) {
            self.heeding.voices = self
                .heeding
                .voices
                .take()
                .map(|voices| voices.callers_only(true));
        }

        /// The call is long past, so only the lamp's words carry.
        fn called_long_ago(&mut self) {
            self.heeding.called_at = Some(Instant::now() - Duration::from_secs(60));
        }

        /// The lamp's answer to the last line, its words ended `gap_ms` ago.
        fn answered_ms(&mut self, gap_ms: u64) {
            self.answered = self.request;
            self.words_ended_ms(gap_ms);
            self.heeding.lamp_did(crate::second_provider::Did::Said(
                self.request,
                monotonic_us() - gap_ms * 1_000,
            ));
        }

        /// The thread's last answer, to the request last answered, its words
        /// ended `gap_ms` ago.
        fn words_ended_ms(&mut self, gap_ms: u64) {
            let ago = |ms: u64| Instant::now() - Duration::from_millis(ms);
            let mut thread = crate::thread::Thread::new();
            thread.person("What time is it?", true, ago(gap_ms + 4_000));
            thread.lamp(self.answered, "It's ten past six.", ago(gap_ms + 2_000));
            *crate::thread::THREAD.lock().unwrap() = thread;
        }

        /// Time passes with no new answer: the lamp's last words, the same
        /// answer's, now ended `gap` s ago. No `Did::Said` is told.
        fn later(&mut self, gap: u64) {
            self.words_ended_ms(gap * 1_000);
            let until = Some(monotonic_us() - gap * 1_000_000);
            self.heeding.said_until_us = until;
            self.heeding.said_until_free_us = until;
        }

        fn answered(&mut self, gap: u64) {
            self.answered_ms(gap * 1_000);
        }

        /// A line of `words` in `voice`, `face` seen speaking it ("" none):
        /// why it is held, if it is.
        fn line(&mut self, voice: i16, face: &str, words: &str) -> Option<&'static str> {
            let index = self.request + 1;
            let (_, now_us) = self.start;
            let began = now_us + (index * 3_000 + 1_000) * 1_000;
            self.line_at(voice, face, words, began)
        }

        /// [`Self::line`], begun at `began_us`.
        fn line_at(
            &mut self,
            voice: i16,
            face: &str,
            words: &str,
            began_us: u64,
        ) -> Option<&'static str> {
            use lamp_msgs::{Face, Faces, Speaking, SpeakingState};
            self.request += 1;
            let index = self.request;
            let (now, now_us) = self.start;
            let at_ms = (began_us - now_us.min(began_us)) / 1_000;
            // A stale look: the face seen speaking until 1.7 s before the
            // line began.
            let from_ms = if std::mem::take(&mut self.stale) {
                at_ms.saturating_sub(2_900)
            } else {
                at_ms + 200
            };
            let away = std::mem::take(&mut self.away);
            for look in (0..4u64).filter(|_| !face.is_empty()) {
                self.seq += 1;
                let faces = Faces {
                    faces: vec![Face {
                        id: face.to_owned(),
                        xmin: 0.2,
                        ymin: 0.2,
                        xmax: 0.4,
                        ymax: 0.5,
                        confidence: 1.0,
                        yaw: None,
                        facing: Some(!away),
                        speaking: Some(Speaking {
                            state: SpeakingState::Yes,
                            score: 0.9,
                            from: None,
                        }),
                    }],
                };
                self.eyes.look(
                    now + Duration::from_millis(from_ms + look * 400),
                    Some((("b", self.seq), &faces)),
                );
            }
            self.heeding.hear(
                index,
                crate::second_provider::Heard::Began {
                    over_the_lamps_voice: false,
                },
            );
            self.heeding.hear(
                index,
                crate::second_provider::Heard::Samples(&[voice; 48_000]),
            );
            self.heeding
                .hear(index, crate::second_provider::Heard::Ended);
            self.heeding
                .heed(index, (began_us, began_us + 2_000_000), words, None)
        }

        /// Five of the owner's lines, each begun `gap` s after the lamp's
        /// words to the one before: why each is held, if it is.
        fn five(&mut self, gap: u64, voice: i16, face: &str) -> Vec<Option<&'static str>> {
            [
                "And what about tomorrow?",
                "Will it rain in the afternoon?",
                "Should I take an umbrella then?",
                "What time does the sun set?",
                "Can you remind me at five?",
            ]
            .iter()
            .map(|words| {
                self.answered(gap);
                self.line(voice, face, words)
            })
            .collect()
        }
    }

    impl Drop for CarryDesk {
        fn drop(&mut self) {
            *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
        }
    }

    /// A two-second colleague line begun at +3 s ends at +5 s. First's
    /// bounded-window contract admits an owner's +25 s start, not +45 s.
    #[test]
    fn a_held_colleague_extends_the_carry_window_but_not_past_its_cap() {
        let _one = crate::provider_worker::tests::one_thread();
        for (after_s, expected) in [(25, true), (45, false)] {
            let mut desk = CarryDesk::new(true);
            desk.answered(60);
            let answer_end = desk.heeding.said_until_us.unwrap();
            let colleague = (answer_end + 3_000_000, answer_end + 5_000_000);
            desk.heeding.signs_begun(2, colleague, colleague.0);
            assert!(
                desk.heeding.carried.is_some(),
                "colleague began in the carry"
            );
            assert!(desk.heeding.colleague_held(2));
            let began = answer_end + after_s * 1_000_000;
            desk.heeding
                .signs_begun(3, (began, began + 2_000_000), began);
            assert_eq!(desk.heeding.carried.is_some(), expected, "+{after_s} s");
        }
    }

    #[test]
    fn the_extended_carry_still_needs_the_owner_and_ends_on_stop_privacy_or_forget() {
        let _one = crate::provider_worker::tests::one_thread();
        for end in ["none", "stop", "privacy", "forget", "two colleagues"] {
            let mut desk = CarryDesk::new(true);
            desk.answered(60);
            let answer = desk.heeding.said_until_us.unwrap();
            let began = answer + 3_000_000;
            desk.heeding
                .signs_begun(2, (began, began + 2_000_000), began);
            assert!(desk.heeding.colleague_held(2));
            match end {
                "stop" => desk.heeding.lamp_did(crate::second_provider::Did::Stopped),
                "privacy" => desk.heeding.privacy_changed(),
                "forget" => crate::thread::THREAD.lock().unwrap().forget(),
                "two colleagues" => assert!(!desk.heeding.colleague_held(3)),
                _ => {}
            }
            let began = answer + 25_000_000;
            desk.heeding
                .signs_begun(4, (began, began + 2_000_000), began);
            assert_eq!(desk.heeding.carried.is_some(), end == "none", "{end}");
            if end == "none" {
                let owner = desk.heeding.caller_faces.clone();
                let colleague = vec!["f2".to_owned()];
                assert_eq!(
                    desk.heeding
                        .carried_by_the_same_speaker(4, None, &owner, &owner)
                        .goes_on,
                    Some(true)
                );
                assert_eq!(
                    desk.heeding
                        .carried_by_the_same_speaker(4, None, &colleague, &colleague)
                        .goes_on,
                    Some(false)
                );
                desk.heeding
                    .carried_by_the_same_speaker(4, None, &owner, &Vec::new());
                assert!(!desk.heeding.carrying, "owner turned away");
                assert!(!desk.heeding.carry_window.extra_allows(began));
            } else {
                assert!(!desk.heeding.carry_window.extra_allows(began), "{end}");
            }
        }
        let mut off = CarryDesk::new(false);
        off.answered(60);
        let answer = off.heeding.said_until_us.unwrap();
        let began = answer + 3_000_000;
        off.heeding
            .signs_begun(2, (began, began + 2_000_000), began);
        assert!(off.heeding.carried.is_none());
        assert!(!off.heeding.carry_window.extra_allows(answer + 25_000_000));
    }

    #[test]
    fn a_fresh_call_and_actual_answer_after_forget_start_a_new_bounded_window() {
        let _one = crate::provider_worker::tests::one_thread();
        let mut desk = CarryDesk::new(true);
        desk.answered(60);
        crate::thread::THREAD.lock().unwrap().forget();
        crate::room_told::room().lock().unwrap().set_quiet();
        assert_eq!(
            desk.line(1, "f1", "Lamp, will it rain in the afternoon?"),
            None
        );
        desk.answered(30);
        let answer = desk.heeding.said_until_us.unwrap();
        let request = desk.request + 1;
        let began = answer + 3_000_000;
        desk.heeding
            .signs_begun(request, (began, began + 2_000_000), began);
        assert!(desk.heeding.colleague_held(request));
        let began = answer + 25_000_000;
        desk.heeding
            .signs_begun(request + 1, (began, began + 2_000_000), began);
        assert!(
            desk.heeding.carried.is_some(),
            "fresh actual answer reopens the allowance"
        );
    }

    /// Synthetic sure-other bar, as the existing late-print control: the
    /// first colleague print is learned only at the owner's +25 s verdict.
    /// Production calibration and activation are unchanged.
    #[test]
    fn a_colleague_print_first_known_at_the_extended_verdict_keeps_the_owner() {
        let _one = crate::provider_worker::tests::one_thread();
        for answered_s in [30, 60] {
            for room in ["busy", "unknown", "quiet"] {
                for (voice, owner) in [(1, true), (4, false), (3, false), (0, false), (-1, false)] {
                    for after_s in [25, 45] {
                        let mut desk = CarryDesk::new(true);
                        desk.heeding.sure_other_samples = crate::same_voice::RATE * 3 / 2;
                        desk.answered(answered_s);
                        let answer = desk.heeding.said_until_us.unwrap();
                        desk.late.store(true, std::sync::atomic::Ordering::Relaxed);
                        assert_eq!(
                            desk.line_at(
                                -1,
                                "",
                                "I sent the slides to the client this morning.",
                                answer + 3_000_000
                            ),
                            None
                        );
                        // Keep the initial legacy follow-up's admission, then
                        // vary the room observed at the owner's later verdict.
                        {
                            let mut told = crate::room_told::room().lock().unwrap();
                            let expected = match room {
                                "unknown" => {
                                    *told = crate::room_told::RoomTold::new();
                                    told.count_only_talk(true);
                                    told.count_held_talk(true);
                                    told.end_busy_sooner(true);
                                    told.end_busy_once_answered(true);
                                    crate::room_told::Room::Unknown
                                }
                                "quiet" => {
                                    told.set_quiet();
                                    crate::room_told::Room::Quiet
                                }
                                _ => crate::room_told::Room::Busy,
                            };
                            assert_eq!(told.at(monotonic_us()), expected, "fixture room {room}");
                        }
                        desk.late.store(false, std::sync::atomic::Ordering::Relaxed);
                        assert_eq!(
                            desk.line_at(
                                voice,
                                "",
                                "Will it rain in the afternoon?",
                                answer + after_s * 1_000_000
                            )
                            .is_none(),
                            owner && after_s == 25,
                            "answer {answered_s} s, room {room}, voice {voice}, +{after_s} s"
                        );
                        if after_s == 25 && voice != -1 {
                            let expected = match room {
                                "unknown" => crate::room_told::Room::Unknown,
                                "quiet" => crate::room_told::Room::Quiet,
                                _ => crate::room_told::Room::Busy,
                            };
                            assert_eq!(
                                crate::room_told::room().lock().unwrap().at(monotonic_us()),
                                expected,
                                "room unchanged: answer {answered_s} s, {room}, voice {voice}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_late_window_keeps_the_same_voice_sign_for_going_on_same_one() {
        let _one = crate::provider_worker::tests::one_thread();
        for answered_s in [30, 60] {
            let mut desk = CarryDesk::new(true);
            desk.heeding.sure_other_samples = crate::same_voice::RATE * 3 / 2;
            desk.answered(answered_s);
            let answer = desk.heeding.said_until_us.unwrap();
            desk.late.store(true, std::sync::atomic::Ordering::Relaxed);
            assert_eq!(
                desk.line_at(
                    -1,
                    "",
                    "I sent the slides to the client this morning.",
                    answer + 3_000_000
                ),
                None
            );
            desk.heeding.going_on_same_one = true;
            desk.late.store(false, std::sync::atomic::Ordering::Relaxed);
            let request = desk.request + 1;
            let began = answer + 25_000_000;
            let line = (began, began + 2_000_000);
            desk.heeding.hear(
                request,
                crate::second_provider::Heard::Began {
                    over_the_lamps_voice: false,
                },
            );
            desk.heeding.hear(
                request,
                crate::second_provider::Heard::Samples(&[1; 48_000]),
            );
            desk.heeding
                .hear(request, crate::second_provider::Heard::Ended);
            let mut signs = desk.heeding.signs_begun(request, line, began);
            assert!(
                desk.heeding.carried.is_none(),
                "the late window is not known yet"
            );
            // Exercise an independently supplied continuation sign: the
            // first lookup consumes the owner print before opening the
            // late window, so the second lookup cannot classify it again.
            signs.going_on = true;
            assert_eq!(
                desk.heeding
                    .judge(request, line, "Will it rain in the afternoon?", signs),
                None,
                "voice-only owner with the same-speaker rule, answer {answered_s} s"
            );
        }
    }

    #[test]
    fn a_late_colleague_print_cannot_revive_a_closed_or_disabled_carry() {
        let _one = crate::provider_worker::tests::one_thread();
        for end in ["stop", "privacy", "forget", "off"] {
            let mut desk = CarryDesk::new(end != "off");
            desk.heeding.sure_other_samples = crate::same_voice::RATE * 3 / 2;
            desk.answered(60);
            let answer = desk.heeding.said_until_us.unwrap();
            desk.late.store(true, std::sync::atomic::Ordering::Relaxed);
            assert_eq!(
                desk.line_at(
                    -1,
                    "",
                    "I sent the slides to the client this morning.",
                    answer + 3_000_000
                ),
                None
            );
            match end {
                "stop" => desk.heeding.lamp_did(crate::second_provider::Did::Stopped),
                "privacy" => desk.heeding.privacy_changed(),
                "forget" => crate::thread::THREAD.lock().unwrap().forget(),
                _ => {}
            }
            desk.late.store(false, std::sync::atomic::Ordering::Relaxed);
            let began = answer + 25_000_000;
            assert!(
                desk.line_at(1, "", "Will it rain in the afternoon?", began)
                    .is_some(),
                "{end}"
            );
            assert!(!desk.heeding.carry_window.extra_allows(began), "{end}");
        }
    }

    /// The owner's P1 on Queue #3126 (lamp-4ace, 23 Oct 18:51): in a busy
    /// room, each line begun 12 s after the lamp's words, past the 8 s a
    /// follow-up has, five turns of the owner are all answered with
    /// `LAMP_SAME_SPEAKER_CARRIES`: by their voice and face, by their voice
    /// alone (at 0.70 or over), by their face alone, and by their face with
    /// short prints that read 0.27 or 0.00 against their voice, which say
    /// nothing either way (the first builder at 03:03: the chain broke on
    /// such prints). Without the switch the first is held as it was on the
    /// lamp.
    #[test]
    fn the_same_speaker_goes_on_five_turns_in_a_busy_room_only_with_the_switch() {
        let _one = crate::provider_worker::tests::one_thread();
        for (voice, face) in [(1, "f1"), (1, ""), (0, "f1"), (2, "f1"), (-1, "f1")] {
            let mut desk = CarryDesk::new(true);
            assert_eq!(
                desk.five(12, voice, face),
                [None; 5],
                "voice {voice}, face {face:?}"
            );
            let room = crate::room_told::room().lock().unwrap();
            assert!(!room.talked_past_the_lamp(), "voice {voice}, face {face:?}");
        }
        let mut off = CarryDesk::new(false);
        assert_eq!(
            off.five(12, 1, "f1")[0],
            Some(crate::attention::BUSY_UNCALLED),
            "without the switch, as today"
        );
        let mut off = CarryDesk::new(false);
        assert_eq!(
            off.five(12, 2, "f1")[0],
            Some(crate::attention::BUSY_UNCALLED),
            "without the switch, the owner's short print"
        );
    }

    /// `LAMP_SAME_SPEAKER_CARRIES`, the first builder's contract at 09:34
    /// on #3296 (it replaces 03:46's, where one colleague line ended the
    /// carry): a colleague's line between the owner's, their own face seen
    /// speaking it, in their voice or not, or in the owner's voice beside
    /// it, is held and ends nothing, so the owner goes on. Two lines in a
    /// row from someone else are people talking between them: the carry is
    /// over, the owner's return is held until the lamp answers them again,
    /// here by its name, and the carry goes on from that answer.
    #[test]
    fn a_colleagues_line_is_held_and_two_in_a_row_end_the_carry() {
        let _one = crate::provider_worker::tests::one_thread();
        let owner = "Will it rain in the afternoon?";
        // A statement, and a question (the reviewer-lead on #3331): held
        // just before the owner's carried line, it makes that line no reply
        // to it (`attention::AFTER_QUESTION`).
        for (colleague, (voice, face)) in [
            "I sent the slides to the client this morning.",
            "Did you send the slides to the client?",
        ]
        .into_iter()
        .flat_map(|colleague| [(-1, "f2"), (0, "f2"), (1, "f2")].map(|line| (colleague, line)))
        {
            let mut desk = CarryDesk::new(true);
            desk.answered(12);
            assert_eq!(desk.line(1, "f1", owner), None);
            desk.answered(12);
            assert!(
                desk.line(voice, face, colleague).is_some(),
                "voice {voice}, face {face:?}: held"
            );
            assert_eq!(
                desk.line(1, "f1", owner),
                None,
                "voice {voice}, face {face:?}: one line ends nothing"
            );
            desk.answered(12);
            assert!(desk.line(voice, face, colleague).is_some());
            assert!(
                desk.line(voice, face, "The client liked the second draft.")
                    .is_some()
            );
            assert!(
                desk.line(1, "f1", owner).is_some(),
                "voice {voice}, face {face:?}: two in a row end the carry"
            );
            assert_eq!(
                desk.line(1, "f1", "Lamp, will it rain tomorrow?"),
                None,
                "by the name"
            );
            desk.called_long_ago();
            desk.answered(12);
            assert_eq!(
                desk.line(1, "f1", owner),
                None,
                "voice {voice}, face {face:?}: carried from the new answer"
            );
        }
    }

    /// The first builder at 09:34 on #3296: the owner's five turns, each
    /// begun 12 s after the lamp's words, with one colleague line between
    /// turns 2 and 3, are all answered, the colleague's held. In a busy
    /// room the colleague's face is seen speaking it; in a quiet one it is
    /// a voice off camera, another than the conversation's, which with the
    /// carry standing does not make the room busy. Without the switch the
    /// same voice makes the quiet room busy, as on lamp-52e6 at 09:18.
    #[test]
    fn the_owners_five_turns_go_on_past_one_colleague_line() {
        let _one = crate::provider_worker::tests::one_thread();
        let turns = [
            "And what about tomorrow?",
            "Will it rain in the afternoon?",
            "Should I take an umbrella then?",
            "What time does the sun set?",
            "Can you remind me at five?",
        ];
        let colleague = "I sent the slides to the client this morning.";
        for (busy, face) in [(true, "f2"), (false, "")] {
            let mut desk = CarryDesk::new_in(true, busy);
            for (turn, words) in turns.iter().enumerate() {
                desk.answered(12);
                if turn == 2 {
                    assert!(
                        desk.line(-1, face, colleague).is_some(),
                        "busy {busy}: the colleague held"
                    );
                    let mut room = crate::room_told::room().lock().unwrap();
                    assert_eq!(
                        room.at(monotonic_us()) == crate::room_told::Room::Busy,
                        busy,
                        "busy {busy}: not made busy by the colleague's voice"
                    );
                }
                assert_eq!(
                    desk.line(1, "f1", words),
                    None,
                    "busy {busy}: turn {}",
                    turn + 1
                );
            }
        }
        // Reviewer-two at 09:52: quiet through the verdict and the line's
        // completion: its voice makes no second person (the worker's
        // other-voice branch, `RoomTold::judged`) and no talk
        // (`RoomTold::left_unanswered`); the owner then goes on.
        let mut desk = CarryDesk::new_in(true, false);
        desk.answered(12);
        assert!(desk.line(-1, "", colleague).is_some(), "held");
        {
            let mut room = crate::room_told::room().lock().unwrap();
            assert_eq!(
                room.left_unanswered(desk.request, monotonic_us()),
                crate::room_told::Left::Held,
                "no talk"
            );
            assert_ne!(room.at(monotonic_us()), crate::room_told::Room::Busy);
            assert!(!room.talked_past_the_lamp());
            assert_eq!(room.left_alone_since_spoke(), 0);
        }
        assert_eq!(desk.line(1, "f1", turns[0]), None, "the owner goes on");
        let mut off = CarryDesk::new_in(false, false);
        off.answered(3);
        assert!(off.line(-1, "", colleague).is_some(), "off: held");
        assert_eq!(
            crate::room_told::room().lock().unwrap().at(monotonic_us()),
            crate::room_told::Room::Busy,
            "off: the colleague's voice makes the room busy, as today"
        );
    }

    /// Reviewer-two at 09:52: the late path is under the same rule. A
    /// colleague's line at 3 s, no face, its print back only after its
    /// verdict, is let out by the ordinary follow-up (the legacy
    /// exception); its print, sure as another voice (the sure bar lowered
    /// to 1.5 s, a synthetic control), is known at the next line's verdict
    /// as one line from someone else: the owner goes on. Such a line and
    /// the colleague's next, seen speaking, are two in a row: the carry
    /// ends, and the owner past the 8 s is held.
    #[test]
    fn a_late_sure_other_voice_is_one_colleague_line() {
        let _one = crate::provider_worker::tests::one_thread();
        let owner = "Will it rain in the afternoon?";
        let late = |desk: &mut CarryDesk, words: &str| {
            desk.late.store(true, std::sync::atomic::Ordering::Relaxed);
            assert_eq!(desk.line(-1, "", words), None, "let out at {words:?}");
            desk.late.store(false, std::sync::atomic::Ordering::Relaxed);
        };
        let mut desk = CarryDesk::new(true);
        desk.heeding.sure_other_samples = crate::same_voice::RATE * 3 / 2;
        desk.answered(3);
        late(&mut desk, "I sent the slides to the client this morning.");
        desk.later(12);
        assert_eq!(
            desk.line(1, "f1", owner),
            None,
            "one line: the owner goes on"
        );
        desk.answered(3);
        late(&mut desk, "I sent the slides to the client this morning.");
        // Known at the verdict of the colleague's next line, seen speaking
        // it: two in a row.
        assert!(
            desk.line(0, "f2", "The client liked the second draft.")
                .is_some()
        );
        desk.later(12);
        assert!(desk.line(1, "f1", owner).is_some(), "two in a row");
    }

    /// Reviewer-three's P2 on #3331: owner and colleague lines alternate,
    /// the owner by voice alone. The colleague, whose voice reads 0.75
    /// against the owner's (over `same_voice::SAME_FROM`), is held once by
    /// their face; off camera their next line is not carried by that voice,
    /// which is no nearer the owner's than their own held line's, so it is
    /// held; the owner's own voice, nearer theirs by `CARRY_NEARER_BY`,
    /// still goes on each time.
    #[test]
    fn after_a_held_colleague_the_owners_voice_alone_must_be_nearer_theirs() {
        let _one = crate::provider_worker::tests::one_thread();
        let mut desk = CarryDesk::new(true);
        desk.answered(12);
        assert_eq!(desk.line(1, "", "Will it rain in the afternoon?"), None);
        desk.answered(12);
        assert!(
            desk.line(4, "f2", "The client liked the second draft.")
                .is_some()
        );
        for (turn, words) in [
            "Should I take an umbrella then?",
            "What time does the sun set?",
            "Can you remind me at five?",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(desk.line(1, "", words), None, "the owner's turn {turn}");
            desk.answered(12);
            assert!(
                desk.line(4, "", "I sent the slides to the client this morning.")
                    .is_some(),
                "the colleague off camera, turn {turn}: held"
            );
        }
    }

    /// A newer visitor's print cannot erase an earlier held visitor's
    /// constraint when that earlier visitor returns without a visible face.
    #[test]
    fn two_held_colleagues_keep_the_older_voice_constraint() {
        let _one = crate::provider_worker::tests::one_thread();
        for room in ["busy", "unknown", "quiet"] {
            let mut desk = CarryDesk::new(true);
            desk.answered(12);
            assert!(
                desk.line(4, "f2", "The client liked the second draft.")
                    .is_some()
            );
            assert_eq!(desk.line(1, "", "Will it rain in the afternoon?"), None);
            desk.answered(12);
            assert!(
                desk.line(-2, "f3", "I sent the slides to the client.")
                    .is_some()
            );
            assert_eq!(desk.line(1, "", "Should I take an umbrella then?"), None);
            desk.answered(12);
            {
                let mut told = crate::room_told::room().lock().unwrap();
                match room {
                    "unknown" => {
                        *told = crate::room_told::RoomTold::new();
                        told.count_only_talk(true);
                        told.count_held_talk(true);
                        told.end_busy_sooner(true);
                        told.end_busy_once_answered(true);
                    }
                    "quiet" => told.set_quiet(),
                    _ => {}
                }
            }
            assert!(
                desk.line(4, "", "The client approved the slides this morning.")
                    .is_some(),
                "the older colleague off camera is still held after another visitor: {room}"
            );
            let expected = match room {
                "unknown" => crate::room_told::Room::Unknown,
                "quiet" => crate::room_told::Room::Quiet,
                _ => crate::room_told::Room::Busy,
            };
            assert_eq!(
                crate::room_told::room().lock().unwrap().at(monotonic_us()),
                expected
            );
            assert_eq!(desk.line(1, "f1", "What time does the sun set?"), None);
        }
    }

    #[test]
    fn held_voice_constraints_close_with_the_carry_and_reject_old_callbacks() {
        let _one = crate::provider_worker::tests::one_thread();
        for end in ["stop", "privacy", "forget", "turned-away", "two-others"] {
            let mut desk = CarryDesk::new(true);
            desk.answered(12);
            assert!(
                desk.line(4, "f2", "The client liked the second draft.")
                    .is_some()
            );
            let old = desk.request;
            match end {
                "stop" => desk.heeding.lamp_did(crate::second_provider::Did::Stopped),
                "privacy" => desk.heeding.privacy_changed(),
                "forget" => {
                    crate::thread::THREAD.lock().unwrap().forget();
                    let now = monotonic_us();
                    desk.heeding
                        .signs_begun(old + 1, (now, now + 2_000_000), now);
                }
                "turned-away" => {
                    desk.away = true;
                    assert!(desk.line(1, "f1", "The meeting starts at five.").is_some());
                }
                _ => assert!(
                    desk.line(-2, "f3", "I sent the slides to the client.")
                        .is_some()
                ),
            }
            desk.heeding.held_colleagues.hold(old, Some(vec![1.0, 0.0]));
            assert!(
                !desk
                    .heeding
                    .held_colleagues
                    .nearer(old + 10, Some(1.0), |_| {
                        panic!("closed scope after {end} must not read a cached print")
                    }),
                "{end}"
            );
        }
    }

    #[test]
    fn a_full_held_voice_bound_keeps_the_owner_by_face_until_a_fresh_call() {
        let _one = crate::provider_worker::tests::one_thread();
        let mut desk = CarryDesk::new(true);
        for _ in 0..9 {
            desk.answered(12);
            assert!(
                desk.line(4, "f2", "The client liked the second draft.")
                    .is_some()
            );
            assert_eq!(desk.line(1, "f1", "Will it rain in the afternoon?"), None);
        }
        desk.answered(12);
        assert!(
            desk.line(1, "", "Should I take an umbrella then?")
                .is_some()
        );
        assert_eq!(desk.line(1, "f1", "What time does the sun set?"), None);
        desk.answered(12);
        assert_eq!(
            desk.line(1, "f1", "Lamp, what is on my calendar today?"),
            None
        );
        desk.called_long_ago();
        desk.answered(12);
        assert_eq!(desk.line(1, "", "And what about tomorrow afternoon?"), None);
    }

    #[test]
    fn a_recent_call_can_resume_after_forget_without_a_stale_closed_scope() {
        let _one = crate::provider_worker::tests::one_thread();
        for room in ["busy", "unknown", "quiet"] {
            let mut desk = CarryDesk::new_in(true, room == "busy");
            desk.answered(30);
            let old_answer_end = desk.heeding.said_until_us.unwrap();
            assert_eq!(
                desk.line(1, "f1", "Lamp, will it rain this afternoon?"),
                None
            );
            let old_call = desk.request;
            crate::thread::THREAD.lock().unwrap().forget();
            {
                let mut told = crate::room_told::room().lock().unwrap();
                let expected = match room {
                    "unknown" => {
                        *told = crate::room_told::RoomTold::new();
                        told.count_only_talk(true);
                        told.count_held_talk(true);
                        told.end_busy_sooner(true);
                        told.end_busy_once_answered(true);
                        crate::room_told::Room::Unknown
                    }
                    "quiet" => {
                        told.set_quiet();
                        crate::room_told::Room::Quiet
                    }
                    _ => {
                        let _ = told.two_people(monotonic_us());
                        crate::room_told::Room::Busy
                    }
                };
                assert_eq!(told.at(monotonic_us()), expected, "fixture room {room}");
            }
            assert_eq!(
                desk.line(1, "f1", "Will it rain in the afternoon?"),
                None,
                "the recent caller's faced line is legitimately admitted"
            );
            assert!(desk.heeding.by_name.is_none(), "no new explicit name call");
            desk.answered(12);
            assert!(
                desk.heeding.said_until_us.unwrap() > old_answer_end,
                "the new answer advances the max-kept physical answer clock"
            );
            desk.called_long_ago();
            // A prior-call callback cannot attach even an owner-identical
            // rejected print to the newly admitted scope.
            desk.heeding
                .held_colleagues
                .hold(old_call, Some(vec![1.0, 0.0]));
            assert_eq!(
                desk.line(1, "", "Should I take an umbrella then?"),
                None,
                "the actual post-forget answer reopens owner carry at +12 s: {room}"
            );
        }
    }

    #[test]
    fn a_fresh_call_clears_the_previous_callers_held_voice_constraints() {
        let _one = crate::provider_worker::tests::one_thread();
        let mut desk = CarryDesk::new(true);
        desk.answered(12);
        assert!(
            desk.line(4, "f2", "The client liked the second draft.")
                .is_some()
        );
        assert_eq!(desk.line(1, "", "Will it rain in the afternoon?"), None);
        desk.answered(12);
        assert_eq!(
            desk.line(4, "f2", "Lamp, what is on my calendar today?"),
            None
        );
        desk.called_long_ago();
        desk.answered(12);
        assert_eq!(
            desk.line(4, "", "And what about tomorrow afternoon?"),
            None,
            "the new caller's voice is not constrained by their earlier held line"
        );
    }

    /// The first builder at 09:34 on #3296: a colleague who calls the lamp
    /// by its name while the owner's carry stands is answered, and is the
    /// caller from then on: their own next line goes on by their face, and
    /// the owner's is someone else's, held.
    #[test]
    fn a_colleague_calling_the_lamp_becomes_the_caller() {
        let _one = crate::provider_worker::tests::one_thread();
        let mut desk = CarryDesk::new(true);
        desk.answered(12);
        assert_eq!(desk.line(1, "f1", "Will it rain in the afternoon?"), None);
        desk.answered(12);
        assert_eq!(
            desk.line(0, "f2", "Lamp, what is on my calendar today?"),
            None,
            "the colleague's call"
        );
        desk.called_long_ago();
        desk.answered(12);
        assert_eq!(
            desk.line(0, "f2", "And what about tomorrow afternoon?"),
            None,
            "the new caller goes on"
        );
        desk.answered(12);
        assert!(
            desk.line(0, "f1", "Should I take an umbrella then?")
                .is_some(),
            "the owner is someone else now"
        );
    }

    /// `LAMP_SAME_SPEAKER_CARRIES`: a line with no face seen speaking it and
    /// a print that says nothing sure, the colleague's own voice at 0.00
    /// from 1.5 s of speech among them, is held but ends nothing; after
    /// people talked past the lamp (two lines left unanswered as talk) the
    /// owner's own next line, by their face, still goes on.
    #[test]
    fn a_line_with_no_sure_sign_is_held_and_ends_no_carry() {
        let _one = crate::provider_worker::tests::one_thread();
        let mut desk = CarryDesk::new(true);
        desk.answered(12);
        {
            let mut room = crate::room_told::room().lock().unwrap();
            for other in 901..=902 {
                room.unanswered(other, monotonic_us());
            }
            assert!(room.talked_past_the_lamp());
        }
        // Statements: a question held would make the next line a reply to
        // it (`attention::AFTER_QUESTION`).
        assert!(
            desk.line(-1, "", "I sent the slides to the client this morning.")
                .is_some(),
            "a colleague's voice, unsure, no face: held"
        );
        assert!(
            desk.line(0, "", "I think it will rain later.").is_some(),
            "no print, no face: held"
        );
        desk.stale = true;
        assert!(
            desk.line(0, "f1", "Then I need my umbrella today.")
                .is_some(),
            "a stale face: held"
        );
        assert_eq!(
            desk.line(2, "f1", "Should I take an umbrella then?"),
            None,
            "the owner's face"
        );
    }

    /// Reviewer-two at 03:01 on Queue #3126: the same speaker tells who
    /// spoke, not to whom. The owner, in their own sure voice, seen speaking
    /// turned away from the lamp to a colleague within the 20 s, is held,
    /// and the carry ends (the first builder at 09:34 on #3296; reviewer-two
    /// at 10:32 on #3331): turned back they are held until a fresh call,
    /// and the carry goes on from its answer. In a room not busy (unknown or
    /// quiet) the line turned away is held too, and ends the carry.
    #[test]
    fn the_owner_turned_to_a_colleague_is_held_and_ends_the_carry() {
        let _one = crate::provider_worker::tests::one_thread();
        let away = "Send me the deck after lunch, please.";
        let mut desk = CarryDesk::new(true);
        desk.answered(12);
        desk.away = true;
        assert!(desk.line(1, "f1", away).is_some(), "turned away: held");
        assert!(
            desk.line(1, "f1", "And what about tomorrow?").is_some(),
            "turned back: the carry is over"
        );
        assert_eq!(
            desk.line(1, "f1", "Lamp, will it rain tomorrow?"),
            None,
            "a fresh call"
        );
        desk.called_long_ago();
        desk.answered(12);
        assert_eq!(
            desk.line(1, "f1", "And what about the weekend?"),
            None,
            "carried from the new answer"
        );
        for quiet in [false, true] {
            let mut desk = CarryDesk::new_in(true, false);
            if quiet {
                crate::room_told::room().lock().unwrap().set_quiet();
            }
            desk.answered(12);
            desk.away = true;
            assert!(desk.line(1, "f1", away).is_some(), "quiet {quiet}: held");
            assert!(!desk.heeding.carrying, "quiet {quiet}: the carry ended");
        }
    }

    /// Reviewer-two at 10:35 on #3331: in a room not busy, unknown or
    /// quiet, a colleague's line, their own face seen speaking it, in their
    /// voice, none, or the owner's, is held under the carry, as in a busy
    /// room; the room stays as it was, and the owner goes on.
    #[test]
    fn a_colleagues_face_is_held_in_a_room_not_busy() {
        let _one = crate::provider_worker::tests::one_thread();
        for quiet in [false, true] {
            for voice in [-1, 0, 1] {
                let mut desk = CarryDesk::new_in(true, false);
                let was = if quiet {
                    crate::room_told::room().lock().unwrap().set_quiet();
                    crate::room_told::Room::Quiet
                } else {
                    crate::room_told::Room::Unknown
                };
                desk.answered(12);
                assert!(
                    desk.line(voice, "f2", "The client liked the second draft.")
                        .is_some(),
                    "quiet {quiet}, voice {voice}: held"
                );
                {
                    let mut room = crate::room_told::room().lock().unwrap();
                    assert_eq!(
                        room.left_unanswered(desk.request, monotonic_us()),
                        crate::room_told::Left::Held,
                        "quiet {quiet}, voice {voice}: no talk"
                    );
                    assert_eq!(room.at(monotonic_us()), was, "quiet {quiet}, voice {voice}");
                }
                assert_eq!(
                    desk.line(1, "f1", "Will it rain in the afternoon?"),
                    None,
                    "quiet {quiet}, voice {voice}: the owner goes on"
                );
            }
        }
    }

    /// `LAMP_SAME_SPEAKER_CARRIES`: the carry lasts
    /// [`SAME_SPEAKER_CARRIES_FOR`] from the end of the lamp's words to the
    /// line's start (reviewer-two at 03:07): begun 19.99 s after them the
    /// owner goes on, 20.01 s after them they are held; and a line begun
    /// 19 s after them, judged 22 s after them, goes on: its start decides.
    #[test]
    fn the_carry_ends_twenty_seconds_after_the_lamps_words_by_the_lines_start() {
        let _one = crate::provider_worker::tests::one_thread();
        assert_eq!(SAME_SPEAKER_CARRIES_FOR, Duration::from_secs(20));
        let line = "And what about tomorrow?";
        let mut desk = CarryDesk::new(true);
        desk.answered_ms(19_990);
        assert_eq!(desk.line(1, "f1", line), None, "19.99 s");
        let mut desk = CarryDesk::new(true);
        desk.answered_ms(20_010);
        assert_eq!(
            desk.line(1, "f1", line),
            Some(crate::attention::BUSY_UNCALLED),
            "20.01 s"
        );
        let mut desk = CarryDesk::new(true);
        desk.answered(22);
        let began = monotonic_us() - 3_000_000;
        assert_eq!(desk.line_at(1, "", line, began), None, "begun at 19 s");
    }

    /// `LAMP_SAME_SPEAKER_CARRIES`: a stop, a privacy change or a
    /// forgetting ends the carry; the owner's next line, in their own voice
    /// and face 12 s after the lamp's words, is held.
    #[test]
    fn a_stop_a_privacy_change_or_a_forgetting_ends_the_carry() {
        let _one = crate::provider_worker::tests::one_thread();
        for end in ["stop", "privacy", "forget"] {
            let mut desk = CarryDesk::new(true);
            desk.answered(12);
            assert_eq!(desk.line(1, "f1", "And what about tomorrow?"), None);
            desk.answered(12);
            match end {
                "stop" => desk.heeding.lamp_did(crate::second_provider::Did::Stopped),
                "privacy" => desk.heeding.privacy_changed(),
                _ => crate::thread::THREAD.lock().unwrap().forget(),
            }
            assert!(
                desk.line(1, "f1", "Will it rain in the afternoon?")
                    .is_some(),
                "after a {end}"
            );
        }
    }

    /// Reviewer-two at 03:07: the late path. A line carried by the owner's
    /// face whose print, at 0.00 from 1.5 s of speech, comes only after its
    /// verdict is unsure: it ends no conversation at the next line, which
    /// goes on; without the switch the same late print is another voice's,
    /// and the next line is held even inside the 8 s.
    #[test]
    fn a_late_unsure_print_of_a_carried_line_ends_nothing() {
        let _one = crate::provider_worker::tests::one_thread();
        let mut desk = CarryDesk::new(true);
        desk.answered(12);
        desk.late.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(desk.line(-1, "f1", "And what about tomorrow?"), None);
        desk.late.store(false, std::sync::atomic::Ordering::Relaxed);
        desk.answered(12);
        assert_eq!(
            desk.line(1, "f1", "Will it rain in the afternoon?"),
            None,
            "with the switch"
        );
        // Without it: a call, a follow-up in the 8 s whose print comes late
        // as another's, and the next line in the 8 s.
        let mut off = CarryDesk::new(false);
        off.answered(3);
        off.late.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(off.line(-1, "f1", "And what about tomorrow?"), None);
        off.late.store(false, std::sync::atomic::Ordering::Relaxed);
        off.answered(3);
        assert!(
            off.line(1, "f1", "Will it rain in the afternoon?")
                .is_some(),
            "without the switch"
        );
    }

    /// The first builder at 03:47 on Queue #3126, the owner's line on
    /// lamp-4ace (request 1888, begun about 7 s after the answer to 1887,
    /// within a follow-up's 8 s, held as "another voice (cosine 0.27)" from
    /// a 1.80 s print): with `LAMP_SAME_SPEAKER_CARRIES`, the owner's face
    /// alone, speaking turned to the lamp, with that short print carries the
    /// line, its print back at the verdict or only after it; the next line
    /// goes on too. Without the switch, the immediate print holds it.
    #[test]
    fn the_owners_line_seven_seconds_after_an_answer_with_a_short_print_is_carried() {
        let _one = crate::provider_worker::tests::one_thread();
        for late in [false, true] {
            let mut desk = CarryDesk::new(true);
            desk.answered(7);
            desk.late.store(late, std::sync::atomic::Ordering::Relaxed);
            assert_eq!(
                desk.line(2, "f1", "Will it rain in the afternoon?"),
                None,
                "late {late}"
            );
            desk.late.store(false, std::sync::atomic::Ordering::Relaxed);
            desk.answered(7);
            assert_eq!(
                desk.line(2, "f1", "Should I take an umbrella then?"),
                None,
                "late {late}: the next line"
            );
        }
        let mut off = CarryDesk::new(false);
        off.answered(7);
        assert!(
            off.line(2, "f1", "Will it rain in the afternoon?")
                .is_some(),
            "without the switch, as on lamp-4ace"
        );
    }

    /// Reviewer-two's second P2 at 03:46 on #3296: within a follow-up's 8 s
    /// a line's signs are read as past them. After the owner's answer, a
    /// colleague at 3 s, their own face seen speaking, is held whatever
    /// their print; one such line ends nothing (the first builder at 09:34,
    /// replacing 03:46's contract), so the owner back at 12 s by face and
    /// voice, with no new answer, goes on; two in a row end the carry, and
    /// the owner is held until a fresh call. The controls at 3 s: a
    /// colleague's short print with no face is no sign either way, so the
    /// ordinary follow-up rule lets it go on (the legacy exception) and it
    /// ends nothing, so the owner at 12 s goes on; the owner's own voice
    /// with no face goes on as a follow-up.
    #[test]
    fn a_colleague_within_the_eight_seconds_is_held_and_two_end_the_carry() {
        let _one = crate::provider_worker::tests::one_thread();
        let owner = "Will it rain in the afternoon?";
        let colleague = "The client liked the second draft.";
        for voice in [-1, 0, 1] {
            let mut desk = CarryDesk::new(true);
            desk.answered(3);
            assert!(
                desk.line(voice, "f2", colleague).is_some(),
                "voice {voice}, face f2 at 3 s: held"
            );
            desk.later(12);
            assert_eq!(
                desk.line(1, "f1", owner),
                None,
                "voice {voice}: the owner at 12 s, the carry standing"
            );
            desk.answered(3);
            assert!(desk.line(voice, "f2", colleague).is_some());
            assert!(
                desk.line(voice, "f2", "I sent them this morning.")
                    .is_some()
            );
            desk.later(12);
            assert!(
                desk.line(1, "f1", owner).is_some(),
                "voice {voice}: the owner at 12 s, the carry over"
            );
            assert_eq!(
                desk.line(1, "f1", "Lamp, will it rain tomorrow?"),
                None,
                "voice {voice}: a fresh call"
            );
        }
        let mut desk = CarryDesk::new(true);
        desk.answered(3);
        assert_eq!(
            desk.line(-1, "", "I sent the slides to the client this morning."),
            None,
            "a short print, no face, at 3 s: the ordinary follow-up"
        );
        desk.later(12);
        assert_eq!(desk.line(1, "f1", owner), None, "the carry stands");
        let mut desk = CarryDesk::new(true);
        desk.answered(3);
        assert_eq!(desk.line(1, "", owner), None, "the owner's voice at 3 s");
    }

    /// Reviewer-two at 04:11 on #3296: under a carry, a line with no face
    /// and no sure sign of who said it, its print short and under 0.30
    /// (0.27 or 0.0), short and between 0.30 and 0.70, or never back, is
    /// judged as if no print had come, at once or late alike: within the
    /// 8 s the ordinary follow-up rule lets it go on (the legacy
    /// exception); past them it is held. Either way the carry stands, so
    /// the owner's next line, by face and voice, goes on.
    #[test]
    fn an_unsure_print_with_no_face_decides_nothing_whenever_it_comes() {
        let _one = crate::provider_worker::tests::one_thread();
        // A statement: a question held would make the owner's next line a
        // reply to it (`attention::AFTER_QUESTION`).
        let words = "I think it will rain in the afternoon.";
        for (gap, admitted) in [(3, true), (12, false)] {
            for voice in [2, -1, 3, 0] {
                for late in [false, true] {
                    let mut desk = CarryDesk::new(true);
                    desk.answered(gap);
                    desk.late.store(late, std::sync::atomic::Ordering::Relaxed);
                    let held = desk.line(voice, "", words);
                    desk.late.store(false, std::sync::atomic::Ordering::Relaxed);
                    assert_eq!(
                        held.is_none(),
                        admitted,
                        "voice {voice} late {late} at {gap} s: {held:?}"
                    );
                    assert_eq!(
                        desk.line(1, "f1", "Should I take an umbrella then?"),
                        None,
                        "voice {voice} late {late} at {gap} s: the carry stands"
                    );
                }
            }
        }
        // Without the switch the immediate short print is held, as today.
        let mut off = CarryDesk::new(false);
        off.answered(3);
        assert!(off.line(2, "", words).is_some(), "without the switch");
    }

    /// Reviewer-two's P2 at 04:31 on #3296, and the first builder's contract
    /// at 04:33: the first 8 s are not identity-gated, and a print that was
    /// no evidence never becomes evidence. The owner, A, is answered; an
    /// unknown B with no face and a short 0.27 print at 3 s goes on as the
    /// ordinary follow-up, its print back at the verdict or after it; the
    /// same B with no face at 12 s is held, since B's print was not kept as
    /// the conversation's voice to match itself. Beside it the identified A,
    /// face f1, at 12 s is carried. Caller-only on and off.
    #[test]
    fn an_unknown_voice_let_out_in_the_eight_seconds_is_no_identity_past_them() {
        let _one = crate::provider_worker::tests::one_thread();
        for caller_only in [false, true] {
            for late in [false, true] {
                let case = format!("caller-only {caller_only}, late {late}");
                let mut desk = CarryDesk::new(true);
                if caller_only {
                    desk.caller_only();
                }
                desk.answered(3);
                desk.late.store(late, std::sync::atomic::Ordering::Relaxed);
                assert_eq!(
                    desk.line(2, "", "I think it will rain in the afternoon."),
                    None,
                    "{case}: B at 3 s, the ordinary follow-up"
                );
                desk.late.store(false, std::sync::atomic::Ordering::Relaxed);
                desk.later(12);
                assert!(
                    desk.line(2, "", "I think I will take an umbrella then.")
                        .is_some(),
                    "{case}: the same B at 12 s, no face"
                );
                assert_eq!(
                    desk.line(1, "f1", "Should I take an umbrella then?"),
                    None,
                    "{case}: the identified A at 12 s"
                );
            }
        }
    }

    /// The first builder's contract at 04:33 on #3296, rule 3: a sure other
    /// voice is held inside the 8 s too. A synthetic classification
    /// control: the sure bar is lowered to the 1.5 s of speech today's
    /// prints carry, since none reaches the provisional 2.5 s
    /// (`CARRY_SURE_OTHER_SPEECH_SAMPLES`), so this is no reachable acoustic
    /// case. At the real bar the same colleague's print, no face, at 3 s is
    /// no evidence, and the ordinary follow-up lets it go on.
    #[test]
    fn a_sure_other_voice_within_the_eight_seconds_is_held() {
        let _one = crate::provider_worker::tests::one_thread();
        let words = "I sent the slides to the client this morning.";
        for (least, admitted) in [
            (CARRY_SURE_OTHER_SPEECH_SAMPLES, true),
            (crate::same_voice::RATE * 3 / 2, false),
        ] {
            let mut desk = CarryDesk::new(true);
            desk.heeding.sure_other_samples = least;
            desk.answered(3);
            let held = desk.line(-1, "", words);
            assert_eq!(held.is_none(), admitted, "sure from {least}: {held:?}");
        }
    }

    /// The reviewer's P3 on #2428: with `LAMP_QUIET_GOING_ON_SAME_ONE` and no
    /// `LAMP_SPOKEN_TO`, a low print is read for `someone_else` alone; #829's
    /// conversation check, never run there, does not end the conversation
    /// for it, so a person's own short follow-up that reads low still goes
    /// on. With `LAMP_SPOKEN_TO` it ends it, as before.
    #[test]
    fn a_low_print_without_spoken_to_keeps_the_conversation_going() {
        let _one = crate::provider_worker::tests::one_thread();
        use crate::second_provider::Heard;
        /// Prints by a line's first sample, answered at once: 1 is A's
        /// voice, -1 B's.
        #[derive(Default)]
        struct Voice(std::collections::VecDeque<(u64, Result<Vec<f32>, String>)>);
        impl crate::same_voice::Printer for Voice {
            fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
                let print = if samples[0] > 0 {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                };
                self.0.push_back((request, Ok(print)));
                true
            }
            fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
                self.0.pop_front()
            }
        }
        let said = (10_000_000, 11_000_000);
        let hear = |heeding: &mut Heeding, request: u64, voice: i16| {
            heeding.hear(
                request,
                Heard::Began {
                    over_the_lamps_voice: false,
                },
            );
            heeding.hear(request, Heard::Samples(&[voice; 48_000]));
            heeding.hear(request, Heard::Ended);
        };
        for spoken_to in [false, true] {
            *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
            let mut heeding = Heeding::new(true, true, None, None);
            heeding.as_the_only_voice(false, spoken_to, false);
            heeding.going_on_quiet_only_for_the_same_one();
            heeding.checking_the_voice(Box::new(Voice::default()));
            // A calls the lamp: A's voice is the conversation's.
            hear(&mut heeding, 1, 1);
            let call = "Lamp, what time is it?";
            assert_eq!(heeding.heed(1, said, call, None), None);
            // A line going on from it reads low.
            hear(&mut heeding, 2, -1);
            let (going_on, other, _, likeness) = heeding.voice_going_on(2, true);
            assert!(
                likeness.is_some_and(|likeness| likeness < crate::same_voice::DIFFERENT_BELOW),
                "spoken_to {spoken_to}: {likeness:?}"
            );
            assert_eq!(going_on, !spoken_to, "spoken_to {spoken_to}");
            assert_eq!(other, spoken_to, "spoken_to {spoken_to}");
        }
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    }

    /// lamp-52e6's re-run at 09:5x on #2428: with `LAMP_GOING_ON_NOT_OWN`
    /// and `LAMP_QUIET_GOING_ON_SAME_ONE`, no `LAMP_SPOKEN_TO`, another
    /// voice's 16 words (cosine 0.23) in a session past its first lines were
    /// played as "the session has heard enough to be trusted". With either
    /// switch that line is held, the caller's own lines in the same trusted
    /// session are still answered, and the other voice's line begun over the
    /// lamp's sound, which has no print, is played as before, as is its line
    /// that calls the lamp by name or answers the lamp's question.
    #[test]
    fn a_trusted_session_does_not_carry_to_another_voice() {
        let _one = crate::provider_worker::tests::one_thread();
        use crate::room_told::Room;
        use crate::second_provider::Heard;
        /// Prints by a line's first sample, answered at once: 1 is the
        /// caller's voice, -1 another's.
        #[derive(Default)]
        struct Voice(std::collections::VecDeque<(u64, Result<Vec<f32>, String>)>);
        impl crate::same_voice::Printer for Voice {
            fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
                let print = if samples[0] > 0 {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                };
                self.0.push_back((request, Ok(print)));
                true
            }
            fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
                self.0.pop_front()
            }
        }
        let said = (10_000_000, 11_000_000);
        let quiet = |going_on, lamp_asked| Signs {
            room: Room::Quiet,
            going_on,
            lamp_asked,
            turned: Some(false),
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let line = |heeding: &mut Heeding,
                    request: u64,
                    voice: i16,
                    over_the_lamps_voice: bool,
                    words: &str,
                    going_on: bool,
                    lamp_asked: bool| {
            heeding.hear(
                request,
                Heard::Began {
                    over_the_lamps_voice,
                },
            );
            heeding.hear(request, Heard::Samples(&[voice; 48_000]));
            heeding.hear(request, Heard::Ended);
            heeding.judge(request, said, words, quiet(going_on, lamp_asked))
        };
        let other = "Honestly I think we should book the train tickets tonight before the prices go up again.";
        assert_eq!(other.split_whitespace().count(), 16);
        // `switch`: 0 none, 1 `LAMP_QUIET_GOING_ON_SAME_ONE`, 2
        // `LAMP_GOING_ON_NOT_OWN`. The other voice's line (its words, and
        // whether it answers the lamp's question), then the caller's.
        let scene = |switch: u8, over: bool, words: &str, lamp_asked: bool| {
            *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
            let mut heeding = Heeding::new(false, true, None, None);
            heeding.as_the_only_voice(false, false, false);
            match switch {
                1 => heeding.going_on_quiet_only_for_the_same_one(),
                2 => heeding.not_counting_own_answers(),
                _ => {}
            }
            heeding.checking_the_voice(Box::new(Voice::default()));
            let call = "Hey Lamp, what's the weather today?";
            assert_eq!(line(&mut heeding, 1, 1, false, call, false, false), None);
            let follow = "And what about tomorrow?";
            assert_eq!(line(&mut heeding, 2, 1, false, follow, true, false), None);
            // Past its first two lines the session is trusted: the
            // caller's own line, going on or not, is answered.
            let trusted = "Is it going to rain on the weekend?";
            assert_eq!(line(&mut heeding, 3, 1, false, trusted, false, false), None);
            // An answer to the lamp's question comes while the conversation
            // is going on.
            let held = line(&mut heeding, 4, -1, over, words, lamp_asked, lamp_asked);
            let after = "What about Sunday morning?";
            (held, line(&mut heeding, 5, 1, false, after, false, false))
        };
        let not_carried = Some(crate::second_provider::TRUSTED_NOT_CARRIED);
        // The caller's own line after another voice's is answered.
        assert_eq!(
            scene(0, false, other, false),
            (None, None),
            "without a switch, as before"
        );
        assert_eq!(scene(1, false, other, false), (not_carried, None));
        assert_eq!(scene(2, false, other, false), (not_carried, None));
        assert_eq!(
            scene(1, true, other, false),
            (None, None),
            "begun over the lamp: no print"
        );
        // Anyone may call the lamp, or answer its question.
        let named = "Hey Lamp, can you book the train tickets for tonight before the prices go up?";
        let answer = "Yes, the one at seven, before the prices go up again tonight please.";
        for switch in [1, 2] {
            // The named line begins the other voice's own conversation, so
            // the first caller's line after it, with no sign, is held.
            assert_eq!(
                scene(switch, false, named, false),
                (None, not_carried),
                "switch {switch}: named"
            );
            assert_eq!(
                scene(switch, false, answer, true),
                (None, None),
                "switch {switch}: answers the lamp"
            );
        }
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    }

    /// #2191 (lamp-52e6, 20 Oct 15:2x): in a busy room, after the lamp
    /// answered one named line, office talk between two others that came
    /// within the going-on window was answered, "a conversation with the
    /// lamp is going on". With `LAMP_GOING_ON_SAME_ONE=on` such a line is
    /// held; a reply to the lamp's question, a face turned to it, and a
    /// quiet room are as before.
    #[test]
    fn going_on_holds_only_for_the_one_the_lamp_answered() {
        use crate::room_told::Room;
        let signs = |room, going_on, lamp_asked, turned| Signs {
            room,
            going_on,
            lamp_asked,
            turned: Some(turned),
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        // The window as the route reads it, 5 s after the lamp's words.
        let window = super::went_on_told(
            2,
            signs(Room::Busy, true, false, false),
            Some(Duration::from_secs(5)),
            None,
            None,
            true,
        );
        assert!(window.going_on, "inside the busy room's 15 s");
        let said = (10_000_000, 11_000_000);
        // The named line answered, then `line` going on from it.
        let judge = |same_one: bool, line: &str, line_signs: Signs| {
            let mut heeding = Heeding::new(true, true, None, None);
            heeding.as_the_only_voice(false, true, false);
            if same_one {
                heeding.going_on_only_for_the_same_one();
            }
            let named = signs(line_signs.room, false, false, false);
            assert_eq!(
                heeding.judge(1, said, "Lamp, what's the weather today?", named),
                None,
                "the named line is answered"
            );
            heeding.judge(2, said, line, line_signs)
        };
        let office = "I'll send the quarterly numbers to finance after lunch.";
        let asks = "Did you see the budget numbers?";
        // Off: played, as today.
        assert_eq!(
            judge(false, office, signs(Room::Busy, true, false, false)),
            None
        );
        // On: held, a question too.
        for line in [office, asks] {
            assert_eq!(
                judge(true, line, signs(Room::Busy, true, false, false)),
                Some(crate::going_on_same_one::HELD),
                "{line}"
            );
        }
        // On: a reply to the lamp's own question goes on.
        assert_eq!(
            judge(
                true,
                "The blue one, please.",
                signs(Room::Busy, true, true, false)
            ),
            None
        );
        // On: a face turned to the lamp lets a question out, as without it.
        let turned = "And what about tomorrow?";
        assert_eq!(
            judge(true, turned, signs(Room::Busy, true, false, true)),
            None
        );
        // On: a quiet room is as without the switch.
        assert_eq!(
            judge(true, office, signs(Room::Quiet, true, false, false)),
            judge(false, office, signs(Room::Quiet, true, false, false)),
        );
        assert_eq!(
            judge(true, office, signs(Room::Quiet, true, false, false)),
            None
        );
    }

    /// lamp-52e6 at 08:33 on #2341: `LAMP_QUIET_GOING_ON_SAME_ONE=on` and
    /// `LAMP_SAME_VOICE_MODEL`, no `LAMP_SPOKEN_TO`, and no print all run:
    /// the child started only with `LAMP_SPOKEN_TO`, so `someone_else` never
    /// had a voice to read. Either rule that reads it starts it now; with
    /// none of the three, nothing is started.
    #[test]
    fn same_voice_starts_for_the_rules_that_read_it() {
        struct Voice;
        impl crate::same_voice::Printer for Voice {
            fn ask(&mut self, _request: u64, _samples: &[i16]) -> bool {
                true
            }
            fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
                None
            }
        }
        let starts = |spoken_to: bool, quiet_same_one: bool, not_own: bool| {
            let heeding = Mutex::new(Heeding::new(true, true, None, None));
            let mut started = false;
            let hears = super::checking_the_voice_for(
                &heeding,
                Some(PathBuf::from("/models/campplus.onnx")),
                spoken_to || quiet_same_one || not_own,
                |_| {
                    started = true;
                    Ok(Box::new(Voice) as Box<dyn crate::same_voice::Printer>)
                },
            );
            assert_eq!(heeding.lock().unwrap().voices.is_some(), started);
            assert_eq!(hears, started);
            started
        };
        assert!(starts(false, true, false));
        assert!(starts(false, false, true));
        assert!(starts(true, false, false));
        assert!(!starts(false, false, false));
    }

    /// #2191 on a lamp without `LAMP_SPOKEN_TO` (lamp-52e6 and lamp-4ace):
    /// the hold read a mark only `LAMP_SPOKEN_TO` sets, and never ran.
    #[test]
    fn going_on_same_one_holds_without_spoken_to() {
        use crate::room_told::Room;
        let signs = |going_on| Signs {
            room: Room::Busy,
            going_on,
            lamp_asked: false,
            turned: Some(false),
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let said = (10_000_000, 11_000_000);
        let judge = |same_one: bool| {
            let mut heeding = Heeding::new(true, true, None, None);
            heeding.as_the_only_voice(false, false, false);
            if same_one {
                heeding.going_on_only_for_the_same_one();
            }
            heeding.judge(1, said, "Lamp, what's the weather today?", signs(false));
            heeding.judge(
                2,
                said,
                "I'll send the quarterly numbers to finance after lunch.",
                signs(true),
            )
        };
        assert_eq!(judge(false), None, "off: played, as today");
        assert_eq!(
            judge(true),
            Some(crate::going_on_same_one::HELD),
            "on: held"
        );
    }

    /// #2191's exception: with the switch on in a busy room, a line going on
    /// from the lamp's answer that the same-voice check finds in the
    /// conversation's voice is played; one it finds in another voice is held.
    #[test]
    fn going_on_same_one_plays_the_conversations_voice_and_holds_another() {
        use crate::room_told::Room;
        use crate::second_provider::Heard;
        /// Prints by a line's first sample: 1 is A's voice, -1 B's.
        #[derive(Default)]
        struct Voice(std::collections::VecDeque<(u64, Result<Vec<f32>, String>)>);
        impl crate::same_voice::Printer for Voice {
            fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
                let print = if samples[0] > 0 {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                };
                self.0.push_back((request, Ok(print)));
                true
            }
            fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
                self.0.pop_front()
            }
        }
        let signs = |going_on| Signs {
            room: Room::Busy,
            going_on,
            lamp_asked: false,
            turned: Some(false),
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let said = (10_000_000, 11_000_000);
        let office = "I'll send the quarterly numbers to finance after lunch.";
        // A names the lamp and is answered, then `voice` says `office`.
        let judge = |voice: i16| {
            let mut heeding = Heeding::new(true, true, None, None);
            heeding.as_the_only_voice(false, true, false);
            heeding.going_on_only_for_the_same_one();
            heeding.checking_the_voice(Box::new(Voice::default()));
            for (request, voice) in [(1, 1), (2, voice)] {
                heeding.hear(
                    request,
                    Heard::Began {
                        over_the_lamps_voice: false,
                    },
                );
                heeding.hear(request, Heard::Samples(&[voice; 48_000]));
                heeding.hear(request, Heard::Ended);
            }
            assert_eq!(
                heeding.judge(1, said, "Lamp, what's the weather today?", signs(false)),
                None,
                "the named line is answered"
            );
            heeding.judge(2, said, office, signs(true))
        };
        assert_eq!(judge(1), None, "the conversation's voice goes on");
        assert!(judge(-1).is_some(), "another voice is held");
    }

    /// The Reviewer's P2 on #2650: a new caller let in by a face turned to
    /// the lamp is a call, for the conversation's voice as for the caller's
    /// face. A names the lamp; B, turned to it, asks; B goes on unturned. B's
    /// voice is the conversation's now, with the caller's voice only or
    /// without it, so B's follow-up goes on in a quiet room.
    #[test]
    fn a_caller_let_in_by_a_turned_face_takes_the_conversations_voice() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        use crate::room_told::Room;
        use crate::second_provider::Heard;
        /// Prints by a line's first sample, answered at once: 1 is A's
        /// voice, -1 B's.
        #[derive(Default)]
        struct Voice(std::collections::VecDeque<(u64, Result<Vec<f32>, String>)>);
        impl crate::same_voice::Printer for Voice {
            fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
                let print = if samples[0] > 0 {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                };
                self.0.push_back((request, Ok(print)));
                true
            }
            fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
                self.0.pop_front()
            }
        }
        let signs = |going_on, turned| Signs {
            room: Room::Quiet,
            going_on,
            lamp_asked: false,
            turned: Some(turned),
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let said = (10_000_000, 11_000_000);
        for caller_only in [false, true] {
            *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
            let mut heeding = Heeding::new(true, true, None, None);
            // `LAMP_SPOKEN_TO` off, as on lamp-52e6: the print is only read.
            heeding.as_the_only_voice(false, false, false);
            heeding.going_on_quiet_only_for_the_same_one();
            heeding.checking_the_voice(Box::new(Voice::default()));
            heeding.voices = Some(
                crate::same_voice::Voices::new(
                    Box::new(Voice::default()) as Box<dyn crate::same_voice::Printer>
                )
                .callers_only(caller_only),
            );
            for (request, voice) in [(1, 1), (2, -1), (3, -1)] {
                heeding.hear(
                    request,
                    Heard::Began {
                        over_the_lamps_voice: false,
                    },
                );
                heeding.hear(request, Heard::Samples(&[voice; 48_000]));
                heeding.hear(request, Heard::Ended);
            }
            assert_eq!(
                heeding.judge(
                    1,
                    said,
                    "Lamp, what's the weather today?",
                    signs(false, false)
                ),
                None,
                "A's named call"
            );
            assert_eq!(
                heeding.judge(2, said, "What time is it there?", signs(true, true)),
                None,
                "B's call, turned to the lamp"
            );
            assert_eq!(
                heeding.judge(3, said, "And what about tomorrow?", signs(true, false)),
                None,
                "B's follow-up goes on (caller-only {caller_only})"
            );
        }
    }

    /// #1712, the first builder at 17:26: a call the lamp never answered
    /// carries the person's next lines for 15 s, not the lamp's own 45 s: a
    /// three-word line later, in a trusted session, is held.
    #[test]
    fn a_call_unanswered_carries_the_next_lines_for_fifteen_seconds() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
        crate::room_told::room()
            .lock()
            .unwrap()
            .count_only_talk(true);
        let said = (10_000_000, 11_000_000);
        let mut heeding = Heeding::new(true, true, None, None);
        heeding.as_the_only_voice(false, true, false);
        assert_eq!(
            heeding.heed(207, said, "Lamp, what time is it?", None),
            None
        );
        assert_eq!(heeding.heed(208, said, "And the weather?", None), None);
        // Trusted now, past the session's first two.
        heeding.called_at = Some(Instant::now() - CALL_CARRIES_FOR - Duration::from_secs(1));
        assert!(heeding.heed(209, said, "Okay yeah sure.", None).is_some());
        heeding.called_at = Some(Instant::now() - Duration::from_secs(10));
        assert_eq!(heeding.heed(210, said, "Okay yeah sure.", None), None);
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    }

    /// The first builder's 17:2x task on Queue #1722 (lamp-4ace, the 16:34
    /// played test): after one question to the lamp, two people's own talk
    /// was answered line after line, "only as the conversation going on",
    /// each answer opening the window for the next. With
    /// `LAMP_GOING_ON_NOT_OWN=on` the lamp's answers to such lines, seen
    /// spoken by another face than the caller's, renew nothing: the line inside the window of the answer to the question is
    /// judged as before, the ones after it are held, in a quiet room and in
    /// a busy one, and after a name heard only in a busy room's sound (the
    /// first builder's 18:20 case); a reply to the lamp's own question still
    /// goes on. With nothing to tell the speakers apart, one person's
    /// follow-ups all go on (the first builder's 01:02).
    #[test]
    fn the_lamps_own_answers_to_talk_keep_no_conversation_going() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        let _clock = clock_with_history();
        let s = Duration::from_secs;
        // Lines at a virtual second, each answered a second later if let
        // out: `answer` says what the lamp answers it. A line with nothing
        // heard is the lamp's own (a reminder), said at its second.
        // `named`: the first line's name heard only in the room's sound, at
        // that score, close to the lamp, its words not calling it.
        // `faces`: the face seen speaking each line, by the eyes; none, no
        // eyes. `voices`: each line's voice for the same-voice check (1 one
        // person, -1 another), its print known only after its verdict.
        /// Prints by a line's first sample, each given only once released.
        struct Late(
            Arc<Mutex<Vec<u64>>>,
            std::collections::VecDeque<(u64, Result<Vec<f32>, String>)>,
        );
        impl crate::same_voice::Printer for Late {
            fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
                let print = if samples[0] > 0 {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                };
                self.1.push_back((request, Ok(print)));
                true
            }
            fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
                let released = self.0.lock().unwrap();
                let at = self
                    .1
                    .iter()
                    .position(|(request, _)| released.contains(request))?;
                self.1.remove(at)
            }
        }
        let run = |not_own: bool,
                   busy: bool,
                   named: Option<f32>,
                   faces: Option<&[&str]>,
                   voices: Option<&[i16]>,
                   lines: &[(u64, &str, &str)]|
         -> Vec<bool> {
            use lamp_msgs::{Face, Faces, Speaking, SpeakingState};
            // ">f2": f2 turned to the lamp.
            let speaking = |id: &str| Faces {
                faces: vec![Face {
                    id: id.trim_start_matches('>').to_owned(),
                    xmin: 0.2,
                    ymin: 0.2,
                    xmax: 0.4,
                    ymax: 0.5,
                    confidence: 1.0,
                    yaw: None,
                    facing: id.starts_with('>').then_some(true),
                    speaking: Some(Speaking {
                        state: SpeakingState::Yes,
                        score: 0.9,
                        from: None,
                    }),
                }],
            };
            let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
            let (eyes_at, eyes_at_us) = (Instant::now(), monotonic_us());
            // The lamp's words for `request`, played to their end at virtual
            // second `end`, told at virtual second `now` (`Did::Said`), so
            // both the thread's window and the words' end are exercised.
            let words_end = |heeding: &mut Heeding, request: u64, end: u64, now: u64| {
                let end_us = (monotonic_us() + end * 1_000_000).saturating_sub(now * 1_000_000);
                heeding.lamp_did(crate::second_provider::Did::Said(request, end_us));
            };
            *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
            crate::room_told::room()
                .lock()
                .unwrap()
                .count_only_talk(true);
            let mut heeding = Heeding::new(true, true, None, None);
            heeding.as_the_only_voice(false, true, false);
            if not_own {
                heeding.not_counting_own_answers();
            }
            if faces.is_some() {
                heeding.seeing_speaking_faces(eyes.clone(), None);
            }
            let released = Arc::new(Mutex::new(Vec::new()));
            if voices.is_some() {
                heeding.checking_the_voice(Box::new(Late(
                    released.clone(),
                    std::collections::VecDeque::new(),
                )));
            }
            let mut answered: Vec<(u64, &str, &str, u64)> = Vec::new();
            let mut was = 0;
            let mut played = Vec::new();
            for (request, (at, heard, answer)) in (1..).zip(lines) {
                if heard.is_empty() {
                    answered.push((request, heard, answer, *at));
                    words_end(&mut heeding, request, at + 1, was);
                    continue;
                }
                // Time passes: the thread's answers, the call and the words
                // kept are that much older.
                let passed = s(at - was);
                was = *at;
                let older = |until: Option<u64>| {
                    until.map(|until| until.saturating_sub(passed.as_micros() as u64))
                };
                heeding.said_until_us = older(heeding.said_until_us);
                heeding.said_until_free_us = older(heeding.said_until_free_us);
                // Each line's own sound, a second long, at its second; with
                // faces, on the eyes' clock, 3 s apart, its face speaking in
                // four looks through it.
                let said = match faces {
                    Some(faces) => {
                        // "": no face seen speaking it.
                        let face = faces[request as usize - 1];
                        for look in (0..4u64).filter(|_| !face.is_empty()) {
                            eyes.look(
                                eyes_at + Duration::from_millis(request * 3_000 + 200 + look * 200),
                                Some((("b", request * 10 + look), &speaking(face))),
                            );
                        }
                        let began = eyes_at_us + request * 3_000_000;
                        (began, began + 1_000_000)
                    }
                    None => (10_000_000 + at * 1_000_000, 11_000_000 + at * 1_000_000),
                };
                let now = Instant::now();
                let mut thread = crate::thread::Thread::new();
                for (asked, person, lamp, at_s) in &answered {
                    let then = now - s(at - at_s);
                    if !person.is_empty() {
                        thread.person(person, true, then);
                    }
                    thread.lamp(*asked, lamp, then);
                }
                *crate::thread::THREAD.lock().unwrap() = thread;
                heeding.called_at = heeding.called_at.map(|at| at - passed);
                // Three lines left alone before the question: a busy room.
                if busy && request == 1 {
                    let mut room = crate::room_told::room().lock().unwrap();
                    for other in 901..=903 {
                        room.unanswered(other, monotonic_us());
                    }
                    assert_eq!(room.at(monotonic_us()), crate::room_told::Room::Busy);
                }
                if let Some(voices) = voices {
                    use crate::second_provider::Heard;
                    let voice = voices[request as usize - 1];
                    heeding.hear(
                        request,
                        Heard::Began {
                            over_the_lamps_voice: false,
                        },
                    );
                    heeding.hear(request, Heard::Samples(&[voice; 48_000]));
                    heeding.hear(request, Heard::Ended);
                }
                let held = match named.filter(|_| request == 1) {
                    Some(score) => {
                        let mut signs = heeding.signs(request, said);
                        signs.names = vec![(said.0 + 100_000, score)];
                        signs.near = true;
                        heeding.judge(request, said, heard, signs)
                    }
                    None => heeding.heed(request, said, heard, None),
                };
                if held.is_none() {
                    answered.push((request, heard, answer, at + 1));
                    words_end(&mut heeding, request, at + 2, was);
                    let mut room = crate::room_told::room().lock().unwrap();
                    room.spoke(request, monotonic_us());
                }
                played.push(held.is_none());
                released.lock().unwrap().push(request);
            }
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
            played
        };
        // A question to the lamp, then two people talking to each other: no
        // name, no face, no reply to anything the lamp asked.
        let quiet = [
            (
                0,
                "Lamp, what's the weather today?",
                "Sunny and warm today.",
            ),
            (
                10,
                "Did you finish the slides for Monday?",
                "I can't see your slides.",
            ),
            (
                50,
                "Not yet, I will do them after lunch.",
                "Lunch sounds good.",
            ),
            (
                90,
                "Do you want to grab coffee first then?",
                "Coffee is a fine idea.",
            ),
            (
                130,
                "Sure, let me just get my jacket.",
                "Enjoy your coffee.",
            ),
        ];
        assert_eq!(
            run(false, false, None, None, None, &quiet),
            [true; 5],
            "today: all answered"
        );
        assert_eq!(
            run(
                true,
                false,
                None,
                Some(&["f1", "f2", "f1", "f2", "f1"]),
                None,
                &quiet
            ),
            [true, true, false, false, false],
            "another face than the caller's speaking: the lamp's answers to it renew nothing"
        );
        assert_eq!(
            run(true, false, None, None, None, &quiet),
            [true; 5],
            "nothing tells the two apart: taken for one person, as without the switch"
        );
        // The first builder's 01:02 on Queue #1722 (lamp-4ace's 00:32 played
        // test, day lines 12/12 to 9/12): one person, a named question, then
        // follow-ups without the name, 20 s apart, no voice model, no face.
        // Each answer renews the window, with the switch as without it.
        let day = [
            (
                0,
                "Lamp, what's the weather today?",
                "Sunny and warm today.",
            ),
            (20, "What do you see right now?", "I see a desk and a mug."),
            (
                40,
                "Remember that my favourite colour is green.",
                "I'll remember that green is your favourite.",
            ),
            (60, "What time is it?", "It's ten past one."),
        ];
        assert_eq!(run(false, false, None, None, None, &day), [true; 4], "off");
        assert_eq!(
            run(true, false, None, None, None, &day),
            [true; 4],
            "on: one person's follow-ups all answered"
        );
        // Another voice known only after its answer (the same-voice check
        // slow) ends the conversation at the next line (#829), with the
        // switch as without it; in one voice the same lines all go on.
        let late = [
            (
                0,
                "Lamp, what's the weather today?",
                "Sunny and warm today.",
            ),
            (
                30,
                "Did you finish the slides for Monday?",
                "I can't see your slides.",
            ),
            (60, "I think I'll go out later.", "Have a nice walk."),
            (70, "Will it rain this evening?", "No rain this evening."),
        ];
        let voices = Some(&[1, -1, 1, 1][..]);
        assert_eq!(
            run(false, false, None, None, voices, &late),
            [true, true, false, false],
            "off"
        );
        assert_eq!(
            run(true, false, None, None, voices, &late),
            [true, true, false, false],
            "on: another voice, known late, ends it"
        );
        assert_eq!(
            run(true, false, None, None, Some(&[1, 1, 1, 1][..]), &late),
            [true; 4],
            "on: one voice"
        );
        assert_eq!(
            run(true, false, None, None, Some(&[1, 1, 1, 1][..]), &day),
            [true; 4],
            "on: one voice's follow-ups all answered"
        );
        assert_eq!(
            run(
                true,
                false,
                None,
                Some(&["f1", "f1", "f1", "f1"]),
                None,
                &day
            ),
            [true; 4],
            "on: the caller's own face speaking each"
        );
        // The reviewer's P3 on #2339 and the first builder at 01:51 on
        // #2341: the call seen spoken by no face, then the caller seen
        // speaking each follow-up turned to the lamp, the last after looking
        // away, with a new id: theirs. Not turned to the lamp, someone
        // else's: the window from its answer is left out.
        assert_eq!(
            run(
                true,
                false,
                None,
                Some(&["", ">f2", ">f2", ">f3"]),
                None,
                &day
            ),
            [true; 4],
            "on: the caller never seen speaking the call, turned to the lamp"
        );
        assert_eq!(
            run(true, false, None, Some(&["", "f2", "f2", "f2"]), None, &day),
            [true, true, true, false],
            "on: no caller face, a face not turned to the lamp"
        );
        // A busy room goes on 15 s from the end of the lamp's words.
        let busy = [
            (
                0,
                "Lamp, what's the weather today?",
                "Sunny and warm today.",
            ),
            (
                10,
                "Did you finish the slides for Monday?",
                "I can't see your slides.",
            ),
            (
                24,
                "Not yet, I will do them after lunch.",
                "Lunch sounds good.",
            ),
            (
                38,
                "Do you want to grab coffee first then?",
                "Coffee is a fine idea.",
            ),
            (52, "Sure, let me just get my jacket.", "Enjoy your coffee."),
        ];
        assert_eq!(
            run(false, true, None, None, None, &busy),
            [true; 5],
            "today: all answered"
        );
        assert_eq!(
            run(
                true,
                true,
                None,
                Some(&["f1", "f2", "f1", "f2", "f1"]),
                None,
                &busy
            ),
            [true, true, false, false, false]
        );
        // A reply to the lamp's own question goes on, and its answer renews.
        let asked = [
            (
                0,
                "Lamp, what's the weather today?",
                "Sunny and warm today.",
            ),
            (
                10,
                "Did you finish the slides for Monday?",
                "Shall I remind you?",
            ),
            (
                50,
                "Yes, remind me after lunch.",
                "I will remind you at one.",
            ),
            (70, "Thanks, that is very helpful.", "You're welcome."),
        ];
        assert_eq!(
            run(
                true,
                false,
                None,
                Some(&["f1", "f2", "f2", "f2"]),
                None,
                &asked
            ),
            [true; 4]
        );
        // The reviewer's 21:39 case: once the chain is cut, a line the lamp
        // says on its own (a reminder) opens its window as before, and the
        // person's reply to it goes on.
        let reminded = [
            (
                0,
                "Lamp, what's the weather today?",
                "Sunny and warm today.",
            ),
            (
                10,
                "Did you finish the slides for Monday?",
                "I can't see your slides.",
            ),
            (
                50,
                "Not yet, I will do them after lunch.",
                "Lunch sounds good.",
            ),
            (300, "", "Your tea is ready."),
            (305, "Thanks, I'll go and get it now.", "Enjoy your tea."),
        ];
        assert_eq!(run(false, false, None, None, None, &reminded), [true; 4]);
        assert_eq!(
            run(
                true,
                false,
                None,
                Some(&["f1", "f2", "f1", "", "f2"]),
                None,
                &reminded
            ),
            [true, true, false, true],
            "a reply to the lamp's own line goes on"
        );
        // The first builder's 18:20 case on lamp-4ace: a busy room, the
        // lamp's name heard only in the room's sound (0.95) and answered,
        // then four lines of talk not for the lamp. Today each answer lets
        // the next out; with the switch, those after the first are held.
        let named = [
            (0, "What time is it now?", "It's twenty past six."),
            (
                10,
                "We should leave before the traffic.",
                "Traffic is heavy now.",
            ),
            (24, "Did you call the restaurant?", "I can't call them."),
            (38, "Yes, they said seven is fine.", "Seven sounds good."),
            (52, "Great, let me get my keys.", "Have a nice dinner."),
        ];
        assert_eq!(
            run(false, true, Some(0.95), None, None, &named),
            [true; 5],
            "today: all answered"
        );
        assert_eq!(
            run(
                true,
                true,
                Some(0.95),
                Some(&["f1", "f2", "f1", "f2", "f1"]),
                None,
                &named
            ),
            [true, true, false, false, false]
        );
    }

    /// The first builder at 02:33 on Queue #1722: a print is someone else's
    /// only under 0.30 against the caller's (mic2 on lamp-4ace: the same
    /// voice in a quiet room down to 0.35, others up to 0.29); no print is
    /// never another voice.
    #[test]
    fn someone_else_by_voice_only_under_0_30() {
        let heeding = Heeding::new(true, true, None, None);
        let seen = |likeness| Seen {
            room: crate::room_told::Room::Quiet,
            going_on: true,
            turned: None,
            busy: false,
            lamp_asked: false,
            speakers: Vec::new(),
            turned_speakers: Vec::new(),
            heard: String::new(),
            named_faced: false,
            other_voice: false,
            same_voice: false,
            likeness,
        };
        assert!(
            !heeding.someone_else(&seen(Some(0.35))),
            "the same voice, quiet"
        );
        assert!(heeding.someone_else(&seen(Some(0.25))), "another voice");
        assert!(!heeding.someone_else(&seen(None)), "no print");
    }

    /// #1712: a line that called the lamp, let out, carries the person's
    /// next line without the name as the conversation going on, though the
    /// lamp has said no word yet; two lines left alone since still end it.
    #[test]
    fn a_call_let_out_carries_the_next_line_though_the_lamp_said_nothing() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
        {
            let mut room = crate::room_told::room().lock().unwrap();
            *room = crate::room_told::RoomTold::new();
            room.count_only_talk(true);
        }
        let said = (10_000_000, 11_000_000);
        let mut heeding = Heeding::new(true, true, None, None);
        heeding.as_the_only_voice(false, true, false);
        assert_eq!(
            heeding.heed(207, said, "Lamp, what time is this?", None),
            None
        );
        assert_eq!(
            heeding.heed(209, said, "Set a timer for five minutes.", None),
            None
        );
        {
            let mut room = crate::room_told::room().lock().unwrap();
            for request in [210, 211] {
                room.unanswered(request, lamp_ipc::monotonic_us());
            }
        }
        assert!(
            heeding
                .heed(212, said, "What's the price of gold?", None)
                .is_some()
        );
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    }

    /// What the engine is told for a checked sound (17 October): the lamp's
    /// own last line a question, in a conversation going on. A line that
    /// asks nothing, or the same question past the conversation, is not.
    #[test]
    fn the_lamp_just_asked_only_with_its_question_in_a_conversation() {
        use std::time::{Duration, Instant};
        let now = Instant::now();
        let asked = |lamp: &str, later: Duration| {
            let mut thread = crate::thread::Thread::new();
            thread.person("Can you get the office build checked?", true, now);
            thread.lamp(1, lamp, now);
            super::lamp_just_asked(&thread, 2, now + later)
        };
        let question = "I can hand that to Codex. Shall I start it?";
        assert!(asked(question, Duration::ZERO));
        assert!(asked(question, Duration::from_secs(20)));
        assert!(!asked(question, Duration::from_secs(120)));
        assert!(!asked(
            "I can hand that to Codex whenever you like.",
            Duration::ZERO
        ));
        // Its own answer is not what it asked before.
        let mut thread = crate::thread::Thread::new();
        thread.lamp(2, question, now);
        assert!(!super::lamp_just_asked(&thread, 2, now));
    }

    /// Each fact as a Live session is told it; the session fact only with
    /// no conversation going on, the room only when known.
    #[test]
    fn the_facts_are_the_live_routes_own() {
        use crate::room_told::Room;
        assert_eq!(super::facts(false, None), "");
        assert_eq!(super::facts(false, Some(Room::Unknown)), "");
        assert_eq!(
            super::facts(true, Some(Room::Busy)),
            format!(
                "\n\n{}\n\n{}",
                crate::session_fact::FACT,
                crate::room_told::BUSY
            )
        );
        assert_eq!(
            super::facts(false, Some(Room::Quiet)),
            format!("\n\n{}", crate::room_told::QUIET)
        );
    }

    /// #829's two-people scene in a quiet room
    /// (`docs/verification/two-people-20261015/two-people-scene.txt`): on
    /// lamp-52e6 at 20:1x (#2301) the talk lines right after "Hey Lamp, what
    /// time is it?" were answered as the conversation going on, before the
    /// room was judged busy. With `LAMP_QUIET_GOING_ON_SAME_ONE=on` but
    /// neither the voice model nor a speaking face, nothing tells the two
    /// people apart, so nothing is held, as without the switch (the first
    /// builder at 01:21 on Queue #1722): the rule does nothing then. With
    /// another voice or another face, see
    /// `quiet_going_on_plays_the_callers_voice` and
    /// `quiet_going_on_plays_the_callers_speaking_face`.
    #[test]
    fn quiet_going_on_holds_nothing_without_a_sign_of_someone_else() {
        let _one = crate::provider_worker::tests::one_thread();
        use crate::room_told::Room;
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
        let signs = |going_on, lamp_asked, turned| Signs {
            room: Room::Quiet,
            going_on,
            lamp_asked,
            turned: Some(turned),
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let said = (10_000_000, 11_000_000);
        let talk = [
            "So did you send the report to Maria yesterday?",
            "I did, but she has not replied yet. I think she is travelling this week.",
            "Right. Well, we need her numbers before Friday.",
        ];
        let scene = |on: bool| {
            let mut heeding = Heeding::new(true, true, None, None);
            heeding.as_the_only_voice(false, true, false);
            if on {
                heeding.going_on_quiet_only_for_the_same_one();
            }
            let mut verdicts = vec![heeding.judge(
                1,
                said,
                "Hey Lamp, what time is it?",
                signs(false, false, false),
            )];
            for (request, line) in (2..).zip(talk) {
                verdicts.push(heeding.judge(request, said, line, signs(true, false, false)));
            }
            verdicts.push(heeding.judge(
                5,
                said,
                "Hey Lamp, what do you think we should have for lunch?",
                signs(false, false, false),
            ));
            let drink = "And what would you drink with that?";
            verdicts.push(heeding.judge(6, said, drink, signs(true, false, false)));
            verdicts.push(heeding.judge(7, said, drink, signs(true, false, true)));
            verdicts.push(heeding.judge(8, said, "The blue one.", signs(true, true, false)));
            verdicts
        };
        assert_eq!(
            scene(true),
            [None; 8],
            "on, no voice model, no face: nothing held"
        );
        assert_eq!(scene(false), [None; 8], "off: as before");
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    }

    /// #829's scene as lamp-52e6 ran it at 22:4x: `LAMP_ATTENTION=busy-near`
    /// (strict), no `LAMP_SPOKEN_TO`, the opening let out by the lamp's name
    /// heard in its sound, and the talk lines begun over the lamp's answer
    /// to it. The three talk lines were played as the conversation going on
    /// with the switch on: the hold read `alone`, which is only ever true
    /// with `LAMP_SPOKEN_TO`. The call is seen spoken by no face, the talk by
    /// one not turned to the lamp: someone else's.
    #[test]
    fn quiet_going_on_holds_the_talk_without_spoken_to() {
        let _one = crate::provider_worker::tests::one_thread();
        use crate::room_told::Room;
        use crate::second_provider::Heard;
        use lamp_msgs::{Face, Faces, Speaking, SpeakingState};
        let other = Faces {
            faces: vec![Face {
                id: "f2".to_owned(),
                xmin: 0.2,
                ymin: 0.2,
                xmax: 0.4,
                ymax: 0.5,
                confidence: 1.0,
                yaw: None,
                facing: None,
                speaking: Some(Speaking {
                    state: SpeakingState::Yes,
                    score: 0.9,
                    from: None,
                }),
            }],
        };
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
        let held = Some(crate::second_provider::QUIET_GOING_ON_HELD);
        let talk = [
            "So did you send the report to Maria yesterday?",
            "I did, but she has not replied yet. I think she is travelling this week.",
            "Right. Well, we need her numbers before Friday.",
        ];
        let scene = |on: bool| {
            let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
            let (now, base) = (Instant::now(), monotonic_us());
            let mut heeding = Heeding::new(true, true, None, None);
            heeding.as_the_only_voice(false, false, false);
            if on {
                heeding.going_on_quiet_only_for_the_same_one();
                heeding.seeing_speaking_faces(eyes.clone(), None);
            }
            let mut verdicts = vec![heeding.judge(
                1,
                (base, base + 1_000_000),
                "Hey Lamp, what time is it?",
                Signs {
                    room: Room::Quiet,
                    going_on: false,
                    lamp_asked: false,
                    turned: Some(false),
                    names: vec![(base + 100_000, 0.95)],
                    near: false,
                    continues_from: None,
                },
            )];
            for (request, line) in (2u64..).zip(talk) {
                heeding.hear(
                    request,
                    Heard::Began {
                        over_the_lamps_voice: true,
                    },
                );
                let after = 8_000_000 * (request - 1);
                for look in 0..4u64 {
                    eyes.look(
                        now + Duration::from_micros(after + 200_000 + look * 600_000),
                        Some((("b", request * 10 + look), &other)),
                    );
                }
                let began = base + after;
                verdicts.push(heeding.judge(
                    request,
                    (began, began + 3_000_000),
                    line,
                    Signs {
                        room: Room::Quiet,
                        going_on: true,
                        lamp_asked: false,
                        turned: Some(false),
                        names: Vec::new(),
                        near: false,
                        continues_from: None,
                    },
                ));
            }
            verdicts
        };
        assert_eq!(
            scene(true),
            [None, held, held, held],
            "on: the call answered, the talk over its answer held"
        );
        assert_eq!(scene(false), [None; 4], "off: as before");
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    }

    /// The first builder's 23:01 on #2305, (a): with the eyes' speaking
    /// faces, a line the answered caller's face is seen speaking through
    /// goes on, said looking away; another face's is held.
    #[test]
    fn quiet_going_on_plays_the_callers_speaking_face() {
        let _one = crate::provider_worker::tests::one_thread();
        use crate::room_told::Room;
        use lamp_msgs::{Face, Faces, Speaking, SpeakingState};
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
        let speaking = |id: &str| Faces {
            faces: vec![Face {
                id: id.to_owned(),
                xmin: 0.2,
                ymin: 0.2,
                xmax: 0.4,
                ymax: 0.5,
                confidence: 1.0,
                yaw: None,
                facing: None,
                speaking: Some(Speaking {
                    state: SpeakingState::Yes,
                    score: 0.9,
                    from: None,
                }),
            }],
        };
        let signs = |going_on| Signs {
            room: Room::Quiet,
            going_on,
            lamp_asked: false,
            turned: Some(false),
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
        let (now, now_us) = (Instant::now(), monotonic_us());
        // Each line 2 s long, 3 s apart, the face speaking in four looks
        // through it.
        let mut seq = 0;
        let mut line = |index: u64, face: &str| {
            for look in 0..4u64 {
                seq += 1;
                eyes.look(
                    now + Duration::from_millis(index * 3_000 + 200 + look * 400),
                    Some((("b", seq), &speaking(face))),
                );
            }
            let began = now_us + index * 3_000_000;
            (began, began + 2_000_000)
        };
        let lines = [line(0, "f1"), line(1, "f2"), line(2, "f1")];
        let mut heeding = Heeding::new(true, true, None, None);
        heeding.as_the_only_voice(false, true, false);
        heeding.going_on_quiet_only_for_the_same_one();
        heeding.seeing_speaking_faces(eyes, None);
        assert_eq!(
            heeding.judge(1, lines[0], "Hey Lamp, what time is it?", signs(false)),
            None
        );
        assert_eq!(
            heeding.judge(
                2,
                lines[1],
                "So did you send the report to Maria yesterday?",
                signs(true)
            ),
            Some(crate::second_provider::QUIET_GOING_ON_HELD),
            "another face speaking: held"
        );
        assert_eq!(
            heeding.judge(
                3,
                lines[2],
                "And what would you drink with that?",
                signs(true)
            ),
            None,
            "the caller's face speaking, looking away: goes on"
        );
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    }

    /// The first builder at 01:51 on #2341: after a call no face was seen
    /// speaking, the caller's follow-up seen speaking turned to the lamp goes
    /// on and its face becomes the caller's, so said looking away it still
    /// goes on; after looking away 2 s the caller comes back with a new id,
    /// turned to the lamp, and goes on; another face not turned to the lamp
    /// is held.
    #[test]
    fn quiet_going_on_takes_a_face_turned_to_the_lamp_for_the_callers() {
        let _one = crate::provider_worker::tests::one_thread();
        use crate::room_told::Room;
        use lamp_msgs::{Face, Faces, Speaking, SpeakingState};
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
        let speaking = |id: &str, turned: bool| Faces {
            faces: vec![Face {
                id: id.to_owned(),
                xmin: 0.2,
                ymin: 0.2,
                xmax: 0.4,
                ymax: 0.5,
                confidence: 1.0,
                yaw: None,
                facing: turned.then_some(true),
                speaking: Some(Speaking {
                    state: SpeakingState::Yes,
                    score: 0.9,
                    from: None,
                }),
            }],
        };
        let signs = |going_on| Signs {
            room: Room::Quiet,
            going_on,
            lamp_asked: false,
            turned: Some(false),
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
        let (now, now_us) = (Instant::now(), monotonic_us());
        // Each line 2 s long, 3 s apart; `face` speaking through it, none
        // for "".
        let mut seq = 0;
        let mut line = |index: u64, face: &str, turned: bool| {
            for look in (0..4u64).filter(|_| !face.is_empty()) {
                seq += 1;
                eyes.look(
                    now + Duration::from_millis(index * 3_000 + 200 + look * 400),
                    Some((("b", seq), &speaking(face, turned))),
                );
            }
            let began = now_us + index * 3_000_000;
            (began, began + 2_000_000)
        };
        let lines = [
            line(0, "", false),
            line(1, "f1", true),
            line(2, "f1", false),
            line(3, "f3", true),
            line(4, "f2", false),
        ];
        let mut heeding = Heeding::new(true, true, None, None);
        heeding.as_the_only_voice(false, true, false);
        heeding.going_on_quiet_only_for_the_same_one();
        heeding.seeing_speaking_faces(eyes, None);
        let held = Some(crate::second_provider::QUIET_GOING_ON_HELD);
        let verdicts: Vec<_> = [
            "Hey Lamp, what time is it?",
            "And what about tomorrow?",
            "Will it rain in the evening?",
            "Remind me to take an umbrella then.",
            "Did you see the game last night?",
        ]
        .into_iter()
        .zip(lines)
        .zip(1..)
        .map(|((words, said), request)| heeding.judge(request, said, words, signs(request > 1)))
        .collect();
        assert_eq!(
            verdicts,
            [None, None, None, None, held],
            "the call unseen; turned: the caller's; looking away: still theirs; back with a new id turned: goes on; another face not turned: held"
        );
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    }

    /// #2305's review: the caller's face ids go with a privacy change (and
    /// the eyes' looks with them), when its conversation is over, and when
    /// the eyes start again: each time the same passing id speaking a line
    /// let out only as the conversation going on, not turned to the lamp,
    /// is held.
    #[test]
    fn the_callers_face_goes_with_privacy_the_window_and_the_eyes_boot() {
        let _one = crate::provider_worker::tests::one_thread();
        use crate::room_told::Room;
        use lamp_msgs::{Face, Faces, Speaking, SpeakingState};
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
        let speaking = Faces {
            faces: vec![Face {
                id: "f1".to_owned(),
                xmin: 0.2,
                ymin: 0.2,
                xmax: 0.4,
                ymax: 0.5,
                confidence: 1.0,
                yaw: None,
                facing: None,
                speaking: Some(Speaking {
                    state: SpeakingState::Yes,
                    score: 0.9,
                    from: None,
                }),
            }],
        };
        let signs = |going_on| Signs {
            room: Room::Quiet,
            going_on,
            lamp_asked: false,
            turned: Some(false),
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let held = Some(crate::second_provider::QUIET_GOING_ON_HELD);
        // f1 speaks the call (the eyes' boot "a"), then a line going on,
        // seen under `boot`; `between` comes between the two.
        let scene = |boot: &str, between: &dyn Fn(&mut Heeding)| {
            let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
            let (now, now_us) = (Instant::now(), monotonic_us());
            let mut seq = 0;
            let mut line = |index: u64, boot: &str| {
                for look in 0..4u64 {
                    seq += 1;
                    eyes.look(
                        now + Duration::from_millis(index * 3_000 + 200 + look * 400),
                        Some(((boot, seq), &speaking)),
                    );
                }
                let began = now_us + index * 3_000_000;
                (began, began + 2_000_000)
            };
            let call = line(0, "a");
            let mut heeding = Heeding::new(true, true, None, None);
            heeding.as_the_only_voice(false, true, false);
            heeding.going_on_quiet_only_for_the_same_one();
            heeding.seeing_speaking_faces(eyes.clone(), None);
            assert_eq!(
                heeding.judge(1, call, "Hey Lamp, what time is it?", signs(false)),
                None
            );
            between(&mut heeding);
            let call_seen = eyes.speakers(now, now + Duration::from_secs(2));
            let going_on = line(1, boot);
            let verdict = heeding.judge(
                3,
                going_on,
                "And what would you drink with that?",
                signs(true),
            );
            (verdict, call_seen)
        };
        let nothing = |_: &mut Heeding| {};
        assert_eq!(scene("a", &nothing).0, None, "the caller's face: goes on");
        assert_eq!(
            scene("b", &nothing).0,
            held,
            "the same id after the eyes start again: someone else"
        );
        assert_eq!(scene("a", &nothing).1, ["f1"]);
        // Gone, the caller's id matches nothing: a face not turned to the
        // lamp is someone else's.
        let (verdict, call_seen) = scene("a", &|heeding| {
            heeding.privacy_changed();
            assert!(heeding.caller_faces.is_empty());
        });
        assert_eq!(verdict, held, "after a privacy change");
        assert!(call_seen.is_empty(), "the looks before it are gone");
        let window_over = |heeding: &mut Heeding| {
            heeding.judge(2, (1, 2), "Nice weather today.", signs(false));
            assert!(heeding.caller_faces.is_empty());
        };
        assert_eq!(
            scene("a", &window_over).0,
            held,
            "after the conversation's window ended"
        );
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    }

    /// The first builder's 01:21 on Queue #1722: lamp-4ace's 01:03 played
    /// test, with no voice model and no face, played 6/12 day lines with
    /// `LAMP_QUIET_GOING_ON_SAME_ONE`, one person's own follow-ups held.
    /// With no sign of someone else they are the same person's: all go on,
    /// each 15 s after the lamp's last words, short or long.
    #[test]
    fn quiet_going_on_without_voice_or_face_plays_one_persons_follow_ups() {
        let _one = crate::provider_worker::tests::one_thread();
        use crate::room_told::Room;
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
        let signs = |going_on| Signs {
            room: Room::Quiet,
            going_on,
            lamp_asked: false,
            turned: Some(false),
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let mut heeding = Heeding::new(true, true, None, None);
        heeding.as_the_only_voice(false, true, false);
        heeding.going_on_quiet_only_for_the_same_one();
        let s = 1_000_000;
        let mut verdicts = vec![heeding.judge(
            1,
            (10 * s, 11 * s),
            "Lamp, what's the weather today?",
            signs(false),
        )];
        heeding.lamp_did(crate::second_provider::Did::Said(1, 14 * s));
        let mut until = 14 * s;
        for (request, (long, words)) in (2..).zip([
            (2_200_000, "Can you make your light a little warmer?"),
            (1_400_000, "What do you see right now?"),
            (2_500_000, "Remember that my favourite colour is green."),
            (2_000_000, "Remind me in two minutes to stretch."),
            (1_300_000, "What's my favourite colour?"),
            (1_000_000, "What time is it?"),
        ]) {
            let began = until + 15 * s;
            let verdict = heeding.judge(request, (began, began + long), words, signs(true));
            if verdict.is_none() {
                until = began + long + 4 * s;
                heeding.lamp_did(crate::second_provider::Did::Said(request, until));
            }
            verdicts.push(verdict);
        }
        assert_eq!(verdicts, [None; 7]);
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    }

    /// The same scene with the same-voice check (faked as in
    /// `another_voice_going_on_is_held_and_the_conversations_own_goes_on`)
    /// and the switch on: the other person's talk is held, the caller's own
    /// follow-up "And what would you drink with that?" looking away is
    /// answered, and so is a line in the caller's voice.
    #[test]
    fn quiet_going_on_plays_the_callers_voice() {
        let _one = crate::provider_worker::tests::one_thread();
        use crate::room_told::Room;
        use crate::second_provider::Heard;
        /// Prints by a line's first sample: 1 is the caller's voice, -1
        /// the other person's.
        #[derive(Default)]
        struct Voice(std::collections::VecDeque<(u64, Result<Vec<f32>, String>)>);
        impl crate::same_voice::Printer for Voice {
            fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
                let print = if samples[0] > 0 {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                };
                self.0.push_back((request, Ok(print)));
                true
            }
            fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
                self.0.pop_front()
            }
        }
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
        let signs = |going_on| Signs {
            room: Room::Quiet,
            going_on,
            lamp_asked: false,
            turned: Some(false),
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let said = (10_000_000, 11_000_000);
        let mut heeding = Heeding::new(true, true, None, None);
        heeding.as_the_only_voice(false, true, false);
        heeding.going_on_quiet_only_for_the_same_one();
        heeding.checking_the_voice(Box::new(Voice::default()));
        let mut line = |request: u64, voice: i16, words: &str, going_on: bool| {
            heeding.hear(
                request,
                Heard::Began {
                    over_the_lamps_voice: false,
                },
            );
            heeding.hear(request, Heard::Samples(&[voice; 48_000]));
            heeding.hear(request, Heard::Ended);
            heeding.judge(request, said, words, signs(going_on))
        };
        assert_eq!(line(1, 1, "Hey Lamp, what time is it?", false), None);
        // Held by the voice check before this rule: another voice ends the
        // conversation for it, so nothing spoke to the lamp.
        assert_eq!(
            line(2, -1, "I did, but she has not replied yet.", true),
            Some(crate::attention::NOT_SPOKEN_TO),
            "the other person's talk is held"
        );
        assert_eq!(
            line(
                3,
                1,
                "Hey Lamp, what do you think we should have for lunch?",
                false
            ),
            None
        );
        assert_eq!(
            line(4, 1, "And what would you drink with that?", true),
            None,
            "the caller's follow-up goes on"
        );
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    }

    /// A request going on from the one begun just before it, the same
    /// utterance after the lamp ended it early (`continues`), keeps what the
    /// rule knew of it when the rule judged that one: "Hey Lamp, what" then
    /// "time is it?" in a busy room is answered. One going on from a request
    /// the rule never judged (replaced before its words came back) is judged
    /// fresh, never as going on from an older question (the reviewer on
    /// #1226: "budget" ten minutes after "weather").
    #[test]
    fn a_request_going_on_keeps_only_the_one_just_before_it() {
        use crate::room_told::Room;
        let busy = |continues_from: Option<u64>| Signs {
            room: Room::Busy,
            going_on: false,
            lamp_asked: false,
            turned: Some(false),
            names: Vec::new(),
            near: false,
            continues_from,
        };
        let said = (10_000_000, 11_000_000);
        let rest = (11_400_000, 12_000_000);
        let mut heeding = Heeding::new(true, true, None, None);
        assert_eq!(heeding.judge(300, said, "Hey Lamp, what", busy(None)), None);
        assert_eq!(
            heeding.judge(301, rest, "time is it?", busy(Some(300))),
            None
        );
        // Not going on: the same words alone are office talk.
        let mut fresh = Heeding::new(true, true, None, None);
        assert_eq!(fresh.judge(300, said, "Hey Lamp, what", busy(None)), None);
        assert!(fresh.judge(301, rest, "time is it?", busy(None)).is_some());
        // The reviewer's probe: 302 goes on from 301, which was never judged
        // (replaced before its words came back), ten minutes after 300.
        let mut probe = Heeding::new(true, true, None, None);
        assert_eq!(
            probe.judge(300, said, "Hey Lamp, what's the weather", busy(None)),
            None
        );
        let later = (610_000_000, 611_000_000);
        assert!(
            probe
                .judge(
                    302,
                    later,
                    "do you think the budget is fine",
                    busy(Some(301))
                )
                .is_some()
        );
    }

    /// Fourth's probe on #1262: "Hey" held, then the request going on from
    /// it with the engine's joined words "Hey Lamp, turn the light on." is a
    /// call, so it plays; the stripped words keep their space.
    #[test]
    fn a_carried_over_call_keeps_the_space_between_its_halves() {
        use crate::room_told::Room;
        let busy = |continues_from: Option<u64>| Signs {
            room: Room::Busy,
            going_on: false,
            lamp_asked: false,
            turned: Some(false),
            names: Vec::new(),
            near: false,
            continues_from,
        };
        let said = (10_000_000, 10_400_000);
        let rest = (10_800_000, 12_000_000);
        let mut heeding = Heeding::new(true, true, None, None);
        assert!(heeding.judge(300, said, "Hey", busy(None)).is_some());
        assert_eq!(
            heeding.judge(301, rest, "Hey Lamp, turn the light on.", busy(Some(300))),
            None
        );
    }

    /// The two remarks on lamp-4ace on 2026-10-16 that this route answered
    /// and Gemini's session left alone (#1014): a busy room, no name, nobody
    /// facing. Held, 0 of 2; a named question in the same room, and a
    /// follow-up in a conversation with the lamp, are still answered.
    #[test]
    fn the_rule_holds_office_talk_and_lets_a_call_or_a_follow_up_through() {
        use crate::room_told::Room;
        let busy = |names: Vec<(u64, f32)>, going_on: bool| Signs {
            room: Room::Busy,
            going_on,
            lamp_asked: false,
            turned: Some(false),
            names,
            near: false,
            continues_from: None,
        };
        let said = (10_000_000, 11_500_000);
        let mut heeding = Heeding::new(true, true, None, None);
        for (request, remark) in [(202, "Did you see what he wants us to look"), (203, "Me.")] {
            assert!(
                heeding
                    .judge(request, said, remark, busy(Vec::new(), false))
                    .is_some(),
                "{remark}"
            );
        }
        // Named in the text, or heard plainly in the sound.
        assert_eq!(
            heeding.judge(
                204,
                said,
                "Hey Lamp, what time is it?",
                busy(Vec::new(), false)
            ),
            None
        );
        assert_eq!(
            heeding.judge(
                205,
                said,
                "What time is it?",
                busy(vec![(9_800_000, 1.0)], false)
            ),
            None
        );
        // A hearing from before the line's window is not its call.
        let later = (30_000_000, 31_500_000);
        assert!(
            heeding
                .judge(
                    206,
                    later,
                    "What time is it?",
                    busy(vec![(25_000_000, 1.0)], false)
                )
                .is_some()
        );
        assert_eq!(
            heeding.judge(
                207,
                said,
                "And what about tomorrow?",
                busy(Vec::new(), true)
            ),
            None
        );
        // Someone turned to the lamp, or a room not known to be busy.
        let mut turned = busy(Vec::new(), false);
        turned.turned = Some(true);
        assert_eq!(heeding.judge(208, said, "Me.", turned), None);
        let mut quiet = busy(Vec::new(), false);
        quiet.room = Room::Quiet;
        assert_eq!(heeding.judge(209, said, "Me.", quiet), None);
        // busy-near: a name heard at 0.95 or more counts only with the room
        // microphone 18 dB over its quiet during the line.
        let (soon, sooner) = ((50_000_000, 51_500_000), (70_000_000, 71_500_000));
        assert!(
            heeding
                .judge(
                    210,
                    soon,
                    "What time is it?",
                    busy(vec![(50_200_000, 0.96)], false)
                )
                .is_some()
        );
        let mut near = busy(vec![(70_200_000, 0.96)], false);
        near.near = true;
        assert_eq!(heeding.judge(211, sooner, "What time is it?", near), None);
    }

    use super::*;

    /// lamp-c0a4 on #2449 (17:31): the held gap's window is measured from
    /// when the lamp's answer ends playing, not from its turn completing,
    /// which comes about as its first sound goes out.
    #[test]
    fn an_answer_ends_when_its_sound_has_all_been_played() {
        let audio = |request, samples: usize| ProviderOutput::Audio {
            request,
            sequence: 0,
            provider_event_at_us: 0,
            source_frames: samples,
            source_offset: 0,
            samples: vec![0; samples],
        };
        let start = Instant::now();
        let mut sound = AnswerSound::default();
        sound.heard(&audio(4, 24_000), start);
        sound.heard(&audio(4, 60_000), start + Duration::from_millis(300));
        // 84,000 samples at 24 kHz: 3.5 s after its first sound.
        assert_eq!(sound.ends(4), start + Duration::from_millis(3_500));
        // Another request's answer starts its own count; one with no sound
        // ends now.
        sound.heard(&audio(5, 2_400), start);
        assert_eq!(sound.ends(5), start + Duration::from_millis(100));
        assert!(sound.ends(6) >= start);
    }
    use crate::second_provider::{Listener, Services, net};

    /// #1712, the first builder at 18:56: with `LAMP_SECOND_FEW_TOOLS=on` a
    /// spoken turn's request is given its few functions and one that names
    /// the rest; once the model has called it, every function. The memory's
    /// remember, recall and forget are among the few (the first builder at
    /// 00:34 on #1722); forgetting everything stays behind it.
    #[test]
    fn a_spoken_turn_is_given_its_few_functions_and_the_rest_behind_one() {
        let tool = |name: &str| lamp_gemini::Tool {
            name: name.into(),
            description: "A function.".into(),
            parameters: None,
            silent: false,
        };
        let declared: Vec<_> = [
            "not_for_me",
            "the_time",
            "remember",
            "recall",
            "forget",
            "forget_everything",
            "light_on",
            "volume_up",
            "set_timer",
            "start_agent_task",
            "remember_my_face",
            "use_skill",
            "web_search",
            "take_a_look",
        ]
        .map(tool)
        .into();
        let names = |tools: &[lamp_gemini::Tool]| -> Vec<String> {
            tools.iter().map(|tool| tool.name.clone()).collect()
        };
        let asked = [
            Message::system("You are a lamp."),
            Message::user("Lights on."),
        ];
        let given = few_tools(declared.clone(), &asked);
        assert_eq!(
            names(&given),
            [
                "not_for_me",
                "remember",
                "recall",
                "forget",
                "light_on",
                "volume_up",
                "set_timer",
                "start_agent_task",
                "use_skill",
                "web_search",
                "take_a_look",
                MORE
            ]
        );
        assert!(
            given[11].description.starts_with(
                "Your other functions, not given to you yet: the_time, forget_everything, remember_my_face. "
            ),
            "{}",
            given[11].description
        );
        // Called: the next round has every one, as declared.
        let mut after = asked.to_vec();
        after.push(Message::assistant_calls(
            None,
            &[net::ToolCall {
                id: "c1".into(),
                name: MORE.into(),
                args: serde_json::json!({}),
                extra: None,
            }],
        ));
        after.push(Message::tool("c1", MORE_GIVEN));
        assert_eq!(few_tools(declared.clone(), &after), declared);
        // Nothing to put behind it: no such function.
        let few: Vec<_> = declared
            .iter()
            .filter(|tool| given_at_once(&tool.name))
            .cloned()
            .collect();
        assert_eq!(few_tools(few.clone(), &asked), few);
        // A note that names a function behind it is given that one at
        // once, setup's step still among them; the rest stay behind it.
        let noted = [
            Message::system("You are a lamp."),
            Message::system(crate::onboarding::WHO_NOTE),
            Message::user("I'm Ana."),
        ];
        let given = names(&few_tools(declared.clone(), &noted));
        assert!(
            given.iter().any(|name| name == "remember_my_face"),
            "{given:?}"
        );
        assert!(given.iter().any(|name| name == MORE), "{given:?}");
        assert!(!given.iter().any(|name| name == "the_time"), "{given:?}");
        let with_step: Vec<_> = declared
            .iter()
            .cloned()
            .chain([tool(crate::onboarding::STEP)])
            .collect();
        let given = names(&few_tools(with_step.clone(), &noted));
        assert!(
            given.iter().any(|name| name == crate::onboarding::STEP),
            "{given:?}"
        );
        // A plain turn still keeps it behind, and so does a word that only
        // holds the name.
        let given = names(&few_tools(with_step, &asked));
        assert!(
            given.iter().any(|name| name == crate::onboarding::STEP),
            "{given:?}"
        );
        assert!(
            !given.iter().any(|name| name == "remember_my_face"),
            "{given:?}"
        );
        let inside = [Message::user("Say xremember_my_face.")];
        let given = names(&few_tools(declared, &inside));
        assert!(
            !given.iter().any(|name| name == "remember_my_face"),
            "{given:?}"
        );
    }

    /// Its call is answered by the route itself, with nothing run.
    #[tokio::test]
    async fn the_call_for_the_rest_is_answered_by_the_route() {
        let runner = tool_runner(crate::tools::Settings::default(), None, None, false);
        assert_eq!(
            runner(1, true, MORE.into(), serde_json::json!({})).await,
            serde_json::json!({"ok": true, "result": MORE_GIVEN})
        );
    }

    fn audio_runner_scope() -> (
        crate::memory_scope::RequestScope,
        lamp_msgs::RequestedAudioContext,
    ) {
        let worker = BootId::new([0xaa; 16]).unwrap();
        let context = lamp_msgs::RequestedAudioContext {
            worker_boot: crate::process::boot_text(worker),
            provider_session: 4,
            request: 5,
            privacy_generation: 1,
            call_id: "original-music-call".into(),
            issued_us: monotonic_us(),
        };
        let owner = crate::memory_scope::RequestScope::new(
            crate::memory_scope::RequestKey {
                worker,
                session: context.provider_session,
                request: context.request,
            },
            crate::memory_scope::ScopeSnapshot::room(),
        );
        (owner, context)
    }

    /// One exchange has one absolute budget, including a held test reply.
    fn audio_runner_exchange(
        listener: std::os::unix::net::UnixListener,
        reply: impl FnOnce(&serde_json::Value, Duration) -> serde_json::Value,
    ) -> serde_json::Value {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        use std::io::{BufRead, BufReader, Read, Write};
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let left = || {
            deadline
                .checked_duration_since(Instant::now())
                .filter(|left| !left.is_zero())
                .expect("the runner did not finish its local RPC within three seconds")
        };
        let mut stream = loop {
            let timeout = Timespec::try_from(left()).unwrap();
            let mut waiting = [PollFd::new(&listener, PollFlags::IN)];
            match poll(&mut waiting, Some(&timeout)) {
                Ok(0) => continue,
                Ok(_) => match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                    Err(error) => panic!("the runner peer could not accept: {error}"),
                },
                Err(error) if error == rustix::io::Errno::INTR => continue,
                Err(error) => panic!("the runner peer could not wait: {error}"),
            }
        };
        // macOS accepted sockets inherit nonblocking mode.
        stream.set_nonblocking(false).unwrap();
        stream.set_read_timeout(Some(left())).unwrap();
        let mut line = String::new();
        BufReader::new((&stream).take(8192))
            .read_line(&mut line)
            .unwrap();
        assert!(line.ends_with('\n'), "one complete bounded request");
        let asked = serde_json::from_str(&line).unwrap();
        let reply = reply(&asked, left());
        stream.set_write_timeout(Some(left())).unwrap();
        writeln!(stream, "{reply}").unwrap();
        asked
    }

    /// A local Skills/executive peer; even a missing dispatch cannot leave
    /// its accept, request read, reply write or terminal await unbounded.
    fn audio_runner_peer(
        directory: &Path,
        reply: serde_json::Value,
    ) -> tokio::task::JoinHandle<serde_json::Value> {
        let listener =
            std::os::unix::net::UnixListener::bind(directory.join(crate::tools::CALL_SOCKET))
                .unwrap();
        tokio::task::spawn_blocking(move || audio_runner_exchange(listener, |_, _| reply))
    }

    #[tokio::test]
    async fn the_production_audio_runner_keeps_skills_identity_and_private_offers() {
        for mode in ["direct", "wrapped", "off", "executive"] {
            let directory = crate::process::SessionDirectory::create().unwrap();
            std::fs::write(
                directory.path.join(crate::tools::CATALOG_FILE),
                r#"{"tools":[{"name":"play_relaxing_music","description":"An explicitly requested recording."}]}"#,
            )
            .unwrap();
            let settings = crate::tools::Settings {
                skills: (mode != "executive").then(|| directory.path.clone()),
                exec: (mode == "executive").then(|| directory.path.clone()),
                use_skill: mode == "wrapped",
                requested_audio: mode != "off",
                ..Default::default()
            };
            let (owner, context) = audio_runner_scope();
            let offer = lamp_msgs::RequestedAudioOffer {
                skills_boot: "skills-test".into(),
                definition_generation: 1,
                offer_id: 1,
                context: context.clone(),
                skill: "relaxing-music".into(),
                asset: "/local/relaxing.wav".into(),
                sha256: "a".repeat(64),
                frames: 24_000,
                issued_us: context.issued_us,
                start_before_us: context.issued_us + lamp_msgs::REQUESTED_AUDIO_START_US,
                finish_before_us: context.issued_us
                    + lamp_msgs::REQUESTED_AUDIO_START_US
                    + 1_000_000,
            };
            let told = serde_json::json!({"next":"Say one short confirmation."});
            let peer = audio_runner_peer(
                &directory.path,
                serde_json::json!({"ok":true,"result":{
                    "next":"Say one short confirmation.","requested_audio":&offer,
                }}),
            );
            let runner = requested_audio_runner(settings, None, None, false);
            let answer = tokio::time::timeout(
                Duration::from_secs(3),
                runner(
                    context.clone(),
                    lamp_gemini::ToolCall {
                        id: context.call_id.clone(),
                        name: if mode == "wrapped" {
                            crate::use_skill::NAME
                        } else {
                            "play_relaxing_music"
                        }
                        .into(),
                        args: if mode == "wrapped" {
                            serde_json::json!({"skill":"play_relaxing_music"})
                        } else {
                            serde_json::json!({})
                        },
                    },
                    owner.lease(),
                ),
            )
            .await
            .expect("the production runner did not answer the local call");
            let asked = tokio::time::timeout(Duration::from_secs(3), peer)
                .await
                .expect("the local peer did not finish its bounded exchange")
                .unwrap();
            let authorized = matches!(mode, "direct" | "wrapped");
            let expected_told = if authorized {
                serde_json::json!({"next":"Say one short confirmation. The recording has only been offered for preparation; playback has not started and may still be refused. Confirm only that you will try to play it. Do not say it is playing, has started, or is ready."})
            } else {
                told
            };
            assert_eq!(answer.told, expected_told, "{mode}");
            assert_eq!(
                answer.requested_audio,
                authorized.then_some(offer),
                "{mode}"
            );
            let mut expected = serde_json::json!({
                "op":"call","name":"play_relaxing_music","args":{},
            });
            if authorized {
                expected["requested_audio_context"] = serde_json::json!(context);
            }
            assert_eq!(asked, expected, "{mode}");
        }
    }

    #[tokio::test]
    async fn the_audio_runner_keeps_more_and_the_requests_early_search() {
        let search = Arc::new(Search::deepseek_at("http://127.0.0.1:9", "k").unwrap());
        let early = Arc::new(Early::default());
        let runner = requested_audio_runner(
            crate::tools::Settings {
                requested_audio: true,
                ..Default::default()
            },
            Some(search),
            Some(early.clone()),
            false,
        );
        let (owner, context) = audio_runner_scope();
        let call = |name: &str, args| lamp_gemini::ToolCall {
            id: context.call_id.clone(),
            name: name.into(),
            args,
        };
        let answer = tokio::time::timeout(
            Duration::from_secs(3),
            runner(
                context.clone(),
                call(MORE, serde_json::json!({})),
                owner.lease(),
            ),
        )
        .await
        .expect("the audio runner did not answer MORE");
        assert_eq!(
            answer.told,
            serde_json::json!({"ok":true,"result":MORE_GIVEN})
        );
        assert!(answer.requested_audio.is_none());
        early.begin(
            context.request,
            "who won the race".into(),
            Box::pin(async { serde_json::json!({"results":["early"]}) }),
        );
        assert!(
            early
                .take(context.request + 1, "who won the race")
                .is_none()
        );
        let args = serde_json::json!({"query":"who won the race"});
        let answer = tokio::time::timeout(
            Duration::from_secs(3),
            runner(
                context.clone(),
                call("web_search", args.clone()),
                owner.lease(),
            ),
        )
        .await
        .expect("the audio runner did not take its early search");
        assert_eq!(answer.told, serde_json::json!({"results":["early"]}));
        assert!(answer.requested_audio.is_none());
        assert!(
            early.take(context.request, "who won the race").is_none(),
            "taken once"
        );
        let again = tokio::time::timeout(
            Duration::from_secs(3),
            runner(context.clone(), call("web_search", args), owner.lease()),
        )
        .await
        .expect("the audio runner's later search did not finish at the closed port");
        assert!(again.told.get("error").is_some(), "{}", again.told);
        assert!(again.requested_audio.is_none());
    }

    #[tokio::test]
    async fn the_audio_runner_refuses_a_changed_or_retired_capability_before_dispatch() {
        for change in ["call", "worker", "session", "request", "retired"] {
            let (owner, mut context) = audio_runner_scope();
            let call = lamp_gemini::ToolCall {
                id: context.call_id.clone(),
                name: MORE.into(),
                args: serde_json::json!({}),
            };
            match change {
                "call" => context.call_id = "another-call".into(),
                "worker" => context.worker_boot = "bb".repeat(16),
                "session" => context.provider_session += 1,
                "request" => context.request += 1,
                "retired" => owner.revoke(),
                _ => unreachable!(),
            }
            let runner = requested_audio_runner(
                crate::tools::Settings {
                    requested_audio: true,
                    ..Default::default()
                },
                None,
                None,
                false,
            );
            let answer =
                tokio::time::timeout(Duration::from_secs(3), runner(context, call, owner.lease()))
                    .await
                    .expect("the audio runner did not refuse the invalid capability");
            assert_eq!(answer.told, crate::tools::unavailable(), "{change}");
            assert!(answer.requested_audio.is_none(), "{change}");
        }
    }
    use futures_util::{
        future::BoxFuture,
        stream::{self, BoxStream},
    };
    use lamp_turn_owner::{AdmissionState, AdmittedInput, CaptureState, Controller, Permission};
    use std::sync::Arc;

    /// Services that never answer: only authority is under test.
    struct Silent;
    impl Services for Silent {
        fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
            Box::pin(std::future::pending())
        }
        fn chat(&self, _: Vec<net::Message>) -> BoxStream<'static, net::Result<net::ChatEvent>> {
            Box::pin(stream::pending())
        }
        fn speak(&self, _: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
            Box::pin(stream::pending())
        }
    }

    /// Stop must return while the real Music RPC is still in the blocking
    /// pool. Its valid late reply cannot revive the revoked owner or chat.
    #[test]
    fn stop_during_music_rpc_revokes_scope_and_finishes_without_waiting_for_the_reply() {
        check_stop_during_music_rpc(false);
    }

    /// The next STT connection can still be opening when Music waits for
    /// Skills. Stop and runtime teardown cancel it without waiting for
    /// either connection, and the late Music reply still has no authority.
    #[test]
    fn stop_during_music_rpc_and_warm_stt_opening_finishes_without_waiting() {
        check_stop_during_music_rpc(true);
    }

    fn check_stop_during_music_rpc(warm_stt: bool) {
        // The real worker reads and writes the process's conversation too.
        // Hold its fixture guard until the runtime stops and THREAD is restored.
        let _one = crate::provider_worker::tests::one_thread();
        use crate::second_provider::requested_audio_tests::{RecordingScript, offered};
        use serde_json::json;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        struct Opening(Arc<AtomicBool>);
        impl Drop for Opening {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        struct WithSpare {
            script: Arc<RecordingScript>,
            listens: AtomicUsize,
            opened: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
            cancelled: Arc<AtomicBool>,
        }
        impl Services for WithSpare {
            fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
                let connection = self.listens.fetch_add(1, Ordering::SeqCst);
                if connection == 0 {
                    return self.script.listen();
                }
                assert_eq!(connection, 1, "only one spare may be opening");
                let opened = self.opened.lock().unwrap().take().unwrap();
                let cancelled = self.cancelled.clone();
                Box::pin(async move {
                    let _opening = Opening(cancelled);
                    opened.send(()).unwrap();
                    std::future::pending().await
                })
            }
            fn chat(
                &self,
                messages: Vec<net::Message>,
            ) -> BoxStream<'static, net::Result<net::ChatEvent>> {
                self.script.chat(messages)
            }
            fn speak(&self, text: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
                self.script.speak(text)
            }
        }

        let directory = crate::process::SessionDirectory::create().unwrap();
        let skills = directory.path.join("skills");
        std::fs::create_dir(&skills).unwrap();
        std::fs::write(
            skills.join(crate::tools::CATALOG_FILE),
            r#"{"tools":[{"name":"play_relaxing_music","description":"A requested recording."}]}"#,
        )
        .unwrap();
        let listener =
            std::os::unix::net::UnixListener::bind(skills.join(crate::tools::CALL_SOCKET)).unwrap();
        let (taken, mut received) = tokio::sync::oneshot::channel();
        let (release, held) = std::sync::mpsc::channel();
        let peer = std::thread::spawn(move || {
            audio_runner_exchange(listener, |asked, left| {
                let context: lamp_msgs::RequestedAudioContext =
                    serde_json::from_value(asked["requested_audio_context"].clone()).unwrap();
                taken.send(context.clone()).unwrap();
                held.recv_timeout(left)
                    .expect("the stopped worker must release the test peer");
                json!({"ok":true,"result":{
                    "next":"Confirm.","requested_audio":offered(context),
                }})
            })
        });
        let boot = BootId::new([37; 16]).unwrap();
        let worker_boot = BootId::new([38; 16]).unwrap();
        let mut parent =
            WorkerChannels::bind(&directory.path, "second", boot, worker_boot, false).unwrap();
        let mut worker =
            WorkerChannels::bind(&directory.path, "second", boot, worker_boot, true).unwrap();
        parent.connect(&directory.path, "second", false).unwrap();
        worker.connect(&directory.path, "second", true).unwrap();
        let script = Arc::new(RecordingScript {
            chats: Mutex::new(
                [
                    vec![
                        Ok(net::ChatEvent::ToolCalls(vec![net::ToolCall {
                            id: "stop-music".into(),
                            name: "play_relaxing_music".into(),
                            args: json!({}),
                            extra: None,
                        }])),
                        Ok(net::ChatEvent::Done),
                    ],
                    vec![Ok(net::ChatEvent::Done)],
                ]
                .into(),
            ),
            messages: Mutex::default(),
        });
        let (opened, mut opening) = tokio::sync::oneshot::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let services = Arc::new(WithSpare {
            script: script.clone(),
            listens: AtomicUsize::new(0),
            opened: Mutex::new(Some(opened)),
            cancelled: cancelled.clone(),
        });
        let settings = crate::tools::Settings {
            skills: Some(skills),
            requested_audio: true,
            ..Default::default()
        };
        let runner = requested_audio_runner(settings, None, None, false);
        let lease: Arc<Mutex<Option<crate::memory_scope::ScopeLease>>> = Arc::default();
        let kept_lease = lease.clone();
        let engine = Engine::new(
            services.clone(),
            Told::default,
            Arc::new(|_, _, _, _| panic!("the admitted Music runner owns dispatch")),
        )
        .with_memory_scope(
            boot,
            Arc::default(),
            Arc::new(|_, _, _, _, _| panic!("the admitted Music runner owns dispatch")),
            Some(Arc::new(|scope: &crate::memory_scope::ScopeLease| {
                Told::from(format!("context {:?}", scope.scope().unwrap().person()))
            })),
        )
        .requesting_audio(
            boot,
            Arc::new(move |context, call, scope| {
                *kept_lease.lock().unwrap() = Some(scope.clone());
                runner(context, call, scope)
            }),
        )
        .keeping_a_listener_ready(warm_stt)
        .announcing(false);
        struct RestoreThread(Option<crate::thread::Thread>);
        impl Drop for RestoreThread {
            fn drop(&mut self) {
                if let Some(saved) = self.0.take() {
                    *crate::thread::THREAD
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = saved;
                }
            }
        }
        // Declared before the runtime so its tasks stop before restoration,
        // including when a later assertion unwinds.
        let _restore_thread = RestoreThread(Some(std::mem::take(
            &mut *crate::thread::THREAD.lock().unwrap(),
        )));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let stopped = runtime.block_on(async {
            let at_us = monotonic_us();
            let at = MonoTime::from_micros(at_us);
            let mut owner = Controller::new(boot, at);
            owner.set_microphone_permission(at, Permission::Allowed).unwrap();
            let renew = |owner: &mut Controller| {
                let now = MonoTime::from_micros(monotonic_us());
                let until = MonoTime::from_micros(now.as_micros() + 100_000);
                owner.set_capture(now, CaptureState::RetainingUntil(until)).unwrap();
                owner.set_admission(now, AdmissionState::OpenUntil(until)).unwrap();
                owner.snapshot(now).unwrap()
            };
            renew(&mut owner);
            let request = owner.admit(
                MonoTime::from_micros(monotonic_us()), AdmittedInput::NewTurn,
            ).unwrap().turn();
            let snapshot = renew(&mut owner);
            let privacy_generation = snapshot.microphone_generation();
            parent.control.send(Control::Authority { snapshot }).unwrap();
            for input in [
                ProviderInput::Start {
                    request, session:1, privacy_generation, continues:false,
                    continues_cut_off:false, over_the_lamp:false, after_stop:false,
                    follows_on:false, over_the_lamps_voice:false,
                    heard_of_cut: None,
                },
                ProviderInput::Audio {
                    request, privacy_generation, sequence:1,
                    read_completed_at_us:monotonic_us(), samples:vec![1;160],
                },
                ProviderInput::End { request, privacy_generation, sound:false, cut_in:false },
            ] { parent.data.send(input).unwrap(); }
            let running = tokio::spawn(run(worker, boot, engine, None, None));
            let mut heartbeat = tokio::time::interval(Duration::from_millis(20));
            let context = tokio::time::timeout(Duration::from_secs(2), async {
                let mut context = None;
                let mut spare_pending = !warm_stt;
                loop {
                    if spare_pending && let Some(context) = context.take() {
                        break context;
                    }
                    tokio::select! {
                        received = &mut received, if context.is_none() => {
                            context = Some(received.unwrap());
                        }
                        opened = &mut opening, if !spare_pending => {
                            opened.expect("the warm STT spare must begin opening");
                            spare_pending = true;
                        }
                        _ = heartbeat.tick() => {
                            parent.control.send(Control::Authority { snapshot:renew(&mut owner) }).unwrap();
                            while let Some(event) = parent.data.receive::<ProviderOutput>().unwrap() {
                                assert!(!matches!(event, ProviderOutput::RequestedAudio { .. } | ProviderOutput::Audio { .. }));
                            }
                        }
                    }
                }
            }).await.expect("the Music RPC and enabled warm STT opening must both be pending");
            assert_eq!(services.listens.load(Ordering::SeqCst), 1 + usize::from(warm_stt));
            assert!(!cancelled.load(Ordering::SeqCst), "the spare is still opening before Stop");
            assert_eq!((context.request, context.provider_session, context.privacy_generation), (request,1,privacy_generation));
            assert_eq!(context.worker_boot, crate::process::boot_text(boot));
            assert_eq!(context.call_id, "stop-music");
            assert!(lease.lock().unwrap().as_ref().unwrap().is_current());
            let stopped = Instant::now();
            parent.control.send(Control::Stop).unwrap();
            tokio::time::timeout(Duration::from_millis(300), running).await
                .expect("Stop must end worker::run while the Music reply is held")
                .unwrap().unwrap();
            assert!(!lease.lock().unwrap().as_ref().unwrap().is_current());
            stopped
        });
        finish(runtime);
        let elapsed = stopped.elapsed();
        assert_eq!(
            cancelled.load(Ordering::SeqCst),
            warm_stt,
            "runtime teardown cancels the pending spare connection"
        );
        release.send(()).unwrap();
        let asked = peer.join().unwrap();
        assert_eq!(asked["name"], "play_relaxing_music");
        assert!(
            elapsed < Duration::from_millis(300),
            "Music shutdown waited {elapsed:?} for its RPC"
        );
        assert_eq!(
            script.messages.lock().unwrap().len(),
            1,
            "no chat round follows a late reply"
        );
        loop {
            match parent.data.receive::<ProviderOutput>() {
                Ok(Some(event)) => assert!(!matches!(
                    event,
                    ProviderOutput::RequestedAudio { .. } | ProviderOutput::Audio { .. }
                )),
                Ok(None) => break,
                // As in reference_link's terminal-notice test, Darwin reports
                // peer closure after its queued datagrams have been drained.
                Err(error) if error.kind() == io::ErrorKind::ConnectionReset => break,
                Err(error) => panic!("unexpected post-Stop receive failure: {error}"),
            }
        }
    }

    /// #960, lamp-4ace 08:47:27: the worker was held up (#784's SD/MMC
    /// stalls) while finishing a turn, and the authority queued meanwhile
    /// had lapsed when it was read. That ended the route and the voice.
    /// Lapsed snapshots are skipped and the next fresh one is taken.
    #[tokio::test]
    async fn authority_that_lapsed_in_the_queue_during_a_stall_is_skipped() {
        let boot = BootId::new([7; 16]).unwrap();
        let start_us = 10_000;
        let at = |us| MonoTime::from_micros(us);
        let mut owner = Controller::new(boot, at(start_us));
        owner
            .set_microphone_permission(at(start_us), Permission::Allowed)
            .unwrap();
        let renew = |owner: &mut Controller, now_us: u64| {
            let until = at(now_us + 100_000);
            owner
                .set_capture(at(now_us), CaptureState::RetainingUntil(until))
                .unwrap();
            owner
                .set_admission(at(now_us), AdmissionState::OpenUntil(until))
                .unwrap();
            owner.snapshot(at(now_us)).unwrap()
        };
        let mut engine = Engine::new(
            Arc::new(Silent),
            Told::default,
            Arc::new(|_, _, _, _| Box::pin(async { serde_json::Value::Null })),
        );
        let mut gate = Gate::new(boot, start_us);
        let first = renew(&mut owner, start_us);
        gate.install(start_us, first, &mut engine).unwrap();
        owner.admit(at(start_us), AdmittedInput::NewTurn).unwrap();
        // Sent every few ms while the worker was held up for 300 ms.
        let queued: Vec<_> = (1..=60)
            .map(|step| renew(&mut owner, start_us + step * 5_000))
            .collect();
        let read_us = start_us + 300_000;
        for snapshot in queued {
            gate.install(read_us, snapshot, &mut engine).unwrap();
        }
        assert_eq!(gate.last_control_us, read_us);
        assert_eq!(gate.skipped_expired, 0);
        gate.fresh(read_us).unwrap();
    }

    /// #2305's review: a camera close and a mute, as the worker installs
    /// them, tell the engine's privacy hook (the caller's face ids go,
    /// `Heeding::privacy_changed`); a renewed authority with neither does not.
    #[tokio::test]
    async fn a_camera_close_and_a_mute_are_told_as_a_privacy_change() {
        let boot = BootId::new([7; 16]).unwrap();
        let at = |us| MonoTime::from_micros(us);
        let mut owner = Controller::new(boot, at(10_000));
        owner
            .set_microphone_permission(at(10_000), Permission::Allowed)
            .unwrap();
        owner
            .set_camera_permission(at(10_000), Permission::Allowed)
            .unwrap();
        let told = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counted = told.clone();
        let mut engine = Engine::new(
            Arc::new(Silent),
            Told::default,
            Arc::new(|_, _, _, _| Box::pin(async { serde_json::Value::Null })),
        )
        .forgetting_on_privacy(Box::new(move || {
            counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }));
        let mut gate = Gate::new(boot, 10_000);
        let told_now = || told.load(std::sync::atomic::Ordering::Relaxed);
        for now_us in [10_000, 20_000] {
            gate.install(now_us, owner.snapshot(at(now_us)).unwrap(), &mut engine)
                .unwrap();
        }
        assert_eq!(told_now(), 0, "renewed, nothing changed");
        owner
            .set_camera_permission(at(30_000), Permission::Denied)
            .unwrap();
        gate.install(30_000, owner.snapshot(at(30_000)).unwrap(), &mut engine)
            .unwrap();
        assert_eq!(told_now(), 1, "the camera closed");
        owner
            .set_microphone_permission(at(40_000), Permission::Denied)
            .unwrap();
        gate.install(40_000, owner.snapshot(at(40_000)).unwrap(), &mut engine)
            .unwrap();
        assert_eq!(told_now(), 2, "a mute");
    }

    #[tokio::test]
    async fn requested_audio_pending_without_ipc_credit_rechecks_camera_scope_and_expiry() {
        const NOW: u64 = 10_000_000;
        let at = MonoTime::from_micros(NOW);
        for change in ["none", "denied", "reopened", "before_deadline", "expired"] {
            let directory = crate::process::SessionDirectory::create().unwrap();
            let boot = BootId::new([7; 16]).unwrap();
            let worker_boot = BootId::new([8; 16]).unwrap();
            let mut parent =
                WorkerChannels::bind(&directory.path, "second", boot, worker_boot, false).unwrap();
            let mut worker =
                WorkerChannels::bind(&directory.path, "second", boot, worker_boot, true).unwrap();
            parent.connect(&directory.path, "second", false).unwrap();
            worker.connect(&directory.path, "second", true).unwrap();
            let mut owner = Controller::new(boot, at);
            owner
                .set_microphone_permission(at, Permission::Allowed)
                .unwrap();
            owner
                .set_camera_permission(at, Permission::Allowed)
                .unwrap();
            let until = MonoTime::from_micros(NOW + 100_000);
            owner
                .set_capture(at, CaptureState::RetainingUntil(until))
                .unwrap();
            owner
                .set_admission(at, AdmissionState::OpenUntil(until))
                .unwrap();
            let request = owner.admit(at, AdmittedInput::NewTurn).unwrap().turn();
            let before = owner.snapshot(at).unwrap();
            let cache = Arc::new(crate::memory_scope::FaceCache::default());
            let mut engine = Engine::new(
                Arc::new(Silent),
                Told::default,
                Arc::new(|_, _, _, _| panic!("the fixture runs no tools")),
            )
            .with_memory_scope(
                boot,
                cache.clone(),
                Arc::new(|_, _, _, _, _| panic!("the fixture runs no tools")),
                None,
            )
            .requesting_audio(
                boot,
                Arc::new(|_, _, _| panic!("the fixture runs no tools")),
            );
            let mut gate = Gate::new(boot, NOW);
            gate.install(NOW, before, &mut engine).unwrap();
            let face = directory.path.join("face");
            std::fs::write(&face, "credit-fixture").unwrap();
            assert!(cache.replace(
                cache.begin_refresh(),
                crate::known_faces::FaceObservation::read(&face)
            ));
            engine.submit(ProviderInput::Start {
                request,
                session: 1,
                privacy_generation: before.microphone_generation(),
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
                heard_of_cut: None,
            });
            engine.submit(ProviderInput::End {
                request,
                privacy_generation: before.microphone_generation(),
                sound: false,
                cut_in: false,
            });
            engine.urgent.clear();
            let lease = engine
                .turn
                .as_ref()
                .unwrap()
                .memory_scope
                .as_ref()
                .unwrap()
                .lease();
            assert!(lease.scope().unwrap().person().is_some());
            // These are already-generated events; taking them does not retire
            // the turn. Both initial credits are spent on the real IPC channel.
            for event in [
                ProviderOutput::GenerationComplete { request },
                ProviderOutput::TurnComplete {
                    request,
                    idle: true,
                },
            ] {
                engine.shared.out.try_send(event).unwrap();
                let pending = engine.try_output().unwrap();
                assert!(
                    gate.send_or_discard(&engine, NOW, &pending, |event| worker
                        .data
                        .send_with(event, || NOW))
                        .unwrap()
                );
                assert!(
                    parent
                        .data
                        .receive_with::<ProviderOutput>(|| NOW)
                        .unwrap()
                        .is_some()
                );
            }
            let offer = lamp_msgs::RequestedAudioOffer {
                skills_boot: "skills-test".into(),
                definition_generation: 1,
                offer_id: 1,
                context: lamp_msgs::RequestedAudioContext {
                    worker_boot: crate::process::boot_text(boot),
                    provider_session: 1,
                    request,
                    privacy_generation: before.microphone_generation(),
                    call_id: "credit-call".into(),
                    issued_us: NOW,
                },
                skill: "relaxing-music".into(),
                asset: "/local/relaxing.wav".into(),
                sha256: "a".repeat(64),
                frames: 24_000,
                issued_us: NOW,
                start_before_us: NOW + lamp_msgs::REQUESTED_AUDIO_START_US,
                finish_before_us: NOW + lamp_msgs::REQUESTED_AUDIO_START_US + 1_000_000,
            };
            engine
                .shared
                .out
                .try_send(ProviderOutput::RequestedAudio {
                    request,
                    session: 1,
                    offer,
                })
                .unwrap();
            // This is the event held outside Engine when the worker has no
            // credit. The later retry must not trust that earlier dequeue.
            let pending = engine.try_output().unwrap();
            assert!(
                !gate
                    .send_or_discard(&engine, NOW, &pending, |event| worker
                        .data
                        .send_with(event, || NOW))
                    .unwrap()
            );
            assert!(
                parent
                    .data
                    .receive_with::<ProviderOutput>(|| NOW)
                    .unwrap()
                    .is_none()
            );
            if matches!(change, "denied" | "reopened") {
                owner.set_camera_permission(at, Permission::Denied).unwrap();
                if change == "reopened" {
                    owner
                        .set_camera_permission(at, Permission::Allowed)
                        .unwrap();
                }
                gate.install(NOW, owner.snapshot(at).unwrap(), &mut engine)
                    .unwrap();
            }
            let retry_us = match change {
                "before_deadline" => NOW + lamp_msgs::REQUESTED_AUDIO_START_US - 1,
                "expired" => NOW + lamp_msgs::REQUESTED_AUDIO_START_US,
                _ => NOW,
            };
            if retry_us != NOW {
                // Renew the same owner and leases through the credit wait.
                // Only the offer's immutable start deadline may expire.
                for renewed_us in ((NOW + 50_000)..retry_us)
                    .step_by(50_000)
                    .chain(std::iter::once(retry_us))
                {
                    let renewed_at = MonoTime::from_micros(renewed_us);
                    let until = MonoTime::from_micros(renewed_us + 100_000);
                    owner
                        .set_capture(renewed_at, CaptureState::RetainingUntil(until))
                        .unwrap();
                    owner
                        .set_admission(renewed_at, AdmissionState::OpenUntil(until))
                        .unwrap();
                    gate.install(renewed_us, owner.snapshot(renewed_at).unwrap(), &mut engine)
                        .unwrap();
                }
            }
            gate.fresh(retry_us).unwrap();
            gate.may_send(retry_us, &pending).unwrap();
            let after = gate.guard.state().unwrap();
            assert_eq!(before.owner(), after.owner());
            assert_eq!(
                before.microphone_generation(),
                after.microphone_generation()
            );
            assert!(request > gate.retired_through);
            assert_eq!(lease.is_current(), !matches!(change, "denied" | "reopened"));
            let delivered = matches!(change, "none" | "before_deadline");
            gate.flow.grant(3).unwrap();
            assert!(
                gate.send_or_discard(&engine, retry_us, &pending, |event| worker
                    .data
                    .send_with(event, || retry_us))
                    .unwrap()
            );
            assert_eq!(
                matches!(
                    parent
                        .data
                        .receive_with::<ProviderOutput>(|| retry_us)
                        .unwrap(),
                    Some(ProviderOutput::RequestedAudio { .. })
                ),
                delivered,
                "{change}"
            );
            assert_eq!(
                gate.flow.can_send(),
                !delivered,
                "a discard spends no credit"
            );
            gate.flow.grant(4).unwrap();
            assert!(
                gate.send_or_discard(
                    &engine,
                    retry_us,
                    &ProviderOutput::GenerationComplete { request },
                    |event| worker.data.send_with(event, || retry_us)
                )
                .unwrap()
            );
            assert!(
                matches!(parent.data.receive_with::<ProviderOutput>(|| retry_us).unwrap(),
                Some(ProviderOutput::GenerationComplete { request: got }) if got == request)
            );
        }
    }

    #[tokio::test]
    async fn a_new_owner_retires_the_old_turn_and_its_late_input() {
        let now_us = 10_000;
        let at = MonoTime::from_micros(now_us);
        let boot = BootId::new([7; 16]).unwrap();
        let mut owner = Controller::new(boot, at);
        owner
            .set_microphone_permission(at, Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(now_us + 100_000);
        owner
            .set_capture(at, CaptureState::RetainingUntil(until))
            .unwrap();
        owner
            .set_admission(at, AdmissionState::OpenUntil(until))
            .unwrap();
        let mut engine = Engine::new(
            Arc::new(Silent),
            Told::default,
            Arc::new(|_, _, _, _| Box::pin(async { serde_json::Value::Null })),
        );
        let mut gate = Gate::new(boot, now_us);
        let first = owner.admit(at, AdmittedInput::NewTurn).unwrap();
        let state = owner.snapshot(at).unwrap();
        gate.install(now_us, state, &mut engine).unwrap();
        let start = |request| ProviderInput::Start {
            request,
            session: 1,
            privacy_generation: state.microphone_generation(),
            continues: false,
            continues_cut_off: false,
            over_the_lamp: false,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice: false,
            heard_of_cut: None,
        };
        assert!(gate.admits(now_us, &start(first.turn())).unwrap());
        engine.submit(start(first.turn()));
        assert!(gate.may_send(now_us, &engine.try_output().unwrap()).is_ok());
        assert!(engine.turn.is_some());

        let second = owner.admit(at, AdmittedInput::Interruption).unwrap();
        gate.install(now_us, owner.snapshot(at).unwrap(), &mut engine)
            .unwrap();
        assert!(engine.turn.is_none());
        assert!(!gate.admits(now_us, &start(first.turn())).unwrap());
        assert!(gate.admits(now_us, &start(second.turn())).unwrap());
        // Output of the old owner would be refused at the boundary.
        let late = ProviderOutput::GenerationComplete {
            request: first.turn(),
        };
        assert!(gate.may_send(now_us, &late).is_err());
    }

    /// `LAMP_SAID_AS_HEARD`: a turn's end holds its hand-on to memory, and an
    /// older one held goes when a newer turn ends; without the switch nothing
    /// is held.
    #[test]
    fn a_turns_end_holds_its_hand_on_to_memory_with_the_switch() {
        let end = |request| ProviderOutput::TurnComplete {
            request,
            idle: true,
        };
        let mut held = crate::said_as_heard::HeldHandOn::default();
        super::keep_scoped(&end(901), None, None, Some(&mut held));
        super::keep_scoped(&end(902), None, None, Some(&mut held));
        assert_eq!(held.release(), Some(902), "the newest is held");
        assert_eq!(held.release(), None, "the older one went");
        super::keep_scoped(&end(903), None, None, None);
        assert_eq!(held.release(), None, "off: nothing held");
    }

    /// #2137's redesign, step 5: the second worker's own path, not only
    /// `sign_in`'s functions. A sign-in's news marks the request it is
    /// written in, and the lamp's words of that request leave the engine as
    /// one line however they were split; another request's are untouched.
    #[test]
    fn the_second_worker_sends_a_sign_ins_words_as_one_line() {
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        crate::sign_in::forget();
        let mut engine = Engine::new(
            Arc::new(Silent),
            Told::default,
            Arc::new(|_, _, _, _| Box::pin(async { serde_json::Value::Null })),
        );
        let written = |request: u64, text: String| ProviderInput::Written {
            request,
            session: 1,
            privacy_generation: 1,
            text,
            look: false,
            search: false,
            quoted: false,
        };
        sign_in_written(&written(3, "Claude Code just finished.".into()));
        sign_in_written(&written(
            4,
            format!(
                "{}Ask whether to sign in as someone at example dot com.",
                crate::sign_in::NEWS_TAG
            ),
        ));
        let words = |request: u64, text: &str| ProviderOutput::Transcript {
            request: Some(request),
            text: text.into(),
            finished: false,
        };
        engine.queue_for_test(words(4, "Shall I sign in as someone at"));
        engine.queue_for_test(words(4, " example dot com?"));
        engine.queue_for_test(words(3, "It wrote the story."));
        let said: Vec<String> = std::iter::from_fn(|| next_output(&mut engine))
            .map(|event| match event {
                ProviderOutput::Transcript { text, .. } => text,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(said, [crate::sign_in::SAID_LINE, "", "It wrote the story."]);
        crate::sign_in::forget();
    }

    fn strings(flags: &[&str]) -> Vec<String> {
        flags.iter().map(|flag| (*flag).to_owned()).collect()
    }

    #[test]
    fn a_written_request_goes_to_the_chat_as_a_note_and_silence_as_nothing() {
        let written = messages(crate::thread::Recent {
            written: Some("Say ORCHID.".into()),
            person: String::new(),
            lamp: "ORCHID.".into(),
        });
        assert_eq!(
            written,
            [
                Message::system(
                    "(A note you were given in writing, not words anyone said: Say ORCHID.)"
                ),
                Message::assistant("ORCHID."),
            ]
        );
        let spoken = messages(crate::thread::Recent {
            written: None,
            person: "Hi.".into(),
            lamp: "Hello.".into(),
        });
        assert_eq!(spoken, [Message::user("Hi."), Message::assistant("Hello.")]);
    }

    /// The reviewer-lead on #2675: an add-on's quoted line, and the lamp's
    /// words of it, are never kept for the thread, so the person's next
    /// turn (with its functions) carries no note of it; the lamp's own
    /// written request still is.
    #[test]
    fn a_quoted_line_is_never_kept_for_a_later_turn() {
        let now = std::time::Instant::now();
        let written = |request: u64, quoted: bool| ProviderInput::Written {
            request,
            session: 1,
            privacy_generation: 1,
            text: format!("note {request}"),
            look: false,
            search: false,
            quoted,
        };
        for scopes_context in [false, true] {
            let mut thread = crate::thread::Thread::new();
            // An onboarding sentence in hand stays marked.
            thread.count_only(Some(9));
            keep_written(&mut thread, &written(1, true), scopes_context);
            thread.lamp(1, "Music is playing.", now);
            thread.lamp(9, "Welcome.", now);
            // The person's words after it are kept (a Start clears the
            // sentence, as the engine's does).
            thread.count_only(None);
            thread.person("And the light?", true, now);
            thread.lamp(2, "It's on.", now);
            keep_written(&mut thread, &written(3, false), scopes_context);
            thread.lamp(3, "Done.", now);
            let recent = thread.recent_for(None, now, std::time::Duration::from_secs(300), 6);
            let said = format!("{recent:?}");
            assert!(!said.contains("note 1"), "{said}");
            assert!(!said.contains("Music is playing"), "{said}");
            assert!(!said.contains("Welcome"), "{said}");
            assert!(said.contains("And the light?"), "{said}");
            assert!(said.contains("It's on."), "{said}");
            assert_eq!(
                recent.last().and_then(|last| last.written.as_deref()),
                Some("note 3"),
                "{said}"
            );
        }
    }

    /// #1722: this route reads who work can go to as the Live route does,
    /// so `start_agent_task` offers `who` for the tasks part's targets and
    /// "ask Codex" goes to Codex. Before they are read, the work goes to
    /// `LAMP_TASKS_TARGET`, as before.
    #[test]
    fn the_second_worker_reads_who_work_can_go_to_as_live_does() {
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _staff = crate::agent_tasks::tests::STAFF_HELD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let directory = std::env::temp_dir().join(format!("second-staff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let seen = crate::agent_tasks::tests::tasks_part(
            &directory,
            vec![(
                "request.sock",
                serde_json::json!({"ok":true,"targets":[{"alias":"office-claude","engine":"claude"},{"alias":"office-codex","engine":"codex"}]}),
            )],
        );
        let flags = strings(&[
            crate::agent_tasks::FLAG,
            directory.to_str().unwrap(),
            crate::agent_tasks::TARGET_FLAG,
            "office-claude",
            crate::agent_tasks::WHO_FLAG,
        ]);
        let place = crate::agent_tasks::take(&flags).unwrap().0.unwrap();
        // Where "ask Codex" is prepared, through a tasks part of its own.
        let prepared_for = |tag: &str| -> serde_json::Value {
            let at =
                std::env::temp_dir().join(format!("second-staff-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&at);
            std::fs::create_dir_all(&at).unwrap();
            let sent = crate::agent_tasks::tests::tasks_part(
                &at,
                vec![
                    (
                        "request.sock",
                        serde_json::json!({"ok":true,"store_id":"s1","revision":0,"summary":{"session_id":"sess"}}),
                    ),
                    (
                        "request.sock",
                        serde_json::json!({"ok":true,"id":7,"prepared_digest":"abc","revision":1}),
                    ),
                ],
            );
            let here = crate::agent_tasks::Place {
                directory: at.clone(),
                ..place.clone()
            };
            let call = lamp_gemini::ToolCall {
                id: "c".into(),
                name: crate::agent_tasks::START.into(),
                args: serde_json::json!({"task":"Write a haiku about coffee","who":"Codex"}),
            };
            let (told, _) =
                crate::agent_tasks::run(&here, &call, 3, true, std::time::Duration::from_secs(5));
            assert_eq!(told["prepared"], true, "{told}");
            let wait = std::time::Duration::from_secs(5);
            // The status first, then the preparation.
            sent.recv_timeout(wait).unwrap();
            let target = sent.recv_timeout(wait).unwrap()["target"].clone();
            let _ = std::fs::remove_dir_all(&at);
            target
        };
        let before = crate::agent_tasks::tools(&place);
        assert!(before[0].parameters.as_ref().unwrap()["properties"]["who"].is_null());
        // No staff read: the target the voice was given, as before.
        assert_eq!(prepared_for("unread"), "office-claude");
        start_tasks(&crate::tools::Settings {
            tasks: Some(place.clone()),
            ..crate::tools::Settings::default()
        });
        assert_eq!(
            seen.recv_timeout(std::time::Duration::from_secs(5))
                .unwrap(),
            serde_json::json!({"op":"targets"})
        );
        let told = crate::agent_tasks::tools(&place);
        let codex = prepared_for("read");
        crate::agent_tasks::tests::forget_staff();
        let _ = std::fs::remove_dir_all(&directory);
        assert_eq!(
            told[0].parameters.as_ref().unwrap()["properties"]["who"]["enum"],
            serde_json::json!(["Claude Code", "Codex"])
        );
        // Two targets read: "ask Codex" goes to Codex.
        assert_eq!(codex, "office-codex");
    }

    /// Tests' map on #1712 (20:11), (2): the task switches
    /// (`LAMP_TASKS_WHO`, `_ASKED`, `_ACCOUNTS`, `_MACHINE_NAMES` and
    /// `LAMP_TOOL_LEAD_IN`) reach the second worker as Live's do. The
    /// coordinator gives it the Live worker's flags (`coordinator/second.rs`,
    /// `arguments`), and it parses them with the same parser into its tools'
    /// place. What each one does is `agent_tasks`', the same code on either
    /// route.
    #[test]
    fn the_second_worker_takes_the_task_switches_as_live_does() {
        let dir = Path::new("/run/lamp-voice");
        let (_, _, files, _) = parse_flags(
            dir,
            &strings(&[
                "--tools",
                crate::agent_tasks::FLAG,
                "/run/lampos-tasks",
                crate::agent_tasks::WHO_FLAG,
                crate::agent_tasks::LEAD_IN_FLAG,
                crate::agent_tasks::ASKED_FLAG,
                crate::agent_tasks::ACCOUNTS_FLAG,
                crate::agent_tasks::NAMES_FLAG,
            ]),
        )
        .unwrap();
        let place = files.tools.tasks.expect("the tasks part");
        assert_eq!(place.directory, Path::new("/run/lampos-tasks"));
        assert!(place.who && place.lead_in && place.asked);
        assert!(place.accounts && place.machine_names);
        let (_, _, files, _) = parse_flags(
            dir,
            &strings(&["--tools", crate::agent_tasks::FLAG, "/run/lampos-tasks"]),
        )
        .unwrap();
        let place = files.tools.tasks.expect("the tasks part");
        assert!(!place.who && !place.lead_in && !place.asked);
        assert!(!place.accounts && !place.machine_names);
    }

    /// Tests' map on #1712 (20:11), (2): `LAMP_MEMORY_DAYS` on the second
    /// route. The voice hands it to this worker (`process.rs`, the table of
    /// worker environments: `provider` and `second`). With it, the memory's
    /// note is asked once a session, by that session's number, and kept for
    /// the session's turns (`Sessions::frozen`), as a Live session is told
    /// it once; a new session, or someone else at the lamp, asks again with
    /// the next number. Without it every turn asks, as session 0, and
    /// `frozen` is not used.
    #[test]
    fn with_memory_days_the_note_is_asked_once_a_session_and_kept_for_its_turns() {
        let mut sessions = Sessions::default();
        let now = Instant::now();
        let mut asked = Vec::new();
        let mut note = |changed: bool| {
            sessions.frozen(changed, now, |session| {
                asked.push(session);
                crate::remember::SessionNote {
                    text: format!("note {session}"),
                    complete: true,
                    cause: None,
                }
            })
        };
        assert_eq!(note(false), "note 1", "the first turn asks");
        assert_eq!(note(false), "note 1", "the same session keeps it");
        assert_eq!(note(true), "note 2", "a new session asks again");
        assert_eq!(note(false), "note 2");
        assert_eq!(asked, [1, 2]);
    }

    /// A note asked as its turn is told (a run's first line) is waited for
    /// [`NOTE_TOLD_WITHIN`] from the ask, past the note's 300 ms: on
    /// lamp-4ace all 56 such notes in two days came late (a median 443 ms,
    /// at most 610), so a fresh session's first question went without what
    /// the lamp keeps. One asked ahead, as a line begins, keeps its 300 ms;
    /// a longer wait given for a loaded test machine stands for both.
    #[test]
    fn a_note_asked_as_the_turn_is_told_is_waited_for_longer_than_one_asked_ahead() {
        let ask: AskNote = Arc::new(|_, _, _, _| crate::remember::NoteRead::unavailable());
        let ahead = Ahead::asking(Arc::new(Mutex::new(Sessions::default())), None, ask.clone());
        let now = Instant::now();
        let (_, until, _) = ahead
            .ready_now(None, now)
            .expect("a run's first turn asks its note");
        assert_eq!(until.duration_since(now), NOTE_TOLD_WITHIN);
        let first = crate::remember::FIRST_READ;
        let asked = |ahead: bool| NoteAsk::start(1, None, (now, 1), &ask, ahead);
        assert_eq!(asked(true).within(first), first);
        assert_eq!(asked(false).within(first), NOTE_TOLD_WITHIN);
        let loaded = Duration::from_secs(2);
        assert_eq!(asked(true).within(loaded), loaded);
        assert_eq!(asked(false).within(loaded), loaded);
    }

    /// #1693 (1), `LAMP_MEMORY_REOPEN_QUIET`: the same person back after ten
    /// quiet minutes (`thread::REMEMBERED_FOR`) is asked their note again,
    /// as a new session, so an open thread due by then can be told; under
    /// ten minutes, or with the switch off, the run of turns keeps its note.
    /// Quiet is counted from the last turn told, so the turns of one long
    /// conversation, and a turn's own second ask (its prepared chat, then
    /// its rounds), never reopen it. On a test clock: `Instant`s set ahead.
    #[test]
    fn after_ten_quiet_minutes_the_same_persons_next_turn_asks_the_note_again() {
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let quiet = crate::thread::REMEMBERED_FOR.as_secs();
        let asked_at = |reopen: bool, turns: &[u64]| {
            let mut sessions = Sessions::reopening_after_quiet(reopen);
            let mut asked = Vec::new();
            for &turn in turns {
                let changed = sessions.quiet_long_enough(at(turn));
                sessions.frozen(changed, at(turn), |session| {
                    asked.push(session);
                    crate::remember::SessionNote {
                        text: format!("note {session}"),
                        complete: true,
                        cause: None,
                    }
                });
            }
            asked
        };
        // Back after ten quiet minutes: asked again, once, and the turns
        // after it, and the same turn's second ask, keep that note.
        assert_eq!(asked_at(true, &[0, quiet, quiet + 1, quiet + 30]), [1, 2]);
        // Just under ten minutes: the same run of turns.
        assert_eq!(asked_at(true, &[0, quiet - 1]), [1]);
        // A long conversation, a turn every two minutes for half an hour.
        let long: Vec<u64> = (0..=15).map(|turn| turn * 120).collect();
        assert_eq!(asked_at(true, &long), [1]);
        // Two quiet gaps, two reopens.
        assert_eq!(asked_at(true, &[0, quiet, 2 * quiet + 5]), [1, 2, 3]);
        // Off: as before, whatever the quiet.
        assert_eq!(asked_at(false, &[0, quiet, 10 * quiet]), [1]);
        assert!(!Sessions::default().reopen_quiet, "off unless set");
    }

    /// Tests' map on #1712 (20:11), (2): `LAMP_ROOM_FILE` on the second route
    /// as the only voice. The room this route keeps (`keeping_room`) goes to
    /// its file at the worker's tick, and the next worker in the same boot
    /// starts from it: a busy room is still busy. The room is kept under the
    /// machine's boot (`room_told::MACHINE_BOOT_FILE`), which only Linux has,
    /// so elsewhere this says so and checks nothing; on Linux the file must be
    /// there. In tests the room is
    /// the thread's own, so each worker is a thread of its own.
    #[test]
    fn as_the_only_voice_the_room_goes_to_its_file_and_the_next_worker_starts_from_it() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        // Skipped by the system, not by the file: on Linux a missing boot
        // file fails here, so a pass there means this ran.
        if !cfg!(target_os = "linux") {
            eprintln!("not Linux: no machine boot file, the room is not kept, nothing checked");
            return;
        }
        assert!(
            Path::new(crate::room_told::MACHINE_BOOT_FILE).exists(),
            "Linux has {}",
            crate::room_told::MACHINE_BOOT_FILE
        );
        let dir = std::env::temp_dir().join(format!(
            "lamp-second-room-file-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("room.json");
        let first = path.clone();
        std::thread::spawn(move || {
            let mut tick = super::keeping_room(false, false, false, Some(&first), None);
            let now = monotonic_us();
            {
                let mut room = crate::room_told::room().lock().unwrap();
                for request in 1..=3 {
                    room.heard(request, now);
                    room.ended(request, now);
                    room.unanswered(request, now);
                }
                assert_eq!(room.at(now), crate::room_told::Room::Busy);
            }
            tick(now);
        })
        .join()
        .unwrap();
        assert!(path.exists(), "the room went to its file");
        let second = path.clone();
        let taken = std::thread::spawn(move || {
            let _tick = super::keeping_room(false, false, false, Some(&second), None);
            let mut room = crate::room_told::room().lock().unwrap();
            room.at(monotonic_us())
        })
        .join()
        .unwrap();
        assert_eq!(
            taken,
            crate::room_told::Room::Busy,
            "the next worker's room"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Tests' map on #1712 (20:11), (2): `LAMP_ATTENTION=busy-near` on the
    /// second route. With `--attention-near` this route samples the room
    /// microphone's rise over its quiet from the bus (`ears/level`,
    /// `hear_levels`, every 100 ms), and a line during which it rose
    /// `NEAR_RISE_DB` or more is near (`signs`). One whose rise was the
    /// lamp's own voice is not, nor one in a quiet room. On the real clock,
    /// about a second.
    #[test]
    fn with_attention_near_the_rooms_rise_on_the_bus_makes_a_line_near() {
        // A short root: a Unix socket's address holds 103 bytes.
        let root = std::path::PathBuf::from(format!("/tmp/lsn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut ears = bus
            .publisher_declared(
                lamp_msgs::RoomSound::TOPIC,
                &lamp_msgs::RoomSound::declaration(),
                "ears",
            )
            .unwrap();
        let mut heeding = super::Heeding::new(true, false, None, None);
        heeding.hear_levels(&root, true);
        let floor = -50.0;
        // A line begins once its first level is the bus's newest, so no
        // sample taken in it can be the line's before.
        let mut line = |rise: f32, own_voice: bool| {
            let level = lamp_msgs::RoomSound {
                level_db: floor + rise,
                floor_db: Some(floor),
                own_voice,
            };
            ears.publish_json(monotonic_us(), &level).unwrap();
            let began = monotonic_us();
            for _ in 0..8 {
                std::thread::sleep(Duration::from_millis(60));
                ears.publish_json(monotonic_us(), &level).unwrap();
            }
            (began, monotonic_us())
        };
        let near = line(crate::attention::NEAR_RISE_DB + 3.0, false);
        let own = line(crate::attention::NEAR_RISE_DB + 3.0, true);
        let quiet = line(1.0, false);
        assert!(heeding.signs(9, near).near, "risen: near");
        assert!(!heeding.signs(10, own).near, "its own voice: not near");
        assert!(!heeding.signs(11, quiet).near, "quiet: not near");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Tests' map on #1712 (20:11), (1): `LAMP_ADDRESSED` on the second
    /// route. In a busy room a line with no name, no conversation going on
    /// and nobody seen turned to the lamp is held; with `--addressed`, the
    /// same line said loud at the desk while a close face was turned to the
    /// lamp is let out as said to it, and the judgement is published once on
    /// `attention/addressed`, as on the Live route. Said from close with the
    /// face turned away, it is still held. On the real clock, about two
    /// seconds.
    #[test]
    fn with_addressed_a_line_said_close_and_facing_is_let_out_in_a_busy_room() {
        // A short root: a Unix socket's address holds 103 bytes.
        let root = std::path::PathBuf::from(format!("/tmp/lsa-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut ears = bus
            .publisher_declared(
                lamp_msgs::RoomSound::TOPIC,
                &lamp_msgs::RoomSound::declaration(),
                "ears",
            )
            .unwrap();
        let mut eyes = bus
            .publisher_declared(
                lamp_msgs::Faces::TOPIC,
                &lamp_msgs::Faces::declaration(),
                "eyes",
            )
            .unwrap();
        let mut told = bus
            .subscribe_typed(
                lamp_msgs::Addressed::TOPIC,
                "test",
                lamp_msgs::Addressed::TYPE,
            )
            .unwrap();
        let face = |facing: bool| lamp_msgs::Faces {
            faces: vec![lamp_msgs::Face {
                id: "f".into(),
                xmin: 0.4,
                ymin: 0.0,
                xmax: 0.7,
                ymax: 0.3,
                confidence: 0.9,
                yaw: None,
                facing: Some(facing),
                speaking: None,
            }],
        };
        let busy = || super::Signs {
            room: crate::room_told::Room::Busy,
            going_on: false,
            lamp_asked: false,
            turned: None,
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let heard = "what time is it in Tokyo now";
        let mut addressed = super::Heeding::new(true, false, None, None);
        addressed.judging_addressed(&root);
        let mut without = super::Heeding::new(true, false, None, None);
        // A line said 3 dB over the near mark, a face turned to the lamp or
        // away from it in every look.
        let mut line = |facing: bool| {
            let level = lamp_msgs::RoomSound {
                level_db: -50.0 + crate::attention::NEAR_RISE_DB + 3.0,
                floor_db: Some(-50.0),
                own_voice: false,
            };
            let began = monotonic_us();
            for _ in 0..8 {
                ears.publish_json(monotonic_us(), &level).unwrap();
                eyes.publish_json(monotonic_us(), &face(facing)).unwrap();
                std::thread::sleep(Duration::from_millis(120));
            }
            (began, monotonic_us())
        };
        let away = line(false);
        assert!(
            addressed.judge(1, away, heard, busy()).is_some(),
            "said close, turned away: held"
        );
        // Past the looks taken before a line, which count for it.
        std::thread::sleep(Duration::from_micros(crate::addressed::LEAD_US + 200_000));
        let facing = line(true);
        assert!(
            without.judge(2, facing, heard, busy()).is_some(),
            "without the switch: held"
        );
        assert_eq!(
            addressed.judge(2, facing, heard, busy()),
            None,
            "said close, facing: let out"
        );
        let published: Vec<lamp_msgs::Addressed> = std::iter::from_fn(|| {
            told.recv(Duration::from_millis(200))
                .ok()
                .flatten()
                .and_then(|message| message.parse().ok())
        })
        .collect();
        let judged: Vec<(u64, bool)> = published
            .iter()
            .map(|said| (said.request, said.addressed))
            .collect();
        assert_eq!(judged, [(1, false), (2, true)], "each published once");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Tests' map on #1712 (20:11), (1): `LAMP_NAME_SIGNAL` on the second
    /// route. Spoken turns are offered `heard_my_name` beside the lamp's
    /// functions, at once even with `LAMP_SECOND_FEW_TOOLS`; its call is
    /// answered as declined with nothing run, and kept for the line that
    /// says the answer's first sound.
    #[tokio::test]
    async fn with_the_name_signal_a_spoken_turn_is_offered_heard_my_name_and_its_call_does_nothing()
    {
        let name = crate::attention::HEARD_MY_NAME;
        let settings = crate::tools::Settings {
            decline: true,
            ..Default::default()
        };
        let declared = crate::tools::declared(&settings);
        let lamps: Vec<String> = declared.iter().map(|tool| tool.name.clone()).collect();
        let declare = super::with_name_signal(Arc::new(move || declared.clone()));
        let offered: Vec<String> = declare().into_iter().map(|tool| tool.name).collect();
        assert_eq!(offered, [lamps, vec![name.to_owned()]].concat());
        assert!(super::given_at_once(name), "given with the few tools too");
        let answered = super::run_call(
            crate::tools::Settings::default(),
            None,
            None,
            (41, true, false),
            lamp_gemini::ToolCall {
                id: String::new(),
                name: name.to_owned(),
                args: serde_json::json!({}),
            },
            None,
            None,
        )
        .await;
        assert_eq!(answered.told, crate::tools::declined());
        assert!(answered.requested_audio.is_none());
        assert_eq!(
            super::NAMED.lock().unwrap().map(|(request, _)| request),
            Some(41),
            "kept for the first sound's line"
        );
    }

    #[test]
    fn the_health_fact_is_empty_with_the_switch_off_and_when_nothing_is_known() {
        assert_eq!(health_fact(None), "");
        let kept = std::sync::Mutex::new(crate::health_told::HealthTold::<()>::default());
        assert_eq!(health_fact(Some(&kept)), "");
    }

    #[test]
    fn the_second_worker_takes_the_live_workers_flags_but_keep_link() {
        let dir = Path::new("/run/lamp-voice");
        let flags = strings(&[
            "--provider-continuation",
            "kept-start",
            "--thread-file",
            "/run/lamp-voice/thread.json",
            "--session-fact",
            "--room-facts",
            "--air-facts",
            "--health-facts",
            "--tools",
            "--actions-dir",
            "/run/lamp-exec",
            "--memory-socket",
            "/run/lamp-memory/memory.sock",
            "--clock",
            "--attention",
            "--attention-busy",
            "--name-signal",
            "--name-note",
            "--bus-root",
            "/run/lampos-bus",
        ]);
        let (_, _, files, visual) = parse_flags(dir, &flags).unwrap();
        assert_eq!(
            files.thread.as_deref(),
            Some(Path::new("/run/lamp-voice/thread.json"))
        );
        assert!(files.tools.decline && files.tools.clock && files.attention);
        assert_eq!(
            files.tools.memory.as_deref(),
            Some(Path::new("/run/lamp-memory/memory.sock"))
        );
        assert!(visual.is_none());
        assert!(parse_flags(dir, &[]).is_ok());
        for refused in [
            strings(&["--keep-link"]),
            strings(&["--tools", "--unknown"]),
            strings(&["--clock", "--tools"]),
        ] {
            assert!(parse_flags(dir, &refused).is_err(), "{refused:?}");
        }
    }

    /// #2539 (`LAMP_SECOND_FACE_NAME`): a turn that is the known face's own
    /// person is told their name, as the Live route tells it; the room's
    /// turn (or a line narrowed to the room), another person's, a face gone,
    /// no eyes, and the switch off are told none. Made-up names.
    #[test]
    fn a_turn_of_the_known_persons_own_is_told_their_name() {
        let dir = std::env::temp_dir().join(format!("lv-known-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let known = dir.join("known");
        std::fs::write(&known, "Pippa").unwrap();
        let pippa = crate::known_faces::memory_person("Pippa");
        let told = known_name_told(true, Some(&known), Some(&pippa));
        assert_eq!(
            told,
            "\n\n(The person at the lamp now is Pippa: you know their face.)"
        );
        // After the memory in the turn's system message, so "what's my
        // name?" is asked with it.
        let message = system_message(
            "You are a lamp.",
            None,
            &format!("\n\nYour memory\n- tea, no sugar{told}"),
            None,
            Some(&pippa),
        );
        assert!(
            message
                .text
                .contains("- tea, no sugar\n\n(The person at the lamp now is Pippa"),
            "{}",
            message.text
        );
        let jim = crate::known_faces::memory_person("Jim");
        for (on, who, person) in [
            // The room's turn, or a line narrowed to the room.
            (true, Some(known.as_path()), None),
            // Someone else's turn.
            (true, Some(known.as_path()), Some(jim.as_str())),
            // The switch off: as today.
            (false, Some(known.as_path()), Some(pippa.as_str())),
            // No eyes.
            (true, None, Some(pippa.as_str())),
        ] {
            assert_eq!(
                known_name_told(on, who, person),
                "",
                "{on} {who:?} {person:?}"
            );
        }
        // The face gone: no known file.
        std::fs::remove_file(&known).unwrap();
        assert_eq!(known_name_told(true, Some(&known), Some(&pippa)), "");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A finished turn goes to the thread, its file and the journal, and
    /// the next turn's system message carries the character, the memory's
    /// note and that thread. In a child process: the thread and the
    /// memory are this process's globals.
    #[test]
    fn a_turn_is_kept_and_told_to_the_next() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        use std::io::{BufRead, BufReader, Write};
        const CHILD: &str = "LAMP_SECOND_KEEP_TEST_CHILD";
        if std::env::var(CHILD).ok().as_deref() != Some("1") {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "second_provider::worker::tests::a_turn_is_kept_and_told_to_the_next",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env_remove(crate::remember::DAYS_ENVIRONMENT)
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success() && stdout.contains("1 passed"),
                "{stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let dir = std::env::temp_dir().join(format!("lv-second-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let socket = dir.join("memory.sock");
        let file = dir.join("thread.json");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        // The memory: the kept list, then the journal's first append.
        let memory = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                let answer = match request["op"].as_str() {
                    Some("kept") => {
                        serde_json::json!({"ok":true,"entries":[{"text":"coffee black"}]})
                    }
                    _ => serde_json::json!({"ok":true}),
                };
                writeln!(stream, "{answer}").unwrap();
                if request["op"] == "append" {
                    return request;
                }
            }
            unreachable!()
        });
        crate::remember::start(socket.clone(), None);
        let request = 7;
        for event in [
            ProviderOutput::Transcript {
                request: None,
                text: "What should I drink?".into(),
                finished: true,
            },
            ProviderOutput::Transcript {
                request: Some(request),
                text: "Try some tea.".into(),
                finished: false,
            },
            ProviderOutput::Transcript {
                request: Some(request),
                text: "It is calm.".into(),
                finished: false,
            },
            ProviderOutput::TurnComplete {
                request,
                idle: true,
            },
        ] {
            keep(&event, Some(&file));
        }
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(
            written.contains("What should I drink?")
                && written.contains("Try some tea. It is calm.")
        );
        let journalled = memory.join().unwrap();
        assert_eq!(journalled["heard"], "What should I drink?");
        assert_eq!(journalled["said"], "Try some tea. It is calm.");

        let told = system_message(
            "You are a lamp.",
            Some(" It is noon."),
            &crate::remember::told(),
            Some(&file),
            None,
        );
        assert!(
            told.text
                .starts_with("You are a lamp. It is noon.\n\nYour memory\n")
        );
        assert!(told.text.contains("- coffee black\n"));
        assert!(
            told.text
                .contains("Person: What should I drink?\nYou: Try some tea. It is calm.\n")
        );
        assert_eq!(
            told.history,
            [
                Message::user("What should I drink?"),
                Message::assistant("Try some tea. It is calm.")
            ]
        );
        // With what stays first (#1712, the first builder at 18:56): the
        // character alone first; the memory, the time and the thread after
        // the exchanges, which are the same.
        let steady = steady_first_message(
            "You are a lamp.".into(),
            &format!("{} It is noon.", crate::remember::told()),
            Some(&file),
            None,
        );
        assert_eq!(steady.text, "You are a lamp.");
        let after = steady.after.clone().unwrap_or_default();
        assert!(after.starts_with("Your memory\n"), "{after}");
        assert!(after.contains("- coffee black\n") && after.contains(" It is noon."));
        assert!(after.contains("Person: What should I drink?\nYou: Try some tea. It is calm.\n"));
        assert_eq!(steady.history, told.history);
        // Forgotten, then another person's turn, then back: nothing of it
        // is told or given again (first builder's reviewer on #778).
        crate::thread::THREAD.lock().unwrap().forget();
        for whose in [Some("sam"), None] {
            let mut thread = crate::thread::THREAD.lock().unwrap();
            thread.at_lamp(whose.map(str::to_owned));
            thread.opening(Instant::now());
        }
        let told = system_message("You are a lamp.", None, "", Some(&file), None);
        assert!(told.history.is_empty(), "{:?}", told.history);
        assert!(!told.text.contains("Try some tea"), "{}", told.text);
        // Nothing changing and no thread: no message after the exchanges.
        assert_eq!(
            steady_first_message("You are a lamp.".into(), "", Some(&file), None).after,
            None
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Third on #2008: a line narrowed to the room (seeing someone speak)
    /// files its exchange, the person's words and the lamp's answer, in the
    /// room's thread, never the admitted person's; one not narrowed stays
    /// theirs.
    #[test]
    fn a_narrowed_lines_exchange_is_the_rooms_not_the_known_persons() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        use crate::memory_scope::{FaceCache, RequestKey, RequestScope};
        let kept = std::mem::take(&mut *crate::thread::THREAD.lock().unwrap());
        let known = std::env::temp_dir().join(format!("ten-narrowed-{}", std::process::id()));
        std::fs::write(&known, "A").unwrap();
        let cache = FaceCache::default();
        cache.replace(
            cache.begin_refresh(),
            crate::known_faces::FaceObservation::read(&known),
        );
        let a = crate::remember::person_id("A");
        crate::thread::THREAD
            .lock()
            .unwrap()
            .at_lamp(Some(a.clone()));
        for (request, words, narrowed) in
            [(51, "The code is 417.", true), (52, "Mine is 9.", false)]
        {
            let scope = RequestScope::new(
                RequestKey {
                    worker: lamp_turn_owner::BootId::new([16; 16]).unwrap(),
                    session: 1,
                    request,
                },
                cache.snapshot(Instant::now()),
            );
            let lease = scope.lease();
            if narrowed {
                lease.narrow_to_room();
            }
            for event in [
                ProviderOutput::Transcript {
                    request: None,
                    text: words.into(),
                    finished: true,
                },
                ProviderOutput::Transcript {
                    request: Some(request),
                    text: "Noted.".into(),
                    finished: true,
                },
                ProviderOutput::TurnComplete {
                    request,
                    idle: true,
                },
            ] {
                keep_scoped(&event, None, Some(&lease), None);
            }
        }
        let now = Instant::now();
        let (room, theirs) = {
            let thread = crate::thread::THREAD.lock().unwrap();
            (
                thread.recall_for(None, now, 4096).unwrap_or_default(),
                thread.recall_for(Some(&a), now, 4096).unwrap_or_default(),
            )
        };
        *crate::thread::THREAD.lock().unwrap() = kept;
        std::fs::remove_file(known).unwrap();
        assert!(
            room.contains("The code is 417.") && !room.contains("Mine is 9."),
            "{room}"
        );
        assert!(
            theirs.contains("Mine is 9.") && !theirs.contains("417"),
            "{theirs}"
        );
    }

    /// Jarvis on #2031 (#1803): onboarding's sentence is count-only. Its
    /// words, a planted marker among them, and the lamp's answer to it
    /// reach neither the thread, its file, the memory's journal nor the
    /// next turn's prompt; the next line is kept as ever, with none of it
    /// attached. In a child process, as `a_turn_is_kept_and_told_to_the_next`.
    #[test]
    fn the_sentence_is_kept_nowhere_and_the_next_line_carries_none_of_it() {
        use std::io::{BufRead, BufReader, Write};
        const CHILD: &str = "LAMP_SECOND_SENTENCE_TEST_CHILD";
        const MARKER: &str = "zebrafinch";
        if std::env::var(CHILD).ok().as_deref() != Some("1") {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "second_provider::worker::tests::the_sentence_is_kept_nowhere_and_the_next_line_carries_none_of_it",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env_remove(crate::remember::DAYS_ENVIRONMENT)
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                output.status.success() && stdout.contains("1 passed"),
                "{stdout}\n{stderr}"
            );
            // Nor in what the worker wrote to its journal.
            assert!(!stdout.contains(MARKER) && !stderr.contains(MARKER));
            return;
        }
        let dir = std::env::temp_dir().join(format!("lv-sentence-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let socket = dir.join("memory.sock");
        let file = dir.join("thread.json");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        // The memory: every request until the journal's first append.
        let memory = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                seen.push(line.clone());
                writeln!(stream, "{}", serde_json::json!({"ok":true,"entries":[]})).unwrap();
                if line.contains("\"append\"") {
                    return seen;
                }
            }
            unreachable!()
        });
        crate::remember::start(socket.clone(), None);
        // The sentence, request 7: its Start, then `CountOnly`.
        crate::thread::count_only(None);
        crate::thread::count_only(Some(7));
        for event in [
            ProviderOutput::Transcript {
                request: None,
                text: format!("I sit by the {MARKER} window"),
                finished: true,
            },
            ProviderOutput::Transcript {
                request: Some(7),
                text: format!("A {MARKER} window, lovely."),
                finished: false,
            },
            ProviderOutput::TurnComplete {
                request: 7,
                idle: true,
            },
        ] {
            keep(&event, Some(&file));
        }
        // The next line, request 8: a Start clears it.
        crate::thread::count_only(None);
        for event in [
            ProviderOutput::Transcript {
                request: None,
                text: "What should I drink?".into(),
                finished: true,
            },
            ProviderOutput::Transcript {
                request: Some(8),
                text: "Try some tea.".into(),
                finished: false,
            },
            ProviderOutput::TurnComplete {
                request: 8,
                idle: true,
            },
        ] {
            keep(&event, Some(&file));
        }
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(!written.contains(MARKER), "{written}");
        assert!(written.contains("What should I drink?") && written.contains("Try some tea."));
        let seen = memory.join().unwrap();
        assert!(seen.iter().all(|line| !line.contains(MARKER)), "{seen:?}");
        let journalled: serde_json::Value = serde_json::from_str(seen.last().unwrap()).unwrap();
        assert_eq!(journalled["heard"], "What should I drink?");
        assert_eq!(journalled["said"], "Try some tea.");
        let told = system_message("You are a lamp.", None, "", Some(&file), None);
        assert!(!told.text.contains(MARKER), "{}", told.text);
        assert!(
            told.history
                .iter()
                .all(|message| !format!("{message:?}").contains(MARKER)),
            "{:?}",
            told.history
        );
        assert_eq!(
            told.history,
            [
                Message::user("What should I drink?"),
                Message::assistant("Try some tea.")
            ]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The reviewer on #1713: a text's answer is never told what was said at
    /// the lamp. With a spoken exchange in the thread, the spoken turn's
    /// system message has it, and a text's has neither it nor any history.
    #[test]
    fn a_texts_system_message_never_has_the_rooms_spoken_conversation() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        let kept = std::mem::take(&mut *crate::thread::THREAD.lock().unwrap());
        let request = 41;
        for event in [
            ProviderOutput::Transcript {
                request: None,
                text: "Where did I put the spare key?".into(),
                finished: true,
            },
            ProviderOutput::Transcript {
                request: Some(request),
                text: "Under the blue mat.".into(),
                finished: true,
            },
            ProviderOutput::TurnComplete {
                request,
                idle: true,
            },
        ] {
            keep(&event, None);
        }
        let spoken = system_message("You are a lamp.", Some(" It is noon."), "", None, None);
        assert!(
            spoken.text.contains("Under the blue mat."),
            "{}",
            spoken.text
        );
        let typed =
            texts_system_message("You are a lamp.", Some(" It is noon."), "\n\nYour memory\n");
        assert_eq!(typed.text, "You are a lamp. It is noon.\n\nYour memory\n");
        assert!(typed.history.is_empty());
        // As the worker gives a text's answer its system message. Music
        // does not change typed isolation or either clock setting (#1921).
        for requested_audio in [false, true] {
            for clock in [false, true] {
                let (texts, _) = texts_ends(
                    "You are a lamp.".into(),
                    crate::tools::Settings {
                        requested_audio,
                        clock,
                        ..Default::default()
                    },
                );
                let told = texts();
                assert!(!told.text.contains("Under the blue mat."), "{}", told.text);
                assert!(!told.text.contains("spare key"), "{}", told.text);
                assert!(told.text.starts_with("You are a lamp."), "{}", told.text);
                assert!(told.history.is_empty());
                // Told the time as it is asked with LAMP_CLOCK, and as
                // before without it, where this machine has a local time.
                if crate::local_time::offset_here().is_some() {
                    if clock {
                        assert!(
                            told.text.contains("Your clock read this as you were asked"),
                            "{}",
                            told.text
                        );
                    } else {
                        assert!(told.text.contains("give it roughly"), "{}", told.text);
                        assert!(!told.text.contains("as you were asked"), "{}", told.text);
                    }
                }
            }
        }
        *crate::thread::THREAD.lock().unwrap() = kept;
    }

    /// What the texts' model was sent, round by round; it answers "Done."
    #[derive(Clone, Default)]
    struct Recording(Arc<std::sync::Mutex<Vec<Vec<net::Message>>>>);

    impl crate::messages::Model for Recording {
        fn round<'a>(
            &'a self,
            messages: &'a [net::Message],
            text: &'a mut (dyn FnMut(&str) + Send),
        ) -> futures_util::future::BoxFuture<'a, Result<Vec<net::ToolCall>, String>> {
            Box::pin(async move {
                self.0.lock().unwrap().push(messages.to_vec());
                text("Done.");
                Ok(Vec::new())
            })
        }
    }

    /// The reviewer on #1726: through a bus, as the worker runs its texts
    /// ([`texts_task`]), the model a text reaches is never sent a line said
    /// at the lamp, though the room's thread holds one.
    #[tokio::test]
    async fn the_worker_answers_a_text_without_the_rooms_spoken_conversation() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        use lamp_msgs::{ChatIn, ChatOut, ChatPart};
        let kept = std::mem::take(&mut *crate::thread::THREAD.lock().unwrap());
        let request = 43;
        for event in [
            ProviderOutput::Transcript {
                request: None,
                text: "Where did I put the spare key?".into(),
                finished: true,
            },
            ProviderOutput::Transcript {
                request: Some(request),
                text: "Under the blue mat.".into(),
                finished: true,
            },
            ProviderOutput::TurnComplete {
                request,
                idle: true,
            },
        ] {
            keep(&event, None);
        }
        // Not hand_controls' `/tmp/lvw-`: the crate's unit tests are one process.
        let root = PathBuf::from(format!("/tmp/lvsp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut replies = bus
            .subscribe_typed(ChatOut::TOPIC, "test", ChatOut::TYPE)
            .unwrap();
        let model = Recording::default();
        let worker = tokio::spawn(texts_task(
            model.clone(),
            "You are a lamp.".into(),
            crate::tools::Settings::default(),
            root.clone(),
        ));
        let mut sends = bus
            .publisher_declared(ChatIn::TOPIC, &ChatIn::declaration(), "cloud")
            .unwrap();
        let text = ChatIn {
            run_id: "run-1".into(),
            session_id: Some("s1".into()),
            text: "What did they just ask you?".into(),
            speak: false,
        };
        let until = Instant::now() + Duration::from_secs(10);
        while sends
            .publish_json(lamp_ipc::monotonic_us(), &text)
            .unwrap()
            .delivered
            == 0
        {
            assert!(Instant::now() < until, "the worker never listened");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        loop {
            assert!(Instant::now() < until, "no answer");
            match replies.recv(Duration::ZERO).unwrap() {
                Some(message) if message.parse::<ChatOut>().unwrap().part == ChatPart::Final => {
                    break;
                }
                Some(_) => {}
                None => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
        worker.abort();
        let _ = std::fs::remove_dir_all(&root);
        *crate::thread::THREAD.lock().unwrap() = kept;
        let sent = model.0.lock().unwrap();
        let all = serde_json::to_string(&sent[0]).unwrap();
        assert!(all.contains("You are a lamp."), "{all}");
        assert!(all.contains("What did they just ask you?"), "{all}");
        assert!(!all.contains("Under the blue mat."), "{all}");
        assert!(!all.contains("spare key"), "{all}");
    }

    /// The reviewer on #1100: this route reads the eyes' file only as a
    /// turn opens, so it never sees the 30 s a person is unseen between
    /// turns. B's last turn at 30 s, then A's turns an hour later: they are
    /// A's at once, never B's thread and kept things.
    #[test]
    fn a_known_person_long_after_another_has_their_own_turns_at_once() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        use std::time::{Duration, Instant};
        let dir =
            std::env::temp_dir().join(format!("lamp-voice-second-sessions-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("known");
        // The process's one thread, set aside for the test and put back.
        let kept = std::mem::take(
            &mut *crate::thread::THREAD
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        let start = Instant::now();
        let mut sessions = Sessions::default();
        std::fs::write(&file, "B\n").unwrap();
        let (_, person) = sessions.open(Some(&file), start + Duration::from_secs(30));
        assert_eq!(person, Some(crate::remember::person_id("B")));
        std::fs::write(&file, "A\n").unwrap();
        let mut opened = Vec::new();
        for at in [3_600, 3_605] {
            opened.push(
                sessions
                    .open(Some(&file), start + Duration::from_secs(at))
                    .1,
            );
        }
        *crate::thread::THREAD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = kept;
        std::fs::remove_dir_all(&dir).unwrap();
        let a = Some(crate::remember::person_id("A"));
        assert_eq!(opened, [a.clone(), a]);
    }

    /// The reviewer's P2 on #2268: with the session still the room's, a
    /// known person's first turn is matched against their own kept things,
    /// and the turn's notes still see the change (their Days note is asked
    /// anew). Someone else sitting down within 10 s keeps the session the
    /// first person's while the face is theirs, so nothing is matched.
    #[test]
    fn a_persons_first_turn_is_matched_against_their_own_kept_things() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        use std::time::{Duration, Instant};
        let dir =
            std::env::temp_dir().join(format!("lamp-voice-second-hits-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("known");
        let kept = std::mem::take(
            &mut *crate::thread::THREAD
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let sessions = Mutex::new(Sessions::default());
        let room = sessions.lock().unwrap().open(None, at(0));
        std::fs::write(&file, "Ossa\n").unwrap();
        let first = hits_person(&sessions, Some(&file), at(1));
        let notes = sessions.lock().unwrap().open(Some(&file), at(1));
        let next = sessions.lock().unwrap().open(Some(&file), at(2));
        std::fs::write(&file, "Kesh\n").unwrap();
        let other = hits_person(&sessions, Some(&file), at(3));
        *crate::thread::THREAD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = kept;
        std::fs::remove_dir_all(&dir).unwrap();
        let ossa = Some(crate::remember::person_id("Ossa"));
        assert_eq!(room, (false, None));
        assert_eq!(first, Some(ossa.clone()), "their first turn is theirs");
        assert_eq!(
            notes,
            (true, ossa.clone()),
            "the notes still see the change"
        );
        assert_eq!(next, (false, ossa));
        assert_eq!(other, None, "a face the session isn't for");
    }

    /// The first builder on #2602 (21 Oct 21:19): a line is the caller's
    /// alone only with the caller's one face seen speaking it.
    #[test]
    fn a_line_is_the_callers_alone_only_with_their_one_face_speaking() {
        let ids = |ids: &[&str]| ids.iter().map(|id| (*id).to_owned()).collect::<Vec<_>>();
        let caller = ids(&["b/f1"]);
        assert!(super::spoken_by_the_caller_alone(&ids(&["b/f1"]), &caller));
        assert!(
            !super::spoken_by_the_caller_alone(&ids(&["b/f1", "b/f2"]), &caller),
            "two faces"
        );
        assert!(
            !super::spoken_by_the_caller_alone(&ids(&["b/f2"]), &caller),
            "another face"
        );
        assert!(
            !super::spoken_by_the_caller_alone(&ids(&[]), &caller),
            "no face"
        );
        assert!(
            !super::spoken_by_the_caller_alone(&ids(&["b/f1"]), &ids(&[])),
            "no caller seen"
        );
        assert!(
            !super::spoken_by_the_caller_alone(&ids(&["b/f1"]), &ids(&["b/f1", "b/f2"])),
            "a call seen spoken by two faces names no caller"
        );
    }

    /// The reviewer-lead's P3 on #2537: with a request's lease, its hits are
    /// asked in the lease's own scope, whoever the sticky session holds; in
    /// the room's once another face spoke the line; and not at all once it is
    /// revoked. Without a lease, the session's person as before.
    #[test]
    fn a_turns_hits_are_asked_in_its_leases_scope() {
        // One test at a time on the process's thread and room.
        let _one = crate::provider_worker::tests::one_thread();
        use crate::memory_scope::{RequestKey, RequestScope, ScopeSnapshot};
        let dir = std::env::temp_dir().join(format!(
            "lamp-voice-second-hits-lease-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("known");
        std::fs::write(&file, "Kesh\n").unwrap();
        let now = Instant::now();
        let sessions = Mutex::new(Sessions::default());
        let key = RequestKey {
            worker: lamp_turn_owner::BootId::new([7; 16]).unwrap(),
            session: 1,
            request: 3,
        };
        let scope = RequestScope::new(key, ScopeSnapshot::of("Ossa"));
        let lease = scope.lease();
        let ossa = Some(crate::remember::person_id("Ossa"));
        let theirs = hits_whose(3, Some(&lease), &sessions, Some(&file), now);
        lease.narrow_to_room();
        let narrowed = hits_whose(3, Some(&lease), &sessions, Some(&file), now);
        scope.revoke();
        let revoked = hits_whose(3, Some(&lease), &sessions, Some(&file), now);
        let unleased = hits_whose(3, None, &sessions, Some(&file), now);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(theirs, Some(ossa), "the lease's person, not the session's");
        assert_eq!(narrowed, Some(None), "another face spoke it: the room's");
        assert_eq!(revoked, None, "a revoked lease asks nothing");
        assert_eq!(
            unleased,
            Some(Some(crate::remember::person_id("Kesh"))),
            "without a lease, the session's person"
        );
    }

    /// #1712: as the lamp's only voice, the rule judges in the room this
    /// route keeps (`room_told::room`), not in an unknown one; without the
    /// switch, and with no bus, the room stays unknown.
    #[test]
    fn as_the_only_voice_the_rule_judges_in_the_room_the_route_keeps() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        {
            let mut room = crate::room_told::room().lock().unwrap();
            let now = monotonic_us();
            for request in 1..=3 {
                room.heard(request, now);
                room.ended(request, now);
                room.unanswered(request, now);
            }
            assert_eq!(room.at(now), crate::room_told::Room::Busy);
        }
        let mut heeding = super::Heeding::new(false, false, None, None);
        assert_eq!(
            heeding.signs(9, (0, 0)).room,
            crate::room_told::Room::Unknown
        );
        heeding.as_the_only_voice(false, false, false);
        assert_eq!(heeding.signs(9, (0, 0)).room, crate::room_told::Room::Busy);
    }

    /// #1712 (Tests 20:22, the first builder 20:29):
    /// `LAMP_SPOKEN_TO=day-and-night` on the second route as the only voice
    /// reads the night as the Live worker does. With the day's own bars, a
    /// name heard close to the lamp at 0.95 in a busy room, nobody turned to
    /// it and no words calling it, is held by day (#829's eye drops) and
    /// plays at night; with `on` alone it plays either way. Before, this
    /// route never told its rule the night, and held it at night too.
    #[test]
    fn as_the_only_voice_the_day_and_the_night_bars_are_read_as_on_live() {
        use crate::room_told::Room;
        let signs = || super::Signs {
            room: Room::Busy,
            going_on: false,
            lamp_asked: false,
            turned: Some(false),
            names: vec![(1_000_100_000, 0.95)],
            near: true,
            continues_from: None,
        };
        let line = (1_000_000_000, 1_001_500_000);
        let judged = |by_day: bool, night: bool| {
            let mut heeding = super::Heeding::new(true, false, None, None);
            heeding.as_the_only_voice(false, true, by_day);
            heeding.night = if night { || true } else { || false };
            heeding.judge(9, line, "What time is it?", signs())
        };
        assert!(judged(true, false).is_some(), "by day, held");
        assert_eq!(judged(true, true), None, "at night, played");
        assert_eq!(judged(false, false), None, "with on alone, by day");
        assert_eq!(judged(false, true), None, "with on alone, at night");
    }

    /// The 18:20 shape on lamp-4ace on 2026-10-20 (and 18:16, 18:17): the
    /// name heard only in the room's sound at 0.9 to 0.95, nobody turned to
    /// the lamp, nothing close, no call in the words, let the line out
    /// ("its name was heard in the room's sound"), and the four lines after
    /// it out as the conversation going on. With
    /// `LAMP_NAME_SECOND_SIGN=on` the first is held, so no call opens
    /// a conversation and the four are held too; without it, as before.
    /// Quiet room and busy room, with `--spoken-to`. Its words were not
    /// read (the owner was talking to another device), so where its
    /// hearing fell is assumed: mid-line, as a passing word heard as the
    /// lamp's name is. Heard at the line's start it would count as a call
    /// ([`crate::attention::NAME_AT_THE_START_US`]), which
    /// `with_a_second_sign_asked_the_name_at_a_lines_start_counts` pins.
    #[test]
    fn with_a_second_sign_asked_a_name_heard_only_in_the_rooms_sound_opens_nothing() {
        use crate::room_told::Room;
        let talk = [
            "We should move the review to Thursday.",
            "Did anyone book the room for that?",
            "I think Sam has the slides already.",
            "Then let us send the invite this afternoon.",
        ];
        for room in [Room::Quiet, Room::Busy] {
            for on in [false, true] {
                let mut heeding = room_name_heeding(on);
                let first = heeding.judge(
                    896,
                    (10_000_000, 11_500_000),
                    "So where do we put the lamp review?",
                    room_name_signs(room, false, None, false, mid_line(0.95)),
                );
                assert_eq!(first.is_none(), !on, "{room:?} {on}: the first line");
                // Going on from the call let out, as `signs` reads it.
                for (request, line) in (897..).zip(talk) {
                    let going_on = heeding.called_at.is_some();
                    let held = heeding.judge(
                        request,
                        (20_000_000, 21_500_000),
                        line,
                        room_name_signs(room, going_on, None, false, Vec::new()),
                    );
                    assert_eq!(held.is_none(), !on, "{room:?} {on}: {line}");
                }
            }
        }
    }

    /// With `LAMP_NAME_SECOND_SIGN=on`, the name heard only in the
    /// room's sound counts with a second sign: a face turned to the lamp,
    /// said close to it, or its words calling it; at any score, the plain
    /// hearing's 0.999 too. Quiet room and busy room.
    #[test]
    fn with_a_second_sign_asked_a_name_heard_in_the_rooms_sound_counts_with_one() {
        use crate::room_told::Room;
        let named = mid_line;
        let plain = "What is on my calendar today?";
        for room in [Room::Quiet, Room::Busy] {
            let judged = |turned, near, heard, score| {
                room_name_heeding(true).judge(
                    896,
                    (10_000_000, 11_500_000),
                    heard,
                    room_name_signs(room, false, turned, near, named(score)),
                )
            };
            assert_eq!(
                judged(Some(true), false, plain, 0.95),
                None,
                "{room:?}: turned"
            );
            assert_eq!(judged(None, true, plain, 0.95), None, "{room:?}: near");
            assert_eq!(
                judged(None, false, "Hey Lamp, what is on my calendar today?", 0.95),
                None,
                "{room:?}: called in its words"
            );
            assert!(
                judged(Some(false), false, plain, 0.95).is_some(),
                "{room:?}"
            );
            assert!(
                judged(None, false, plain, 0.999).is_some(),
                "{room:?}: plainly"
            );
            assert_eq!(judged(Some(true), false, plain, 0.999), None, "{room:?}");
        }
    }

    /// The reviewer's P3 on #2313: with `LAMP_NAME_NOTE=on`, a name the
    /// switch drops is not told to the model either; without the switch,
    /// or with a second sign, it is.
    #[test]
    fn a_name_the_second_sign_drops_earns_no_name_note() {
        use crate::room_told::Room;
        let noted = |on: bool, turned| {
            let mut heeding = room_name_heeding(on);
            heeding.telling_the_name(true, false);
            heeding.heed_signs(
                896,
                (10_000_000, 11_500_000),
                "What is on my calendar today?",
                room_name_signs(Room::Quiet, false, turned, false, mid_line(0.95)),
            );
            heeding.name_note_for(896)
        };
        assert_eq!(noted(true, None), None, "dropped");
        assert_eq!(noted(false, None), Some(crate::called::note(false)), "off");
        assert_eq!(
            noted(true, Some(true)),
            Some(crate::called::note(false)),
            "turned"
        );
    }

    /// `LAMP_NAME_STAYS_IN_ITS_LINE=on`, the busy scene on lamp-4ace on
    /// 2026-10-26 at 06:21 (build 6919) with the product's second sign: a
    /// hearing at 0.92 that began at 11.0 s in "Can you pass me the
    /// charger? The white one." (9.2 s to 11.7 s), which has no name in it,
    /// played "Here you go. Is your phone dying again?" begun at 12.4 s. It
    /// came late, among the next line's signs, as on 4ace, or in time, among
    /// its own. With the switch the next line is held either way, and a line
    /// with its own hearing as it begins is played.
    #[test]
    fn a_name_heard_in_the_line_before_does_not_play_the_next() {
        use crate::room_told::Room;
        let charger = (9_200_000, 11_700_000);
        let phone = (12_400_000, 14_000_000);
        let desk = "Can you pass me the charger? The white one.";
        let hearing = |here: bool| {
            if here {
                vec![(11_000_000, 0.92)]
            } else {
                Vec::new()
            }
        };
        for late in [true, false] {
            let judged = |stays: bool| {
                let mut heeding = room_name_heeding(true);
                if stays {
                    heeding.keeping_the_name_in_its_line();
                }
                heeding.heed_signs(
                    45,
                    charger,
                    desk,
                    room_name_signs(Room::Busy, false, None, false, hearing(!late)),
                );
                heeding.heed_signs(
                    46,
                    phone,
                    "Here you go. Is your phone dying again?",
                    room_name_signs(Room::Busy, false, None, false, hearing(late)),
                )
            };
            assert_eq!(judged(false), None, "off, late {late}: played, as on 4ace");
            assert!(judged(true).is_some(), "late {late}");
        }
        let mut heeding = room_name_heeding(true);
        heeding.keeping_the_name_in_its_line();
        heeding.heed_signs(
            45,
            charger,
            desk,
            room_name_signs(Room::Busy, false, None, false, Vec::new()),
        );
        assert_eq!(
            heeding.heed_signs(
                46,
                phone,
                "Lamp, is my phone dying?",
                room_name_signs(Room::Busy, false, None, false, vec![(12_500_000, 0.95)]),
            ),
            None
        );
    }

    /// Reviewer-two's P2 on `2110d04a9`: a 0.999 hearing that began 2.9 s
    /// into a first line (0 to 3.0 s), come only with the third (3.7 to
    /// 4.8 s), after the short second (3.1 to 3.6 s) was judged, stays with
    /// the first: the third line is held. Off, it is played on it.
    #[test]
    fn a_hearing_come_two_lines_late_stays_with_its_line() {
        use crate::room_told::Room;
        let signs = |names| room_name_signs(Room::Busy, false, None, false, names);
        for stays in [false, true] {
            let mut heeding = room_name_heeding(true);
            if stays {
                heeding.keeping_the_name_in_its_line();
            }
            heeding.heed_signs(
                1,
                (10_000_000, 13_000_000),
                "I think the oven is still on, by the way.",
                signs(Vec::new()),
            );
            heeding.heed_signs(2, (13_100_000, 13_600_000), "Yes.", signs(Vec::new()));
            let third = heeding.heed_signs(
                3,
                (13_700_000, 14_800_000),
                "What time is it?",
                signs(vec![(12_900_000, 0.999)]),
            );
            assert_eq!(third.is_some(), stays, "the third line, stays {stays}");
        }
        // The reviewer-lead's P3 on `db3bc2d59`: the third line going on from
        // the second (cut by the lamp) still leaves out the first line's
        // hearing; off, it is played on it.
        for stays in [false, true] {
            let mut heeding = room_name_heeding(true);
            if stays {
                heeding.keeping_the_name_in_its_line();
            }
            heeding.heed_signs(
                1,
                (10_000_000, 13_000_000),
                "I think the oven is still on, by the way.",
                signs(Vec::new()),
            );
            heeding.heed_signs(2, (13_100_000, 13_600_000), "Yes, and", signs(Vec::new()));
            let going_on = heeding.heed_signs(
                3,
                (13_700_000, 14_800_000),
                "Yes, and what time is it?",
                super::Signs {
                    continues_from: Some(2),
                    ..signs(vec![(12_900_000, 0.999)])
                },
            );
            assert_eq!(going_on.is_some(), stays, "going on, stays {stays}");
        }
    }

    /// The readers' P2s on `da9a597de`, through `heed_signs` with the
    /// product's second sign in a busy room and the switch on. The
    /// reviewer-lead's: "Lamp." written "Ramp.", its hearing at its start
    /// and come late, then "What time is it?" 0.7 s after: the call goes
    /// on, and the question is played. Reviewer-two's: a 0.999 hearing
    /// mid-way through a first line, in time, stays with it; the short line
    /// after it and the third, with no hearing of their own, are both held.
    /// Off, the second line is played on it (the control).
    #[test]
    fn a_call_at_a_lines_start_goes_on_and_a_kept_hearing_is_used_up() {
        use crate::room_told::Room;
        let signs = |names| room_name_signs(Room::Busy, false, None, false, names);
        let mut heeding = room_name_heeding(true);
        heeding.keeping_the_name_in_its_line();
        heeding.heed_signs(1, (10_000_000, 10_600_000), "Ramp.", signs(Vec::new()));
        assert_eq!(
            heeding.heed_signs(
                2,
                (11_300_000, 12_300_000),
                "What time is it?",
                signs(vec![(10_300_000, 0.95)]),
            ),
            None,
            "a call at its start goes on"
        );
        for stays in [false, true] {
            let mut heeding = room_name_heeding(true);
            if stays {
                heeding.keeping_the_name_in_its_line();
            }
            heeding.heed_signs(
                1,
                (10_000_000, 13_000_000),
                "I think the oven is still on, by the way.",
                signs(vec![(12_900_000, 0.999)]),
            );
            let second = heeding.heed_signs(2, (13_100_000, 13_600_000), "Yes.", signs(Vec::new()));
            assert_eq!(second.is_some(), stays, "the second line, stays {stays}");
            if stays {
                assert!(
                    heeding
                        .heed_signs(
                            3,
                            (13_700_000, 14_500_000),
                            "What time is it?",
                            signs(Vec::new())
                        )
                        .is_some(),
                    "the third line"
                );
            }
        }
    }

    /// A hearing of the name mid-line in a line said from 10 s to 11.5 s,
    /// past [`crate::attention::NAME_AT_THE_START_US`]: a word heard as the
    /// lamp's name in passing.
    fn mid_line(score: f32) -> Vec<(u64, f32)> {
        vec![(11_400_000, score)]
    }

    /// b0333 and b0413 on lamp-4ace on 2026-10-21 (the first builder at
    /// 03:54 on #1722): "Hey Lamp, good morning!", heard at 1.000 with
    /// `began_ms_ago=480`, its words written "Mainland. Good morning.",
    /// nobody turned to the lamp and nothing close, was dropped by
    /// `LAMP_NAME_SECOND_SIGN=on` and the day lines after it lost. A
    /// hearing that began within [`crate::attention::NAME_AT_THE_START_US`]
    /// of the line's sound is a call, so it is played: b0802's, about
    /// 1.06 s in, as `lamp-name` dates a hearing from when its score
    /// reached the bar, as the name ends (the first builder at 09:12 on
    /// #1722). One a moment later is dropped as before. Quiet room and
    /// busy room.
    #[test]
    fn with_a_second_sign_asked_the_name_at_a_lines_start_counts() {
        use crate::attention::NAME_AT_THE_START_US;
        use crate::room_told::Room;
        let line = (10_000_000, 11_500_000);
        for room in [Room::Quiet, Room::Busy] {
            let judged = |at: u64| {
                room_name_heeding(true).judge(
                    896,
                    line,
                    "Mainland. Good morning.",
                    room_name_signs(room, false, None, false, vec![(at, 1.0)]),
                )
            };
            assert_eq!(judged(line.0 + 1_060_000), None, "{room:?}: b0802");
            assert_eq!(judged(line.0 + NAME_AT_THE_START_US), None, "{room:?}");
            assert!(
                judged(line.0 + NAME_AT_THE_START_US + 1).is_some(),
                "{room:?}"
            );
            // Heard just before the line's sound began: at its start too.
            assert_eq!(judged(line.0 - 300_000), None, "{room:?}");
        }
    }

    /// #829 scene 2, "the lamp named in passing" (the first builder's
    /// `twopeople829.sh`, 15 October), in a busy room: each "lamp" comes a
    /// few words into its line, so a hearing of it begins past
    /// [`crate::attention::NAME_AT_THE_START_US`] and, with
    /// `LAMP_NAME_SECOND_SIGN=on`, is dropped: none of the three is
    /// answered. Their words were played, not measured: at the office's
    /// rate, about 0.3 s a word after a 0.25 s pre-roll, "Yes," with its
    /// comma 0.45 s; `lamp-name`'s hearing begins as "lamp" ends and about
    /// 0.26 s after, as b0802's "Hey Lamp" did. Without the switch, a
    /// hearing lets each out.
    #[test]
    fn with_a_second_sign_asked_the_lamp_named_in_passing_stays_dropped() {
        use crate::room_told::Room;
        let began = 10_000_000;
        for (line, into) in [
            (
                "Did you see the lamp turn its head this morning?",
                250_000 + 5 * 300_000 + 260_000,
            ),
            (
                "Yes, the lamp looked at me when I came in.",
                250_000 + 450_000 + 2 * 300_000 + 260_000,
            ),
            (
                "I left the lamp on in the kitchen yesterday.",
                250_000 + 4 * 300_000 + 260_000,
            ),
        ] {
            assert!(into > crate::attention::NAME_AT_THE_START_US, "{line}");
            for on in [false, true] {
                let held = room_name_heeding(on).judge(
                    896,
                    (began, began + 3_000_000),
                    line,
                    room_name_signs(Room::Busy, false, None, false, vec![(began + into, 0.97)]),
                );
                assert_eq!(held.is_some(), on, "{on}: {line}");
            }
        }
    }

    /// The first builder at 09:12 on #1722 (b0802): `lamp-name` says a
    /// hearing once its score falls, up to `LONGEST` (0.8 s) after it began,
    /// and the rule dates it from when it began (`signs`: the message's
    /// time less its `ago_ms`), never from when it was said. A name whose
    /// hearing began as the line's sound did, said 0.8 s later, is the name
    /// at the line's start and played; one whose hearing began 1.5 s in is
    /// held. Busy room. On the real clock, about a tenth of a second.
    #[test]
    fn with_a_second_sign_asked_a_name_said_late_is_dated_from_when_it_began() {
        use lamp_msgs::NameHeard;
        // A short root: a Unix socket's address holds 103 bytes.
        let root = std::path::PathBuf::from(format!("/tmp/lnb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut name = bus
            .publisher_declared(NameHeard::TOPIC, &NameHeard::declaration(), "name")
            .unwrap();
        for (into, held) in [(0, false), (1_500_000, true)] {
            let mut heeding = super::Heeding::new(true, true, Some(&root), None);
            heeding.attention.speak_when_spoken_to(true);
            heeding.attention.ask_a_second_sign_of_the_name(true);
            let said = monotonic_us();
            let ago_ms = 800;
            name.publish_json(said, &NameHeard { score: 1.0, ago_ms })
                .unwrap();
            std::thread::sleep(Duration::from_millis(50));
            let line = (said - u64::from(ago_ms) * 1_000 - into, monotonic_us());
            let names = heeding.signs(896, line).names;
            assert_eq!(names.len(), 1, "{into}");
            let verdict = heeding.judge(
                896,
                line,
                "Mainland. Good morning.",
                room_name_signs(crate::room_told::Room::Busy, false, None, false, names),
            );
            assert_eq!(verdict.is_some(), held, "{into}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The rule with `--spoken-to`, strict, and the eyes and the voice's
    /// closeness read; the switch when `on`.
    fn room_name_heeding(on: bool) -> super::Heeding {
        let mut heeding = super::Heeding::new(true, true, None, None);
        heeding.attention.speak_when_spoken_to(true);
        heeding.attention.ask_a_second_sign_of_the_name(on);
        heeding
    }

    fn room_name_signs(
        room: crate::room_told::Room,
        going_on: bool,
        turned: Option<bool>,
        near: bool,
        names: Vec<(u64, f32)>,
    ) -> super::Signs {
        super::Signs {
            room,
            going_on,
            lamp_asked: false,
            turned,
            names,
            near,
            continues_from: None,
        }
    }

    /// #2191, `LAMP_NAMED_ALWAYS=on`: in a busy room a line that begins
    /// with the lamp's name (heard in its sound, as on the production floor
    /// at 0.976 to 0.978, or in its words, as on lamp-52e6 with no hearing
    /// at all), said to a face turned to the lamp, is answered: not held,
    /// and marked so the model's `not_for_me` is not taken. With no face
    /// turned to the lamp (only one in view, or none), with the name heard
    /// late in the line, or with no name, it is judged as before; and
    /// without the switch, nothing changes.
    #[test]
    fn with_named_always_a_line_begun_with_the_name_to_a_face_is_answered_in_a_busy_room() {
        let at_start = vec![(1_000_200_000, 0.977)];
        let late = vec![(1_002_000_000, 0.977)];
        let judged = named_always_judged;
        let plain = "turn the light on please.";
        // Someone turned to the lamp, no question: the busy room's bar held
        // it before.
        assert_eq!(
            judged(false, Some(true), at_start.clone(), plain).0,
            Some(super::BUSY_TURNED)
        );
        assert_eq!(
            judged(true, Some(true), at_start.clone(), plain),
            (None, true)
        );
        // Its words open with the name, nothing heard in the sound: marked
        // named, so a `not_for_me` is not taken.
        let words = "Hey Lamp, turn the light on.";
        assert_eq!(judged(true, Some(true), vec![], words), (None, true));
        assert!(!judged(false, Some(true), vec![], words).1);
        // No face turned to the lamp (a face in view not judged turned, or
        // turned away, or none): as before.
        for turned in [None, Some(false)] {
            assert_eq!(
                judged(true, turned, at_start.clone(), plain),
                judged(false, turned, at_start.clone(), plain),
                "{turned:?}"
            );
            assert!(!judged(true, turned, vec![], words).1, "{turned:?}");
        }
        // The name heard two seconds into the line, or not at all.
        // The name heard two seconds into the line, or not at all: someone
        // turned to the lamp is held by the busy room's bar as before.
        assert_eq!(
            judged(true, Some(true), late, plain),
            (Some(super::BUSY_TURNED), false)
        );
        assert_eq!(
            judged(true, Some(true), vec![], "Pass me the stapler."),
            (Some(super::BUSY_TURNED), false)
        );
    }

    /// The reviewer's P2 on #2210: office talk that opens with a word like
    /// the name ("Man", "Plan" and "Mom" are no call, nor "Lamp" or "Lamb"
    /// not set off as one), said with nobody turned to the lamp or with
    /// someone turned to it, is never marked named, so the switch neither
    /// lifts a hold for it nor overrules the model's `not_for_me` on it: it
    /// is judged as without the switch.
    #[test]
    fn with_named_always_office_talk_that_opens_like_the_name_is_not_named() {
        for heard in [
            "Man, that meeting ran long.",
            "Plan the sprint for tomorrow.",
            "Mom, can you pick me up?",
            "Lamp is broken, can you fix it?",
            "Lamb for dinner tonight?",
        ] {
            assert!(!crate::attention::begins_with_the_name(heard), "{heard}");
            for turned in [None, Some(true)] {
                let (held, named) = named_always_judged(true, turned, vec![], heard);
                assert!(!named, "{heard} {turned:?}");
                assert_eq!(
                    held,
                    named_always_judged(false, turned, vec![], heard).0,
                    "{heard} {turned:?}"
                );
            }
        }
    }

    /// The reviewer's P3 on #2210: with the switch, a line named to a face
    /// turned to the lamp lifts only the strict busy rule's and `busy_bar`'s
    /// holds; a conversation's line heard in another script is still held
    /// and asked again.
    #[test]
    fn with_named_always_an_unclear_line_named_to_a_face_stays_held() {
        use crate::room_told::Room;
        let mut heeding = super::Heeding::new(true, false, None, None);
        heeding.always_answering_named();
        heeding.asking_again(false);
        let signs = super::Signs {
            room: Room::Busy,
            going_on: true,
            lamp_asked: false,
            turned: Some(true),
            names: vec![(1_000_200_000, 0.977)],
            near: false,
            continues_from: None,
        };
        let held = heeding.judge(9, (1_000_000_000, 1_003_000_000), "नमस्ते, आप कैसे हैं?", signs);
        assert_eq!(held, Some(crate::not_speech::OTHER_SCRIPT));
    }

    /// One line judged in a busy room, with `LAMP_NAMED_ALWAYS` when
    /// `always`: (held, marked named to a face).
    fn named_always_judged(
        always: bool,
        turned: Option<bool>,
        names: Vec<(u64, f32)>,
        heard: &str,
    ) -> (Option<&'static str>, bool) {
        let mut heeding = super::Heeding::new(true, false, None, None);
        if always {
            heeding.always_answering_named();
        }
        let signs = super::Signs {
            room: crate::room_told::Room::Busy,
            going_on: false,
            lamp_asked: false,
            turned,
            names,
            near: false,
            continues_from: None,
        };
        let held = heeding.judge(9, (1_000_000_000, 1_003_000_000), heard, signs);
        (held, heeding.named_to_a_face(9))
    }

    /// The office lines the busy room let out as "the text shows it was
    /// called" (#1712, the first builder on #1722 at 16:11).
    const OFFICE_LINES: [&str; 5] = [
        "Man, that meeting ran long.",
        "Plan the sprint for tomorrow.",
        "Mom, can you pick me up?",
        "Lamp is broken, can you fix it?",
        "Lamb for dinner tonight?",
    ];

    /// `LAMP_BUSY_CALLED_STRICT=on`: in a busy room, with no name heard and
    /// nobody turned to the lamp, the office lines are held; without the
    /// switch, or in a quiet room, they are judged as before.
    #[test]
    fn with_busy_called_strict_office_lines_are_no_call_in_a_busy_room() {
        use crate::room_told::Room;
        for heard in OFFICE_LINES {
            assert_eq!(
                busy_called_judged(false, Room::Busy, None, vec![], heard),
                None,
                "{heard}: let out without the switch"
            );
            assert_eq!(
                busy_called_judged(true, Room::Busy, None, vec![], heard),
                Some(crate::attention::BUSY_UNCALLED),
                "{heard}"
            );
            assert_eq!(
                busy_called_judged(true, Room::Quiet, None, vec![], heard),
                busy_called_judged(false, Room::Quiet, None, vec![], heard),
                "{heard}: a quiet room"
            );
        }
    }

    /// With the switch, the name set off as a call still calls the lamp in
    /// a busy room, and a stand-in does with the name heard in the sound or
    /// a face turned to the lamp.
    #[test]
    fn with_busy_called_strict_a_call_set_off_or_backed_still_counts() {
        use crate::room_told::Room;
        for heard in ["Hey Lamp, what time is it?", "Lamp, what's the weather?"] {
            assert_eq!(
                busy_called_judged(true, Room::Busy, None, vec![], heard),
                None,
                "{heard}"
            );
        }
        let heard = "Mom, can you pick me up?";
        let named = vec![(1_000_200_000, 0.95)];
        assert_eq!(
            busy_called_judged(true, Room::Busy, None, named, heard),
            None
        );
        // Not a question: a face alone would leave it to the busy room's bar.
        assert_eq!(
            busy_called_judged(
                true,
                Room::Busy,
                Some(true),
                vec![],
                "Man, turn the light on please."
            ),
            None
        );
    }

    /// Each switch the worker reads turns on its own part of the rule, and
    /// only that: `LAMP_NAMED_ALWAYS` alone still has the model asked once
    /// more (#2210), and `LAMP_BUSY_CALLED_STRICT` alone only narrows the
    /// call (#2257's review).
    #[test]
    fn the_rule_s_switches_each_turn_on_only_their_own_part() {
        for (named_always, strict) in [(false, false), (true, false), (false, true), (true, true)] {
            let heeding = Arc::new(Mutex::new(super::Heeding::new(true, false, None, None)));
            let engine = super::heeding_switches(
                Engine::new(
                    Arc::new(Silent),
                    Told::default,
                    Arc::new(|_, _, _, _| Box::pin(async { serde_json::Value::Null })),
                ),
                &heeding,
                named_always,
                strict,
            );
            let case =
                format!("LAMP_NAMED_ALWAYS {named_always}, LAMP_BUSY_CALLED_STRICT {strict}");
            assert_eq!(
                engine.shared.named_to_a_face.is_some(),
                named_always,
                "{case}"
            );
            let heeding = heeding.lock().unwrap();
            assert_eq!(heeding.named_always, named_always, "{case}");
            assert_eq!(heeding.busy_called_strict, strict, "{case}");
        }
    }

    /// One line judged in `room` by the strict rule, with
    /// `LAMP_BUSY_CALLED_STRICT` when `on`: why it is held, if it is.
    fn busy_called_judged(
        on: bool,
        room: crate::room_told::Room,
        turned: Option<bool>,
        names: Vec<(u64, f32)>,
        heard: &str,
    ) -> Option<&'static str> {
        let mut heeding = super::Heeding::new(true, false, None, None);
        if on {
            heeding.narrowing_busy_calls();
        }
        let signs = super::Signs {
            room,
            going_on: false,
            lamp_asked: false,
            turned,
            names,
            near: false,
            continues_from: None,
        };
        heeding.judge(9, (1_000_000_000, 1_003_000_000), heard, signs)
    }

    /// `LAMP_TURNED_SPEAKING` (lamp-4ace, 21 Oct 14:00): in a busy room, a
    /// question let out by a face turned to the lamp is held when that face
    /// read silent through the line, and let out as before when it spoke,
    /// when its mouth was not read, or without the switch.
    #[test]
    fn with_turned_speaking_a_silent_face_turned_to_the_lamp_is_nobody_turned_to_it() {
        use lamp_msgs::{Face, Faces, Speaking, SpeakingState};
        let judged = |on: bool, state: Option<SpeakingState>, turned: Option<bool>| {
            let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
            let (now, now_us) = (Instant::now(), monotonic_us());
            for look in 0..5u64 {
                let faces = Faces {
                    faces: vec![Face {
                        id: "f1".into(),
                        xmin: 0.4,
                        ymin: 0.3,
                        xmax: 0.5,
                        ymax: 0.45,
                        confidence: 1.0,
                        yaw: Some(0.1),
                        facing: Some(true),
                        speaking: state.map(|state| Speaking {
                            state,
                            score: 0.0,
                            from: None,
                        }),
                    }],
                };
                eyes.look(
                    now + Duration::from_millis(200 + look * 400),
                    Some((("b", look), &faces)),
                );
            }
            let mut heeding = super::Heeding::new(true, false, None, None);
            heeding.seeing_speaking_faces(eyes, None);
            if on {
                heeding.counting_only_turned_speakers();
            }
            let signs = super::Signs {
                room: crate::room_told::Room::Busy,
                going_on: false,
                lamp_asked: false,
                turned,
                names: vec![],
                near: false,
                continues_from: None,
            };
            heeding.heed_signs(
                9,
                (now_us, now_us + 2_200_000),
                "So are we still doing lunch at noon?",
                signs,
            )
        };
        use SpeakingState::{No, Unknown, Yes};
        let turned = Some(true);
        assert_eq!(judged(false, Some(No), turned), None, "without the switch");
        assert_eq!(
            judged(true, Some(No), turned),
            Some(crate::attention::BUSY_UNCALLED),
            "someone at the desk, still"
        );
        assert_eq!(
            judged(true, Some(Yes), turned),
            None,
            "the face turned to it speaks"
        );
        assert_eq!(
            judged(true, Some(Unknown), turned),
            None,
            "its mouth not read"
        );
        assert_eq!(judged(true, None, turned), None, "no speaking read at all");
        // Nothing fresh from the eyes stays nothing: the busy rule's own
        // reading of an unknown sign, never a face taken away.
        assert_eq!(
            judged(true, Some(No), None),
            judged(false, Some(No), None),
            "the eyes said nothing fresh"
        );
    }

    /// The eyes' looks of one face turned to the lamp, from `from_ms` after
    /// `now`, five 400 ms apart, its mouth read as `states` gives, one by
    /// one and round again (`None`: not judged at all).
    fn turned_face_looks(
        eyes: &crate::speaking_faces::SpeakingFaces,
        now: Instant,
        from_ms: u64,
        states: &[Option<lamp_msgs::SpeakingState>],
        seq: &mut u64,
    ) {
        use lamp_msgs::{Face, Faces, Speaking};
        for look in 0..5u64 {
            *seq += 1;
            let faces = Faces {
                faces: vec![Face {
                    id: "f1".into(),
                    xmin: 0.4,
                    ymin: 0.3,
                    xmax: 0.5,
                    ymax: 0.45,
                    confidence: 1.0,
                    yaw: Some(0.1),
                    facing: Some(true),
                    speaking: states[look as usize % states.len()].map(|state| Speaking {
                        state,
                        score: 0.0,
                        from: None,
                    }),
                }],
            };
            eyes.look(
                now + Duration::from_millis(from_ms + look * 400),
                Some((("b", *seq), &faces)),
            );
        }
    }

    /// The signs of a line said with a face turned to the lamp, in `room`.
    fn turned_signs(room: crate::room_told::Room) -> super::Signs {
        super::Signs {
            room,
            going_on: false,
            lamp_asked: false,
            turned: Some(true),
            names: vec![],
            near: false,
            continues_from: None,
        }
    }

    /// `LAMP_TURNED_FACE_SPEAKS` (lamp-52e6, 24 Oct 05:31: with a still face
    /// turned to the lamp, two other voices' desk talk was answered as
    /// "someone is turned to the lamp"): with `LAMP_SPOKEN_TO`, in a room
    /// not known to be busy, quiet, or busy, a line let out by a face turned
    /// to the lamp is held when that face was read and never seen speaking
    /// it, a look unread among them; let out when it was seen speaking (its
    /// mouth still between words too), when its mouth was never read, or
    /// without the switch. `LAMP_TURNED_SPEAKING` alone lets the still face
    /// with one unread look through, as it did on the lamp.
    #[test]
    fn with_turned_face_speaks_a_turned_face_lets_out_only_a_line_it_is_seen_speaking() {
        use crate::room_told::Room;
        use lamp_msgs::SpeakingState::{No, Unknown, Yes};
        let judged = |on: bool,
                      turned_speaking: bool,
                      room: Room,
                      states: &[Option<lamp_msgs::SpeakingState>]| {
            let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
            let (now, now_us) = (Instant::now(), monotonic_us());
            turned_face_looks(&eyes, now, 200, states, &mut 0);
            let mut heeding = super::Heeding::new(true, false, None, None);
            heeding.as_the_only_voice(false, true, false);
            heeding.seeing_speaking_faces(eyes, None);
            if turned_speaking {
                heeding.counting_only_turned_speakers();
            }
            if on {
                heeding.needing_the_turned_face_to_speak();
            }
            let verdict = heeding.heed_signs(
                9,
                (now_us, now_us + 2_200_000),
                "So are we still doing lunch at noon?",
                turned_signs(room),
            );
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
            verdict
        };
        let still = [Some(No), Some(No), Some(Unknown), Some(No), Some(No)];
        let speaking = [Some(Yes), Some(No), Some(Yes), Some(No), Some(No)];
        for room in [Room::Unknown, Room::Quiet, Room::Busy] {
            assert_eq!(
                judged(false, false, room, &still),
                None,
                "{room:?}: without the switch"
            );
            assert_eq!(
                judged(false, true, room, &still),
                None,
                "{room:?}: LAMP_TURNED_SPEAKING alone, one look unread"
            );
            assert!(
                judged(true, false, room, &still).is_some(),
                "{room:?}: a still face"
            );
            assert_eq!(
                judged(true, false, room, &speaking),
                None,
                "{room:?}: the face speaks"
            );
            assert_eq!(
                judged(true, false, room, &[Some(Unknown)]),
                None,
                "{room:?}: its mouth never read"
            );
            assert_eq!(
                judged(true, false, room, &[None]),
                None,
                "{room:?}: no speaking read"
            );
        }
        assert_eq!(
            judged(true, false, Room::Busy, &still),
            Some(crate::attention::BUSY_UNCALLED)
        );
    }

    /// `LAMP_TURNED_FACE_SPEAKS` (lamp-52e6, 24 Oct 05:31): desk talk between
    /// two voices, neither saying the lamp's name, with someone at the lamp
    /// turned to it and still, the room not yet known (as on the lamp). With
    /// the switch none of it is answered, and the room is busy after the
    /// third line within a minute; without it every line is answered, as on
    /// the lamp. The same with `LAMP_TURNED_SPEAKING` on too, the face read
    /// in every look or with one unread (the reviewer-lead's P2 on #3305).
    #[test]
    fn with_turned_face_speaks_desk_talk_past_a_still_face_is_held_and_makes_the_room_busy() {
        let _one = crate::provider_worker::tests::one_thread();
        use crate::room_told::Room;
        use lamp_msgs::SpeakingState::{No, Unknown};
        let run = |on: bool, turned_speaking: bool, still: &[Option<lamp_msgs::SpeakingState>]| {
            *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
            let (now, now_us) = (Instant::now(), monotonic_us());
            {
                let mut room = crate::room_told::room().lock().unwrap();
                *room = crate::room_told::RoomTold::new();
                room.count_only_talk(true);
            }
            let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
            let mut heeding = Heeding::new(true, true, None, None);
            heeding.as_the_only_voice(false, true, false);
            heeding.seeing_speaking_faces(eyes.clone(), None);
            if turned_speaking {
                heeding.counting_only_turned_speakers();
            }
            if on {
                heeding.needing_the_turned_face_to_speak();
            }
            let lines = [
                "Did you get my message about the budget?",
                "Yes, I'll read it after lunch.",
                "Can we move the review to Thursday?",
            ];
            let mut seq = 0;
            let mut seen = Vec::new();
            for (request, line) in (1..).zip(lines) {
                let at_ms = request * 10_000;
                turned_face_looks(&eyes, now, at_ms + 200, still, &mut seq);
                let said = (now_us + at_ms * 1_000, now_us + at_ms * 1_000 + 2_200_000);
                let room = {
                    let mut room = crate::room_told::room().lock().unwrap();
                    room.heard(request, said.0);
                    room.at(said.0)
                };
                let held = heeding.heed_signs(request, said, line, turned_signs(room));
                let mut room = crate::room_told::room().lock().unwrap();
                room.ended(request, said.1);
                room.unanswered(request, said.1);
                seen.push((held.is_some(), room.at(said.1 + 1)));
            }
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
            seen
        };
        let unread_once = [Some(No), Some(No), Some(Unknown), Some(No), Some(No)];
        for (turned_speaking, still) in [
            (false, &unread_once[..]),
            (true, &unread_once[..]),
            (true, &[Some(No)][..]),
        ] {
            assert_eq!(
                run(true, turned_speaking, still),
                [
                    (true, Room::Unknown),
                    (true, Room::Unknown),
                    (true, Room::Busy)
                ],
                "LAMP_TURNED_SPEAKING {turned_speaking}, {still:?}"
            );
        }
        assert_eq!(
            run(false, false, &unread_once)
                .iter()
                .filter(|(held, _)| *held)
                .count(),
            0,
            "as on the lamp"
        );
    }

    /// `LAMP_TURNED_FACE_SPEAKS`: a line in the conversation's voice is let
    /// out by a face turned to the lamp though the face was not seen speaking
    /// it; another voice's line, past the same still face, is held. The same
    /// with `LAMP_TURNED_SPEAKING` on too, the face read in every look or
    /// with one unread (the reviewer-lead's P2 on #3305).
    #[test]
    fn with_turned_face_speaks_the_conversations_voice_still_counts_with_a_still_face() {
        let _one = crate::provider_worker::tests::one_thread();
        use lamp_msgs::SpeakingState::{No, Unknown};
        let unread_once = [Some(No), Some(No), Some(Unknown), Some(No), Some(No)];
        for (turned_speaking, still) in [
            (false, &[Some(No)][..]),
            (true, &[Some(No)][..]),
            (true, &unread_once[..]),
        ] {
            conversations_voice_with_a_still_face(turned_speaking, still);
        }
    }

    fn conversations_voice_with_a_still_face(
        turned_speaking: bool,
        still: &[Option<lamp_msgs::SpeakingState>],
    ) {
        use crate::room_told::Room;
        *crate::thread::THREAD.lock().unwrap() = crate::thread::Thread::new();
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
        let (now, now_us) = (Instant::now(), monotonic_us());
        let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
        let mut heeding = Heeding::new(true, true, None, None);
        heeding.as_the_only_voice(false, true, false);
        heeding.voices = Some(crate::same_voice::Voices::new(
            Box::new(CarryVoice::default()) as Box<dyn crate::same_voice::Printer>,
        ));
        heeding.seeing_speaking_faces(eyes.clone(), None);
        if turned_speaking {
            heeding.counting_only_turned_speakers();
        }
        heeding.needing_the_turned_face_to_speak();
        let mut seq = 0;
        let mut line =
            |heeding: &mut Heeding, request: u64, voice: i16, words: &str, turned: bool| {
                let at_ms = request * 10_000;
                if turned {
                    turned_face_looks(&eyes, now, at_ms + 200, still, &mut seq);
                }
                heeding.hear(
                    request,
                    crate::second_provider::Heard::Began {
                        over_the_lamps_voice: false,
                    },
                );
                heeding.hear(
                    request,
                    crate::second_provider::Heard::Samples(&[voice; 48_000]),
                );
                heeding.hear(request, crate::second_provider::Heard::Ended);
                let signs = super::Signs {
                    turned: Some(turned),
                    ..turned_signs(Room::Unknown)
                };
                let said = (now_us + at_ms * 1_000, now_us + at_ms * 1_000 + 2_200_000);
                heeding.heed_signs(request, said, words, signs)
            };
        assert_eq!(
            line(&mut heeding, 1, 1, "Lamp, what time is it?", false),
            None,
            "the call"
        );
        heeding.called_at = Some(Instant::now() - Duration::from_secs(60));
        assert_eq!(
            line(&mut heeding, 2, 1, "And what about tomorrow?", true),
            None,
            "the conversation's voice"
        );
        assert!(
            line(&mut heeding, 3, -1, "Did you send the slides?", true).is_some(),
            "another voice"
        );
        *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
    }

    /// `LAMP_TURNED_FACE_SPEAKS` beside `LAMP_SAME_SPEAKER_CARRIES` (#3296):
    /// the owner's five turns in a busy room are carried as before, by voice
    /// and face, by voice alone, and by face alone.
    #[test]
    fn with_turned_face_speaks_the_same_speaker_still_goes_on_five_turns() {
        let _one = crate::provider_worker::tests::one_thread();
        for (voice, face) in [(1, "f1"), (1, ""), (0, "f1")] {
            let mut desk = CarryDesk::new(true);
            desk.heeding.needing_the_turned_face_to_speak();
            assert_eq!(
                desk.five(12, voice, face),
                [None; 5],
                "voice {voice}, face {face:?}"
            );
        }
    }

    /// `LAMP_JOIN_CUT_WORDS`: one person said a line cut short and the line
    /// that cut it when their prints are alike, or, with no print to tell,
    /// when one face, the same, was seen speaking each. Two voices are two
    /// people whatever the eyes saw; a print still to come is waited for.
    #[test]
    fn one_person_said_both_lines_by_voice_or_by_one_face() {
        use crate::second_provider::Heard;
        // Prints by the line's first sample: 1 one voice, -1 another, 2 a
        // middling likeness to the first (cosine 0.5); held back while `slow`.
        #[derive(Default)]
        struct Voice {
            slow: bool,
            waiting: std::collections::VecDeque<(u64, Result<Vec<f32>, String>)>,
        }
        impl crate::same_voice::Printer for Voice {
            fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
                let print = match samples[0] {
                    1 => vec![1.0, 0.0],
                    2 => vec![0.5, 0.866],
                    _ => vec![0.0, 1.0],
                };
                self.waiting.push_back((request, Ok(print)));
                true
            }
            fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
                if self.slow {
                    return None;
                }
                self.waiting.pop_front()
            }
        }
        let line = |heeding: &mut Heeding, request: u64, voice: i16| {
            heeding.hear(
                request,
                Heard::Began {
                    over_the_lamps_voice: false,
                },
            );
            heeding.hear(request, Heard::Samples(&[voice; 48_000]));
            heeding.hear(request, Heard::Ended);
        };
        let faces = |heeding: &mut Heeding, request: u64, faces: &[&str]| {
            heeding.spoke.push_back((
                request,
                faces.iter().map(|face| (*face).to_owned()).collect(),
            ));
        };
        let voiced = |slow: bool| {
            let mut heeding = Heeding::new(true, true, None, None);
            heeding.checking_the_voice(Box::new(Voice {
                slow,
                ..Voice::default()
            }));
            heeding
        };
        // The same voice: one person.
        let mut heeding = voiced(false);
        line(&mut heeding, 1, 1);
        line(&mut heeding, 2, 1);
        assert_eq!(heeding.same_speaker(1, 2), Joined::Same(SAME_VOICE_JOINS));
        // The same voice, but the eyes saw two faces: two people (the review
        // of #2620), each line's face another, or two faces on one line.
        let two = Joined::Not("two faces were seen speaking them".to_owned());
        for (first, second) in [(&["a"][..], &["b"][..]), (&["a"][..], &["a", "b"][..])] {
            let mut heeding = voiced(false);
            line(&mut heeding, 1, 1);
            line(&mut heeding, 2, 1);
            faces(&mut heeding, 1, first);
            faces(&mut heeding, 2, second);
            assert_eq!(heeding.same_speaker(1, 2), two, "{first:?} {second:?}");
        }
        // Two faces on one line whose other line's record is gone (cleared by
        // a camera change, the prints kept), either way round: two people,
        // whatever the voices say.
        for (cut, current) in [(None, Some(&["a", "b"][..])), (Some(&["a", "b"][..]), None)] {
            let mut heeding = voiced(false);
            line(&mut heeding, 1, 1);
            line(&mut heeding, 2, 1);
            if let Some(seen) = cut {
                faces(&mut heeding, 1, seen);
            }
            if let Some(seen) = current {
                faces(&mut heeding, 2, seen);
            }
            assert_eq!(heeding.same_speaker(1, 2), two, "{cut:?} {current:?}");
        }
        // A camera closed or a mute since: no face is kept to join on.
        let mut heeding = Heeding::new(true, true, None, None);
        faces(&mut heeding, 1, &["a"]);
        faces(&mut heeding, 2, &["a"]);
        heeding.privacy_changed();
        assert!(matches!(heeding.same_speaker(1, 2), Joined::Not(_)));
        // Two voices: two people, though one face was seen speaking both.
        let mut heeding = voiced(false);
        line(&mut heeding, 1, 1);
        line(&mut heeding, 2, -1);
        faces(&mut heeding, 1, &["a"]);
        faces(&mut heeding, 2, &["a"]);
        assert_eq!(
            heeding.same_speaker(1, 2),
            Joined::Not("two voices (cosine 0.00)".to_owned())
        );
        // A middling likeness tells nothing: the face does.
        let mut heeding = voiced(false);
        line(&mut heeding, 1, 1);
        line(&mut heeding, 2, 2);
        assert_eq!(
            heeding.same_speaker(1, 2),
            Joined::Not(
                "a likeness between the bars (cosine 0.50), and no eyes read; nothing kept of the room at request 1"
                    .to_owned()
            )
        );
        faces(&mut heeding, 1, &["a"]);
        faces(&mut heeding, 2, &["a"]);
        assert_eq!(heeding.same_speaker(1, 2), Joined::Same(SAME_FACE_JOINS));
        // A print still to come: waited for, with the face's why for after.
        let mut heeding = voiced(true);
        line(&mut heeding, 1, 1);
        line(&mut heeding, 2, 1);
        assert_eq!(
            heeding.same_speaker(1, 2),
            Joined::Coming(Box::new(Joined::Not(
                "no eyes read; nothing kept of the room at request 1".to_owned()
            )))
        );
        faces(&mut heeding, 1, &["a"]);
        faces(&mut heeding, 2, &["a"]);
        assert_eq!(
            heeding.same_speaker(1, 2),
            Joined::Coming(Box::new(Joined::Same(SAME_FACE_JOINS)))
        );
        // No voice check: the eyes alone. One face, the same, each time.
        let unvoiced = |first: &[&str], second: &[&str]| {
            let mut heeding = Heeding::new(true, true, None, None);
            faces(&mut heeding, 1, first);
            faces(&mut heeding, 2, second);
            heeding.same_speaker(1, 2)
        };
        assert_eq!(unvoiced(&["a"], &["a"]), Joined::Same(SAME_FACE_JOINS));
        // Each refusal says what was missing, for the journal.
        let nothing = Joined::Not(
            "no voice check, and no eyes read; nothing kept of the room at request 1".to_owned(),
        );
        assert_eq!(unvoiced(&["a"], &["b"]), two, "two faces");
        assert_eq!(unvoiced(&["a"], &["a", "b"]), two, "another face spoke too");
        assert_eq!(unvoiced(&[], &["a"]), nothing, "nobody seen speaking");
        assert_eq!(
            Heeding::new(true, true, None, None).same_speaker(1, 2),
            nothing,
            "nothing known"
        );
        let mut heeding = voiced(false);
        line(&mut heeding, 1, 1);
        assert_eq!(
            heeding.same_speaker(1, 2),
            Joined::Not(
                "request 2 has no voice print (too short, or begun over the lamp's sound), and no eyes read; nothing kept of the room at request 1"
                    .to_owned()
            )
        );
    }

    /// The rule as `LAMP_JOIN_CUT_WORDS` runs it, with the eyes read but no
    /// face seen speaking and no voice check, as on lamp-4ace on 22 Oct:
    /// requests 1 and 2 judged in `room`, each with the eyes' newest look
    /// holding `in_view` faces just before (none: no fresh look, as with the
    /// camera closed), on a bus of its own named `name`.
    fn judged_in_rooms(
        name: &str,
        in_view: [Option<usize>; 2],
        room: crate::room_told::Room,
    ) -> Heeding {
        judged_lines(name, &in_view, room)
    }

    /// [`judged_in_rooms`] for as many lines as `in_view` has, requests 1
    /// on.
    fn judged_lines(
        name: &str,
        in_view: &[Option<usize>],
        room: crate::room_told::Room,
    ) -> Heeding {
        use lamp_msgs::{Face, Faces};
        // A short root: a Unix socket's address holds 103 bytes.
        let root = std::path::PathBuf::from(format!("/tmp/lj{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut eyes = bus
            .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
            .unwrap();
        // A look lives 300 ms here: one with none waits it out.
        eyes.expires_after(Some(Duration::from_millis(300)));
        let face = |n: usize| Face {
            speaking: None,
            id: format!("f{n}"),
            xmin: 0.4,
            ymin: 0.3,
            xmax: 0.6,
            ymax: 0.6,
            confidence: 1.0,
            yaw: Some(0.1),
            facing: Some(true),
        };
        let mut heeding = Heeding::new(true, true, Some(&root), None);
        heeding.seeing_speaking_faces(
            Arc::new(crate::speaking_faces::SpeakingFaces::default()),
            None,
        );
        heeding.keeping_line_rooms();
        for (request, faces) in (1..).zip(in_view.iter().copied()) {
            match faces {
                Some(faces) => {
                    eyes.publish_json(
                        monotonic_us(),
                        &Faces {
                            faces: (0..faces).map(face).collect(),
                        },
                    )
                    .unwrap();
                }
                None => std::thread::sleep(Duration::from_millis(400)),
            }
            let line = (monotonic_us().saturating_sub(2_000_000), monotonic_us());
            let signs = heeding.signs(request, line);
            heeding.judge(
                request,
                line,
                "What's the weather like",
                Signs {
                    room,
                    going_on: true,
                    ..signs
                },
            );
        }
        let _ = std::fs::remove_dir_all(&root);
        heeding
    }

    /// `LAMP_JOIN_CUT_WORDS` with no voice print and no face seen speaking,
    /// as on lamp-4ace on 22 Oct at 19:04-19:08, where 5 of 28 lines were a
    /// request's second half answered alone: nothing showing a second
    /// person joins a line let out. One face in the eyes' newest look at
    /// each line, in any room; no face in view or no eyes at all (the camera
    /// closed), only in a quiet room, never an unknown one (a reviewer's P2,
    /// 22 Oct). Two faces in
    /// view, two faces seen speaking, another voice heard in the
    /// conversation, or a privacy change since: no join. The journal's why
    /// is counts only.
    #[test]
    fn with_no_sign_of_a_second_person_a_line_let_out_joins_the_one_it_cut_short() {
        use crate::room_told::Room::{Busy, Quiet, Unknown};
        let same =
            |name: &str, in_view, room| judged_in_rooms(name, in_view, room).same_speaker(1, 2);
        let one_each = Joined::NoSecond(
            "no sign of a second person: 1 face in view at request 1's end, 1 face in view at request 2's, no other voice heard and no two faces seen speaking in the conversation"
                .to_owned(),
        );
        assert_eq!(same("a", [Some(1), Some(1)], Quiet), one_each);
        assert_eq!(
            same("b", [Some(1), Some(1)], Busy),
            one_each,
            "one face: a busy room too"
        );
        assert_eq!(
            same("b2", [Some(1), Some(1)], Unknown),
            one_each,
            "one face: a room not known too"
        );
        // A second face in view at either line: two people may be there.
        let missing = "no voice check, and no one face was seen speaking both";
        assert_eq!(
            same("c", [Some(1), Some(2)], Quiet),
            Joined::Not(format!(
                "{missing}; 1 face in view at request 1's end, 2 faces in view at request 2's"
            ))
        );
        assert!(matches!(
            same("d", [Some(2), Some(1)], Quiet),
            Joined::Not(_)
        ));
        // No eyes (the camera closed, or nobody in view): the room is the one
        // sign left, so only where it is known to be quiet.
        let unseen = |looks: &str| {
            Joined::NoSecond(format!(
                "no sign of a second person: {looks}, a quiet room, no other voice heard and no two faces seen speaking in the conversation"
            ))
        };
        assert_eq!(
            same("e", [None, None], Quiet),
            unseen(
                "no fresh look from the eyes at request 1's end, no fresh look from the eyes at request 2's"
            )
        );
        assert_eq!(
            same("f", [None, None], Busy),
            Joined::Not(format!(
                "{missing}; no fresh look from the eyes at request 1's end, no fresh look from the eyes at request 2's, in a busy room"
            ))
        );
        assert_eq!(
            same("g", [Some(0), Some(0)], Quiet),
            unseen("no face in view at request 1's end, no face in view at request 2's")
        );
        assert!(matches!(
            same("h", [Some(0), Some(0)], Busy),
            Joined::Not(_)
        ));
        // A room not known to be quiet (after a start, its facts off, a
        // two-person scene's first minute): no face in view joins nothing.
        assert_eq!(
            same("u", [None, None], Unknown),
            Joined::Not(format!(
                "{missing}; no fresh look from the eyes at request 1's end, no fresh look from the eyes at request 2's, in a room not known to be quiet"
            ))
        );
        assert!(matches!(
            same("v", [Some(0), Some(0)], Unknown),
            Joined::Not(_)
        ));
        assert!(matches!(
            same("w", [Some(1), None], Unknown),
            Joined::Not(_)
        ));
        assert!(matches!(same("i", [Some(1), None], Busy), Joined::Not(_)));
        assert!(matches!(
            same("j", [Some(1), None], Quiet),
            Joined::NoSecond(_)
        ));
        // Two faces seen speaking: two people first, whatever was in view.
        let mut heeding = judged_in_rooms("k", [Some(1), Some(1)], Quiet);
        heeding.spoke.push_back((1, vec!["a".to_owned()]));
        heeding.spoke.push_back((2, vec!["b".to_owned()]));
        assert_eq!(
            heeding.same_speaker(1, 2),
            Joined::Not("two faces were seen speaking them".to_owned())
        );
        // Another voice heard in the conversation, until a line is judged
        // with none going on.
        let mut heeding = judged_in_rooms("l", [Some(1), Some(1)], Quiet);
        heeding.keep_line_room(3, crate::room_told::Room::Quiet, true, true);
        assert_eq!(
            heeding.same_speaker(1, 2),
            Joined::Not(format!(
                "{missing}; another voice or two faces speaking in the conversation (request 3)"
            ))
        );
        heeding.keep_line_room(4, crate::room_told::Room::Quiet, false, false);
        assert_eq!(heeding.same_speaker(1, 2), one_each);
        // A camera closed or a mute since: nothing kept to join on.
        let mut heeding = judged_in_rooms("m", [Some(1), Some(1)], Quiet);
        heeding.privacy_changed();
        assert_eq!(
            heeding.same_speaker(1, 2),
            Joined::Not(format!("{missing}; nothing kept of the room at request 1"))
        );
    }

    /// A reviewer's P2 on the chain (22 Oct): a face two pieces apart is two
    /// people as much as one beside it. One face in view at each of three
    /// lines, a quiet room; X seen saying the first, nobody the short middle
    /// one, Y the last: each link alone passes, but the first and the last
    /// are two faces, and then only a piece Y was seen saying is kept
    /// (reviewer-three's P2): the last line alone. Y seen saying the middle
    /// one too, it goes with it. The same face at both ends, all three are
    /// asked as one. Through the rule's own judgement.
    #[tokio::test]
    async fn a_chain_with_two_faces_seen_speaking_in_it_breaks_before_the_second() {
        use crate::room_told::Room::Quiet;
        use crate::second_provider::net::Message;
        use crate::second_provider::tests::asked_after_pieces;
        let pieces = ["I want the", "um, and", "weather in Saigon this weekend?"];
        let rule =
            |name: &str, middle: &[&str], last: &str| -> crate::second_provider::SameSpeaker {
                let mut heeding = judged_lines(name, &[Some(1), Some(1), Some(1)], Quiet);
                heeding.spoke.push_back((1, vec!["x".to_owned()]));
                heeding
                    .spoke
                    .push_back((2, middle.iter().map(|face| (*face).to_owned()).collect()));
                heeding.spoke.push_back((3, vec![last.to_owned()]));
                let heeding = Arc::new(Mutex::new(heeding));
                Arc::new(move |cut, request| heeding.lock().unwrap().same_speaker(cut, request))
            };
        let soon = Duration::from_millis(1_500);
        assert_eq!(
            asked_after_pieces(&pieces, soon, rule("f1", &[], "y"), |_, _| {}).await,
            Message::user("weather in Saigon this weekend?"),
            "X then nobody then Y"
        );
        assert_eq!(
            asked_after_pieces(&pieces, soon, rule("f3", &["y"], "y"), |_, _| {}).await,
            Message::user("um, and weather in Saigon this weekend?"),
            "X then Y then Y"
        );
        assert_eq!(
            asked_after_pieces(&pieces, soon, rule("f2", &[], "x"), |_, _| {}).await,
            Message::user("I want the um, and weather in Saigon this weekend?"),
            "X then nobody then X"
        );
    }

    /// `LAMP_NAME_NEAR` with speaking faces read (reviewer-three's P3 on
    /// 1600b0a69): the face that counts for a first word of the name's shape
    /// is one turned to the lamp and seen speaking the line. The listener
    /// turned to it while a colleague, turned away, speaks is no one; the
    /// speaker turned to it and speaking is. Without speaking faces read,
    /// the newest look as before.
    #[test]
    fn a_turned_face_counts_for_a_first_word_only_when_it_speaks_the_line() {
        use lamp_msgs::{Face, Faces, Speaking, SpeakingState};
        let turned_said = |speaker_turned: bool| {
            let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
            let (now, now_us) = (Instant::now(), monotonic_us());
            let face = |id: &str, facing: bool, state: SpeakingState| Face {
                id: id.into(),
                xmin: 0.4,
                ymin: 0.3,
                xmax: 0.5,
                ymax: 0.45,
                confidence: 1.0,
                yaw: Some(0.1),
                facing: Some(facing),
                speaking: Some(Speaking {
                    state,
                    score: 0.0,
                    from: None,
                }),
            };
            for look in 0..5u64 {
                let faces = Faces {
                    faces: vec![
                        // The listener, turned to the lamp, quiet.
                        face("f1", true, SpeakingState::No),
                        // The one speaking.
                        face("f2", speaker_turned, SpeakingState::Yes),
                    ],
                };
                eyes.look(
                    now + Duration::from_millis(200 + look * 400),
                    Some((("b", look), &faces)),
                );
            }
            let mut heeding = super::Heeding::new(true, false, None, None);
            heeding.seeing_speaking_faces(eyes, None);
            heeding.turned_near_speaking(true);
            // The close face turned to the lamp over the line, whoever's
            // (`addressed::Looks`, Sixth's P2s on 1600b0a69).
            let looks = Looked::default();
            if let Ok(mut looks) = looks.lock() {
                let mut close = face("f1", true, SpeakingState::No);
                close.ymax = 0.7;
                let seen = Faces { faces: vec![close] };
                for look in 0..4u64 {
                    looks.look(now_us + 200_000 + look * 400_000, &seen, now_us);
                }
            }
            heeding.near_looks = Some(looks);
            heeding.turned_to_the_lamp((now_us, now_us + 2_200_000))
        };
        assert!(!turned_said(false), "only the listener is turned");
        assert!(turned_said(true), "the speaker turned to it, speaking");
        let unread = super::Heeding::new(true, false, None, None);
        assert!(
            !unread.turned_to_the_lamp((0, 1)),
            "no speaking faces read and no bus: no face"
        );
    }

    /// The reviewer-lead's P3 on #3801: with `--addressed` and
    /// `LAMP_FIRST_SECONDS_QUIET` both on, the levels' thread reads the eyes
    /// beside the levels, whichever started it first: the first seconds'
    /// levels started before `--addressed`'s looks, as `serve` once did,
    /// still fill them. With `--addressed` off, no looks.
    #[test]
    fn the_levels_thread_reads_the_eyes_for_addressed_started_before_it() {
        use lamp_msgs::{Face, Faces};
        let root = std::path::PathBuf::from(format!("/tmp/lle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut eyes = bus
            .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
            .unwrap();
        let mut looked = |levels_first: bool| {
            let mut heeding = Heeding::new(true, true, Some(&root), None);
            heeding.showing_the_first_seconds_quiet();
            if levels_first {
                heeding.hear_levels(&root, false);
            }
            heeding.judging_addressed(&root);
            heeding.hear_levels(&root, false);
            let began = monotonic_us();
            for _ in 0..6 {
                eyes.publish_json(
                    monotonic_us(),
                    &Faces {
                        faces: vec![Face {
                            id: "f0".into(),
                            xmin: 0.4,
                            ymin: 0.2,
                            xmax: 0.6,
                            ymax: 0.7,
                            confidence: 1.0,
                            yaw: Some(0.1),
                            facing: Some(true),
                            speaking: None,
                        }],
                    },
                )
                .unwrap();
                std::thread::sleep(LEVEL_EVERY);
            }
            std::thread::sleep(LEVEL_EVERY * 2);
            let (looks, _) = heeding.addressed.as_ref().unwrap();
            looks.lock().unwrap().between(began, monotonic_us()).looks
        };
        assert!(looked(true) > 0, "the levels started first");
        assert!(looked(false) > 0, "--addressed set up first");
        let mut off = Heeding::new(true, true, Some(&root), None);
        off.showing_the_first_seconds_quiet();
        off.hear_levels(&root, false);
        assert!(off.levels_looks.get().is_none() && off.addressed.is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Sixth's P3 on c0890e8db: with `--addressed` set up first, as `serve`
    /// does, `LAMP_NAME_NEAR` keeps its looks, one reader of the eyes for
    /// both; without it, its own.
    #[test]
    fn the_near_check_reads_the_eyes_through_addressed_when_it_runs() {
        let root = std::path::PathBuf::from(format!("/tmp/lna-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut heeding = Heeding::new(true, true, Some(&root), None);
        heeding.judging_addressed(&root);
        heeding.looking_for_near(&root);
        let (addressed, _) = heeding.addressed.as_ref().unwrap();
        assert!(Arc::ptr_eq(addressed, heeding.near_looks.as_ref().unwrap()));
        let mut alone = Heeding::new(true, true, Some(&root), None);
        alone.looking_for_near(&root);
        assert!(alone.near_looks.is_some() && alone.addressed.is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Sixth's P2s and P3 on 1600b0a69, through the eyes' wiring
    /// ([`Heeding::looking_for_near`] reading `humans/faces/tracked` off a
    /// bus): a face counts for a first word of the name's shape only when a
    /// close one was turned to the lamp over the line, as `--addressed`
    /// judges it. A far face turned while the close speaker faces away is
    /// nobody; a face turned to it only after the line is nobody; a close
    /// face turned to it over the line is someone. With
    /// `LAMP_TURNED_SPEAKING`, one seen silent through it is nobody.
    #[test]
    fn a_close_face_turned_over_the_line_counts_for_a_first_word_through_the_eyes() {
        use lamp_msgs::{Face, Faces, Speaking, SpeakingState};
        let run =
            |name: &str, during: &[(f32, bool)], after: &[(f32, bool)], silent_turned: bool| {
                let root =
                    std::path::PathBuf::from(format!("/tmp/lnl{name}-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&root);
                std::fs::create_dir_all(&root).unwrap();
                let bus = lamp_bus::Bus::new(&root).unwrap();
                let mut eyes = bus
                    .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
                    .unwrap();
                let mut heeding = Heeding::new(true, true, Some(&root), None);
                heeding.looking_for_near(&root);
                if silent_turned {
                    // The eyes saw the turned face still through the line.
                    let speaking = Arc::new(crate::speaking_faces::SpeakingFaces::default());
                    let start = Instant::now();
                    for look in 0..8u64 {
                        let still = Faces {
                            faces: vec![Face {
                                id: "f0".into(),
                                xmin: 0.4,
                                ymin: 0.2,
                                xmax: 0.6,
                                ymax: 0.6,
                                confidence: 1.0,
                                yaw: Some(0.1),
                                facing: Some(true),
                                speaking: Some(Speaking {
                                    state: SpeakingState::No,
                                    score: 0.0,
                                    from: None,
                                }),
                            }],
                        };
                        speaking.look(
                            start + Duration::from_millis(look * 150),
                            Some((("b", look), &still)),
                        );
                    }
                    heeding.seeing_speaking_faces(speaking, None);
                    heeding.counting_only_turned_speakers();
                }
                let faces = |set: &[(f32, bool)]| Faces {
                    faces: set
                        .iter()
                        .enumerate()
                        .map(|(n, (height, facing))| Face {
                            id: format!("f{n}"),
                            xmin: 0.4,
                            ymin: 0.2,
                            xmax: 0.6,
                            ymax: 0.2 + height,
                            confidence: 1.0,
                            yaw: Some(0.1),
                            facing: Some(*facing),
                            speaking: None,
                        })
                        .collect(),
                };
                std::thread::sleep(Duration::from_millis(150));
                let began = monotonic_us();
                for _ in 0..4 {
                    eyes.publish_json(monotonic_us(), &faces(during)).unwrap();
                    std::thread::sleep(Duration::from_millis(150));
                }
                let ended = monotonic_us();
                for _ in 0..4 {
                    eyes.publish_json(monotonic_us(), &faces(after)).unwrap();
                    std::thread::sleep(Duration::from_millis(150));
                }
                let turned = heeding.turned_to_the_lamp((began, ended));
                let _ = std::fs::remove_dir_all(&root);
                turned
            };
        let close_away = (0.4, false);
        let far_turned = (0.08, true);
        let close_turned = (0.4, true);
        assert!(
            !run(
                "a",
                &[close_away, far_turned],
                &[close_away, far_turned],
                false
            ),
            "a far face turned while the close speaker faces away"
        );
        assert!(
            !run("b", &[close_away], &[close_turned], false),
            "turned to the lamp only after the line"
        );
        assert!(
            run("c", &[close_turned], &[close_turned], false),
            "a close face turned to it over the line"
        );
        assert!(
            !run("d", &[close_turned], &[close_turned], true),
            "turned to it but seen silent through the line (LAMP_TURNED_SPEAKING)"
        );
    }

    /// `LAMP_NAME_NEAR`: lamp-name's near hearings on the bus count for a
    /// line's first word from 0.3 s before its first sound to 1.3 s after,
    /// the best of them; none outside that, or without the reader.
    #[test]
    fn a_near_hearing_counts_only_around_the_lines_start() {
        let _clock = clock_with_history();
        let root = std::path::PathBuf::from(format!("/tmp/lnn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut heeding = Heeding::new(true, true, Some(&root), None);
        assert_eq!(heeding.near_name(monotonic_us()), None, "no reader");
        assert!(heeding.hearing_near());
        let mut near = bus
            .publisher_declared(
                lamp_msgs::NameHeard::NEAR_TOPIC,
                &lamp_msgs::NameHeard::declaration(),
                "name",
            )
            .unwrap();
        for score in [0.55, 0.62] {
            near.publish_json(monotonic_us(), &lamp_msgs::NameHeard { score, ago_ms: 0 })
                .unwrap();
        }
        let now = monotonic_us();
        assert_eq!(
            heeding.near_name(now - 500_000),
            Some(0.62),
            "within its start"
        );
        assert_eq!(
            heeding.near_name(now + 200_000),
            Some(0.62),
            "just before it"
        );
        assert_eq!(heeding.near_name(now - 2_000_000), None, "past 1.3 s after");
        assert_eq!(heeding.near_name(now + 1_000_000), None, "long before it");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `LAMP_SECOND_NAME_CARRIES` (El's P1s on first/name-carries, 22 Oct):
    /// the lamp's name carries to the line after it only on the join's own
    /// sign of one speaker, its no sign of a second person only in a quiet
    /// room. A busy room, even with one face in view at each; two faces in
    /// view, or seen speaking; no eyes in a room not known to be quiet; two
    /// voices (a print under 0.30): no carry. No eyes in a quiet room, or
    /// one face in view at each there: it carries. The same voice carries
    /// in any room.
    #[test]
    fn the_lamps_name_carries_only_on_a_sign_of_one_speaker() {
        use crate::room_told::Room::{Busy, Quiet, Unknown};
        use crate::second_provider::Heard;
        let one =
            |name: &str, in_view, room| judged_in_rooms(name, in_view, room).one_speaker(1, 2);
        assert!(
            matches!(one("na", [Some(1), Some(1)], Busy), Joined::Not(why) if why.ends_with("but not both in a quiet room")),
            "one face each, a busy room"
        );
        assert!(
            matches!(one("nb", [None, None], Busy), Joined::Not(_)),
            "no eyes, busy"
        );
        assert!(
            matches!(one("nc", [Some(1), Some(1)], Unknown), Joined::Not(_)),
            "one face each, a room not known"
        );
        assert!(
            matches!(one("nd", [Some(1), Some(2)], Quiet), Joined::Not(_)),
            "two faces"
        );
        let mut heeding = judged_in_rooms("ne", [Some(1), Some(1)], Quiet);
        heeding.spoke.push_back((1, vec!["a".to_owned()]));
        heeding.spoke.push_back((2, vec!["b".to_owned()]));
        assert_eq!(
            heeding.one_speaker(1, 2),
            Joined::Not("two faces were seen speaking them".to_owned()),
            "two faces seen speaking"
        );
        assert!(
            matches!(one("nf", [None, None], Quiet), Joined::NoSecond(_)),
            "no eyes, quiet"
        );
        assert!(
            matches!(one("ng", [Some(1), Some(1)], Quiet), Joined::NoSecond(_)),
            "one face each, quiet"
        );
        assert!(
            matches!(one("nh", [None, None], Unknown), Joined::Not(_)),
            "no eyes, unknown"
        );
        // Prints: 1 one voice, anything else another (cosine 0).
        #[derive(Default)]
        struct Voice(std::collections::VecDeque<(u64, Result<Vec<f32>, String>)>);
        impl crate::same_voice::Printer for Voice {
            fn ask(&mut self, request: u64, samples: &[i16]) -> bool {
                let print = if samples[0] == 1 {
                    vec![1.0, 0.0]
                } else {
                    vec![0.0, 1.0]
                };
                self.0.push_back((request, Ok(print)));
                true
            }
            fn answer(&mut self) -> Option<(u64, Result<Vec<f32>, String>)> {
                self.0.pop_front()
            }
        }
        let voiced = |name: &str, room, second: i16| {
            let mut heeding = judged_in_rooms(name, [None, None], room);
            heeding.checking_the_voice(Box::new(Voice::default()));
            for (request, voice) in [(1, 1), (2, second)] {
                heeding.hear(
                    request,
                    Heard::Began {
                        over_the_lamps_voice: false,
                    },
                );
                heeding.hear(request, Heard::Samples(&[voice; 48_000]));
                heeding.hear(request, Heard::Ended);
            }
            heeding.one_speaker(1, 2)
        };
        assert!(
            matches!(voiced("ni", Quiet, -1), Joined::Not(why) if why.starts_with("two voices")),
            "a print under 0.30, even in a quiet room"
        );
        assert_eq!(
            voiced("nj", Busy, 1),
            Joined::Same(SAME_VOICE_JOINS),
            "the same voice"
        );
    }

    /// `LAMP_SECOND_NAME_CARRIES`: a line kept before it is judged is kept as
    /// a judgement keeps it, from the bus: the eyes' newest look and the
    /// room, so the name and the line after it are told apart as judged
    /// lines are.
    #[test]
    fn a_line_kept_unjudged_is_read_for_one_speaker_as_a_judged_one() {
        use lamp_msgs::{Face, Faces, RoomState};
        let kept = |name: &str, in_view: usize, room: lamp_msgs::Room| {
            let root = std::path::PathBuf::from(format!("/tmp/lk{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            let bus = lamp_bus::Bus::new(&root).unwrap();
            let mut eyes = bus
                .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
                .unwrap();
            let mut rooms = bus
                .publisher_declared(RoomState::TOPIC, &RoomState::declaration(), "room")
                .unwrap();
            let mut heeding = Heeding::new(true, true, Some(&root), None);
            heeding.keeping_line_rooms();
            for request in [1, 2] {
                eyes.publish_json(
                    monotonic_us(),
                    &Faces {
                        faces: (0..in_view)
                            .map(|n| Face {
                                speaking: None,
                                id: format!("f{n}"),
                                xmin: 0.4,
                                ymin: 0.3,
                                xmax: 0.6,
                                ymax: 0.6,
                                confidence: 1.0,
                                yaw: Some(0.1),
                                facing: Some(true),
                            })
                            .collect(),
                    },
                )
                .unwrap();
                rooms
                    .publish_json(monotonic_us(), &RoomState { state: room })
                    .unwrap();
                let now = monotonic_us();
                heeding.keep_unjudged(request, (now.saturating_sub(1_000_000), now));
            }
            let _ = std::fs::remove_dir_all(&root);
            heeding.one_speaker(1, 2)
        };
        assert!(matches!(
            kept("a", 1, lamp_msgs::Room::Quiet),
            Joined::NoSecond(_)
        ));
        assert!(matches!(
            kept("b", 1, lamp_msgs::Room::Busy),
            Joined::Not(_)
        ));
        assert!(matches!(
            kept("c", 2, lamp_msgs::Room::Quiet),
            Joined::Not(_)
        ));
        assert!(matches!(
            kept("d", 0, lamp_msgs::Room::Unknown),
            Joined::Not(_)
        ));
    }

    /// `LAMP_FIRST_SECONDS_QUIET` (#3711): right after a start the room is
    /// unknown only for want of history, and on lamp-4ace on 26 Oct a bare
    /// "Lamp.", a pause, then the request, with nobody in view, was held and
    /// had to be said twice. With the switch, the ears' own stream shows the
    /// room quiet for the join: outside the lines said, the room microphone
    /// stayed within a few dB of its own quiet, that quiet under a loud
    /// room's, and no talk was left alone, no line held and no other voice
    /// heard since the start. lamp-c0a4 on 27 Oct (7071), a quiet room read at a quiet of
    /// -45 dBFS and levels of -43 to -46, joins; the person's own lines rise
    /// as they will. Talk or a television between the lines, a loud room's
    /// quiet, no levels, another voice since the start, talk left alone,
    /// a line held since the start, two faces in view, or the switch off:
    /// held, as before. The held line is another person's, with no voice
    /// check and no face seen speaking it: held as said to nobody, the room
    /// told counts it as no talk, and nothing else shows the other voice
    /// (reviewer-three's P3 on #3801, lamp-c0a4's run 2).
    #[test]
    fn right_after_a_start_the_ears_quiet_room_lets_a_bare_name_join_its_request() {
        use crate::room_told::Room;
        use lamp_msgs::{Face, Faces, RoomSound};
        /// How the first seconds after the start went.
        #[derive(Clone, Copy)]
        struct Start {
            /// The ears' quiet, in dBFS; none: no level from the ears.
            floor: Option<f32>,
            /// The room microphone's rises over its quiet outside the two
            /// lines, every 100 ms in turn; none: no levels sampled.
            room: Option<&'static [f32]>,
            /// Faces in the eyes' newest look at each line.
            in_view: usize,
            /// A line said and left alone since the start.
            talk: bool,
            /// A line in another voice since the start, its conversation
            /// over before the name.
            other_voice: bool,
            /// A line since the start that the rule held, said to nobody
            /// with nobody in view, before the name.
            held: bool,
            on: bool,
        }
        let quiet = Start {
            floor: Some(-52.0),
            room: Some(&[0.5, 1.0, 0.0, 1.5]),
            in_view: 0,
            talk: false,
            other_voice: false,
            held: false,
            on: true,
        };
        let run = |name: &str, start: Start| {
            let root = std::path::PathBuf::from(format!("/tmp/lq{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            let bus = lamp_bus::Bus::new(&root).unwrap();
            let mut eyes = bus
                .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
                .unwrap();
            let mut ears = bus
                .publisher_declared(RoomSound::TOPIC, &RoomSound::declaration(), "ears")
                .unwrap();
            let now = monotonic_us();
            {
                // The voice has just started: the room's watch begins now.
                let mut room = crate::room_told::room().lock().unwrap();
                *room = crate::room_told::RoomTold::new();
                assert_eq!(room.at(now), Room::Unknown);
                if start.talk {
                    room.heard(90, now);
                    room.ended(90, now);
                    assert!(room.unanswered(90, now));
                }
            }
            let mut heeding = Heeding::new(true, true, Some(&root), None);
            heeding.as_the_only_voice(false, true, false);
            heeding.keeping_line_rooms();
            if start.on {
                heeding.showing_the_first_seconds_quiet();
            }
            if start.other_voice {
                heeding.keep_line_room(98, Room::Unknown, true, true);
                heeding.keep_line_room(99, Room::Unknown, false, false);
            }
            if start.held {
                // From 11.5 s to 10 s ago, before the name: no name, nobody
                // in view, no conversation going on.
                assert!(
                    heeding
                        .heed(
                            97,
                            (now - 11_500_000, now - 10_000_000),
                            "Did you see the match last night?",
                            None
                        )
                        .is_some()
                );
            }
            // "Lamp." from 5 s to 4.4 s ago, a pause, the request from 3 s
            // to 0.2 s ago; the ears read every 100 ms over the last 8 s,
            // or 12 s with the held line, the person's own lines 20 dB over
            // the quiet.
            let windows = [
                (now - 5_000_000, now - 4_400_000),
                (now - 3_000_000, now - 200_000),
            ];
            if let Some(rises) = start.room {
                let levels: Levels = Arc::default();
                let from = now - if start.held { 12_000_000 } else { 8_000_000 };
                for (n, at) in (from..=now).step_by(100_000).enumerate() {
                    let said = windows
                        .iter()
                        .any(|(began, ended)| (*began..=*ended).contains(&at));
                    let rise = if said { 20.0 } else { rises[n % rises.len()] };
                    levels.lock().unwrap().push_back((at, rise));
                }
                heeding.levels = Some(levels);
            }
            if let Some(floor) = start.floor {
                ears.publish_json(
                    monotonic_us(),
                    &RoomSound {
                        level_db: floor + 1.0,
                        floor_db: Some(floor),
                        own_voice: false,
                    },
                )
                .unwrap();
            }
            for (request, window) in [1, 2].into_iter().zip(windows) {
                eyes.publish_json(
                    monotonic_us(),
                    &Faces {
                        faces: (0..start.in_view)
                            .map(|n| Face {
                                speaking: None,
                                id: format!("f{n}"),
                                xmin: 0.4,
                                ymin: 0.3,
                                xmax: 0.6,
                                ymax: 0.6,
                                confidence: 1.0,
                                yaw: Some(0.1),
                                facing: Some(true),
                            })
                            .collect(),
                    },
                )
                .unwrap();
                heeding.keep_unjudged(request, window);
            }
            let _ = std::fs::remove_dir_all(&root);
            *crate::room_told::room().lock().unwrap() = crate::room_told::RoomTold::new();
            heeding.one_speaker(1, 2)
        };
        let held = |said: Joined| matches!(said, Joined::Not(why) if why.ends_with("not known to be quiet"));
        assert!(
            matches!(run("a", quiet), Joined::NoSecond(why) if why.contains("a quiet room")),
            "nobody in view, the ears' quiet room, nothing else since the start"
        );
        // lamp-c0a4, 27 Oct (7071): a quiet of -45 dBFS, levels -43 to -46.
        assert!(
            matches!(
                run(
                    "c0a4",
                    Start {
                        floor: Some(-45.0),
                        room: Some(&[2.0, -1.0, 0.5, 1.0, -0.5]),
                        ..quiet
                    }
                ),
                Joined::NoSecond(_)
            ),
            "lamp-c0a4's quiet room"
        );
        assert!(
            held(run(
                "b",
                Start {
                    other_voice: true,
                    ..quiet
                }
            )),
            "another voice heard since the start"
        );
        assert!(
            held(run(
                "c",
                Start {
                    floor: Some(-28.0),
                    ..quiet
                }
            )),
            "a loud room's quiet"
        );
        assert!(
            held(run(
                "c2",
                Start {
                    room: Some(&[1.0, 9.0, 12.0, 3.0]),
                    ..quiet
                }
            )),
            "talk or a television between the lines"
        );
        assert!(
            held(run(
                "d",
                Start {
                    talk: true,
                    ..quiet
                }
            )),
            "talk left alone since the start"
        );
        assert!(
            held(run(
                "d2",
                Start {
                    held: true,
                    ..quiet
                }
            )),
            "a line held since the start"
        );
        assert!(
            held(run(
                "e",
                Start {
                    floor: None,
                    ..quiet
                }
            )),
            "no level from the ears"
        );
        assert!(
            held(run(
                "e2",
                Start {
                    room: None,
                    ..quiet
                }
            )),
            "no levels sampled"
        );
        assert!(
            matches!(run("f", Start { in_view: 2, ..quiet }), Joined::Not(why) if why.contains("2 faces in view")),
            "two faces in view"
        );
        assert!(
            held(run("g", Start { on: false, ..quiet })),
            "the switch off"
        );
    }

    /// The same rule's verdicts as the engine asks them: the owner's request
    /// with a pause, one face in view, is asked whole; two faces in view, or
    /// no eyes in a busy room or one not known to be quiet, keep the second
    /// line alone; a held second line, too long a pause, or the switch off,
    /// as before.
    #[tokio::test]
    async fn a_request_with_a_pause_and_one_face_in_view_is_asked_whole() {
        use crate::room_told::Room::{Busy, Quiet, Unknown};
        use crate::second_provider::net::Message;
        use crate::second_provider::tests::asked_after_a_cut;
        let lines = ("What's the weather like", "in Saigon this weekend?");
        let joined = Message::user("What's the weather like in Saigon this weekend?");
        let alone = Message::user("in Saigon this weekend?");
        let rule = |name: &str, in_view, room| -> Option<crate::second_provider::SameSpeaker> {
            let heeding = Arc::new(Mutex::new(judged_in_rooms(name, in_view, room)));
            Some(Arc::new(move |cut, request| {
                heeding.lock().unwrap().same_speaker(cut, request)
            }))
        };
        let soon = Duration::from_millis(1_500);
        assert_eq!(
            asked_after_a_cut(lines, soon, rule("n", [Some(1), Some(1)], Busy), false).await,
            joined,
            "one face"
        );
        assert_eq!(
            asked_after_a_cut(lines, soon, rule("o", [Some(1), Some(2)], Quiet), false).await,
            alone,
            "two faces"
        );
        assert_eq!(
            asked_after_a_cut(lines, soon, rule("p", [None, None], Quiet), false).await,
            joined,
            "no eyes, a quiet room"
        );
        assert_eq!(
            asked_after_a_cut(lines, soon, rule("q", [None, None], Busy), false).await,
            alone,
            "no eyes, a busy room"
        );
        assert_eq!(
            asked_after_a_cut(lines, soon, rule("q2", [None, None], Unknown), false).await,
            alone,
            "no eyes, a room not known to be quiet"
        );
        assert_eq!(
            asked_after_a_cut(lines, soon, rule("r", [Some(1), Some(1)], Quiet), true).await,
            Message::user("What's the weather like"),
            "held: the cut words alone, in its place"
        );
        assert_eq!(
            asked_after_a_cut(
                lines,
                crate::second_provider::JOIN_WITHIN + Duration::from_millis(100),
                rule("s", [Some(1), Some(1)], Quiet),
                false
            )
            .await,
            alone,
            "too long a pause"
        );
        assert_eq!(
            asked_after_a_cut(lines, soon, None, false).await,
            alone,
            "the switch off"
        );
    }

    #[test]
    fn a_line_begins_with_the_name_only_when_it_is_set_off_as_a_call() {
        for heard in [
            "Lamp, what time is it?",
            "Hey Lamp, turn the light on.",
            "hey lamp turn the light on",
            "Lamb. What time is it?",
        ] {
            assert!(crate::attention::begins_with_the_name(heard), "{heard}");
        }
        for heard in [
            "I left the lamp on in the kitchen.",
            "lamp turn the light on",
            "Okay, hey Len. How are you today?",
            "Milan, what is the capital of Italy?",
            "Len said she would come.",
            "Hey there, lamp.",
            "",
        ] {
            assert!(!crate::attention::begins_with_the_name(heard), "{heard}");
        }
    }

    /// #1712: the name note, as the Live route gives it: the model is told
    /// when the lamp's name was heard as the line began or during it, not
    /// for a hearing long before; in a busy room, with `needs_face`, only
    /// with someone turned to the lamp or a sure hearing.
    #[test]
    fn the_name_heard_as_a_line_began_earns_the_live_routes_note() {
        use crate::room_told::Room;
        let signs = |room: Room, names: Vec<(u64, f32)>, turned: Option<bool>| super::Signs {
            room,
            going_on: false,
            lamp_asked: false,
            turned,
            names,
            near: false,
            continues_from: None,
        };
        let line = (10_000_000, 12_000_000);
        let heard = crate::called::LEAST_SCORE;
        let mut heeding = super::Heeding::new(false, false, None, None);
        heeding.note_the_name(
            1,
            line,
            &signs(Room::Quiet, vec![(10_200_000, heard)], None),
        );
        assert_eq!(heeding.name_note_for(1), None, "not switched on");
        heeding.telling_the_name(true, false);
        heeding.note_the_name(
            2,
            line,
            &signs(Room::Quiet, vec![(10_200_000, heard)], None),
        );
        assert_eq!(heeding.name_note_for(2), Some(crate::called::note(false)));
        assert_eq!(heeding.name_note_for(1), None, "only the request judged");
        heeding.note_the_name(3, line, &signs(Room::Quiet, vec![(1_000_000, heard)], None));
        assert_eq!(heeding.name_note_for(3), None, "heard long before the line");
        heeding.note_the_name(
            4,
            line,
            &signs(Room::Busy, vec![(10_200_000, heard)], Some(false)),
        );
        assert_eq!(heeding.name_note_for(4), None, "busy, nobody turned");
        heeding.note_the_name(
            5,
            line,
            &signs(Room::Busy, vec![(10_200_000, heard)], Some(true)),
        );
        assert!(heeding.name_note_for(5).is_some(), "busy, someone turned");
        let sure = crate::attention::SURE_SCORE;
        heeding.note_the_name(6, line, &signs(Room::Busy, vec![(10_200_000, sure)], None));
        assert!(heeding.name_note_for(6).is_some(), "busy, a sure hearing");
        heeding.telling_the_name(false, true);
        heeding.note_the_name(
            7,
            line,
            &signs(Room::Busy, vec![(10_200_000, heard)], Some(false)),
        );
        assert_eq!(heeding.name_note_for(7), Some(crate::called::note(true)));
        // Both switches (Fourth on #1712): the heard-as line, and in a busy
        // room only with someone turned to the lamp.
        heeding.telling_the_name(true, true);
        heeding.note_the_name(
            8,
            line,
            &signs(Room::Busy, vec![(10_200_000, heard)], Some(true)),
        );
        assert_eq!(heeding.name_note_for(8), Some(crate::called::note(true)));
        heeding.note_the_name(
            9,
            line,
            &signs(Room::Busy, vec![(10_200_000, heard)], Some(false)),
        );
        assert_eq!(heeding.name_note_for(9), None, "busy, nobody turned");
    }

    /// #1712: without the Policy holding, a call the Live route would hold
    /// for the Policy is said as that route says one it lets run ("; run
    /// after N ms"), which the battery counts; a lookup, which no route
    /// holds, is not. With the Policy holding, the gate says its own line
    /// with its wait (`policy::Desk`), so this one is not said as well.
    #[test]
    fn a_call_the_live_route_would_hold_is_said_as_run_for_the_battery() {
        let exec = crate::tools::Route::Executive("/run/lampos-actions".into());
        assert!(super::run_after(true, &exec, "light_on", 7).is_none());
        let line = super::run_after(false, &exec, "light_on", 7).unwrap();
        assert_eq!(
            line,
            "lamp-voice second: request 7 asked for light_on; run after 0 ms: this route holds no call for the Policy"
        );
        let counted = line
            .split("; run after ")
            .nth(1)
            .and_then(|rest| rest.split(" ms").next())
            .and_then(|ms| ms.parse::<u64>().ok());
        assert_eq!(counted, Some(0), "the battery's pattern reads it");
        assert!(super::run_after(false, &crate::tools::Route::Clock, "the_time", 7).is_none());
    }

    /// #1712 item 6 on #1969: the route reads the Policy's decisions once.
    /// With the lamp's actions held and a line going on asked about both,
    /// one subscriber is on `policy/decisions`, and both read each decision
    /// by its request: the line's verdict, and the decision's kind.
    #[test]
    fn the_lamps_actions_and_a_line_going_on_read_one_reader_of_the_policy() {
        let bus = PathBuf::from(format!("/tmp/lspo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&bus);
        std::fs::create_dir_all(&bus).unwrap();
        let root = lamp_bus::Bus::new(&bus).unwrap();
        let mut decisions = root
            .publisher_declared(
                lamp_msgs::Decision::TOPIC,
                &lamp_msgs::Decision::declaration(),
                "policy",
            )
            .unwrap();
        let mut decide = |request: u64, choice: &str| {
            let decision = serde_json::json!({"about": "attention", "choice": choice,
                "probability": 0.235, "answers": {"follows_lamp": 0.7},
                "by": "typesafe/jev-1.13-20260917", "took_ms": 540, "shadow": false,
                "request": request});
            decisions.publish_json(0, &decision).unwrap();
        };
        let mut heeding = super::Heeding::new(true, false, None, None);
        assert_eq!(heeding.policy_verdict(1), None, "no reader, nothing");
        heeding.reading_the_policy(super::policy_verdicts(true, true, Some(&bus)).unwrap());
        decide(1, "stay_quiet");
        assert_eq!(
            heeding.policy_verdict(1),
            Some(crate::policy_holds::Verdict::StayQuietFollowing)
        );
        // Asked of a line going on too: the same reader, what it read kept.
        heeding.asking_the_policy(&bus);
        assert_eq!(
            root.subscribers(lamp_msgs::Decision::TOPIC).unwrap().len(),
            1,
            "one reader"
        );
        assert_eq!(
            heeding.policy_verdict(1),
            Some(crate::policy_holds::Verdict::StayQuietFollowing),
            "the reader's decisions kept"
        );
        decide(2, "answer");
        assert_eq!(
            heeding.policy_verdict(2),
            Some(crate::policy_holds::Verdict::TakenUp)
        );
        assert_eq!(heeding.policy_verdict(3), None);
        let _ = std::fs::remove_dir_all(&bus);
    }

    /// #1712 item 6: the second route waits for the Policy's decisions only
    /// with the switch, the bus, and its rule, which knows each turn's name
    /// and conversation; without any of them its actions run at once.
    #[test]
    fn the_policy_holds_this_routes_actions_only_with_the_bus_and_the_rule() {
        let dir = std::path::PathBuf::from(format!("/tmp/lspv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(super::policy_verdicts(true, true, Some(&dir)).is_some());
        assert!(super::policy_verdicts(false, true, Some(&dir)).is_none());
        assert!(super::policy_verdicts(true, true, None).is_none());
        assert!(super::policy_verdicts(true, false, Some(&dir)).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #1712 item 6: what the Policy's gate is told of a turn, as the Live
    /// worker tells it of its own (#800, #1054): the lamp's name heard in
    /// it at `NAME_FROM` or more; that it follows someone's line the lamp
    /// left unanswered; that the lamp's words were heard within 20 s before
    /// it. Each of the turn judged last only.
    #[test]
    fn the_policy_is_told_the_name_a_line_left_unanswered_and_the_lamp_speaking_lately() {
        let _clock = clock_with_history();
        use crate::second_provider::policy::Turn;
        let signs = |room, names: Vec<(u64, f32)>| super::Signs {
            room,
            going_on: false,
            lamp_asked: false,
            turned: None,
            names,
            near: false,
            continues_from: None,
        };
        const S: u64 = 1_000_000;
        let now = monotonic_us();
        let mut heeding = super::Heeding::new(false, true, None, None);
        heeding.judge(
            1,
            (now, now + 2 * S),
            "Lamp, turn the light on.",
            signs(crate::room_told::Room::Quiet, vec![(now, 0.95)]),
        );
        assert_eq!(
            heeding.policy_turn(1),
            Turn {
                name_heard: true,
                ..Turn::default()
            }
        );
        assert_eq!(heeding.policy_turn(2), Turn::default(), "not judged");
        // A name heard below `NAME_FROM` is not the name heard.
        heeding.judge(
            2,
            (now + 10 * S, now + 12 * S),
            "Lamp, a bit warmer.",
            signs(crate::room_told::Room::Quiet, vec![(now + 10 * S, 0.85)]),
        );
        assert!(!heeding.policy_turn(2).name_heard);
        assert_eq!(heeding.policy_turn(1), Turn::default(), "the last only");
        // Someone's line to someone else, held, then one just after it.
        let mut room = super::Heeding::new(false, true, None, None);
        room.as_the_only_voice(false, true, false);
        assert!(
            room.judge(
                3,
                (now + 20 * S, now + 22 * S),
                "Daniel, are you coming to the meeting at two?",
                signs(crate::room_told::Room::Quiet, Vec::new()),
            )
            .is_some()
        );
        room.judge(
            4,
            (now + 23 * S, now + 25 * S),
            "Yes, give me five minutes.",
            signs(crate::room_told::Room::Quiet, Vec::new()),
        );
        assert!(room.policy_turn(4).after_unanswered);
        assert!(!heeding.policy_turn(2).after_unanswered);
        // The lamp's words heard to their end 19 s ago, then 21 s ago.
        heeding.lamp_did(crate::second_provider::Did::Said(
            1,
            monotonic_us() - 19 * S,
        ));
        heeding.judge(
            5,
            (now + 30 * S, now + 32 * S),
            "And the volume.",
            signs(crate::room_told::Room::Quiet, Vec::new()),
        );
        assert!(heeding.policy_turn(5).lamp_spoke_lately);
        let mut quiet = super::Heeding::new(false, true, None, None);
        quiet.lamp_did(crate::second_provider::Did::Said(
            1,
            monotonic_us() - 21 * S,
        ));
        quiet.judge(
            6,
            (now, now + 2 * S),
            "And the volume.",
            signs(crate::room_told::Room::Quiet, Vec::new()),
        );
        assert!(!quiet.policy_turn(6).lamp_spoke_lately);
    }

    /// Tests' map on #1712 (20:11), (2): `LAMP_REMARKS` on the second
    /// route as the lamp's only voice. In a busy room a remark to the room,
    /// let out only as the conversation going on, is held as on Live (#829,
    /// the umbrella and the car); what calls the lamp plays; without the
    /// switch the remark plays. With `--attention` busy or not, and with
    /// `--spoken-to` or not.
    #[test]
    fn as_the_only_voice_a_remark_to_the_room_is_held_with_the_switch() {
        use crate::room_told::Room;
        let signs = || super::Signs {
            room: Room::Busy,
            going_on: true,
            lamp_asked: false,
            turned: None,
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let line = (1_000_000_000, 1_001_500_000);
        let remarks = [
            "I should bring an umbrella then.",
            "Okay, I will book the car for eight.",
        ];
        for strict in [false, true] {
            for spoken_to in [false, true] {
                let judged = |held_back: bool, heard: &str| {
                    let mut heeding = super::Heeding::new(strict, false, None, None);
                    heeding.as_the_only_voice(held_back, spoken_to, false);
                    heeding.judge(9, line, heard, signs())
                };
                for remark in remarks {
                    assert_eq!(
                        judged(true, remark),
                        Some(crate::remarks::REMARK),
                        "{remark}, strict {strict}, spoken_to {spoken_to}"
                    );
                    assert_eq!(judged(false, remark), None, "{remark} without the switch");
                }
                assert_eq!(judged(true, "Lamp, and tomorrow?"), None, "called");
            }
        }
    }

    /// Tests' map on #1712 (20:11), (2): `LAMP_CUT_INS_NOT_TALK` on the second
    /// route as the only voice. The room this route keeps (`keeping_room`)
    /// leaves lines said over the lamp's own answer out of its talk, as the
    /// Live worker's room does: three such lines left alone do not make it
    /// busy; without the switch they do. The engine marks such a line at its
    /// Start (`over_the_lamps_voice`, `second_provider.rs`). In tests the room
    /// is the thread's own, so each case runs on a thread of its own.
    #[test]
    fn as_the_only_voice_lines_said_over_the_lamp_are_not_talk_with_the_switch() {
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        let busy_after_three = |cut_ins_not_talk: bool| {
            std::thread::spawn(move || {
                let _tick = super::keeping_room(false, cut_ins_not_talk, false, None, None);
                let mut room = crate::room_told::room().lock().unwrap();
                let now = monotonic_us();
                for request in 1..=3 {
                    room.heard(request, now);
                    room.began_over_the_lamp(request);
                    room.ended(request, now);
                    room.unanswered(request, now);
                }
                room.at(now) == crate::room_told::Room::Busy
            })
            .join()
            .unwrap()
        };
        assert!(!busy_after_three(true), "with the switch: not talk");
        assert!(busy_after_three(false), "without it: talk, and busy");
    }

    /// `LAMP_BUSY_ENDS_SOONER` on the second route as the only voice (the
    /// first builder on Queue #1722, 04:00): the room this route keeps ends
    /// a busy room a minute after the last talk when nothing since was
    /// answered (`end_busy_sooner`), and a minute after the lamp's first
    /// answer since the talk when it was, as Live's does
    /// (`end_busy_once_answered`); without the switch, three minutes after
    /// the talk either way.
    #[test]
    fn as_the_only_voice_an_answer_ends_a_busy_room_a_minute_later_with_the_switch() {
        use crate::room_told::Room;
        // One test at a time on the process's thread and room (the reviewer's P2 on #2283).
        let _one = crate::provider_worker::tests::one_thread();
        const S: u64 = 1_000_000;
        // The room around `end` s after the talk, answered `answered` s
        // after it or not at all.
        let room_at = |ends_sooner: bool, answered: Option<u64>, end: u64| {
            std::thread::spawn(move || {
                let _tick = super::keeping_room(true, false, ends_sooner, None, None);
                let mut room = crate::room_told::room().lock().unwrap();
                let start = 1_000 * S;
                for request in 1..=3 {
                    room.heard(request, start);
                    room.ended(request, start);
                    room.unanswered(request, start);
                }
                assert_eq!(room.at(start), Room::Busy);
                if let Some(answered) = answered {
                    room.heard(4, start + answered * S);
                    room.spoke(4, start + answered * S);
                }
                let at = [
                    room.at(start + (end - 1) * S),
                    room.at(start + (end + 1) * S),
                ];
                *room = crate::room_told::RoomTold::new();
                at
            })
            .join()
            .unwrap()
        };
        let ends = [Room::Busy, Room::Quiet];
        // Not answered: a minute after the talk with the switch.
        assert_eq!(room_at(true, None, 60), ends, "with, unanswered");
        assert_eq!(
            room_at(false, None, 60),
            [Room::Busy; 2],
            "without, unanswered"
        );
        // Answered 30 s after the talk: a minute after the answer with it.
        assert_eq!(room_at(true, Some(30), 90), ends, "with, answered");
        assert_eq!(
            room_at(false, Some(30), 90),
            [Room::Busy; 2],
            "without, answered"
        );
        // Without the switch, three minutes after the talk either way.
        assert_eq!(room_at(false, None, 180), ends, "without, 180 s");
    }

    /// Tests' map on #1712 (20:11), (2): `LAMP_SPOKEN_TO=day-and-night` on
    /// the second route as the only voice. By day, with the day's own bars,
    /// a name heard close to the lamp at 0.95 in a busy room, nobody turned
    /// to it and no words calling it, is held, as on Live (#829, the eye
    /// drops); with `on` alone it plays. The night's bars are not read here:
    /// this route never tells its rule the night (Live does, from
    /// `local_time::night_here`), so it judges every hour as day.
    #[test]
    fn as_the_only_voice_the_days_bars_hold_a_name_heard_close_by_day() {
        use crate::room_told::Room;
        let signs = || super::Signs {
            room: Room::Busy,
            going_on: false,
            lamp_asked: false,
            turned: Some(false),
            names: vec![(1_000_100_000, 0.95)],
            near: true,
            continues_from: None,
        };
        let line = (1_000_000_000, 1_001_500_000);
        let judged = |by_day: bool| {
            let mut heeding = super::Heeding::new(true, false, None, None);
            heeding.as_the_only_voice(false, true, by_day);
            // By day, whatever the hour the test runs at (Third on #1712,
            // 21:36: from 21:00 it failed once #1928 read the night).
            heeding.night = || false;
            heeding.judge(9, line, "What time is it?", signs())
        };
        assert!(judged(true).is_some(), "by day, held");
        assert_eq!(judged(false), None, "with on alone, played");
    }

    /// #1712, item 4 (`LAMP_GOING_ON_ASKS`, #1522): in a busy room, a line
    /// the rule would let out only as a conversation going on waits for the
    /// Policy's decision, then is played on `answer` and held on
    /// `stay_quiet`, as on the Live route; without the switch, or in a room
    /// not busy, nothing waits. With no decision the line waits until
    /// `policy_holds::WAIT` (here, no Policy's decision before the bound).
    #[test]
    fn in_a_busy_room_a_line_going_on_waits_for_the_policy_and_goes_as_it_says() {
        use crate::room_told::Room;
        let signs = |room| super::Signs {
            room,
            going_on: true,
            lamp_asked: false,
            turned: Some(false),
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        // Short: a socket's whole path must fit in 103 bytes.
        let bus = PathBuf::from(format!("/tmp/lgo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&bus);
        std::fs::create_dir_all(&bus).unwrap();
        let mut decisions = lamp_bus::Bus::new(&bus)
            .unwrap()
            .publisher_declared(
                lamp_msgs::Decision::TOPIC,
                &lamp_msgs::Decision::declaration(),
                "policy",
            )
            .unwrap();
        let mut decide = |request: u64, choice: &str| {
            let decision = serde_json::json!({"about": "attention", "choice": choice,
                "probability": 0.235, "answers": {}, "by": "typesafe/jev-1.13-20260917",
                "took_ms": 540, "shadow": false, "request": request});
            decisions.publish_json(0, &decision).unwrap();
        };
        let line = (1_000_000_000, 1_001_500_000);
        let mut heeding = super::Heeding::new(true, false, None, None);
        heeding.asking_the_policy(&bus);
        let said = "And what about tomorrow?";
        assert_eq!(
            heeding.judge(9, line, said, signs(Room::Busy)),
            Some(crate::second_provider::WAITS_FOR_POLICY)
        );
        assert_eq!(heeding.policy_now(9), None, "nothing decided yet");
        assert_eq!(heeding.policy_now(8), None, "not the line that waits");
        decide(9, "answer");
        assert_eq!(heeding.policy_now(9), Some(None), "played on answer");
        assert_eq!(heeding.policy_now(9), None, "decided once");
        assert_eq!(
            heeding.judge(10, line, said, signs(Room::Busy)),
            Some(crate::second_provider::WAITS_FOR_POLICY)
        );
        decide(10, "stay_quiet");
        assert_eq!(
            heeding.policy_now(10),
            Some(Some(crate::going_on_asks::NOT_THE_LAMPS)),
            "held on stay_quiet"
        );
        // A decision already there: judged at once, nothing waits.
        decide(11, "stay_quiet");
        assert_eq!(
            heeding.judge(11, line, said, signs(Room::Busy)),
            Some(crate::going_on_asks::NOT_THE_LAMPS)
        );
        // With no decision in `policy_holds::WAIT`, it is played.
        assert_eq!(
            heeding.judge(13, line, said, signs(Room::Busy)),
            Some(crate::second_provider::WAITS_FOR_POLICY)
        );
        assert_eq!(heeding.policy_now(13), None);
        let waiting = heeding.waiting.as_mut().unwrap();
        waiting.since = waiting
            .since
            .checked_sub(crate::policy_holds::WAIT)
            .unwrap();
        assert_eq!(
            heeding.policy_now(13),
            Some(None),
            "played with no decision"
        );
        // In a room not busy, nothing waits.
        assert_eq!(heeding.judge(12, line, said, signs(Room::Quiet)), None);
        // Without the switch, nothing waits.
        let mut without = super::Heeding::new(true, false, None, None);
        assert_ne!(
            without.judge(9, line, said, signs(Room::Busy)),
            Some(crate::second_provider::WAITS_FOR_POLICY)
        );
        let _ = std::fs::remove_dir_all(&bus);
    }

    /// #1712, item 4 (`LAMP_FOLLOWS_ON`): the rule knows which line it
    /// judged in a busy room, so that line is not told it may follow on.
    #[test]
    fn a_line_judged_in_a_busy_room_is_known_as_judged_busy() {
        use crate::room_told::Room;
        let signs = |room| super::Signs {
            room,
            going_on: false,
            lamp_asked: false,
            turned: Some(false),
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let line = (1_000_000_000, 1_001_500_000);
        let mut heeding = super::Heeding::new(true, false, None, None);
        heeding.judge(9, line, "Lamp, what time is it?", signs(Room::Busy));
        assert!(heeding.judged_busy(9));
        assert!(!heeding.judged_busy(8));
        heeding.judge(10, line, "Lamp, what time is it?", signs(Room::Quiet));
        assert!(!heeding.judged_busy(10) && !heeding.judged_busy(9));
    }

    /// #1712, lamp-4ace's A/B: after a hand-over and 104 s, every line
    /// without the name was held as no conversation, where Live answered.
    /// With `--spoken-to`, as on the Live worker: a question about the lamp's
    /// work 105 s after a tool turn is for it, and so is a line 105 s after
    /// its own news, in a room not busy; with neither, it is held.
    #[test]
    fn after_a_tool_turn_or_the_lamps_news_a_line_105_s_later_is_judged_as_live_judges_it() {
        let signs = || super::Signs {
            room: crate::room_told::Room::Quiet,
            going_on: false,
            lamp_asked: false,
            turned: None,
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let at = |seconds: u64| 1_000_000_000 + seconds * 1_000_000;
        let line = |seconds: u64| (at(seconds), at(seconds) + 1_500_000);
        let judged = |did: Option<crate::second_provider::Did>, heard: &str| {
            let mut heeding = super::Heeding::new(false, false, None, None);
            heeding.as_the_only_voice(false, true, false);
            if let Some(did) = did {
                heeding.lamp_did(did);
            }
            heeding.judge(9, line(105), heard, signs())
        };
        use crate::second_provider::Did;
        let held = judged(None, "How is that task going?");
        assert_eq!(held, Some(crate::attention::NOT_SPOKEN_TO));
        assert_eq!(
            judged(Some(Did::Worked(at(0))), "How is that task going?"),
            None
        );
        assert_eq!(
            judged(None, "What time is it?"),
            Some(crate::attention::NOT_SPOKEN_TO)
        );
        assert_eq!(judged(Some(Did::Spoke(at(0))), "What time is it?"), None);
    }

    /// #1712, lamp-4ace's busy room: colleagues' talk just after a named
    /// request was let out only as the conversation going on, and this
    /// route's model answered it. With `--spoken-to`, such a line earns the
    /// note; a line that calls the lamp, answers its question, or is judged
    /// without the switch, does not; nor does one with no conversation going
    /// on, which is held.
    #[test]
    fn a_line_let_out_only_as_the_conversation_going_on_earns_the_note() {
        let signs = |going_on: bool, lamp_asked: bool| super::Signs {
            room: crate::room_told::Room::Quiet,
            going_on,
            lamp_asked,
            turned: None,
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let line = (1_000_000_000, 1_001_500_000);
        let noted = |spoken_to: bool, heard: &str, signs: super::Signs| {
            let mut heeding = super::Heeding::new(false, false, None, None);
            heeding.as_the_only_voice(false, spoken_to, false);
            let held = heeding.judge(9, line, heard, signs);
            (
                held,
                heeding.conversation_note_for(9),
                heeding.conversation_note_for(8),
            )
        };
        let talk = "I saved a hundred dollars on the flight.";
        assert_eq!(
            noted(true, talk, signs(true, false)),
            (None, Some(super::CONVERSATION_ALONE), None)
        );
        assert_eq!(
            noted(true, "Lamp, what about tomorrow?", signs(true, false)),
            (None, None, None)
        );
        assert_eq!(noted(true, talk, signs(true, true)), (None, None, None));
        assert_eq!(noted(false, talk, signs(true, false)), (None, None, None));
        assert_eq!(
            noted(true, talk, signs(false, false)),
            (Some(crate::attention::NOT_SPOKEN_TO), None, None)
        );
        // The next line, let out as the first thing said after the lamp's
        // news, does not keep the note of the line before.
        let mut heeding = super::Heeding::new(false, false, None, None);
        heeding.as_the_only_voice(false, true, false);
        assert_eq!(heeding.judge(9, line, talk, signs(true, false)), None);
        heeding.lamp_did(crate::second_provider::Did::Spoke(1_002_000_000));
        let next = (1_003_000_000, 1_004_000_000);
        assert_eq!(heeding.judge(10, next, talk, signs(true, false)), None);
        assert_eq!(heeding.conversation_note_for(10), None);
        // #1712: a word or two in it, no question and no short word, is the
        // room's; a short word said to the lamp, or a question, goes on.
        for (heard, held) in [
            ("Surprise!", true),
            ("Oh wow.", true),
            ("Louder.", false),
            ("Thank you.", false),
            ("And Paris?", false),
            ("What's the price of gold?", false),
        ] {
            let mut heeding = super::Heeding::new(false, false, None, None);
            heeding.as_the_only_voice(false, true, false);
            let verdict = heeding.judge(9, line, heard, signs(true, false));
            assert_eq!(verdict, held.then_some(super::LONE_WORDS), "{heard}");
            assert_eq!(heeding.conversation_note_for(9).is_some(), !held, "{heard}");
        }
        // With a name, a word or two is the lamp's.
        let mut heeding = super::Heeding::new(false, false, None, None);
        heeding.as_the_only_voice(false, true, false);
        assert_eq!(
            heeding.judge(9, line, "Lamp, surprise!", signs(true, false)),
            None
        );
        // #1712, line 8 of the battery: in such a line, nothing set to come.
        let mut heeding = super::Heeding::new(false, false, None, None);
        heeding.as_the_only_voice(false, true, false);
        assert_eq!(
            heeding.judge(9, line, "Yes, give me five minutes.", signs(true, false)),
            None
        );
        assert!(heeding.holds_call(9, "set_timer"));
        assert!(heeding.holds_call(9, "set_reminder"));
        assert!(heeding.holds_call(9, "cancel_reminder"));
        assert!(!heeding.holds_call(9, "the_time"));
        assert!(!heeding.holds_call(8, "set_timer"), "another request");
        assert_eq!(
            heeding.judge(10, next, "Lamp, set a timer.", signs(true, false)),
            None
        );
        assert!(!heeding.holds_call(10, "set_timer"), "called by name");
        // #1712 (the first builder at 04:54): a line that names the
        // schedule sets it, as Live's busy-room rule lets it, and isn't told
        // it may be people's talk; nor is one that asks to remember.
        let mut heeding = super::Heeding::new(false, false, None, None);
        heeding.as_the_only_voice(false, true, false);
        assert_eq!(
            heeding.judge(
                11,
                line,
                "Remind me in two minutes to stretch.",
                signs(true, false)
            ),
            None
        );
        assert!(!heeding.holds_call(11, "set_reminder"));
        assert_eq!(heeding.conversation_note_for(11), None);
        let mut heeding = super::Heeding::new(false, false, None, None);
        heeding.as_the_only_voice(false, true, false);
        assert_eq!(
            heeding.judge(
                12,
                line,
                "Remember that my favourite colour is green.",
                signs(true, false)
            ),
            None
        );
        assert_eq!(heeding.conversation_note_for(12), None);
        let mut heeding = super::Heeding::new(false, false, None, None);
        heeding.as_the_only_voice(false, true, false);
        assert_eq!(
            heeding.judge(14, line, "Forget my favourite colour.", signs(true, false)),
            None
        );
        assert_eq!(heeding.conversation_note_for(14), None);
        // An answer to the lamp's own question sets what it asks for.
        let mut heeding = super::Heeding::new(false, false, None, None);
        heeding.as_the_only_voice(false, true, false);
        assert_eq!(
            heeding.judge(15, line, "Yes, give me five minutes.", signs(true, true)),
            None
        );
        assert!(!heeding.holds_call(15, "set_timer"));
        // Eighth on #2054: a request going on from one cut short is judged
        // on the whole chain's words, so a first half naming the reminder,
        // or asking to remember, counts.
        for (first, then, call) in [
            (
                "Remind me",
                "in two minutes to stretch.",
                Some("set_reminder"),
            ),
            ("Remember that", "my favourite colour is green.", None),
        ] {
            let mut heeding = super::Heeding::new(false, false, None, None);
            heeding.as_the_only_voice(false, true, false);
            heeding.judge(16, line, first, signs(true, false));
            let going_on = super::Signs {
                continues_from: Some(16),
                ..signs(true, false)
            };
            assert_eq!(
                heeding.judge(17, next, &format!("{first} {then}"), going_on),
                None,
                "{first}"
            );
            if let Some(call) = call {
                assert!(!heeding.holds_call(17, call), "{first}");
            }
            assert_eq!(heeding.conversation_note_for(17), None, "{first}");
        }
        // Talk going on from talk still gets the note.
        let mut heeding = super::Heeding::new(false, false, None, None);
        heeding.as_the_only_voice(false, true, false);
        heeding.judge(16, line, "I saved", signs(true, false));
        let going_on = super::Signs {
            continues_from: Some(16),
            ..signs(true, false)
        };
        assert_eq!(heeding.judge(17, next, talk, going_on), None);
        assert_eq!(
            heeding.conversation_note_for(17),
            Some(super::CONVERSATION_ALONE)
        );
        // Talk that names neither still gets the note.
        let mut heeding = super::Heeding::new(false, false, None, None);
        heeding.as_the_only_voice(false, true, false);
        assert_eq!(heeding.judge(13, line, talk, signs(true, false)), None);
        assert_eq!(
            heeding.conversation_note_for(13),
            Some(super::CONVERSATION_ALONE)
        );
    }

    /// lamp-52e6, 24 Oct 15:33: over the lamp's story, "Sorry, can you say
    /// that last part again?" was let out only as the conversation going on,
    /// told it may be people's talk, and got `not_for_me`. A line begun over
    /// the lamp's own answer is said to it: no such note. Begun with the lamp
    /// quiet, or over the wait sound, the same line still earns it.
    #[test]
    fn a_line_begun_over_the_lamps_answer_is_not_told_it_may_be_peoples_talk() {
        use crate::second_provider::Heard;
        let signs = super::Signs {
            room: crate::room_told::Room::Quiet,
            going_on: true,
            lamp_asked: false,
            turned: None,
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let line = (1_000_000_000, 1_001_500_000);
        let noted = |heard: Option<Heard<'static>>| {
            let mut heeding = super::Heeding::new(false, false, None, None);
            heeding.as_the_only_voice(false, true, false);
            if let Some(heard) = heard {
                heeding.hear(9, heard);
            }
            let held = heeding.judge(
                9,
                line,
                "Sorry, can you say that last part again?",
                signs.clone(),
            );
            (held, heeding.conversation_note_for(9))
        };
        let over = |over_the_lamps_voice| Heard::Began {
            over_the_lamps_voice,
        };
        assert_eq!(noted(Some(over(true))), (None, None), "over the lamp");
        for (heard, why) in [
            (Some(over(false)), "the lamp quiet"),
            (None, "not heard"),
            (Some(Heard::BeganOverTheWaitSound), "over the wait sound"),
        ] {
            assert_eq!(
                noted(heard),
                (None, Some(super::CONVERSATION_ALONE)),
                "{why}"
            );
        }
    }

    /// #2136 (the first builder at 11:29 and 11:37): A asked for a story by
    /// name, and over it answered B, "Yes, give me five minutes." With the
    /// switch, a reply begun over the lamp's answer, let out only as the
    /// conversation going on and answering nothing the lamp asked, is held
    /// as someone else's. Not without the switch, not when it began with the
    /// lamp quiet, not when the lamp asked, not a line that isn't a reply,
    /// and not one that calls the lamp.
    #[test]
    fn a_reply_to_nothing_the_lamp_asked_said_over_its_answer_is_someone_elses() {
        use crate::second_provider::Heard;
        let signs = |lamp_asked: bool| super::Signs {
            room: crate::room_told::Room::Quiet,
            going_on: true,
            lamp_asked,
            turned: None,
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let line = (1_000_000_000, 1_001_500_000);
        let judged = |on: bool, over: bool, lamp_asked: bool, heard: &str| {
            let mut heeding = super::Heeding::new(false, false, None, None);
            heeding.as_the_only_voice(false, true, false);
            if on {
                heeding.leaving_others_answers();
            }
            heeding.hear(
                9,
                Heard::Began {
                    over_the_lamps_voice: over,
                },
            );
            heeding.judge(9, line, heard, signs(lamp_asked))
        };
        let reply = "Yes, give me five minutes.";
        assert_eq!(
            judged(true, true, false, reply),
            Some(crate::others_answer::HELD)
        );
        assert_eq!(judged(false, true, false, reply), None, "the switch off");
        assert_eq!(
            judged(true, false, false, reply),
            None,
            "begun with the lamp quiet"
        );
        assert_eq!(judged(true, true, true, reply), None, "the lamp asked");
        assert_eq!(
            judged(true, true, false, "Tell me more about the fox."),
            None,
            "not a reply"
        );
        assert_eq!(
            judged(
                true,
                true,
                false,
                "Yes, Lamp, set a timer for five minutes."
            ),
            None,
            "it calls the lamp"
        );
        // A correction or a question to the lamp, said over its answer by
        // someone not facing it, goes on (the reviewer's P2 on #2175).
        for heard in ["No, the fox one.", "Yes, but who was he?"] {
            assert_eq!(judged(true, true, false, heard), None, "{heard}");
        }
    }

    /// The reviewer's P3 on #2175: with the switch alone, no voice check,
    /// the rule's hearing hook is installed, so the rule learns which line
    /// began over the lamp's answer; with neither, there's none.
    #[test]
    fn the_hearing_hook_is_installed_with_the_switch_alone() {
        use crate::second_provider::Heard;
        use std::sync::{Arc, Mutex};
        let heeding = Arc::new(Mutex::new(super::Heeding::new(false, false, None, None)));
        assert!(super::hearing_hook(&heeding, false, false).is_none());
        assert!(super::hearing_hook(&heeding, true, false).is_some());
        let hook = super::hearing_hook(&heeding, false, true).expect("with the switch alone");
        heeding.lock().unwrap().leaving_others_answers();
        hook(
            9,
            Heard::Began {
                over_the_lamps_voice: true,
            },
        );
        assert_eq!(heeding.lock().unwrap().began_over, Some(9));
    }

    /// #1712: spoken turns are told how to take what they heard; the
    /// character comes first, unchanged.
    #[test]
    fn spoken_turns_are_told_to_answer_the_likeliest_meaning() {
        let told = super::spoken_character("CHARACTER", false, false);
        assert!(told.starts_with("CHARACTER\n\n"));
        assert!(!told.contains("take_a_look"));
        assert!(super::spoken_character("CHARACTER", true, false).ends_with(super::LOOK_CALL_NOTE));
        // #1712 (the first builder at 00:34): with a memory, remember,
        // forget and recall are named; without one, none is.
        assert!(!told.contains("call remember"));
        let remembers = super::spoken_character("CHARACTER", false, true);
        assert!(remembers.ends_with(super::MEMORY_CALLS_NOTE));
        for call in [
            "call remember",
            "call forget",
            "call recall",
            "\"please forget that\" or \"forget what I just told you\", call forget",
            "\"forget it, it doesn't matter\" or \"never mind\" about the conversation is not that",
        ] {
            assert!(remembers.contains(call), "{call}");
        }
        assert!(told.contains("answer it for the likeliest meaning, here first"));
        assert!(
            !told.contains("not_for_me"),
            "the decline is the line's own note"
        );
        assert!(told.contains("ask back only when you cannot guess at all"));
        assert!(told.contains("for the time of day it is now"));
        assert!(told.contains("at least four lines"));
    }

    /// #1712: a plain ask for something new is searched as heard, the name
    /// left off and the month and year after it; anything else is not.
    #[test]
    fn a_plain_ask_for_something_new_is_searched_from_its_words() {
        let when = || Some("October 2026".to_owned());
        assert_eq!(
            super::early_query("Hey Lamp, who won the last Formula One race?", when()).as_deref(),
            Some("who won the last Formula One race October 2026")
        );
        assert_eq!(
            super::early_query("What's the price of gold today?", None).as_deref(),
            Some("What's the price of gold today?")
        );
        assert_eq!(
            super::early_query("Lamp, check the weather in Da Nang.", when()).as_deref(),
            Some("check the weather in Da Nang October 2026")
        );
        for heard in [
            "What time is it?",
            "Tell me a story about a fox.",
            "I saw the news this morning.",
            "Make your light a little warmer.",
            "",
        ] {
            assert_eq!(super::early_query(heard, when()), None, "{heard}");
        }
    }

    /// #1712: the request's first search takes the one begun early, once;
    /// another request's search does not.
    #[tokio::test]
    async fn the_search_begun_early_is_taken_once_by_its_request() {
        let early = super::Early::default();
        early.begin(
            7,
            "who won the race October 2026".into(),
            Box::pin(async { serde_json::json!({"results": ["early"]}) }),
        );
        assert!(early.take(8, "who won").is_none(), "another request's");
        let (_, answer) = early.take(7, "Who won the race?").unwrap().unwrap();
        assert_eq!(answer.await, serde_json::json!({"results": ["early"]}));
        assert!(early.take(7, "who won").is_none(), "taken once");
    }

    /// lamp-4ace, 20 Oct: "what's the weather this weekend" was searched as
    /// heard and the model, which had asked for the lamp's own city, was
    /// given American towns' forecasts. A query that adds words the heard
    /// ones lacked is searched as asked; one that only drops or reorders
    /// them, or adds small words, takes the early answer.
    #[tokio::test]
    async fn the_models_query_adding_a_place_is_not_answered_by_the_early_search() {
        let heard = "What's the weather this weekend October 2026";
        assert_eq!(
            super::early_adds(heard, "Ho Chi Minh City weather this weekend"),
            4
        );
        assert_eq!(super::early_adds(heard, "weather in Saigon"), 1);
        assert_eq!(super::early_adds(heard, "the weather for this weekend"), 0);
        assert_eq!(super::early_adds(heard, "weekend weather October 2026"), 0);
        assert_eq!(super::early_adds(heard, ""), 0);
        let early = super::Early::default();
        early.begin(
            3,
            heard.into(),
            Box::pin(async { serde_json::json!({"results": ["early"]}) }),
        );
        assert!(matches!(
            early.take(3, "weather in Saigon this weekend"),
            Some(Err(1))
        ));
        assert!(
            early.take(3, "weather this weekend").is_none(),
            "taken once, either way"
        );
    }

    /// #1712: the request's first `web_search` is answered by the search
    /// begun early when it asks nothing more; a later one, or one asking
    /// more, goes to the service (here a closed port, so it fails).
    #[tokio::test]
    async fn a_requests_first_search_takes_the_one_begun_early() {
        let search = Arc::new(
            crate::second_provider::net::search::Search::deepseek_at("http://127.0.0.1:9", "k")
                .unwrap(),
        );
        let early = Arc::new(super::Early::default());
        let runner = super::tool_runner(
            crate::tools::Settings::default(),
            Some(search),
            Some(early.clone()),
            false,
        );
        early.begin(
            5,
            "who won the race".into(),
            Box::pin(async { serde_json::json!({"results": ["early"]}) }),
        );
        let args = serde_json::json!({"query": "who won the race"});
        assert_eq!(
            runner(5, true, "web_search".into(), args.clone()).await,
            serde_json::json!({"results": ["early"]})
        );
        let again = runner(5, true, "web_search".into(), args).await;
        assert!(again.get("error").is_some(), "{again}");
        early.begin(
            6,
            "what's the weather this weekend".into(),
            Box::pin(async { serde_json::json!({"results": ["early"]}) }),
        );
        let asked = serde_json::json!({"query": "Saigon weather this weekend"});
        let own = runner(6, true, "web_search".into(), asked).await;
        assert!(own.get("error").is_some(), "searched as asked: {own}");
    }

    /// The first builder on #2602 (22 Oct 12:3x): the look is one of the
    /// core, so a list already at the bound before it keeps it and the bound
    /// cuts the next in its order instead, `fetch_page` first. On lamp-75c9
    /// at 12:2x the head's list was cut so; one at the bound with the look
    /// added after it had left the look out, and said nothing.
    /// The route's own bound is past Live's and within what the chat model
    /// it asks takes: DeepSeek's API, up to 128 functions a request (the
    /// Reviewer-lead's P3 on #2961). A chat provider with a lower limit
    /// fails here first.
    #[test]
    fn the_routes_bound_is_past_lives_and_within_the_chat_models_own() {
        const DEEPSEEK_MOST_FUNCTIONS: usize = 128;
        let bound = crate::second_provider::MAX_TOOLS;
        assert!(bound > lamp_gemini::MAX_TOOLS, "{bound}");
        assert!(bound <= DEEPSEEK_MOST_FUNCTIONS, "{bound}");
    }

    #[test]
    fn a_spoken_turns_look_is_never_the_one_the_bound_cuts() {
        let tool = |name: &str| lamp_gemini::Tool {
            name: name.into(),
            description: String::new(),
            parameters: None,
            silent: false,
        };
        let settings = crate::tools::Settings::default();
        let lamp = crate::tools::declared(&settings).len();
        // The search's two, and others of its, to exactly the bound.
        let mut search = vec![
            tool(crate::second_provider::net::search::NAME),
            tool(crate::second_provider::net::search::FETCH),
        ];
        let fill = crate::second_provider::MAX_TOOLS - lamp - search.len();
        search.extend((0..fill).map(|n| tool(&format!("search_{n}"))));
        let names = |tools: &[lamp_gemini::Tool]| -> Vec<String> {
            tools.iter().map(|tool| tool.name.clone()).collect()
        };
        let plain = chat_tools(&settings, &search, false);
        assert_eq!(
            plain.len(),
            crate::second_provider::MAX_TOOLS,
            "exactly at the bound"
        );
        assert!(!names(&plain).contains(&crate::second_provider::LOOK_CALL.to_owned()));
        let looked = names(&chat_tools(&settings, &search, true));
        assert_eq!(looked.len(), crate::second_provider::MAX_TOOLS);
        assert!(looked.contains(&crate::second_provider::LOOK_CALL.to_owned()));
        assert!(!looked.contains(&crate::second_provider::net::search::FETCH.to_owned()));
        assert!(looked.contains(&crate::second_provider::net::search::NAME.to_owned()));
        // With nothing left in the cut's order to cut, the last that isn't
        // core leaves, never the look.
        let others: Vec<_> = (0..crate::second_provider::MAX_TOOLS - lamp)
            .map(|n| tool(&format!("other_{n}")))
            .collect();
        let past = names(&chat_tools(&settings, &others, true));
        assert_eq!(past.len(), crate::second_provider::MAX_TOOLS);
        assert!(past.contains(&crate::second_provider::LOOK_CALL.to_owned()));
        assert!(!past.contains(&format!(
            "other_{}",
            crate::second_provider::MAX_TOOLS - lamp - 1
        )));
        // Below the bound, nothing leaves for it.
        let room = chat_tools(&settings, &search[..2], true);
        assert_eq!(room.len(), lamp + 3);
        // The route's own bound, not Live's: on lamp-4ace on 22 Oct the
        // lamp's, the skills' and the search's came to five past Live's 32,
        // and Live's bound left the search out. Here nothing leaves.
        let past_live: Vec<_> = search[..2]
            .iter()
            .cloned()
            .chain((0..lamp_gemini::MAX_TOOLS + 5 - lamp - 2).map(|n| tool(&format!("skill_{n}"))))
            .collect();
        let kept = names(&chat_tools(&settings, &past_live, false));
        assert_eq!(kept.len(), lamp_gemini::MAX_TOOLS + 5);
        assert!(kept.contains(&crate::second_provider::net::search::NAME.to_owned()));
        assert!(kept.contains(&crate::second_provider::net::search::FETCH.to_owned()));
        // The probe's list, fitted already: the look kept, the last leaves.
        let full: Vec<_> = (0..crate::second_provider::MAX_TOOLS)
            .map(|n| tool(&format!("f{n}")))
            .collect();
        let probed = names(&super::with_look(full));
        assert_eq!(probed.len(), crate::second_provider::MAX_TOOLS);
        assert_eq!(
            probed.last().map(String::as_str),
            Some(crate::second_provider::LOOK_CALL)
        );
        assert_eq!(
            names(&super::with_look(vec![tool("light_on")])),
            ["light_on", crate::second_provider::LOOK_CALL]
        );
    }

    /// #1712, lamp-4ace 19 Oct: "Hey Lamp, how are you today?" heard as
    /// "Hey, Lynn. How are you today?", and "Lamp, what time is it?" as
    /// "Lynn, what time is it?", were held as said to someone else. As the
    /// lamp runs (the name in passing ignored, `--spoken-to`), they are its
    /// calls; a question to a person by name is still held.
    #[test]
    fn the_lamps_name_heard_as_lynn_is_its_call_not_someone_elses() {
        let signs = || super::Signs {
            room: crate::room_told::Room::Quiet,
            going_on: false,
            lamp_asked: false,
            turned: None,
            names: Vec::new(),
            near: false,
            continues_from: None,
        };
        let line = (1_000_000_000, 1_002_000_000);
        let judged = |heard: &str| {
            let mut heeding = super::Heeding::new(false, true, None, None);
            heeding.as_the_only_voice(false, true, false);
            heeding.judge(9, line, heard, signs())
        };
        assert_eq!(judged("Hey, Lynn. How are you today?"), None);
        assert_eq!(judged("Lynn, what time is it?"), None);
        assert!(judged("Daniel, are you coming to the meeting?").is_some());
    }

    /// #1712, the first builder at 18:09: a line its words or the lamp's
    /// name called the lamp for is told to answer, with no decline line; a
    /// line let out by a face alone gets the decline line.
    #[test]
    fn a_line_said_by_name_is_told_to_answer_and_a_face_only_line_may_decline() {
        let signs = |turned: Option<bool>, names: Vec<(u64, f32)>| super::Signs {
            room: crate::room_told::Room::Busy,
            going_on: false,
            lamp_asked: false,
            turned,
            names,
            near: false,
            continues_from: None,
        };
        let line = (1_000_000_000, 1_002_000_000);
        let note = |heard: &str, signs: super::Signs| {
            let mut heeding = super::Heeding::new(true, true, None, None);
            heeding.as_the_only_voice(false, true, false);
            assert_eq!(heeding.judge(9, line, heard, signs), None, "{heard}");
            heeding.call_note_for(9)
        };
        assert_eq!(
            note("Lamp, what time is it?", signs(None, Vec::new())),
            super::BY_NAME_NOTE
        );
        assert_eq!(
            note("Hey Lamp, tell me a short joke.", signs(None, Vec::new())),
            super::BY_NAME_NOTE
        );
        assert_eq!(
            note(
                "What time is it?",
                signs(None, vec![(1_000_000_000, crate::attention::SURE_SCORE)])
            ),
            super::BY_NAME_NOTE,
            "lamp-name heard it"
        );
        assert_eq!(
            note("What time is it?", signs(Some(true), Vec::new())),
            super::DECLINE_NOTE
        );
        assert!(super::BY_NAME_NOTE.contains("answer it"));
        assert!(!super::BY_NAME_NOTE.contains("not_for_me"));
        assert!(super::DECLINE_NOTE.contains("call not_for_me and say nothing"));
    }

    /// #1712: a hearing of the name counts for a piece begun at `from` when
    /// it is sure enough and no earlier than the rule's own allowance before.
    #[test]
    fn a_name_hearing_counts_from_its_time_and_score() {
        let from = 10_000_000;
        let sure = crate::called::LEAST_SCORE;
        assert!(super::name_counts(sure, from + 200_000, from));
        assert!(super::name_counts(
            sure,
            from - crate::called::BEFORE_US,
            from
        ));
        assert!(!super::name_counts(
            sure,
            from - crate::called::BEFORE_US - 1,
            from
        ));
        assert!(!super::name_counts(sure - 0.01, from + 200_000, from));
    }

    /// A fetcher of Open-Meteo's answers with no network, counting each
    /// fetch: every name found as Hanoi, and one forecast for it.
    fn counted_weather_fetch() -> (
        Arc<std::sync::atomic::AtomicUsize>,
        crate::second_provider::net::weather::Fetch,
    ) {
        let fetched = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = fetched.clone();
        let fetch: crate::second_provider::net::weather::Fetch = Arc::new(move |url: String| {
            count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Box::pin(async move {
                Ok(if url.contains("geocoding-api") {
                    serde_json::json!({"results": [{"name": "Hanoi", "country": "Vietnam",
                                                    "latitude": 21.0245, "longitude": 105.84117}]})
                } else {
                    serde_json::json!({
                        "utc_offset_seconds": 25_200,
                        "current": {"temperature_2m": 24.0, "weather_code": 61},
                        "daily": {"time": ["2026-10-22", "2026-10-23"],
                                  "weather_code": [61, 3],
                                  "temperature_2m_max": [27, 29],
                                  "temperature_2m_min": [22, 23],
                                  "precipitation_probability_max": [80, 20]}
                    })
                })
            })
        });
        (fetched, fetch)
    }

    /// A clock set at 10:00 on Thursday 22 Oct 2026 in Hanoi, seven hours
    /// ahead of UTC: [`counted_weather_fetch`]'s today.
    fn thursday_in_hanoi() -> crate::second_provider::net::weather::Clock {
        // 03:00 UTC on 22 Oct 2026, 20,748 days after 1 January 1970.
        Arc::new(|| 20_748 * 86_400 + 3 * 3_600)
    }

    /// The owner on 22 Oct: no weather is fetched every 15 minutes any
    /// more. With the switch on nothing is fetched at start or as time
    /// passes, only when a call asks, and the call is answered by the
    /// route's runners, each of them, before the lamp's own calls.
    #[tokio::test(start_paused = true)]
    async fn the_weather_is_fetched_only_when_a_call_asks() {
        use std::sync::atomic::Ordering;
        let (fetched, fetch) = counted_weather_fetch();
        let weather = super::weather_asked(
            Some("on"),
            Some("Hanoi, Vietnam".into()),
            fetch,
            thursday_in_hanoi(),
        )
        .expect("on");
        tokio::time::advance(Duration::from_secs(60 * 60)).await;
        assert_eq!(fetched.load(Ordering::Relaxed), 0, "nothing ahead");
        let settings = crate::tools::Settings::default();
        let runner = super::answering_weather(
            super::tool_runner(settings.clone(), None, None, false),
            Some(weather.clone()),
        );
        let asked = serde_json::json!({"place": "Hanoi", "day": "tomorrow"});
        let told = runner(7, true, "weather".into(), asked.clone()).await;
        assert_eq!(
            told,
            serde_json::json!({"place": "Hanoi, Vietnam",
                               "weather": "In Hanoi, Vietnam: Tomorrow, Friday, 23 to 29°C, overcast, 20% chance of rain."})
        );
        assert_eq!(fetched.load(Ordering::Relaxed), 2);
        let other = runner(7, true, "light_on".into(), serde_json::json!({})).await;
        assert!(other.get("place").is_none(), "{other}");
        let (owner, context) = audio_runner_scope();
        let scoped = super::scoped_answering_weather(
            super::scoped_tool_runner(settings.clone(), None, None, false),
            Some(weather.clone()),
        );
        assert_eq!(
            scoped(7, true, "weather".into(), asked.clone(), owner.lease()).await,
            told
        );
        let audio = super::audio_answering_weather(
            super::requested_audio_runner(settings, None, None, false),
            Some(weather),
        );
        let call = lamp_gemini::ToolCall {
            id: context.call_id.clone(),
            name: "weather".into(),
            args: asked,
        };
        let answered = audio(context, call, owner.lease()).await;
        assert_eq!(answered.told, told);
        assert!(answered.requested_audio.is_none());
        assert_eq!(
            fetched.load(Ordering::Relaxed),
            2,
            "kept for the follow-ups"
        );
    }

    /// With the switch off (anything but `on`) there is no weather: no
    /// function declared, nothing answered or fetched for one, and the
    /// Policy is told nothing new. With it on, spoken turns are given it
    /// after the search's, and the Policy never holds it.
    #[tokio::test]
    async fn the_weather_switch_off_declares_and_fetches_nothing() {
        use std::sync::atomic::Ordering;
        let (fetched, fetch) = counted_weather_fetch();
        for switch in [None, Some(""), Some("yes"), Some("ON")] {
            let clock = thursday_in_hanoi();
            assert!(
                super::weather_asked(switch, Some("Hanoi".into()), fetch.clone(), clock).is_none()
            );
        }
        let settings = crate::tools::Settings::default();
        let named = |tools: Vec<lamp_gemini::Tool>| -> Vec<String> {
            tools.into_iter().map(|tool| tool.name).collect()
        };
        let search = lamp_gemini::Tool {
            name: crate::second_provider::net::search::NAME.into(),
            description: String::new(),
            parameters: None,
            silent: false,
        };
        let off = named(chat_tools(
            &settings,
            &super::with_weather_tool(std::slice::from_ref(&search), false),
            false,
        ));
        assert!(!off.iter().any(|name| name == "weather"), "{off:?}");
        let on = named(super::with_weather_tool(&[search], true));
        assert_eq!(on, ["web_search", "weather"]);
        let runner =
            super::answering_weather(super::tool_runner(settings, None, None, false), None);
        let told = runner(
            8,
            true,
            "weather".into(),
            serde_json::json!({"place": "Hanoi"}),
        )
        .await;
        assert!(told.get("place").is_none(), "not the weather's: {told}");
        assert_eq!(fetched.load(Ordering::Relaxed), 0);
        let here = super::answered_here_with_weather(super::answered_here(None), false);
        assert!(!here("weather"));
        let here = super::answered_here_with_weather(super::answered_here(None), true);
        assert!(here("weather") && here(super::MORE) && !here("light_on"));
        assert!(super::given_at_once("weather"));
    }

    /// The weather is cut at this route's bound with the search's group:
    /// after the skills' that aren't a core skill's, before `web_search`.
    #[test]
    fn the_weather_is_cut_after_the_skills_and_before_the_search() {
        let cut = super::chat_cut(vec!["installed".into()]);
        let at = |name: &str| cut.iter().position(|cut| cut == name).unwrap();
        assert!(at("installed") < at("weather") && at("weather") < at("web_search"));
        let tool = |name: String| lamp_gemini::Tool {
            name,
            description: "A function.".into(),
            parameters: None,
            silent: false,
        };
        let told = |lamp: usize| -> Vec<lamp_gemini::Tool> {
            (0..lamp)
                .map(|i| format!("tool_{i}"))
                .chain(["dance", "installed", "weather", "web_search"].map(String::from))
                .map(tool)
                .collect()
        };
        let kept = |lamp: usize| -> Vec<String> {
            crate::tools::within(told(lamp), &cut, crate::second_provider::MAX_TOOLS)
                .unwrap()
                .into_iter()
                .map(|tool| tool.name)
                .collect()
        };
        let one_over = kept(crate::second_provider::MAX_TOOLS - 3);
        assert!(
            !one_over.iter().any(|name| name == "installed"),
            "{one_over:?}"
        );
        assert!(
            one_over.iter().any(|name| name == "weather"),
            "{one_over:?}"
        );
        let two_over = kept(crate::second_provider::MAX_TOOLS - 2);
        assert!(
            !two_over.iter().any(|name| name == "weather"),
            "{two_over:?}"
        );
        assert!(
            two_over.iter().any(|name| name == "web_search"),
            "{two_over:?}"
        );
        assert!(two_over.iter().any(|name| name == "dance"), "{two_over:?}");
    }

    /// #1712: the lamp's place follows its time, or stands alone.
    #[test]
    fn the_lamps_place_follows_its_time() {
        let place = crate::local_time::place_told(Some("Hanoi"));
        assert_eq!(
            super::with_place(Some("It is noon.".into()), place.clone()).unwrap(),
            format!("It is noon.{}", place.clone().unwrap())
        );
        assert!(
            super::with_place(None, place)
                .unwrap()
                .starts_with("You are in Hanoi.")
        );
        assert_eq!(
            super::with_place(Some("It is noon.".into()), None).as_deref(),
            Some("It is noon.")
        );
        assert_eq!(super::with_place(None, None), None);
    }

    /// #1712: the room this route keeps is told on the bus (`room/state`)
    /// at the worker's tick, as the Live worker tells its own.
    #[test]
    fn the_room_the_route_keeps_is_told_on_the_bus() {
        let root = std::env::temp_dir().join(format!("lamp-second-room-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut tick = super::keeping_room(false, false, false, None, Some(&root));
        tick(monotonic_us());
        let told = lamp_bus::Bus::new(&root)
            .unwrap()
            .latest(lamp_msgs::RoomState::TOPIC)
            .unwrap()
            .expect("the room is told");
        assert!(told.parse::<lamp_msgs::RoomState>().is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Gemini's key: the voice unit's credential `gemini.key`, as
    /// OpenRouter's and DeepSeek's are, else the Live route's when that is a
    /// Google API key. A `gemini.key` there but not private is refused, not
    /// passed over for the Live key. With the Live route through a relay and
    /// no credential, a Gemini switch keeps the worker from starting: the
    /// relay's key is never sent to Google, and without a Gemini switch
    /// nothing is asked for.
    #[test]
    fn gemini_is_called_with_the_units_key_or_the_live_routes_google_key_never_a_relays() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "lamp-second-gemini-key-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let credentials = dir.join("credentials");
        let empty = dir.join("no-credentials");
        let open = dir.join("open-credentials");
        for directory in [&credentials, &empty, &open] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let private = |path: std::path::PathBuf, text: &str, mode: u32| {
            std::fs::write(&path, text).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            path
        };
        let llm = private(dir.join("llm.key"), "relay-key", 0o600);
        let live = private(dir.join("live.key"), "live-google-key\n", 0o600);
        private(credentials.join("gemini.key"), "unit-google-key", 0o600);
        private(open.join("gemini.key"), "open-google-key", 0o644);
        let config = private(
            dir.join("second.json"),
            &format!(
                r#"{{"llm_base_url":"https://relay.example.invalid/v1","llm_model":"m","llm_credential_file":"{}"}}"#,
                llm.display()
            ),
            0o600,
        );
        let second = || crate::second_config::load_from_path(&config).unwrap();
        let provider = |endpoint: &str| -> crate::config::ProviderConfig {
            serde_json::from_str(&format!(
                r#"{{"endpoint":"{endpoint}","model":"m","voice":"Kore","credential_file":"{}"}}"#,
                live.display()
            ))
            .unwrap()
        };
        let direct = provider(lamp_gemini::GOOGLE_ENDPOINT);
        let relay = provider("wss://relay.example.invalid/ws/gemini");
        let unit = Some(credentials.as_os_str());
        for live_route in [&relay, &direct] {
            assert_eq!(
                super::gemini_key(live_route, unit).unwrap().expose(),
                "unit-google-key"
            );
        }
        for none in [None, Some(empty.as_os_str())] {
            assert_eq!(
                super::gemini_key(&direct, none).unwrap().expose(),
                "live-google-key"
            );
        }
        let not_private = super::gemini_key(&direct, Some(open.as_os_str()))
            .unwrap_err()
            .to_string();
        assert!(not_private.contains("gemini.key"), "{not_private}");
        assert!(!not_private.contains("open-google-key"));
        let refused = super::gemini_key(&relay, None).unwrap_err().to_string();
        assert!(refused.contains("gemini.key"), "{refused}");
        assert!(!refused.contains("relay-key") && !refused.contains("live-google-key"));
        let none: super::super::net::Declare = std::sync::Arc::new(Vec::new);
        let speech = super::RouteFlags {
            speech: Some(super::SpeechRoute::Gemini),
            ..super::RouteFlags::default()
        };
        assert!(super::services(second(), &relay, speech, None, none.clone()).is_err());
        assert!(super::services(second(), &relay, speech, unit, none.clone()).is_ok());
        assert!(
            super::services(
                second(),
                &relay,
                super::RouteFlags::default(),
                None,
                none.clone()
            )
            .is_ok()
        );
        assert!(super::services(second(), &direct, speech, None, none).is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// OpenRouter's and DeepSeek's keys are the voice unit's credentials:
    /// with them the chat starts, without them the worker does not, saying
    /// which credential it lacks and nothing of a key; DeepSeek's search the
    /// same; the Exa search needs its service named.
    #[test]
    fn the_hosted_chats_and_searches_take_the_units_credentials() {
        use super::{ChatRoute, RouteFlags, SearchRoute};
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!(
            "lamp-second-hosted-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let credentials = dir.join("credentials");
        std::fs::create_dir_all(&credentials).unwrap();
        let private = |path: std::path::PathBuf, text: &str| {
            std::fs::write(&path, text).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            path
        };
        let llm = private(dir.join("llm.key"), "relay-key");
        let second = private(
            dir.join("second.json"),
            &format!(
                r#"{{"llm_base_url":"https://relay.example.invalid/v1","llm_model":"m","llm_credential_file":"{}"}}"#,
                llm.display()
            ),
        );
        let config = || crate::second_config::load_from_path(&second).unwrap();
        let provider: crate::config::ProviderConfig = serde_json::from_str(&format!(
            r#"{{"endpoint":"wss://relay.example.invalid/ws","model":"m","voice":"Kore","credential_file":"{}"}}"#,
            llm.display()
        ))
        .unwrap();
        let none: super::super::net::Declare = std::sync::Arc::new(Vec::new);
        let chat = |chat| RouteFlags {
            chat: Some(chat),
            ..RouteFlags::default()
        };
        let search = |search| RouteFlags {
            search: Some(search),
            ..RouteFlags::default()
        };
        let dirs = Some(credentials.as_os_str());
        for (route, name) in [
            (chat(ChatRoute::OpenRouter), "openrouter.key"),
            (chat(ChatRoute::DeepSeek), "deepseek.key"),
        ] {
            let error = super::services(config(), &provider, route, dirs, none.clone())
                .err()
                .unwrap()
                .to_string();
            assert!(error.contains(name), "{error}");
        }
        let error = super::search_for(&config(), search(SearchRoute::DeepSeek), dirs)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("deepseek.key"), "{error}");
        private(credentials.join("openrouter.key"), "or-secret\n");
        private(credentials.join("deepseek.key"), "ds-secret\n");
        for route in [chat(ChatRoute::OpenRouter), chat(ChatRoute::DeepSeek)] {
            assert!(super::services(config(), &provider, route, dirs, none.clone()).is_ok());
            // Without the unit's credentials at all.
            let error = super::services(config(), &provider, route, None, none.clone())
                .err()
                .unwrap()
                .to_string();
            assert!(error.contains("CREDENTIALS_DIRECTORY"), "{error}");
            assert!(!error.contains("secret"), "{error}");
        }
        let found = super::search_for(&config(), search(SearchRoute::DeepSeek), dirs)
            .unwrap()
            .unwrap();
        assert!(found.answers(super::super::net::search::FETCH));
        assert!(
            super::search_for(&config(), search(SearchRoute::OpenRouter), dirs)
                .unwrap()
                .is_none()
        );
        assert!(super::search_for(&config(), search(SearchRoute::Exa), dirs).is_err());
        assert!(
            super::search_for(&config(), RouteFlags::default(), dirs)
                .unwrap()
                .is_none()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// #1934: with `deepseek_base_url`, DeepSeek's chat and search go to the
    /// relay with the relay's credential (`llm_credential_file`), and a lamp
    /// with no `deepseek.key`, nor any unit credential at all, runs. The
    /// chat's request is DeepSeek's (`/chat/completions`, thinking off),
    /// the search's DeepSeek's (`/v1/tools/search`), each with the relay
    /// key as its bearer.
    #[tokio::test]
    async fn through_the_relay_deepseek_takes_the_relay_key_and_needs_no_deepseek_key() {
        use super::super::services::Services;
        use super::{ChatRoute, RouteFlags, SearchRoute};
        use futures_util::StreamExt;
        use std::os::unix::fs::PermissionsExt;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = std::env::temp_dir().join(format!(
            "lamp-second-relay-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let private = |path: std::path::PathBuf, text: &str| {
            std::fs::write(&path, text).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            path
        };
        let llm = private(dir.join("llm.key"), "relay-key");
        let second = private(
            dir.join("second.json"),
            &format!(
                r#"{{"llm_base_url":"https://relay.example.invalid/v1","llm_model":"m","llm_credential_file":"{}","deepseek_base_url":"https://relay.example.invalid/deepseek"}}"#,
                llm.display()
            ),
        );
        let loaded = crate::second_config::load_from_path(&second).unwrap();
        assert_eq!(
            loaded.hosted.deepseek_base_url.as_deref(),
            Some("https://relay.example.invalid/deepseek")
        );
        assert_eq!(
            super::deepseek_key(&loaded, None).unwrap().expose(),
            "relay-key"
        );
        // A local server stands in for the relay: each request's first line,
        // its bearer and its body.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut seen = Vec::new();
            for answer in [
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"Hi.\"}}]}\n\ndata: [DONE]\n\n",
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\ncontent-length: 14\r\n\r\n{\"results\":[]}",
            ] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 65_536];
                loop {
                    let n = socket.read(&mut buffer).await.unwrap();
                    request.extend_from_slice(&buffer[..n]);
                    let text = String::from_utf8_lossy(&request).to_string();
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let length: usize = head
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(|n| n.trim().parse().unwrap())
                            })
                            .unwrap_or(0);
                        if body.len() >= length {
                            break;
                        }
                    }
                }
                seen.push(String::from_utf8(request).unwrap());
                socket.write_all(answer.as_bytes()).await.unwrap();
            }
            seen
        });
        let mut config = loaded;
        config.hosted.deepseek_base_url = Some(format!("http://{address}/deepseek"));
        let provider: crate::config::ProviderConfig = serde_json::from_str(&format!(
            r#"{{"endpoint":"wss://relay.example.invalid/ws","model":"m","voice":"Kore","credential_file":"{}"}}"#,
            llm.display()
        ))
        .unwrap();
        let none: super::super::net::Declare = std::sync::Arc::new(Vec::new);
        let chat_route = RouteFlags {
            chat: Some(ChatRoute::DeepSeek),
            ..RouteFlags::default()
        };
        // No unit credentials at all: no deepseek.key is asked for.
        let net = super::services(config.clone(), &provider, chat_route, None, none).unwrap();
        let events: Vec<_> = net
            .chat(vec![super::super::net::Message::user("hello")])
            .collect()
            .await;
        assert!(events.iter().all(Result::is_ok), "{events:?}");
        let search_route = RouteFlags {
            search: Some(SearchRoute::DeepSeek),
            ..RouteFlags::default()
        };
        let search = super::search_for(&config, search_route, None)
            .unwrap()
            .unwrap();
        search
            .answer(
                super::super::net::search::NAME,
                &serde_json::json!({"query": "rain in Hanoi"}),
            )
            .await;
        let seen = server.await.unwrap();
        let chat = seen[0].to_lowercase();
        assert!(
            chat.starts_with("post /deepseek/chat/completions http/1.1"),
            "{chat}"
        );
        assert!(chat.contains("authorization: bearer relay-key"));
        assert!(chat.contains(r#""thinking":{"type":"disabled"}"#));
        let searched = seen[1].to_lowercase();
        assert!(
            searched.starts_with("post /deepseek/v1/tools/search http/1.1"),
            "{searched}"
        );
        assert!(searched.contains("authorization: bearer relay-key"));
        // #2094: the person's own key goes over the relay's, and the
        // journal's words name it, never the key.
        let mut theirs = config.clone();
        theirs.hosted.own_deepseek = crate::second_config::credential(&private(
            dir.join("own_deepseek.key"),
            "sk-planted-2094",
        ))
        .ok();
        assert_eq!(
            super::deepseek_key(&theirs, None).unwrap().expose(),
            "sk-planted-2094"
        );
        // Its host, not its path, at each start (Third's P3 on #2167).
        let said = super::deepseek_where(&theirs.hosted);
        let at = crate::second_config::host(theirs.hosted.deepseek_base_url.as_deref().unwrap());
        assert!(!at.is_empty());
        assert!(
            said.contains("person's own key")
                && said.contains(&format!("at {at} "))
                && !said.contains("sk-")
                && !said.contains("://"),
            "{said}"
        );
        // Without a base URL, DeepSeek's own API still needs deepseek.key.
        let mut own = config;
        own.hosted.deepseek_base_url = None;
        let error = super::deepseek_key(&own, None).err().unwrap().to_string();
        assert!(error.contains("CREDENTIALS_DIRECTORY"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// #2094: a request the person's own key is refused for (401, after a
    /// rotation) goes through the relay with the relay's key, never a failed
    /// answer, and `voice-keys` warns from then on; neither the journal's
    /// words nor the warning hold the key.
    #[tokio::test]
    async fn a_refused_own_key_sends_that_request_through_the_relay() {
        use super::super::services::Services;
        use super::{ChatRoute, RouteFlags};
        use futures_util::StreamExt;
        use std::os::unix::fs::PermissionsExt;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = std::env::temp_dir().join(format!(
            "lamp-second-own-refused-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let private = |path: std::path::PathBuf, text: &str| {
            std::fs::write(&path, text).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            path
        };
        let llm = private(dir.join("llm.key"), "relay-key");
        let second = private(
            dir.join("second.json"),
            &format!(
                r#"{{"llm_base_url":"https://relay.example.invalid/v1","llm_model":"m","llm_credential_file":"{}"}}"#,
                llm.display()
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut seen = Vec::new();
            for answer in [
                "HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"Hi.\"}}]}\n\ndata: [DONE]\n\n",
            ] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 65_536];
                loop {
                    let n = socket.read(&mut buffer).await.unwrap();
                    request.extend_from_slice(&buffer[..n]);
                    let text = String::from_utf8_lossy(&request).to_string();
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let length: usize = head
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(|n| n.trim().parse().unwrap())
                            })
                            .unwrap_or(0);
                        if body.len() >= length {
                            break;
                        }
                    }
                }
                seen.push(String::from_utf8(request).unwrap());
                socket.write_all(answer.as_bytes()).await.unwrap();
            }
            seen
        });
        let mut config = crate::second_config::load_from_path(&second).unwrap();
        config.hosted.deepseek_base_url = Some(format!("http://{address}/own"));
        config.hosted.relay_deepseek_base_url = Some(format!("http://{address}/relay"));
        config.hosted.own_deepseek = crate::second_config::credential(&private(
            dir.join("own_deepseek.key"),
            "sk-planted-2094-own",
        ))
        .ok();
        let provider: crate::config::ProviderConfig = serde_json::from_str(&format!(
            r#"{{"endpoint":"wss://relay.example.invalid/ws","model":"m","voice":"Kore","credential_file":"{}"}}"#,
            llm.display()
        ))
        .unwrap();
        let none: super::super::net::Declare = std::sync::Arc::new(Vec::new);
        let route = RouteFlags {
            chat: Some(ChatRoute::DeepSeek),
            ..RouteFlags::default()
        };
        let net = super::services(config, &provider, route, None, none).unwrap();
        let events: Vec<_> = net
            .chat(vec![super::super::net::Message::user("hello")])
            .collect()
            .await;
        assert!(events.iter().all(Result::is_ok), "{events:?}");
        let seen = server.await.unwrap();
        let own = seen[0].to_lowercase();
        assert!(own.starts_with("post /own/chat/completions"), "{own}");
        assert!(own.contains("authorization: bearer sk-planted-2094-own"));
        let relay = seen[1].to_lowercase();
        assert!(relay.starts_with("post /relay/chat/completions"), "{relay}");
        assert!(relay.contains("authorization: bearer relay-key"));
        assert!(!relay.contains("sk-planted"));
        assert_eq!(
            super::OWN_KEY_REFUSED.load(std::sync::atomic::Ordering::Relaxed),
            401
        );
        let warned = serde_json::to_string(&super::own_key_health(true, 401)).unwrap();
        assert!(warned.contains(super::OWN_KEY_REFUSED_WORDS), "{warned}");
        assert!(warned.contains("\"warn\""), "{warned}");
        assert!(!warned.contains("sk-"), "{warned}");
        let well = serde_json::to_string(&super::own_key_health(true, 0)).unwrap();
        assert!(well.contains("\"ok\""), "{well}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The reviewer's P2 on #2167: with the switch on, `voice-keys` reports
    /// whether the person's key is kept or not, so a key kept and then
    /// removed (the voice restarted without it) leaves a part that goes on
    /// saying `ok`, the lamp's own key, never one quiet for good. Off, the
    /// part never starts. On the bus, as lamp-health reads it.
    #[test]
    fn a_removed_own_key_leaves_voice_keys_reporting() {
        // Kept: well. Removed: still reported, well, the lamp's own key.
        let kept = super::own_key_report(true, true).unwrap();
        assert_eq!(kept.level, lamp_msgs::Level::Ok);
        let uses = |said: &lamp_msgs::Diagnostic| {
            said.values
                .iter()
                .find(|value| value.key == "uses")
                .map(|value| value.value.clone())
        };
        assert_eq!(uses(&kept).as_deref(), Some("own"));
        let removed = super::own_key_report(true, false).unwrap();
        assert_eq!(removed.level, lamp_msgs::Level::Ok);
        assert_eq!(uses(&removed).as_deref(), Some("lamp"));
        // Without the person's key nothing can be refused for it.
        assert_eq!(
            super::own_key_health(false, 401).level,
            lamp_msgs::Level::Ok
        );
        // Off: never started, so never stale.
        assert!(super::own_key_report(false, false).is_none());
        assert!(super::own_key_report(false, true).is_none());
        // A short root: a Unix socket's address holds 103 bytes.
        let root = std::path::PathBuf::from(format!("/tmp/lok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut hears = bus
            .subscribe_typed(
                lamp_msgs::Diagnostic::TOPIC,
                "test",
                lamp_msgs::Diagnostic::TYPE,
            )
            .unwrap();
        super::report_own_key(Some(&root), false);
        let mut heard = Vec::new();
        while heard.len() < 2 {
            let said = hears
                .recv(Duration::from_secs(5))
                .unwrap()
                .expect("voice-keys goes on reporting")
                .parse::<lamp_msgs::Diagnostic>()
                .unwrap();
            if said.name == super::OWN_KEY_HEALTH {
                heard.push(said);
            }
        }
        assert!(
            heard
                .iter()
                .all(|said| said.level == lamp_msgs::Level::Ok
                    && uses(said).as_deref() == Some("lamp")),
            "{heard:?}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// #2415's P3: own keys turned off at runtime (the second worker starts
    /// again with the switch off) and the second worker dropped (#960) each
    /// say once that `voice-keys` is stopping, so lamp-health forgets it
    /// instead of counting it quiet. Started with the switch on, or dropped
    /// with it off (the part never reported), nothing is said. On the bus,
    /// as lamp-health reads it.
    #[test]
    fn own_keys_turned_off_or_the_worker_dropped_says_voice_keys_is_stopping() {
        let stopping = lamp_msgs::Diagnostic::stopping(super::OWN_KEY_HEALTH);
        // Own keys turned off: the worker's start with the switch off.
        assert_eq!(
            super::own_key_stopping_at_start(false),
            Some(stopping.clone())
        );
        assert_eq!(super::own_key_stopping_at_start(true), None);
        // The second worker dropped, with the switch on.
        assert_eq!(
            super::own_key_stopping_on_drop(true),
            Some(stopping.clone())
        );
        assert_eq!(super::own_key_stopping_on_drop(false), None);
        let root = std::path::PathBuf::from(format!("/tmp/lks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut hears = bus
            .subscribe_typed(
                lamp_msgs::Diagnostic::TOPIC,
                "test",
                lamp_msgs::Diagnostic::TYPE,
            )
            .unwrap();
        super::say_own_key_once(Some(&root), &stopping);
        let said = hears
            .recv(Duration::from_secs(5))
            .unwrap()
            .expect("voice-keys says it is stopping")
            .parse::<lamp_msgs::Diagnostic>()
            .unwrap();
        assert_eq!(said, stopping);
        assert!(said.is_stopping());
        // Without a bus, nothing, and nothing fails.
        super::say_own_key_once(None, &stopping);
        let _ = std::fs::remove_dir_all(root);
    }

    /// The owner's rule at the bound (the first builder on #2614): the cut
    /// takes the search's fetch first, then the time elsewhere, then the
    /// skills' functions that are not a core skill's, then the search, and
    /// never a core skill's; the lamp's own and core skills' alone past 32
    /// are an error, and a session still opens with the core skill's.
    #[test]
    fn the_bound_cuts_the_fetch_then_the_time_and_never_a_core_skill() {
        let tool = |name: String| lamp_gemini::Tool {
            name,
            description: "A function.".into(),
            parameters: None,
            silent: false,
        };
        let lamp = |count: usize| -> Vec<lamp_gemini::Tool> {
            (0..count).map(|i| tool(format!("tool_{i}"))).collect()
        };
        let names = |tools: &[lamp_gemini::Tool]| -> Vec<String> {
            tools.iter().map(|tool| tool.name.clone()).collect()
        };
        let cut = super::chat_cut(vec!["installed".into()]);
        let has = |kept: &[String], name: &str| kept.iter().any(|told| told == name);
        // 28 of the lamp's, the time, a core skill's `dance`, an
        // installable one's, the search's two: 33, so the fetch goes.
        let mut told = lamp(28);
        for name in [
            crate::tools::THE_TIME,
            "dance",
            "installed",
            "web_search",
            "fetch_page",
        ] {
            told.push(tool(name.into()));
        }
        let kept = names(&crate::tools::within_bound(told.clone(), &cut).unwrap());
        assert_eq!(kept.len(), lamp_gemini::MAX_TOOLS);
        assert!(!has(&kept, "fetch_page"), "{kept:?}");
        assert!(has(&kept, crate::tools::THE_TIME), "{kept:?}");
        // Each one more of the lamp's: the time, the installable skill's,
        // then the search, and `dance` stays.
        for (more, gone) in [
            (1, crate::tools::THE_TIME),
            (2, "installed"),
            (3, "web_search"),
        ] {
            let mut over = lamp(28 + more);
            over.extend(told[28..].iter().cloned());
            let kept = names(&crate::tools::within_bound(over, &cut).unwrap());
            assert_eq!(kept.len(), lamp_gemini::MAX_TOOLS);
            assert!(!has(&kept, gone), "{gone}: {kept:?}");
            assert!(has(&kept, "dance"), "{kept:?}");
        }
        // One more still: nothing left to cut but `dance`, an error; the
        // session opens with `dance` and the lamp's last left out.
        let mut over = lamp(32);
        over.extend(told[28..].iter().cloned());
        assert!(crate::tools::within_bound(over.clone(), &cut).is_err());
        let core = std::collections::BTreeSet::from(["dance".to_owned()]);
        let kept = names(&crate::tools::fit(over, &cut, &core));
        assert_eq!(kept.len(), lamp_gemini::MAX_TOOLS);
        assert!(has(&kept, "dance"), "{kept:?}");
        assert!(!has(&kept, "tool_31"), "{kept:?}");
        // Within the bound, nothing is cut.
        assert_eq!(crate::tools::within_bound(lamp(5), &cut).unwrap().len(), 5);
    }

    /// #1712, the first builder at 19:43: on lamp-4ace "what time is it?"
    /// called the_time, a round whose first event came 2100 ms later. Every
    /// request is told the time here, so the_time is for another place's
    /// time only, its place needed, and is not given without places.
    #[test]
    fn the_time_is_given_only_for_another_places_time() {
        let settings = |places| crate::tools::Settings {
            clock: true,
            places,
            ..Default::default()
        };
        let the_time = |tools: Vec<lamp_gemini::Tool>| {
            tools
                .into_iter()
                .find(|tool| tool.name == crate::tools::THE_TIME)
        };
        let live = the_time(crate::tools::declared(&settings(true))).unwrap();
        assert!(
            live.description
                .starts_with("Call this whenever you need the time")
        );
        let elsewhere = the_time(super::time_elsewhere(
            crate::tools::declared(&settings(true)),
            true,
        ))
        .unwrap();
        assert_eq!(elsewhere.description, super::TIME_ELSEWHERE);
        assert!(!elsewhere.description.contains("woke"));
        assert_eq!(
            elsewhere.parameters.unwrap()["required"],
            serde_json::json!(["place"])
        );
        assert!(
            the_time(super::time_elsewhere(
                crate::tools::declared(&settings(false)),
                false
            ))
            .is_none()
        );
        // The rest are untouched.
        let others = |tools: Vec<lamp_gemini::Tool>| -> Vec<lamp_gemini::Tool> {
            tools
                .into_iter()
                .filter(|tool| tool.name != crate::tools::THE_TIME)
                .collect()
        };
        assert_eq!(
            others(super::time_elsewhere(
                crate::tools::declared(&settings(false)),
                false
            )),
            others(crate::tools::declared(&settings(false)))
        );
    }
}

/// twelve-P2 (#1693, the first builder at 21:14 on #1930): with the memory's
/// switches on, the second route offers and runs what the Live route does,
/// through its own calls: `remember::start` as its worker starts it,
/// `tools::declared` as it declares, `tools::answer_keeping_news` as it
/// answers a call within a turn's memory scope, and `remember::told_for_session`
/// as it opens a person's session. In a child process: the memory, its
/// switches and the Days client are this process's globals.
#[cfg(test)]
mod memory_switches {
    use crate::memory_scope::{FaceCache, RequestKey, RequestScope};
    use lamp_gemini::ToolCall;
    use serde_json::{Value, json};
    use std::{
        io::{BufRead, BufReader, Write},
        sync::{Arc, Mutex},
        time::Instant,
    };

    const CHILD: &str = "LAMP_SECOND_MEMORY_SWITCHES_TEST_CHILD";
    const NAME: &str =
        "with_the_memorys_switches_the_second_route_forgets_tells_a_thread_and_lists_rules";

    fn call(name: &str) -> ToolCall {
        ToolCall {
            id: format!("c-{name}"),
            name: name.into(),
            args: json!({}),
        }
    }

    /// The memory part, as much of it as these calls ask: every request
    /// line kept, and each answered as `lamp-memory` answers it.
    fn memory(socket: &std::path::Path) -> Arc<Mutex<Vec<Value>>> {
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        let read = Arc::new(Mutex::new(Vec::new()));
        let kept = read.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let kept = kept.clone();
                std::thread::spawn(move || {
                    let mut lines = BufReader::new(stream.try_clone().unwrap()).lines();
                    while let Some(Ok(line)) = lines.next() {
                        let request: Value = serde_json::from_str(&line).unwrap();
                        let answer = match request["op"].as_str() {
                            // Each snapshot carries its requested scope.
                            Some("session_open") => json!({"ok":true,"snapshot":{
                                "session":request["session"],"opened_unix_ms":1,"generation":1,
                                "invalidated":false,"source_ids":[7],
                                "note":json!({"scope":if request["person"].is_string() {
                                    "caller-supplied person; identity not verified"
                                } else { "said in this room; speaker unknown" },
                                    "untrusted":true,"facts":[{"id":7,"kind":"preference","text":"tea"}]})
                                    .to_string()}}),
                            Some("follow_up") => json!({"ok":true,"follow_up":
                                {"id":12,"category":"thread","text":"Sister's wedding is on Saturday"}}),
                            Some("forget_all") => json!({"ok":true,"removed":5}),
                            Some("kept") => json!({"ok":true,"entries":[]}),
                            _ => json!({"ok":true}),
                        };
                        kept.lock().unwrap().push(request);
                        if writeln!(stream, "{answer}").is_err() {
                            return;
                        }
                    }
                });
            }
        });
        read
    }

    #[test]
    fn with_the_memorys_switches_the_second_route_forgets_tells_a_thread_and_lists_rules() {
        if std::env::var(CHILD).ok().as_deref() != Some("1") {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    &format!("second_provider::worker::memory_switches::{NAME}"),
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env(crate::remember::DAYS_ENVIRONMENT, "on")
                .env(crate::remember::FORGET_ALL_ENVIRONMENT, "on")
                .env(crate::remember::THREADS_ENVIRONMENT, "on")
                .env(crate::rules::ENVIRONMENT, "on")
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success() && stdout.contains("1 passed"),
                "{stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let dir = std::env::temp_dir().join(format!("lv-second-memory-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let socket = dir.join("memory.sock");
        let read = memory(&socket);
        // As the second worker starts its memory, declares and answers.
        crate::remember::start(socket.clone(), None);
        let settings = crate::tools::Settings {
            memory: Some(socket.clone()),
            ..crate::tools::Settings::default()
        };
        let declared: Vec<String> = crate::tools::declared(&settings)
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        for name in [
            crate::remember::FORGET_EVERYTHING,
            crate::rules::LIST_RULES,
            crate::rules::KEEP_RULE,
        ] {
            assert!(declared.iter().any(|declared| declared == name), "{name}");
        }
        // Someone known at the lamp, bound to the turn as its engine binds it.
        let known = dir.join("known");
        std::fs::write(&known, "A").unwrap();
        let faces = FaceCache::default();
        faces.replace(
            faces.begin_refresh(),
            crate::known_faces::FaceObservation::read(&known),
        );
        let request = 5;
        let scope = RequestScope::new(
            RequestKey {
                worker: lamp_turn_owner::BootId::new([7; 16]).unwrap(),
                session: 1,
                request,
            },
            faces.snapshot(Instant::now()),
        );
        let lease = scope.lease();
        let person = crate::remember::person_id("A");
        assert_eq!(lease.scope().unwrap().person(), Some(person.as_str()));

        // "Forget everything about me" reaches the memory and is answered.
        let told = crate::tools::answer_keeping_news(
            &settings,
            &call(crate::remember::FORGET_EVERYTHING),
            request,
            true,
            Some(&lease),
        );
        assert_eq!(told["forgotten"], true, "{told}");
        assert_eq!(told["everything"], true, "{told}");
        assert!(
            read.lock()
                .unwrap()
                .iter()
                .any(|line| line["op"] == "forget_all" && line["person"] == person.as_str()),
            "{:?}",
            read.lock().unwrap()
        );

        // A session opened for them is told the one open thread.
        let note = crate::remember::told_for_session(&socket, 2, Some(&person));
        assert!(
            note.contains("\"Sister's wedding is on Saturday\""),
            "{note}"
        );
        assert!(
            read.lock()
                .unwrap()
                .iter()
                .any(|line| line["op"] == "follow_up" && line["person"] == person.as_str())
        );

        // Their rules are listed from the memory too.
        let before = read.lock().unwrap().len();
        let listed = crate::tools::answer_keeping_news(
            &settings,
            &call(crate::rules::LIST_RULES),
            request,
            true,
            Some(&lease),
        );
        assert!(listed.get("error").is_none(), "{listed}");
        assert!(
            read.lock().unwrap()[before..]
                .iter()
                .any(|line| line["op"] == "kept"
                    && line["person"]
                        .as_str()
                        .is_some_and(|scope| scope.starts_with("r-"))),
            "{:?}",
            read.lock().unwrap()
        );

        // #1693 (1), `LAMP_MEMORY_REOPEN_QUIET`: the same person's turn after
        // ten quiet minutes opens a new Days session, told the open thread
        // once; the turns of each run keep their note and ask nothing more.
        let mut sessions = super::Sessions::reopening_after_quiet(true);
        let start = Instant::now();
        let count = |op: &str| {
            read.lock()
                .unwrap()
                .iter()
                .filter(|line| line["op"] == op)
                .count()
        };
        let (opens, follow_ups) = (count("session_open"), count("follow_up"));
        let quiet = crate::thread::REMEMBERED_FOR.as_secs();
        let mut notes = Vec::new();
        for seconds in [0, 60, 60 + quiet, 61 + quiet] {
            let at = start + std::time::Duration::from_secs(seconds);
            let changed = sessions.quiet_long_enough(at);
            notes.push(sessions.frozen(changed, at, |session| {
                crate::remember::fresh_session_note(&socket, session + 10, Some(&person), false)
            }));
            let runs = if seconds < 60 + quiet { 1 } else { 2 };
            assert_eq!(count("follow_up"), follow_ups + runs, "at {seconds} s");
            assert!(count("session_open") > opens, "at {seconds} s");
        }
        let after_first = notes[0].clone();
        assert_eq!(notes[1], after_first, "the same run keeps its note");
        assert!(
            notes[2].contains("\"Sister's wedding is on Saturday\""),
            "{}",
            notes[2]
        );
        assert_eq!(notes[3], notes[2]);
        drop(scope);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// #1693 (2): one observation pass asks this route's DeepSeek model with
/// thinking off, the prompt as the system message and the job as quoted
/// data, and reads its streamed answer as one JSON object.
#[cfg(test)]
mod memory_observer {
    use serde_json::json;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn a_pass_asks_deepseek_without_thinking_and_reads_its_streamed_json() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 8192];
            while !String::from_utf8_lossy(&request).contains("\"stream\":true") {
                let n = socket.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..n]);
            }
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n\
                      data: {\"choices\":[{\"delta\":{\"content\":\"{\\\"observations\\\":[{\\\"text\\\":\\\"Tea, \"}}]}\n\n\
                      data: {\"choices\":[{\"delta\":{\"content\":\"no sugar\\\",\\\"category\\\":\\\"preference\\\",\\\"source_ids\\\":[3],\\\"supersedes\\\":null}]}\"}}]}\n\n\
                      data: [DONE]\n\n",
                )
                .await
                .unwrap();
            String::from_utf8(request).unwrap()
        });
        let chat = crate::second_provider::net::Chat::deepseek_at(
            &format!("http://{address}"),
            "deepseek-flash",
            "secret",
            Arc::new(Vec::new),
            false,
        )
        .unwrap();
        let input = json!({"job":"j7","source_ids":[3],"records":[{"id":3,"heard":"synthetic: tea, no sugar"}]});
        let output = super::ask_observations(&chat, &input).await.unwrap();
        assert_eq!(
            output,
            json!({"observations":[{"text":"Tea, no sugar","category":"preference","source_ids":[3],"supersedes":null}]})
        );
        let request = server.await.unwrap();
        assert!(
            request.contains("\"model\":\"deepseek-flash\""),
            "{request}"
        );
        assert!(
            request.contains("\"thinking\":{\"type\":\"disabled\"}"),
            "{request}"
        );
        assert!(request.contains("untrusted history"), "{request}");
        assert!(request.contains("\\\"job\\\":\\\"j7\\\""), "{request}");
        assert!(!request.contains("\"tools\""), "{request}");
    }

    /// `LAMP_TASKS_NAMED_ASK` (#1722): with it, the first round of a line
    /// that asks one of the staff by name must call `start_agent_task`; a
    /// mention, a round after a call and a chat not forcing are asked as
    /// before, with no `tool_choice`.
    #[test]
    fn a_named_ask_makes_the_first_round_call_start_agent_task_and_nothing_else_changes() {
        use super::super::net::{Chat, Message};
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _staff = crate::agent_tasks::tests::STAFF_HELD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        crate::agent_tasks::hold_staff(&[("office-claude", "claude"), ("office-codex", "codex")]);
        let place = crate::agent_tasks::Place {
            directory: "/run/t".into(),
            target: "office-claude".into(),
            who: true,
            lead_in: true,
            asked: true,
            accounts: false,
            machine_names: false,
            needs: None,
            staff_note: true,
            make_skill: false,
            named_only: false,
            correct: false,
            sign_in: false,
        };
        let tools = crate::agent_tasks::tools(&place);
        let declare: super::super::net::Declare = Arc::new(move || tools.clone());
        let plain = Chat::deepseek("deepseek-chat", "k", declare.clone(), false).unwrap();
        let forcing = Chat::deepseek("deepseek-chat", "k", declare, false).unwrap();
        forcing.forcing(crate::second_provider::worker::named_asks(place));
        let ask = vec![
            Message::system("You are a lamp."),
            Message::user("Hey Lamp, ask Claude Code to write a two-line poem about the rain."),
        ];
        let mention = vec![
            Message::system("You are a lamp."),
            Message::user("Is Codex done?"),
        ];
        let mut after = ask.clone();
        after.push(Message::tool("1", "{\"ok\": true}"));
        let choice =
            |chat: &Chat, messages: &[Message]| chat.wire(messages).2.get("tool_choice").cloned();
        assert_eq!(
            choice(&forcing, &ask),
            Some(serde_json::json!({"type": "function", "function": {"name": "start_agent_task"}}))
        );
        assert_eq!(choice(&forcing, &mention), None);
        assert_eq!(choice(&forcing, &after), None);
        // Off: no tool_choice. A request not forced is the same as off,
        // word for word; a forced one's `who` is pinned (the test below).
        for messages in [&ask, &mention, &after] {
            assert_eq!(choice(&plain, messages), None);
        }
        for messages in [&mention, &after] {
            assert_eq!(plain.wire(messages).2, forcing.wire(messages).2);
        }
        crate::agent_tasks::tests::forget_staff();
    }

    /// `LAMP_SECOND_REMEMBER_ASKED`: on lamp-4ace on 23 Oct at 18:30, the
    /// owner's "Lamp, remember my face, I'm …" called take_a_look, while the
    /// lamp said it knew the face. Forced, it calls remember_my_face in its
    /// first round. An ask with no name, a name with no ask, a name in the
    /// line after an ask (whoever said it), a question whether the lamp knows
    /// them, a refusal, someone else's face, a round after a call and a chat
    /// without the face functions are asked as before, with no `tool_choice`.
    #[test]
    fn an_ask_to_remember_a_face_with_a_name_makes_the_first_round_call_remember_my_face() {
        use super::super::net::{Chat, Message};
        let look = lamp_gemini::Tool {
            name: crate::second_provider::LOOK_CALL.into(),
            description: "Look.".into(),
            parameters: None,
            silent: false,
        };
        let faces: Vec<_> = crate::known_faces::tools_checked(true)
            .into_iter()
            .chain([look.clone()])
            .collect();
        let declare: super::super::net::Declare = Arc::new(move || faces.clone());
        let plain = Chat::deepseek("deepseek-chat", "k", declare.clone(), false).unwrap();
        let forcing = Chat::deepseek("deepseek-chat", "k", declare, false).unwrap();
        forcing.forcing(crate::second_provider::worker::remember_asks());
        let unseeing = Chat::deepseek(
            "deepseek-chat",
            "k",
            Arc::new(move || vec![look.clone()]),
            false,
        )
        .unwrap();
        unseeing.forcing(crate::second_provider::worker::remember_asks());
        let turn = |lines: &[(&'static str, &str)]| -> Vec<Message> {
            std::iter::once(Message::system("You are a lamp."))
                .chain(lines.iter().map(|(role, words)| match *role {
                    "user" => Message::user(*words),
                    _ => Message::assistant(*words),
                }))
                .collect()
        };
        let choice =
            |chat: &Chat, messages: &[Message]| chat.wire(messages).2.get("tool_choice").cloned();
        let remember =
            Some(serde_json::json!({"type": "function", "function": {"name": "remember_my_face"}}));
        let asked = turn(&[("user", "Lamp, remember my face, I'm Kess.")]);
        let told_after = turn(&[
            ("user", "Can you remember my face?"),
            ("assistant", "Sure. What's your name?"),
            ("user", "I'm Kess."),
        ]);
        let spelled_after = turn(&[
            ("user", "Lamp, remember my face, I'm Kess."),
            ("assistant", "Could you spell that for me?"),
            ("user", "K, E, S, S."),
        ]);
        assert_eq!(choice(&forcing, &asked), remember);
        assert_eq!(choice(&plain, &asked), None);
        assert_eq!(choice(&unseeing, &asked), None);
        let mut after_a_call = asked.clone();
        after_a_call.push(Message::tool("1", "{\"remembered\": true}"));
        for messages in [
            turn(&[("user", "Lamp, remember my face.")]),
            turn(&[("user", "I'm Kess.")]),
            turn(&[("user", "K, E, S, S.")]),
            turn(&[("user", "Do you remember me? I'm Kess.")]),
            turn(&[("user", "Don't remember my face, I'm Kess.")]),
            turn(&[("user", "I don't want you to remember my face, I'm Kess.")]),
            turn(&[("user", "I'd rather you didn't remember me, I'm Kess.")]),
            // A name after an ask may be another person's (reviewer-three's
            // P2 on #3298): the model's, as before.
            told_after,
            spelled_after,
            turn(&[("user", "Remember her face, she's Kess.")]),
            turn(&[
                ("user", "What's the weather?"),
                ("assistant", "Sunny."),
                ("user", "I'm Kess."),
            ]),
            after_a_call,
        ] {
            assert_eq!(choice(&forcing, &messages), None, "{messages:?}");
            assert_eq!(plain.wire(&messages).2, forcing.wire(&messages).2);
        }
    }

    /// Both forces at once: each line is forced to its own function, the
    /// named ask's first, as one chat takes only one force.
    #[test]
    fn the_first_force_that_names_a_function_is_the_one_kept() {
        use super::super::net::{Force, Forced, Message};
        let naming = |word: &'static str, name: &'static str| -> Force {
            Arc::new(move |messages: &[Message]| {
                messages
                    .last()
                    .and_then(Message::words)
                    .is_some_and(|words| words.contains(word))
                    .then_some(Forced { name, only: None })
            })
        };
        let both = crate::second_provider::worker::first_force(
            Some(naming("ask", "start_agent_task")),
            Some(naming("face", "remember_my_face")),
        )
        .unwrap();
        let named = |words: &str| both(&[Message::user(words)]).map(|forced| forced.name);
        assert_eq!(named("ask Codex"), Some("start_agent_task"));
        assert_eq!(named("my face"), Some("remember_my_face"));
        assert_eq!(named("ask about my face"), Some("start_agent_task"));
        assert_eq!(named("hello"), None);
        assert!(crate::second_provider::worker::first_force(None, None).is_none());
        let one = crate::second_provider::worker::first_force(
            None,
            Some(naming("face", "remember_my_face")),
        )
        .unwrap();
        assert_eq!(
            one(&[Message::user("my face")]).map(|forced| forced.name),
            Some("remember_my_face")
        );
    }

    /// b0701 on lamp-4ace (#1722, 07:19): "can you ask Claude Code on my
    /// office computer to write a short story …" was forced to
    /// `start_agent_task` and went to Codex: the model filled in `who`.
    /// The forced round is told only the named agent, and must give it;
    /// over the lamp's voice, after a stop, as a conversation going on, the
    /// same. Without forcing, the request is as before, both agents told.
    #[test]
    fn a_named_ask_tells_the_forced_round_only_the_agent_its_words_name() {
        use super::super::net::{Chat, Message};
        let _one = crate::agent_tasks::tests::ONE_AT_A_TIME
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _staff = crate::agent_tasks::tests::STAFF_HELD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        crate::agent_tasks::hold_staff(&[("office-claude", "claude"), ("office-codex", "codex")]);
        let place = crate::agent_tasks::Place {
            directory: "/run/t".into(),
            target: "office-claude".into(),
            who: true,
            lead_in: true,
            asked: true,
            accounts: false,
            machine_names: false,
            needs: None,
            staff_note: true,
            make_skill: false,
            named_only: false,
            correct: false,
            sign_in: false,
        };
        let tools = crate::agent_tasks::tools(&place);
        let declare: super::super::net::Declare = Arc::new(move || tools.clone());
        let plain = Chat::deepseek("deepseek-chat", "k", declare.clone(), false).unwrap();
        let forcing = Chat::deepseek("deepseek-chat", "k", declare, false).unwrap();
        forcing.forcing(crate::second_provider::worker::named_asks(place));
        let line = "Hey Lamp, can you ask Claude Code on my office computer to write a short story, about two hundred words, about a desk lamp that learns to sing?";
        let quiet = vec![Message::system("You are a lamp."), Message::user(line)];
        // Over the lamp's voice, after a stop, let out as the conversation
        // going on: the notes go before the person's words.
        let over = vec![
            Message::system("You are a lamp."),
            Message::user("Tell me a poem about the sea."),
            Message::assistant("The sea rolls in"),
            Message::system(crate::provider_worker::STOPPED_NOTE),
            Message::system(crate::cut_in_told::NOTE),
            Message::system(super::CONVERSATION_ALONE),
            Message::user(line),
        ];
        let start = |body: &serde_json::Value| {
            body["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["function"]["name"] == crate::agent_tasks::START)
                .unwrap()["function"]["parameters"]
                .clone()
        };
        for messages in [&quiet, &over] {
            let body = forcing.wire(messages).2;
            assert_eq!(
                body["tool_choice"],
                serde_json::json!({"type": "function", "function": {"name": "start_agent_task"}})
            );
            let parameters = start(&body);
            assert_eq!(
                parameters["properties"]["who"]["enum"],
                serde_json::json!(["Claude Code"])
            );
            assert_eq!(parameters["required"], serde_json::json!(["task", "who"]));
            // Off: both told, `who` left to the model, no tool_choice.
            let before = plain.wire(messages).2;
            assert!(before.get("tool_choice").is_none());
            let parameters = start(&before);
            assert_eq!(
                parameters["properties"]["who"]["enum"],
                serde_json::json!(["Claude Code", "Codex"])
            );
            assert_eq!(parameters["required"], serde_json::json!(["task"]));
        }
        // The reviewer on #2410: a line that asks two of the staff is still
        // forced, but `who` is told both, so each task can go to its own.
        let two = vec![
            Message::system("You are a lamp."),
            Message::user(
                "Hey Lamp, ask Codex to check the build and ask Claude Code to write a poem.",
            ),
        ];
        let body = forcing.wire(&two).2;
        assert_eq!(
            body["tool_choice"],
            serde_json::json!({"type": "function", "function": {"name": "start_agent_task"}})
        );
        let mut unforced = body.clone();
        unforced.as_object_mut().unwrap().remove("tool_choice");
        assert_eq!(unforced, plain.wire(&two).2);
        assert_eq!(
            start(&body)["properties"]["who"]["enum"],
            serde_json::json!(["Claude Code", "Codex"])
        );
        crate::agent_tasks::tests::forget_staff();
    }
}

#[cfg(test)]
#[path = "worker/kept_name_callback_tests.rs"]
mod kept_name_callback;
