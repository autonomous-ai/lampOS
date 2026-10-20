//! Streamed speech as 24 kHz mono 16-bit PCM from OpenAI or ElevenLabs, as
//! V1's `hal/drivers/voice/tts/{openai,elevenlabs}.py` ask for it.
use super::{
    CONNECT, Error, Result, STALL, chunk, http_client, local_socket, openai_base, post, warm_get,
};
use crate::second_config::{SecondConfig, TtsProvider};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{Arc, Mutex},
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
                "voice": voice.unwrap_or(OPENAI_VOICE),
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
#[derive(Debug, PartialEq)]
pub(crate) struct SocketRequest {
    pub url: String,
    pub header: &'static str,
    pub value: String,
}

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
    Ok(SocketRequest {
        url: format!(
            "wss://{root}/text-to-speech/{id}/stream-input?model_id={model}&output_format=pcm_24000"
        ),
        header,
        value,
    })
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
    let last = message.get("isFinal").and_then(Value::as_bool) == Some(true);
    (last || !audio.is_empty()).then_some(SocketAnswer { audio, last })
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
    /// them came on a connection reused from the pool.
    sockets: Mutex<VecDeque<SocketAddr>>,
    tally: Arc<Mutex<Tally>>,
    /// The speech socket opened ahead for a request
    /// (`LAMP_SECOND_SPEECH_SOCKET=on`, [`Tts::open_ahead`]).
    ahead: Mutex<Option<Ahead>>,
}

/// A speech socket, ElevenLabs' `stream-input`.
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// The speech socket opened ahead of a request's answer (#2135's row; the
/// first builder on #1722, 20 Oct 10:37): opened as its chat is asked, so
/// its first piece goes to a socket already open. A piece that finds it
/// still opening goes over HTTP as before.
enum Ahead {
    Opening(u64),
    /// Open for the request since then, told the voice's settings.
    Open(u64, Box<Socket>, Instant),
}

impl Tts {
    pub fn new(config: &SecondConfig) -> Result<Self> {
        Ok(Self {
            model: config.tts_model.clone(),
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
            sockets: Mutex::new(VecDeque::with_capacity(SOCKETS)),
            tally: Arc::default(),
            ahead: Mutex::new(None),
        })
    }

    /// With `LAMP_SECOND_SPEECH_SOCKET=on`, a speech socket opened now for
    /// `request`'s answer, its voice's settings sent, for the first of its
    /// pieces that finds it open ([`Self::speak_for`]). ElevenLabs only; one
    /// opened before, for an older request, is closed. Nothing on failure:
    /// the pieces go over HTTP as before.
    pub async fn open_ahead(&self, request: u64) {
        if self.provider != TtsProvider::ElevenLabs {
            return;
        }
        let Ok(socket) = socket_request(
            &self.base_url,
            &self.key,
            self.voice.as_deref(),
            self.model.as_deref(),
        ) else {
            return;
        };
        self.open_ahead_on(request, &socket).await;
    }

