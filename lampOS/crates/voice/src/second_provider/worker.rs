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
    Engine, Net, Told, ToolRunner,
    net::{Message, search::Search},
    output_request,
};
use crate::{
    provider_flow::OutputSender,
    provider_worker::{ProviderInput, ProviderOutput},
    transport::{Channel, WorkerChannels},
    wire::{Control, WorkerEvent},
};
use lamp_ipc::monotonic_us;
use lamp_turn_owner::{BootId, BoundaryGuard, MonoTime, Snapshot};
use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

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
        self.last_control_us = now_us;
        if let Some(owner) = state.owner() {
            if owner.turn() <= self.retired_through || owner.turn() < self.highest_seen {
                return Err(invalid(
                    "second route authority resurrected a retired owner",
                ));
            }
            self.highest_seen = owner.turn();
            self.retired_through = self.retired_through.max(owner.turn() - 1);
        } else {
            self.retired_through = self.retired_through.max(self.highest_seen);
        }
        engine.retire_through(self.retired_through);
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

    fn drain(&mut self, control: &mut Channel, engine: &mut Engine) -> io::Result<Drain> {
        let mut drained = Drain::Deferred;
        for _ in 0..CONTROL_SLICE {
            match control.receive()? {
                Some(Control::Stop) => return Ok(Drain::Stopped),
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

/// Whether a turn is told and kept for its admitted memory scope (#1675):
/// its recall, history, Days note and answer that person's or the room's,
/// never the sticky session owner's. `on` only; otherwise as before.
pub const SCOPED_CONTEXT_ENVIRONMENT: &str = "LAMP_SECOND_SCOPED_CONTEXT";

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

/// DeepSeek's key for its chat and search: the relay's credential
/// (`llm_credential_file`) when they go through the relay
/// (`deepseek_base_url`, #1934), else the voice unit's `deepseek.key`.
pub fn deepseek_key(
    config: &crate::second_config::SecondConfig,
    credentials: Option<&std::ffi::OsStr>,
) -> io::Result<crate::second_config::Key> {
    use crate::second_config::{DEEPSEEK_CREDENTIAL, unit_credential};
    match config.hosted.deepseek_base_url {
        Some(_) => Ok(config.llm_key.clone()),
        None => unit_credential(credentials, DEEPSEEK_CREDENTIAL),
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
/// model's whole answer read as [`crate::remember::observe_output`].
async fn ask_observations(
    chat: &crate::second_provider::net::Chat,
    input: &serde_json::Value,
) -> Option<serde_json::Value> {
    let mut stream = chat
        .send(&[
            Message::system(crate::remember::OBSERVE_PROMPT),
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

/// How the journal names where DeepSeek is asked: never a URL.
fn deepseek_where(hosted: &crate::second_config::Hosted) -> &'static str {
    if hosted.deepseek_base_url.is_some() {
        "through the relay with its credential (deepseek_base_url, LAMP_SECOND_DEEPSEEK_BASE_URL)"
    } else {
        "on DeepSeek's own API"
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
    Ok(net.warming_the_chat(warm).warming_the_speech(warm_speech))
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

/// The lamp's functions, then the search's, within Live's bound of
/// [`lamp_gemini::MAX_TOOLS`]: the search is given room before a skill's
/// function, and the fetch only when room is left.
pub fn with_search(
    mut tools: Vec<lamp_gemini::Tool>,
    search: &[lamp_gemini::Tool],
) -> Vec<lamp_gemini::Tool> {
    let Some((first, rest)) = search.split_first() else {
        return tools;
    };
    tools.truncate(lamp_gemini::MAX_TOOLS - 1);
    tools.push(first.clone());
    for tool in rest {
        if tools.len() < lamp_gemini::MAX_TOOLS {
            tools.push(tool.clone());
        }
    }
    tools
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
/// #2007, 01:17 and 01:18).
pub(crate) const MEMORY_CALLS_NOTE: &str = " When someone asks you to remember something, call remember: saying you will is not keeping it. When they ask you to forget something you remembered for them, call forget. 'Forget it' or 'never mind' about the conversation is not that. For something from an earlier conversation, call recall.";

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

/// The lamp's functions with the look's after them, within Live's bound of
/// [`lamp_gemini::MAX_TOOLS`] (#1712).
pub(crate) fn with_look(mut tools: Vec<lamp_gemini::Tool>) -> Vec<lamp_gemini::Tool> {
    if tools.len() < lamp_gemini::MAX_TOOLS {
        tools.push(lamp_gemini::Tool {
            name: crate::second_provider::LOOK_CALL.into(),
            description: "Look with your eye at what is in front of you: when someone asks what you see, shows you something, or asks how they or something looks. Call it instead of answering; you will then be shown what you see, and answer from that. Never for looking something up on the web.".into(),
            parameters: None,
            silent: false,
        });
    }
    tools
}

/// A search begun from a request's words before the model asks (#1712):
/// the last request's, until its first `web_search` takes it.
#[derive(Default)]
pub(crate) struct Early {
    begun: Mutex<Option<EarlySearch>>,
}

/// One search begun early: its request, when, and its answer to come.
type EarlySearch = (
    u64,
    Instant,
    futures_util::future::Shared<futures_util::future::BoxFuture<'static, serde_json::Value>>,
);

impl Early {
    /// Begin `search` for `request`, in place of any earlier one. Driven on
    /// its own task, so it runs while the model's first round does.
    pub(crate) fn begin(
        &self,
        request: u64,
        search: futures_util::future::BoxFuture<'static, serde_json::Value>,
    ) {
        use futures_util::FutureExt;
        let shared = search.shared();
        tokio::spawn(shared.clone());
        if let Ok(mut begun) = self.begun.lock() {
            *begun = Some((request, Instant::now(), shared));
        }
    }

    /// The search begun for `request`, once: when it began, and its answer.
    pub(crate) fn take(
        &self,
        request: u64,
    ) -> Option<(
        Instant,
        futures_util::future::Shared<futures_util::future::BoxFuture<'static, serde_json::Value>>,
    )> {
        let mut begun = self.begun.lock().ok()?;
        match begun.take() {
            Some((of, at, answer)) if of == request => Some((at, answer)),
            other => {
                *begun = other;
                None
            }
        }
    }
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

/// The weather last fetched, and when.
type Weather = Arc<Mutex<Option<(Instant, String)>>>;

/// What the model is told of the weather fetched at `at`, while it is
/// fresh ([`crate::second_provider::net::weather::FRESH`]).
fn weather_told(fetched: Option<(Instant, String)>, now: Instant) -> Option<String> {
    let (at, said) = fetched?;
    let ago = now.checked_duration_since(at)?;
    (ago <= crate::second_provider::net::weather::FRESH).then(|| {
        format!(
            " The weather where you are, as of {} minutes ago: {said} Use it when someone asks about the weather here; never bring it up unasked.",
            ago.as_secs() / 60
        )
    })
}

/// Ask the weather in `place` every 15 minutes, on the worker's runtime:
/// its coordinates once, then the forecast. A failure is said once until
/// the next success; the weather kept goes stale on its own.
async fn weather_task(place: String, weather: Weather) {
    use crate::second_provider::net::weather;
    let mut every = tokio::time::interval(weather::EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut at = None;
    let mut failing = false;
    loop {
        every.tick().await;
        if at.is_none() {
            at = match weather::locate(&place).await {
                Ok(Some(found)) => Some(found),
                Ok(None) => {
                    if !std::mem::replace(&mut failing, true) {
                        eprintln!(
                            "lamp-voice second: no place named {place} was found for the weather"
                        );
                    }
                    None
                }
                Err(error) => {
                    if !std::mem::replace(&mut failing, true) {
                        eprintln!(
                            "lamp-voice second: the weather's place was not looked up: {error}"
                        );
                    }
                    None
                }
            };
        }
        let Some((latitude, longitude)) = at else {
            continue;
        };
        let began = Instant::now();
        match weather::fetch(latitude, longitude, &place).await {
            Ok(Some(said)) => {
                if std::mem::replace(&mut failing, false)
                    || weather.lock().is_ok_and(|kept| kept.is_none())
                {
                    eprintln!(
                        "lamp-voice second: the weather in {place} came in {} ms",
                        began.elapsed().as_millis()
                    );
                }
                if let Ok(mut kept) = weather.lock() {
                    *kept = Some((Instant::now(), said));
                }
            }
            Ok(None) => {
                if !std::mem::replace(&mut failing, true) {
                    eprintln!("lamp-voice second: the weather came with nothing for now");
                }
            }
            Err(error) => {
                if !std::mem::replace(&mut failing, true) {
                    eprintln!("lamp-voice second: the weather was not fetched: {error}");
                }
            }
        }
    }
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

/// The second route's worker process until Stop. `files` are the Live
/// provider worker's flags; this route uses the thread's file and the
/// tools (the memory among them), and has no use yet for the rest. With
/// `route.only_voice` (`--only-voice`) it is the lamp's only voice.
pub fn serve(
    channels: WorkerChannels,
    boot: BootId,
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
    let declared = tools.clone();
    // The time here is told in every request; the_time is for elsewhere.
    let clock = tools.clock;
    let places = clock && tools.places;
    // With `use_skill` the skills' lines are told after the character, as
    // the Live route tells them (#1668).
    let skills_told = tools.clone();
    let declare: crate::second_provider::net::Declare = Arc::new(move || {
        with_search(
            time_elsewhere(crate::tools::declared(&declared), places),
            &searched,
        )
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
    let spoken_declare: crate::second_provider::net::Declare = if look_by_call {
        let declare = declare.clone();
        Arc::new(move || with_look(declare()))
    } else {
        declare.clone()
    };
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
    let thread = files.thread;
    let file = thread.clone();
    // Each turn is one person's, as a Live session is (#764): whose it is,
    // counted from the eyes' known file as the Live worker counts it, opens
    // the thread for them and asks for their kept things. A Days note is
    // frozen for a session; here a session is the run of turns for one
    // person, so it is asked again only when the person changes.
    let socket = tools.memory.clone();
    let who = tools
        .who
        .as_deref()
        .map(crate::known_faces::known_file_of)
        .filter(|_| socket.is_some());
    let scope_file = who.clone();
    let face_cache = Arc::new(crate::memory_scope::FaceCache::default());
    let scoped_settings = tools.clone();
    let days = crate::remember::days_from_environment();
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
    // What the texts' task is built from, kept before the spoken turns take
    // their own (`texts_task`).
    let texts_from = (character.clone(), tools.clone());
    // Spoken turns only: a text is typed, not heard (#1712).
    let character = spoken_character(&character, look_by_call, tools.memory.is_some());
    let sessions = std::sync::Mutex::new(Sessions::default());
    let place = crate::local_time::place_here();
    if let Some(place) = &place {
        eprintln!("lamp-voice second: the lamp's place is {place} (LAMP_PLACE)");
    }
    // The weather where the lamp stands, asked ahead (#1712): of its place,
    // or else the place its zone names.
    let weather: Weather = Arc::default();
    let weather_place = crate::second_provider::net::weather::on(
        std::env::var(crate::second_provider::net::weather::ENVIRONMENT)
            .ok()
            .as_deref(),
    )
    .then(|| place.clone().or_else(crate::local_time::zone_place_here))
    .flatten();
    let weather_now = weather.clone();
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
    // With a lease, the turn is told for its admitted scope alone: that
    // person's (or the room's) Days note and exchanges, never those of the
    // person the eyes or the sticky session name when its chat begins.
    let told_for = move |lease: Option<&crate::memory_scope::ScopeLease>| {
        let now = Instant::now();
        let Ok(mut sessions) = sessions.lock() else {
            return Told::default();
        };
        let (opened, person) = sessions.open(who.as_deref(), now);
        let (changed, person) = match lease {
            None => (opened, person),
            Some(lease) => {
                let Some(scope) = lease.scope() else {
                    return Told::default();
                };
                let person = scope.person().map(str::to_owned);
                (sessions.told_for(&person), person)
            }
        };
        let memory = match &socket {
            Some(socket) if days => sessions.frozen(changed, |session| {
                crate::remember::told_for_session(socket, session, person.as_deref())
            }),
            Some(socket) => crate::remember::told_for_session(socket, 0, person.as_deref()),
            None => crate::remember::told(),
        };
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
        // The lamp's own place, with `LAMP_PLACE`, beside its time (#1712),
        // and the weather there while it is fresh.
        let time = with_place(
            crate::local_time::told_asked_here(clock, places),
            crate::local_time::place_told(place.as_deref()),
        );
        let time = with_place(
            time,
            weather_told(
                weather_now.lock().ok().and_then(|weather| weather.clone()),
                Instant::now(),
            ),
        );
        let time = crate::agent_tasks::noted(time, tasks.as_ref());
        let facts = facts(session_fact && !going_on, room);
        if steady_first {
            steady_first_message(
                format!("{character}{skills}"),
                &format!("{memory}{}{facts}", time.as_deref().unwrap_or_default()),
                file.as_deref(),
                person.as_deref(),
            )
        } else {
            system_message(
                &character,
                time.as_deref(),
                &(memory + &facts + &skills),
                file.as_deref(),
                person.as_deref(),
            )
        }
    };
    let told_for = Arc::new(told_for);
    let scoped_system = scoped_context_from_environment().then(|| {
        let told_for = told_for.clone();
        Arc::new(move |lease: &crate::memory_scope::ScopeLease| told_for(Some(lease)))
            as crate::second_provider::ScopedSystem
    });
    let system = move || told_for(None);
    let staff = tools
        .tasks
        .clone()
        .filter(|_| files.staff_note)
        .map(staff_noting);
    let has_tasks = tools.tasks.is_some();
    let declines = tools.decline;
    // A plain ask for news, a score, a price, hours or the weather starts
    // its search from the words heard, while the model's first round runs
    // (#1712).
    let early = search.as_ref().map(|_| Arc::new(Early::default()));
    // A held line's `reports_stop` is done, its answer kept held, as on the
    // Live route (`crate::attention::DONE_THOUGH_HELD`), with LAMP_REPORTS=on.
    let though_held = tools.reports_stop;
    let runner = tool_runner(tools, search.clone(), early.clone());
    // The same calls with the admitted request's memory authority (#1794).
    let scoped_runner = scoped_tool_runner(scoped_settings, search.clone(), early.clone());
    let system: crate::second_provider::System = Arc::new(system);
    let mut engine = Engine::new(
        Arc::new(net),
        {
            let system = system.clone();
            move || system()
        },
        runner.clone(),
    )
    .with_memory_scope(boot, face_cache.clone(), scoped_runner, scoped_system)
    .checking_sounds(checked)
    .telling_cut_ins(files.cut_in_told)
    .waiting_with_a_sound(route.wait_cue)
    .saying_when_down(route.down_line)
    .doing_though_held(though_held)
    .announcing(!route.only_voice)
    .saying_reminders(reminders);
    if let Some(staff) = staff {
        engine = engine.noting_staff(staff);
    }
    // Seeing someone speak (`LAMP_SECOND_SPEAKING_FACE=on`, the first
    // builder on #1722, 00:08): the eyes' faces, read every 100 ms on their
    // own thread, say whose a line is when another face spoke it. Only with
    // the memory's known file, the bus and the scoped context, which tells
    // and files each turn for its lease (Second on #2008); the reader stops
    // when this worker returns.
    let _speaking_reader = match (&scope_file, files.bus_root.as_deref()) {
        (Some(known), Some(root))
            if crate::speaking_faces::from_environment() && engine.scopes_context() =>
        {
            let eyes = Arc::new(crate::speaking_faces::SpeakingFaces::default());
            let reader = crate::speaking_faces::read_from(eyes.clone(), root);
            if reader.is_some() {
                engine = engine.seeing_who_speaks(eyes, known.clone());
            }
            reader
        }
        _ => {
            if crate::speaking_faces::from_environment() && !engine.scopes_context() {
                eprintln!(
                    "lamp-voice second: seeing who speaks is not on: it needs LAMP_SECOND_SCOPED_CONTEXT=on"
                );
            }
            None
        }
    };
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
    // The next line's speech to text kept connected (#1712).
    if std::env::var(crate::second_provider::STT_WARM_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on")
    {
        eprintln!(
            "lamp-voice second: speech to text's socket for the next line is kept open (LAMP_SECOND_STT_WARM)"
        );
        engine = engine.keeping_a_listener_ready(true);
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
    // A hand-over waiting for its yes, with the tasks part (#1712).
    if has_tasks {
        engine = engine.noting_a_task_waiting(Arc::new(|request| {
            crate::agent_tasks::pending_note(request, Instant::now())
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
    // As the lamp's only voice this route keeps the room itself (#1712).
    if route.only_voice {
        engine = engine.keeping_room(keeping_room(
            files.attention && files.spoken_to,
            files.cut_ins_not_talk,
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
        {
            let heeding = heeding.clone();
            engine = engine.telling_what_it_did(Arc::new(move |did| {
                if let Ok(mut heeding) = heeding.lock() {
                    heeding.lamp_did(did);
                }
            }));
        }
        if files.attention_near
            && let (Ok(mut heeding), Some(root)) = (heeding.lock(), files.bus_root.as_deref())
        {
            heeding.hear_levels(root);
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
            Some(call_note(crate::attention::called_by_name(words)))
        }));
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
    if let Some(place) = weather_place {
        eprintln!(
            "lamp-voice second: the weather in {place} is asked of Open-Meteo every 15 minutes ({})",
            crate::second_provider::net::weather::ENVIRONMENT
        );
        runtime.spawn(weather_task(place, weather));
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
                let mut interval = tokio::time::interval(Duration::from_millis(200));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    interval.tick().await;
                    let token = face_cache.begin_refresh();
                    let path = path.clone();
                    let observation = tokio::task::spawn_blocking(move || {
                        crate::known_faces::FaceObservation::read(&path)
                    })
                    .await
                    .ok()
                    .flatten();
                    face_cache.replace(token, observation);
                }
            })
        });
        let result = run(channels, boot, engine, thread, probe_targets).await;
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
pub(crate) fn tool_runner(
    tools: crate::tools::Settings,
    search: Option<Arc<Search>>,
    early: Option<Arc<Early>>,
) -> ToolRunner {
    Arc::new(move |request, heard, name, args| {
        Box::pin(run_call(
            tools.clone(),
            search.clone(),
            early.clone(),
            (request, heard),
            name,
            args,
            None,
        ))
    })
}

/// [`tool_runner`] with the admitted request's memory authority (#1794):
/// the same search and journal lines, and the lamp's own call answered
/// within that request's person or room (`crate::tools::answer_scoped`).
pub(crate) fn scoped_tool_runner(
    tools: crate::tools::Settings,
    search: Option<Arc<Search>>,
    early: Option<Arc<Early>>,
) -> crate::second_provider::ScopedToolRunner {
    Arc::new(move |request, heard, name, args, scope| {
        Box::pin(run_call(
            tools.clone(),
            search.clone(),
            early.clone(),
            (request, heard),
            name,
            args,
            Some(scope),
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
/// the look's, and the decline, the skills' one function and setup's walk,
/// which a busy room, the skills' lines and a setup talk need at every turn.
pub(crate) fn given_at_once(name: &str) -> bool {
    name.starts_with("light_")
        || name.starts_with("volume_")
        || crate::tools::is_schedule_tool(name)
        || crate::agent_tasks::is_tool(name)
        || [
            crate::tools::NOT_FOR_ME,
            crate::use_skill::NAME,
            crate::onboarding::STEP,
            crate::onboarding::REDO,
            crate::second_provider::net::search::NAME,
            crate::second_provider::net::search::FETCH,
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
        .partition(|tool| given_at_once(&tool.name));
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

/// One call of the model's, as both runners run it.
async fn run_call(
    settings: crate::tools::Settings,
    search: Option<Arc<Search>>,
    early: Option<Arc<Early>>,
    (request, heard): (u64, bool),
    name: String,
    args: serde_json::Value,
    scope: Option<crate::memory_scope::ScopeLease>,
) -> serde_json::Value {
    if name == MORE {
        eprintln!(
            "lamp-voice second: request {request} called {MORE}: its next round is given every function"
        );
        return serde_json::json!({"ok": true, "result": MORE_GIVEN});
    }
    if let Some(search) = search.filter(|search| search.answers(&name)) {
        // The request's first search takes the one begun from its
        // words, if any (#1712).
        if name == crate::second_provider::net::search::NAME
            && let Some((begun, answer)) = early.and_then(|early| early.take(request))
        {
            eprintln!(
                "lamp-voice second: request {request} web_search answered by the search begun {} ms before the call",
                begun.elapsed().as_millis()
            );
            return answer.await;
        }
        return search.answer(&name, &args).await;
    }
    let call = lamp_gemini::ToolCall {
        id: String::new(),
        name,
        args,
    };
    tokio::task::spawn_blocking(move || {
        use crate::tools::Route;
        let route = crate::tools::route(&settings, &call.name);
        // The Live route's line for a call the Policy held and then
        // let run, which the battery counts (`followups.sh`'s "; run
        // after N ms"): said for each call that route would hold, at
        // 0 ms, since this route holds none (#1712: 0 tool calls were
        // counted on it at 09:00).
        if let Some(line) = run_after(&route, &call.name, request) {
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
        crate::tools::answer_keeping_news(&settings, &call, request, heard, scope.as_ref())
    })
    .await
    .unwrap_or_else(|_| crate::tools::unavailable())
}

/// The journal's line for a call the Live route would hold for the Policy
/// (`policy_holds::holds`), as that route says it once it lets one run,
/// for the battery's counters: at 0 ms, since this route holds none.
pub(crate) fn run_after(route: &crate::tools::Route, name: &str, request: u64) -> Option<String> {
    crate::policy_holds::holds(route, name).then(|| {
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
/// left out, as on the Live worker.
pub(crate) fn keeping_room(
    spoken_to: bool,
    cut_ins_not_talk: bool,
    room_file: Option<&Path>,
    bus_root: Option<&Path>,
) -> Box<dyn FnMut(u64) + Send> {
    let mut kept_boot = None;
    if let Ok(mut room) = crate::room_told::room().lock() {
        room.leave_cut_ins_out(cut_ins_not_talk);
        room.count_only_talk(spoken_to);
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
/// it. Not given: a tap on the head and whether it was said from close
/// (`--addressed`), so a line in a busy room that only those would let
/// through on the Live route is held here. Each turn is judged as a session's first: this
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
    /// with `--attention-near`: the last [`LEVELS_KEPT_US`].
    levels: Option<Levels>,
    /// The request the rule judged last, the session it is judged in, which
    /// a request going on from it keeps, and the words it was judged with,
    /// which are stripped from the front of that request's.
    judged: Option<(u64, u64, String)>,
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
    /// The lamp hums for a misheard cut-in (`LAMP_MISHEARD_HUM`,
    /// [`Self::asking_again`]).
    misheard_hum: bool,
    /// The request last ended as a cut-in over the lamp's answer.
    cut_in: Option<u64>,
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
/// name, its words, a tap, or said close to the lamp is let out as before.
fn busy_bar(why: &str, heard: &str, lamp_asked: bool) -> Option<&'static str> {
    let question = heard.trim_end().ends_with('?');
    if why == crate::attention::GOING_ON {
        (!(question || heard.split_whitespace().count() >= 3 || lamp_asked)).then_some(BUSY_SHORT)
    } else if why == crate::attention::TURNED {
        (!question).then_some(BUSY_TURNED)
    } else {
        None
    }
}

/// How long ago the lamp last said words, counted to their end: the
/// thread's `said_ago` is from when its answer began, `ended_ago` from when
/// the words it sent were played to their end, if it sent any. Nothing
/// when the thread has no words of it, as after it forgot them.
fn to_the_end(said_ago: Option<Duration>, ended_ago: Option<Duration>) -> Option<Duration> {
    said_ago.map(|ago| ended_ago.map_or(ago, |ended| ended.min(ago)))
}

/// `signs` as the turn is judged with them: in a busy room a conversation
/// goes on only [`BUSY_GOING_ON_FOR`] from the end of the lamp's last words
/// (`said_ago`)
/// or [`CALL_CARRIES_FOR`] from the person's call (`called_ago`). When none
/// is going on, the journal says why, without words (#1712, the first
/// builder at 16:16: which of these had held the owner's lines was not
/// told).
fn went_on_told(
    request: u64,
    mut signs: Signs,
    said_ago: Option<Duration>,
    called_ago: Option<Duration>,
    left_alone: Option<usize>,
) -> Signs {
    let busy = signs.room == crate::room_told::Room::Busy;
    if busy {
        signs.going_on &= said_ago.is_some_and(|ago| ago <= BUSY_GOING_ON_FOR)
            || called_ago.is_some_and(|ago| ago <= CALL_CARRIES_FOR);
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

/// How long a call let out carries the person's next lines when the lamp
/// has said no word since (`Heeding::called_at`): the owner's lines on
/// lamp-4ace came every 6 s. The lamp's own words carry them for
/// [`crate::thread::GOING_ON_FOR`], as on Live (the first builder at 17:26:
/// noises after a call the lamp never answered were carried and answered).
pub(crate) const CALL_CARRIES_FOR: Duration = Duration::from_secs(15);

/// Lines held in a row before the rule's session is renewed, as Live renews
/// its own after eight unanswered (`provider_worker`).
pub(crate) const RENEW_AFTER_HELD: u32 = 8;
/// At most this often.
pub(crate) const RENEW_AT_MOST_EVERY: Duration = Duration::from_secs(900);

/// The room microphone's rise over its quiet, dB, by when it was sampled.
type Levels = Arc<Mutex<std::collections::VecDeque<(u64, f32)>>>;

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
    heard: String,
}

/// A line that waits for the Policy's decision: its request, since when,
/// and how the rule saw it.
struct Waiting {
    request: u64,
    since: Instant,
    seen: Seen,
}

/// What the bus and the thread say as a spoken turn is judged.
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

/// Hearings of the name read in one turn's judgement; more wait for the next.
const NAMES_READ: usize = 64;
/// The room microphone's level is sampled this often, as the Live worker does.
const LEVEL_EVERY: Duration = Duration::from_millis(100);
/// A level older than this is let go: longer than a line replayed to this
/// route takes to be heard.
const LEVELS_KEPT_US: u64 = 30_000_000;

impl Heeding {
    pub(crate) fn new(
        strict: bool,
        name_in_passing_ignored: bool,
        bus_root: Option<&Path>,
        thread: Option<PathBuf>,
    ) -> Self {
        let mut attention = crate::attention::Attention::default();
        attention.ignore_name_in_passing(name_in_passing_ignored);
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
            misheard_hum: false,
            cut_in: None,
        }
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
            crate::second_provider::Did::Said(until_us) => {
                self.said_until_us = self.said_until_us.max(Some(until_us));
            }
        }
    }

    /// How long ago the lamp's words sent were played to their end
    /// ([`crate::second_provider::Did::Said`]), zero while they still play.
    fn said_ended_ago(&self, now_us: u64) -> Option<Duration> {
        self.said_until_us
            .map(|until| Duration::from_micros(now_us.saturating_sub(until)))
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
        self.attention.judge_by_day(by_day);
    }

    /// Sample the room microphone's rise over its quiet from now on, on a
    /// thread of its own: the turn's task cannot, since the line is judged
    /// only after it has been heard.
    fn hear_levels(&mut self, root: &Path) {
        let Ok(bus) = lamp_bus::Bus::new(root) else {
            return;
        };
        let levels: Levels = Arc::default();
        // Weak: the thread ends once this `Heeding` is gone (Second on #1028).
        let kept = Arc::downgrade(&levels);
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
        let signs = Signs {
            continues_from,
            ..self.signs(request, said)
        };
        self.note_the_name(request, said, &signs);
        self.judge(request, said, heard, signs)
    }

    /// What the bus and the thread say now of a line said in `began..=ended`.
    fn signs(&mut self, request: u64, (began, ended): (u64, u64)) -> Signs {
        let now = Instant::now();
        let (said_going_on, lamp_asked, said_ago, person) =
            crate::thread::current(self.thread.as_deref(), |thread| {
                (
                    thread.going_on(now),
                    thread.lamp_asked(request),
                    thread.said_last_ago(now),
                    thread.opened_for().map(str::to_owned),
                )
            })
            .unwrap_or_default();
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
        let going_on = said_going_on || called_ago.is_some_and(|ago| ago <= CALL_CARRIES_FOR);
        // From the end of what the lamp said, not from when its answer began,
        // which the thread keeps (#1712, the first builder at 19:21: "And what
        // about tomorrow?" 18 s after the weather line, a long answer, was
        // held in a busy room).
        let said_ago = to_the_end(said_ago, self.said_ended_ago(monotonic_us()));
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
                .and_then(|levels| levels.lock().ok())
                .is_some_and(|levels| {
                    levels.iter().any(|(at, rise)| {
                        (began..=ended).contains(at) && *rise >= crate::attention::NEAR_RISE_DB
                    })
                }),
            continues_from: None,
        };
        // The route's own room as the only voice, as the Live worker reads
        // its own (`attention_now`): with people talking past the lamp since
        // it last spoke, no conversation is going on.
        if self.keeps_room
            && let Ok(mut room) = crate::room_told::room().lock()
        {
            signs.room = room.at(monotonic_us());
            left_alone = Some(room.left_alone_since_spoke());
            signs.going_on &= !room.talked_past_the_lamp();
        }
        let ended = |signs: Signs| went_on_told(request, signs, said_ago, called_ago, left_alone);
        let Some((bus, names)) = &mut self.bus else {
            return ended(signs);
        };
        if !self.keeps_room {
            signs.room = crate::bus_out::room_now(bus);
        }
        signs.turned = crate::bus_out::someone_turned_to_lamp(bus);
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
        signs: Signs,
    ) -> Option<&'static str> {
        let attention = &mut self.attention;
        // Each turn is its own session here. One going on from the request
        // begun just before it, when the rule judged that one last, keeps
        // what the rule knew of it (its name, the words so far, whether it
        // was close), as on the Live route. Anything else, a request never
        // judged among them, is judged fresh (the reviewer on #1226).
        let (session, heard) = match self
            .judged
            .as_ref()
            .filter(|(last, _, _)| signs.continues_from == Some(*last))
        {
            Some((_, session, before)) => {
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
        self.judged = Some((request, session, heard.to_owned()));
        attention.ended(request, ended_us);
        if self.cut_in == Some(request) {
            attention.ended_as_cut_in(request);
        }
        for (at_us, score) in signs.names {
            attention.name_heard_scored(at_us, score);
        }
        attention.heard(heard);
        attention.lamp_asked(signs.lamp_asked);
        let (busy, lamp_asked) = (signs.room == crate::room_told::Room::Busy, signs.lamp_asked);
        attention.set_near(signs.near);
        // The night's bars, as the Live worker reads them before each
        // verdict (`provider_worker.rs`): only with the day's own bars, and
        // otherwise every hour was judged as day (Tests and the first
        // builder on #1712, 20:22 and 20:29).
        if attention.judges_by_day() {
            attention.set_night((self.night)());
        }
        let turned = signs.turned.filter(|_| self.strict);
        let seen = Seen {
            room: signs.room,
            going_on: signs.going_on,
            turned,
            busy,
            lamp_asked,
            heard: heard.to_owned(),
        };
        // #1522 on this route (`LAMP_GOING_ON_ASKS`): what the Policy said of
        // the line is given before it is judged, and in a busy room a line
        // going on waits for it, as on the Live route.
        if let Some(policy) = self.policy.as_mut() {
            policy.take();
            if let Some(says) = policy.says(request) {
                attention.policy_said(request, Some(says));
            } else if attention.waits_for_policy(request, signs.room, signs.going_on, turned) {
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
    pub(crate) fn asking_the_policy(&mut self, root: &Path) {
        match crate::policy_holds::Verdicts::on(root) {
            Ok(verdicts) => {
                eprintln!(
                    "lamp-voice second: in a busy room, a line let out only as a conversation going on waits for the Policy's decision (LAMP_GOING_ON_ASKS)"
                );
                self.policy = Some(verdicts);
                self.attention.ask_policy_when_going_on(true);
            }
            Err(error) => eprintln!(
                "lamp-voice second: the Policy's decisions can't be read ({error}); a line going on is judged without them"
            ),
        }
    }

    /// The verdict on `request`, seen as `seen`, said and kept.
    fn verdict(&mut self, request: u64, seen: &Seen) -> Option<&'static str> {
        let attention = &mut self.attention;
        let (heard, busy, lamp_asked, turned) =
            (seen.heard.as_str(), seen.busy, seen.lamp_asked, seen.turned);
        let mut verdict =
            attention.first_output_busy(request, seen.room, seen.going_on, self.strict, turned);
        let alone = attention.played_on_the_conversation_alone();
        if alone && lone_words(heard) {
            verdict = Some(crate::attention::Verdict::Hold(LONE_WORDS));
        }
        if busy && let Some(crate::attention::Verdict::Play(why)) = verdict {
            verdict = busy_bar(why, heard, lamp_asked)
                .map(crate::attention::Verdict::Hold)
                .or(verdict);
        }
        let played = matches!(verdict, Some(crate::attention::Verdict::Play(_)));
        let called = played && (attention.called_for_itself() || turned == Some(true));
        // The verdict for the room this route keeps (`super::room`), and a
        // call let out, which ends the count of talk past the lamp.
        if let Ok(mut room) = crate::room_told::room().lock() {
            room.judged(request, verdict, false);
            if called {
                room.called(request);
            }
        }
        self.alone = (alone && matches!(verdict, Some(crate::attention::Verdict::Play(_))))
            .then_some(request);
        self.busy = busy.then_some(request);
        // Its words or the name heard called the lamp, not a face alone.
        self.by_name = (played && attention.called_for_itself()).then_some(request);
        // A call let out: the conversation goes on from it.
        if called {
            self.called_at = Some(Instant::now());
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
        match verdict {
            Some(crate::attention::Verdict::Hold(why)) => Some(why),
            // Said, as each hold is by the route (#829, 17:21).
            Some(crate::attention::Verdict::Play(why)) => {
                let alone = if self.alone.is_some() {
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
    pub(crate) fn holds_call(&self, request: u64, name: &str) -> bool {
        self.alone == Some(request) && crate::policy_holds::SCHEDULE_HELD.contains(&name)
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
    /// conversation going on ([`CONVERSATION_ALONE`]).
    pub(crate) fn conversation_note_for(&self, request: u64) -> Option<&'static str> {
        (self.alone == Some(request)).then_some(CONVERSATION_ALONE)
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

/// Whose the second route's turns are, and the Days note of that person's
/// run of turns.
#[derive(Default)]
struct Sessions {
    presence: crate::known_faces::Presence,
    /// Runs of turns opened so far: the Days session number.
    opened: u64,
    note: Option<String>,
    /// The admitted scope the Days note was last asked for.
    scoped: Option<Option<String>>,
}
impl Sessions {
    /// Open the thread for whoever is at the lamp now ([`Thread::opening`]):
    /// whether that is someone other than the last turn's, and whom.
    ///
    /// [`Thread::opening`]: crate::thread::Thread::opening
    fn open(&mut self, who: Option<&Path>, now: Instant) -> (bool, Option<String>) {
        let known = who
            .and_then(crate::known_faces::known_now)
            .map(|name| crate::remember::person_id(&name));
        let whose = who.and(self.presence.seen(known.clone(), now).map(str::to_owned));
        let Ok(mut thread) = crate::thread::THREAD.lock() else {
            return (false, None);
        };
        if who.is_some() {
            thread.at_lamp(whose);
            thread.seen_now(known);
        }
        let changed = thread.opening(now);
        (changed, thread.opened_for().map(str::to_owned))
    }

    /// Whether `person`'s admitted scope differs from the last turn's.
    fn told_for(&mut self, person: &Option<String>) -> bool {
        let changed = self.scoped.as_ref() != Some(person);
        self.scoped = Some(person.clone());
        changed
    }

    /// The Days note for this run of turns, asked anew when the person changed.
    fn frozen(&mut self, changed: bool, ask: impl FnOnce(u64) -> String) -> String {
        if changed || self.note.is_none() {
            self.opened += 1;
            self.note = Some(ask(self.opened));
        }
        self.note.clone().unwrap_or_default()
    }
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
/// and `whose` exchanges as messages, both under the thread's one lock.
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
        .and_then(|place| crate::task_news::ForTexts::for_tasks(&place.directory))
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
    keep_scoped(event, file, None)
}

fn keep_scoped(
    event: &ProviderOutput,
    file: Option<&Path>,
    lease: Option<&crate::memory_scope::ScopeLease>,
) {
    if lease.is_some_and(|lease| !lease.is_current()) {
        return;
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
            crate::remember::finished(*request);
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
    'tick: loop {
        // The turn tasks run while this waits.
        ticker.tick().await;
        engine.tick_room(monotonic_us());
        match gate.drain(&mut channels.control, &mut engine)? {
            Drain::Stopped => return Ok(()),
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
                Drain::Stopped => return Ok(()),
                Drain::Deferred => continue 'tick,
                Drain::Empty => {}
            }
            let Some(command) = retained.take() else {
                break;
            };
            if gate.admits(monotonic_us(), &command)? {
                // A written request is kept for the thread as the note it
                // is, with the answer to it (`Thread::written`).
                if let ProviderInput::Written { request, text, .. } = &command
                    && let Ok(mut thread) = crate::thread::THREAD.lock()
                {
                    // A written request is admitted as the room's.
                    if engine.scopes_context() {
                        thread.written_for(*request, text, None);
                    } else {
                        thread.written(*request, text);
                    }
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
            pending = engine.try_output();
            if let Some(event) = &pending {
                let lease = engine
                    .scopes_context()
                    .then(|| engine.memory_lease())
                    .flatten();
                keep_scoped(event, thread.as_deref(), lease.as_ref());
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
        if let Some(event) = pending.take() {
            gate.may_send(monotonic_us(), &event)?;
            if !gate.flow.try_send(|| channels.data.send(&event))? {
                pending = Some(event);
            }
        }
    }
}

#[cfg(test)]
mod tests {
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

    /// #1712, the first builder at 18:57: in a busy room a conversation goes
    /// on 15 s from the lamp's last words; in a quiet one, Live's 45 s.
    /// #1712, the first builder at 19:21: "And what about tomorrow?" came
    /// 18 s after the weather line, after a long answer, and was held in a
    /// busy room. The 15 s count from the end of the lamp's words.
    #[test]
    fn a_busy_rooms_fifteen_seconds_count_from_the_end_of_the_lamps_words() {
        let s = Duration::from_secs;
        let mut heeding = super::Heeding::new(false, false, None, None);
        assert_eq!(heeding.said_ended_ago(100_000_000), None);
        heeding.lamp_did(crate::second_provider::Did::Said(105_000_000));
        // Still playing, then over.
        assert_eq!(heeding.said_ended_ago(100_000_000), Some(Duration::ZERO));
        assert_eq!(heeding.said_ended_ago(114_000_000), Some(s(9)));
        // An earlier end told late moves nothing back.
        heeding.lamp_did(crate::second_provider::Did::Said(101_000_000));
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
        assert!(went_on_told(5, busy(), said_ago, None, None).going_on);
        assert!(!went_on_told(5, busy(), Some(s(18)), None, None).going_on);
        // No words in the thread: none, whatever was played.
        assert_eq!(to_the_end(None, Some(s(1))), None);
        assert_eq!(to_the_end(Some(s(30)), None), Some(s(30)));
    }

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
            super::went_on_told(1, signs(room), Some(s(said)), called.map(s), None).going_on
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

    /// #1712, the first builder at 17:26: a call the lamp never answered
    /// carries the person's next lines for 15 s, not the lamp's own 45 s: a
    /// three-word line later, in a trusted session, is held.
    #[test]
    fn a_call_unanswered_carries_the_next_lines_for_fifteen_seconds() {
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

    /// #1712: a line that called the lamp, let out, carries the person's
    /// next line without the name as the conversation going on, though the
    /// lamp has said no word yet; two lines left alone since still end it.
    #[test]
    fn a_call_let_out_carries_the_next_line_though_the_lamp_said_nothing() {
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
    }

    /// Its call is answered by the route itself, with nothing run.
    #[tokio::test]
    async fn the_call_for_the_rest_is_answered_by_the_route() {
        let runner = tool_runner(crate::tools::Settings::default(), None, None);
        assert_eq!(
            runner(1, true, MORE.into(), serde_json::json!({})).await,
            serde_json::json!({"ok": true, "result": MORE_GIVEN})
        );
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
        let mut asked = Vec::new();
        let mut note = |changed: bool| {
            sessions.frozen(changed, |session| {
                asked.push(session);
                format!("note {session}")
            })
        };
        assert_eq!(note(false), "note 1", "the first turn asks");
        assert_eq!(note(false), "note 1", "the same session keeps it");
        assert_eq!(note(true), "note 2", "a new session asks again");
        assert_eq!(note(false), "note 2");
        assert_eq!(asked, [1, 2]);
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
            let mut tick = super::keeping_room(false, false, Some(&first), None);
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
            let _tick = super::keeping_room(false, false, Some(&second), None);
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
        heeding.hear_levels(&root);
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

    /// A finished turn goes to the thread, its file and the journal, and
    /// the next turn's system message carries the character, the memory's
    /// note and that thread. In a child process: the thread and the
    /// memory are this process's globals.
    #[test]
    fn a_turn_is_kept_and_told_to_the_next() {
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
                keep_scoped(&event, None, Some(&lease));
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

    /// The reviewer on #1713: a text's answer is never told what was said at
    /// the lamp. With a spoken exchange in the thread, the spoken turn's
    /// system message has it, and a text's has neither it nor any history.
    #[test]
    fn a_texts_system_message_never_has_the_rooms_spoken_conversation() {
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
        // As the worker gives a text's answer its system message.
        let (texts, _) = texts_ends("You are a lamp.".into(), crate::tools::Settings::default());
        let told = texts();
        assert!(!told.text.contains("Under the blue mat."), "{}", told.text);
        assert!(!told.text.contains("spare key"), "{}", told.text);
        assert!(told.text.starts_with("You are a lamp."), "{}", told.text);
        assert!(told.history.is_empty());
        // Told the time as it is asked (#1712) with `LAMP_CLOCK`, and as
        // before without it (the first builder on #1921), where this
        // machine has one.
        if crate::local_time::offset_here().is_some() {
            assert!(told.text.contains("give it roughly"), "{}", told.text);
            assert!(!told.text.contains("as you were asked"), "{}", told.text);
            let (texts, _) = texts_ends(
                "You are a lamp.".into(),
                crate::tools::Settings {
                    clock: true,
                    ..Default::default()
                },
            );
            let told = texts();
            assert!(
                told.text.contains("Your clock read this as you were asked"),
                "{}",
                told.text
            );
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
        let root = PathBuf::from(format!("/tmp/lvw-{}", std::process::id()));
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

    /// #1712: as the lamp's only voice, the rule judges in the room this
    /// route keeps (`room_told::room`), not in an unknown one; without the
    /// switch, and with no bus, the room stays unknown.
    #[test]
    fn as_the_only_voice_the_rule_judges_in_the_room_the_route_keeps() {
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

    /// #1712: a call the Live route would hold for the Policy is said as
    /// that route says one it lets run ("; run after N ms"), which the
    /// battery counts; a lookup, which no route holds, is not.
    #[test]
    fn a_call_the_live_route_would_hold_is_said_as_run_for_the_battery() {
        let exec = crate::tools::Route::Executive("/run/lampos-actions".into());
        let line = super::run_after(&exec, "light_on", 7).unwrap();
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
        assert!(super::run_after(&crate::tools::Route::Clock, "the_time", 7).is_none());
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
        let busy_after_three = |cut_ins_not_talk: bool| {
            std::thread::spawn(move || {
                let _tick = super::keeping_room(false, cut_ins_not_talk, None, None);
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
            "'Forget it' or 'never mind' about the conversation is not that",
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
            Box::pin(async { serde_json::json!({"results": ["early"]}) }),
        );
        assert!(early.take(8).is_none(), "another request's");
        let (_, answer) = early.take(7).unwrap();
        assert_eq!(answer.await, serde_json::json!({"results": ["early"]}));
        assert!(early.take(7).is_none(), "taken once");
    }

    /// #1712: the request's first `web_search` is answered by the search
    /// begun early, whatever its query; a later one goes to the service
    /// (here a closed port, so it fails).
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
        );
        early.begin(
            5,
            Box::pin(async { serde_json::json!({"results": ["early"]}) }),
        );
        let args = serde_json::json!({"query": "who won the race"});
        assert_eq!(
            runner(5, true, "web_search".into(), args.clone()).await,
            serde_json::json!({"results": ["early"]})
        );
        let again = runner(5, true, "web_search".into(), args).await;
        assert!(again.get("error").is_some(), "{again}");
    }

    /// #1712: the look's function follows the lamp's, while there is room.
    #[test]
    fn the_look_is_offered_after_the_lamps_functions_while_there_is_room() {
        let tool = |name: &str| lamp_gemini::Tool {
            name: name.into(),
            description: String::new(),
            parameters: None,
            silent: false,
        };
        let looked = super::with_look(vec![tool("light_on")]);
        let names: Vec<&str> = looked.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(names, ["light_on", crate::second_provider::LOOK_CALL]);
        let full: Vec<_> = (0..lamp_gemini::MAX_TOOLS)
            .map(|n| tool(&format!("f{n}")))
            .collect();
        assert_eq!(super::with_look(full).len(), lamp_gemini::MAX_TOOLS);
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

    /// #1712: the weather is told while fresh, never once stale.
    #[test]
    fn the_weather_is_told_while_fresh() {
        let now = Instant::now();
        let said = "In Hanoi now: 31°C, partly cloudy.".to_owned();
        let fresh = super::weather_told(
            Some((now, said.clone())),
            now + Duration::from_secs(20 * 60),
        )
        .unwrap();
        assert!(
            fresh.starts_with(" The weather where you are, as of 20 minutes ago: In Hanoi now"),
            "{fresh}"
        );
        assert!(fresh.contains("never bring it up unasked"));
        let stale = now + crate::second_provider::net::weather::FRESH + Duration::from_secs(1);
        assert_eq!(super::weather_told(Some((now, said)), stale), None);
        assert_eq!(super::weather_told(None, now), None);
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
        let mut tick = super::keeping_room(false, false, None, Some(&root));
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
        // Without a base URL, DeepSeek's own API still needs deepseek.key.
        let mut own = config;
        own.hosted.deepseek_base_url = None;
        let error = super::deepseek_key(&own, None).err().unwrap().to_string();
        assert!(error.contains("CREDENTIALS_DIRECTORY"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The search's functions go beside the lamp's within Live's bound of
    /// 32: the search before a skill's last function, the fetch only with
    /// room left.
    #[test]
    fn the_search_is_given_room_before_a_skill_and_the_fetch_only_with_room() {
        let tool = |name: String| lamp_gemini::Tool {
            name,
            description: "A function.".into(),
            parameters: None,
            silent: false,
        };
        let lamp = |count: usize| -> Vec<lamp_gemini::Tool> {
            (0..count).map(|i| tool(format!("tool_{i}"))).collect()
        };
        let search = [tool("web_search".into()), tool("fetch_page".into())];
        let names = |tools: Vec<lamp_gemini::Tool>| -> Vec<String> {
            tools.into_iter().map(|tool| tool.name).collect()
        };
        let roomy = names(super::with_search(lamp(10), &search));
        assert_eq!(roomy.len(), 12);
        assert_eq!(roomy[10..], ["web_search", "fetch_page"]);
        let full = names(super::with_search(lamp(32), &search));
        assert_eq!(full.len(), 32);
        assert_eq!(full[31], "web_search");
        assert!(!full.contains(&"tool_31".to_owned()));
        assert!(!full.contains(&"fetch_page".to_owned()));
        assert_eq!(names(super::with_search(lamp(5), &[])).len(), 5);
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
                            // A person's frozen note (`remember::PERSON_SCOPE`).
                            Some("session_open") => json!({"ok":true,"snapshot":{
                                "session":request["session"],"opened_unix_ms":1,"generation":1,
                                "invalidated":false,"source_ids":[7],
                                "note":json!({"scope":"caller-supplied person; identity not verified",
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
}
