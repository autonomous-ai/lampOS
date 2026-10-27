//! Streamed speech as 24 kHz mono 16-bit PCM from OpenAI or ElevenLabs, as
//! V1's `hal/drivers/voice/tts/{openai,elevenlabs}.py` ask for it.
use super::{
    CONNECT, Error, Result, STALL, chunk, http_client, keeps_its_connection, local_socket,
    openai_base, post_guarded, warm_get,
};
use crate::second_config::{SecondConfig, TtsProvider};
use crate::second_provider::services::{ChatPermission, permitted_result, permitted_until};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
    tungstenite::{Message as Frame, client::IntoClientRequest, http::HeaderValue},
};

pub const TTS_SAMPLE_RATE: u32 = 24_000;
/// 40 ms: the most samples one packet carries.
pub const PACKET_SAMPLES: usize = 960;

const OPENAI_MODEL: &str = "tts-1";
/// V1's `TTS_VOICE`, used when `tts_voice` is unset.
const OPENAI_VOICE: &str = "nova";
/// The quickest to its first sound. Through the company relay from
/// lamp-52e6 on 2026-10-15, two sentences each: `eleven_flash_v2_5` 0.60
/// and 0.68 s to the first audio, `eleven_v4_turbo` (V1's) 0.81 and 1.30 s.
const ELEVENLABS_MODEL: &str = "eleven_flash_v2_5";

/// The model a provider is asked for when none is set (`tts_model`,
/// `LAMP_SECOND_SPEECH_MODEL`).
pub fn default_model(provider: TtsProvider) -> &'static str {
    match provider {
        TtsProvider::OpenAi => OPENAI_MODEL,
        TtsProvider::ElevenLabs => ELEVENLABS_MODEL,
    }
}
/// The voice a provider speaks in for `voice` (`tts_voice`): the one set,
/// or, unset, the provider's default ([`request`]'s), by name. For a
/// recording made in the lamp's voice to name it (`render_line_second`).
pub fn voice_name(provider: TtsProvider, voice: Option<&str>) -> &str {
    voice.unwrap_or(match provider {
        TtsProvider::OpenAi => OPENAI_VOICE,
        TtsProvider::ElevenLabs => ELEVENLABS_VOICES[0].0,
    })
}
const ELEVENLABS_HOSTS: &[&str] = &[
    "api.elevenlabs.io",
    "api.us.elevenlabs.io",
    "api.eu.elevenlabs.io",
];
/// V1's curated ElevenLabs voices by name (`VOICE_IDS_BY_LANG`), so a
/// V1 `tts_voice` works here; any other value is taken as a voice id. The
/// first is the default, as `DefaultElevenLabsVoiceForLang` gives in English.
const ELEVENLABS_VOICES: &[(&str, &str)] = &[
    ("Rachel", "21m00Tcm4TlvDq8ikWAM"),
    ("Sarah", "EXAVITQu4vr4xnSDxMaL"),
    ("Nicole", "piTKgcLEGmPE4e6mEKli"),
    ("Terra", "aFueGIISJUmscc05ZNfD"),
    ("Maria", "vZzlAds9NzvLsFSWp0qk"),
    ("Sophie", "AEW6JTgnyoPaoB9zlK3S"),
    ("Piper", "rzgrf9VyEb0LLa824k8Q"),
    ("Mia", "052jzHJceQiZr7ltnY0C"),
    ("Kimmy", "TmK7x2BFDD7TOVlR69J2"),
    ("Brianna", "2NzqTfQARqdn4tcBKTSh"),
    ("Ally", "qmm0vRXCIew16ilYAeiI"),
    ("Tori", "lAxf5ma5HGtzxC434SWT"),
    ("Brian", "nPczCjzI2devNBz1zQrb"),
    ("Adam", "pNInz6obpgDQGcFmaJgB"),
    ("Daniel", "onwK4e9ZLuTAKqWW03F9"),
    ("George", "JBFqnCBsd6RMkjVDRZzb"),
    ("James", "ZQe5CZNOzWyzPSCn5a3c"),
    ("Liam", "TX3LPaxmHKxFdv7VOQHJ"),
    ("Charlie", "IKne3meq5aSn9XLyUdCD"),
    ("Sam", "yoZ06aMxZJJ28mfd3POQ"),
    ("Sean", "FgARTjeugpFkVodK0Ovq"),
    ("Kael", "RxsTyZQJnPygpas5IyzL"),
    ("Brooks", "sUzXYdokj3o9QQ91yPRF"),
    ("Erion", "BSgaLWMIhbNhOCIH1apf"),
    ("Ngan", "a3AkyqGG4v8Pg7SWQ0Y3"),
    ("Linh", "L5c6tGA8OiORYKxez5Zu"),
    ("Huyen", "foH7s9fX31wFFH2yqrFa"),
    ("Freya", "rXOGzMiqbmjugMpzKMEx"),
    ("Nathan", "u8EWWYyBDfXFxHak7WM3"),
    ("Quan", "puBBfOSRT9Dbk3FUJQGd"),
    ("Shizuka", "WQz3clzUdMqvBf0jswZQ"),
    ("Konoha", "T7yYq3WpB94yAuOXraRi"),
    ("Rin", "NxfO5zydfqwpYnWQJ7jJ"),
    ("Asahi", "GKDaBI8TKSBJVhsCLD6n"),
    ("Hinata", "j210dv0vWm7fCknyQpbA"),
    ("Hiroki", "vzIXwvf41vKosKu00hYj"),
    ("Amy", "bhJUNIXWQQ94l8eI2VUf"),
    ("Sage", "APSIkVZudNbPAwyPoeVO"),
    ("Xiaoxi", "9DMBSOAnMDPiFAsz1ZGK"),
    ("Yun", "YxbjaPemDJV2xlfvkiIG"),
    ("Evan Zhao", "MI36FIkp9wRP7cpWKPTl"),
    ("Jin", "vZZLclMx4wouUtKBRfZn"),
];

/// One request's URL, key header and body.
#[derive(Debug, PartialEq)]
pub(crate) struct Request {
    pub url: String,
    pub header: &'static str,
    pub value: String,
    pub body: Value,
}

/// `None` when nothing in `text` would be spoken: V1 sends no request then.
pub(crate) fn request(
    provider: TtsProvider,
    base_url: &str,
    key: &str,
    voice: Option<&str>,
    model: Option<&str>,
    text: &str,
) -> Result<Option<Request>> {
    let model = model.unwrap_or(default_model(provider));
    let base = service_base(provider, base_url)?;
    let spoken = without_tags(text);
    if spoken.trim().is_empty() {
        return Ok(None);
    }
    let (header, value) = auth(provider, key);
    let request = match provider {
        // ponytail: every `[...]` is dropped, where V1 drops only its list
        // of ElevenLabs emotion tags; narrow it if bracketed words are lost.
        TtsProvider::OpenAi => Request {
            url: format!("{base}/audio/speech"),
            header,
            value,
            body: json!({
                "model": model,
                "input": spoken.trim(),
                "voice": voice_name(provider, voice),
                "response_format": "pcm",
            }),
        },
        TtsProvider::ElevenLabs => {
            let id = elevenlabs_voice(voice)?;
            Request {
                url: format!("{base}/text-to-speech/{id}/stream?output_format=pcm_24000"),
                header,
                value,
                // Speed 1.0 is given because, absent, the voice's stored
                // speed applies (V1).
                body: json!({
                    "text": text,
                    "model_id": model,
                    "voice_settings": {"speed": 1.0},
                }),
            }
        }
    };
    Ok(Some(request))
}

/// The ElevenLabs voice id for `voice`: one of V1's names, or an id as
/// given; the first of V1's when unset.
fn elevenlabs_voice(voice: Option<&str>) -> Result<&str> {
    let voice = voice.unwrap_or(ELEVENLABS_VOICES[0].0);
    let id = ELEVENLABS_VOICES
        .iter()
        .find(|(name, _)| *name == voice)
        .map_or(voice, |(_, id)| id);
    if !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(Error::Url);
    }
    Ok(id)
}

/// A speech socket's request: ElevenLabs' `stream-input`, where text goes in
/// as it comes and audio comes back as it is made, so the socket can be
/// opened before the answer's first piece exists (#2135's row; the first
/// builder on #1722, 20 Oct 10:37). Through the Autonomous proxy it is under
/// `/ws/`, with the key as a bearer (the backend on #1934, 10:05); on
/// ElevenLabs' own API, with `xi-api-key`. Its URL, key header and value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SocketRequest {
    pub url: String,
    pub header: &'static str,
    pub value: String,
    /// Its endpoint, and so the shape of its messages ([`socket_shape`]).
    pub shape: SocketShape,
    /// The voice's id, which a dialogue socket names in its messages.
    pub voice: String,
}

/// Which of ElevenLabs' socket endpoints a speech socket is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SocketShape {
    /// `text-to-speech/{voice}/stream-input`: text in as it comes; opened
    /// ahead, as the chat is asked.
    StreamInput,
    /// `text-to-dialogue/stream-input`, the one v4 is served on (#2602,
    /// #2135): `stream-input` closes on v4 with 4400 `unsupported_model`.
    /// Each open one holds one of the account's concurrent dialogue
    /// sessions, idle too (105, then `too_many_concurrent_requests`), and
    /// it is closed after 20 s without a message from the lamp, so it is
    /// opened only when its piece is ready, never ahead, and told
    /// `close_socket` with that piece.
    Dialogue,
}

/// The model the dialogue endpoint is used for: v4 Turbo, which takes one
/// voice per socket (ElevenLabs' "Stream dialogue in real time" guide).
pub(crate) const DIALOGUE_MODEL: &str = "eleven_v4_turbo";

/// The endpoint a speech socket in `model` is opened on: the dialogue one
/// for [`DIALOGUE_MODEL`] only; every other model keeps `stream-input`.
pub(crate) fn socket_shape(model: &str) -> SocketShape {
    if model == DIALOGUE_MODEL {
        SocketShape::Dialogue
    } else {
        SocketShape::StreamInput
    }
}

/// How long a speech socket waits for text before ElevenLabs closes it, in
/// seconds: it is opened as the chat is asked, before the answer exists,
/// and an answer whose first piece took past the default 20 s was closed
/// with 1008 `input_timeout_exceeded` (the backend on #2135, 21 Oct 17:4x).
/// ElevenLabs takes up to 180.
pub const SOCKET_INACTIVITY_S: u32 = 60;

pub(crate) fn socket_request(
    base_url: &str,
    key: &str,
    voice: Option<&str>,
    model: Option<&str>,
) -> Result<SocketRequest> {
    let model = model.unwrap_or(ELEVENLABS_MODEL);
    if model.is_empty()
        || !model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(Error::Url);
    }
    let id = elevenlabs_voice(voice)?;
    let base = openai_base(base_url);
    reqwest::Url::parse(&base).map_err(|_| Error::Url)?;
    let (root, header, value) = if on_elevenlabs(&base) {
        (base, "xi-api-key", key.to_owned())
    } else {
        (
            format!("{base}/ws/elevenlabs"),
            "authorization",
            format!("Bearer {key}"),
        )
    };
    let Some(root) = root.strip_prefix("https://") else {
        return Err(Error::Url);
    };
    let shape = socket_shape(model);
    // `inactivity_timeout` only on `stream-input`, which is opened ahead and
    // waits for the answer. ElevenLabs' dialogue reference does not list it
    // (its idle close is a fixed 20 s), and the dialogue socket never waits:
    // it opens when its piece is ready and is told `close_socket` with it.
    let path = match shape {
        SocketShape::StreamInput => format!(
            "text-to-speech/{id}/stream-input?model_id={model}&output_format=pcm_24000&inactivity_timeout={SOCKET_INACTIVITY_S}"
        ),
        SocketShape::Dialogue => {
            format!("text-to-dialogue/stream-input?model_id={model}&output_format=pcm_24000")
        }
    };
    Ok(SocketRequest {
        url: format!("wss://{root}/{path}"),
        header,
        value,
        shape,
        voice: id.to_owned(),
    })
}

impl SocketRequest {
    /// Its first message: `stream-input`'s voice settings, or the dialogue's
    /// one voice. ElevenLabs' dialogue reference takes only `stability` as a
    /// setting there, so no speed is sent.
    pub(crate) fn opening(&self) -> Value {
        match self.shape {
            SocketShape::StreamInput => socket_opening(),
            SocketShape::Dialogue => json!({"voices": [self.voice]}),
        }
    }

