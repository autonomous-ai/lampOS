//! Streaming speech-to-text over Deepgram's websocket protocol, straight to
//! Deepgram or through the Autonomous proxy, as V1's
//! `hal/drivers/voice/stt/{deepgram,autonomous}.py` do with `nova-3-general`.
use super::{CONNECT, Error, Result, openai_base, tls};
use crate::second_config::{SecondConfig, Stt};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{sync::mpsc, task::JoinHandle, time::Instant};
use tokio_tungstenite::{
    Connector, connect_async_tls_with_config,
    tungstenite::{Message, client::IntoClientRequest, http::HeaderValue},
};

mod flux;

const MODEL: &str = "nova-3-general";
const DEEPGRAM: &str = "wss://api.deepgram.com/v1/listen";
/// Deepgram closes a socket that hears nothing for 10 s.
const KEEP_ALIVE: Duration = Duration::from_secs(5);
/// V1 waits as long for the final words after `CloseStream`.
const FINISH: Duration = Duration::from_secs(15);
/// How long the end of the line waits for `Finalize`'s final words before
/// falling back to `CloseStream` (#779): on lamp-52e6 `CloseStream`'s took
/// 0.56 to 0.66 s, so a proxy that passes `Finalize` answers well within.
const FINALIZE: Duration = Duration::from_secs(1);
/// About 6 s of 20 ms frames waiting to be written.
const QUEUE: usize = 300;
const MAX_MESSAGE: usize = 1 << 20;

/// V1's query for nova models, in its order, with the lamp's name as a
/// key term as V1 boosts it: without it, on lamp-52e6 on 2026-10-15, "Hey
/// Lamp" came back as "Palin", "Palan" and "Len", and the model took the
/// question for other people's talk.
// ponytail: the name is the character's own ("Lamp"); a lamp renamed
// would pass its name here.
/// The unit's own key terms follow the name ([`SecondConfig::stt_key_terms`]).
fn query<'a>(language: Option<&'a str>, key_terms: &'a [String]) -> Vec<(&'static str, &'a str)> {
    let mut query = vec![
        ("model", MODEL),
        ("encoding", "linear16"),
        ("sample_rate", "16000"),
        ("channels", "1"),
        ("language", language.unwrap_or("en")),
        ("smart_format", "true"),
        ("interim_results", "true"),
        // V1 waited for 1.5 s of silence; the coordinator ends a line
        // after 0.4 to 1.5 s, so Deepgram's `speech_final` came after the
        // End it could have spared (#779). At 300 ms it is usually there.
        ("endpointing", "300"),
        ("vad_events", "true"),
        ("keyterm", "Lamp"),
    ];
    query.extend(key_terms.iter().map(|term| ("keyterm", term.as_str())));
    query
}

/// The websocket URL: the Autonomous proxy at `proxy`, else Deepgram.
pub(crate) fn url(
    proxy: Option<&str>,
    language: Option<&str>,
    key_terms: &[String],
) -> Result<reqwest::Url> {
    let base = proxy.map_or_else(|| DEEPGRAM.to_owned(), proxied);
    secure(
        reqwest::Url::parse_with_params(&base, query(language, key_terms))
            .map_err(|_| Error::Url)?,
    )
}

/// The Autonomous proxy's speech-to-text socket at `base_url`. It passes
/// the query's `model` on to Deepgram, Flux's included (the first builder
/// on lamp-4ace, 19 Oct 15:54).
fn proxied(base_url: &str) -> String {
    let base = openai_base(base_url)
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    format!("{base}/ws/audio/transcriptions")
}

/// `url` when it is a secure websocket; a plain one only in tests.
fn secure(url: reqwest::Url) -> Result<reqwest::Url> {
    match url.scheme() {
        "wss" => Ok(url),
        "ws" if cfg!(test) => Ok(url),
        _ => Err(Error::Url),
    }
}

/// Audio seconds a final segment may end short of the audio sent and still
/// cover it: a 20 ms frame and a little rounding.
const COVERED: f64 = 0.05;

/// A block is the line's sound, not its quiet, when its level is within
/// this many dB of the line's loudest block so far: a bar relative to the
/// line, so a far voice's soft last word still counts as sound (Jarvis on
/// #1993). After the canceller, talk near the lamp is at -21 to -31 dBFS
/// and the quiet between sounds near -75 dBFS (the first builder on #1712,
/// 15:42).
const SOUND_UNDER_PEAK_DB: f64 = 30.0;
/// And over this RMS, about -60 dBFS, however quiet the line.
const SOUND_FLOOR_RMS: f64 = 32.8;

/// The RMS of a block of little-endian 16-bit samples; 0 for none.
fn level(bytes: &[u8]) -> f64 {
    let samples = bytes.len() / 2;
    if samples == 0 {
        return 0.0;
    }
    let energy: f64 = bytes
        .chunks_exact(2)
        .map(|pair| f64::from(i16::from_le_bytes([pair[0], pair[1]])).powi(2))
        .sum();
    (energy / samples as f64).sqrt()
}

/// Where a line's sound ends, in seconds of the audio sent: the end of its
/// last block within [`SOUND_UNDER_PEAK_DB`] of the loudest block up to it
/// and over [`SOUND_FLOOR_RMS`]. A level that stays up after the words, a
/// noisy room's, keeps the end at the line's end, so such a line waits for
/// `CloseStream` as before.
#[derive(Debug, Default)]
struct LineSound {
    peak: f64,
    to: f64,
}

impl LineSound {
    /// A block sent, ending `ends_at` seconds into the audio.
    fn sent(&mut self, bytes: &[u8], ends_at: f64) {
        let level = level(bytes);
        self.peak = self.peak.max(level);
        if level > SOUND_FLOOR_RMS && level >= self.peak * 10f64.powf(-SOUND_UNDER_PEAK_DB / 20.0) {
            self.to = ends_at;
        }
    }
}

/// One `Results` message: the best alternative's words (empty for
/// silence), and where in the audio the segment ends, in seconds.
#[derive(Debug, PartialEq)]
pub(crate) struct Heard {
    pub transcript: String,
    pub is_final: bool,
    pub speech_final: bool,
    /// The answer to a `Finalize`: the words it asked for are all here.
    pub from_finalize: bool,
    pub end: f64,
}

/// `None` for any other message (metadata, `SpeechStarted`,
/// `UtteranceEnd`).
pub(crate) fn heard(text: &str) -> Option<Heard> {
    #[derive(Deserialize)]
    struct Results {
        #[serde(rename = "type")]
        kind: String,
        #[serde(default)]
        channel: Option<Channel>,
        #[serde(default)]
        is_final: bool,
        #[serde(default)]
        speech_final: bool,
        #[serde(default)]
        from_finalize: bool,
        #[serde(default)]
        start: f64,
        #[serde(default)]
        duration: f64,
    }
    #[derive(Deserialize)]
    struct Channel {
        alternatives: Vec<Alternative>,
    }
    #[derive(Deserialize)]
    struct Alternative {
        transcript: String,
    }
    let results: Results = serde_json::from_str(text).ok()?;
    if results.kind != "Results" {
        return None;
    }
    let end = results.start + results.duration;
    let transcript = results
        .channel?
        .alternatives
        .into_iter()
        .next()?
        .transcript
        .trim()
        .to_owned();
    Some(Heard {
        transcript,
        is_final: results.is_final,
        speech_final: results.speech_final,
        from_finalize: results.from_finalize,
        end,
    })
}

/// For a `SpeechStarted` message (someone is talking again), where in the
/// audio the speech began, in seconds.
pub(crate) fn speech_started(text: &str) -> Option<f64> {
    let message = serde_json::from_str::<serde_json::Value>(text).ok()?;
    (message["type"] == "SpeechStarted").then(|| message["timestamp"].as_f64().unwrap_or(0.0))
}

/// What was heard since the last final, and where in the audio, in seconds.
#[derive(Clone, Copy)]
enum Pending {
    /// An interim result with words, ending there.
    Interim(f64),
    /// A `SpeechStarted`, starting there.
    SpeechStarted(f64),
}

/// The final segments in the order heard, joined by spaces. Interim
/// results are previews of a segment and are dropped.
#[derive(Default)]
pub(crate) struct Transcript {
    segments: Vec<String>,
    /// Where the last final segment ends in the audio, in seconds; `None`
    /// before any.
    covered: Option<f64>,
    /// The last final segment closed an utterance (`speech_final`).
    ended: bool,
    /// Words (an interim result or `SpeechStarted`) since the last final.
    pending: Option<Pending>,
    /// The last interim result's words since the last final, kept for a
    /// line whose finals never came with them (#1712).
    interim: Option<String>,
}

impl Transcript {
    pub fn add(&mut self, heard: Heard) {
        if !heard.is_final {
            if !heard.transcript.is_empty() {
                self.pending = Some(Pending::Interim(heard.end));
                self.interim = Some(heard.transcript);
            }
            return;
        }
        self.interim = None;
        self.covered = Some(heard.end);
        self.ended = heard.speech_final;
        self.pending = None;
        if !heard.transcript.is_empty() {
            self.segments.push(heard.transcript);
        }
    }
    pub fn speech_started(&mut self, at: f64) {
        self.pending = Some(Pending::SpeechStarted(at));
    }
    /// Nothing more is coming for the `sent` seconds of audio: final
    /// segments cover all of it, and nobody has been heard since. Then the
    /// words need not wait for `Finalize` or `CloseStream`. An earlier
    /// `speech_final` is only a pause: audio sent after it may still have
    /// words on their way (#878).
    pub fn settled(&self, sent: f64) -> bool {
        self.pending.is_none()
            && self
                .covered
                .is_some_and(|covered| covered + COVERED >= sent)
    }

