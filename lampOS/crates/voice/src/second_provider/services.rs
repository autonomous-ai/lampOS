//! The seam between the engine and the network: the three services as the
//! engine uses them, with [`Net`] wrapping the real clients.
use super::net::{self, Chat, ChatEvent, GeminiTts, Message, SttStream, Tts};
use crate::second_config::SecondConfig;
use futures_util::{
    StreamExt, TryStreamExt,
    future::BoxFuture,
    stream::{self, BoxStream},
};
use std::sync::Arc;
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
}

/// Where the worker reads whether the chat's connection is opened as a line
/// begins: `on`, and nothing else.
pub const CHAT_WARM_ENVIRONMENT: &str = "LAMP_SECOND_CHAT_WARM";
/// Where the worker reads whether the configured voice's connection is
/// opened as a line begins: `on`, and nothing else.
pub const SPEECH_WARM_ENVIRONMENT: &str = "LAMP_SECOND_SPEECH_WARM";
/// Where the worker reads whether a speech socket is opened as each answer
/// begins (#2135's row): `on`, and nothing else.
pub const SPEECH_SOCKET_ENVIRONMENT: &str = "LAMP_SECOND_SPEECH_SOCKET";

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

    /// The chat's requests are given only the tools `narrow` keeps
    /// ([`net::Chat::narrowing`]). Called after the chat is chosen: one
    /// chosen later is given every tool.
    pub fn narrowing(self, narrow: net::Narrow) -> Self {
        self.chat.narrowing(narrow);
        self
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
        stream::once(async move { chat.send_only(&messages, only).await })
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
}

/// How long a voice has to give a sentence's first sound, each time asked
/// (#1844's C): a host that never answers costs 4 s, not the connection's
/// 10 s plus its response's.
const START_WITHIN: std::time::Duration = std::time::Duration::from_secs(4);

/// `text` in the lamp's one voice, never another's (#1712, the owner: "one
/// voice per lamp"). A sentence the voice fails to start within
/// [`START_WITHIN`] is asked once more; failing again, it is not said, and
/// the turn goes on with the next sentence (#1844's C: before, the second
/// failure failed the whole turn). A sentence begun and then lost fails as
/// any does.
fn in_one_voice(
    voice: Voice,
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
        match first(&voice).await {
            Ok(speech) => Ok(speech),
            Err(error) => {
                eprintln!(
                    "lamp-voice second: {} failed to start a sentence ({error}); asked once more",
                    voice.name()
                );
                match first(&voice).await {
                    Ok(speech) => Ok(speech),
                    Err(again) => {
                        eprintln!(
                            "lamp-voice second: {} failed again ({again}); the sentence is not said, and the turn goes on",
                            voice.name()
                        );
                        Ok(stream::empty().boxed())
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
}

impl Services for Net {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        let config = self.config.clone();
        Box::pin(async move {
            let stream = SttStream::connect(&config).await?;
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

    fn speak(&self, text: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
        in_one_voice(self.voice(), text, None)
    }

    fn speak_for(
        &self,
        text: String,
        turn: net::Turn,
    ) -> BoxStream<'static, net::Result<Vec<i16>>> {
        in_one_voice(self.voice(), text, Some(turn))
    }

    fn spoken(&self, request: u64) {
        if let Some(told) = self.tts.told(request) {
            eprintln!("lamp-voice second: {told}");
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
        let tts = self.tts.clone();
        tokio::spawn(async move { tts.open_ahead(request).await });
    }

    fn warm(&self) -> BoxFuture<'static, ()> {
        let gemini = self.gemini_tts.clone();
        let chat = self.warm_chat.then(|| self.chat.clone());
        // The configured voice only when it is the lamp's one voice.
        let configured = (self.warm_speech && gemini.is_none()).then(|| self.tts.clone());
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
mod tests {
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

    const SSE: &str =
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";

    /// #1712, one voice per lamp: a sentence Gemini refuses is asked of
    /// Gemini once more, and said when it answers; refused again, it is not
    /// said. No other voice is ever asked.
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
        assert!(results.is_empty(), "not said, and no error: {results:?}");
        assert_eq!(gemini_asked.await.unwrap(), 2, "asked twice, no more");
    }

    /// The same for the configured voice: once more, then not said.
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
        assert!(results.is_empty(), "not said, and no error: {results:?}");
        assert_eq!(asked.await.unwrap(), 2);
    }

    /// #1844's C: a voice whose host takes the connection and never answers
    /// is given 4 s a try, twice, and the sentence is not said, with no
    /// error, so the turn goes on.
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
        assert!(results.is_empty(), "{results:?}");
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
}
