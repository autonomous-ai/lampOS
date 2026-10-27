//! Speech from Gemini's own speech models, not Live (`LAMP_SECOND_SPEECH=gemini`):
//! one streamed request a sentence or first clause, as the route asks any
//! voice, to `<base>/models/<model>:streamGenerateContent?alt=sse` with the
//! key in `x-goog-api-key`. The audio comes as base64 parts of server-sent
//! events, 24 kHz mono 16-bit PCM, and is handed on in packets as it
//! arrives. Google takes no streamed text in: one request per sentence
//! ([speech generation](https://ai.google.dev/gemini-api/docs/speech-generation),
//! read 2026-10-19).
use super::{
    Error, Result,
    chat::Lines,
    chunk, http_client, post_guarded,
    tts::{Packets, without_tags},
};
use crate::second_provider::services::{ChatPermission, permitted_result};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// A connection unused this long is opened again ahead of the next
/// sentence ([`GeminiTts::warm`]): the relay may have closed it, and a new
/// one cost up to 0.5 s more to the first audio from lamp-4ace (the probe of
/// 19 Oct, first run against the next two).
pub const WARM_AFTER: Duration = Duration::from_secs(20);
/// The most of a warming answer read, so its connection goes back to the
/// pool; a longer one is dropped with its connection.
const WARM_BYTES: usize = 64 << 10;

/// A server-sent event of base64 audio stays far under this; a stream
/// without line ends does not.
const MAX_LINE: usize = 4 << 20;

pub struct GeminiTts {
    client: reqwest::Client,
    url: String,
    /// The model's own address, which a warming `GET` asks.
    model_url: String,
    key: String,
    voice: String,
    /// When a sentence or a warming last used the connection.
    used: Mutex<Option<Instant>>,
    /// Sentences asked and not yet begun.
    asking: Arc<AtomicUsize>,
}

/// A sentence asked of Gemini until its first audio or its failure: how
/// many others were then asked and not begun.
pub struct Asking {
    count: Arc<AtomicUsize>,
    pub others: usize,
}

impl Drop for Asking {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::Relaxed);
    }
}

impl GeminiTts {
    /// `voice` is a Gemini voice name: the Live route's own (`provider.json`'s
    /// `voice`, Kore on the lamps), so the lamp keeps the voice it has.
    pub fn new(base_url: &str, model: &str, key: &str, voice: &str) -> Result<Self> {
        if !voice.chars().all(|c| c.is_ascii_alphanumeric()) || voice.is_empty() {
            return Err(Error::Url);
        }
        Ok(Self {
            client: http_client()?,
            url: url(base_url, model),
            model_url: format!("{}/models/{model}", base_url.trim_end_matches('/')),
            key: key.to_owned(),
            voice: voice.to_owned(),
            used: Mutex::new(None),
            asking: Arc::default(),
        })
    }

    /// Whether `text` has anything to say: text in `[...]` is not spoken,
    /// as for the other voices.
    pub fn speaks(text: &str) -> bool {
        !without_tags(text).trim().is_empty()
    }

    /// One more sentence asked, until the [`Asking`] is dropped.
    pub fn asking(&self) -> Asking {
        let others = self.asking.fetch_add(1, Ordering::Relaxed);
        Asking {
            count: self.asking.clone(),
            others,
        }
    }

    /// Open the connection the next sentence takes, with a `GET` of the
    /// model, unless one was used within [`WARM_AFTER`]: the status and how
    /// long it took, or `None` when not asked or nothing came. Asked as a
    /// line begins, so the answer's first sentence does not open it.
    pub async fn warm(&self) -> Option<(u16, Duration)> {
        let (status, took, ..) = super::warm_get(
            &self.client,
            &self.used,
            WARM_AFTER,
            &self.model_url,
            ("x-goog-api-key", &self.key),
            WARM_BYTES,
        )
        .await?;
        Some((status, took))
    }

    /// Start speaking `text`; its samples come from the stream as they
    /// arrive. Dropping the stream stops the download.
    pub async fn speak(&self, text: &str) -> Result<GeminiSpeech> {
        Ok(self.speak_permitted(text, None).await?.unwrap_or_default())
    }

