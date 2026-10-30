//! The seam between the engine and the network: the three services as the
//! engine uses them, with [`Net`] wrapping the real clients.
use super::net::{
    self, Chat, ChatEvent, GeminiTts, Message, SttStream, Tts,
    routes::{self, Routes},
};
use crate::second_config::{SecondConfig, Stt};
use futures_util::{
    StreamExt, TryStreamExt,
    future::BoxFuture,
    stream::{self, BoxStream},
};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Speech-to-text, a chat model and text-to-speech.
pub trait Services: Send + Sync + 'static {
    /// Open speech-to-text for one utterance.
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>>;
    /// The answer to `messages`, as it is written; a failure to connect is
    /// the stream's first item.
    fn chat(&self, messages: Vec<Message>) -> BoxStream<'static, net::Result<ChatEvent>>;
    /// [`Self::chat`], told only the lamp's functions named in `only`, for
    /// a held line's calls done though its answer is held
    /// ([`crate::attention::DONE_THOUGH_HELD`]); never narrowed further.
    /// None, from services that cannot.
    fn chat_only(
        &self,
        messages: Vec<Message>,
        only: &'static [&'static str],
    ) -> BoxStream<'static, net::Result<ChatEvent>> {
        let _ = (messages, only);
        stream::iter([Err(net::Error::Protocol(
            "no chat told only some functions",
        ))])
        .boxed()
    }
    /// [`Self::chat`], never declaring the functions named in `without`: a
    /// line held only by a known face's gap is answered from the
    /// conversation as it stood, without memory's reads (#2265's follow-up).
    /// Services that can't leave them out chat as [`Self::chat`].
    fn chat_without(
        &self,
        messages: Vec<Message>,
        without: &'static [&'static str],
    ) -> BoxStream<'static, net::Result<ChatEvent>> {
        let _ = without;
        self.chat(messages)
    }
    /// The same chat asked once more on a connection of its own
    /// (`LAMP_SECOND_CHAT_AGAIN`, #2602): told only `only`, when given, as
    /// [`Self::chat_only`], else without `without`, as
    /// [`Self::chat_without`]. Services that can't open a fresh connection
    /// ask as those do.
    fn chat_again(
        &self,
        messages: Vec<Message>,
        only: Option<&'static [&'static str]>,
        without: &'static [&'static str],
    ) -> BoxStream<'static, net::Result<ChatEvent>> {
        match only {
            Some(only) => self.chat_only(messages, only),
            None if !without.is_empty() => self.chat_without(messages, without),
            None => self.chat(messages),
        }
    }
    /// Whether a chat on `messages`, told without the functions named in
    /// `without`, declares the function `name` ([`Self::chat_without`]), for
    /// a turn's journal line (`LAMP_MEMORY_REPLACES`, #2510). None, from
    /// services that cannot say.
    fn declares(&self, messages: &[Message], without: &[&str], name: &str) -> Option<bool> {
        let _ = (messages, without, name);
        None
    }
    /// `text` spoken, as 24 kHz mono packets; dropping it stops the download.
    fn speak(&self, text: String) -> BoxStream<'static, net::Result<Vec<i16>>>;
    /// As [`Services::speak`], a piece of `turn`'s answer, which the voice
    /// may journal ([`net::Tts::speak_for`]).
    fn speak_for(
        &self,
        text: String,
        turn: net::Turn,
    ) -> BoxStream<'static, net::Result<Vec<i16>>> {
        let _ = turn;
        self.speak(text)
    }
    /// `request`'s turn is over: the voice's line on its pieces, if it
    /// keeps one ([`net::Tts::told`]).
    fn spoken(&self, request: u64) {
        let _ = request;
    }
    /// Whether the chat model takes a look's picture; one that does not is
    /// told it saw nothing ([`crate::speak_up::NO_LOOK_NOTE`]).
    fn sees(&self) -> bool {
        true
    }
    /// Make ready to speak as a line begins: nothing, unless a voice keeps
    /// a connection to open ahead ([`GeminiTts::warm`], [`Tts::warm`]).
    fn warm(&self) -> BoxFuture<'static, ()> {
        Box::pin(async {})
    }
    /// `request`'s answer begins: nothing, unless its voice opens a speech
    /// socket for it ahead of its first piece ([`Tts::open_ahead`],
    /// `LAMP_SECOND_SPEECH_SOCKET=on`).
    fn open_speech_ahead(&self, request: u64) {
        let _ = request;
    }
    /// A turn begins: what its chat and speech use is settled now, and kept
    /// to its end (`LAMP_RELAY_ROUTES`, #2882); nothing for services that
    /// route nothing.
    fn turn_begins(&self) {}
    /// A service host answers again after the link was down: nothing,
    /// unless the relay's routes are asked again then ([`routes::Kept::ask_again`]).
    fn link_back(&self) {}
}

/// One open utterance.
pub trait Listener: Send {
    /// Queue 16 kHz mono samples; nothing waits.
    fn send(&self, samples: &[i16]) -> net::Result<()>;
    /// The final words; empty when nothing was heard.
    /// `request` names the turn in the journal.
    fn finish(self: Box<Self>, request: u64) -> BoxFuture<'static, net::Result<String>>;
    /// The words it has before they are final, for one taker; none from a
    /// listener that never has any.
    fn early(&mut self) -> Option<mpsc::UnboundedReceiver<net::Early>> {
        None
    }
    /// Each result's words as they come, interim and final, for one taker
    /// (`LAMP_SECOND_UNFINISHED_WAIT`); none from a listener that never has
    /// any.
    fn said(&mut self) -> Option<mpsc::UnboundedReceiver<String>> {
        None
    }
    /// Whether it can still take a line: one kept ready whose socket has
    /// closed since cannot ([`crate::second_provider::Engine::keeping_a_listener_ready`]).
    fn open(&self) -> bool {
        true
    }
    /// Ask for the words so far and keep the line open, at a pause
    /// (`LAMP_SECOND_EARLY_FINALIZE`); nothing for a listener that can't.
    fn flush(&self) -> net::Result<()> {
        Ok(())
    }
}

impl Listener for SttStream {
    fn open(&self) -> bool {
        SttStream::still_open(self)
    }
    fn send(&self, samples: &[i16]) -> net::Result<()> {
        SttStream::send(self, samples)
    }
    fn early(&mut self) -> Option<mpsc::UnboundedReceiver<net::Early>> {
        self.take_early()
    }
    fn said(&mut self) -> Option<mpsc::UnboundedReceiver<String>> {
        self.take_words()
    }
    fn flush(&self) -> net::Result<()> {
        SttStream::paused(self)
    }
    fn finish(self: Box<Self>, request: u64) -> BoxFuture<'static, net::Result<String>> {
        Box::pin(SttStream::finish(*self, request))
    }
}

/// The real services, from the route's configuration.
pub struct Net {
    config: Arc<SecondConfig>,
    chat: Arc<Chat>,
    tts: Arc<Tts>,
    /// `LAMP_SECOND_SPEECH=gemini`: Gemini's voice first, the configured
    /// one when it fails.
    gemini_tts: Option<Arc<GeminiTts>>,
    declare: net::Declare,
    /// `LAMP_SECOND_CHAT_WARM=on`: the chat's connection is opened as a line
    /// begins ([`Chat::warm`]).
    warm_chat: bool,
    /// `LAMP_SECOND_SPEECH_WARM=on`: the configured voice's connection is
    /// opened as a line begins ([`Tts::warm`]).
    warm_speech: bool,
    /// `LAMP_SECOND_SPEECH_SOCKET=on`: a speech socket is opened as each
    /// answer begins, for its first piece ([`Tts::open_ahead`]).
    speech_socket: bool,
    /// With the person's own DeepSeek key (`LAMP_OWN_KEYS`, #2094): the
    /// relay's chat a request goes to when their endpoint refuses it (401,
    /// 403) before any answer, and what is told of each refusal.
    refused_to: Option<RefusedTo>,
    /// What [`Self::forcing`] and [`Self::narrowing`] gave the chat, for a
    /// near chat made later (#2882).
    force: Option<net::Force>,
    narrow: Option<net::Narrow>,
    /// `LAMP_RELAY_ROUTES=on`: the relay's near addresses, for the services
    /// [`Routable`] lets them take (#2882).
    near: Option<Arc<Near>>,
    /// The Gemini the route calls, for a near one made later (#2882): its
    /// key, and its chat's model or its voice's model and voice, as
    /// [`Self::chatting_with_gemini`] and [`Self::speaking_with_gemini`]
    /// were given them.
    gemini: GeminiParts,
}

/// [`Net::gemini`].
#[derive(Default)]
struct GeminiParts {
    key: Option<String>,
    chat_model: Option<String>,
    speech: Option<(String, String)>,
}

/// Which services the relay's near addresses may take (#2882): the voice
/// unit's own address for one, or a configuration that does not go through
/// the relay for it, wins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Routable {
    pub chat: bool,
    pub speech: bool,
    pub listening: bool,
    /// The second route's own Gemini, its chat and its voice, never Live's.
    pub gemini: bool,
}

/// The relay's routes as the services use them: those last read, the
/// turn's (chat and speech keep theirs from [`Services::turn_begins`] to the
/// turn's end), and a client for each near address, made when first used
/// and kept while the address stays.
struct Near {
    routable: Routable,
    kept: Arc<routes::Kept>,
    /// Where the relay is asked ([`routes::url`]).
    url: String,
    /// The routes [`Services::turn_begins`] settled last, and how many
    /// times it has: a client taken for a turn carries that count, so a
    /// failure of an earlier turn's (a cue or down line still rendering
    /// after the next turn began) never clears a later one's (reviewer-two's
    /// P2 on #2919).
    turn: Mutex<(u64, Routes)>,
    chat: Mutex<Option<(String, Arc<Chat>)>>,
    tts: Mutex<Option<(String, Arc<Tts>)>>,
    hearing: Mutex<Option<(String, Arc<SecondConfig>)>>,
    gemini_chat: Mutex<Option<(String, Arc<Chat>)>>,
    gemini_tts: Mutex<Option<(String, Arc<GeminiTts>)>>,
}