    /// Whether the finals cover the line's sound, `sound_to` seconds in
    /// ([`LineSound`]), to within one block ([`COVERED`]), with nothing
    /// pending: what is not covered is the line's quiet end. On lamp-4ace on
    /// 19 Oct `Finalize`'s answer reached about 0.48 s short of the audio
    /// sent on 114 of 131 lines, so their words waited for `CloseStream`,
    /// 652 ms after the line's end against 280 ms (#1712, the first builder
    /// at 23:46). Any sound past the answer is a word left, however short
    /// or soft (Jarvis on #1993: a word loud for 0.10 s after a 40 ms gap,
    /// an "on" at -48 dBFS).
    pub fn covers_the_sound(&self, sound_to: f64) -> bool {
        self.pending.is_none()
            && self
                .covered
                .is_some_and(|covered| covered + COVERED >= sound_to)
    }
    /// What [`Self::settled`] decided on for `sent` seconds, in ms and
    /// without words: the journal's account of the end of the line (#779).
    fn at_end(&self, sent: f64) -> String {
        let ms = |seconds: f64| (seconds * 1000.0).round() as i64;
        let covered = self
            .covered
            .map_or("none".to_owned(), |covered| ms(covered).to_string());
        let pending = match self.pending {
            None => "none".to_owned(),
            Some(Pending::Interim(at)) => format!(
                "interim {} ms after the last final",
                ms(at - self.covered.unwrap_or(0.0))
            ),
            Some(Pending::SpeechStarted(at)) => format!(
                "speech_started {} ms after the last final",
                ms(at - self.covered.unwrap_or(0.0))
            ),
        };
        format!(
            "settled={}, speech_final={}, finals cover {covered} of {} ms sent, pending={pending}",
            self.settled(sent),
            self.ended,
            ms(sent)
        )
    }
    /// The words at the line's end: the final segments, then the last
    /// interim result's words if no final came after them, and whether
    /// those were kept. On lamp-4ace on 19 Oct (#1712) a line came back
    /// with no words; kept, an interim's words are not lost to an empty
    /// final.
    pub fn words(&self) -> (String, bool) {
        let mut words = self.segments.clone();
        let interim = self
            .interim
            .clone()
            .filter(|_| matches!(self.pending, Some(Pending::Interim(_))));
        let kept = interim.is_some();
        words.extend(interim);
        (words.join(" "), kept)
    }
}

/// What a stream's service sent back, by kind: for telling a service that
/// said nothing from one whose results held no words (#1047, the first
/// builder at 15:29).
#[derive(Debug, Default)]
pub struct Replies {
    with_words: AtomicUsize,
    without_words: AtomicUsize,
    speech_started: AtomicUsize,
    other: AtomicUsize,
    /// How the stream ended, once it has: the service's close code and
    /// reason, the socket ending, or a failure (#1047, the first builder at
    /// 20:58).
    ended: std::sync::Mutex<Option<String>>,
}

impl Replies {
    fn count(&self, kind: &AtomicUsize) {
        kind.fetch_add(1, Ordering::Relaxed);
    }

    fn end(&self, how: String) {
        if let Ok(mut ended) = self.ended.lock() {
            ended.get_or_insert(how);
        }
    }

    /// How the stream ended, if it has.
    pub fn ended(&self) -> Option<String> {
        self.ended.lock().ok().and_then(|ended| ended.clone())
    }

    /// The counts, and how the stream ended if it has, for a file
    /// (`stop_words::COUNTS_ENVIRONMENT`); no words of what was heard.
    pub fn counts(&self) -> serde_json::Value {
        let read = |kind: &AtomicUsize| kind.load(Ordering::Relaxed);
        serde_json::json!({
            "results_with_words": read(&self.with_words),
            "results_without_words": read(&self.without_words),
            "speech_starts": read(&self.speech_started),
            "other_messages": read(&self.other),
            "ended": self.ended(),
        })
    }

    /// The results with and without words, for the line's end in the
    /// journal (#1712: whether the service sent any words at all).
    fn results_told(&self) -> String {
        format!(
            ", results {} with words, {} without",
            self.with_words.load(Ordering::Relaxed),
            self.without_words.load(Ordering::Relaxed)
        )
    }

    /// The counts, in words, and how the stream ended if it has; no words
    /// of what was heard.
    pub fn told(&self) -> String {
        let read = |kind: &AtomicUsize| kind.load(Ordering::Relaxed);
        format!(
            "{} results with words, {} without, {} speech starts, {} other messages{}",
            read(&self.with_words),
            read(&self.without_words),
            read(&self.speech_started),
            read(&self.other),
            self.ended()
                .map_or(String::new(), |how| format!("; the stream {how}"))
        )
    }
}

/// What the stream's task writes, in the order queued.
enum Outgoing {
    Audio(Vec<u8>),
    /// `Finalize` with the stream kept open: the words of what was sent so
    /// far, then more audio ([`SttStream::flush`]).
    Flush,
}

/// One utterance's stream. The socket is owned by a task that writes the
/// queued audio, keeps an idle socket alive and collects the transcript.
pub struct SttStream {
    /// `None` once finishing: the task then sends `CloseStream`.
    audio: Option<mpsc::Sender<Outgoing>>,
    /// The words, and the account of the end of the line once it came.
    task: JoinHandle<Result<(String, Option<String>)>>,
    /// When the first words came back, interim or final (#828).
    words_at: Arc<OnceLock<Instant>>,
    /// Each result's words as they come, interim and final, until taken
    /// ([`Self::take_words`], `LAMP_STOP_WORDS=heard-always`, #828).
    words: Option<mpsc::UnboundedReceiver<String>>,
    /// What the service has sent back so far, by kind.
    replies: Arc<Replies>,
    /// The words a line has before they are final, until taken
    /// ([`Self::take_early`]).
    early: Option<mpsc::UnboundedReceiver<Early>>,
    /// Set by a request's [`Self::finish`]: its words may be `Finalize`'s
    /// answer at once when what that leaves is quiet
    /// ([`Transcript::covers_the_sound`]). A stop word's stream
    /// ([`Self::finished`]) leaves it unset and waits for `CloseStream`:
    /// over the lamp the canceller can take a stop word under any bar, 51 of
    /// 88 with nothing over -28 dBFS left (#828).
    quiet_end: Arc<AtomicBool>,
}

/// Words a stream has before they are final, for a chat begun on them
/// (`LAMP_SECOND_PREPARED`, #1712, the first builder at 17:02): with
/// Nova-3, at the line's end, the finals and the last interim's words while
/// `Finalize`'s answer is still to come.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Early {
    /// The words so far, likely the line's.
    Words(String),
    /// The person went on: the words given are not the line's.
    Resumed,
}

impl SttStream {
    pub async fn connect(config: &SecondConfig) -> Result<Self> {
        let (proxy, key) = match &config.stt {
            Stt::Deepgram { key } => (None, key),
            Stt::Autonomous { base_url, key } => (Some(base_url.as_str()), key),
        };
        let key_terms = super::key_terms::listening(&config.stt_key_terms);
        // `LAMP_SECOND_FLUX=on`, in English.
        if flux::chosen(config) {
            return Ok(Self::start_flux(
                socket(flux::url(proxy, &key_terms, flux::eager())?, key.expose()).await?,
            ));
        }
        Self::open(
            url(proxy, config.stt_language.as_deref(), &key_terms)?,
            key.expose(),
        )
        .await
    }

    async fn open(url: reqwest::Url, key: &str) -> Result<Self> {
        Ok(Self::start(socket(url, key).await?))
    }
}

/// The service's websocket at `url`, signed with `key`.
async fn socket(
    url: reqwest::Url,
    key: &str,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
> {
    let mut request = url.as_str().into_client_request().map_err(|_| Error::Url)?;
    let mut auth =
        HeaderValue::from_str(&format!("Token {key}")).map_err(|_| Error::Protocol("key"))?;
    auth.set_sensitive(true);
    request.headers_mut().insert("authorization", auth);
    let socket_config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE));
    let (socket, _) = tokio::time::timeout(
        CONNECT,
        connect_async_tls_with_config(
            request,
            Some(socket_config),
            true,
            Some(Connector::Rustls(Arc::new(tls()?))),
        ),
    )
    .await
    .map_err(|_| Error::Timeout("speech-to-text connect"))?
    .map_err(|error| match error {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            Error::Status(response.status().as_u16())
        }
        _ => Error::Transport("speech-to-text connect"),
    })?;
    Ok(socket)
}

impl SttStream {
    /// A stream on `socket`, for tests of what is sent on it (#970).
    #[cfg(test)]
    pub(crate) fn on<S>(socket: tokio_tungstenite::WebSocketStream<S>) -> Self
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
    {
        Self::start(socket)
    }

    fn start<S>(socket: tokio_tungstenite::WebSocketStream<S>) -> Self
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
    {
        Self::start_on(socket, false)
    }

    /// A stream on Flux's socket ([`flux`]).
    fn start_flux<S>(socket: tokio_tungstenite::WebSocketStream<S>) -> Self
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
    {
        Self::start_on(socket, true)
    }