    pub(crate) async fn speak_permitted(
        &self,
        text: &str,
        permitted: Option<&ChatPermission>,
    ) -> Result<Option<GeminiSpeech>> {
        let Some(body) = body(&self.voice, text) else {
            return Ok(Some(GeminiSpeech::default()));
        };
        if let Ok(mut used) = self.used.lock() {
            *used = Some(Instant::now());
        }
        let Some(response) = permitted_result(
            post_guarded(
                &self.client,
                &self.url,
                ("x-goog-api-key", &self.key),
                &body,
                permitted,
                #[cfg(test)]
                None,
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
        Ok(Some(GeminiSpeech {
            response: Some(response),
            ..GeminiSpeech::default()
        }))
    }
}

pub(crate) fn url(base_url: &str, model: &str) -> String {
    format!(
        "{}/models/{model}:streamGenerateContent?alt=sse",
        base_url.trim_end_matches('/')
    )
}

/// The request for `text` in `voice`; `None` when nothing would be said.
pub(crate) fn body(voice: &str, text: &str) -> Option<Value> {
    let spoken = without_tags(text);
    let spoken = spoken.trim();
    (!spoken.is_empty()).then(|| {
        json!({
            "contents": [{"role": "user", "parts": [{"text": spoken}]}],
            "generationConfig": {
                "responseModalities": ["AUDIO"],
                "speechConfig": {"voiceConfig": {"prebuiltVoiceConfig": {"voiceName": voice}}},
            },
        })
    })
}

/// The audio bytes of one event's `data:`: every inline audio part, in
/// order. Audio at another rate than 24 kHz, or an error, is refused.
pub(crate) fn audio_of(data: &str) -> Result<Vec<u8>> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let event: Value =
        serde_json::from_str(data).map_err(|_| Error::Protocol("speech stream data"))?;
    if event.get("error").is_some() {
        return Err(Error::Protocol("speech stream error"));
    }
    let mut bytes = Vec::new();
    for candidate in event["candidates"].as_array().into_iter().flatten() {
        for part in candidate
            .pointer("/content/parts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(inline) = part.get("inlineData") else {
                continue;
            };
            let mime = inline["mimeType"].as_str().unwrap_or("");
            // No type said is taken as the documented 24 kHz PCM. A type is
            // read without case: through the relay it is `audio/l16;
            // rate=24000; channels=1`, refused until 19 Oct 16:09 on
            // lamp-4ace, so most answers fell back to ElevenLabs (#1712).
            let lower = mime.to_ascii_lowercase();
            let pcm_24k = lower.starts_with("audio/l16") && lower.contains("rate=24000");
            if !(mime.is_empty() || pcm_24k) {
                eprintln!(
                    "lamp-voice second: Gemini's speech came as {:?}, not 24 kHz 16-bit PCM",
                    mime.chars().take(80).collect::<String>()
                );
                return Err(Error::Protocol("speech audio format"));
            }
            let data = inline["data"]
                .as_str()
                .ok_or(Error::Protocol("speech audio part"))?;
            bytes.extend(
                STANDARD
                    .decode(data)
                    .map_err(|_| Error::Protocol("speech audio part"))?,
            );
        }
    }
    Ok(bytes)
}

#[derive(Default)]
pub struct GeminiSpeech {
    response: Option<reqwest::Response>,
    lines: Lines,
    packets: Packets,
    ready: VecDeque<Vec<i16>>,
}