    async fn open_ahead_on(&self, request: u64, socket: &SocketRequest) {
        if let Ok(mut ahead) = self.ahead.lock() {
            *ahead = Some(Ahead::Opening(request));
        }
        let from = Instant::now();
        let opened = open_socket(socket).await;
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

    /// `text` as `turn`'s piece on `socket`, opened at `opened`; `None` when
    /// nothing in it would be spoken. It is handed on once its first audio
    /// came, so a socket closed or failed before it is told to the caller,
    /// which sends the piece over HTTP instead.
    async fn speak_on(
        &self,
        mut socket: Socket,
        opened: Instant,
        text: &str,
        turn: Turn,
    ) -> Result<Option<TtsStream>> {
        let Some(piece) = socket_piece(text) else {
            return Ok(None);
        };
        let sent = Instant::now();
        for message in [piece, socket_end()] {
            socket
                .send(Frame::Text(message.to_string().into()))
                .await
                .map_err(|_| Error::Transport("speech socket"))?;
        }
        if let Ok(mut used) = self.used.lock() {
            *used = Some(sent);
        }
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
        if !stream.first_audio().await? {
            return Err(Error::Transport("speech socket closed before its audio"));
        }
        Ok(Some(stream))
    }

    /// Whether `socket` was seen before, remembering it either way.
    // ponytail: a new connection the OS happens to give a recent local port
    // reads as reused; ports are rarely handed out again within minutes.
    fn seen(&self, socket: Option<SocketAddr>) -> bool {
        let (Some(socket), Ok(mut sockets)) = (socket, self.sockets.lock()) else {
            return false;
        };
        if sockets.contains(&socket) {
            return true;
        }
        if sockets.len() == SOCKETS {
            sockets.pop_front();
        }
        sockets.push_back(socket);
        false
    }

    /// The connection opened ahead, as a line begins, when no piece or
    /// warming went in the last [`WARM_AFTER`] (`LAMP_SECOND_SPEECH_WARM=on`,
    /// #1712): a `GET` of the provider's `models` with the key, read whole,
    /// so the answer's first piece goes on a connection already made. No
    /// text is sent, so no characters are billed (ElevenLabs bills the text
    /// of speech requests; `GET /v1/models` lists models). Its status and how
    /// long it took; `None` when not due, or when it failed (the piece then
    /// connects as before).
    pub async fn warm(&self) -> Option<(u16, Duration)> {
        let models = format!(
            "{}/models",
            service_base(self.provider, &self.base_url).ok()?
        );
        let (header, value) = auth(self.provider, &self.key);
        let (status, took, socket) = warm_get(
            &self.client,
            &self.used,
            WARM_AFTER,
            &models,
            (header, &value),
            WARM_BYTES,
        )
        .await?;
        self.seen(socket);
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
        if let Some(turn) = turn
            && let Some((socket, opened)) = self.take_ahead(turn.request)
        {
            match self.speak_on(socket, opened, text, turn).await {
                Ok(Some(stream)) => return Ok(stream),
                Ok(None) => return Ok(TtsStream::default()),
                Err(error) => eprintln!(
                    "lamp-voice second: request {}: the speech socket failed before its first audio ({error}); the piece goes over HTTP",
                    turn.request
                ),
            }
        }
        let Some(request) = request(
            self.provider,
            &self.base_url,
            &self.key,
            self.voice.as_deref(),
            self.model.as_deref(),
            text,
        )?
        else {
            return Ok(TtsStream::default());
        };
        let sent = Instant::now();
        if let Ok(mut used) = self.used.lock() {
            *used = Some(sent);
        }
        let index = turn.map(|turn| self.asked(turn.request));
        let response = post(
            &self.client,
            &request.url,
            (request.header, &request.value),
            &request.body,
        )
        .await?;
        let reused = self.seen(local_socket(&response));
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
        Ok(TtsStream {
            source: Some(Source::Http(response)),
            piece,
            ..TtsStream::default()
        })
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
            Some(Source::Socket(socket)) => socket_audio(socket).await?,
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
                self.ready.extend(self.packets.finish().map(louder));
            }
        }
        Ok(true)
    }
}

/// A speech socket opened with `socket`'s key, its voice's settings sent.
/// `wss` only, but `ws` in tests.
async fn open_socket(socket: &SocketRequest) -> Result<Socket> {
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
        CONNECT,
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
        .send(Frame::Text(socket_opening().to_string().into()))
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
            None | Some(Ok(Frame::Close(_))) => return Ok(None),
            Some(Err(_)) => return Err(Error::Transport("speech socket")),
            Some(Ok(Frame::Text(text))) => {
                let answer =
                    socket_answer(text.as_str()).ok_or(Error::Protocol("speech socket answer"))?;
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

    /// A local keep-alive HTTP/1.1 server: `GET`s are answered with a short
    /// models list (or never, with `hang_get`), `POST`s with `pcm`. What it
    /// gives back: its address, the connections it accepted and each
    /// request's first line.
    async fn keeping(
        pcm: Vec<u8>,
        hang_get: bool,
    ) -> (String, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let connections = Arc::new(AtomicUsize::new(0));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let (counted, seen) = (connections.clone(), asked.clone());
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                counted.fetch_add(1, Ordering::SeqCst);
                let (seen, pcm) = (seen.clone(), pcm.clone());
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
                        let answer =
                            format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n", body.len());
                        if socket.write_all(answer.as_bytes()).await.is_err()
                            || socket.write_all(&body).await.is_err()
                        {
                            return;
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
        }
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
        let proxied = socket_request(PROXY, "k", None, Some("eleven_v4_turbo")).unwrap();
        assert_eq!(
            proxied.url,
            format!(
                "{}/v1/ws/elevenlabs/text-to-speech/21m00Tcm4TlvDq8ikWAM/stream-input?model_id=eleven_v4_turbo&output_format=pcm_24000",
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
            "wss://api.elevenlabs.io/v1/text-to-speech/a3AkyqGG4v8Pg7SWQ0Y3/stream-input?model_id=eleven_flash_v2_5&output_format=pcm_24000"
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
