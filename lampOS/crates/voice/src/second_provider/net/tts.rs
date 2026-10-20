//! Streamed speech as 24 kHz mono 16-bit PCM from OpenAI or ElevenLabs, as
//! V1's `hal/drivers/voice/tts/{openai,elevenlabs}.py` ask for it.
use super::{Error, Result, chunk, http_client, local_socket, openai_base, post, warm_get};
use crate::second_config::{SecondConfig, TtsProvider};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
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
            let voice = voice_name(provider, voice);
            let id = ELEVENLABS_VOICES
                .iter()
                .find(|(name, _)| *name == voice)
                .map_or(voice, |(_, id)| id);
            if !id.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(Error::Url);
            }
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

/// The provider's API root: OpenAI's `/v1`, or ElevenLabs' (on the
/// Autonomous proxy, its `/elevenlabs` path).
fn service_base(provider: TtsProvider, base_url: &str) -> Result<String> {
    let base = openai_base(base_url);
    if provider == TtsProvider::OpenAi {
        return Ok(base);
    }
    let host = reqwest::Url::parse(&base).map_err(|_| Error::Url)?;
    let direct = host
        .host_str()
        .is_some_and(|h| ELEVENLABS_HOSTS.iter().any(|d| h.eq_ignore_ascii_case(d)));
    Ok(if direct {
        base
    } else {
        format!("{base}/elevenlabs")
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
        })
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
            chars: text.chars().count(),
        });
        Ok(TtsStream {
            response: Some(response),
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
    chars: usize,
}

impl Piece {
    /// The piece in the journal's words, its first audio byte `first_ms`
    /// after it went: never its words.
    fn told(&self, first_ms: u128) -> String {
        format!(
            "request {} {} piece {}: {} chars, sent {} ms after the words were heard on a {} connection; its first audio byte {first_ms} ms after it went",
            self.request,
            self.provider,
            self.index,
            self.chars,
            self.sent_ms,
            if self.reused { "reused" } else { "new" },
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
    response: Option<reqwest::Response>,
    packets: Packets,
    ready: VecDeque<Vec<i16>>,
    /// Journaled at the first audio byte.
    piece: Option<Piece>,
}

impl TtsStream {
    /// The next packet of at most [`PACKET_SAMPLES`]; `None` at the end.
    pub async fn next(&mut self) -> Result<Option<Vec<i16>>> {
        loop {
            if let Some(packet) = self.ready.pop_front() {
                return Ok(Some(packet));
            }
            let Some(response) = &mut self.response else {
                return Ok(None);
            };
            match chunk(response).await? {
                Some(bytes) => {
                    if let Some(piece) = self.piece.take() {
                        piece.began();
                    }
                    self.ready
                        .extend(self.packets.push(&bytes).into_iter().map(louder));
                }
                None => {
                    self.response = None;
                    self.ready.extend(self.packets.finish().map(louder));
                }
            }
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
            chars: 42,
        };
        assert_eq!(
            piece.told(690),
            "request 2 ElevenLabs piece 1: 42 chars, sent 2185 ms after the words were heard on a reused connection; its first audio byte 690 ms after it went"
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