impl GeminiSpeech {
    /// The next packet of at most [`super::PACKET_SAMPLES`]; `None` at the
    /// end. Played at its own level, as Live's voice is (no gain is added,
    /// unlike the other voices; not measured on a lamp).
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
                    for data in self.lines.push(&bytes) {
                        let audio = audio_of(&data)?;
                        self.ready.extend(self.packets.push(&audio));
                    }
                    if self.lines.pending() > MAX_LINE {
                        return Err(Error::Protocol("speech stream line length"));
                    }
                }
                None => {
                    self.response = None;
                    self.ready.extend(self.packets.finish());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn a_sentence_is_asked_in_the_live_voice_with_audio_out() {
        assert_eq!(
            url(
                "https://generativelanguage.googleapis.com/v1beta/",
                "gemini-3.8-flash-lite-tts"
            ),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.8-flash-lite-tts:streamGenerateContent?alt=sse"
        );
        assert_eq!(
            body("Kore", " It is [laughs] noon. ").unwrap(),
            json!({
                "contents": [{"role": "user", "parts": [{"text": "It is  noon."}]}],
                "generationConfig": {
                    "responseModalities": ["AUDIO"],
                    "speechConfig": {"voiceConfig": {"prebuiltVoiceConfig": {"voiceName": "Kore"}}},
                },
            })
        );
        assert_eq!(body("Kore", " [sigh] "), None);
        assert!(!GeminiTts::speaks("[sigh]"));
        assert!(GeminiTts::speaks("Hi."));
        assert!(GeminiTts::new("https://x.invalid", "m", "k", "../x").is_err());
    }

    fn event(samples: &[i16]) -> String {
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        json!({"candidates": [{"content": {"role": "model", "parts": [
            {"inlineData": {"mimeType": "audio/L16;codec=pcm;rate=24000", "data": STANDARD.encode(bytes)}}
        ]}}]})
        .to_string()
    }

    /// #1712: the relay's type, in lowercase with its channels, is the
    /// 24 kHz PCM it says; another rate is still refused.
    #[test]
    fn the_relays_lowercase_type_is_taken() {
        let part = |mime: &str| {
            json!({"candidates": [{"content": {"parts": [
                {"inlineData": {"mimeType": mime, "data": STANDARD.encode([1u8, 0, 254, 255])}}
            ]}}]})
            .to_string()
        };
        for mime in [
            "audio/l16; rate=24000; channels=1",
            "audio/L16;codec=pcm;rate=24000",
            "AUDIO/L16; RATE=24000",
        ] {
            assert_eq!(audio_of(&part(mime)).unwrap(), [1, 0, 254, 255], "{mime}");
        }
        assert_eq!(
            audio_of(&part("audio/l16; rate=16000; channels=1")).unwrap_err(),
            Error::Protocol("speech audio format")
        );
    }

    #[test]
    fn audio_parts_are_decoded_and_other_rates_refused() {
        assert_eq!(audio_of(&event(&[1, -2])).unwrap(), [1, 0, 254, 255]);
        assert_eq!(
            audio_of(r#"{"candidates":[{"content":{"parts":[{"text":"hm"}]}}]}"#).unwrap(),
            Vec::<u8>::new()
        );
        let other = r#"{"candidates":[{"content":{"parts":[{"inlineData":{"mimeType":"audio/L16;codec=pcm;rate=16000","data":"AAA="}}]}}]}"#;
        assert_eq!(
            audio_of(other).unwrap_err(),
            Error::Protocol("speech audio format")
        );
        assert_eq!(
            audio_of(r#"{"error":{"code":429}}"#).unwrap_err(),
            Error::Protocol("speech stream error")
        );
    }

    /// #1712: a warming `GET` of the model opens the connection, and the
    /// next sentence goes on it: the server takes one connection only. It is
    /// not asked again while the connection was used within [`WARM_AFTER`].
    #[tokio::test]
    async fn the_connection_opened_ahead_is_the_one_the_sentence_takes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let audio = format!("data: {}\r\n\r\n", event(&[5; 960]));
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = socket.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..n]);
            }
            let warmed = String::from_utf8_lossy(&request).into_owned();
            socket
                .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 2\r\n\r\n{}")
                .await
                .unwrap();
            let mut request = Vec::new();
            while !String::from_utf8_lossy(&request).contains("voiceName") {
                let n = socket.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..n]);
            }
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n",
                audio.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(audio.as_bytes()).await.unwrap();
            warmed
        });
        let tts = GeminiTts::new(
            &format!("http://{address}/v1beta"),
            "m-tts",
            "g-key",
            "Kore",
        )
        .unwrap();
        let (status, _) = tts.warm().await.unwrap();
        assert_eq!(status, 404);
        assert_eq!(tts.warm().await, None, "used a moment ago");
        let mut speech = tts.speak("Hi.").await.unwrap();
        assert_eq!(speech.next().await.unwrap(), Some(vec![5; 960]));
        let warmed = server.await.unwrap();
        assert!(
            warmed.starts_with("GET /v1beta/models/m-tts HTTP/1.1"),
            "{warmed}"
        );
        assert!(warmed.contains("x-goog-api-key: g-key"));
        let asking = tts.asking();
        assert_eq!((asking.others, tts.asking().others), (0, 1));
        drop(asking);
        assert_eq!(tts.asking().others, 0);
    }

    /// A local plain HTTP server streams two events cut mid-event: the
    /// samples come out whole, in packets, with the key in its header.
    #[tokio::test]
    async fn streamed_speech_is_read_over_http_in_packets() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let first: Vec<i16> = (0..1_000).map(|i| i as i16).collect();
        let second: Vec<i16> = (1_000..1_500).map(|i| i as i16).collect();
        let stream = format!(
            "data: {}\r\n\r\ndata: {}\r\n\r\n",
            event(&first),
            event(&second)
        );
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            while !String::from_utf8_lossy(&request).contains("voiceName") {
                let n = socket.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..n]);
            }
            let (a, b) = stream.as_bytes().split_at(stream.len() / 2);
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            socket.write_all(a).await.unwrap();
            socket.flush().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            socket.write_all(b).await.unwrap();
            String::from_utf8(request).unwrap()
        });
        let tts = GeminiTts::new(
            &format!("http://{address}/v1beta"),
            "tts-model",
            "g-key",
            "Kore",
        )
        .unwrap();
        let mut speech = tts.speak("Hello there.").await.unwrap();
        let mut packets = Vec::new();
        while let Some(packet) = speech.next().await.unwrap() {
            packets.push(packet);
        }
        assert_eq!(
            packets.iter().map(Vec::len).collect::<Vec<_>>(),
            [960, 540],
            "whole packets of 40 ms, the rest last"
        );
        assert_eq!(packets.concat(), [first, second].concat());
        let request = server.await.unwrap().to_lowercase();
        assert!(
            request.starts_with(
                "post /v1beta/models/tts-model:streamgeneratecontent?alt=sse http/1.1"
            ),
            "{request}"
        );
        assert!(request.contains("x-goog-api-key: g-key"));
    }
}
