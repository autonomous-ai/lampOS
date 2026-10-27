//! `lamp-voice probe-cascade PROVIDER_CONFIG SECOND_CONFIG [RUNS] [--look JPEG | --wire]`:
//! the second route's candidate services timed from where it runs, for the
//! A/B against Gemini Live (the owner, 19 Oct). Each run asks every
//! candidate in turn:
//!
//! - **chat**: the configured chat (`llm_*`, the relay on the lamps); Gemini
//!   called directly (with a Google key, `worker::gemini_key`); through
//!   OpenRouter, DeepSeek V4.1 Flash's 17 October snapshot and Gemini 3.6
//!   Flash (and `openrouter_model` if it is another), each thinking least;
//!   and on DeepSeek's own API each model its `/models` lists, thinking off
//!   and on; and with `deepseek_base_url` (or `LAMP_SECOND_DEEPSEEK_BASE_URL`),
//!   DeepSeek through the relay with the relay's credential, thinking off
//!   (#1934). The character of `provider.json` and one fixed question; with
//!   `--look`, once more with that JPEG as the question's picture (an image
//!   part, as a look sends it), to each model that takes one. No tools are
//!   declared.
//! - **search**: OpenRouter's web plugin on its first candidate (to its
//!   first event and its first search result), and DeepSeek's search (to its
//!   answer, with the number of results).
//! - **speech**: the configured voice and Gemini's speech model in the Live
//!   route's voice, one fixed sentence each.
//!
//! OpenRouter's and DeepSeek's keys are the unit's credentials
//! (`openrouter.key`, `deepseek.key` in `CREDENTIALS_DIRECTORY`): run it as
//! `systemd-run --pty -p LoadCredential=openrouter.key:… -p
//! LoadCredential=deepseek.key:… lamp-voice probe-cascade …`. A candidate
//! without its key is left out, saying so.
//!
//! It prints milliseconds and counts only: never the words asked, heard or
//! said, nor a key or a URL. Each time runs from the request's start: to the
//! first event (text or a call), to the first clause or sentence the route
//! would say, and to the end; for speech, to the first audio and to the end,
//! with the audio's length. Then the median and the 90th percentile of each.
//!
//! With `LAMP_RELAY_ROUTES=on` in its environment, as in the voice unit's,
//! the probe asks the relay for its near addresses once as it starts, with
//! the route's key, and each arm that goes through the relay reaches the
//! address the voice would: near where the routes name one and the voice
//! may route that service (`worker::relay_routable`), else its normal
//! address; an address the unit's environment gives
//! (`LAMP_SECOND_*_BASE_URL`) is pinned and never routed. Each such arm's
//! run line ends `relay=near`, `relay=normal` or `relay=pinned` and the
//! host it reached, never a path or a key ([`Relay`]).
//!
//! With `--wire`, only where a chat request's time goes between the lamp
//! and DeepSeek, through the relay and on its own API, and with `--speech`
//! a piece of speech's between the lamp and ElevenLabs ([`wire`]).
use super::net::{
    self, Chat, ChatEvent, GeminiTts, Message, Sentences, Tts,
    routes::{self, Routes},
    search::Search,
};
use super::worker::{ChatRoute, RouteFlags, relay_routable};

pub mod cache;
pub mod handover;
pub mod kept_earlier;
pub mod quality;
pub mod says_kept;
pub mod tools;
pub mod wire;
use crate::second_config::{
    CREDENTIALS_ENV, DEEPSEEK_CREDENTIAL, DEEPSEEK_TEXT_ONLY, Key, OPENROUTER_CREDENTIAL,
    OPENROUTER_MODEL, OPENROUTER_SECOND_MODEL, SecondConfig, least_effort, unit_credential,
};
use futures_util::{
    StreamExt,
    stream::{self, BoxStream},
};
use serde_json::Value;
use std::{io, path::Path, sync::Arc};
use tokio::time::Instant;

/// The question every chat is asked: plain, one or two sentences back.
pub const QUESTION: &str = "What's a nice thing to do on a rainy afternoon?";
/// The question that goes with `--look`'s picture.
pub const LOOK_QUESTION: &str = "What do you see in front of you?";
/// What a search is asked: something only the web knows today.
pub const SEARCH_QUESTION: &str = "What's the weather in Ho Chi Minh City today?";
pub const SEARCH_QUERY: &str = "Ho Chi Minh City weather today";
/// The sentence every voice says: a short first clause, as the route asks.
pub const SENTENCE: &str = "Sure, I can help with that.";
/// The biggest picture a look sends (`LAMP_LOOK_FULL`'s bound).
const MOST_JPEG: u64 = 184 * 1024;
/// The most of DeepSeek's models probed.
const MOST_MODELS: usize = 8;

