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
    chunk, http_client, post,
    tts::{Packets, without_tags},
};
use serde_json::{Value, json};
use std::collections::VecDeque;

/// A server-sent event of base64 audio stays far under this; a stream
/// without line ends does not.
const MAX_LINE: usize = 4 << 20;

pub struct GeminiTts {
    client: reqwest::Client,
    url: String,
    key: String,
    voice: String,
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
            key: key.to_owned(),
            voice: voice.to_owned(),
        })
    }

    /// Whether `text` has anything to say: text in `[...]` is not spoken,
    /// as for the other voices.
    pub fn speaks(text: &str) -> bool {
        !without_tags(text).trim().is_empty()
    }

    /// Start speaking `text`; its samples come from the stream as they
    /// arrive. Dropping the stream stops the download.
    pub async fn speak(&self, text: &str) -> Result<GeminiSpeech> {
        let Some(body) = body(&self.voice, text) else {
            return Ok(GeminiSpeech::default());
        };
        let response = post(
            &self.client,
            &self.url,
            ("x-goog-api-key", &self.key),
            &body,
        )
        .await?;
        Ok(GeminiSpeech {
            response: Some(response),
            ..GeminiSpeech::default()
        })
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
            // No type said is taken as the documented 24 kHz PCM.
            let pcm_24k = mime.starts_with("audio/L16") && mime.contains("rate=24000");
            if !(mime.is_empty() || pcm_24k) {
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