/// A service the relay may route.
#[derive(Clone, Copy)]
enum Service {
    Chat,
    Speech,
    Gemini,
}

/// A near address taken for the turn [`Near::turn`] settled.
struct Taken {
    near: Arc<Near>,
    generation: u64,
    url: String,
}

impl Taken {
    /// `service` failed at its near address (`why`, [`routes::fallback_why`]):
    /// its own for the rest of the turn it was taken for, if that turn is
    /// still the latest; a later turn's routes stay as they are. The
    /// address is held until the routes are next read, so later turns don't
    /// try it again (El's P2 on #2919).
    fn fell(&self, service: Service, why: &str) {
        if let Ok(mut turn) = self.near.turn.lock()
            && turn.0 == self.generation
        {
            match service {
                Service::Chat => turn.1.chat = None,
                Service::Speech => turn.1.speech = None,
                Service::Gemini => turn.1.gemini = None,
            }
        }
        let name = service.name();
        self.near.kept.hold(name, &self.url);
        routes::fell_back(name, why);
    }
}

impl Service {
    /// The service's name in the routes and the journal.
    fn name(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Speech => "speech",
            Self::Gemini => "gemini",
        }
    }
}

impl Near {
    /// The turn's address for `service`, if near, taken for that turn.
    fn turn(self: &Arc<Self>, service: Service) -> Option<(String, Taken)> {
        let turn = self.turn.lock().ok()?;
        let url = match service {
            Service::Chat => turn.1.chat.clone(),
            Service::Speech => turn.1.speech.clone(),
            Service::Gemini => turn.1.gemini.clone(),
        }?;
        Some((
            url.clone(),
            Taken {
                near: self.clone(),
                generation: turn.0,
                url,
            },
        ))
    }

    /// The client kept in `slot` for `url`, or one made by `make` and kept;
    /// `None` when it can't be made.
    fn client<T>(
        slot: &Mutex<Option<(String, Arc<T>)>>,
        url: &str,
        make: impl FnOnce() -> Option<T>,
    ) -> Option<Arc<T>> {
        let mut slot = slot.lock().ok()?;
        if let Some((at, client)) = slot.as_ref()
            && at == url
        {
            return Some(client.clone());
        }
        let client = Arc::new(make()?);
        *slot = Some((url.to_owned(), client.clone()));
        Some(client)
    }
}

/// Where a request the person's own key was refused for goes instead.
#[derive(Clone)]
pub struct RefusedTo {
    pub relay: Arc<Chat>,
    /// Told each refusal's status: the journal's line, the health warning.
    pub told: Arc<dyn Fn(u16) + Send + Sync>,
}

/// Every send of an ask again goes through here, near, own key and relay
/// alike (`LAMP_SECOND_CHAT_AGAIN`, #3107): the one place a per-dispatch
/// permission check (Fourth's #3064) stops a request whose scope has lapsed
/// since the turn's own check, before anything goes (reviewer-two's P2 on
/// #3107). Until that lands it only sends.
async fn send_again(
    chat: &Chat,
    messages: &[Message],
    only: Option<&[&str]>,
    without: &[&str],
) -> net::Result<super::net::ChatStream> {
    chat.send_fresh(messages, only, without).await
}

/// Whether a chat's failure is its key refused: the request goes to the
/// relay then, and for nothing else.
pub fn key_refused(error: &net::Error) -> Option<u16> {
    match error {
        net::Error::Status(status @ (401 | 403)) => Some(*status),
        _ => None,
    }
}

/// Where the worker reads whether the chat's connection is opened as a line
/// begins: `on`, and nothing else.
pub const CHAT_WARM_ENVIRONMENT: &str = "LAMP_SECOND_CHAT_WARM";
/// Where the worker reads whether the relay's near addresses go over the
/// second route's chat, speech and listening (#2882): `on`, and nothing else.
pub const RELAY_ROUTES_ENVIRONMENT: &str = "LAMP_RELAY_ROUTES";
/// Where the worker reads whether the configured voice's connection is
/// opened as a line begins: `on`, and nothing else.
pub const SPEECH_WARM_ENVIRONMENT: &str = "LAMP_SECOND_SPEECH_WARM";
/// Where the worker reads whether a speech socket is opened as each answer
/// begins (#2135's row): `on`, and nothing else.
pub const SPEECH_SOCKET_ENVIRONMENT: &str = "LAMP_SECOND_SPEECH_SOCKET";
/// Where the voice reads the speech socket's own model (#2135, the backend at
/// 17:5x on 21 Oct): a model ElevenLabs' `stream-input` serves, or
/// `eleven_v4_turbo`, which takes its dialogue socket (#2602); absent,
/// `eleven_flash_v2_5`. Every piece of an answer whose socket opened is
/// said in it, those over HTTP too.
pub const SPEECH_SOCKET_MODEL_ENVIRONMENT: &str = "LAMP_SECOND_SPEECH_SOCKET_MODEL";

/// Gemini's API as the route calls it: where, with which key and model.
pub struct GeminiCall<'a> {
    pub base_url: &'a str,
    pub key: &'a str,
    pub model: &'a str,
}

impl Net {
    /// `declare` gives the lamp's tools as they stand, at each chat request.
    pub fn new(config: SecondConfig, declare: net::Declare) -> net::Result<Self> {
        Ok(Self {
            chat: Arc::new(Chat::new(&config, declare.clone())?),
            tts: Arc::new(Tts::new(&config)?),
            gemini_tts: None,
            declare,
            warm_chat: false,
            warm_speech: false,
            speech_socket: false,
            refused_to: None,
            force: None,
            narrow: None,
            near: None,
            gemini: GeminiParts::default(),
            config: Arc::new(config),
        })
    }

    /// The chat's connection is opened ahead as a line begins, when it has
    /// been idle ([`Chat::warm`], `LAMP_SECOND_CHAT_WARM=on`, #1712: the
    /// chat's first event came in 0.9 to 1.9 s on lamp-4ace, 0.7 s from the
    /// probe).
    pub fn warming_the_chat(mut self, on: bool) -> Self {
        self.warm_chat = on;
        self
    }

    /// The configured voice's connection is opened ahead as a line begins,
    /// when it has been idle ([`Tts::warm`], `LAMP_SECOND_SPEECH_WARM=on`,
    /// #1712: `eleven_v4_turbo`'s first sound came about 690 ms after its
    /// first piece went on lamp-4ace at 19:41; Flash 306 ms from the probe).
    /// Gemini's voice is warmed whenever it is the lamp's.
    pub fn warming_the_speech(mut self, on: bool) -> Self {
        self.warm_speech = on;
        self
    }

    /// A speech socket is opened as each answer begins, while its chat
    /// thinks, and the first of its pieces that finds it open speaks on it
    /// ([`Tts::open_ahead`], `LAMP_SECOND_SPEECH_SOCKET=on`, #2135's row).
    /// Only the configured voice, and only ElevenLabs.
    pub fn speaking_on_a_socket(mut self, on: bool) -> Self {
        self.speech_socket = on;
        self
    }

    /// The chat's requests must call the function `force` names for their
    /// messages ([`net::Chat::forcing`]). Called after the chat is chosen.
    pub fn forcing(mut self, force: net::Force) -> Self {
        self.chat.forcing(force.clone());
        self.force.get_or_insert(force);
        self
    }

    /// The chat as the route chose it, for the hand-over probe
    /// (`probe::handover`), which asks it as a spoken turn would.
    pub(crate) fn chat_client(&self) -> Arc<Chat> {
        self.chat.clone()
    }

    /// The chat's requests are given only the tools `narrow` keeps
    /// ([`net::Chat::narrowing`]). Called after the chat is chosen: one
    /// chosen later is given every tool.
    pub fn narrowing(mut self, narrow: net::Narrow) -> Self {
        if let Some(refused) = &self.refused_to {
            refused.relay.narrowing(narrow.clone());
        }
        self.chat.narrowing(narrow.clone());
        self.narrow.get_or_insert(narrow);
        self
    }

    /// The relay's near addresses go over the services' own for those
    /// `routable` names (`LAMP_RELAY_ROUTES=on`, #2882), once
    /// [`Self::routes_task`] has read them: chat and speech from a turn's
    /// beginning to its end, listening at each utterance. A near service
    /// that fails on the link, with a 5xx or a 401 goes to its own address
    /// for the rest of the turn.
    pub fn routing_through_the_relay(mut self, routable: Routable) -> Self {
        self.near = Some(Arc::new(Near {
            routable,
            kept: Arc::default(),
            url: routes::url(&self.config.llm_base_url),
            turn: Mutex::default(),
            chat: Mutex::default(),
            tts: Mutex::default(),
            hearing: Mutex::default(),
            gemini_chat: Mutex::default(),
            gemini_tts: Mutex::default(),
        }));
        self
    }