    fn start_on<S>(socket: tokio_tungstenite::WebSocketStream<S>, on_flux: bool) -> Self
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin + 'static,
    {
        let (audio, queued) = mpsc::channel(QUEUE);
        let words_at = Arc::new(OnceLock::new());
        let (said, words) = mpsc::unbounded_channel();
        let (early_words, early) = mpsc::unbounded_channel();
        let replies = Arc::new(Replies::default());
        let quiet_end = Arc::new(AtomicBool::new(false));
        let task = if on_flux {
            tokio::spawn(flux::run(
                socket,
                queued,
                Arc::clone(&words_at),
                (said, early_words),
                Arc::clone(&replies),
            ))
        } else {
            tokio::spawn(run(
                socket,
                queued,
                Arc::clone(&words_at),
                (said, early_words),
                Arc::clone(&replies),
                Arc::clone(&quiet_end),
            ))
        };
        Self {
            audio: Some(audio),
            task,
            words_at,
            words: Some(words),
            replies,
            early: Some(early),
            quiet_end,
        }
    }

    /// Each result's words as they come back, interim and final, in order,
    /// for one taker: a stop said over the lamp is heard as it is said, not
    /// when the stream ends (#828).
    pub fn take_words(&mut self) -> Option<mpsc::UnboundedReceiver<String>> {
        self.words.take()
    }

    /// The words the line has before they are final ([`Early`]), for one
    /// taker.
    pub fn take_early(&mut self) -> Option<mpsc::UnboundedReceiver<Early>> {
        self.early.take()
    }

    /// What the service has sent back on this stream so far, by kind.
    pub fn replies(&self) -> Arc<Replies> {
        Arc::clone(&self.replies)
    }

    /// When the service first gave back words for this stream, interim or
    /// final, once it has; it stays readable after [`Self::finish`]. For
    /// measuring how soon a word said can be known (#828).
    pub fn first_words(&self) -> Arc<OnceLock<Instant>> {
        Arc::clone(&self.words_at)
    }

    /// Whether its socket is still there to take audio: not once its task
    /// has ended (the service or the relay closed it).
    pub fn still_open(&self) -> bool {
        self.audio.as_ref().is_some_and(|audio| !audio.is_closed()) && !self.task.is_finished()
    }

    /// Queue 16 kHz mono samples. Fails when the socket has gone or the
    /// queue is full (the service stopped reading); nothing waits.
    pub fn send(&self, samples: &[i16]) -> Result<()> {
        let bytes = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        self.queue(Outgoing::Audio(bytes))
    }

    /// Ask for the words of everything sent so far, and keep the stream
    /// open for more (#1047, the first builder at 16:59: through the
    /// relay, a short "Stop." gets no interim result at all, and its words
    /// come only once `Finalize` asks for them, 269 ms after its audio).
    pub fn flush(&self) -> Result<()> {
        self.queue(Outgoing::Flush)
    }

    fn queue(&self, outgoing: Outgoing) -> Result<()> {
        let audio = self
            .audio
            .as_ref()
            .ok_or(Error::Transport("speech-to-text stream"))?;
        audio.try_send(outgoing).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => Error::Protocol("speech-to-text backlog"),
            mpsc::error::TrySendError::Closed(_) => Error::Transport("speech-to-text stream"),
        })
    }

    /// Ask for the last words and wait, bounded, for the service to close.
    /// The final segments, joined; empty when nothing was heard.
    /// Dropping the stream instead abandons it at once. Journals, for
    /// `request`, what the words' state was when the line ended.
    pub async fn finish(self, request: u64) -> Result<String> {
        let (words, at_end) = self.finish_told(true).await?;
        if let Some(at_end) = at_end {
            eprintln!("lamp-voice second: request {request} at the line's end: {at_end}");
        }
        Ok(words)
    }

    /// The same, without the journal's line: for a stop word's words
    /// ([`crate::stop_words`], #828), which are not a request. They wait for
    /// `CloseStream` when `Finalize`'s answer is short ([`Self::quiet_end`]).
    pub async fn finished(self) -> Result<String> {
        self.finish_told(false).await.map(|(words, _)| words)
    }

    /// The words, and the journal's account of the end of the line;
    /// `quiet_end` for a request's line ([`Self::quiet_end`]).
    async fn finish_told(mut self, quiet_end: bool) -> Result<(String, Option<String>)> {
        self.quiet_end.store(quiet_end, Ordering::Release);
        self.audio = None;
        match tokio::time::timeout(FINISH, &mut self.task).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::Transport("speech-to-text stream")),
            Err(_) => Err(Error::Timeout("speech-to-text final transcript")),
        }
    }
}

impl Drop for SttStream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// For `lamp-voice stt-check` (#1047, the first builder at 15:29): stream
/// `samples` (16 kHz mono) to the speech-to-text `config` names, in 10 ms
/// blocks at the pace they were recorded, as the stop words do. Then, 1.5 s
/// after the last block, `Finalize`, and 2 s later `CloseStream`. Print
/// everything the service sends back, by kind and when, from the first
/// block sent. That shows whether results come at all, and if so only
/// after `Finalize` or only at the close. On Flux (`LAMP_SECOND_FLUX=on`),
/// `ForceEndTurn` takes `Finalize`'s place.
pub async fn check(config: &SecondConfig, samples: &[i16]) -> Result<()> {
    let (proxy, key) = match &config.stt {
        Stt::Deepgram { key } => (None, key),
        Stt::Autonomous { base_url, key } => (Some(base_url.as_str()), key),
    };
    let asked = Instant::now();
    let key_terms = super::key_terms::listening(&config.stt_key_terms);
    let on_flux = flux::chosen(config);
    let (address, (asking, ask)) = if on_flux {
        (
            flux::url(proxy, &key_terms, flux::eager())?,
            ("ForceEndTurn", flux::FORCE_END_TURN),
        )
    } else {
        (
            url(proxy, config.stt_language.as_deref(), &key_terms)?,
            ("Finalize", r#"{"type":"Finalize"}"#),
        )
    };
    let (mut write, mut read) = socket(address, key.expose()).await?.split();
    println!(
        "connected in {} ms; sending {:.2} s of audio",
        asked.elapsed().as_millis(),
        samples.len() as f64 / 16_000.0
    );
    let started = Instant::now();
    let at = move || started.elapsed().as_millis();
    let writer = async {
        for (index, block) in samples.chunks(160).enumerate() {
            tokio::time::sleep_until(started + Duration::from_millis(10 * index as u64)).await;
            let bytes: Vec<u8> = block
                .iter()
                .flat_map(|sample| sample.to_le_bytes())
                .collect();
            write
                .send(Message::binary(bytes))
                .await
                .map_err(|_| Error::Transport("speech-to-text send"))?;
        }
        println!("{:>6} ms  the last block went", at());
        tokio::time::sleep(Duration::from_millis(1_500)).await;
        write
            .send(Message::text(ask))
            .await
            .map_err(|_| Error::Transport("speech-to-text send"))?;
        println!("{:>6} ms  {asking} went", at());
        tokio::time::sleep(Duration::from_secs(2)).await;
        write
            .send(Message::text(r#"{"type":"CloseStream"}"#))
            .await
            .map_err(|_| Error::Transport("speech-to-text send"))?;
        println!("{:>6} ms  CloseStream went", at());
        // The close is waited for below.
        std::future::pending::<()>().await;
        Ok::<(), Error>(())
    };
    let counts = Replies::default();
    let reader = async {
        while let Some(message) = read.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    if let Some(heard) = heard(&text) {
                        counts.count(if heard.transcript.is_empty() {
                            &counts.without_words
                        } else {
                            &counts.with_words
                        });
                        println!(
                            "{:>6} ms  Results final={} speech_final={} from_finalize={} audio {:.2} s: {:?}",
                            at(),
                            heard.is_final,
                            heard.speech_final,
                            heard.from_finalize,
                            heard.end,
                            heard.transcript
                        );
                    } else if let Some(turn) = flux::turn_info(&text) {
                        counts.count(if turn.transcript.is_empty() {
                            &counts.without_words
                        } else {
                            &counts.with_words
                        });
                        println!(
                            "{:>6} ms  TurnInfo {:?} turn={} trigger={} audio {:.2} s: {:?}",
                            at(),
                            turn.event,
                            turn.turn_index,
                            turn.trigger.as_deref().unwrap_or("none"),
                            turn.audio_window_end,
                            turn.transcript
                        );
                    } else {
                        let kind = serde_json::from_str::<serde_json::Value>(&text)
                            .ok()
                            .and_then(|message| message["type"].as_str().map(str::to_owned))
                            .unwrap_or_else(|| "not JSON".to_owned());
                        counts.count(if kind == "SpeechStarted" {
                            &counts.speech_started
                        } else {
                            &counts.other
                        });
                        println!(
                            "{:>6} ms  {kind}: {}",
                            at(),
                            text.chars().take(300).collect::<String>()
                        );
                    }
                }
                Ok(Message::Binary(bytes)) => {
                    counts.count(&counts.other);
                    println!("{:>6} ms  binary, {} bytes", at(), bytes.len());
                }
                Ok(Message::Close(frame)) => {
                    println!("{:>6} ms  closed: {frame:?}", at());
                    break;
                }
                Ok(_) => {}
                Err(error) => {
                    println!("{:>6} ms  failed: {error}", at());
                    break;
                }
            }
        }
    };
    tokio::select! {
        written = writer => written?,
        read = tokio::time::timeout(
            Duration::from_secs_f64(samples.len() as f64 / 16_000.0) + Duration::from_secs(8),
            reader,
        ) => {
            if read.is_err() {
                println!("{:>6} ms  no close came", at());
            }
        }
    }
    println!("{}", counts.told());
    Ok(())
}