/// One chat request's times, in ms from its start.
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub struct ChatTimes {
    pub first: Option<u64>,
    pub clause: Option<u64>,
    pub done: u64,
    /// The first chunk with the web search's results, when it searched.
    pub searched: Option<u64>,
}

/// One speech request's times, in ms from its start, and its audio's length.
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub struct SpeechTimes {
    pub first: Option<u64>,
    pub done: u64,
    pub audio: u64,
}

fn ms(from: Instant) -> u64 {
    from.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

/// The times of a chat's answer: its first event, its first clause or
/// sentence as the route cuts them (`Sentences`), its first search result
/// if it searched, and its end.
pub async fn time_chat(
    mut answer: BoxStream<'_, net::Result<ChatEvent>>,
    from: Instant,
) -> net::Result<ChatTimes> {
    let mut times = ChatTimes::default();
    let mut split = Sentences::default();
    while let Some(event) = answer.next().await {
        let event = event?;
        if times.first.is_none() && matches!(event, ChatEvent::Text(_) | ChatEvent::ToolCalls(_)) {
            times.first = Some(ms(from));
        }
        match event {
            ChatEvent::Text(piece) => {
                let mut said = split.push(&piece);
                if said.is_empty() {
                    said.extend(split.clause());
                }
                if times.clause.is_none() && !said.is_empty() {
                    times.clause = Some(ms(from));
                }
            }
            ChatEvent::ToolCalls(_) | ChatEvent::Reasoning(_) => {}
            ChatEvent::Searched => {
                times.searched.get_or_insert(ms(from));
            }
            ChatEvent::Done => break,
        }
    }
    if times.clause.is_none() && split.finish().is_some() {
        times.clause = Some(ms(from));
    }
    times.done = ms(from);
    Ok(times)
}

/// The times of a voice saying something: its first audio and its end.
pub async fn time_speech(
    mut speech: BoxStream<'_, net::Result<Vec<i16>>>,
    from: Instant,
) -> net::Result<SpeechTimes> {
    let mut times = SpeechTimes::default();
    let mut samples = 0u64;
    while let Some(packet) = speech.next().await {
        let packet = packet?;
        if times.first.is_none() && !packet.is_empty() {
            times.first = Some(ms(from));
        }
        samples += packet.len() as u64;
    }
    times.done = ms(from);
    times.audio = samples * 1_000 / u64::from(net::TTS_SAMPLE_RATE);
    Ok(times)
}

/// The `p`th percentile of `values` (nearest rank); `None` when empty.
pub fn percentile(values: &[u64], p: u64) -> Option<u64> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = (p * sorted.len() as u64).div_ceil(100).max(1);
    sorted.get(usize::try_from(rank).ok()? - 1).copied()
}

/// DeepSeek's `/models` as the probe reads it: each model's id and whether
/// it takes a picture (`input_modalities`; a model its pricing page says
/// takes none, [`DEEPSEEK_TEXT_ONLY`], never does), at most [`MOST_MODELS`].
pub fn deepseek_models(listed: &Value) -> Vec<(String, bool)> {
    listed["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| {
            let id = model["id"].as_str()?.to_owned();
            if id.len() > 128
                || !id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-._".contains(c))
            {
                return None;
            }
            let images = model["input_modalities"]
                .as_array()
                .is_some_and(|modes| modes.iter().any(|mode| mode == "image"))
                && !DEEPSEEK_TEXT_ONLY.contains(&id.as_str());
            Some((id, images))
        })
        .take(MOST_MODELS)
        .collect()
}

fn chat_stream(
    chat: Arc<Chat>,
    messages: Vec<Message>,
) -> BoxStream<'static, net::Result<ChatEvent>> {
    stream::once(async move { chat.send(&messages).await })
        .map(|answer| match answer {
            Ok(answer) => stream::try_unfold(answer, |mut answer| async move {
                Ok(answer.next().await?.map(|event| (event, answer)))
            })
            .boxed(),
            Err(error) => stream::iter([Err(error)]).boxed(),
        })
        .flatten()
        .boxed()
}

enum Voice {
    Configured(Box<Tts>),
    Gemini(GeminiTts),
}