    /// `text` as its only piece, then its end; `None` when nothing in it
    /// would be spoken. `stream-input`: the piece made at once and the empty
    /// text. Dialogue: the piece as an input in the socket's voice, `flush`,
    /// then `close_socket`, after which the service sends the rest of the
    /// audio, `is_final` and closes.
    pub(crate) fn messages(&self, text: &str) -> Option<Vec<Value>> {
        let piece = socket_piece(text)?;
        Some(match self.shape {
            SocketShape::StreamInput => vec![piece, socket_end()],
            // ponytail: one piece per dialogue socket, as one per
            // `stream-input` socket: the voice asks each piece's speech
            // apart and at once, and the dialogue socket marks no piece's
            // end before `close_socket`; the whole answer on one socket
            // needs the answer's end told to the voice, and `keep_alive`
            // while the chat is slow.
            SocketShape::Dialogue => vec![
                json!({"inputs": [{"text": text.trim(), "voice_id": self.voice}]}),
                json!({"flush": true}),
                json!({"close_socket": true}),
            ],
        })
    }
}

/// A speech socket's first message: the voice's settings, with speed 1.0 as
/// a piece over HTTP is given it, and no words yet.
pub(crate) fn socket_opening() -> Value {
    json!({"text": " ", "voice_settings": {"speed": 1.0}})
}

/// `text` as a piece on a speech socket, made into speech at once
/// (`flush`), as it is sent over HTTP; `None` when nothing in it would be
/// spoken.
pub(crate) fn socket_piece(text: &str) -> Option<Value> {
    if without_tags(text).trim().is_empty() {
        return None;
    }
    Some(json!({"text": format!("{} ", text.trim()), "flush": true}))
}

/// A speech socket's last message: no more words, so the rest is made and
/// the socket ends.
pub(crate) fn socket_end() -> Value {
    json!({"text": ""})
}

/// What one of a speech socket's messages said: its audio, 24 kHz 16-bit
/// PCM, and whether it was the last.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct SocketAnswer {
    pub audio: Vec<u8>,
    pub last: bool,
}

/// A speech socket's message read: `None` when it isn't one of its answers
/// (an error the service sent, or something else).
pub(crate) fn socket_answer(text: &str) -> Option<SocketAnswer> {
    use base64::Engine as _;
    let message: Value = serde_json::from_str(text).ok()?;
    let audio = match message.get("audio") {
        Some(Value::String(audio)) => base64::engine::general_purpose::STANDARD
            .decode(audio)
            .ok()?,
        Some(Value::Null) | None => Vec::new(),
        Some(_) => return None,
    };
    // `isFinal` on `stream-input`, `is_final` on the dialogue socket.
    let last = ["isFinal", "is_final"]
        .iter()
        .any(|key| message.get(key).and_then(Value::as_bool) == Some(true));
    // The dialogue socket's end of a turn's audio (`is_final_audio_for_turn`)
    // carries no audio: read past.
    if !last && audio.is_empty() && message.get("is_final_audio_for_turn").is_some() {
        return Some(SocketAnswer::default());
    }
    (last || !audio.is_empty()).then_some(SocketAnswer { audio, last })
}

/// The dialogue socket's refusal when the account's concurrent dialogue
/// sessions are all held (the backend on #2135, 21 Oct: 105 of them).
pub(crate) const TOO_MANY: &str = "too_many_concurrent_requests";

/// How long no socket is opened after [`TOO_MANY`]: the sessions held are
/// others' or ones closing, so a later answer may find one free.
pub const SOCKET_BUSY_PAUSE: Duration = Duration::from_secs(60);

/// What the service said in a speech socket's message that isn't one of its
/// answers, for the journal: ElevenLabs' `error` and `message`, as when it
/// closes an idle socket (`input_timeout_exceeded`), cut to 160 characters.
/// `None` for a message with neither. Never the answer's words: the service
/// sends none in an error.
pub(crate) fn socket_error(text: &str) -> Option<String> {
    let message: Value = serde_json::from_str(text).ok()?;
    let error = message.get("error").and_then(Value::as_str);
    let said = message.get("message").and_then(Value::as_str);
    let told = match (error, said) {
        (Some(error), Some(said)) => format!("{error}: {said}"),
        (Some(one), None) | (None, Some(one)) => one.to_owned(),
        (None, None) => return None,
    };
    Some(told.chars().take(160).collect())
}

/// The provider's API root: OpenAI's `/v1`, or ElevenLabs' (on the
/// Autonomous proxy, its `/elevenlabs` path).
fn service_base(provider: TtsProvider, base_url: &str) -> Result<String> {
    let base = openai_base(base_url);
    if provider == TtsProvider::OpenAi {
        return Ok(base);
    }
    reqwest::Url::parse(&base).map_err(|_| Error::Url)?;
    Ok(if on_elevenlabs(&base) {
        base
    } else {
        format!("{base}/elevenlabs")
    })
}

/// Whether `base_url` is ElevenLabs' own API ([`ELEVENLABS_HOSTS`]), not
/// the Autonomous proxy.
pub(crate) fn on_elevenlabs(base_url: &str) -> bool {
    reqwest::Url::parse(&openai_base(base_url)).is_ok_and(|url| {
        url.host_str()
            .is_some_and(|h| ELEVENLABS_HOSTS.iter().any(|d| h.eq_ignore_ascii_case(d)))
    })
}

/// The provider's key header and its value.
fn auth(provider: TtsProvider, key: &str) -> (&'static str, String) {
    match provider {
        TtsProvider::OpenAi => ("authorization", format!("Bearer {key}")),
        TtsProvider::ElevenLabs => ("xi-api-key", key.to_owned()),
    }
}

pub(super) fn without_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// The configured voice's connection is opened ahead as a line begins
/// when no request went this long ([`Tts::warm`]), as the chat's is.
pub const WARM_AFTER: Duration = super::chat::WARM_AFTER;
/// The most of a warming answer read (a models list), so its connection
/// goes back to the pool; a longer one is dropped with its connection.
const WARM_BYTES: usize = 256 << 10;
/// Local sockets remembered to tell a reused connection from a new one:
/// more than the pieces of one answer in flight at once.
const SOCKETS: usize = 8;

/// The latest connections by local socket, each with whether it is known
/// to be idle in the client's pool: a piece's or warming's answer read to
/// its end gives its connection back (HTTP/1.1, one request on it at a
/// time); one dropped mid-body, as at a cut-in, or failed, takes it with it,
/// and is forgotten.
type Connections = Arc<Mutex<VecDeque<(SocketAddr, bool)>>>;

/// A piece's connection while its answer streams: given back as idle when
/// the answer is read to its end, forgotten when it is dropped before.
struct Lease {
    connections: Connections,
    socket: SocketAddr,
    ended: bool,
}

impl Drop for Lease {
    fn drop(&mut self) {
        let Ok(mut connections) = self.connections.lock() else {
            return;
        };
        let at = connections.iter().position(|(s, _)| *s == self.socket);
        match at {
            Some(at) if self.ended => connections[at].1 = true,
            Some(at) => {
                connections.remove(at);
            }
            None => {}
        }
    }
}

/// The one warming in flight ([`Tts::warm`]): taken, or `None` while
/// another is.
struct Warming<'a>(&'a AtomicBool);

impl<'a> Warming<'a> {
    fn take(flag: &'a AtomicBool) -> Option<Self> {
        (!flag.swap(true, Ordering::SeqCst)).then_some(Self(flag))
    }
}

impl Drop for Warming<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Whose a piece of speech is, for the journal: its request, and when the
/// words it answers were heard.
#[derive(Clone, Copy, Debug)]
pub struct Turn {
    pub request: u64,
    pub heard_at: Instant,
}

/// One turn's pieces in the journal's summary ([`Tts::told`]).
#[derive(Default)]
struct Tally {
    request: u64,
    /// Pieces asked, and of them begun (a first audio byte came).
    asked: u32,
    begun: u32,
    reused: u32,
    chars: usize,
    /// The first piece's first audio byte after it went, and the slowest.
    first_ms: Option<u128>,
    slowest_ms: u128,
}

pub struct Tts {
    client: reqwest::Client,
    provider: TtsProvider,
    base_url: String,
    key: String,
    voice: Option<String>,
    /// The model asked for; absent, the provider's default.
    model: Option<String>,
    /// When a piece or a warming last went.
    used: Mutex<Option<Instant>>,
    /// The local sockets of the latest connections: a response on one of
    /// them came on a connection reused from the pool. Each says whether it
    /// is known idle, for [`Tts::warm`].
    sockets: Connections,
    /// A warming is in flight: at most one is.
    warming: AtomicBool,
    tally: Arc<Mutex<Tally>>,
    /// The speech socket opened ahead for a request
    /// (`LAMP_SECOND_SPEECH_SOCKET=on`, [`Tts::open_ahead`]).
    ahead: Mutex<Option<Ahead>>,
    /// The speech socket's own model (`LAMP_SECOND_SPEECH_SOCKET_MODEL`);
    /// absent, [`ELEVENLABS_MODEL`]: `stream-input` refuses
    /// `eleven_v4_turbo`, which HTTP serves (53 of 53 refused on lamp-4ace
    /// on 21 Oct, the backend on #2135), and which the dialogue endpoint
    /// serves instead ([`SocketShape::Dialogue`]).
    socket_model: Option<String>,
    /// The request whose socket opened: every piece of its answer is said
    /// in the socket's model, those over HTTP too, so one answer keeps one
    /// voice.
    socketed: Mutex<Option<u64>>,
    /// A socket model the service refused (a 44xx close): no socket is
    /// opened ahead for it again until the voice starts again, since a
    /// refused model is refused on every try.
    refused: Mutex<Option<String>>,
    /// Until when no socket is opened: the dialogue sessions were all held
    /// ([`TOO_MANY`], [`SOCKET_BUSY_PAUSE`]).
    paused_until: Mutex<Option<Instant>>,
    #[cfg(test)]
    send_polls: Option<Arc<std::sync::atomic::AtomicUsize>>,
    #[cfg(test)]
    after_socket_attempt: Option<Arc<dyn Fn() + Send + Sync>>,
    #[cfg(test)]
    after_socket_message: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// A speech socket, ElevenLabs' `stream-input` or its dialogue one.
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// The longest a piece waits on the speech socket for its first audio before
/// it goes over HTTP instead: about twice a kept HTTP connection's (420 to
/// 690 ms on lamp-4ace), so a socket open but silent costs the piece at most
/// this, never the stream's 30 s (the reviewer's P3 on #2209).
pub const SOCKET_FIRST_AUDIO_WITHIN: Duration = Duration::from_millis(1_500);

/// The longest the dialogue socket may take to open, once its piece is
/// ready, before the piece goes over HTTP instead. It is opened on the
/// piece's own path (never ahead, [`SocketShape::Dialogue`]), so a slow
/// relay handshake would hold the answer's first sound for up to
/// [`CONNECT`], 10 s; a kept HTTP connection's first audio is 420 to 690
/// ms on lamp-4ace, so the socket gets less than that to be worth it (the
/// reviewer-lead's P2 on #2658). `stream-input`, opened ahead while the
/// chat thinks, keeps [`CONNECT`].
pub const DIALOGUE_OPEN_WITHIN: Duration = Duration::from_millis(400);

/// The speech socket opened ahead of a request's answer (#2135's row; the
/// first builder on #1722, 20 Oct 10:37): opened as its chat is asked, so
/// its first piece goes to a socket already open. A piece that finds it
/// still opening goes over HTTP as before.
enum Ahead {
    /// The dialogue socket for the request, opened by its first piece with
    /// words ([`SocketShape::Dialogue`]): nothing is held until then.
    OnReply(u64, Box<SocketRequest>),
    Opening(u64),
    /// Open for the request since then, told the voice's settings.
    Open(u64, Box<Socket>, Instant),
}

impl Tts {
    pub fn new(config: &SecondConfig) -> Result<Self> {
        Ok(Self {
            model: config.tts_model.clone(),
            socket_model: std::env::var(crate::second_provider::SPEECH_SOCKET_MODEL_ENVIRONMENT)
                .ok()
                .filter(|model| !model.trim().is_empty()),
            ..Self::with(
                config.tts_provider,
                &config.tts_base_url,
                config.tts_key.expose(),
                config.tts_voice.as_deref(),
            )?
        })
    }