    /// The task that asks the relay for its routes, for the worker's
    /// runtime ([`routes::task`]); `None` without [`Self::routing_through_the_relay`].
    pub fn routes_task(&self) -> Option<BoxFuture<'static, ()>> {
        let near = self.near.as_ref()?;
        Some(Box::pin(routes::task(
            near.url.clone(),
            format!("Bearer {}", self.config.llm_key.expose()),
            near.kept.clone(),
        )))
    }

    /// The turn's near chat, if its chat is near: DeepSeek's at that
    /// address with the relay's key, or the route's Gemini at the relay's
    /// near Gemini, forced and narrowed as the chat is; and which it is.
    fn near_chat(&self) -> Option<(Arc<Chat>, Taken, Service)> {
        let near = self.near.as_ref()?;
        if let (Some(model), Some(key)) = (&self.gemini.chat_model, &self.gemini.key) {
            let (url, taken) = near.turn(Service::Gemini)?;
            let chat = Near::client(&near.gemini_chat, &url, || {
                let chat = Chat::gemini(&url, model, key, self.declare.clone()).ok()?;
                if let Some(force) = &self.force {
                    chat.forcing(force.clone());
                }
                if let Some(narrow) = &self.narrow {
                    chat.narrowing(narrow.clone());
                }
                Some(chat)
            })?;
            return Some((chat, taken, Service::Gemini));
        }
        let (url, taken) = near.turn(Service::Chat)?;
        let chat = Near::client(&near.chat, &url, || {
            let chat = Chat::deepseek_at(
                &url,
                &self.config.hosted.deepseek_model,
                self.config.llm_key.expose(),
                self.declare.clone(),
                false,
            )
            .ok()?;
            if let Some(force) = &self.force {
                chat.forcing(force.clone());
            }
            if let Some(narrow) = &self.narrow {
                chat.narrowing(narrow.clone());
            }
            Some(chat)
        })?;
        Some((chat, taken, Service::Chat))
    }

    /// The turn's near Gemini voice, if the route speaks in Gemini's and
    /// its Gemini is near.
    fn near_gemini_tts(&self) -> Option<(Arc<GeminiTts>, Taken)> {
        let near = self.near.as_ref()?;
        let (model, voice) = self.gemini.speech.as_ref()?;
        let key = self.gemini.key.as_ref()?;
        let (url, taken) = near.turn(Service::Gemini)?;
        let tts = Near::client(&near.gemini_tts, &url, || {
            GeminiTts::new(&url, model, key, voice).ok()
        })?;
        Some((tts, taken))
    }

    /// The turn's near voice, if its speech is near: the configured voice
    /// at that address.
    fn near_tts(&self) -> Option<(Arc<Tts>, Taken)> {
        let near = self.near.as_ref()?;
        let (url, taken) = near.turn(Service::Speech)?;
        let tts = Near::client(&near.tts, &url, || {
            let mut config = (*self.config).clone();
            config.tts_base_url = url.clone();
            Tts::new(&config).ok()
        })?;
        Some((tts, taken))
    }

    /// The configuration listening opens with, and where, if its address
    /// is near now and has not failed since the routes were read.
    fn near_hearing(&self) -> Option<(Arc<SecondConfig>, String, Arc<Near>)> {
        let near = self.near.as_ref()?;
        if !near.routable.listening {
            return None;
        }
        let url = near.kept.latest().listening?;
        if near.kept.held("listening", &url) {
            return None;
        }
        let config = Near::client(&near.hearing, &url, || {
            let mut config = (*self.config).clone();
            match &mut config.stt {
                Stt::Autonomous { base_url, .. } => *base_url = url.clone(),
                Stt::Deepgram { .. } => return None,
            }
            Some(config)
        })?;
        Some((config, url, near.clone()))
    }

    /// A request the person's own key is refused for goes to the relay's
    /// DeepSeek chat at `relay` with the relay's key, never a failed answer
    /// (the first builder on #2094); `told` hears each refusal's status.
    pub fn falling_back_to_the_relay(
        mut self,
        relay: &str,
        told: Arc<dyn Fn(u16) + Send + Sync>,
    ) -> net::Result<Self> {
        self.refused_to = Some(RefusedTo {
            relay: Arc::new(Chat::deepseek_at(
                relay,
                &self.config.hosted.deepseek_model,
                self.config.llm_key.expose(),
                self.declare.clone(),
                false,
            )?),
            told,
        });
        Ok(self)
    }

    /// The text model is Gemini, called directly (`LAMP_SECOND_CHAT=gemini`),
    /// in place of the configured chat.
    pub fn chatting_with_gemini(mut self, gemini: &GeminiCall<'_>) -> net::Result<Self> {
        self.chat = Arc::new(Chat::gemini(
            gemini.base_url,
            gemini.model,
            gemini.key,
            self.declare.clone(),
        )?);
        self.gemini.key = Some(gemini.key.to_owned());
        self.gemini.chat_model = Some(gemini.model.to_owned());
        Ok(self)
    }

    /// The text model is OpenRouter's (`LAMP_SECOND_CHAT=openrouter`), its
    /// web plugin on with `search` (`LAMP_SECOND_SEARCH=openrouter`).
    pub fn chatting_with_openrouter(mut self, key: &str, search: bool) -> net::Result<Self> {
        self.chat = Arc::new(Chat::openrouter(
            &self.config.hosted.openrouter_model,
            &self.config.hosted.openrouter_effort,
            key,
            self.declare.clone(),
            search,
        )?);
        Ok(self)
    }

    /// The text model is DeepSeek's own (`LAMP_SECOND_CHAT=deepseek`),
    /// thinking off.
    pub fn chatting_with_deepseek(mut self, key: &str) -> net::Result<Self> {
        let base = self
            .config
            .hosted
            .deepseek_base_url
            .clone()
            .unwrap_or_else(|| crate::second_config::DEEPSEEK_BASE_URL.to_owned());
        self.chat = Arc::new(Chat::deepseek_at(
            &base,
            &self.config.hosted.deepseek_model,
            key,
            self.declare.clone(),
            false,
        )?);
        Ok(self)
    }

    /// A chat told every function, or only those named.
    fn chat_told(
        &self,
        messages: Vec<Message>,
        only: Option<&'static [&'static str]>,
    ) -> BoxStream<'static, net::Result<ChatEvent>> {
        let chat = self.chat.clone();
        let refused_to = self.refused_to.clone();
        let near = self.near_chat();
        stream::once(async move {
            // Near, unless it fails on the link: then the same request goes
            // to the chat's own address (#2882).
            if let Some((near_chat, taken, service)) = near {
                let answer = near_chat.send_only(&messages, only).await;
                match answer.as_ref().err().and_then(routes::fallback_why) {
                    Some(why) => taken.fell(service, &why),
                    None => return answer,
                }
            }
            let answer = chat.send_only(&messages, only).await;
            if let (Err(error), Some(refused)) = (&answer, &refused_to)
                && let Some(status) = key_refused(error)
            {
                (refused.told)(status);
                return refused.relay.send_only(&messages, only).await;
            }
            answer
        })
        .map_ok(|answer| {
            stream::try_unfold(answer, |mut answer| async move {
                Ok(answer.next().await?.map(|event| (event, answer)))
            })
        })
        .try_flatten()
        .boxed()
    }

    /// Speech is Gemini's, in `voice` (`LAMP_SECOND_SPEECH=gemini`); a
    /// sentence it cannot say goes to the configured voice.
    pub fn speaking_with_gemini(
        mut self,
        gemini: &GeminiCall<'_>,
        voice: &str,
    ) -> net::Result<Self> {
        self.gemini_tts = Some(Arc::new(GeminiTts::new(
            gemini.base_url,
            gemini.model,
            gemini.key,
            voice,
        )?));
        self.gemini.key = Some(gemini.key.to_owned());
        self.gemini.speech = Some((gemini.model.to_owned(), voice.to_owned()));
        Ok(self)
    }
}

/// Gemini's speech of `text` from its first packet on, or why there is
/// none: it failed before any sound, or ended with none.
async fn gemini_first(
    tts: &GeminiTts,
    text: &str,
) -> net::Result<BoxStream<'static, net::Result<Vec<i16>>>> {
    let mut speech = tts.speak(text).await?;
    let first = speech
        .next()
        .await?
        .ok_or(net::Error::Protocol("speech with no sound"))?;
    let rest = stream::try_unfold(speech, |mut speech| async move {
        Ok(speech.next().await?.map(|packet| (packet, speech)))
    });
    Ok(stream::iter([Ok(first)]).chain(rest).boxed())
}

/// The lamp's one voice ([`Net`]): Gemini's or the configured one.
#[derive(Clone)]
enum Voice {
    Gemini(Arc<GeminiTts>),
    Configured(Arc<Tts>, &'static str),
}

impl Voice {
    fn name(&self) -> &'static str {
        match self {
            Self::Gemini(_) => "Gemini",
            Self::Configured(_, name) => name,
        }
    }

    /// The speech of `text` from its first packet on, or why there is none.
    async fn first(
        &self,
        text: &str,
        turn: Option<net::Turn>,
    ) -> net::Result<BoxStream<'static, net::Result<Vec<i16>>>> {
        match self {
            Self::Gemini(gemini) => gemini_first(gemini, text).await,
            Self::Configured(tts, _) => {
                let mut speech = match turn {
                    Some(turn) => tts.speak_for(text, turn).await?,
                    None => tts.speak(text).await?,
                };
                let first = speech.next().await?;
                let rest = stream::try_unfold(speech, |mut speech| async move {
                    Ok(speech.next().await?.map(|packet| (packet, speech)))
                });
                Ok(stream::iter(first.map(Ok)).chain(rest).boxed())
            }
        }
    }

    /// [`Voice::first`] asked again after a piece failed to start (#4338):
    /// the configured voice over a connection of its own
    /// ([`net::Tts::speak_fresh`]); Gemini as before.
    async fn first_fresh(
        &self,
        text: &str,
        turn: Option<net::Turn>,
    ) -> net::Result<BoxStream<'static, net::Result<Vec<i16>>>> {
        match (self, turn) {
            (Self::Configured(tts, _), Some(turn)) => {
                let mut speech = tts.speak_fresh(text, turn).await?;
                let first = speech.next().await?;
                let rest = stream::try_unfold(speech, |mut speech| async move {
                    Ok(speech.next().await?.map(|packet| (packet, speech)))
                });
                Ok(stream::iter(first.map(Ok)).chain(rest).boxed())
            }
            _ => self.first(text, turn).await,
        }
    }

    /// Whether [`Voice::first_fresh`] asks on a connection of its own: the
    /// configured voice for a turn's piece. Gemini, and speech with no turn,
    /// are asked as the first time (El's P3 on #4345).
    fn asks_fresh(&self, turn: Option<net::Turn>) -> bool {
        matches!((self, turn), (Self::Configured(..), Some(_)))
    }
}

/// What a piece of an answer's speech fails with when it could not be
/// started twice ([`in_one_voice_near`], #4338): not said, so the answer's
/// player says it with the next piece rather than drop it.
pub(crate) const UNSAID: &str = "a sentence's speech";

/// Whether `error` is [`UNSAID`]: a piece not started, never begun.
pub(crate) fn unsaid(error: &net::Error) -> bool {
    matches!(error, net::Error::Timeout(what) if *what == UNSAID)
}

/// How long a voice has to give a sentence's first sound, each time asked
/// (#1844's C): a host that never answers costs 4 s, not the connection's
/// 10 s plus its response's.
const START_WITHIN: std::time::Duration = std::time::Duration::from_secs(4);