async fn run<S>(
    socket: tokio_tungstenite::WebSocketStream<S>,
    mut queued: mpsc::Receiver<Outgoing>,
    words_at: Arc<OnceLock<Instant>>,
    (said, early): (mpsc::UnboundedSender<String>, mpsc::UnboundedSender<Early>),
    replies: Arc<Replies>,
    quiet_end: Arc<AtomicBool>,
) -> Result<(String, Option<String>)>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut write, mut read) = socket.split();
    let mut transcript = Transcript::default();
    let mut last_sent = Instant::now();
    let mut samples_sent = 0usize;
    // Where the line's sound ends, in seconds of the audio sent.
    let mut sound = LineSound::default();
    // Set at the end of the line, when `Finalize` went: until when its
    // final words are waited for.
    let mut finalize_by: Option<Instant> = None;
    let mut finalize_went: Option<Instant> = None;
    // `Finalize`'s answer: how long after it went, and where in the audio
    // it ends, for the journal (#1712: on lamp-4ace most lines' words came
    // from `CloseStream`, and whether `Finalize` was ignored or answered
    // short was not told).
    let mut answered: Option<(Duration, f64)> = None;
    // `CloseStream` went: its final words, or its close, are waited for.
    let mut closing = false;
    let mut last_final: Option<Instant> = None;
    let mut at_end = String::new();
    // The words at the end, and the journal's account: `Finalize`'s answer
    // (when it went), the results with and without words, and where the
    // words came from, with the last interim's if those were kept.
    let ending = |transcript: &Transcript,
                  at_end: &str,
                  answered: Option<Option<(Duration, f64)>>,
                  from: &str| {
        let answer = match answered {
            None => String::new(),
            Some(Some((after, end))) => format!(
                ", finalize answered in {} ms to {} ms",
                after.as_millis(),
                (end * 1000.0).round() as i64
            ),
            Some(None) => ", finalize unanswered".to_owned(),
        };
        let (words, kept) = transcript.words();
        let interim = if kept { "+interim" } else { "" };
        (
            words,
            Some(format!(
                "{at_end}{answer}{}, words from={from}{interim}",
                replies.results_told()
            )),
        )
    };
    let close_stream = || Message::text(r#"{"type":"CloseStream"}"#);
    loop {
        let sent = samples_sent as f64 / 16_000.0;
        tokio::select! {
            frame = queued.recv(), if finalize_by.is_none() && !closing => {
                match frame {
                    Some(Outgoing::Audio(bytes)) => {
                        sound.sent(&bytes, (samples_sent + bytes.len() / 2) as f64 / 16_000.0);
                        samples_sent += bytes.len() / 2;
                        write
                            .send(Message::binary(bytes))
                            .await
                            .map_err(|_| Error::Transport("speech-to-text send"))?;
                        last_sent = Instant::now();
                        continue;
                    }
                    // Its answer comes as `from_finalize` results, read as
                    // any other; the stream goes on.
                    Some(Outgoing::Flush) => {
                        write
                            .send(Message::text(r#"{"type":"Finalize"}"#))
                            .await
                            .map_err(|_| Error::Transport("speech-to-text send"))?;
                        last_sent = Instant::now();
                        continue;
                    }
                    None => {}
                }
                // The end of the line. Words already settled are the
                // answer; otherwise `Finalize` asks for the final at once
                // (#779), and `CloseStream` only if it does not come.
                let before = last_final.map_or("none".to_owned(), |at| {
                    format!("{} ms before End", at.elapsed().as_millis())
                });
                at_end = format!("{}, last final {before}", transcript.at_end(sent));
                if transcript.settled(sent) {
                    let _ = write.send(close_stream()).await;
                    return Ok(ending(&transcript, &at_end, None, "settled"));
                }
                // The words so far, the last interim's among them, while
                // `Finalize`'s answer comes: a chat may begin on them.
                let (words, interim) = transcript.words();
                if interim && words.chars().any(char::is_alphanumeric) {
                    let _ = early.send(Early::Words(words));
                }
                write
                    .send(Message::text(r#"{"type":"Finalize"}"#))
                    .await
                    .map_err(|_| Error::Transport("speech-to-text send"))?;
                finalize_went = Some(Instant::now());
                finalize_by = Some(Instant::now() + FINALIZE);
            }
            _ = tokio::time::sleep_until(last_sent + KEEP_ALIVE), if finalize_by.is_none() && !closing => {
                write
                    .send(Message::text(r#"{"type":"KeepAlive"}"#))
                    .await
                    .map_err(|_| Error::Transport("speech-to-text send"))?;
                last_sent = Instant::now();
            }
            _ = tokio::time::sleep_until(finalize_by.unwrap_or(last_sent)), if finalize_by.is_some() && !closing => {
                // `Finalize` went unanswered: the proxy may not pass it.
                closing = true;
                write
                    .send(close_stream())
                    .await
                    .map_err(|_| Error::Transport("speech-to-text send"))?;
            }
            message = read.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    if let Some(words) = heard(&text) {
                        replies.count(if words.transcript.is_empty() {
                            &replies.without_words
                        } else {
                            &replies.with_words
                        });
                        if !words.transcript.is_empty() {
                            let _ = words_at.set(Instant::now());
                            let _ = said.send(words.transcript.clone());
                        }
                        let finalized = words.from_finalize;
                        if words.is_final && finalize_by.is_none() {
                            last_final = Some(Instant::now());
                        }
                        if let Some(went) = finalize_went
                            && finalized
                            && answered.is_none()
                        {
                            answered = Some((went.elapsed(), words.end));
                        }
                        transcript.add(words);
                        if closing && transcript.settled(sent) {
                            // `CloseStream`'s words cover all the audio sent:
                            // its close is not waited for.
                            return Ok(ending(&transcript, &at_end, Some(answered), "close_stream"));
                        }
                        if finalize_by.is_some() && !closing {
                            if transcript.settled(sent) {
                                // Closes the socket; its answer is not waited for.
                                let _ = write.send(close_stream()).await;
                                return Ok(ending(&transcript, &at_end, Some(answered), "finalize"));
                            }
                            if finalized
                                && quiet_end.load(Ordering::Acquire)
                                && transcript.covers_the_sound(sound.to)
                            {
                                // Short of the audio sent, but what is left is
                                // the line's quiet end: its words are these.
                                let _ = write.send(close_stream()).await;
                                let (words, told) = ending(&transcript, &at_end, Some(answered), "finalize");
                                return Ok((
                                    words,
                                    told.map(|told| {
                                        format!(
                                            "{told}, the rest quiet: sound to {} ms",
                                            (sound.to * 1000.0).round() as i64
                                        )
                                    }),
                                ));
                            }
                            if finalized {
                                // `Finalize`'s answer short of the audio sent:
                                // audio sent faster than speech (a held
                                // burst) was not all heard yet, and nothing
                                // more came for it on lamp-52e6 ("Hang" for
                                // "Hang on.", #828). `CloseStream` flushes
                                // the rest and closes; its words are waited
                                // for.
                                closing = true;
                                write
                                    .send(close_stream())
                                    .await
                                    .map_err(|_| Error::Transport("speech-to-text send"))?;
                            }
                        }
                    } else if let Some(at) = speech_started(&text) {
                        replies.count(&replies.speech_started);
                        transcript.speech_started(at);
                    } else {
                        replies.count(&replies.other);
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    replies.end(frame.map_or_else(
                        || "was closed by the service, with no code".to_owned(),
                        |frame| {
                            format!(
                                "was closed by the service, code {} {:?}",
                                u16::from(frame.code),
                                frame.reason.as_str()
                            )
                        },
                    ));
                    return if finalize_by.is_some() {
                        Ok(ending(&transcript, &at_end, Some(answered), "close_stream"))
                    } else {
                        Err(Error::Transport("speech-to-text stream"))
                    };
                }
                None => {
                    replies.end("ended with no close".to_owned());
                    return if finalize_by.is_some() {
                        Ok(ending(&transcript, &at_end, Some(answered), "close_stream"))
                    } else {
                        Err(Error::Transport("speech-to-text stream"))
                    };
                }
                Some(Ok(Message::Binary(_))) => replies.count(&replies.other),
                Some(Ok(_)) => {}
                Some(Err(error)) => {
                    replies.end(format!("failed: {error}"));
                    return Err(Error::Transport("speech-to-text stream"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUERY: &str = "model=nova-3-general&encoding=linear16&sample_rate=16000&channels=1\
        &language=en&smart_format=true&interim_results=true&endpointing=300&vad_events=true\
        &keyterm=Lamp";

    #[test]
    fn the_proxy_url_is_v1s() {
        let url = url(
            Some("https://campaign-api.autonomous.ai/api/v1/ai/"),
            None,
            &[],
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            format!(
                "wss://campaign-api.autonomous.ai/api/v1/ai/v1/ws/audio/transcriptions?{QUERY}"
            )
        );
    }

    /// #1712: the unit's key terms follow the lamp's name, each its own
    /// `keyterm`, encoded.
    #[test]
    fn the_units_key_terms_follow_the_name() {
        let terms = ["Hanoi".to_owned(), "Ho Chi Minh City".to_owned()];
        assert_eq!(
            url(None, None, &terms).unwrap().as_str(),
            format!(
                "wss://api.deepgram.com/v1/listen?{QUERY}&keyterm=Hanoi&keyterm=Ho+Chi+Minh+City"
            )
        );
    }

    #[test]
    fn deepgram_is_called_directly_in_the_language_asked() {
        assert_eq!(
            url(None, Some("vi"), &[]).unwrap().as_str(),
            format!(
                "wss://api.deepgram.com/v1/listen?{}",
                QUERY.replace("language=en", "language=vi")
            )
        );
    }

    /// #1712 (the first builder at 21:28 and 22:54): a local server stands
    /// in for the proxy and reads the stream's query. `second.json`'s
    /// `stt_language` `vi` asks Nova-3 for `vi`, with the key terms, even
    /// with Flux on, which has no Vietnamese; English with Flux on asks
    /// Flux; unset asks Nova-3 in English.
    // The large error is tungstenite's own handshake callback signature.
    #[allow(clippy::result_large_err)]
    #[tokio::test]
    async fn vietnamese_keeps_off_flux_and_reaches_the_stream_with_its_key_terms() {
        let directory =
            std::env::temp_dir().join(format!("lamp-voice-stt-language-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let key = directory.join("llm.key");
        std::fs::write(&key, "secret\n").unwrap();
        std::fs::set_permissions(&key, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .unwrap();
        for (stt_language, flux, model, language) in [
            (Some("vi"), true, "nova-3-general", Some("vi")),
            (Some("vi"), false, "nova-3-general", Some("vi")),
            (Some("en-US"), true, "flux-general-en", None),
            (None, true, "flux-general-en", None),
            (None, false, "nova-3-general", Some("en")),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut asked = String::new();
                let _socket = tokio_tungstenite::accept_hdr_async(
                    stream,
                    |request: &tokio_tungstenite::tungstenite::handshake::server::Request,
                     response| {
                        asked = request.uri().to_string();
                        Ok(response)
                    },
                )
                .await
                .unwrap();
                asked
            });
            let config = directory.join("second.json");
            std::fs::write(
                &config,
                format!(
                    r#"{{"llm_base_url":"https://llm.example.invalid/v1","llm_model":"m","llm_credential_file":"{}","stt_base_url":"https://stt.example.invalid"}}"#,
                    key.display()
                ),
            )
            .unwrap();
            std::fs::set_permissions(&config, std::os::unix::fs::PermissionsExt::from_mode(0o600))
                .unwrap();
            let mut route = crate::second_config::load_from_path(&config).unwrap();
            route.stt_language = stt_language.map(str::to_owned);
            // The file takes https only: the stream is pointed here after.
            let Stt::Autonomous { key: secret, .. } = route.stt.clone() else {
                panic!("the proxy's speech to text");
            };
            route.stt = Stt::Autonomous {
                base_url: format!("http://{address}"),
                key: secret,
            };
            route.stt_flux = flux;
            route.stt_key_terms = vec!["Hanoi".to_owned()];
            let _stream = SttStream::connect(&route).await.unwrap();
            let asked = server.await.unwrap();
            let query: Vec<(String, String)> = reqwest::Url::parse(&format!("ws://x{asked}"))
                .unwrap()
                .query_pairs()
                .into_owned()
                .collect();
            let value = |name: &str| {
                query
                    .iter()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value.as_str())
            };
            assert_eq!(
                value("model"),
                Some(model),
                "{stt_language:?}, flux {flux}: {asked}"
            );
            assert_eq!(
                value("language"),
                language,
                "{stt_language:?}, flux {flux}: {asked}"
            );
            let terms: Vec<&str> = query
                .iter()
                .filter(|(key, _)| key == "keyterm")
                .map(|(_, value)| value.as_str())
                .collect();
            assert_eq!(terms, ["Lamp", "Hanoi"], "{stt_language:?}: {asked}");
        }
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_service_that_is_not_a_websocket_is_refused() {
        assert_eq!(
            url(Some("ftp://example.com"), None, &[]).unwrap_err(),
            Error::Url
        );
    }

    #[test]
    fn interim_results_are_dropped_and_finals_joined_in_order() {
        let recorded = [
            r#"{"type":"Metadata","request_id":"x"}"#,
            r#"{"type":"SpeechStarted","timestamp":0.1}"#,
            r#"{"type":"Results","is_final":false,"speech_final":false,"channel":{"alternatives":[{"transcript":"what","confidence":0.9}]}}"#,
            r#"{"type":"Results","is_final":true,"speech_final":false,"channel":{"alternatives":[{"transcript":"What time","confidence":0.9}]}}"#,
            r#"{"type":"Results","is_final":true,"speech_final":false,"channel":{"alternatives":[{"transcript":"","confidence":0}]}}"#,
            r#"{"type":"Results","is_final":false,"speech_final":false,"channel":{"alternatives":[{"transcript":"is","confidence":0.9}]}}"#,
            r#"{"type":"Results","is_final":true,"speech_final":true,"channel":{"alternatives":[{"transcript":" is it? ","confidence":0.9}]}}"#,
            r#"{"type":"UtteranceEnd","last_word_end":2.1}"#,
            "not json",
        ];
        let mut transcript = Transcript::default();
        let mut ends = 0;
        for text in recorded {
            if let Some(words) = heard(text) {
                ends += usize::from(words.speech_final);
                transcript.add(words);
            }
        }
        assert_eq!(transcript.words().0, "What time is it?");
        assert_eq!(ends, 1);
    }

    #[test]
    fn words_settle_only_on_full_cover_and_unsettle_on_new_speech() {
        let result = |text: &str, is_final, speech_final, end| Heard {
            transcript: text.to_owned(),
            is_final,
            speech_final,
            from_finalize: false,
            end,
        };
        let mut transcript = Transcript::default();
        assert!(!transcript.settled(1.0), "nothing final yet");
        assert!(!transcript.settled(0.0), "nor for no audio");
        transcript.add(result("what time", true, false, 0.8));
        assert!(!transcript.settled(1.0), "0.2 s not yet covered");
        assert!(transcript.settled(0.8));
        transcript.add(result("is", false, false, 1.0));
        assert!(!transcript.settled(0.8), "an interim with words is pending");
        transcript.add(result("is it", true, true, 1.0));
        assert!(
            !transcript.settled(2.5),
            "an utterance end leaves 1.5 s uncovered"
        );
        assert!(transcript.settled(1.0));
        transcript.add(result("", true, false, 2.5));
        assert!(transcript.settled(2.5), "a silent final covers the rest");
        transcript.add(result("", false, false, 2.5));
        assert!(transcript.settled(2.5), "an empty interim is silence");
        transcript.speech_started(2.6);
        assert!(!transcript.settled(2.5), "someone talks again");
        assert_eq!(transcript.words().0, "what time is it");
    }

    #[test]
    fn the_end_of_the_line_is_told_in_ms_without_words() {
        let mut transcript = Transcript::default();
        assert_eq!(
            transcript.at_end(1.5),
            "settled=false, speech_final=false, finals cover none of 1500 ms sent, pending=none"
        );
        transcript.add(Heard {
            transcript: "secret words".to_owned(),
            is_final: true,
            speech_final: true,
            from_finalize: false,
            end: 1.23,
        });
        assert_eq!(
            speech_started(r#"{"type":"SpeechStarted","timestamp":1.35}"#),
            Some(1.35)
        );
        transcript.speech_started(1.35);
        let told = transcript.at_end(1.5);
        assert_eq!(
            told,
            "settled=false, speech_final=true, finals cover 1230 of 1500 ms sent, \
             pending=speech_started 120 ms after the last final"
        );
        assert!(!told.contains("secret"));
    }

    /// Speech to text in memory, on the paused clock: the server reads
    /// audio, sends `before` once some arrived, answers `Finalize` at once
    /// with `finalized`, 50 ms apart (none: ignores it, as a proxy might),
    /// and answers `CloseStream` `round_trip` later with a silent final short
    /// of the audio, a last final and its close. The client sends one second of audio, waits for its
    /// results, then sends `more` frames and finishes. Returns the words,
    /// the journal's account and how long `finish` took.
    async fn finish_after(
        before: &'static str,
        finalized: &'static [&'static str],
        round_trip: Duration,
        more: usize,
    ) -> (String, String, Duration) {
        finish_closing_after(before, finalized, round_trip, more, Duration::ZERO).await
    }

    /// [`finish_after`], with the close `close_after` after `CloseStream`'s
    /// last final.
    async fn finish_closing_after(
        before: &'static str,
        finalized: &'static [&'static str],
        round_trip: Duration,
        more: usize,
        close_after: Duration,
    ) -> (String, String, Duration) {
        use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Role};
        let (near, far) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let mut socket = WebSocketStream::from_raw_socket(far, Role::Server, None).await;
            let mut told = false;
            while let Some(Ok(message)) = socket.next().await {
                match message {
                    Message::Binary(_) if !told => {
                        told = true;
                        socket.send(Message::text(before)).await.unwrap();
                    }
                    Message::Text(text) if text.contains("Finalize") => {
                        for finalized in finalized {
                            socket.send(Message::text(*finalized)).await.unwrap();
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                    }
                    Message::Text(text) if text.contains("CloseStream") => break,
                    _ => {}
                }
            }
            tokio::time::sleep(round_trip).await;
            // A segment short of the audio sent, then the last final.
            let silent = r#"{"type":"Results","is_final":true,"speech_final":false,"start":0.3,"duration":0.2,"channel":{"alternatives":[{"transcript":""}]}}"#;
            let last = r#"{"type":"Results","is_final":true,"speech_final":true,"start":0.5,"duration":0.5,"channel":{"alternatives":[{"transcript":"from close"}]}}"#;
            let _ = socket.send(Message::text(silent)).await;
            let _ = socket.send(Message::text(last)).await;
            tokio::time::sleep(close_after).await;
            let _ = socket.close(None).await;
        });
        let stream =
            SttStream::start(WebSocketStream::from_raw_socket(near, Role::Client, None).await);
        // One second of audio, then its results have time to arrive.
        for _ in 0..50 {
            stream.send(&[SOUND; 320]).unwrap();
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        for _ in 0..more {
            stream.send(&[SOUND; 320]).unwrap();
        }
        let began = Instant::now();
        let (words, at_end) = stream.finish_told(true).await.unwrap();
        let took = began.elapsed();
        server.abort();
        (words, at_end.unwrap(), took)
    }

    const INTERIM: &str = r#"{"type":"Results","is_final":false,"speech_final":false,"start":0.0,"duration":0.6,"channel":{"alternatives":[{"transcript":"hello"}]}}"#;

    /// A sample of a line's sound: about -21 dBFS, talk near the lamp.
    const SOUND: i16 = 3_000;

    /// `sounding` 20 ms blocks of sound and then `quiet` of silence:
    /// [`finish_sounding_as`] for a request.
    async fn finish_sounding(
        sounding: usize,
        quiet: usize,
        finalized: &'static str,
    ) -> (String, String, Duration) {
        finish_sounding_as(&[(sounding, SOUND), (quiet, 0)], finalized, true).await
    }

    /// 20 ms blocks, `runs` of them each holding one sample, an interim,
    /// then `Finalize` answered with `finalized` at once and `CloseStream`
    /// 800 ms later with "from close". Finished as a request's line
    /// (`quiet_end`) or a stop word's. Returns the words, the journal's
    /// account and how long finishing took.
    async fn finish_sounding_as(
        runs: &[(usize, i16)],
        finalized: &'static str,
        quiet_end: bool,
    ) -> (String, String, Duration) {
        use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Role};
        let (near, far) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let mut socket = WebSocketStream::from_raw_socket(far, Role::Server, None).await;
            let mut told = false;
            while let Some(Ok(message)) = socket.next().await {
                match message {
                    Message::Binary(_) if !told => {
                        told = true;
                        socket.send(Message::text(INTERIM)).await.unwrap();
                    }
                    Message::Text(text) if text.contains("Finalize") => {
                        socket.send(Message::text(finalized)).await.unwrap();
                    }
                    Message::Text(text) if text.contains("CloseStream") => break,
                    _ => {}
                }
            }
            tokio::time::sleep(Duration::from_millis(800)).await;
            let last = r#"{"type":"Results","is_final":true,"speech_final":true,"start":0.5,"duration":0.5,"channel":{"alternatives":[{"transcript":"from close"}]}}"#;
            let _ = socket.send(Message::text(last)).await;
            let _ = socket.close(None).await;
        });
        let stream =
            SttStream::start(WebSocketStream::from_raw_socket(near, Role::Client, None).await);
        for &(blocks, sample) in runs {
            for _ in 0..blocks {
                stream.send(&[sample; 320]).unwrap();
            }
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        let began = Instant::now();
        let (words, at_end) = stream.finish_told(quiet_end).await.unwrap();
        let took = began.elapsed();
        server.abort();
        (words, at_end.unwrap(), took)
    }

    /// #1712, lamp-4ace 19 Oct (the first builder at 23:46): on 114 of 131
    /// lines `Finalize`'s answer reached about 0.48 s short of the audio
    /// sent, and the words waited for `CloseStream`, 652 ms after the
    /// line's end. When what is left is the line's quiet end, the answer's
    /// words are the line's at once; when sound is left past the answer,
    /// as "Hang" for "Hang on." (#828), `CloseStream` still flushes it.
    #[tokio::test(start_paused = true)]
    async fn a_finalize_answer_short_of_only_the_quiet_end_is_the_lines_words_at_once() {
        const ANSWERED: &str = r#"{"type":"Results","is_final":true,"speech_final":true,"from_finalize":true,"start":0.0,"duration":0.6,"channel":{"alternatives":[{"transcript":"hello lamp"}]}}"#;
        // 0.6 s of sound, then 0.4 s of quiet: the answer covers the sound.
        let (words, at_end, took) = finish_sounding(30, 20, ANSWERED).await;
        assert_eq!(words, "hello lamp");
        assert_eq!(took, Duration::ZERO, "no wait for CloseStream");
        assert!(
            at_end.ends_with(", words from=finalize, the rest quiet: sound to 600 ms"),
            "{at_end}"
        );
        // Sound 0.2 s past the answer: a word may be there, so it is flushed.
        let (words, at_end, took) = finish_sounding(40, 10, ANSWERED).await;
        assert_eq!(words, "hello lamp from close");
        assert_eq!(took, Duration::from_millis(800));
        assert!(at_end.ends_with(", words from=close_stream"), "{at_end}");
        // Sound to within one block of the answer's end: taken; one block
        // more is a word left.
        let (words, _, took) = finish_sounding(32, 18, ANSWERED).await;
        assert_eq!((words.as_str(), took), ("hello lamp", Duration::ZERO));
        let (words, _, took) = finish_sounding(33, 17, ANSWERED).await;
        assert_eq!(
            (words.as_str(), took),
            ("hello lamp from close", Duration::from_millis(800))
        );
    }

    /// Jarvis on #1993: a whole word past `Finalize`'s answer, however short
    /// or soft, is sound left, and `CloseStream` still flushes it as before:
    /// "hang" answered to 0.30 s, then a word loud for 0.10 s after a 40 ms
    /// gap, or an "on" at about -48 dBFS to 0.60 s. With only quiet after
    /// it, "hang" is the line's at once.
    #[tokio::test(start_paused = true)]
    async fn a_short_or_soft_last_word_past_the_answer_still_waits_for_close_stream() {
        const HANG: &str = r#"{"type":"Results","is_final":true,"speech_final":true,"from_finalize":true,"start":0.0,"duration":0.3,"channel":{"alternatives":[{"transcript":"hang"}]}}"#;
        let short: &[(usize, i16)] = &[(15, SOUND), (2, 0), (5, SOUND), (28, 0)];
        // An RMS of 130: -48 dBFS, 27 dB under the line's sound.
        let soft: &[(usize, i16)] = &[(15, SOUND), (15, 130), (20, 0)];
        for runs in [short, soft] {
            let (words, at_end, took) = finish_sounding_as(runs, HANG, true).await;
            assert_eq!(words, "hang from close", "{runs:?}");
            assert_eq!(took, Duration::from_millis(800));
            assert!(at_end.ends_with(", words from=close_stream"), "{at_end}");
        }
        let (words, at_end, took) = finish_sounding_as(&[(15, SOUND), (35, 0)], HANG, true).await;
        assert_eq!((words.as_str(), took), ("hang", Duration::ZERO));
        assert!(
            at_end.ends_with("the rest quiet: sound to 300 ms"),
            "{at_end}"
        );
    }

    /// Jarvis's P3 on #1993: a stop word's stream ([`SttStream::finished`])
    /// keeps `CloseStream` after a short answer, whatever is left: over the
    /// lamp a stop word can be pushed under any bar (#828).
    #[tokio::test(start_paused = true)]
    async fn a_stop_words_stream_waits_for_close_stream_after_a_short_answer() {
        const HANG: &str = r#"{"type":"Results","is_final":true,"speech_final":true,"from_finalize":true,"start":0.0,"duration":0.3,"channel":{"alternatives":[{"transcript":"hang"}]}}"#;
        let (words, at_end, took) = finish_sounding_as(&[(15, SOUND), (35, 0)], HANG, false).await;
        assert_eq!(
            (words.as_str(), took),
            ("hang from close", Duration::from_millis(800))
        );
        assert!(at_end.ends_with(", words from=close_stream"), "{at_end}");
    }

    /// A block is sound within 30 dB of the line's loudest so far, and over
    /// -60 dBFS: a far voice's soft ending counts; the quiet between sounds,
    /// and a level 30 dB under the words, do not.
    #[test]
    fn a_blocks_sound_is_told_from_its_quiet_against_the_lines_loudest() {
        let block = |sample: i16| -> Vec<u8> {
            std::iter::repeat_n(sample.to_le_bytes(), 320)
                .flatten()
                .collect()
        };
        let mut sound = LineSound::default();
        let mut at = 0.0;
        let mut sent = |sound: &mut LineSound, sample: i16| {
            at += 0.02;
            sound.sent(&block(sample), at);
        };
        sent(&mut sound, SOUND);
        sent(&mut sound, -SOUND);
        assert!((sound.to - 0.04).abs() < 1e-9);
        // -48 dBFS, 27 dB under: sound. -51 dBFS, just over 30 dB under:
        // quiet, as are -75 dBFS and silence.
        sent(&mut sound, 130);
        assert!((sound.to - 0.06).abs() < 1e-9);
        for quiet in [92, 6, 0] {
            sent(&mut sound, quiet);
        }
        assert!((sound.to - 0.06).abs() < 1e-9);
        // A far voice: its -45 dBFS words and a -58 dBFS ending are sound,
        // -62 dBFS is under the floor.
        let mut far = LineSound::default();
        far.sent(&block(184), 0.02);
        far.sent(&block(41), 0.04);
        far.sent(&block(26), 0.06);
        assert!((far.to - 0.04).abs() < 1e-9);
        assert_eq!(level(&[]), 0.0);
    }

    /// #1712 (`LAMP_SECOND_STT_WARM`): a stream kept ready says it is still
    /// open while its socket is, and not once the service has closed it.
    #[tokio::test]
    async fn a_kept_stream_says_whether_its_socket_is_still_open() {
        use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Role};
        let (near, far) = tokio::io::duplex(1 << 16);
        let (close, closing) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let mut socket = WebSocketStream::from_raw_socket(far, Role::Server, None).await;
            let _ = closing.await;
            let _ = socket.close(None).await;
        });
        let stream =
            SttStream::start(WebSocketStream::from_raw_socket(near, Role::Client, None).await);
        assert!(stream.still_open());
        let _ = close.send(());
        for _ in 0..200 {
            if !stream.still_open() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(!stream.still_open());
        server.abort();
    }

    /// #1712, request 329 on lamp-4ace: finals with words covering most of
    /// the line came before its end; `Finalize` went unanswered and the
    /// words came from `CloseStream`. The finals' words are kept, and the
    /// line's end says how many results had words.
    #[tokio::test(start_paused = true)]
    async fn finals_with_words_before_the_close_keep_their_words() {
        const SAID: &str = r#"{"type":"Results","is_final":true,"speech_final":true,"start":0.0,"duration":0.8,"channel":{"alternatives":[{"transcript":"hey lamp what time"}]}}"#;
        let (words, at_end, _) = finish_after(SAID, &[], Duration::from_millis(800), 0).await;
        assert_eq!(words, "hey lamp what time from close");
        assert!(
            at_end.contains("finals cover 800 of 1000 ms sent"),
            "{at_end}"
        );
        assert!(
            at_end.ends_with(
                ", finalize unanswered, results 2 with words, 1 without, words from=close_stream"
            ),
            "{at_end}"
        );
    }

    /// #1712: a line whose only words came in an interim result, its
    /// finals empty, keeps those words; the journal says so.
    #[tokio::test(start_paused = true)]
    async fn an_interims_words_with_no_final_after_them_are_kept() {
        const EMPTY: &str = r#"{"type":"Results","is_final":true,"speech_final":true,"from_finalize":true,"start":0.0,"duration":1.0,"channel":{"alternatives":[{"transcript":""}]}}"#;
        let mut transcript = Transcript::default();
        transcript.add(heard(INTERIM).unwrap());
        assert_eq!(transcript.words(), ("hello".to_owned(), true));
        transcript.add(heard(EMPTY).unwrap());
        assert_eq!(
            transcript.words(),
            (String::new(), false),
            "a final came after it"
        );
        let (words, at_end, _) = finish_after(INTERIM, &[], Duration::from_millis(800), 0).await;
        assert_eq!(
            words, "from close",
            "the close's final came after the interim"
        );
        assert!(at_end.ends_with("words from=close_stream"), "{at_end}");
    }

    #[tokio::test(start_paused = true)]
    async fn finals_covering_all_the_audio_sent_do_not_wait_for_close_stream() {
        let ended = r#"{"type":"Results","is_final":true,"speech_final":true,"start":0.0,"duration":1.0,"channel":{"alternatives":[{"transcript":"hello lamp"}]}}"#;
        let (words, at_end, took) = finish_after(ended, &[], Duration::from_millis(800), 0).await;
        assert_eq!(words, "hello lamp");
        assert_eq!(took, Duration::ZERO);
        assert!(at_end.ends_with(", words from=settled"), "{at_end}");
    }

    #[tokio::test(start_paused = true)]
    async fn words_still_coming_take_finalizes_answer_without_waiting_for_close_stream() {
        const FINALIZED: &str = r#"{"type":"Results","is_final":true,"speech_final":true,"from_finalize":true,"start":0.0,"duration":1.0,"channel":{"alternatives":[{"transcript":"hello lamp"}]}}"#;
        let (words, at_end, took) =
            finish_after(INTERIM, &[FINALIZED], Duration::from_millis(800), 0).await;
        assert_eq!(words, "hello lamp");
        assert_eq!(took, Duration::ZERO);
        assert!(
            at_end.ends_with(", finalize answered in 0 ms to 1000 ms, results 2 with words, 0 without, words from=finalize"),
            "{at_end}"
        );
        assert!(!at_end.contains("hello"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_proxy_that_ignores_finalize_falls_back_to_close_stream() {
        let (words, at_end, took) = finish_after(INTERIM, &[], Duration::from_millis(800), 0).await;
        assert_eq!(words, "from close");
        assert_eq!(took, FINALIZE + Duration::from_millis(800));
        assert!(
            at_end.ends_with(
                ", finalize unanswered, results 2 with words, 1 without, words from=close_stream"
            ),
            "{at_end}"
        );
    }

    /// #828: when the first words came back is kept, from the first result
    /// with words, interim or final, and it stays readable after the finish.
    #[tokio::test(start_paused = true)]
    async fn the_first_words_are_timed_from_the_first_result_with_words() {
        use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Role};
        const SILENT: &str = r#"{"type":"Results","is_final":false,"speech_final":false,"start":0.0,"duration":0.2,"channel":{"alternatives":[{"transcript":""}]}}"#;
        const FINALIZED: &str = r#"{"type":"Results","is_final":true,"speech_final":true,"from_finalize":true,"start":0.0,"duration":0.6,"channel":{"alternatives":[{"transcript":"hello"}]}}"#;
        let (near, far) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let mut socket = WebSocketStream::from_raw_socket(far, Role::Server, None).await;
            let mut blocks = 0;
            while let Some(Ok(message)) = socket.next().await {
                match message {
                    Message::Binary(_) => {
                        blocks += 1;
                        if blocks == 1 {
                            socket.send(Message::text(SILENT)).await.unwrap();
                        } else if blocks == 2 {
                            tokio::time::sleep(Duration::from_millis(300)).await;
                            socket.send(Message::text(INTERIM)).await.unwrap();
                        }
                    }
                    Message::Text(text) if text.contains("Finalize") => {
                        socket.send(Message::text(FINALIZED)).await.unwrap();
                    }
                    Message::Text(text) if text.contains("CloseStream") => break,
                    _ => {}
                }
            }
        });
        let stream =
            SttStream::start(WebSocketStream::from_raw_socket(near, Role::Client, None).await);
        let first_words = stream.first_words();
        let began = Instant::now();
        stream.send(&[SOUND; 320]).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            first_words.get().is_none(),
            "a result with no words is not words"
        );
        stream.send(&[SOUND; 320]).unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        let at = *first_words.get().expect("the words never came");
        let after = at - began;
        assert!(
            (Duration::from_millis(400)..Duration::from_millis(450)).contains(&after),
            "{after:?}"
        );
        let (words, _) = stream.finish_told(true).await.unwrap();
        assert_eq!(words, "hello");
        assert_eq!(first_words.get().copied(), Some(at));
        server.abort();
    }

    /// #1047, the first builder at 15:29: what came back is counted by kind,
    /// so that a service that said nothing is told from one whose results
    /// held no words, and both from messages that are not read.
    #[tokio::test(start_paused = true)]
    async fn what_the_service_sends_back_is_counted_by_kind() {
        use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Role};
        const SILENT: &str = r#"{"type":"Results","is_final":false,"speech_final":false,"start":0.0,"duration":0.2,"channel":{"alternatives":[{"transcript":""}]}}"#;
        let (near, far) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let mut socket = WebSocketStream::from_raw_socket(far, Role::Server, None).await;
            if let Some(Ok(Message::Binary(_))) = socket.next().await {
                for message in [
                    SILENT,
                    SILENT,
                    INTERIM,
                    r#"{"type":"SpeechStarted","timestamp":0.1}"#,
                    r#"{"type":"Metadata"}"#,
                    r#"{"type":"UtteranceEnd","last_word_end":0.4}"#,
                ] {
                    socket.send(Message::text(message)).await.unwrap();
                }
            }
            std::future::pending::<()>().await;
        });
        let stream =
            SttStream::start(WebSocketStream::from_raw_socket(near, Role::Client, None).await);
        let replies = stream.replies();
        assert_eq!(
            replies.told(),
            "0 results with words, 0 without, 0 speech starts, 0 other messages"
        );
        stream.send(&[SOUND; 320]).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            replies.told(),
            "1 results with words, 2 without, 1 speech starts, 2 other messages"
        );
        server.abort();
    }

    /// #1047: `flush` asks for the words of what was sent so far with
    /// `Finalize`, in order with the audio, and the stream goes on: more
    /// audio follows, and the answer's words come as they are given.
    #[tokio::test(start_paused = true)]
    async fn a_flush_asks_for_the_words_so_far_and_the_stream_goes_on() {
        use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Role};
        const STOP: &str = r#"{"type":"Results","is_final":true,"speech_final":true,"from_finalize":true,"start":0.0,"duration":0.46,"channel":{"alternatives":[{"transcript":"Stop."}]}}"#;
        let (near, far) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let mut socket = WebSocketStream::from_raw_socket(far, Role::Server, None).await;
            let mut seen = Vec::new();
            while let Some(Ok(message)) = socket.next().await {
                match message {
                    Message::Binary(_) => seen.push("audio"),
                    Message::Text(text) if text.contains("Finalize") => {
                        seen.push("finalize");
                        socket.send(Message::text(STOP)).await.unwrap();
                    }
                    Message::Text(text) if text.contains("CloseStream") => break,
                    _ => {}
                }
                if seen.len() == 3 {
                    break;
                }
            }
            seen
        });
        let mut stream =
            SttStream::start(WebSocketStream::from_raw_socket(near, Role::Client, None).await);
        let mut words = stream.take_words().unwrap();
        stream.send(&[0; 160]).unwrap();
        stream.flush().unwrap();
        stream.send(&[0; 160]).unwrap();
        assert_eq!(words.recv().await.as_deref(), Some("Stop."));
        assert_eq!(server.await.unwrap(), ["audio", "finalize", "audio"]);
    }

    /// #1047, the first builder at 20:58: how the stream ended is kept,
    /// with the service's close code and reason, and said with the counts.
    #[tokio::test]
    async fn how_the_service_closed_the_stream_is_kept() {
        use tokio_tungstenite::WebSocketStream;
        use tokio_tungstenite::tungstenite::protocol::{
            CloseFrame, Role, frame::coding::CloseCode,
        };
        let (near, far) = tokio::io::duplex(1 << 16);
        let server = tokio::spawn(async move {
            let mut socket = WebSocketStream::from_raw_socket(far, Role::Server, None).await;
            if let Some(Ok(Message::Text(text))) = socket.next().await {
                assert!(text.contains("Finalize"), "{text}");
                socket
                    .close(Some(CloseFrame {
                        code: CloseCode::Normal,
                        reason: "finalized".into(),
                    }))
                    .await
                    .unwrap();
            }
        });
        let stream =
            SttStream::start(WebSocketStream::from_raw_socket(near, Role::Client, None).await);
        let replies = stream.replies();
        assert_eq!(replies.ended(), None);
        stream.flush().unwrap();
        server.await.unwrap();
        let began = Instant::now();
        while replies.ended().is_none() {
            assert!(began.elapsed() < Duration::from_secs(5), "never ended");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(
            replies.ended().as_deref(),
            Some("was closed by the service, code 1000 \"finalized\"")
        );
        assert!(
            replies
                .told()
                .ends_with("; the stream was closed by the service, code 1000 \"finalized\"")
        );
    }

    const SET_A_TIMER: &str = r#"{"type":"Results","is_final":true,"speech_final":true,"start":0.0,"duration":1.0,"channel":{"alternatives":[{"transcript":"set a timer"}]}}"#;
    const THE_REST: &str = r#"{"type":"Results","is_final":true,"speech_final":true,"from_finalize":true,"start":1.0,"duration":1.0,"channel":{"alternatives":[{"transcript":"for twenty minutes"}]}}"#;

    /// An utterance ended at 1 s, then another second was sent whose
    /// results had not come by the end of the line: they are waited for.
    #[tokio::test(start_paused = true)]
    async fn an_utterance_ended_before_more_audio_was_sent_does_not_end_the_line() {
        let (words, at_end, _) =
            finish_after(SET_A_TIMER, &[THE_REST], Duration::from_millis(800), 50).await;
        assert_eq!(words, "set a timer for twenty minutes");
        assert!(at_end.ends_with(", words from=finalize"), "{at_end}");
    }

    /// After `Finalize`, an older utterance's final arrives before the
    /// flush's answer: the flush is still waited for.
    #[tokio::test(start_paused = true)]
    async fn an_older_final_after_finalize_does_not_end_the_line() {
        let (words, at_end, took) = finish_after(
            INTERIM,
            &[SET_A_TIMER, THE_REST],
            Duration::from_millis(800),
            50,
        )
        .await;
        assert_eq!(words, "set a timer for twenty minutes");
        assert_eq!(took, Duration::from_millis(50));
        assert!(at_end.ends_with(", words from=finalize"), "{at_end}");
    }

    /// #828, lamp-52e6: after a held burst, `Finalize`'s answer covered
    /// only part of the audio sent ("Hang" for "Hang on.", 0.30 of
    /// 0.59 s). The line does not end on it: `CloseStream` goes at once,
    /// without waiting out `FINALIZE`, and the rest of the words are kept.
    #[tokio::test(start_paused = true)]
    async fn a_finalize_answer_short_of_the_audio_sent_flushes_the_rest() {
        const SHORT: &str = r#"{"type":"Results","is_final":true,"speech_final":true,"from_finalize":true,"start":0.0,"duration":0.3,"channel":{"alternatives":[{"transcript":"hang"}]}}"#;
        let (words, at_end, took) =
            finish_after(INTERIM, &[SHORT], Duration::from_millis(800), 0).await;
        assert_eq!(words, "hang from close");
        // The server's 50 ms after its answer, then `CloseStream`'s round
        // trip: `FINALIZE` is not waited out.
        assert_eq!(took, Duration::from_millis(850));
        // The journal tells the answer short: 300 of 1000 ms.
        assert!(
            at_end.ends_with(
                "of 1000 ms sent, pending=interim 600 ms after the last final, last final none, \
                 finalize answered in 0 ms to 300 ms, results 3 with words, 1 without, words from=close_stream"
            ),
            "{at_end}"
        );
    }

    /// #1712, lamp-4ace: most lines' words came from `CloseStream`, 0.8 to
    /// 1.0 s after the line. Once its last final covers all the audio sent,
    /// the words are taken; the socket's close, which may come later, is not
    /// waited for.
    #[tokio::test(start_paused = true)]
    async fn close_streams_words_are_taken_without_waiting_for_the_close() {
        const SHORT: &str = r#"{"type":"Results","is_final":true,"speech_final":true,"from_finalize":true,"start":0.0,"duration":0.3,"channel":{"alternatives":[{"transcript":"hang"}]}}"#;
        let (words, at_end, took) = finish_closing_after(
            INTERIM,
            &[SHORT],
            Duration::from_millis(800),
            0,
            Duration::from_millis(500),
        )
        .await;
        assert_eq!(words, "hang from close");
        assert_eq!(took, Duration::from_millis(850));
        assert!(at_end.ends_with(", words from=close_stream"), "{at_end}");
    }

    /// #1712 (`LAMP_SECOND_PREPARED`, the first builder at 17:02): at the
    /// line's end, with an interim's words still to be made final, they are
    /// given at once as early words, before `Finalize`'s answer; words
    /// already settled give none, as they are final at once.
    #[tokio::test(start_paused = true)]
    async fn an_interims_words_are_given_early_at_the_lines_end() {
        use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Role};
        const FINALIZED: &str = r#"{"type":"Results","is_final":true,"speech_final":true,"from_finalize":true,"start":0.0,"duration":1.0,"channel":{"alternatives":[{"transcript":"hello lamp"}]}}"#;
        const ENDED: &str = r#"{"type":"Results","is_final":true,"speech_final":true,"start":0.0,"duration":1.0,"channel":{"alternatives":[{"transcript":"hello lamp"}]}}"#;
        for (before, early_words) in [(INTERIM, Some("hello")), (ENDED, None)] {
            let (near, far) = tokio::io::duplex(1 << 16);
            let server = tokio::spawn(async move {
                let mut socket = WebSocketStream::from_raw_socket(far, Role::Server, None).await;
                let mut told = false;
                while let Some(Ok(message)) = socket.next().await {
                    match message {
                        Message::Binary(_) if !told => {
                            told = true;
                            socket.send(Message::text(before)).await.unwrap();
                        }
                        Message::Text(text) if text.contains("Finalize") => {
                            tokio::time::sleep(Duration::from_millis(300)).await;
                            socket.send(Message::text(FINALIZED)).await.unwrap();
                        }
                        Message::Text(text) if text.contains("CloseStream") => break,
                        _ => {}
                    }
                }
            });
            let mut stream =
                SttStream::start(WebSocketStream::from_raw_socket(near, Role::Client, None).await);
            let mut early = stream.take_early().unwrap();
            assert!(stream.take_early().is_none(), "one taker");
            for _ in 0..50 {
                stream.send(&[SOUND; 320]).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
            let began = Instant::now();
            let finishing = tokio::spawn(stream.finish_told(true));
            if let Some(words) = early_words {
                assert_eq!(early.recv().await, Some(Early::Words(words.to_owned())));
                assert_eq!(began.elapsed(), Duration::ZERO, "before Finalize's answer");
            }
            let (words, _) = finishing.await.unwrap().unwrap();
            assert_eq!(words, "hello lamp");
            if early_words.is_none() {
                assert!(early.try_recv().is_err(), "settled words are final at once");
            }
            server.abort();
        }
    }

    /// A local plain websocket server stands in for Deepgram.
    // The large error is tungstenite's own handshake callback signature.
    #[allow(clippy::result_large_err)]
    #[tokio::test]
    async fn audio_goes_out_and_the_final_words_come_back() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_hdr_async(
                stream,
                |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                    assert_eq!(request.headers()["authorization"], "Token secret");
                    Ok(response)
                },
            )
            .await
            .unwrap();
            let mut bytes = 0;
            while let Some(Ok(message)) = socket.next().await {
                match message {
                    Message::Binary(audio) => bytes += audio.len(),
                    Message::Text(text) if text.contains("CloseStream") => break,
                    _ => {}
                }
            }
            let result = r#"{"type":"Results","is_final":true,"speech_final":true,"channel":{"alternatives":[{"transcript":"hello lamp"}]}}"#;
            socket.send(Message::text(result)).await.unwrap();
            socket.close(None).await.unwrap();
            bytes
        });
        let url = reqwest::Url::parse(&format!("ws://{address}/v1/listen")).unwrap();
        let stream = SttStream::open(url, "secret").await.unwrap();
        stream.send(&[1, -1, 3]).unwrap();
        stream.send(&[7]).unwrap();
        assert_eq!(stream.finish(1).await.unwrap(), "hello lamp");
        assert_eq!(server.await.unwrap(), 8);
    }
}
