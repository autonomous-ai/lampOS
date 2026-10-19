//! Streamed speech as 24 kHz mono 16-bit PCM from OpenAI or ElevenLabs, as
//! V1's `hal/drivers/voice/tts/{openai,elevenlabs}.py` ask for it.
use super::{Error, Result, chunk, http_client, openai_base, post};
use crate::second_config::{SecondConfig, TtsProvider};
use serde_json::{Value, json};

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
    text: &str,
) -> Result<Option<Request>> {
    let base = openai_base(base_url);
    let spoken = without_tags(text);
    if spoken.trim().is_empty() {
        return Ok(None);
    }
    let request = match provider {
        // ponytail: every `[...]` is dropped, where V1 drops only its list
        // of ElevenLabs emotion tags; narrow it if bracketed words are lost.
        TtsProvider::OpenAi => Request {
            url: format!("{base}/audio/speech"),
            header: "authorization",
            value: format!("Bearer {key}"),
            body: json!({
                "model": OPENAI_MODEL,
                "input": spoken.trim(),
                "voice": voice.unwrap_or(OPENAI_VOICE),
                "response_format": "pcm",
            }),
        },
        TtsProvider::ElevenLabs => {
            let host = reqwest::Url::parse(&base).map_err(|_| Error::Url)?;
            let direct = host
                .host_str()
                .is_some_and(|h| ELEVENLABS_HOSTS.iter().any(|d| h.eq_ignore_ascii_case(d)));
            let base = if direct {
                base
            } else {
                format!("{base}/elevenlabs")
            };
            let voice = voice.unwrap_or(ELEVENLABS_VOICES[0].0);
            let id = ELEVENLABS_VOICES
                .iter()
                .find(|(name, _)| *name == voice)
                .map_or(voice, |(_, id)| id);
            if !id.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(Error::Url);
            }
            Request {
                url: format!("{base}/text-to-speech/{id}/stream?output_format=pcm_24000"),
                header: "xi-api-key",
                value: key.to_owned(),
                // Speed 1.0 is given because, absent, the voice's stored
                // speed applies (V1).
                body: json!({
                    "text": text,
                    "model_id": ELEVENLABS_MODEL,
                    "voice_settings": {"speed": 1.0},
                }),
            }
        }
    };
    Ok(Some(request))
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

pub struct Tts {
    client: reqwest::Client,
    provider: TtsProvider,
    base_url: String,
    key: String,
    voice: Option<String>,
}

impl Tts {
    pub fn new(config: &SecondConfig) -> Result<Self> {
        Self::with(
            config.tts_provider,
            &config.tts_base_url,
            config.tts_key.expose(),
            config.tts_voice.as_deref(),
        )
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
        })
    }

    /// Start speaking `text`; its samples come from the stream as they
    /// arrive. Dropping the stream stops the download.
    pub async fn speak(&self, text: &str) -> Result<TtsStream> {
        let Some(request) = request(
            self.provider,
            &self.base_url,
            &self.key,
            self.voice.as_deref(),
            text,
        )?
        else {
            return Ok(TtsStream::default());
        };
        let response = post(
            &self.client,
            &request.url,
            (request.header, &request.value),
            &request.body,
        )
        .await?;
        Ok(TtsStream {
            response: Some(response),
            ..TtsStream::default()
        })
    }
}

#[derive(Default)]
pub struct TtsStream {
    response: Option<reqwest::Response>,
    packets: Packets,
    ready: std::collections::VecDeque<Vec<i16>>,
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
                Some(bytes) => self
                    .ready
                    .extend(self.packets.push(&bytes).into_iter().map(louder)),
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

    const PROXY: &str = "https://campaign-api.autonomous.ai/api/v1/ai";

    #[test]
    fn openai_speech_is_asked_as_v1_did() {
        let request = request(TtsProvider::OpenAi, PROXY, "k", None, "Hi [laughs] there.")
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
        let proxied = request(TtsProvider::ElevenLabs, PROXY, "k", None, "Hi.")
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
            "Hello.",
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            direct.url,
            "https://api.elevenlabs.io/v1/text-to-speech/a3AkyqGG4v8Pg7SWQ0Y3/stream?output_format=pcm_24000"
        );
        let by_id = request(TtsProvider::ElevenLabs, PROXY, "k", Some("abc123"), "Hi.")
            .unwrap()
            .unwrap();
        assert!(by_id.url.contains("/text-to-speech/abc123/stream"));
        assert_eq!(
            request(TtsProvider::ElevenLabs, PROXY, "k", Some("../x"), "Hi.").unwrap_err(),
            Error::Url
        );
    }

    #[test]
    fn only_tags_ask_for_nothing() {
        for provider in [TtsProvider::OpenAi, TtsProvider::ElevenLabs] {
            assert_eq!(
                request(provider, PROXY, "k", None, " [sigh] ").unwrap(),
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