/// `text` in the lamp's one voice, never another's (#1712, the owner: "one
/// voice per lamp"). A sentence the voice fails to start within
/// [`START_WITHIN`] is asked once more, on a connection of its own
/// ([`Voice::first_fresh`], #4338); failing again, it fails with [`UNSAID`],
/// which the answer's player says with the next piece, so the turn goes on
/// and no sentence is silently dropped (#1844's C: before, the second
/// failure failed the whole turn; #4338: before, the sentence was dropped).
/// A sentence begun and then lost fails as any does.
#[cfg(test)]
fn in_one_voice(
    voice: Voice,
    text: String,
    turn: Option<net::Turn>,
) -> BoxStream<'static, net::Result<Vec<i16>>> {
    in_one_voice_near(voice, None, text, turn)
}

/// [`in_one_voice`], with `voice` at a near address and `own` the same
/// voice at its own (#2882): a sentence the near one fails to start on the
/// link, with a 5xx or a 401 is said in `own`, which the rest of the turn
/// speaks in; it is asked as any sentence is.
fn in_one_voice_near(
    voice: Voice,
    own: Option<(Voice, Taken, Service)>,
    text: String,
    turn: Option<net::Turn>,
) -> BoxStream<'static, net::Result<Vec<i16>>> {
    stream::once(async move {
        if !GeminiTts::speaks(&text) {
            return Ok(stream::empty().boxed());
        }
        let first = |voice: &Voice| {
            let text = text.clone();
            let voice = voice.clone();
            async move {
                tokio::time::timeout(START_WITHIN, voice.first(&text, turn))
                    .await
                    .unwrap_or(Err(net::Error::Timeout("speech's first sound")))
            }
        };
        let fresh = |voice: &Voice| {
            let text = text.clone();
            let voice = voice.clone();
            async move {
                tokio::time::timeout(START_WITHIN, voice.first_fresh(&text, turn))
                    .await
                    .unwrap_or(Err(net::Error::Timeout("speech's first sound")))
            }
        };
        let mut voice = voice;
        let mut tried = first(&voice).await;
        let why = tried.as_ref().err().and_then(routes::fallback_why);
        if let (Some(why), Some((own, taken, service))) = (why, own) {
            taken.fell(service, &why);
            voice = own;
            tried = first(&voice).await;
        }
        match tried {
            Ok(speech) => Ok(speech),
            Err(error) => {
                let how = if voice.asks_fresh(turn) {
                    ", on a connection of its own"
                } else {
                    ""
                };
                eprintln!(
                    "lamp-voice second: {} failed to start a sentence ({error}); asked once more{how}",
                    voice.name()
                );
                match fresh(&voice).await {
                    Ok(speech) => Ok(speech),
                    Err(again) => {
                        eprintln!(
                            "lamp-voice second: {} failed again ({again}); the sentence could not be started",
                            voice.name()
                        );
                        Err(net::Error::Timeout(UNSAID))
                    }
                }
            }
        }
    })
    .try_flatten()
    .boxed()
}

impl Net {
    /// The lamp's one voice: Gemini's, or the configured one.
    fn voice(&self) -> Voice {
        match &self.gemini_tts {
            Some(gemini) => Voice::Gemini(gemini.clone()),
            None => Voice::Configured(self.tts.clone(), self.config.tts_provider.name()),
        }
    }

    /// The turn's voice, and the same at its own address when that one is
    /// near (#2882).
    fn turn_voice(&self) -> (Voice, Option<(Voice, Taken, Service)>) {
        if let Some((gemini, taken)) = self.near_gemini_tts() {
            return (
                Voice::Gemini(gemini),
                Some((self.voice(), taken, Service::Gemini)),
            );
        }
        match self.near_tts() {
            Some((tts, taken)) if self.gemini_tts.is_none() => (
                Voice::Configured(tts, self.config.tts_provider.name()),
                Some((self.voice(), taken, Service::Speech)),
            ),
            _ => (self.voice(), None),
        }
    }

    /// The configured voice the turn speaks in: near, or its own.
    fn turn_tts(&self) -> Arc<Tts> {
        self.near_tts()
            .map_or_else(|| self.tts.clone(), |(tts, _)| tts)
    }
}