impl Voice {
    fn speak(self: Arc<Self>, text: &'static str) -> BoxStream<'static, net::Result<Vec<i16>>> {
        stream::once(async move {
            match &*self {
                Voice::Configured(tts) => match tts.speak(text).await {
                    Ok(speech) => stream::try_unfold(speech, |mut speech| async move {
                        Ok(speech.next().await?.map(|packet| (packet, speech)))
                    })
                    .boxed(),
                    Err(error) => stream::iter([Err(error)]).boxed(),
                },
                Voice::Gemini(tts) => match tts.speak(text).await {
                    Ok(speech) => stream::try_unfold(speech, |mut speech| async move {
                        Ok(speech.next().await?.map(|packet| (packet, speech)))
                    })
                    .boxed(),
                    Err(error) => stream::iter([Err(error)]).boxed(),
                },
            }
        })
        .flatten()
        .boxed()
    }
}

/// One candidate's times over the runs, for the summary.
#[derive(Default)]
struct Kept {
    first: Vec<u64>,
    clause: Vec<u64>,
    searched: Vec<u64>,
    done: Vec<u64>,
    failed: u32,
}

impl Kept {
    fn summary(&self, name: &str) -> String {
        let show =
            |values: &[u64], p| percentile(values, p).map_or("none".into(), |v| v.to_string());
        let mut line = format!(
            "{name} runs={} failed={} first_p50_ms={} first_p90_ms={} clause_p50_ms={} done_p50_ms={}",
            self.done.len() as u32 + self.failed,
            self.failed,
            show(&self.first, 50),
            show(&self.first, 90),
            show(&self.clause, 50),
            show(&self.done, 50),
        );
        if !self.searched.is_empty() {
            line += &format!(
                " searched_p50_ms={} searched_p90_ms={}",
                show(&self.searched, 50),
                show(&self.searched, 90)
            );
        }
        line
    }
}

fn shown(value: Option<u64>) -> String {
    value.map_or_else(|| "none".to_owned(), |v| v.to_string())
}

/// A chat candidate: its name, the chat, whether it takes a picture, and
/// the relay it reaches if it goes through one.
type Candidate = (String, Arc<Chat>, bool, Option<Relay>);

/// One request a run makes of a chat: its name, the chat, the messages,
/// and the relay it reaches if it goes through one.
type Ask = (String, Arc<Chat>, Vec<Message>, Option<Relay>);

/// The address an arm through the relay reaches, as the voice would.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relay {
    /// The relay's near address for the service (`LAMP_RELAY_ROUTES=on`).
    Near(String),
    /// The service's normal address: `second.json`'s.
    Normal(String),
    /// The address the unit's environment gave (`LAMP_SECOND_*_BASE_URL`):
    /// never routed.
    Pinned(String),
}

impl Relay {
    /// Where the arm's requests go.
    pub fn url(&self) -> &str {
        match self {
            Self::Near(url) | Self::Normal(url) | Self::Pinned(url) => url,
        }
    }

    /// The end of a run's line: which relay and its host, never a path or
    /// a key ([`crate::second_config::host`]).
    pub fn told(&self) -> String {
        let which = match self {
            Self::Near(_) => "near",
            Self::Normal(_) => "normal",
            Self::Pinned(_) => "pinned",
        };
        format!(
            " relay={which} host={}",
            crate::second_config::host(self.url())
        )
    }

    /// The run line's ending for an arm that may have a relay.
    fn shown(relay: Option<&Self>) -> String {
        relay.map(Self::told).unwrap_or_default()
    }
}