    pub(crate) fn with(
        provider: TtsProvider,
        base_url: &str,
        key: &str,
        voice: Option<&str>,
    ) -> Result<Self> {
        Ok(Self {
            client: http_client()?,
            provider,
            base_url: base_url.to_owned(),
            key: key.to_owned(),
            voice: voice.map(str::to_owned),
            model: None,
            used: Mutex::new(None),
            sockets: Arc::new(Mutex::new(VecDeque::with_capacity(SOCKETS))),
            warming: AtomicBool::new(false),
            tally: Arc::default(),
            ahead: Mutex::new(None),
            socket_model: None,
            socketed: Mutex::new(None),
            refused: Mutex::new(None),
            paused_until: Mutex::new(None),
            #[cfg(test)]
            send_polls: None,
            #[cfg(test)]
            after_socket_attempt: None,
            #[cfg(test)]
            after_socket_message: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn counting_send_polls(
        mut self,
        polls: Arc<std::sync::atomic::AtomicUsize>,
    ) -> Self {
        self.send_polls = Some(polls);
        self
    }

    /// Whether a socket is opened for an answer: ElevenLabs only, not for a
    /// model the service refused, and not while the dialogue sessions were
    /// all held lately.
    fn opens_ahead(&self) -> bool {
        self.provider == TtsProvider::ElevenLabs
            && !self
                .refused
                .lock()
                .is_ok_and(|refused| refused.as_deref() == Some(self.socket_model()))
            && !self
                .paused_until
                .lock()
                .is_ok_and(|until| until.is_some_and(|until| Instant::now() < until))
    }

    /// The speech socket's model: the one set, or [`ELEVENLABS_MODEL`].
    fn socket_model(&self) -> &str {
        self.socket_model.as_deref().unwrap_or(ELEVENLABS_MODEL)
    }

    /// The model a piece of `turn`'s answer is said in over HTTP: the
    /// socket's when its socket opened, so the answer keeps one voice;
    /// otherwise the route's.
    fn model_for(&self, turn: Option<Turn>) -> Option<&str> {
        let socketed = turn.is_some_and(|turn| {
            self.socketed
                .lock()
                .is_ok_and(|socketed| *socketed == Some(turn.request))
        });
        if socketed {
            Some(self.socket_model())
        } else {
            self.model.as_deref()
        }
    }

    /// The service refused the socket's model (`error`, a 4400, 4404 or
    /// 4422 close): no socket is opened ahead for it again, said once, and
    /// the rest of `request`'s answer goes in the route's model, since none
    /// of it was heard from the socket and a model the socket refuses may
    /// be one HTTP refuses too (the review of #2629).
    fn refuse(&self, request: u64, error: &Error) {
        if let Ok(mut socketed) = self.socketed.lock()
            && *socketed == Some(request)
        {
            *socketed = None;
        }
        let model = self.socket_model().to_owned();
        let Ok(mut refused) = self.refused.lock() else {
            return;
        };
        if refused.as_deref() != Some(model.as_str()) {
            eprintln!(
                "lamp-voice second: request {request}: the speech socket refused {model} ({error}): no socket is opened ahead for it again until the voice starts again"
            );
            *refused = Some(model);
        }
    }

    /// The dialogue sessions were all held ([`TOO_MANY`]), or the socket's
    /// upgrade was answered 429: no socket is opened for
    /// [`SOCKET_BUSY_PAUSE`], said once. The model isn't refused, and HTTP
    /// serves it, so the answer keeps its voice.
    fn pause(&self, request: u64, error: &Error) {
        let Ok(mut until) = self.paused_until.lock() else {
            return;
        };
        let now = Instant::now();
        if until.is_none_or(|until| until <= now) {
            eprintln!(
                "lamp-voice second: request {request}: the speech socket is too busy ({error}): no socket is opened for {} s",
                SOCKET_BUSY_PAUSE.as_secs()
            );
            *until = Some(now + SOCKET_BUSY_PAUSE);
        }
    }

    /// With `LAMP_SECOND_SPEECH_SOCKET=on`, a speech socket opened now for
    /// `request`'s answer, its voice's settings sent, for the first of its
    /// pieces that finds it open ([`Self::speak_for`]). ElevenLabs only; one
    /// opened before, for an older request, is closed. Nothing on failure:
    /// the pieces go over HTTP as before.
    pub async fn open_ahead(&self, request: u64) {
        if !self.opens_ahead() {
            return;
        }
        let Ok(socket) = socket_request(
            &self.base_url,
            &self.key,
            self.voice.as_deref(),
            Some(self.socket_model()),
        ) else {
            return;
        };
        self.open_ahead_on(request, &socket).await;
    }

    async fn open_ahead_on(&self, request: u64, socket: &SocketRequest) {
        if let Ok(mut ahead) = self.ahead.lock() {
            *ahead = Some(match socket.shape {
                SocketShape::StreamInput => Ahead::Opening(request),
                // Not opened now: it would hold a dialogue session while
                // the chat thinks.
                SocketShape::Dialogue => Ahead::OnReply(request, Box::new(socket.clone())),
            });
        }
        // From the ask, not the open: a piece that goes over HTTP while the
        // socket still opens is said in the socket's model too, so one
        // answer keeps one voice (the review of #2629).
        if let Ok(mut socketed) = self.socketed.lock() {
            *socketed = Some(request);
        }
        if socket.shape == SocketShape::Dialogue {
            return;
        }
        let from = Instant::now();
        let opened = open_socket(socket, CONNECT).await;
        // A 429 at the upgrade pauses the sockets; a 400, 404 or 422 there
        // refuses the model, as its 4400, 4404 or 4422 close does, so the
        // open ahead isn't tried again on every answer (the Reviewer's P3
        // on #2658).
        match &opened {
            Err(error @ Error::Status(429)) => self.pause(request, error),
            Err(error @ Error::Status(400 | 404 | 422)) => self.refuse(request, error),
            _ => {}
        }
        let Ok(mut ahead) = self.ahead.lock() else {
            return;
        };
        // Only if it is still this request's: a newer one's is left alone.
        if !matches!(*ahead, Some(Ahead::Opening(of)) if of == request) {
            return;
        }
        match opened {
            Ok(socket) => {
                eprintln!(
                    "lamp-voice second: request {request}: the speech socket is open, {} ms after it was asked",
                    from.elapsed().as_millis()
                );
                *ahead = Some(Ahead::Open(request, Box::new(socket), Instant::now()));
            }
            Err(error) => {
                eprintln!(
                    "lamp-voice second: request {request}: the speech socket didn't open ({error}); its pieces go over HTTP"
                );
                *ahead = None;
            }
        }
    }

    /// The socket opened ahead for `request`, when it is open: taken, so one
    /// piece speaks on it.
    fn take_ahead(&self, request: u64) -> Option<(Socket, Instant)> {
        let mut ahead = self.ahead.lock().ok()?;
        match ahead.take() {
            Some(Ahead::Open(of, socket, at)) if of == request => Some((*socket, at)),
            other => {
                *ahead = other;
                None
            }
        }
    }

    /// The dialogue socket waiting for `request`'s first piece with words,
    /// taken with `text`'s messages; a piece with none leaves it.
    fn take_on_reply(&self, request: u64, text: &str) -> Option<(SocketRequest, Vec<Value>)> {
        let mut ahead = self.ahead.lock().ok()?;
        let messages = match &*ahead {
            Some(Ahead::OnReply(of, socket)) if *of == request => socket.messages(text)?,
            _ => return None,
        };
        match ahead.take() {
            Some(Ahead::OnReply(_, socket)) => Some((*socket, messages)),
            _ => None,
        }
    }

    /// `turn`'s piece on its speech socket: the one opened ahead, or the
    /// dialogue socket opened now. `None` when it has none, so it goes over
    /// HTTP; `Some(Ok(None))` when nothing in it would be spoken.
    async fn on_socket(
        &self,
        text: &str,
        turn: Turn,
        permitted: Option<&ChatPermission>,
    ) -> Option<Result<Option<TtsStream>>> {
        if let Some((socket, opened)) = self.take_ahead(turn.request) {
            let Some(piece) = socket_piece(text) else {
                return Some(Ok(None));
            };
            let messages = vec![piece, socket_end()];
            return Some(
                self.speak_on(socket, opened, messages, text, turn, permitted)
                    .await,
            );
        }
        let (request, messages) = self.take_on_reply(turn.request, text)?;
        let from = Instant::now();
        let until = tokio::time::Instant::now() + DIALOGUE_OPEN_WITHIN;
        let Some(opened) = permitted_until(
            open_socket(&request, DIALOGUE_OPEN_WITHIN),
            permitted,
            until,
        )
        .await
        else {
            // An exhausted recognition wait is not "no socket available":
            // terminate quietly rather than starting a fresh HTTP attempt.
            return Some(Ok(None));
        };
        let opened = match opened {
            Ok(opened) => opened,
            Err(()) => return Some(Err(Error::Timeout("dialogue speech socket open"))),
        };
        let socket = match opened {
            Ok(socket) => socket,
            Err(error) => {
                if matches!(error, Error::Timeout(_)) {
                    eprintln!(
                        "lamp-voice second: request {}: the dialogue speech socket was slow to open: not open {} ms after its piece was ready",
                        turn.request,
                        from.elapsed().as_millis()
                    );
                }
                return Some(Err(error));
            }
        };
        eprintln!(
            "lamp-voice second: request {}: the dialogue speech socket is open, {} ms after its piece was ready",
            turn.request,
            from.elapsed().as_millis()
        );
        Some(
            self.speak_on(socket, from, messages, text, turn, permitted)
                .await,
        )
    }

    /// `text` as `turn`'s piece on `socket`, opened at `opened`, sent as
    /// `messages`. It is handed on once its first audio came, so a socket
    /// closed or failed before it is told to the caller, which sends the
    /// piece over HTTP instead.
    async fn speak_on(
        &self,
        mut socket: Socket,
        opened: Instant,
        messages: Vec<Value>,
        text: &str,
        turn: Turn,
        permitted: Option<&ChatPermission>,
    ) -> Result<Option<TtsStream>> {
        let sent = Instant::now();
        for message in messages {
            let Some(sent) = permitted_result(
                socket.send(Frame::Text(message.to_string().into())),
                permitted,
            )
            .await
            else {
                return Ok(None);
            };
            #[cfg(test)]
            if sent.is_ok()
                && let Some(after) = &self.after_socket_message
            {
                after();
            }
            if sent.is_err() {
                // Closed before the piece went: its close, if it came, says
                // why (a refused model closes within a second of the open).
                let until = tokio::time::Instant::now() + Duration::from_millis(200);
                let Some(why) = permitted_until(closed_why(&mut socket), permitted, until).await
                else {
                    return Ok(None);
                };
                return Err(why
                    .ok()
                    .flatten()
                    .unwrap_or(Error::Transport("speech socket")));
            }
        }
        // Not `used`: that says the kept HTTP connection went lately, for
        // `warm` (the reviewer's P3 on #2209).
        let index = self.asked(turn.request);
        let mut stream = TtsStream {
            source: Some(Source::Socket(Box::new(socket))),
            piece: Some(Piece {
                tally: self.tally.clone(),
                provider: self.provider.name(),
                request: turn.request,
                index,
                sent_ms: sent.duration_since(turn.heard_at).as_millis(),
                sent,
                reused: true,
                ahead_ms: Some(sent.duration_since(opened).as_millis()),
                chars: text.chars().count(),
            }),
            ..TtsStream::default()
        };
        let until = tokio::time::Instant::now() + SOCKET_FIRST_AUDIO_WITHIN;
        let Some(first) = permitted_until(stream.first_audio(), permitted, until).await else {
            return Ok(None);
        };
        match first {
            Ok(Ok(true)) => Ok(Some(stream)),
            Ok(Ok(false)) => Err(Error::Transport("speech socket closed before its audio")),
            Ok(Err(error)) => Err(error),
            Err(()) => Err(Error::Timeout("speech socket's first audio")),
        }
    }

    /// Whether `socket` was seen before, remembering it either way, as in
    /// use now (not idle).
    // ponytail: a new connection the OS happens to give a recent local port
    // reads as reused; ports are rarely handed out again within minutes,
    // and one whose connection was lost is forgotten.
    fn seen(&self, socket: Option<SocketAddr>) -> bool {
        let (Some(socket), Ok(mut sockets)) = (socket, self.sockets.lock()) else {
            return false;
        };
        if let Some(known) = sockets.iter_mut().find(|(s, _)| *s == socket) {
            known.1 = false;
            return true;
        }
        if sockets.len() == SOCKETS {
            sockets.pop_front();
        }
        sockets.push_back((socket, false));
        false
    }

    /// A lease on `socket`'s connection while its answer is read; none when
    /// the answer closes it.
    fn lease(&self, socket: Option<SocketAddr>, keeps: bool) -> Option<Lease> {
        Some(Lease {
            connections: self.sockets.clone(),
            socket: socket.filter(|_| keeps)?,
            ended: false,
        })
    }

    /// Whether a connection is known to be idle in the pool now.
    fn idle_known(&self) -> bool {
        self.sockets
            .lock()
            .is_ok_and(|sockets| sockets.iter().any(|(_, idle)| *idle))
    }

    /// The connection opened ahead, as a line begins, when no piece or
    /// warming went in the last [`WARM_AFTER`] (`LAMP_SECOND_SPEECH_WARM=on`,
    /// #1712), or when no connection is known to be idle though one went
    /// lately: the last answer's pieces still stream on them, or one was
    /// dropped mid-body at a cut-in and took its connection with it (#1722,
    /// the first builder at 01:52 on 21 Oct: 13 of 20 first pieces on a new
    /// connection came within 10 s of the piece before). A `GET` of the
    /// provider's `models` with the key, read whole, so the answer's first
    /// piece goes on a connection already made. No text is sent, so no
    /// characters are billed (ElevenLabs bills the text of speech requests;
    /// `GET /v1/models` lists models). At most one is in flight. Its status
    /// and how long it took; `None` when not due, when one is in flight, or
    /// when it failed (the piece then connects as before).
    pub async fn warm(&self) -> Option<(u16, Duration)> {
        let _one = Warming::take(&self.warming)?;
        let models = format!(
            "{}/models",
            service_base(self.provider, &self.base_url).ok()?
        );
        let (header, value) = auth(self.provider, &self.key);
        let after = if self.idle_known() {
            WARM_AFTER
        } else {
            Duration::ZERO
        };
        let (status, took, socket, ended, keeps) = warm_get(
            &self.client,
            &self.used,
            after,
            &models,
            (header, &value),
            WARM_BYTES,
        )
        .await?;
        self.seen(socket);
        // Kept only when read to its end and not told to close, as a
        // piece's is.
        if let Some(mut lease) = self.lease(socket, keeps) {
            lease.ended = ended;
        }
        Some((status, took))
    }

    /// Start speaking `text`; its samples come from the stream as they
    /// arrive. Dropping the stream stops the download.
    pub async fn speak(&self, text: &str) -> Result<TtsStream> {
        self.speak_in(text, None).await
    }

    /// As [`Tts::speak`], a piece of `turn`'s answer: the journal says when
    /// it went, on which connection, how soon its first audio byte came and
    /// its characters, never its words; and the turn's pieces at its end
    /// ([`Tts::told`]).
    pub async fn speak_for(&self, text: &str, turn: Turn) -> Result<TtsStream> {
        self.speak_in(text, Some(turn)).await
    }

    async fn speak_in(&self, text: &str, turn: Option<Turn>) -> Result<TtsStream> {
        Ok(self
            .speak_permitted(text, turn, None)
            .await?
            .unwrap_or_default())
    }

    /// One exact request permission follows every socket/HTTP text attempt.
    /// None denotes terminal denial, never a transport failure to retry.
    pub(crate) async fn speak_permitted(
        &self,
        text: &str,
        turn: Option<Turn>,
        permitted: Option<&ChatPermission>,
    ) -> Result<Option<TtsStream>> {
        // Do not put this clock-owning operation behind a generic Pending
        // wrapper: each socket stage must keep polling its original timer.
        let spoken = match turn {
            Some(turn) => {
                let answer = self.on_socket(text, turn, permitted).await;
                #[cfg(test)]
                if answer.is_some()
                    && let Some(after) = &self.after_socket_attempt
                {
                    after();
                }
                answer
            }
            None => None,
        };
        if let Some(spoken) = spoken {
            match spoken {
                Ok(Some(stream)) => return Ok(Some(stream)),
                Ok(None) => return Ok(None),
                Err(error) => {
                    let turn = turn.expect("socket result belongs to a turn");
                    eprintln!(
                        "lamp-voice second: request {}: the speech socket failed before its first audio ({error}); the piece goes over HTTP",
                        turn.request
                    );
                    // The model or the request refused, not a timeout, which
                    // passes (the review of #2629); or the dialogue sessions
                    // all held, or a 429 at the upgrade, which pause the
                    // sockets (the reviewer-lead's P3 on #2658).
                    match error {
                        Error::Status(400 | 404 | 422) => self.refuse(turn.request, &error),
                        Error::Protocol(TOO_MANY) | Error::Status(429) => {
                            self.pause(turn.request, &error)
                        }
                        _ => {}
                    }
                }
            }
        }
        let Some(request) = request(
            self.provider,
            &self.base_url,
            &self.key,
            self.voice.as_deref(),
            self.model_for(turn),
            text,
        )?
        else {
            return Ok(Some(TtsStream::default()));
        };
        let sent = Instant::now();
        if let Ok(mut used) = self.used.lock() {
            *used = Some(sent);
        }
        let index = turn.map(|turn| self.asked(turn.request));
        let Some(response) = permitted_result(
            post_guarded(
                &self.client,
                &request.url,
                (request.header, &request.value),
                &request.body,
                permitted,
                #[cfg(test)]
                self.send_polls.as_deref(),
            ),
            permitted,
        )
        .await
        else {
            return Ok(None);
        };
        let Some(response) = response? else {
            return Ok(None);
        };
        let socket = local_socket(&response);
        let reused = self.seen(socket);
        let lease = self.lease(socket, keeps_its_connection(&response));
        let piece = turn.zip(index).map(|(turn, index)| Piece {
            tally: self.tally.clone(),
            provider: self.provider.name(),
            request: turn.request,
            index,
            sent_ms: sent.duration_since(turn.heard_at).as_millis(),
            sent,
            reused,
            ahead_ms: None,
            chars: text.chars().count(),
        });
        Ok(Some(TtsStream {
            source: Some(Source::Http(response)),
            piece,
            lease,
            ..TtsStream::default()
        }))
    }

    /// One more piece asked for `request`: its number in the turn.
    fn asked(&self, request: u64) -> u32 {
        let Ok(mut tally) = self.tally.lock() else {
            return 0;
        };
        if tally.request != request {
            *tally = Tally {
                request,
                ..Tally::default()
            };
        }
        tally.asked += 1;
        tally.asked
    }

    /// The journal's line for `request`'s pieces, once its turn is over;
    /// `None` when it asked this voice for none.
    pub fn told(&self, request: u64) -> Option<String> {
        let mut tally = self.tally.lock().ok()?;
        if tally.request != request || tally.asked == 0 {
            return None;
        }
        let tally = std::mem::take(&mut *tally);
        Some(format!(
            "request {request} {}: {} of {} pieces began, {} on a reused connection, {} chars; the first piece's first audio byte {} after it went, the slowest {} ms",
            self.provider.name(),
            tally.begun,
            tally.asked,
            tally.reused,
            tally.chars,
            tally
                .first_ms
                .map_or_else(|| "never came".to_owned(), |ms| format!("{ms} ms")),
            tally.slowest_ms,
        ))
    }
}

/// A turn's piece until its first audio byte, when it is journaled.
struct Piece {
    tally: Arc<Mutex<Tally>>,
    provider: &'static str,
    request: u64,
    index: u32,
    /// After the words were heard.
    sent_ms: u128,
    sent: Instant,
    reused: bool,
    /// On the speech socket, opened this long before the piece went.
    ahead_ms: Option<u128>,
    chars: usize,
}

impl Piece {
    /// The piece in the journal's words, its first audio byte `first_ms`
    /// after it went: never its words.
    fn told(&self, first_ms: u128) -> String {
        let on = match self.ahead_ms {
            Some(ahead) => format!("the speech socket opened {ahead} ms before"),
            None => format!(
                "a {} connection",
                if self.reused { "reused" } else { "new" }
            ),
        };
        format!(
            "request {} {} piece {}: {} chars, sent {} ms after the words were heard on {on}; its first audio byte {first_ms} ms after it went",
            self.request, self.provider, self.index, self.chars, self.sent_ms,
        )
    }

    fn began(self) {
        let first_ms = self.sent.elapsed().as_millis();
        eprintln!("lamp-voice second: {}", self.told(first_ms));
        let Ok(mut tally) = self.tally.lock() else {
            return;
        };
        if tally.request != self.request {
            return;
        }
        tally.begun += 1;
        tally.reused += u32::from(self.reused);
        tally.chars += self.chars;
        if self.index == 1 {
            tally.first_ms = Some(first_ms);
        }
        tally.slowest_ms = tally.slowest_ms.max(first_ms);
    }
}

#[derive(Default)]
pub struct TtsStream {
    source: Option<Source>,
    packets: Packets,
    ready: VecDeque<Vec<i16>>,
    /// Journaled at the first audio byte.
    piece: Option<Piece>,
    /// Its connection, over HTTP: given back at the answer's end, lost when
    /// it is dropped before.
    lease: Option<Lease>,
}

/// Where a piece's audio comes from: its HTTP answer's body, or the speech
/// socket's messages.
enum Source {
    Http(reqwest::Response),
    Socket(Box<Socket>),
}

impl TtsStream {
    /// The next packet of at most [`PACKET_SAMPLES`]; `None` at the end.
    pub async fn next(&mut self) -> Result<Option<Vec<i16>>> {
        loop {
            if let Some(packet) = self.ready.pop_front() {
                return Ok(Some(packet));
            }
            if !self.read().await? && self.ready.is_empty() {
                return Ok(None);
            }
        }
    }

    /// Whether audio came before the end: read until it did.
    async fn first_audio(&mut self) -> Result<bool> {
        loop {
            if !self.ready.is_empty() {
                return Ok(true);
            }
            if !self.read().await? {
                return Ok(!self.ready.is_empty());
            }
        }
    }

    /// The next of its audio read into packets; false at the end.
    async fn read(&mut self) -> Result<bool> {
        let bytes = match &mut self.source {
            None => return Ok(false),
            Some(Source::Http(response)) => chunk(response).await?,
            // A close after its first audio ends the piece, as it did
            // before: what was said stands (the review of #2629).
            Some(Source::Socket(socket)) => match socket_audio(socket).await {
                Err(Error::Status(_)) if self.piece.is_none() => None,
                other => other?,
            },
        };
        match bytes {
            Some(bytes) => {
                if let Some(piece) = self.piece.take() {
                    piece.began();
                }
                self.ready
                    .extend(self.packets.push(&bytes).into_iter().map(louder));
            }
            None => {
                self.source = None;
                if let Some(mut lease) = self.lease.take() {
                    lease.ended = true;
                }
                self.ready.extend(self.packets.finish().map(louder));
            }
        }
        Ok(true)
    }
}

/// A speech socket opened with `socket`'s key, its voice's settings sent,
/// within `within` for the connect and the upgrade. `wss` only, but `ws`
/// in tests.
async fn open_socket(socket: &SocketRequest, within: Duration) -> Result<Socket> {
    let mut request = socket
        .url
        .as_str()
        .into_client_request()
        .map_err(|_| Error::Url)?;
    let mut key = HeaderValue::from_str(&socket.value).map_err(|_| Error::Protocol("key"))?;
    key.set_sensitive(true);
    request.headers_mut().insert(socket.header, key);
    let connector = if socket.url.starts_with("wss://") {
        Some(Connector::Rustls(Arc::new(super::tls()?)))
    } else if cfg!(test) {
        None
    } else {
        return Err(Error::Url);
    };
    let (mut opened, _) = tokio::time::timeout(
        within,
        connect_async_tls_with_config(request, None, true, connector),
    )
    .await
    .map_err(|_| Error::Timeout("speech socket connect"))?
    .map_err(|error| match error {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            Error::Status(response.status().as_u16())
        }
        _ => Error::Transport("speech socket connect"),
    })?;
    opened
        .send(Frame::Text(socket.opening().to_string().into()))
        .await
        .map_err(|_| Error::Transport("speech socket"))?;
    Ok(opened)
}

/// A speech socket's next audio, `None` at its last message or its close.
/// A message that isn't one of its answers is the service's own words: told
/// by its kind only.
async fn socket_audio(socket: &mut Socket) -> Result<Option<Vec<u8>>> {
    loop {
        let message = tokio::time::timeout(STALL, socket.next())
            .await
            .map_err(|_| Error::Timeout("speech socket"))?;
        match message {
            None => return Ok(None),
            Some(Ok(Frame::Close(frame))) => return closed(frame.as_ref()),
            Some(Err(_)) => return Err(Error::Transport("speech socket")),
            Some(Ok(Frame::Text(text))) => {
                let Some(answer) = socket_answer(text.as_str()) else {
                    // The service's own words before it closes, said; its
                    // close then says how it ended.
                    match socket_error(text.as_str()) {
                        // The dialogue sessions all held: no audio comes.
                        Some(said) if said.starts_with(TOO_MANY) => {
                            eprintln!(
                                "lamp-voice second: the speech socket's service said: {said}"
                            );
                            return Err(Error::Protocol(TOO_MANY));
                        }
                        Some(said) => {
                            eprintln!(
                                "lamp-voice second: the speech socket's service said: {said}"
                            );
                            continue;
                        }
                        None => return Err(Error::Protocol("speech socket answer")),
                    }
                };
                if !answer.audio.is_empty() {
                    if answer.last {
                        let _ = socket.close(None).await;
                    }
                    return Ok(Some(answer.audio));
                }
                if answer.last {
                    let _ = socket.close(None).await;
                    return Ok(None);
                }
            }
            Some(Ok(_)) => {}
        }
    }
}

/// A speech socket's close, said with its code and reason: one in 4400 to
/// 4499 is the service's refusal passed on by the relay as 4000 plus its
/// HTTP status (the backend on #2135: "4400 `unsupported_model`"), told as
/// that status; any other is the socket's end.
fn closed(
    frame: Option<&tokio_tungstenite::tungstenite::protocol::CloseFrame>,
) -> Result<Option<Vec<u8>>> {
    let Some(frame) = frame else {
        return Ok(None);
    };
    let code = u16::from(frame.code);
    let reason: String = frame.reason.chars().take(160).collect();
    eprintln!("lamp-voice second: the speech socket was closed: {code} \"{reason}\"");
    // The relay may pass the dialogue sessions all held as a close only.
    if reason.contains(TOO_MANY) {
        return Err(Error::Protocol(TOO_MANY));
    }
    match code {
        4400..=4499 => Err(Error::Status(code - 4000)),
        _ => Ok(None),
    }
}

/// Why `socket` closed, when its close came: read for at most a moment
/// after a send failed.
async fn closed_why(socket: &mut Socket) -> Option<Error> {
    let until = tokio::time::Instant::now() + Duration::from_millis(200);
    loop {
        match tokio::time::timeout_at(until, socket.next()).await {
            Ok(Some(Ok(Frame::Close(frame)))) => return closed(frame.as_ref()).err(),
            Ok(Some(Ok(_))) => {}
            _ => return None,
        }
    }
}

/// +6 dB. OpenAI's `tts-1` voice `nova` through the company relay, measured
/// from lamp-52e6 on 2026-10-15: peak -7.3 dBFS, RMS -28.8 dBFS, so the
/// second voice was heard much quieter than Gemini's. Twice the amplitude,
/// clipped at full scale, puts that peak near -1.3 dBFS.
// ponytail: one fixed gain for every provider and voice; a per-voice level
// in `second.json` if another voice is too loud or still too quiet.
const GAIN: i32 = 2;

fn louder(packet: Vec<i16>) -> Vec<i16> {
    packet
        .into_iter()
        .map(|sample| {
            (i32::from(sample) * GAIN).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
        })
        .collect()
}

/// Little-endian bytes cut anywhere, even mid-sample, to full packets.
#[derive(Default)]
pub(crate) struct Packets {
    odd: Option<u8>,
    samples: Vec<i16>,
}

impl Packets {
    pub fn push(&mut self, mut bytes: &[u8]) -> Vec<Vec<i16>> {
        if let (Some(low), Some((&high, rest))) = (self.odd, bytes.split_first()) {
            self.samples.push(i16::from_le_bytes([low, high]));
            self.odd = None;
            bytes = rest;
        }
        let pairs = bytes.chunks_exact(2);
        if let [last] = pairs.remainder() {
            self.odd = Some(*last);
        }
        self.samples
            .extend(pairs.map(|pair| i16::from_le_bytes([pair[0], pair[1]])));
        let full = self.samples.len() / PACKET_SAMPLES * PACKET_SAMPLES;
        let out = self.samples[..full]
            .chunks(PACKET_SAMPLES)
            .map(<[i16]>::to_vec)
            .collect();
        self.samples.drain(..full);
        out
    }

    /// The short last packet. A dangling odd byte is not a sample.
    pub fn finish(&mut self) -> Option<Vec<i16>> {
        self.odd = None;
        let rest = std::mem::take(&mut self.samples);
        (!rest.is_empty()).then_some(rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn a_voice_is_named_as_set_or_as_the_providers_default() {
        assert_eq!(voice_name(TtsProvider::ElevenLabs, None), "Rachel");
        assert_eq!(voice_name(TtsProvider::OpenAi, None), "nova");
        assert_eq!(voice_name(TtsProvider::ElevenLabs, Some("Ngan")), "Ngan");
        // The default named is the one a request is made with.
        let asked = request(
            TtsProvider::ElevenLabs,
            "https://api.elevenlabs.io",
            "k",
            None,
            None,
            "Hi.",
        )
        .unwrap()
        .unwrap();
        assert!(asked.url.contains("/text-to-speech/21m00Tcm4TlvDq8ikWAM/"));
    }

    /// A local keep-alive HTTP/1.1 server: `GET`s are answered with a short
    /// models list (or never, with `hang_get`), `POST`s with `pcm`. What it
    /// gives back: its address, the connections it accepted and each
    /// request's first line.
    async fn keeping(
        pcm: Vec<u8>,
        hang_get: bool,
    ) -> (String, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>) {
        serving(pcm, hang_get, false, None).await
    }

    /// As [`keeping`], but with `hold` each `POST` sends its head and its
    /// first two bytes, then waits for one of `hold`'s permits before the
    /// rest: a piece still streaming; and with `close_get` a `GET` is
    /// answered whole with `connection: close`, and its connection closed.
    async fn serving(
        pcm: Vec<u8>,
        hang_get: bool,
        close_get: bool,
        hold: Option<Arc<tokio::sync::Semaphore>>,
    ) -> (String, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let connections = Arc::new(AtomicUsize::new(0));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let (counted, seen) = (connections.clone(), asked.clone());
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                counted.fetch_add(1, Ordering::SeqCst);
                let (seen, pcm, hold) = (seen.clone(), pcm.clone(), hold.clone());
                tokio::spawn(async move {
                    let mut buffer = Vec::new();
                    let mut read = [0; 4096];
                    loop {
                        let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") else {
                            match socket.read(&mut read).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => buffer.extend_from_slice(&read[..n]),
                            }
                            continue;
                        };
                        let head = String::from_utf8_lossy(&buffer[..end]).to_lowercase();
                        let length: usize = head
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .map_or(0, |n| n.trim().parse().unwrap());
                        while buffer.len() < end + 4 + length {
                            match socket.read(&mut read).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => buffer.extend_from_slice(&read[..n]),
                            }
                        }
                        buffer.drain(..end + 4 + length);
                        let line = head.lines().next().unwrap_or_default().to_owned();
                        let get = line.starts_with("get");
                        assert!(head.contains("xi-api-key: k"), "{head}");
                        seen.lock().unwrap().push(line);
                        if get && hang_get {
                            std::future::pending::<()>().await;
                        }
                        let body = if get {
                            br#"{"models":[]}"#.to_vec()
                        } else {
                            pcm.clone()
                        };
                        let closing = get && close_get;
                        let answer = format!(
                            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n{}\r\n",
                            body.len(),
                            if closing { "connection: close\r\n" } else { "" }
                        );
                        let held = match &hold {
                            Some(_) if !get => 2.min(body.len()),
                            _ => body.len(),
                        };
                        if socket.write_all(answer.as_bytes()).await.is_err()
                            || socket.write_all(&body[..held]).await.is_err()
                        {
                            return;
                        }
                        if closing {
                            let _ = socket.shutdown().await;
                            return;
                        }
                        if held < body.len() {
                            if let Some(hold) = &hold {
                                hold.acquire().await.unwrap().forget();
                            }
                            if socket.write_all(&body[held..]).await.is_err() {
                                return;
                            }
                        }
                    }
                });
            }
        });
        (format!("http://{address}/v1"), connections, asked)
    }

    fn turn(request: u64) -> Turn {
        Turn {
            request,
            heard_at: Instant::now(),
        }
    }

    async fn said(tts: &Tts, text: &str, request: u64) -> Vec<i16> {
        let mut speech = tts.speak_for(text, turn(request)).await.unwrap();
        let mut samples = Vec::new();
        while let Some(packet) = speech.next().await.unwrap() {
            samples.extend(packet);
        }
        samples
    }

    /// A local speech socket: it reads the voice's settings, the piece and
    /// the end, then sends `answers`, each a message's text, and "close"
    /// closes it there. Its address, and on join what it read.
    async fn socket_serving(
        answers: Vec<&'static str>,
    ) -> (String, tokio::task::JoinHandle<Vec<Value>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            let mut read = Vec::new();
            while read.len() < 3 {
                match ws.next().await {
                    Some(Ok(Frame::Text(text))) => {
                        read.push(serde_json::from_str(text.as_str()).unwrap());
                    }
                    Some(Ok(_)) => {}
                    _ => return read,
                }
            }
            for answer in answers {
                if answer == "close" {
                    let _ = ws.close(None).await;
                    return read;
                }
                // "close:CODE:REASON": closed as the relay passes a refusal.
                if let Some(rest) = answer.strip_prefix("close:") {
                    let (code, reason) = rest.split_once(':').unwrap();
                    let frame = tokio_tungstenite::tungstenite::protocol::CloseFrame {
                        code: code.parse::<u16>().unwrap().into(),
                        reason: reason.to_owned().into(),
                    };
                    let _ = ws.close(Some(frame)).await;
                    return read;
                }
                if answer == "hang" {
                    std::future::pending::<()>().await;
                }
                ws.send(Frame::Text(answer.into())).await.unwrap();
            }
            read
        });
        (address, server)
    }

    /// What a local socket read, within 5 s: one never sent its messages
    /// fails the test rather than hanging it.
    async fn served(server: tokio::task::JoinHandle<Vec<Value>>) -> Vec<Value> {
        tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("the socket was sent its messages")
            .unwrap()
    }

    fn local_socket(address: &str) -> SocketRequest {
        SocketRequest {
            url: format!("ws://{address}/v1/ws/elevenlabs/text-to-speech/x/stream-input"),
            header: "authorization",
            value: "Bearer k".to_owned(),
            shape: SocketShape::StreamInput,
            voice: "x".to_owned(),
        }
    }

    /// A local dialogue socket: it counts the connections it accepts, reads
    /// the opening, the input, `flush` and `close_socket` into `read`, then
    /// sends `answers` as [`socket_serving`] does, and holds the socket open
    /// after them, so only the client's reading ends a piece.
    async fn dialogue_serving(
        answers: Vec<&'static str>,
    ) -> (String, Arc<AtomicUsize>, Arc<Mutex<Vec<Value>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let (accepted, read) = (Arc::new(AtomicUsize::new(0)), Arc::default());
        let (counted, kept): (_, Arc<Mutex<Vec<Value>>>) = (accepted.clone(), Arc::clone(&read));
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            counted.fetch_add(1, Ordering::SeqCst);
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            while kept.lock().unwrap().len() < 4 {
                match ws.next().await {
                    Some(Ok(Frame::Text(text))) => kept
                        .lock()
                        .unwrap()
                        .push(serde_json::from_str(text.as_str()).unwrap()),
                    Some(Ok(_)) => {}
                    _ => return,
                }
            }
            for answer in answers {
                if let Some(rest) = answer.strip_prefix("close:") {
                    let (code, reason) = rest.split_once(':').unwrap();
                    let frame = tokio_tungstenite::tungstenite::protocol::CloseFrame {
                        code: code.parse::<u16>().unwrap().into(),
                        reason: reason.to_owned().into(),
                    };
                    let _ = ws.close(Some(frame)).await;
                    return;
                }
                ws.send(Frame::Text(answer.into())).await.unwrap();
            }
            std::future::pending::<()>().await;
        });
        (address, accepted, read)
    }

    fn local_dialogue(address: &str) -> SocketRequest {
        SocketRequest {
            url: format!("ws://{address}/v1/ws/elevenlabs/text-to-dialogue/stream-input"),
            header: "authorization",
            value: "Bearer k".to_owned(),
            shape: SocketShape::Dialogue,
            voice: "21m00Tcm4TlvDq8ikWAM".to_owned(),
        }
    }

    /// #2602: v4 Turbo's socket is the dialogue one, opened by the answer's
    /// first piece with words, never as the chat is asked; the piece goes
    /// as an input in the voice, then `flush` and `close_socket`. Its audio
    /// comes as packets as on `stream-input`, past the end of the turn's
    /// audio, and `is_final` ends it though the socket stays open.
    #[tokio::test]
    async fn a_dialogue_socket_opens_with_its_piece_and_is_told_to_close_after_it() {
        let (address, accepted, read) = dialogue_serving(vec![
            r#"{"audio":"AQACAA==","alignment":null}"#,
            r#"{"is_final_audio_for_turn":true}"#,
            r#"{"audio":"AwA="}"#,
            r#"{"is_final":true}"#,
        ])
        .await;
        let tts = Tts::with(TtsProvider::ElevenLabs, PROXY, "k", None).unwrap();
        tts.open_ahead_on(7, &local_dialogue(&address)).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(accepted.load(Ordering::SeqCst), 0, "not opened ahead");
        assert!(said(&tts, " [laughs] ", 7).await.is_empty());
        assert_eq!(accepted.load(Ordering::SeqCst), 0, "no words: not opened");
        let from = Instant::now();
        assert_eq!(said(&tts, " Hello there. ", 7).await, [2, 4, 6]);
        assert!(from.elapsed() < Duration::from_secs(1), "is_final ended it");
        assert_eq!(accepted.load(Ordering::SeqCst), 1);
        assert_eq!(
            *read.lock().unwrap(),
            [
                json!({"voices": ["21m00Tcm4TlvDq8ikWAM"]}),
                json!({"inputs": [{"text": "Hello there.", "voice_id": "21m00Tcm4TlvDq8ikWAM"}]}),
                json!({"flush": true}),
                json!({"close_socket": true}),
            ]
        );
        assert!(tts.ahead.lock().unwrap().is_none(), "used once");
        let told = tts.told(7).unwrap();
        assert!(told.contains("1 of 1 pieces began"), "{told}");
        // Another request's piece finds no socket.
        let other = Tts::with(TtsProvider::ElevenLabs, PROXY, "k", None).unwrap();
        other.open_ahead_on(8, &local_dialogue("127.0.0.1:9")).await;
        assert!(other.take_on_reply(9, "Hi.").is_none());
        assert!(other.take_on_reply(8, "Hi.").is_some());
    }

    /// The backend on #2135: past 105 open dialogue sessions the service
    /// says `too_many_concurrent_requests`. The piece goes over HTTP in the
    /// socket's model, nothing is refused, and no socket is opened for
    /// `SOCKET_BUSY_PAUSE`; the relay's close alone says it too. A 4400
    /// refuses v4 as #2629 refuses a model on `stream-input`.
    #[tokio::test]
    async fn all_dialogue_sessions_held_or_a_refusal_stops_the_sockets() {
        let (url, _, asked) = keeping(vec![5, 0, 6, 0], false).await;
        let v4 = || Tts {
            socket_model: Some(DIALOGUE_MODEL.to_owned()),
            ..Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap()
        };
        for said_so in [
            r#"{"message":"Too many concurrent dialogue sessions","error":"too_many_concurrent_requests","code":1008}"#,
            "close:4429:too_many_concurrent_requests",
        ] {
            let (address, _, _) = dialogue_serving(vec![said_so, "close:1008:busy"]).await;
            let tts = v4();
            assert!(tts.opens_ahead());
            tts.open_ahead_on(8, &local_dialogue(&address)).await;
            assert_eq!(said(&tts, "Hello there.", 8).await, [10, 12]);
            assert!(tts.refused.lock().unwrap().is_none(), "{said_so}");
            assert!(!tts.opens_ahead(), "paused: {said_so}");
            assert_eq!(tts.model_for(Some(turn(8))), Some(DIALOGUE_MODEL));
            *tts.paused_until.lock().unwrap() = Some(Instant::now());
            assert!(tts.opens_ahead(), "the pause ends");
        }
        let (address, _, _) =
            dialogue_serving(vec!["close:4400:ElevenLabs 400: unsupported_model"]).await;
        let tts = v4();
        tts.open_ahead_on(9, &local_dialogue(&address)).await;
        assert_eq!(said(&tts, "Hello there.", 9).await, [10, 12]);
        assert_eq!(tts.refused.lock().unwrap().as_deref(), Some(DIALOGUE_MODEL));
        assert!(!tts.opens_ahead(), "refused: never asked again");
        assert_eq!(asked.lock().unwrap().len(), 3, "each piece over HTTP");
    }

    /// A local socket server that accepts each connection and reads its
    /// upgrade request, then `answer`s it: `None` holds the handshake
    /// unanswered, a slow relay; `Some(head)` writes that HTTP head. The
    /// connections it accepted.
    async fn upgrade_answering(answer: Option<&'static str>) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let accepted = Arc::new(AtomicUsize::new(0));
        let counted = accepted.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                counted.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut buffer = Vec::new();
                    let mut read = [0; 1024];
                    while !buffer.windows(4).any(|w| w == b"\r\n\r\n") {
                        match socket.read(&mut read).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buffer.extend_from_slice(&read[..n]),
                        }
                    }
                    match answer {
                        Some(head) => {
                            let _ = socket.write_all(head.as_bytes()).await;
                            let _ = socket.shutdown().await;
                        }
                        None => std::future::pending::<()>().await,
                    }
                });
            }
        });
        (address, accepted)
    }

    /// The reviewer-lead's P2 on #2658: the dialogue socket opens on the
    /// piece's own path, so a relay slow to answer its handshake costs the
    /// piece at most `DIALOGUE_OPEN_WITHIN`, not `CONNECT`'s 10 s, and the
    /// piece goes over HTTP well within 500 ms. A slow open is no refusal
    /// and no pause.
    #[tokio::test]
    async fn a_dialogue_socket_slow_to_open_gives_its_piece_to_http_within_its_budget() {
        let (address, accepted) = upgrade_answering(None).await;
        let (url, _, asked) = keeping(vec![5, 0, 6, 0], false).await;
        let tts = Tts {
            socket_model: Some(DIALOGUE_MODEL.to_owned()),
            ..Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap()
        };
        tts.open_ahead_on(5, &local_dialogue(&address)).await;
        let from = Instant::now();
        assert_eq!(said(&tts, "Hello there.", 5).await, [10, 12]);
        let took = from.elapsed();
        assert!(
            took >= DIALOGUE_OPEN_WITHIN && took < Duration::from_millis(500),
            "{took:?}"
        );
        assert_eq!(accepted.load(Ordering::SeqCst), 1, "the open was tried");
        assert_eq!(asked.lock().unwrap().len(), 1, "the piece over HTTP");
        assert!(tts.refused.lock().unwrap().is_none());
        assert!(tts.opens_ahead(), "a slow open pauses nothing");
    }

    /// The reviewer-lead's P3 on #2658: a 429 at the socket's upgrade pauses
    /// the sockets for `SOCKET_BUSY_PAUSE`, as `too_many_concurrent_requests`
    /// does, so the next answers don't each try the open again: on the
    /// dialogue socket, opened by its piece, and on `stream-input`, opened
    /// ahead. The piece goes over HTTP and nothing is refused.
    #[tokio::test]
    async fn a_429_at_the_sockets_upgrade_pauses_the_sockets() {
        const TOO_MANY_HEAD: &str = "HTTP/1.1 429 Too Many Requests\r\ncontent-length: 0\r\n\r\n";
        let (address, accepted) = upgrade_answering(Some(TOO_MANY_HEAD)).await;
        let (url, _, asked) = keeping(vec![5, 0, 6, 0], false).await;
        let tts = Tts {
            socket_model: Some(DIALOGUE_MODEL.to_owned()),
            ..Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap()
        };
        assert!(tts.opens_ahead());
        tts.open_ahead_on(6, &local_dialogue(&address)).await;
        assert_eq!(said(&tts, "Hello there.", 6).await, [10, 12]);
        assert_eq!(accepted.load(Ordering::SeqCst), 1);
        assert_eq!(asked.lock().unwrap().len(), 1, "the piece over HTTP");
        assert!(tts.refused.lock().unwrap().is_none(), "not refused");
        assert!(!tts.opens_ahead(), "paused after the 429");
        let until = tts.paused_until.lock().unwrap().unwrap();
        assert!(until > Instant::now() + SOCKET_BUSY_PAUSE - Duration::from_secs(5));
        // `stream-input`, opened ahead, pauses on its 429 too.
        let ahead = Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap();
        ahead.open_ahead_on(7, &local_socket(&address)).await;
        assert!(!ahead.opens_ahead(), "paused after the 429 ahead");
        assert!(ahead.refused.lock().unwrap().is_none());
    }

    /// The Reviewer's P3 on #2658: a 400 at `stream-input`'s upgrade, opened
    /// ahead, refuses the socket's model as its 4400 close does, so no open
    /// ahead is tried again on the next answers; nothing is paused.
    #[tokio::test]
    async fn a_400_at_the_upgrade_ahead_refuses_the_model() {
        const REFUSED_HEAD: &str = "HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\n\r\n";
        let (address, accepted) = upgrade_answering(Some(REFUSED_HEAD)).await;
        let tts = Tts::with(TtsProvider::ElevenLabs, PROXY, "k", None).unwrap();
        assert!(tts.opens_ahead());
        tts.open_ahead_on(9, &local_socket(&address)).await;
        assert_eq!(accepted.load(Ordering::SeqCst), 1);
        assert_eq!(
            tts.refused.lock().unwrap().as_deref(),
            Some(tts.socket_model())
        );
        assert!(!tts.opens_ahead(), "refused: not tried again");
        assert!(tts.paused_until.lock().unwrap().is_none(), "not paused");
    }

    /// `LAMP_SECOND_SPEECH_SOCKET` (#2135's row): the socket opened ahead
    /// for a request is taken by its first piece that finds it open. Its
    /// audio comes as packets, as loud as over HTTP, and it is used once. A
    /// piece of another request finds nothing.
    #[tokio::test]
    async fn the_first_piece_speaks_on_the_socket_opened_ahead() {
        let (address, server) = socket_serving(vec![
            r#"{"audio":"AQACAA==","isFinal":false}"#,
            r#"{"audio":"AwA=","isFinal":false}"#,
            r#"{"audio":null,"isFinal":true}"#,
        ])
        .await;
        let tts = Tts::with(TtsProvider::ElevenLabs, PROXY, "k", None).unwrap();
        tts.open_ahead_on(7, &local_socket(&address)).await;
        assert!(tts.take_ahead(6).is_none(), "another request's");
        assert_eq!(said(&tts, "Hello there.", 7).await, [2, 4, 6]);
        assert_eq!(
            served(server).await,
            [
                socket_opening(),
                socket_piece("Hello there.").unwrap(),
                socket_end()
            ]
        );
        assert!(tts.take_ahead(7).is_none(), "used once");
        assert!(
            tts.used.lock().unwrap().is_none(),
            "the kept HTTP connection wasn't used"
        );
        let told = tts.told(7).unwrap();
        assert!(told.contains("1 of 1 pieces began"), "{told}");
    }

    /// A socket closed before any audio: the piece goes over HTTP instead,
    /// and is said.
    #[tokio::test]
    async fn a_piece_goes_over_http_when_its_socket_closes_before_any_audio() {
        let (address, server) = socket_serving(vec!["close"]).await;
        let (url, _, asked) = keeping(vec![5, 0, 6, 0], false).await;
        let tts = Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap();
        tts.open_ahead_on(8, &local_socket(&address)).await;
        assert_eq!(said(&tts, "Hello there.", 8).await, [10, 12]);
        assert_eq!(served(server).await.len(), 3);
        assert_eq!(asked.lock().unwrap().len(), 1, "one piece over HTTP");
    }

    /// A socket open but silent after its piece costs it at most
    /// `SOCKET_FIRST_AUDIO_WITHIN`, then the piece goes over HTTP (the
    /// reviewer's P3 on #2209); and a socket piece doesn't mark the kept
    /// HTTP connection as used, so `warm` still opens it.
    #[tokio::test]
    async fn a_silent_socket_gives_its_piece_to_http_within_its_bound() {
        let (address, _server) = socket_serving(vec!["hang"]).await;
        let (url, _, asked) = keeping(vec![5, 0, 6, 0], false).await;
        let tts = Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap();
        tts.open_ahead_on(12, &local_socket(&address)).await;
        let from = Instant::now();
        assert_eq!(said(&tts, "Hello there.", 12).await, [10, 12]);
        assert!(
            from.elapsed() >= SOCKET_FIRST_AUDIO_WITHIN
                && from.elapsed() < SOCKET_FIRST_AUDIO_WITHIN + Duration::from_secs(1),
            "{:?}",
            from.elapsed()
        );
        assert_eq!(asked.lock().unwrap().len(), 1);
    }

    /// #2135 (lamp-4ace, 21 Oct 17:00–17:19): the relay accepts the socket,
    /// then passes ElevenLabs' refusal of the model as close 4400. The piece
    /// goes over HTTP, the close's code and reason are read, and no socket
    /// is opened ahead for that model again (53 of 53 were refused there).
    #[tokio::test]
    async fn a_refused_model_closes_the_socket_and_no_socket_is_opened_for_it_again() {
        let (address, server) =
            socket_serving(vec!["close:4400:ElevenLabs 400: unsupported_model"]).await;
        let (url, _, asked) = keeping(vec![5, 0, 6, 0], false).await;
        let tts = Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap();
        assert!(tts.opens_ahead());
        tts.open_ahead_on(8, &local_socket(&address)).await;
        assert_eq!(said(&tts, "Hello there.", 8).await, [10, 12]);
        assert_eq!(served(server).await.len(), 3);
        assert_eq!(asked.lock().unwrap().len(), 1, "the piece over HTTP");
        assert_eq!(
            tts.refused.lock().unwrap().as_deref(),
            Some("eleven_flash_v2_5")
        );
        assert!(!tts.opens_ahead(), "refused: never asked again");
        // Another model, set for the socket, may be asked.
        let other = Tts {
            socket_model: Some("eleven_turbo_v2_5".to_owned()),
            ..Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap()
        };
        *other.refused.lock().unwrap() = Some("eleven_flash_v2_5".to_owned());
        assert!(other.opens_ahead());
    }

    /// ElevenLabs says why before it closes an idle socket (a JSON error,
    /// then close 1008): read and said, the piece goes over HTTP, and the
    /// model isn't refused, since an idle close is no refusal.
    #[tokio::test]
    async fn a_service_error_before_the_close_is_read_and_the_piece_goes_over_http() {
        let (address, server) = socket_serving(vec![
            r#"{"message":"Have not received a new text input within the timeout of 20 seconds","error":"input_timeout_exceeded","code":1008}"#,
            "close:1008:input timeout",
        ])
        .await;
        let (url, _, asked) = keeping(vec![5, 0, 6, 0], false).await;
        let tts = Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap();
        tts.open_ahead_on(13, &local_socket(&address)).await;
        assert_eq!(said(&tts, "Hello there.", 13).await, [10, 12]);
        assert_eq!(served(server).await.len(), 3);
        assert_eq!(asked.lock().unwrap().len(), 1);
        assert!(tts.refused.lock().unwrap().is_none());
        assert_eq!(
            socket_error(r#"{"message":"Have not received a new text input","error":"input_timeout_exceeded"}"#)
                .as_deref(),
            Some("input_timeout_exceeded: Have not received a new text input")
        );
        assert_eq!(socket_error(r#"{"audio":null,"isFinal":true}"#), None);
    }

    /// One answer keeps one voice: every piece of an answer whose socket
    /// opened is said in the socket's model, those over HTTP too; another
    /// request's in the route's model.
    #[tokio::test]
    async fn every_piece_of_a_socketed_answer_is_said_in_the_sockets_model() {
        let (address, _server) = socket_serving(vec!["hang"]).await;
        let tts = Tts {
            model: Some("eleven_v4_turbo".to_owned()),
            ..Tts::with(TtsProvider::ElevenLabs, PROXY, "k", None).unwrap()
        };
        assert_eq!(tts.model_for(Some(turn(7))), Some("eleven_v4_turbo"));
        tts.open_ahead_on(7, &local_socket(&address)).await;
        assert_eq!(tts.model_for(Some(turn(7))), Some("eleven_flash_v2_5"));
        assert_eq!(tts.model_for(Some(turn(8))), Some("eleven_v4_turbo"));
        assert_eq!(tts.model_for(None), Some("eleven_v4_turbo"));
        let set = Tts {
            socket_model: Some("eleven_turbo_v2_5".to_owned()),
            ..Tts::with(TtsProvider::ElevenLabs, PROXY, "k", None).unwrap()
        };
        *set.socketed.lock().unwrap() = Some(9);
        assert_eq!(set.model_for(Some(turn(9))), Some("eleven_turbo_v2_5"));
    }

    /// The review of #2629: only a refusal of the model or the request
    /// (4400, 4404, 4422) refuses the model; a rate limit's 4429 sends its
    /// piece over HTTP and refuses nothing, but pauses the sockets for
    /// `SOCKET_BUSY_PAUSE`, as a 429 at the upgrade does (#2658). A refusal
    /// puts the rest of that answer in the route's model.
    #[tokio::test]
    async fn only_a_refused_model_stops_the_sockets_and_its_answer_goes_on_in_the_routes_model() {
        let (address, _server) = socket_serving(vec!["close:4429:rate limited"]).await;
        let (url, _, asked) = keeping(vec![5, 0, 6, 0], false).await;
        let tts = Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap();
        tts.open_ahead_on(8, &local_socket(&address)).await;
        assert_eq!(said(&tts, "Hello there.", 8).await, [10, 12]);
        assert_eq!(asked.lock().unwrap().len(), 1);
        assert!(
            tts.refused.lock().unwrap().is_none(),
            "a rate limit refuses nothing"
        );
        assert!(!tts.opens_ahead(), "a rate limit pauses the sockets");
        let (address, _server) =
            socket_serving(vec!["close:4400:ElevenLabs 400: unsupported_model"]).await;
        let tts = Tts {
            model: Some("eleven_v4_turbo".to_owned()),
            ..Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap()
        };
        tts.open_ahead_on(9, &local_socket(&address)).await;
        assert_eq!(tts.model_for(Some(turn(9))), Some("eleven_flash_v2_5"));
        assert_eq!(said(&tts, "Hello there.", 9).await, [10, 12]);
        assert_eq!(
            tts.model_for(Some(turn(9))),
            Some("eleven_v4_turbo"),
            "refused: the rest of the answer in the route's model"
        );
    }

    /// The review of #2629: a close after the piece's first audio ends the
    /// piece with what was said, as before, never the whole answer; and a
    /// socket that couldn't open still keeps the answer in one voice.
    #[tokio::test]
    async fn a_close_after_the_first_audio_ends_only_the_piece() {
        let (address, _server) = socket_serving(vec![
            r#"{"audio":"AQACAA==","isFinal":false}"#,
            "close:4400:ElevenLabs 400: late",
        ])
        .await;
        let tts = Tts::with(TtsProvider::ElevenLabs, PROXY, "k", None).unwrap();
        tts.open_ahead_on(7, &local_socket(&address)).await;
        assert_eq!(said(&tts, "Hello there.", 7).await, [2, 4]);
        let refused = SocketRequest {
            url: "ws://127.0.0.1:9/x".to_owned(),
            header: "authorization",
            value: "Bearer k".to_owned(),
            shape: SocketShape::StreamInput,
            voice: "x".to_owned(),
        };
        tts.open_ahead_on(10, &refused).await;
        assert_eq!(tts.model_for(Some(turn(10))), Some("eleven_flash_v2_5"));
    }

    /// A piece that finds its socket still opening goes over HTTP and
    /// leaves it for a later piece; a socket that can't open leaves
    /// nothing; a newer request's opening isn't touched by an older one's.
    #[tokio::test]
    async fn a_socket_still_opening_or_not_opened_leaves_its_pieces_to_http() {
        let tts = Tts::with(TtsProvider::ElevenLabs, PROXY, "k", None).unwrap();
        *tts.ahead.lock().unwrap() = Some(Ahead::Opening(9));
        assert!(tts.take_ahead(9).is_none());
        assert!(matches!(
            *tts.ahead.lock().unwrap(),
            Some(Ahead::Opening(9))
        ));
        let refused = SocketRequest {
            url: "ws://127.0.0.1:9/x".to_owned(),
            header: "authorization",
            value: "Bearer k".to_owned(),
            shape: SocketShape::StreamInput,
            voice: "x".to_owned(),
        };
        tts.open_ahead_on(10, &refused).await;
        assert!(tts.ahead.lock().unwrap().is_none());
        // OpenAI's voice has no socket.
        let openai = Tts::with(TtsProvider::OpenAi, PROXY, "k", None).unwrap();
        openai.open_ahead(11).await;
        assert!(openai.ahead.lock().unwrap().is_none());
    }

    /// #1712, the first builder at 19:57: as a line begins after a quiet
    /// minute, the configured voice's connection is opened with a `GET` of
    /// `models`, and the answer's first piece goes on it; one used lately,
    /// or warmed just now, is not asked again.
    #[tokio::test]
    async fn an_idle_voice_opens_its_connection_ahead_and_its_first_piece_reuses_it() {
        let (url, connections, asked) = keeping(vec![1, 0, 2, 0], false).await;
        let tts = Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap();
        assert_eq!(tts.warm().await.map(|(status, _)| status), Some(200));
        assert_eq!(tts.warm().await, None, "warmed just now");
        assert_eq!(said(&tts, "Hello there.", 7).await, [2, 4]);
        assert_eq!(connections.load(Ordering::SeqCst), 1, "one connection");
        assert_eq!(
            *asked.lock().unwrap(),
            [
                "get /v1/elevenlabs/models http/1.1",
                "post /v1/elevenlabs/text-to-speech/21m00tcm4tlvdq8ikwam/stream?output_format=pcm_24000 http/1.1",
            ]
        );
        let told = tts.told(7).unwrap();
        assert!(
            told.starts_with(
                "request 7 ElevenLabs: 1 of 1 pieces began, 1 on a reused connection, 12 chars; the first piece's first audio byte "
            ),
            "{told}"
        );
        assert_eq!(tts.told(7), None, "told once");
    }

    /// A voice that spoke lately is not warmed: no `GET` goes.
    #[tokio::test]
    async fn a_busy_voice_is_not_warmed() {
        let (url, connections, asked) = keeping(vec![1, 0], false).await;
        let tts = Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap();
        said(&tts, "Hi.", 1).await;
        assert_eq!(tts.warm().await, None);
        said(&tts, "Hi again.", 1).await;
        assert_eq!(connections.load(Ordering::SeqCst), 1);
        assert!(asked.lock().unwrap().iter().all(|l| l.starts_with("post")));
        let told = tts.told(1).unwrap();
        assert!(
            told.contains("2 of 2 pieces began, 1 on a reused connection, 12 chars"),
            "{told}"
        );
    }

    /// A warming that never answers neither holds the piece up nor fails
    /// it: the piece makes its own connection.
    #[tokio::test]
    async fn a_warming_that_hangs_does_not_hold_the_piece() {
        let (url, connections, _) = keeping(vec![5, 0], true).await;
        let tts = Arc::new(Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap());
        let warming = tokio::spawn({
            let tts = tts.clone();
            async move { tts.warm().await }
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        let samples = tokio::time::timeout(Duration::from_secs(2), said(&tts, "Hello.", 3))
            .await
            .expect("not held by the warming");
        assert_eq!(samples, [10]);
        assert_eq!(connections.load(Ordering::SeqCst), 2);
        assert!(tts.told(3).unwrap().contains("0 on a reused connection"));
        warming.abort();
    }

    /// Whether `stream`, a piece over HTTP, came on a reused connection.
    /// (The servers count a connection before they answer on it, so a count
    /// read after an answer came is final.)
    fn on_a_kept_connection(stream: &TtsStream) -> bool {
        stream.piece.as_ref().expect("a turn's piece").reused
    }

    /// #1722 (the first builder, 21 Oct 01:52): a line begun while the last
    /// answer's piece still streams on the one kept connection finds no
    /// idle one, so the warming opens one more though a piece went a moment
    /// ago, and the next answer's first piece goes on it.
    #[tokio::test]
    async fn a_line_begun_while_the_last_answer_streams_finds_a_kept_connection() {
        let hold = Arc::new(tokio::sync::Semaphore::new(0));
        let (url, connections, _) = serving(vec![1, 0, 2, 0], false, false, Some(hold)).await;
        let tts = Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap();
        assert!(tts.warm().await.is_some(), "a quiet voice is warmed");
        let streaming = tts.speak_for("The last answer.", turn(1)).await.unwrap();
        assert!(on_a_kept_connection(&streaming));
        assert_eq!(connections.load(Ordering::SeqCst), 1);
        // The next line begins while that piece streams.
        assert!(
            tts.warm().await.is_some(),
            "no idle connection is known, so one more is opened"
        );
        assert_eq!(connections.load(Ordering::SeqCst), 2);
        let first = tts.speak_for("Hello.", turn(2)).await.unwrap();
        assert!(on_a_kept_connection(&first), "the first piece reused one");
        assert_eq!(connections.load(Ordering::SeqCst), 2, "and opened none");
        drop(streaming);
    }

    /// A piece cut off mid-stream (a cut-in) takes its connection with it:
    /// the next line's warming opens another, and its first piece reuses it.
    #[tokio::test]
    async fn a_cut_in_mid_stream_leaves_the_next_first_piece_a_kept_connection() {
        let hold = Arc::new(tokio::sync::Semaphore::new(0));
        let (url, connections, _) = serving(vec![1, 0, 2, 0], false, false, Some(hold)).await;
        let tts = Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap();
        assert!(tts.warm().await.is_some());
        let mut streaming = tts.speak_for("The last answer.", turn(1)).await.unwrap();
        assert!(on_a_kept_connection(&streaming));
        // Its first bytes are read, then it is dropped mid-body.
        assert!(streaming.read().await.unwrap());
        drop(streaming);
        assert!(
            tts.warm().await.is_some(),
            "the connection went with the cut piece, so one more is opened"
        );
        assert_eq!(connections.load(Ordering::SeqCst), 2);
        let first = tts.speak_for("Hello.", turn(2)).await.unwrap();
        assert!(on_a_kept_connection(&first));
        assert_eq!(connections.load(Ordering::SeqCst), 2);
    }

    /// The reviewer's P3 1 on #2351: a warming whose `models` answer is read
    /// to its end but says `connection: close` leaves no connection idle,
    /// so the next line, within [`WARM_AFTER`], still warms one.
    #[tokio::test]
    async fn a_warming_told_to_close_leaves_the_next_line_its_warming() {
        let (url, connections, asked) = serving(vec![1, 0], false, true, None).await;
        let tts = Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap();
        assert_eq!(tts.warm().await.map(|(status, _)| status), Some(200));
        assert!(!tts.idle_known(), "its connection closed");
        assert_eq!(
            tts.warm().await.map(|(status, _)| status),
            Some(200),
            "the next line still warms"
        );
        assert_eq!(connections.load(Ordering::SeqCst), 2);
        assert!(asked.lock().unwrap().iter().all(|l| l.starts_with("get")));
    }

    /// With `LAMP_SECOND_SPEECH_WARM` off nothing warms, and the pieces make
    /// the connections they made before: a piece asked while another
    /// streams opens its own, and so does one after a cut-in.
    #[tokio::test]
    async fn without_warming_the_pieces_open_the_connections_they_did() {
        let hold = Arc::new(tokio::sync::Semaphore::new(0));
        let (url, connections, asked) = serving(vec![1, 0, 2, 0], false, false, Some(hold)).await;
        let tts = Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap();
        let streaming = tts.speak_for("The last answer.", turn(1)).await.unwrap();
        let first = tts.speak_for("Hello.", turn(2)).await.unwrap();
        assert!(!on_a_kept_connection(&streaming) && !on_a_kept_connection(&first));
        assert_eq!(connections.load(Ordering::SeqCst), 2);
        drop((streaming, first));
        let after_the_cut = tts.speak_for("Hello again.", turn(3)).await.unwrap();
        assert!(!on_a_kept_connection(&after_the_cut));
        assert_eq!(connections.load(Ordering::SeqCst), 3);
        assert!(asked.lock().unwrap().iter().all(|l| l.starts_with("post")));
    }

    /// At most one warming is in flight: a line begun while one hangs, with
    /// no idle connection known, sends no second.
    #[tokio::test]
    async fn at_most_one_warming_is_in_flight() {
        let (url, _, asked) = keeping(vec![5, 0], true).await;
        let tts = Arc::new(Tts::with(TtsProvider::ElevenLabs, &url, "k", None).unwrap());
        let warming = tokio::spawn({
            let tts = tts.clone();
            async move { tts.warm().await }
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while asked.lock().unwrap().is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the first warming went");
        assert_eq!(tts.warm().await, None, "one is in flight");
        assert_eq!(asked.lock().unwrap().len(), 1);
        warming.abort();
    }

    /// The piece's journal line has its times, connection and size, never
    /// its words.
    #[test]
    fn a_piece_is_journaled_without_its_words() {
        let piece = Piece {
            tally: Arc::default(),
            provider: "ElevenLabs",
            request: 2,
            index: 1,
            sent_ms: 2185,
            sent: Instant::now(),
            reused: true,
            ahead_ms: None,
            chars: 42,
        };
        assert_eq!(
            piece.told(690),
            "request 2 ElevenLabs piece 1: 42 chars, sent 2185 ms after the words were heard on a reused connection; its first audio byte 690 ms after it went"
        );
        // On the speech socket opened ahead (`LAMP_SECOND_SPEECH_SOCKET`).
        let on_socket = Piece {
            ahead_ms: Some(820),
            ..piece
        };
        assert_eq!(
            on_socket.told(410),
            "request 2 ElevenLabs piece 1: 42 chars, sent 2185 ms after the words were heard on the speech socket opened 820 ms before; its first audio byte 410 ms after it went"
        );
    }

    /// #1712, the owner: "use v4 turbo". The model set is asked for; absent,
    /// the provider's default.
    #[test]
    fn the_model_set_is_asked_for() {
        let turbo = request(
            TtsProvider::ElevenLabs,
            PROXY,
            "k",
            None,
            Some("eleven_v4_turbo"),
            "Hi.",
        )
        .unwrap()
        .unwrap();
        assert_eq!(turbo.body["model_id"], "eleven_v4_turbo");
        let flash = request(TtsProvider::ElevenLabs, PROXY, "k", None, None, "Hi.")
            .unwrap()
            .unwrap();
        assert_eq!(
            flash.body["model_id"],
            default_model(TtsProvider::ElevenLabs)
        );
        let openai = request(
            TtsProvider::OpenAi,
            PROXY,
            "k",
            None,
            Some("gpt-4o-mini-tts"),
            "Hi.",
        )
        .unwrap()
        .unwrap();
        assert_eq!(openai.body["model"], "gpt-4o-mini-tts");
    }

    const PROXY: &str = "https://campaign-api.autonomous.ai/api/v1/ai";

    #[test]
    fn openai_speech_is_asked_as_v1_did() {
        let request = request(
            TtsProvider::OpenAi,
            PROXY,
            "k",
            None,
            None,
            "Hi [laughs] there.",
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            request,
            Request {
                url: format!("{PROXY}/v1/audio/speech"),
                header: "authorization",
                value: "Bearer k".into(),
                body: json!({"model": "tts-1", "input": "Hi  there.", "voice": "nova",
                    "response_format": "pcm"}),
            }
        );
    }

    /// #2135's row: the speech socket is ElevenLabs' `stream-input`, under the
    /// proxy's `/ws/` with the key as a bearer (the backend on #1934), or on
    /// ElevenLabs' own API with `xi-api-key`; with the route's voice and
    /// model, and 24 kHz PCM as a piece over HTTP.
    #[test]
    fn the_speech_socket_is_elevenlabs_stream_input_through_the_proxys_ws_path() {
        let proxied = socket_request(PROXY, "k", None, Some("eleven_turbo_v2_5")).unwrap();
        assert_eq!(proxied.shape, SocketShape::StreamInput);
        assert_eq!(
            proxied.url,
            format!(
                "{}/v1/ws/elevenlabs/text-to-speech/21m00Tcm4TlvDq8ikWAM/stream-input?model_id=eleven_turbo_v2_5&output_format=pcm_24000&inactivity_timeout=60",
                PROXY.replacen("https://", "wss://", 1)
            )
        );
        assert_eq!(
            (proxied.header, proxied.value.as_str()),
            ("authorization", "Bearer k")
        );
        let direct =
            socket_request("https://api.elevenlabs.io/v1/", "k", Some("Ngan"), None).unwrap();
        assert_eq!(
            direct.url,
            "wss://api.elevenlabs.io/v1/text-to-speech/a3AkyqGG4v8Pg7SWQ0Y3/stream-input?model_id=eleven_flash_v2_5&output_format=pcm_24000&inactivity_timeout=60"
        );
        assert_eq!((direct.header, direct.value.as_str()), ("xi-api-key", "k"));
        for (voice, model) in [(Some("a/b"), None), (None, Some("m&x")), (None, Some(""))] {
            assert_eq!(socket_request(PROXY, "k", voice, model), Err(Error::Url));
        }
        assert_eq!(
            socket_request("http://tts.example.invalid", "k", None, None),
            Err(Error::Url)
        );
        // Its messages: the settings first, each piece made at once, and the end.
        assert_eq!(
            socket_opening(),
            json!({"text": " ", "voice_settings": {"speed": 1.0}})
        );
        assert_eq!(
            socket_piece(" Sure, [laughs] I can. "),
            Some(json!({"text": "Sure, [laughs] I can. ", "flush": true}))
        );
        assert_eq!(socket_piece(" [sighs] "), None);
        assert_eq!(socket_end(), json!({"text": ""}));
        assert_eq!(direct.opening(), socket_opening());
        assert_eq!(
            direct.messages(" Sure. "),
            Some(vec![json!({"text": "Sure. ", "flush": true}), socket_end()])
        );
    }

    /// #2602: only v4 Turbo takes ElevenLabs' dialogue endpoint, which
    /// serves it where `stream-input` closes with 4400; its URL asks for the
    /// model and 24 kHz PCM, with no voice in the path, and its messages
    /// name the voice: the opening's one voice, then the piece as an input,
    /// `flush` and `close_socket`.
    #[test]
    fn only_v4_turbo_takes_the_dialogue_endpoint_in_its_shape() {
        assert_eq!(socket_shape("eleven_v4_turbo"), SocketShape::Dialogue);
        for model in [
            "eleven_flash_v2_5",
            "eleven_turbo_v2_5",
            "eleven_v4",
            "eleven_v3",
        ] {
            assert_eq!(socket_shape(model), SocketShape::StreamInput, "{model}");
        }
        let proxied = socket_request(PROXY, "k", None, Some("eleven_v4_turbo")).unwrap();
        assert_eq!(
            proxied.url,
            format!(
                "{}/v1/ws/elevenlabs/text-to-dialogue/stream-input?model_id=eleven_v4_turbo&output_format=pcm_24000",
                PROXY.replacen("https://", "wss://", 1)
            )
        );
        assert_eq!(proxied.shape, SocketShape::Dialogue);
        let direct = socket_request(
            "https://api.elevenlabs.io/v1/",
            "k",
            Some("Ngan"),
            Some("eleven_v4_turbo"),
        )
        .unwrap();
        assert_eq!(
            direct.url,
            "wss://api.elevenlabs.io/v1/text-to-dialogue/stream-input?model_id=eleven_v4_turbo&output_format=pcm_24000"
        );
        assert_eq!(
            direct.opening(),
            json!({"voices": ["a3AkyqGG4v8Pg7SWQ0Y3"]})
        );
        assert_eq!(
            direct.messages(" Sure, [laughs] I can. "),
            Some(vec![
                json!({"inputs": [{"text": "Sure, [laughs] I can.", "voice_id": "a3AkyqGG4v8Pg7SWQ0Y3"}]}),
                json!({"flush": true}),
                json!({"close_socket": true}),
            ])
        );
        assert_eq!(direct.messages(" [sighs] "), None);
        // Its answers: `is_final`, and the turn's end read past.
        assert_eq!(
            socket_answer(r#"{"is_final":true}"#),
            Some(SocketAnswer {
                audio: Vec::new(),
                last: true
            })
        );
        assert_eq!(
            socket_answer(r#"{"is_final_audio_for_turn":true}"#),
            Some(SocketAnswer::default())
        );
    }

    /// A speech socket's answers: audio as base64, and the last one marked.
    #[test]
    fn a_speech_sockets_answers_carry_audio_and_say_the_last() {
        assert_eq!(
            socket_answer(r#"{"audio":"AQIDBA==","isFinal":false,"normalizedAlignment":null}"#),
            Some(SocketAnswer {
                audio: vec![1, 2, 3, 4],
                last: false
            })
        );
        assert_eq!(
            socket_answer(r#"{"audio":null,"isFinal":true}"#),
            Some(SocketAnswer {
                audio: Vec::new(),
                last: true
            })
        );
        for other in [
            r#"{"error":"quota","message":"no"}"#,
            r#"{"audio":"not base64!","isFinal":false}"#,
            r#"{"audio":7}"#,
            "not json",
        ] {
            assert_eq!(socket_answer(other), None, "{other}");
        }
    }

    #[test]
    fn elevenlabs_goes_through_the_proxy_path_unless_direct() {
        let proxied = request(TtsProvider::ElevenLabs, PROXY, "k", None, None, "Hi.")
            .unwrap()
            .unwrap();
        assert_eq!(
            proxied.url,
            format!(
                "{PROXY}/v1/elevenlabs/text-to-speech/21m00Tcm4TlvDq8ikWAM/stream?output_format=pcm_24000"
            )
        );
        assert_eq!(
            (proxied.header, proxied.value.as_str()),
            ("xi-api-key", "k")
        );
        assert_eq!(
            proxied.body,
            json!({"text": "Hi.", "model_id": "eleven_flash_v2_5", "voice_settings": {"speed": 1.0}})
        );
        let direct = request(
            TtsProvider::ElevenLabs,
            "https://api.elevenlabs.io/v1/",
            "k",
            Some("Ngan"),
            None,
            "Hello.",
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            direct.url,
            "https://api.elevenlabs.io/v1/text-to-speech/a3AkyqGG4v8Pg7SWQ0Y3/stream?output_format=pcm_24000"
        );
        let by_id = request(
            TtsProvider::ElevenLabs,
            PROXY,
            "k",
            Some("abc123"),
            None,
            "Hi.",
        )
        .unwrap()
        .unwrap();
        assert!(by_id.url.contains("/text-to-speech/abc123/stream"));
        assert_eq!(
            request(
                TtsProvider::ElevenLabs,
                PROXY,
                "k",
                Some("../x"),
                None,
                "Hi."
            )
            .unwrap_err(),
            Error::Url
        );
    }

    #[test]
    fn only_tags_ask_for_nothing() {
        for provider in [TtsProvider::OpenAi, TtsProvider::ElevenLabs] {
            assert_eq!(
                request(provider, PROXY, "k", None, None, " [sigh] ").unwrap(),
                None
            );
        }
    }

    #[test]
    fn samples_cut_mid_byte_come_out_whole_in_packets() {
        let samples: Vec<i16> = (0..2000).map(|i| (i * 31 - 20_000) as i16).collect();
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut packets = Packets::default();
        let mut out = Vec::new();
        // Odd and even piece sizes, one byte at a time included.
        for piece in bytes[..3].chunks(1).chain(bytes[3..].chunks(777)) {
            out.extend(packets.push(piece));
        }
        out.extend(packets.finish());
        assert_eq!(out.iter().map(Vec::len).collect::<Vec<_>>(), [960, 960, 80]);
        assert_eq!(out.concat(), samples);
    }

    #[test]
    fn a_dangling_byte_is_dropped() {
        let mut packets = Packets::default();
        assert!(packets.push(&[1, 0, 9]).is_empty());
        assert_eq!(packets.finish(), Some(vec![1]));
    }
}

#[cfg(test)]
#[path = "tts/authority_tests.rs"]
mod authority_tests;