impl Services for Net {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        let config = self.config.clone();
        let near = self.near_hearing();
        // A line ending on "um" waits only if the service keeps it
        // (`LAMP_SECOND_UNFINISHED_WAIT`, #3126).
        let fillers = crate::unfinished_words::on();
        Box::pin(async move {
            // Near, unless it fails on the link: then once at its own
            // address (#2882).
            if let Some((hearing, url, near)) = near {
                match SttStream::connect_hearing_fillers(&hearing, fillers).await {
                    Ok(stream) => return Ok(Box::new(stream) as Box<dyn Listener>),
                    Err(error) => match routes::fallback_why(&error) {
                        // Its own until the routes are next read (El's P2).
                        Some(why) => {
                            near.kept.hold("listening", &url);
                            routes::fell_back("listening", &why);
                        }
                        None => return Err(error),
                    },
                }
            }
            let stream = SttStream::connect_hearing_fillers(&config, fillers).await?;
            Ok(Box::new(stream) as Box<dyn Listener>)
        })
    }

    fn chat(&self, messages: Vec<Message>) -> BoxStream<'static, net::Result<ChatEvent>> {
        self.chat_told(messages, None)
    }

    fn chat_only(
        &self,
        messages: Vec<Message>,
        only: &'static [&'static str],
    ) -> BoxStream<'static, net::Result<ChatEvent>> {
        self.chat_told(messages, Some(only))
    }

    fn chat_again(
        &self,
        messages: Vec<Message>,
        only: Option<&'static [&'static str]>,
        without: &'static [&'static str],
    ) -> BoxStream<'static, net::Result<ChatEvent>> {
        let chat = self.chat.clone();
        let refused_to = self.refused_to.clone();
        let near = self.near_chat();
        stream::once(async move {
            // As the first ask went: near, unless it fails on the link (#2882).
            if let Some((near_chat, taken, service)) = near {
                let answer = send_again(&near_chat, &messages, only, without).await;
                match answer.as_ref().err().and_then(routes::fallback_why) {
                    Some(why) => taken.fell(service, &why),
                    None => return answer,
                }
            }
            let answer = send_again(&chat, &messages, only, without).await;
            // A person's own key refused goes to the relay, as the first
            // ask's does (El's P2 on #3107).
            if let (Err(error), Some(refused)) = (&answer, &refused_to)
                && let Some(status) = key_refused(error)
            {
                (refused.told)(status);
                return send_again(&refused.relay, &messages, only, without).await;
            }
            answer
        })
        .map_ok(|answer| {
            stream::try_unfold(answer, |mut answer| async move {
                Ok(answer.next().await?.map(|event| (event, answer)))
            })
        })
        .try_flatten()
        .boxed()
    }

    fn chat_without(
        &self,
        messages: Vec<Message>,
        without: &'static [&'static str],
    ) -> BoxStream<'static, net::Result<ChatEvent>> {
        let chat = self.chat.clone();
        let near = self.near_chat();
        stream::once(async move {
            if let Some((near_chat, taken, service)) = near {
                let answer = near_chat.send_without(&messages, None, without).await;
                match answer.as_ref().err().and_then(routes::fallback_why) {
                    Some(why) => taken.fell(service, &why),
                    None => return answer,
                }
            }
            chat.send_without(&messages, None, without).await
        })
        .map_ok(|answer| {
            stream::try_unfold(answer, |mut answer| async move {
                Ok(answer.next().await?.map(|event| (event, answer)))
            })
        })
        .try_flatten()
        .boxed()
    }

    fn declares(&self, messages: &[Message], without: &[&str], name: &str) -> Option<bool> {
        Some(self.chat.declares(messages, without, name))
    }

    fn speak(&self, text: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
        let (voice, own) = self.turn_voice();
        in_one_voice_near(voice, own, text, None)
    }

    fn speak_for(
        &self,
        text: String,
        turn: net::Turn,
    ) -> BoxStream<'static, net::Result<Vec<i16>>> {
        let (voice, own) = self.turn_voice();
        in_one_voice_near(voice, own, text, Some(turn))
    }

    fn spoken(&self, request: u64) {
        // The near voice's pieces too, kept while its address stays (#2882).
        let near = self
            .near
            .as_ref()
            .and_then(|near| near.tts.lock().ok()?.as_ref().map(|(_, tts)| tts.clone()));
        for tts in std::iter::once(self.tts.clone()).chain(near) {
            if let Some(told) = tts.told(request) {
                eprintln!("lamp-voice second: {told}");
            }
        }
    }

    fn sees(&self) -> bool {
        self.chat.sees()
    }

    fn open_speech_ahead(&self, request: u64) {
        // The configured voice only when it is the lamp's one voice.
        if !self.speech_socket || self.gemini_tts.is_some() {
            return;
        }
        let tts = self.turn_tts();
        tokio::spawn(async move { tts.open_ahead(request).await });
    }

    fn turn_begins(&self) {
        let Some(near) = &self.near else {
            return;
        };
        let latest = near.kept.latest();
        if let Ok(mut turn) = near.turn.lock() {
            *turn = (
                turn.0.wrapping_add(1),
                // Not one that failed since the routes were read (El's P2).
                Routes {
                    chat: latest
                        .chat
                        .filter(|url| near.routable.chat && !near.kept.held("chat", url)),
                    speech: latest
                        .speech
                        .filter(|url| near.routable.speech && !near.kept.held("speech", url)),
                    listening: None,
                    gemini: latest
                        .gemini
                        .filter(|url| near.routable.gemini && !near.kept.held("gemini", url)),
                },
            );
        }
    }

    fn link_back(&self) {
        if let Some(near) = &self.near {
            near.kept.ask_again();
        }
    }

    fn warm(&self) -> BoxFuture<'static, ()> {
        // The turn's chat and voice, near or their own (#2882).
        let gemini = self
            .near_gemini_tts()
            .map(|(tts, _)| tts)
            .or_else(|| self.gemini_tts.clone());
        let chat = self.warm_chat.then(|| {
            self.near_chat()
                .map_or_else(|| self.chat.clone(), |(chat, _, _)| chat)
        });
        // The configured voice only when it is the lamp's one voice.
        let configured = (self.warm_speech && gemini.is_none()).then(|| self.turn_tts());
        let provider = self.config.tts_provider.name();
        Box::pin(async move {
            let speech = async {
                if let Some(gemini) = gemini
                    && let Some((status, took)) = gemini.warm().await
                {
                    eprintln!(
                        "lamp-voice second: Gemini's speech connection opened ahead in {} ms (HTTP {status})",
                        took.as_millis()
                    );
                }
            };
            let chatting = async {
                if let Some(chat) = chat
                    && let Some((status, took)) = chat.warm().await
                {
                    eprintln!(
                        "lamp-voice second: the chat's connection opened ahead in {} ms (HTTP {status})",
                        took.as_millis()
                    );
                }
            };
            let configured_speech = async {
                if let Some(tts) = configured
                    && let Some((status, took)) = tts.warm().await
                {
                    eprintln!(
                        "lamp-voice second: {provider}: the speech connection opened ahead in {} ms (HTTP {status})",
                        took.as_millis()
                    );
                }
            };
            tokio::join!(speech, chatting, configured_speech);
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::second_config::TtsProvider;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// What a stand-in reads of one connection: the request's head, or
    /// `None` if it closed first, as a bare connect from another test does.
    async fn head_of(socket: &mut tokio::net::TcpStream) -> Option<Vec<u8>> {
        let mut request = Vec::new();
        let mut buffer = [0; 4096];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = socket.read(&mut buffer).await.ok()?;
            if n == 0 {
                return None;
            }
            request.extend_from_slice(&buffer[..n]);
        }
        Some(request)
    }

    const NOT_HERE: &str =
        "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";

    /// The next connection whose request carries `key`, the test's own, within
    /// 500 ms. Any other is answered 404 and not counted: another test's
    /// client, on a port the host has given again, made one of these count 3
    /// for 2 in a full `--lib` run (#1569 row 50). A connection that sends no
    /// whole request head within 200 ms is passed over too, so one left open
    /// and silent can't hold the stand-in (the reviewer on #2316).
    async fn next_ours(
        listener: &tokio::net::TcpListener,
        key: &str,
    ) -> Option<tokio::net::TcpStream> {
        let until = tokio::time::Instant::now() + std::time::Duration::from_millis(500);
        loop {
            let (mut socket, _) = tokio::time::timeout_at(until, listener.accept())
                .await
                .ok()?
                .ok()?;
            let read_until =
                until.min(tokio::time::Instant::now() + std::time::Duration::from_millis(200));
            match tokio::time::timeout_at(read_until, head_of(&mut socket)).await {
                Ok(Some(head)) if head.windows(key.len()).any(|w| w == key.as_bytes()) => {
                    return Some(socket);
                }
                Ok(Some(_)) => {
                    let _ = socket.write_all(NOT_HERE.as_bytes()).await;
                }
                Ok(None) | Err(_) => {}
            }
        }
    }

    /// A local plain HTTP server answering one request carrying `key` with
    /// `head` and `body`; whether one came is read from the handle.
    async fn server(
        head: &'static str,
        body: Vec<u8>,
        key: &'static str,
    ) -> (String, tokio::task::JoinHandle<usize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let Some(mut socket) = next_ours(&listener, key).await else {
                return 0;
            };
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(&body).await.unwrap();
            1
        });
        (format!("http://{address}"), task)
    }

    const PCM: &str =
        "HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\nconnection: close\r\n\r\n";

    async fn said(speech: BoxStream<'static, net::Result<Vec<i16>>>) -> Vec<i16> {
        speech
            .map(|packet| packet.unwrap())
            .collect::<Vec<_>>()
            .await
            .concat()
    }

    fn bytes(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    /// A local plain HTTP server answering each request carrying `key`, on
    /// its own connection, with the next of `answers`; how many came is read
    /// from the handle.
    async fn servers(
        answers: Vec<(&'static str, Vec<u8>)>,
        key: &'static str,
    ) -> (String, tokio::task::JoinHandle<usize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let mut came = 0;
            for (head, body) in answers {
                let Some(mut socket) = next_ours(&listener, key).await else {
                    break;
                };
                came += 1;
                socket.write_all(head.as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
            }
            came
        });
        (format!("http://{address}"), task)
    }

    /// #1569 row 50: a request without the test's own key, such as another
    /// test's client finding this port, is answered 404 and not counted,
    /// nor is a bare connect; the test's own is answered and counted.
    #[tokio::test]
    async fn a_stand_in_counts_only_its_own_tests_requests() {
        let (url, came) = servers(vec![(PCM, bytes(&[1]))], "ours-key").await;
        let address = url.trim_start_matches("http://").to_owned();
        let ask = |key: &'static str| {
            let address = address.clone();
            async move {
                let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
                socket
                    .write_all(
                        format!("POST / HTTP/1.1\r\nx-goog-api-key: {key}\r\n\r\n").as_bytes(),
                    )
                    .await
                    .unwrap();
                let mut answer = Vec::new();
                socket.read_to_end(&mut answer).await.unwrap();
                String::from_utf8_lossy(&answer).into_owned()
            }
        };
        drop(tokio::net::TcpStream::connect(&address).await.unwrap());
        // One left open and silent is passed over, not waited on (the
        // reviewer on #2316); the whole check is bounded, so a stand-in that
        // waited on it fails here rather than hanging.
        let _silent = tokio::net::TcpStream::connect(&address).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            assert!(ask("theirs-key").await.starts_with("HTTP/1.1 404"));
            assert!(ask("ours-key").await.starts_with("HTTP/1.1 200"));
        })
        .await
        .expect("the stand-in waited on a silent connection");
        assert_eq!(came.await.unwrap(), 1);
    }

    const REFUSED: &str =
        "HTTP/1.1 429 Too Many Requests\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";

    fn gemini_event(samples: &[i16]) -> Vec<u8> {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let event = serde_json::json!({"candidates": [{"content": {"parts": [
            {"inlineData": {"mimeType": "audio/l16; rate=24000; channels=1",
                "data": STANDARD.encode(bytes(samples))}}
        ]}}]});
        format!("data: {event}\n\n").into_bytes()
    }

    pub(crate) const SSE: &str =
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";

    /// #1712, one voice per lamp: a sentence Gemini refuses is asked of
    /// Gemini once more, and said when it answers; refused again, it fails
    /// with [`UNSAID`], for the player to say with the next piece (#4338).
    /// No other voice is ever asked.
    #[tokio::test]
    async fn a_sentence_the_voice_cannot_start_is_asked_once_more_never_of_another() {
        let (gemini_url, gemini_asked) = servers(
            vec![(REFUSED, Vec::new()), (SSE, gemini_event(&[7, 9]))],
            "g-once-more",
        )
        .await;
        let gemini = Arc::new(GeminiTts::new(&gemini_url, "tts", "g-once-more", "Kore").unwrap());
        let samples = said(in_one_voice(
            Voice::Gemini(gemini),
            "Hello there.".into(),
            None,
        ))
        .await;
        assert_eq!(samples, [7, 9], "Gemini's own level, on its second asking");
        assert_eq!(gemini_asked.await.unwrap(), 2);

        let (gemini_url, gemini_asked) = servers(
            vec![
                (REFUSED, Vec::new()),
                (REFUSED, Vec::new()),
                (SSE, gemini_event(&[1])),
            ],
            "g-once-more",
        )
        .await;
        let gemini = Arc::new(GeminiTts::new(&gemini_url, "tts", "g-once-more", "Kore").unwrap());
        let results: Vec<_> = in_one_voice(Voice::Gemini(gemini), "Hello there.".into(), None)
            .collect()
            .await;
        assert!(
            matches!(results.as_slice(), [Err(error)] if unsaid(error)),
            "not said, and said so: {results:?}"
        );
        assert_eq!(gemini_asked.await.unwrap(), 2, "asked twice, no more");
    }

    /// The same for the configured voice: once more, then [`UNSAID`].
    #[tokio::test]
    async fn the_configured_voice_is_asked_once_more_then_not() {
        let (openai, asked) = servers(
            vec![(REFUSED, Vec::new()), (PCM, bytes(&[100, -100]))],
            "k-once-more",
        )
        .await;
        let tts = Arc::new(Tts::with(TtsProvider::OpenAi, &openai, "k-once-more", None).unwrap());
        let samples = said(in_one_voice(
            Voice::Configured(tts, "OpenAI"),
            "Hello there.".into(),
            None,
        ))
        .await;
        // The configured voice's level, +6 dB.
        assert_eq!(samples, [200, -200]);
        assert_eq!(asked.await.unwrap(), 2);
        let (openai, asked) = servers(
            vec![(REFUSED, Vec::new()), (REFUSED, Vec::new())],
            "k-once-more",
        )
        .await;
        let tts = Arc::new(Tts::with(TtsProvider::OpenAi, &openai, "k-once-more", None).unwrap());
        let results: Vec<_> = in_one_voice(
            Voice::Configured(tts, "OpenAI"),
            "Hello there.".into(),
            None,
        )
        .collect()
        .await;
        assert!(
            matches!(results.as_slice(), [Err(error)] if unsaid(error)),
            "not said, and said so: {results:?}"
        );
        assert_eq!(asked.await.unwrap(), 2);
    }

    /// #4338 (lamp-3e83, 29 Oct): a piece of a turn's answer the configured
    /// voice fails to start is asked again on a connection of its own
    /// ([`net::Tts::speak_fresh`]), and said when it answers.
    #[tokio::test]
    async fn a_turns_piece_not_started_is_asked_again_on_its_own_connection() {
        let (openai, asked) = servers(
            vec![(REFUSED, Vec::new()), (PCM, bytes(&[100, -100]))],
            "k-fresh",
        )
        .await;
        let tts = Arc::new(Tts::with(TtsProvider::OpenAi, &openai, "k-fresh", None).unwrap());
        let turn = net::Turn {
            request: 4338,
            heard_at: std::time::Instant::now(),
        };
        let samples = said(in_one_voice(
            Voice::Configured(tts, "OpenAI"),
            "Hello there.".into(),
            Some(turn),
        ))
        .await;
        assert_eq!(samples, [200, -200], "said on its second asking");
        assert_eq!(asked.await.unwrap(), 2);
    }

    /// The journal says "on a connection of its own" only when the second
    /// asking takes one: the configured voice for a turn's piece, never
    /// Gemini or speech with no turn (El's P3 on #4345).
    #[test]
    fn only_a_turns_piece_in_the_configured_voice_is_asked_on_its_own_connection() {
        let turn = Some(net::Turn {
            request: 4345,
            heard_at: std::time::Instant::now(),
        });
        let tts =
            Arc::new(Tts::with(TtsProvider::OpenAi, "http://127.0.0.1:9", "k", None).unwrap());
        let gemini = Arc::new(GeminiTts::new("http://127.0.0.1:9", "tts", "k", "Kore").unwrap());
        assert!(Voice::Configured(tts.clone(), "OpenAI").asks_fresh(turn));
        assert!(!Voice::Configured(tts, "OpenAI").asks_fresh(None));
        assert!(!Voice::Gemini(gemini.clone()).asks_fresh(turn));
        assert!(!Voice::Gemini(gemini).asks_fresh(None));
    }

    /// #1844's C: a voice whose host takes the connection and never answers
    /// is given 4 s a try, twice, and the sentence fails with [`UNSAID`],
    /// which the player says with the next piece, so the turn goes on.
    #[tokio::test(start_paused = true)]
    async fn a_voice_that_never_answers_is_given_four_seconds_twice() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let held = tokio::spawn(async move {
            let mut kept = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                kept.push(socket);
            }
        });
        let tts = Arc::new(
            Tts::with(
                TtsProvider::ElevenLabs,
                &format!("http://{address}"),
                "k",
                Some("v"),
            )
            .unwrap(),
        );
        let began = tokio::time::Instant::now();
        let results: Vec<_> = in_one_voice(
            Voice::Configured(tts, "ElevenLabs"),
            "Hello there.".into(),
            None,
        )
        .collect()
        .await;
        assert!(
            matches!(results.as_slice(), [Err(error)] if unsaid(error)),
            "{results:?}"
        );
        let took = began.elapsed();
        assert!(
            took >= 2 * START_WITHIN && took < 2 * START_WITHIN + std::time::Duration::from_secs(1),
            "{took:?}"
        );
        held.abort();
    }

    /// Nothing to say asks no voice at all.
    #[tokio::test]
    async fn only_tags_ask_neither_voice() {
        let (gemini_url, gemini_asked) = server(PCM, Vec::new(), "g-tags-only").await;
        let (openai, openai_asked) = server(PCM, Vec::new(), "k-tags-only").await;
        let gemini = Arc::new(GeminiTts::new(&gemini_url, "tts", "g-tags-only", "Kore").unwrap());
        let tts = Arc::new(Tts::with(TtsProvider::OpenAi, &openai, "k-tags-only", None).unwrap());
        let samples = said(in_one_voice(Voice::Gemini(gemini), "[sigh]".into(), None)).await;
        assert!(samples.is_empty());
        let samples = said(in_one_voice(
            Voice::Configured(tts, "OpenAI"),
            "[sigh]".into(),
            None,
        ))
        .await;
        assert!(samples.is_empty());
        assert_eq!(
            (gemini_asked.await.unwrap(), openai_asked.await.unwrap()),
            (0, 0)
        );
    }

    /// #2882's stand-ins: a local plain HTTP server answering each request
    /// carrying `key`, each on its own connection, by the first of `answers`
    /// whose text is in its request line: that head and body, or, for
    /// `None`, nothing ever. Every request line of the test's own is kept.
    pub(crate) struct Stub {
        pub(crate) url: String,
        seen: Arc<Mutex<Vec<String>>>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Stub {
        pub(crate) fn seen(&self) -> Vec<String> {
            self.seen.lock().unwrap().clone()
        }
        pub(crate) fn asked(&self, path: &str) -> usize {
            self.seen()
                .iter()
                .filter(|line| line.contains(path))
                .count()
        }
    }

    impl Drop for Stub {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    pub(crate) type Answers = Vec<(&'static str, Option<(&'static str, Vec<u8>)>)>;

    pub(crate) async fn stub(key: &'static str, answers: Answers) -> Stub {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let answers = Arc::new(answers);
        let kept = seen.clone();
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let seen = kept.clone();
                let answers = answers.clone();
                tokio::spawn(async move {
                    let Ok(Some(head)) = tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        head_of(&mut socket),
                    )
                    .await
                    else {
                        return;
                    };
                    let text = String::from_utf8_lossy(&head).into_owned();
                    if !text.contains(key) {
                        let _ = socket.write_all(NOT_HERE.as_bytes()).await;
                        return;
                    }
                    // The body read to its end, so the answer is not lost to
                    // a reset.
                    let ends = text.find("\r\n\r\n").unwrap() + 4;
                    let length: usize = text
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|n| n.trim().parse().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    let mut left = length.saturating_sub(head.len() - ends);
                    let mut buffer = [0; 4096];
                    while left > 0 {
                        match socket.read(&mut buffer).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => left = left.saturating_sub(n),
                        }
                    }
                    let line = text.lines().next().unwrap_or_default().to_owned();
                    seen.lock().unwrap().push(line.clone());
                    match answers.iter().find(|(path, _)| line.contains(path)) {
                        Some((_, Some((head, body)))) => {
                            let _ = socket.write_all(head.as_bytes()).await;
                            let _ = socket.write_all(body).await;
                        }
                        Some((_, None)) => {
                            tokio::time::sleep(std::time::Duration::from_secs(600)).await;
                        }
                        None => {
                            let _ = socket.write_all(NOT_HERE.as_bytes()).await;
                        }
                    }
                });
            }
        });
        Stub {
            url: format!("http://{address}"),
            seen,
            task,
        }
    }

    pub(crate) const CHAT: &str = "/chat/completions";
    const SPEECH: &str = "/text-to-speech/";
    /// A near route on another host is a whole API base, as the unit's own
    /// addresses are (`net::openai_base` adds `/v1` to the company's alone).
    pub(crate) const BASE: &str = "/api/v1/ai/v1";
    pub(crate) const ROUTES: &str = "/relay/routes";

    pub(crate) fn answered(words: &str) -> Option<(&'static str, Vec<u8>)> {
        Some((
            SSE,
            format!(
                "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{words}\"}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n"
            )
            .into_bytes(),
        ))
    }

    pub(crate) fn json_answer(body: serde_json::Value) -> Option<(&'static str, Vec<u8>)> {
        Some((
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n",
            body.to_string().into_bytes(),
        ))
    }

    const UNAVAILABLE: &str =
        "HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
    const UNAUTHORIZED: &str =
        "HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
    const BAD_REQUEST: &str =
        "HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";

    /// The route at `relay` (its chat, speech and the routes asked of it),
    /// ElevenLabs through the relay, the relay's key `key`, and listening at `listening`.
    pub(crate) fn config_at(name: &str, key: &str, relay: &str, listening: &str) -> SecondConfig {
        use std::os::unix::fs::PermissionsExt;
        let directory =
            std::env::temp_dir().join(format!("lamp-voice-routes-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let private = |path: &std::path::Path, text: &str| {
            std::fs::write(path, text).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        };
        let credential = directory.join("llm.key");
        private(&credential, key);
        let file = directory.join("second.json");
        private(
            &file,
            &format!(
                r#"{{"llm_base_url":"https://relay.example.invalid/api/v1/ai","llm_model":"m","llm_credential_file":"{}","tts_provider":"elevenlabs"}}"#,
                credential.display()
            ),
        );
        let mut config = crate::second_config::load_from_path(&file).unwrap();
        // The file takes https only: pointed at the stand-ins after.
        config.llm_base_url = relay.to_owned();
        config.tts_base_url = relay.to_owned();
        if let Stt::Autonomous { base_url, .. } = &mut config.stt {
            *base_url = listening.to_owned();
        }
        config
    }

    const EVERY: Routable = Routable {
        chat: true,
        speech: true,
        listening: true,
        gemini: true,
    };
    /// DeepSeek's chat, the configured voice and the relay's listening.
    const THREE: Routable = Routable {
        gemini: false,
        ..EVERY
    };

    fn routed(config: SecondConfig, routable: Routable) -> Net {
        Net::new(config, Arc::new(Vec::new))
            .unwrap()
            .routing_through_the_relay(routable)
    }

    fn kept(net: &Net) -> Arc<routes::Kept> {
        net.near.as_ref().unwrap().kept.clone()
    }

    fn near_chat(near: &Stub) -> Routes {
        Routes {
            chat: Some(format!("{}{BASE}/deepseek", near.url)),
            ..Routes::default()
        }
    }

    async fn words(net: &Net) -> String {
        net.chat(vec![Message::user("hello")])
            .filter_map(|event| async move {
                match event.unwrap() {
                    ChatEvent::Text(text) => Some(text),
                    _ => None,
                }
            })
            .collect::<Vec<_>>()
            .await
            .concat()
    }

    /// #3107, El's P2 and Second's test (02:25): the ask again keeps the
    /// own key's refusal fallback, as the first ask does: refused, it goes to
    /// the relay, and the refusal is told.
    #[tokio::test]
    async fn an_ask_again_keeps_the_own_key_refusal_relay() {
        let own = stub(
            "k-second-refused",
            vec![(CHAT, Some((UNAUTHORIZED, Vec::new())))],
        )
        .await;
        let relay = stub("k-second-refused", vec![(CHAT, answered("via relay"))]).await;
        let told: Arc<Mutex<Vec<u16>>> = Arc::default();
        let observed = told.clone();
        let net = Net::new(
            config_at(
                "second-hedge-refusal",
                "k-second-refused",
                &own.url,
                &own.url,
            ),
            Arc::new(Vec::new),
        )
        .unwrap()
        .falling_back_to_the_relay(
            &relay.url,
            Arc::new(move |status| observed.lock().unwrap().push(status)),
        )
        .unwrap();
        assert_eq!(
            words(&net).await,
            "via relay",
            "the ordinary request keeps its configured fallback"
        );
        assert_eq!(*told.lock().unwrap(), vec![401]);
        assert_eq!(relay.asked(CHAT), 1);
        let mut hedged = net.chat_again(vec![Message::user("hello")], None, &[]);
        let next = tokio::time::timeout(std::time::Duration::from_secs(2), hedged.next())
            .await
            .unwrap();
        assert!(
            matches!(&next, Some(Ok(ChatEvent::Text(text))) if text == "via relay"),
            "the fresh request must preserve the same configured fallback; got {next:?}"
        );
        assert_eq!(*told.lock().unwrap(), vec![401, 401]);
        assert_eq!(relay.asked(CHAT), 2);
    }

    /// #2882 (b): the routes are read in the background, with the relay's
    /// key, and a turn begun after goes to the near chat they name.
    #[tokio::test]
    async fn the_routes_read_send_the_next_turns_chat_near() {
        let near = stub("k-routes-near", vec![(CHAT, answered("near"))]).await;
        let routes = serde_json::json!({"status": 1, "data": {
            "chat": format!("{}{BASE}/deepseek", near.url), "speech": null,
            "listening": null, "gemini": null, "ttl_seconds": 600}});
        let relay = stub(
            "k-routes-near",
            vec![(ROUTES, json_answer(routes)), (CHAT, answered("relay"))],
        )
        .await;
        let net = routed(
            config_at("near", "k-routes-near", &relay.url, &relay.url),
            EVERY,
        );
        let task = tokio::spawn(net.routes_task().unwrap());
        let read = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while kept(&net).latest().chat.is_none() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await;
        assert!(read.is_ok(), "the routes never came: {:?}", relay.seen());
        assert_eq!(relay.seen(), ["GET /relay/routes HTTP/1.1"]);
        // Not before a turn begins.
        assert_eq!(words(&net).await, "relay");
        net.turn_begins();
        assert_eq!(words(&net).await, "near");
        assert_eq!(
            near.seen(),
            ["POST /api/v1/ai/v1/deepseek/chat/completions HTTP/1.1"]
        );
        assert_eq!(relay.asked(CHAT), 1);
        task.abort();
    }

    /// #2882: the route's own Gemini chat (`LAMP_SECOND_CHAT=gemini`) goes
    /// to the relay's near Gemini from the turn after the routes say so, at
    /// the path its own takes (`/openai/chat/completions` under the base),
    /// and back to its own for the turn when the near one answers a 503.
    #[tokio::test]
    async fn the_routes_gemini_chat_goes_near_and_falls_back() {
        const GEMINI: &str = "/google-search/v1beta";
        let near = stub("k-routes-gemini", vec![(CHAT, answered("near gemini"))]).await;
        let relay = stub("k-routes-gemini", vec![(CHAT, answered("own gemini"))]).await;
        let own = format!("{}{BASE}{GEMINI}", relay.url);
        let net = Net::new(
            config_at("gemini", "k-routes-gemini", &relay.url, &relay.url),
            Arc::new(Vec::new),
        )
        .unwrap()
        .chatting_with_gemini(&GeminiCall {
            base_url: &own,
            key: "k-routes-gemini",
            model: "gemini-test",
        })
        .unwrap()
        .routing_through_the_relay(EVERY);
        kept(&net).set(Routes {
            gemini: Some(format!("{}{BASE}{GEMINI}", near.url)),
            ..Routes::default()
        });
        assert_eq!(words(&net).await, "own gemini");
        net.turn_begins();
        assert_eq!(words(&net).await, "near gemini");
        assert_eq!(
            near.seen(),
            ["POST /api/v1/ai/v1/google-search/v1beta/openai/chat/completions HTTP/1.1"]
        );
        // Not routable: its own.
        let kept_own = Net::new(
            config_at("gemini-own", "k-routes-gemini", &relay.url, &relay.url),
            Arc::new(Vec::new),
        )
        .unwrap()
        .chatting_with_gemini(&GeminiCall {
            base_url: &own,
            key: "k-routes-gemini",
            model: "gemini-test",
        })
        .unwrap()
        .routing_through_the_relay(THREE);
        kept(&kept_own).set(Routes {
            gemini: Some(format!("{}{BASE}{GEMINI}", near.url)),
            ..Routes::default()
        });
        kept_own.turn_begins();
        assert_eq!(words(&kept_own).await, "own gemini");
        assert_eq!(near.asked(CHAT), 1);
        // A near Gemini that answers a 503: its own, for the rest of the turn.
        let down = stub(
            "k-routes-gemini",
            vec![(CHAT, Some((UNAVAILABLE, Vec::new())))],
        )
        .await;
        kept(&net).set(Routes {
            gemini: Some(format!("{}{BASE}{GEMINI}", down.url)),
            ..Routes::default()
        });
        net.turn_begins();
        assert_eq!(words(&net).await, "own gemini");
        assert_eq!(words(&net).await, "own gemini");
        assert_eq!(down.asked(CHAT), 1);
    }

    /// #2882 (c), (d): a route null is the chat's own address, and so is
    /// one for a service the route may not route.
    #[tokio::test]
    async fn null_or_not_routable_is_the_chats_own() {
        let near = stub("k-routes-null", vec![(CHAT, answered("near"))]).await;
        let relay = stub("k-routes-null", vec![(CHAT, answered("relay"))]).await;
        let net = routed(
            config_at("null", "k-routes-null", &relay.url, &relay.url),
            EVERY,
        );
        kept(&net).set(Routes::default());
        net.turn_begins();
        assert_eq!(words(&net).await, "relay");
        let pinned = routed(
            config_at("pinned", "k-routes-null", &relay.url, &relay.url),
            Routable {
                chat: false,
                ..EVERY
            },
        );
        kept(&pinned).set(near_chat(&near));
        pinned.turn_begins();
        assert_eq!(words(&pinned).await, "relay");
        assert!(near.seen().is_empty(), "{:?}", near.seen());
        assert_eq!(relay.asked(CHAT), 2);
    }

    /// #2882 (d): the unit's own address for a service wins over the
    /// relay's, and so does a chat or voice not through the relay's key.
    #[test]
    fn the_units_own_addresses_and_other_chats_are_not_routed() {
        use super::super::worker::{ChatRoute, RouteFlags, SpeechRoute, relay_routable};
        let mut config = config_at("routable", "k-routable", "http://127.0.0.1:9", "x");
        // DeepSeek's own API, not the relay's: not routed (reviewer-two on #2919).
        let own_api = RouteFlags {
            chat: Some(ChatRoute::DeepSeek),
            ..RouteFlags::default()
        };
        assert!(!relay_routable(&config, own_api, None).chat);
        config.hosted.deepseek_base_url =
            Some("https://relay.example.invalid/api/v1/ai/v1/deepseek".into());
        let deepseek = RouteFlags {
            chat: Some(ChatRoute::DeepSeek),
            ..RouteFlags::default()
        };
        assert_eq!(relay_routable(&config, deepseek, None), THREE);
        assert!(!relay_routable(&config, RouteFlags::default(), None).chat);
        let gemini = RouteFlags {
            speech: Some(SpeechRoute::Gemini),
            ..deepseek
        };
        assert!(!relay_routable(&config, gemini, None).speech);
        config.pinned = crate::second_config::Pinned {
            gemini: true,
            chat: true,
            speech: true,
            listening: true,
        };
        assert_eq!(relay_routable(&config, deepseek, None), Routable::default());
        config.pinned = crate::second_config::Pinned::default();
        // Speech or listening with another service's key, not the relay's.
        let relay_key = config.llm_key.clone();
        let other = crate::second_config::Key::for_test("k-another-service");
        config.tts_key = other.clone();
        assert!(!relay_routable(&config, deepseek, None).speech);
        config.tts_key = relay_key.clone();
        if let Stt::Autonomous { key, .. } = &mut config.stt {
            *key = other;
        }
        assert!(!relay_routable(&config, deepseek, None).listening);
        if let Stt::Autonomous { key, .. } = &mut config.stt {
            *key = relay_key;
        }
        assert_eq!(relay_routable(&config, deepseek, None), THREE);
        config.hosted.own_deepseek = config.tts_key.clone().into();
        assert!(!relay_routable(&config, deepseek, None).chat);
        config.stt = Stt::Deepgram {
            key: config.tts_key.clone(),
        };
        assert!(!relay_routable(&config, deepseek, None).listening);
    }

    /// #2882: the route's own Gemini, chat or voice, is routed only when it
    /// already goes to the relay with the relay's key: never with a Google
    /// key, at Google's own address, or at an address the unit gave.
    #[test]
    fn the_routes_gemini_is_routed_only_through_the_relay_with_its_key() {
        use super::super::worker::{ChatRoute, RouteFlags, SpeechRoute, relay_routable};
        let mut config = config_at("routable-gemini", "k-gemini", "http://127.0.0.1:9", "x");
        config.gemini.base_url = "https://relay.example.invalid/v1/google-search/v1beta".into();
        let relay_key = config.llm_key.clone();
        let chat = RouteFlags {
            chat: Some(ChatRoute::Gemini),
            ..RouteFlags::default()
        };
        let speech = RouteFlags {
            speech: Some(SpeechRoute::Gemini),
            ..RouteFlags::default()
        };
        assert!(relay_routable(&config, chat, Some(&relay_key)).gemini);
        assert!(relay_routable(&config, speech, Some(&relay_key)).gemini);
        assert!(!relay_routable(&config, RouteFlags::default(), Some(&relay_key)).gemini);
        let google = crate::second_config::Key::for_test("k-a-google-key");
        assert!(!relay_routable(&config, chat, Some(&google)).gemini);
        assert!(!relay_routable(&config, chat, None).gemini);
        config.pinned.gemini = true;
        assert!(!relay_routable(&config, chat, Some(&relay_key)).gemini);
        config.pinned.gemini = false;
        config.gemini.base_url = crate::second_config::GEMINI_BASE_URL.into();
        assert!(!relay_routable(&config, chat, Some(&relay_key)).gemini);
    }

    /// #2882 (e): a near chat that refuses the connection, answers 503 or
    /// 401 is asked no more this turn: the same request goes to the chat's
    /// own address, and the answer comes. A 400 is not the link's, and
    /// fails as it would.
    #[tokio::test]
    async fn a_near_chat_failing_on_the_link_goes_to_its_own_for_the_turn() {
        let relay = stub("k-routes-fall", vec![(CHAT, answered("relay"))]).await;
        let closed = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}", listener.local_addr().unwrap())
        };
        for (head, why) in [
            (None, "closed"),
            (Some(UNAVAILABLE), "503"),
            (Some(UNAUTHORIZED), "401"),
        ] {
            let near = stub(
                "k-routes-fall",
                vec![(CHAT, head.map(|head| (head, Vec::new())))],
            )
            .await;
            let at = if head.is_some() { &near.url } else { &closed };
            let net = routed(
                config_at("fall", "k-routes-fall", &relay.url, &relay.url),
                EVERY,
            );
            kept(&net).set(Routes {
                chat: Some(format!("{at}{BASE}/deepseek")),
                ..Routes::default()
            });
            net.turn_begins();
            let before = relay.asked(CHAT);
            assert_eq!(words(&net).await, "relay", "{why}");
            // The rest of the turn at its own address.
            assert_eq!(words(&net).await, "relay", "{why}");
            assert_eq!(relay.asked(CHAT), before + 2, "{why}");
            let expected: &[&str] = if head.is_some() {
                &["POST /api/v1/ai/v1/deepseek/chat/completions HTTP/1.1"]
            } else {
                &[]
            };
            assert_eq!(near.seen(), expected, "{why}");
            // Nor the next turn: held at its own until the routes are next
            // read (El's P2 on #2919), then tried afresh.
            net.turn_begins();
            assert!(net.near_chat().is_none(), "{why}");
            assert_eq!(words(&net).await, "relay", "{why}");
            assert_eq!(near.seen(), expected, "{why}");
            kept(&net).set(Routes {
                chat: Some(format!("{at}{BASE}/deepseek")),
                ..Routes::default()
            });
            net.turn_begins();
            assert!(net.near_chat().is_some(), "{why}");
        }
        let refusing = stub(
            "k-routes-fall",
            vec![(CHAT, Some((BAD_REQUEST, Vec::new())))],
        )
        .await;
        let net = routed(
            config_at("fall-400", "k-routes-fall", &relay.url, &relay.url),
            EVERY,
        );
        kept(&net).set(Routes {
            chat: Some(format!("{}{BASE}/deepseek", refusing.url)),
            ..Routes::default()
        });
        net.turn_begins();
        let before = relay.asked(CHAT);
        let events: Vec<_> = net.chat(vec![Message::user("hello")]).collect().await;
        assert_eq!(events, [Err(net::Error::Status(400))]);
        assert_eq!(relay.asked(CHAT), before);
    }

    /// #2882 (e): a sentence the near voice can't start for a 503 is said
    /// in the voice at its own address, and so is the rest of the turn's.
    #[tokio::test]
    async fn a_near_voice_failing_goes_to_its_own_for_the_turn() {
        let relay = stub("k-routes-speech", vec![(SPEECH, Some((PCM, bytes(&[5]))))]).await;
        let near = stub(
            "k-routes-speech",
            vec![(SPEECH, Some((UNAVAILABLE, Vec::new())))],
        )
        .await;
        let net = routed(
            config_at("speech", "k-routes-speech", &relay.url, &relay.url),
            EVERY,
        );
        kept(&net).set(Routes {
            speech: Some(format!("{}{BASE}", near.url)),
            ..Routes::default()
        });
        net.turn_begins();
        assert!(!said(net.speak("Hello there.".into())).await.is_empty());
        assert!(!said(net.speak("Hello again.".into())).await.is_empty());
        // ElevenLabs through the near relay, as `service_base` builds it.
        let near_seen = near.seen();
        assert_eq!(near_seen.len(), 1, "{near_seen:?}");
        assert!(
            near_seen[0].starts_with("POST /api/v1/ai/v1/elevenlabs/text-to-speech/")
                && near_seen[0].contains("/stream?output_format=pcm_24000 "),
            "{near_seen:?}"
        );
        assert_eq!(relay.asked(SPEECH), 2);
    }

    /// Reviewer-two's P2 on #2919: a sentence taken for an earlier turn at
    /// near address A (a cue or down line still rendering) that fails there
    /// after the next turn began at B goes to its own address, and leaves B
    /// to that turn: its next sentence is said at B.
    #[tokio::test]
    async fn an_earlier_turns_failure_never_clears_a_later_turns_route() {
        let relay = stub("k-routes-later", vec![(SPEECH, Some((PCM, bytes(&[5]))))]).await;
        let a = stub(
            "k-routes-later",
            vec![(SPEECH, Some((UNAVAILABLE, Vec::new())))],
        )
        .await;
        let b = stub("k-routes-later", vec![(SPEECH, Some((PCM, bytes(&[7]))))]).await;
        let net = routed(
            config_at("later", "k-routes-later", &relay.url, &relay.url),
            EVERY,
        );
        let at = |stub: &Stub| Routes {
            speech: Some(format!("{}{BASE}", stub.url)),
            ..Routes::default()
        };
        kept(&net).set(at(&a));
        net.turn_begins();
        // Taken for the first turn, asked only after the second began.
        let earlier = net.speak("Hold on.".into());
        kept(&net).set(at(&b));
        net.turn_begins();
        assert!(!said(earlier).await.is_empty());
        assert_eq!((a.asked(SPEECH), relay.asked(SPEECH)), (1, 1));
        assert!(!said(net.speak("Hello there.".into())).await.is_empty());
        assert_eq!(b.asked(SPEECH), 1, "the later turn kept B");
        assert_eq!(relay.asked(SPEECH), 1);
    }

    /// #2882 (e): listening that can't open at its near address for a 503
    /// opens once at its own; listening reads the latest routes, with no
    /// turn begun.
    #[tokio::test]
    async fn near_listening_failing_opens_at_its_own() {
        let own = stub("k-routes-hear", vec![]).await;
        let near = stub(
            "k-routes-hear",
            vec![("/ws/audio/transcriptions", Some((UNAVAILABLE, Vec::new())))],
        )
        .await;
        let net = routed(
            config_at("hear", "k-routes-hear", &own.url, &own.url),
            EVERY,
        );
        kept(&net).set(Routes {
            listening: Some(format!("{}{BASE}", near.url)),
            ..Routes::default()
        });
        // The stand-in at its own address is no socket: answered 404.
        assert!(matches!(
            net.listen().await.err(),
            Some(net::Error::Status(404))
        ));
        // The socket's path as `proxied` builds it, ws:// here.
        let near_seen = near.seen();
        assert_eq!(near_seen.len(), 1, "{near_seen:?}");
        assert!(
            near_seen[0].starts_with("GET /api/v1/ai/v1/ws/audio/transcriptions?"),
            "{near_seen:?}"
        );
        assert_eq!(own.asked("/ws/audio/transcriptions"), 1);
        // The next utterance opens at its own at once: the near address is
        // held until the routes are next read (El's P2 on #2919).
        assert!(net.listen().await.is_err());
        assert_eq!(near.seen().len(), 1);
        assert_eq!(own.asked("/ws/audio/transcriptions"), 2);
        kept(&net).set(Routes {
            listening: Some(format!("{}{BASE}", near.url)),
            ..Routes::default()
        });
        assert!(net.listen().await.is_err());
        assert_eq!(near.seen().len(), 2);
    }

    /// #2882 (f): a relay that never answers its routes delays no turn: the
    /// chat goes to its own address at once.
    #[tokio::test]
    async fn a_routes_call_that_never_answers_delays_no_turn() {
        let relay = stub(
            "k-routes-slow",
            vec![(ROUTES, None), (CHAT, answered("relay"))],
        )
        .await;
        let net = routed(
            config_at("slow", "k-routes-slow", &relay.url, &relay.url),
            EVERY,
        );
        let task = tokio::spawn(net.routes_task().unwrap());
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while relay.asked(ROUTES) == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the routes were asked");
        let began = std::time::Instant::now();
        net.turn_begins();
        assert_eq!(words(&net).await, "relay");
        assert!(
            began.elapsed() < std::time::Duration::from_secs(2),
            "{:?}",
            began.elapsed()
        );
        task.abort();
    }

    /// #2882 (g): routes that change while a turn goes on are its chat's
    /// and speech's only from the next turn's beginning.
    #[tokio::test]
    async fn a_route_changed_mid_turn_waits_for_the_next() {
        let near = stub("k-routes-mid", vec![(CHAT, answered("near"))]).await;
        let relay = stub("k-routes-mid", vec![(CHAT, answered("relay"))]).await;
        let net = routed(
            config_at("mid", "k-routes-mid", &relay.url, &relay.url),
            EVERY,
        );
        net.turn_begins();
        kept(&net).set(near_chat(&near));
        assert_eq!(words(&net).await, "relay");
        net.turn_begins();
        assert_eq!(words(&net).await, "near");
        kept(&net).set(Routes::default());
        assert_eq!(words(&net).await, "near");
        net.turn_begins();
        assert_eq!(words(&net).await, "relay");
        // The near chat was made once and kept for its address.
        assert_eq!(near.asked(CHAT), 2);
    }

    /// #2882: a service host answering again asks the relay for its routes
    /// at once, not at their time's end.
    #[tokio::test]
    async fn the_link_back_asks_the_routes_again() {
        let relay = stub(
            "k-routes-back",
            vec![(
                ROUTES,
                json_answer(serde_json::json!({"status": 1, "data": {}})),
            )],
        )
        .await;
        let net = routed(
            config_at("back", "k-routes-back", &relay.url, &relay.url),
            EVERY,
        );
        let task = tokio::spawn(net.routes_task().unwrap());
        let asked = |times: usize| {
            let relay = &relay;
            async move {
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    while relay.asked(ROUTES) < times {
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    }
                })
                .await
                .is_ok()
            }
        };
        assert!(asked(1).await);
        net.link_back();
        assert!(asked(2).await, "{:?}", relay.seen());
        task.abort();
    }
}
