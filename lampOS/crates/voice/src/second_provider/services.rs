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

/// Speech-to-text, a chat model and text-to-speech.
pub trait Services: Send + Sync + 'static {
    /// Open speech-to-text for one utterance.
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>>;
    /// The answer to `messages`, as it is written; a failure to connect is
    /// the stream's first item.
    fn chat(&self, messages: Vec<Message>) -> BoxStream<'static, net::Result<ChatEvent>>;
    /// `text` spoken, as 24 kHz mono packets; dropping it stops the download.
    fn speak(&self, text: String) -> BoxStream<'static, net::Result<Vec<i16>>>;
    /// Whether the chat model takes a look's picture; one that does not is
    /// told it saw nothing ([`crate::speak_up::NO_LOOK_NOTE`]).
    fn sees(&self) -> bool {
        true
    }
}

/// One open utterance.
pub trait Listener: Send {
    /// Queue 16 kHz mono samples; nothing waits.
    fn send(&self, samples: &[i16]) -> net::Result<()>;
    /// The final words; empty when nothing was heard.
    /// `request` names the turn in the journal.
    fn finish(self: Box<Self>, request: u64) -> BoxFuture<'static, net::Result<String>>;
}

impl Listener for SttStream {
    fn send(&self, samples: &[i16]) -> net::Result<()> {
        SttStream::send(self, samples)
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
}

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
            config: Arc::new(config),
        })
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
        self.chat = Arc::new(Chat::deepseek(
            &self.config.hosted.deepseek_model,
            key,
            self.declare.clone(),
            false,
        )?);
        Ok(self)
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

/// `text` from the configured voice, as packets.
fn configured(tts: Arc<Tts>, text: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
    stream::once(async move { tts.speak(&text).await })
        .map_ok(|speech| {
            stream::try_unfold(speech, |mut speech| async move {
                Ok(speech.next().await?.map(|packet| (packet, speech)))
            })
        })
        .try_flatten()
        .boxed()
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

/// Gemini's speech, the configured voice saying a sentence Gemini could
/// not start. A sentence Gemini began and then lost fails as any voice's.
fn with_fallback(
    gemini: Arc<GeminiTts>,
    tts: Arc<Tts>,
    fallback: &'static str,
    text: String,
) -> BoxStream<'static, net::Result<Vec<i16>>> {
    stream::once(async move {
        if !GeminiTts::speaks(&text) {
            return Ok(stream::empty().boxed());
        }
        match gemini_first(&gemini, &text).await {
            Ok(speech) => Ok(speech),
            Err(error) => {
                eprintln!(
                    "lamp-voice second: Gemini's speech failed ({error}); {fallback} says the sentence"
                );
                Ok(configured(tts, text))
            }
        }
    })
    .try_flatten()
    .boxed()
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
        let chat = self.chat.clone();
        stream::once(async move { chat.send(&messages).await })
            .map_ok(|answer| {
                stream::try_unfold(answer, |mut answer| async move {
                    Ok(answer.next().await?.map(|event| (event, answer)))
                })
            })
            .try_flatten()
            .boxed()
    }

    fn speak(&self, text: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
        match &self.gemini_tts {
            Some(gemini) => with_fallback(
                gemini.clone(),
                self.tts.clone(),
                self.config.tts_provider.name(),
                text,
            ),
            None => configured(self.tts.clone(), text),
        }
    }

    fn sees(&self) -> bool {
        self.chat.sees()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::second_config::TtsProvider;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A local plain HTTP server answering one request with `head` and
    /// `body`; whether a request came is read from the handle.
    async fn server(head: &'static str, body: Vec<u8>) -> (String, tokio::task::JoinHandle<usize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let accepted =
                tokio::time::timeout(std::time::Duration::from_millis(500), listener.accept())
                    .await;
            let Ok(Ok((mut socket, _))) = accepted else {
                return 0;
            };
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = socket.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..n]);
            }
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

    /// `LAMP_SECOND_SPEECH=gemini`: a sentence Gemini refuses before any of
    /// its sound (a quota, say) is said by the configured voice; one Gemini
    /// says is Gemini's alone, the configured voice never asked.
    #[tokio::test]
    async fn a_sentence_gemini_cannot_start_is_said_by_the_configured_voice() {
        let (refusing, refused) = server(
            "HTTP/1.1 429 Too Many Requests\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
            Vec::new(),
        )
        .await;
        let (openai, asked) = server(PCM, bytes(&[100, -100])).await;
        let gemini = Arc::new(GeminiTts::new(&refusing, "tts", "g", "Kore").unwrap());
        let tts = Arc::new(Tts::with(TtsProvider::OpenAi, &openai, "k", None).unwrap());
        let samples = said(with_fallback(
            gemini,
            tts,
            "OpenAI's voice",
            "Hello there.".into(),
        ))
        .await;
        // The configured voice's level, +6 dB.
        assert_eq!(samples, [200, -200]);
        assert_eq!((refused.await.unwrap(), asked.await.unwrap()), (1, 1));

        use base64::{Engine, engine::general_purpose::STANDARD};
        let event = serde_json::json!({"candidates": [{"content": {"parts": [
            {"inlineData": {"mimeType": "audio/L16;codec=pcm;rate=24000",
                "data": STANDARD.encode(bytes(&[7, 9]))}}
        ]}}]});
        let (speaking, spoke) = server(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
            format!("data: {event}\n\n").into_bytes(),
        )
        .await;
        let (openai, unasked) = server(PCM, Vec::new()).await;
        let gemini = Arc::new(GeminiTts::new(&speaking, "tts", "g", "Kore").unwrap());
        let tts = Arc::new(Tts::with(TtsProvider::OpenAi, &openai, "k", None).unwrap());
        let samples = said(with_fallback(
            gemini,
            tts,
            "OpenAI's voice",
            "Hello there.".into(),
        ))
        .await;
        // Gemini's own level: no gain.
        assert_eq!(samples, [7, 9]);
        assert_eq!((spoke.await.unwrap(), unasked.await.unwrap()), (1, 0));
    }

    /// Nothing to say asks no voice at all.
    #[tokio::test]
    async fn only_tags_ask_neither_voice() {
        let (gemini_url, gemini_asked) = server(PCM, Vec::new()).await;
        let (openai, openai_asked) = server(PCM, Vec::new()).await;
        let gemini = Arc::new(GeminiTts::new(&gemini_url, "tts", "g", "Kore").unwrap());
        let tts = Arc::new(Tts::with(TtsProvider::OpenAi, &openai, "k", None).unwrap());
        let samples = said(with_fallback(
            gemini,
            tts,
            "OpenAI's voice",
            "[sigh]".into(),
        ))
        .await;
        assert!(samples.is_empty());
        assert_eq!(
            (gemini_asked.await.unwrap(), openai_asked.await.unwrap()),
            (0, 0)
        );
    }
}