/// Whether the unit's environment asks for the relay's near addresses, as
/// the second worker reads it: `on`, and nothing else.
pub fn routes_on() -> bool {
    std::env::var(super::services::RELAY_ROUTES_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on")
}

/// The relay's near addresses, when `on`: asked once where the voice asks
/// them ([`routes::url`]), with the route's key, and read as it reads them
/// ([`routes::read`]); off, or not read, none. Says which services are
/// near, never where.
pub async fn relay_routes(config: &SecondConfig, on: bool) -> Routes {
    if !on {
        println!("relay routes: off (LAMP_RELAY_ROUTES)");
        return Routes::default();
    }
    let url = routes::url(&config.llm_base_url);
    let authorization = format!("Bearer {}", config.llm_key.expose());
    match routes::read(net::get_json(&url, &authorization).await) {
        routes::Read::Routes {
            routes, refused, ..
        } => {
            for service in refused {
                println!(
                    "relay routes: the {service} route is not an address to use; {service} stays at its normal one"
                );
            }
            println!("relay {} (LAMP_RELAY_ROUTES)", routes.told());
            routes
        }
        routes::Read::Keep(why) => {
            println!("relay routes: not read ({why}); every arm at its normal address");
            Routes::default()
        }
    }
}

/// [`relay_routes`] for the route at `second_config`, with the unit's
/// environment over the file, for `--wire`, whose requests are made by hand.
pub async fn relay_routes_at(second_config: &Path) -> io::Result<Routes> {
    let config = crate::second_config::load_from_path(second_config)?
        .with_environment(|name| std::env::var_os(name))?;
    Ok(relay_routes(&config, routes_on()).await)
}

/// A service's relay: the near address `near` when the voice may route it
/// (`routable`, never a pinned one), else the address the unit pinned, else
/// its normal one.
fn relay_for(normal: &str, pinned: bool, routable: bool, near: Option<&String>) -> Relay {
    match near.filter(|_| routable) {
        Some(url) => Relay::Near(url.clone()),
        None if pinned => Relay::Pinned(normal.to_owned()),
        None => Relay::Normal(normal.to_owned()),
    }
}

/// DeepSeek through the relay (`deepseek_base_url`), as the route's
/// `LAMP_SECOND_CHAT=deepseek` reaches it; `None` without one.
pub fn deepseek_relay(config: &SecondConfig, routes: &Routes) -> Option<Relay> {
    let base = config.hosted.deepseek_base_url.as_deref()?;
    let route = RouteFlags {
        chat: Some(ChatRoute::DeepSeek),
        ..RouteFlags::default()
    };
    let routable = relay_routable(config, route, None).chat;
    Some(relay_for(
        base,
        config.pinned.chat,
        routable,
        routes.chat.as_ref(),
    ))
}

/// The configured voice's relay (`tts_base_url`), as the route reaches it
/// when it is the lamp's one voice.
pub fn speech_relay(config: &SecondConfig, routes: &Routes) -> Relay {
    let routable = relay_routable(config, RouteFlags::default(), None).speech;
    relay_for(
        &config.tts_base_url,
        config.pinned.speech,
        routable,
        routes.speech.as_ref(),
    )
}

/// The route's own Gemini's relay (`gemini_base_url`) with `key`, as
/// `LAMP_SECOND_CHAT=gemini` or `LAMP_SECOND_SPEECH=gemini` reaches it.
pub fn gemini_relay(config: &SecondConfig, routes: &Routes, key: &Key) -> Relay {
    let route = RouteFlags {
        chat: Some(ChatRoute::Gemini),
        ..RouteFlags::default()
    };
    let routable = relay_routable(config, route, Some(key)).gemini;
    relay_for(
        &config.gemini.base_url,
        config.pinned.gemini,
        routable,
        routes.gemini.as_ref(),
    )
}

/// DeepSeek through the relay, with the relay's credential, thinking off,
/// at the address the voice would reach (#1934, #2882); `None` without a
/// `deepseek_base_url`.
fn deepseek_relay_arm(
    config: &SecondConfig,
    routes: &Routes,
    none: net::Declare,
) -> io::Result<Option<Candidate>> {
    let Some(relay) = deepseek_relay(config, routes) else {
        return Ok(None);
    };
    let model = &config.hosted.deepseek_model;
    let chat = Chat::deepseek_at(relay.url(), model, config.llm_key.expose(), none, false)
        .map_err(io::Error::other)?;
    Ok(Some((
        format!("chat=deepseek-relay({model},thinking-off)"),
        Arc::new(chat),
        !DEEPSEEK_TEXT_ONLY.contains(&model.as_str()),
        Some(relay),
    )))
}

/// The probe, as `lamp-voice probe-cascade` runs it.
pub async fn run(
    provider_config: &Path,
    second_config: &Path,
    runs: u32,
    look: Option<&Path>,
) -> io::Result<()> {
    let provider = crate::config::ProviderConfig::load(provider_config)?;
    let character = provider.instruction(false)?;
    // The unit's environment over the file, as the worker reads it: a
    // drop-in's LAMP_SECOND_DEEPSEEK_BASE_URL is then probed too (#1934).
    let config = crate::second_config::load_from_path(second_config)?
        .with_environment(|name| std::env::var_os(name))?;
    // The relay's near addresses, as the voice unit's environment asks.
    let routes = relay_routes(&config, routes_on()).await;
    let picture = look
        .map(|path| {
            let picture = std::fs::read(path)?;
            if picture.len() as u64 > MOST_JPEG || !picture.starts_with(&[0xff, 0xd8]) {
                return Err(io::Error::other(
                    "probe-cascade: --look takes a JPEG of at most 184 KiB",
                ));
            }
            Ok(picture)
        })
        .transpose()?;
    let none: net::Declare = Arc::new(Vec::new);
    let credentials = std::env::var_os(CREDENTIALS_ENV);
    let left_out = |what: &str, error: &io::Error| {
        println!("{what}: left out: {error}");
    };
    // The configured chat: never routed.
    let mut chats: Vec<Candidate> = vec![(
        "chat=configured".into(),
        Arc::new(Chat::new(&config, none.clone()).map_err(io::Error::other)?),
        true,
        Some(Relay::Normal(config.llm_base_url.clone())),
    )];
    let speech = speech_relay(&config, &routes);
    let mut at = config.clone();
    at.tts_base_url = speech.url().to_owned();
    let mut voices: Vec<(String, Arc<Voice>, Option<Relay>)> = vec![(
        format!("speech={}", config.tts_provider.name().replace(' ', "_")),
        Arc::new(Voice::Configured(Box::new(
            Tts::new(&at).map_err(io::Error::other)?,
        ))),
        Some(speech),
    )];
    match super::worker::gemini_key(&provider, credentials.as_deref()) {
        Ok(key) => {
            let gemini = gemini_relay(&config, &routes, &key);
            chats.push((
                format!("chat=gemini({})", config.gemini.chat_model),
                Arc::new(
                    Chat::gemini(
                        gemini.url(),
                        &config.gemini.chat_model,
                        key.expose(),
                        none.clone(),
                    )
                    .map_err(io::Error::other)?,
                ),
                true,
                Some(gemini.clone()),
            ));
            voices.push((
                format!(
                    "speech=gemini({},{})",
                    config.gemini.speech_model,
                    provider.voice()
                ),
                Arc::new(Voice::Gemini(
                    GeminiTts::new(
                        gemini.url(),
                        &config.gemini.speech_model,
                        key.expose(),
                        provider.voice(),
                    )
                    .map_err(io::Error::other)?,
                )),
                Some(gemini),
            ));
        }
        Err(error) => left_out("gemini", &error),
    }
    // OpenRouter: the owner's two candidates, and the configured one.
    let mut searching: Vec<Candidate> = Vec::new();
    match unit_credential(credentials.as_deref(), OPENROUTER_CREDENTIAL) {
        Ok(key) => {
            let mut models = vec![
                OPENROUTER_MODEL.to_owned(),
                OPENROUTER_SECOND_MODEL.to_owned(),
            ];
            if !models.contains(&config.hosted.openrouter_model) {
                models.push(config.hosted.openrouter_model.clone());
            }
            for model in &models {
                let effort = if *model == config.hosted.openrouter_model {
                    config.hosted.openrouter_effort.as_str()
                } else {
                    least_effort(model)
                };
                let chat = |search| {
                    Chat::openrouter(model, effort, key.expose(), none.clone(), search)
                        .map(Arc::new)
                        .map_err(io::Error::other)
                };
                chats.push((
                    format!("chat=openrouter({model},{effort})"),
                    chat(false)?,
                    true,
                    None,
                ));
                if searching.is_empty() {
                    searching.push((
                        format!("search=openrouter({model})"),
                        chat(true)?,
                        true,
                        None,
                    ));
                }
            }
        }
        Err(error) => left_out("openrouter", &error),
    }
    // DeepSeek's own API: each model it lists, thinking off and on.
    let mut deepseek_search = None;
    match unit_credential(credentials.as_deref(), DEEPSEEK_CREDENTIAL) {
        Ok(key) => {
            let url = format!("{}/models", crate::second_config::DEEPSEEK_BASE_URL);
            let models = match net::get_json(&url, &format!("Bearer {}", key.expose())).await {
                Ok(listed) => deepseek_models(&listed),
                Err(error) => {
                    println!(
                        "deepseek: its model list failed ({error}); the two known ones are probed"
                    );
                    vec![
                        ("deepseek-flash".into(), true),
                        ("deepseek-v4-pro".into(), false),
                    ]
                }
            };
            println!(
                "deepseek: models {}",
                models
                    .iter()
                    .map(|(id, _)| id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            for (model, images) in &models {
                for thinking in [false, true] {
                    chats.push((
                        format!(
                            "chat=deepseek({model},thinking-{})",
                            if thinking { "on" } else { "off" }
                        ),
                        Arc::new(
                            Chat::deepseek(model, key.expose(), none.clone(), thinking)
                                .map_err(io::Error::other)?,
                        ),
                        *images,
                        None,
                    ));
                }
            }
            deepseek_search = Some(Search::deepseek(key.expose()).map_err(io::Error::other)?);
        }
        Err(error) => left_out("deepseek", &error),
    }
    // DeepSeek through the relay, with the relay's credential, as the
    // route asks it with `deepseek_base_url` (#1934), at the address the
    // voice would reach (#2882): its first token beside the direct one's,
    // in the same runs.
    chats.extend(deepseek_relay_arm(&config, &routes, none.clone())?);
    let mut asks: Vec<Ask> = Vec::new();
    for (name, chat, sees, relay) in &chats {
        asks.push((
            name.clone(),
            chat.clone(),
            vec![Message::system(character.clone()), Message::user(QUESTION)],
            relay.clone(),
        ));
        if let Some(jpeg) = picture.as_ref().filter(|_| *sees) {
            asks.push((
                format!("look:{name}"),
                chat.clone(),
                vec![
                    Message::system(character.clone()),
                    Message::user_seeing(LOOK_QUESTION, jpeg),
                ],
                relay.clone(),
            ));
        }
    }
    for (name, chat, _, relay) in &searching {
        asks.push((
            name.clone(),
            chat.clone(),
            vec![
                Message::system(character.clone()),
                Message::user(SEARCH_QUESTION),
            ],
            relay.clone(),
        ));
    }
    let mut chat_kept: Vec<Kept> = asks.iter().map(|_| Kept::default()).collect();
    let mut voice_kept: Vec<Kept> = voices.iter().map(|_| Kept::default()).collect();
    let mut search_kept = Kept::default();
    for run in 1..=runs {
        for ((name, chat, messages, relay), kept) in asks.iter().zip(&mut chat_kept) {
            let relay = Relay::shown(relay.as_ref());
            let from = Instant::now();
            match time_chat(chat_stream(chat.clone(), messages.clone()), from).await {
                Ok(times) => {
                    let searched = times
                        .searched
                        .map(|at| format!(" searched_ms={at}"))
                        .unwrap_or_default();
                    println!(
                        "{name} run={run} first_ms={} clause_ms={} done_ms={}{searched}{relay}",
                        shown(times.first),
                        shown(times.clause),
                        times.done
                    );
                    kept.first.extend(times.first);
                    kept.clause.extend(times.clause);
                    kept.searched.extend(times.searched);
                    kept.done.push(times.done);
                }
                Err(error) => {
                    println!(
                        "{name} run={run} failed after {} ms: {error}{relay}",
                        ms(from)
                    );
                    kept.failed += 1;
                }
            }
        }
        if let Some(search) = &deepseek_search {
            let from = Instant::now();
            let (url, header, body) = search.search_request(SEARCH_QUERY);
            match search.ask(&url, header, &body).await {
                Ok(answer) => {
                    let found = answer["results"].as_array().map_or(0, Vec::len);
                    println!(
                        "search=deepseek run={run} first_result_ms={} results={found}",
                        ms(from)
                    );
                    search_kept.first.push(ms(from));
                    search_kept.done.push(ms(from));
                }
                Err(error) => {
                    println!(
                        "search=deepseek run={run} failed after {} ms: {error}",
                        ms(from)
                    );
                    search_kept.failed += 1;
                }
            }
        }
        for ((name, voice, relay), kept) in voices.iter().zip(&mut voice_kept) {
            let relay = Relay::shown(relay.as_ref());
            let from = Instant::now();
            match time_speech(voice.clone().speak(SENTENCE), from).await {
                Ok(times) => {
                    println!(
                        "{name} run={run} first_audio_ms={} done_ms={} audio_ms={}{relay}",
                        shown(times.first),
                        times.done,
                        times.audio
                    );
                    kept.first.extend(times.first);
                    kept.done.push(times.done);
                }
                Err(error) => {
                    println!(
                        "{name} run={run} failed after {} ms: {error}{relay}",
                        ms(from)
                    );
                    kept.failed += 1;
                }
            }
        }
    }
    for ((name, ..), kept) in asks.iter().zip(&chat_kept) {
        println!("{}", kept.summary(name));
    }
    if deepseek_search.is_some() {
        println!("{}", search_kept.summary("search=deepseek"));
    }
    for ((name, ..), kept) in voices.iter().zip(&voice_kept) {
        println!("{}", kept.summary(name));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    fn later<T: Send + 'static>(
        items: Vec<(u64, net::Result<T>)>,
    ) -> BoxStream<'static, net::Result<T>> {
        stream::iter(items)
            .then(|(wait, item)| async move {
                tokio::time::sleep(Duration::from_millis(wait)).await;
                item
            })
            .boxed()
    }

    /// The first event, the first clause the route would say, the first
    /// search result, and the end, each from the request's start.
    #[tokio::test(start_paused = true)]
    async fn a_chat_is_timed_to_its_first_event_first_clause_search_and_end() {
        let answer = later(vec![
            (300, Ok(ChatEvent::Searched)),
            (500, Ok(ChatEvent::Text("Curl up".into()))),
            (100, Ok(ChatEvent::Text(" with a good book. Or".into()))),
            (300, Ok(ChatEvent::Text(" bake something.".into()))),
            (50, Ok(ChatEvent::Done)),
        ]);
        let times = time_chat(answer, Instant::now()).await.unwrap();
        assert_eq!(
            times,
            ChatTimes {
                first: Some(800),
                clause: Some(900),
                done: 1_250,
                searched: Some(300),
            }
        );
        // A call first: an event, no clause; one short answer is a clause
        // at its end; reasoning is not an event.
        let answer = later(vec![
            (100, Ok(ChatEvent::Reasoning(json!([])))),
            (500, Ok(ChatEvent::ToolCalls(Vec::new()))),
            (20, Ok(ChatEvent::Text("Done".into()))),
            (10, Ok(ChatEvent::Done)),
        ]);
        let times = time_chat(answer, Instant::now()).await.unwrap();
        assert_eq!(
            times,
            ChatTimes {
                first: Some(600),
                clause: Some(630),
                done: 630,
                searched: None,
            }
        );
        let failed = later::<ChatEvent>(vec![(5, Err(net::Error::Status(429)))]);
        assert_eq!(
            time_chat(failed, Instant::now()).await.unwrap_err(),
            net::Error::Status(429)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn speech_is_timed_to_its_first_audio_and_its_length_counted() {
        let speech = later(vec![
            (450, Ok(vec![1i16; 960])),
            (40, Ok(vec![1i16; 960])),
            (40, Ok(vec![1i16; 480])),
        ]);
        assert_eq!(
            time_speech(speech, Instant::now()).await.unwrap(),
            SpeechTimes {
                first: Some(450),
                done: 530,
                audio: 100
            }
        );
    }

    #[test]
    fn percentiles_are_the_nearest_rank() {
        let values = [900, 100, 500, 300, 700, 200, 400, 600, 800, 1_000];
        assert_eq!(percentile(&values, 50), Some(500));
        assert_eq!(percentile(&values, 90), Some(900));
        assert_eq!(percentile(&[42], 90), Some(42));
        assert_eq!(percentile(&[], 50), None);
    }

    use crate::second_provider::services::tests::{
        BASE, CHAT, ROUTES, answered, config_at, json_answer, stub,
    };

    /// The route at `relay`, DeepSeek through it at its normal address.
    fn through(name: &str, key: &str, relay: &str) -> crate::second_config::SecondConfig {
        let mut config = config_at(name, key, relay, relay);
        config.hosted.deepseek_base_url = Some(format!("{relay}{BASE}/deepseek"));
        config
    }

    /// The relay's DeepSeek arm asked once, as a run asks it.
    async fn asked(arm: &Candidate) -> ChatTimes {
        let messages = vec![Message::system("character"), Message::user(QUESTION)];
        time_chat(chat_stream(arm.1.clone(), messages), Instant::now())
            .await
            .unwrap()
    }

    /// The first builder on the Queue (#3126): with `LAMP_RELAY_ROUTES=on`
    /// the probe reads the relay's routes as the voice does, and its DeepSeek
    /// arm reaches the near address they name, saying so on each run's line;
    /// so does the configured voice.
    #[tokio::test]
    async fn a_probe_with_routes_on_reaches_the_near_address() {
        let near = stub("k-probe-near", vec![(CHAT, answered("near"))]).await;
        let routes = json!({"status": 1, "data": {
            "chat": format!("{}{BASE}/deepseek", near.url),
            "speech": format!("{}{BASE}", near.url),
            "listening": null, "gemini": null, "ttl_seconds": 600}});
        let relay = stub(
            "k-probe-near",
            vec![(ROUTES, json_answer(routes)), (CHAT, answered("relay"))],
        )
        .await;
        let config = through("probe-near", "k-probe-near", &relay.url);
        let routes = relay_routes(&config, true).await;
        assert_eq!(relay.seen(), ["GET /relay/routes HTTP/1.1"]);
        let arm = deepseek_relay_arm(&config, &routes, Arc::new(Vec::new))
            .unwrap()
            .unwrap();
        assert_eq!(
            arm.3,
            Some(Relay::Near(format!("{}{BASE}/deepseek", near.url)))
        );
        assert!(asked(&arm).await.first.is_some());
        assert_eq!(
            near.seen(),
            ["POST /api/v1/ai/v1/deepseek/chat/completions HTTP/1.1"]
        );
        assert_eq!(relay.asked(CHAT), 0);
        let told = arm.3.as_ref().unwrap().told();
        assert_eq!(told, format!(" relay=near host={}", &near.url[7..]));
        assert_eq!(
            speech_relay(&config, &routes),
            Relay::Near(format!("{}{BASE}", near.url))
        );
    }

    /// With the routes off the probe never asks for them, and its arms
    /// reach the normal address; an address the unit's environment gave is
    /// never routed, whatever the routes say.
    #[tokio::test]
    async fn a_probe_with_routes_off_reaches_the_normal_address() {
        let near = stub("k-probe-normal", vec![(CHAT, answered("near"))]).await;
        let routes = json!({"status": 1, "data": {
            "chat": format!("{}{BASE}/deepseek", near.url),
            "speech": format!("{}{BASE}", near.url), "ttl_seconds": 600}});
        let relay = stub(
            "k-probe-normal",
            vec![(ROUTES, json_answer(routes)), (CHAT, answered("relay"))],
        )
        .await;
        let config = through("probe-normal", "k-probe-normal", &relay.url);
        let off = relay_routes(&config, false).await;
        assert_eq!(off, Routes::default());
        assert_eq!(relay.asked(ROUTES), 0);
        let arm = deepseek_relay_arm(&config, &off, Arc::new(Vec::new))
            .unwrap()
            .unwrap();
        let normal = format!("{}{BASE}/deepseek", relay.url);
        assert_eq!(arm.3, Some(Relay::Normal(normal.clone())));
        assert!(asked(&arm).await.first.is_some());
        assert_eq!(relay.asked(CHAT), 1);
        assert_eq!(
            arm.3.as_ref().unwrap().told(),
            format!(" relay=normal host={}", &relay.url[7..])
        );
        assert_eq!(
            speech_relay(&config, &off),
            Relay::Normal(relay.url.clone())
        );
        // Pinned by the unit: the routes on, its own address all the same.
        let mut pinned = config.clone();
        pinned.pinned.chat = true;
        pinned.pinned.speech = true;
        let on = relay_routes(&pinned, true).await;
        assert!(on.chat.is_some());
        let arm = deepseek_relay_arm(&pinned, &on, Arc::new(Vec::new))
            .unwrap()
            .unwrap();
        assert_eq!(arm.3, Some(Relay::Pinned(normal)));
        assert!(asked(&arm).await.first.is_some());
        assert_eq!(relay.asked(CHAT), 2);
        assert!(near.seen().is_empty(), "{:?}", near.seen());
        assert_eq!(speech_relay(&pinned, &on), Relay::Pinned(relay.url.clone()));
        assert!(arm.3.unwrap().told().starts_with(" relay=pinned host="));
    }

    /// DeepSeek's list as its docs show it: each model, and whether it
    /// takes a picture; `deepseek-v4-pro` never does, whatever it says.
    #[test]
    fn deepseeks_models_are_read_with_whether_each_takes_a_picture() {
        let listed = json!({"object": "list", "data": [
            {"id": "deepseek-flash", "object": "model", "owned_by": "deepseek", "input_modalities": ["text", "image"]},
            {"id": "deepseek-v4-pro", "object": "model", "owned_by": "deepseek", "input_modalities": ["text", "image"]},
            {"id": "odd name", "object": "model", "owned_by": "x"},
            {"id": "deepseek-text", "object": "model", "owned_by": "deepseek"},
        ]});
        assert_eq!(
            deepseek_models(&listed),
            [
                ("deepseek-flash".to_owned(), true),
                ("deepseek-v4-pro".to_owned(), false),
                ("deepseek-text".to_owned(), false),
            ]
        );
        assert!(deepseek_models(&json!({})).is_empty());
    }
}
