//! One line rendered in the second route's configured voice (ElevenLabs on
//! the product, #1712), at build time, for a recording the lamp plays when
//! it cannot reach its voice (#1844: the failure notices). It is
//! `lamp-gemini`'s `render_line` for the voice every lamp speaks with now,
//! so an offline lamp sounds as it does online. It runs only when named.
//!
//! ```sh
//! LAMP_SECOND_SPEECH_MODEL=eleven_v4_turbo \
//! cargo run --locked -p lamp-voice --example render_line_second -- \
//!     /etc/lampOS/voice/second.json NEW_FOLDER KIND "The words to say." production|test
//! ```
//!
//! The file is the voice's own second route (`deploy/product/settings/second.json`,
//! with its credential file beside it, mode 0600), read as the voice reads
//! it, with the voice unit's `LAMP_SECOND_SPEECH_MODEL` from the
//! environment when set: the same provider, voice and model the lamp
//! speaks with. The words go as one request to that voice ([`Tts::speak`]),
//! and its audio is kept as the lamp plays it (24 kHz mono 16-bit, with the
//! route's own gain), with the silence before and after it cut, as
//! `KIND.en-US.wav` in NEW_FOLDER, which it makes and which must not exist.
//! Beside it, the failure-notice manifest (`docs/failure-notices.md`), as
//! `render_line` writes it: the voice's name as `voice` and its model as
//! `voice_revision`. It prints what it rendered, never a credential, and
//! opens no microphone or speaker.
use lamp_voice::second_config::load_from_path;
use lamp_voice::second_provider::net::{TTS_SAMPLE_RATE, Tts, default_model, voice_name};
use sha2::{Digest, Sha256};
use std::{error::Error, fs, path::Path, time::Duration};

type Outcome<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// The recording's language, as the voice loads it (`--failure-notice-language`).
const LANGUAGE: &str = "en-US";
/// Below this a sample is silence, for the cut at either end (about -50 dBFS).
const QUIET: u16 = 100;

/// A 44-byte RIFF/WAVE header and the samples, mono 16-bit at `rate`.
fn wav(rate: u32, samples: &[i16]) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

/// The samples with the silence at either end cut, 50 ms kept each side.
fn trimmed(samples: &[i16]) -> &[i16] {
    let keep = TTS_SAMPLE_RATE as usize / 20;
    let loud = |sample: &i16| sample.unsigned_abs() > QUIET;
    let (Some(first), Some(last)) = (
        samples.iter().position(loud),
        samples.iter().rposition(loud),
    ) else {
        return &[];
    };
    &samples[first.saturating_sub(keep)..(last + keep + 1).min(samples.len())]
}

/// The arguments' own checks, as `render_line` makes them.
fn checked(kind: &str, words: &str, approval: &str) -> Outcome<()> {
    if words.trim().is_empty() || words.len() > 240 || words.chars().any(char::is_control) {
        return Err("the words are 1 to 240 bytes with no control characters".into());
    }
    if kind.is_empty()
        || kind.len() > 64
        || !kind
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    {
        return Err("the kind is a failure notice's name, a-z and _".into());
    }
    if !matches!(approval, "production" | "test") {
        return Err("the approval is production or test".into());
    }
    Ok(())
}

/// The recording's file name: `wifi_lost` gives `wifi-lost.en-US.wav`.
fn file_name(kind: &str) -> String {
    format!("{}.{LANGUAGE}.wav", kind.replace('_', "-"))
}

/// The WAV's SHA-256, in hex.
fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// One rendered notice, as its manifest names it.
struct Notice<'a> {
    kind: &'a str,
    words: &'a str,
    voice: &'a str,
    model: &'a str,
    approval: &'a str,
}

impl Notice<'_> {
    /// The manifest of this notice alone, as `render_line` writes it.
    fn manifest(&self, sha: &str, frames: usize) -> serde_json::Value {
        serde_json::json!({"schema_version": 1, "notices": [{
            "kind": self.kind, "language": LANGUAGE, "transcript": self.words,
            "voice": self.voice, "voice_revision": self.model, "approval": self.approval,
            "file": file_name(self.kind), "wav_sha256": sha, "frames": frames}]})
    }
}

fn main() -> Outcome<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Outcome<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [second, folder, kind, words, approval] = arguments.as_slice() else {
        return Err(
            "usage: render_line_second SECOND.json NEW_FOLDER KIND WORDS production|test".into(),
        );
    };
    checked(kind, words, approval)?;
    let folder = Path::new(folder);
    if !folder.is_absolute() || folder.exists() {
        return Err(
            "NEW_FOLDER is an absolute path that is not there; nothing is overwritten".into(),
        );
    }
    let config =
        load_from_path(Path::new(second))?.with_environment(|name: &str| std::env::var_os(name))?;
    let voice = voice_name(config.tts_provider, config.tts_voice.as_deref()).to_owned();
    let model = config
        .tts_model
        .clone()
        .unwrap_or_else(|| default_model(config.tts_provider).to_owned());
    let tts = Tts::new(&config)?;
    let mut samples = Vec::new();
    let whole = async {
        let mut speech = tts.speak(words).await?;
        while let Some(packet) = speech.next().await? {
            samples.extend_from_slice(&packet);
        }
        Ok::<_, Box<dyn Error + Send + Sync>>(())
    };
    tokio::time::timeout(Duration::from_secs(40), whole)
        .await
        .map_err(|_| "no whole reply within 40 s")??;
    let line = trimmed(&samples);
    if line.is_empty() {
        return Err("the reply was silent".into());
    }
    let file = file_name(kind);
    let bytes = wav(TTS_SAMPLE_RATE, line);
    let sha = sha256(&bytes);
    let notice = Notice {
        kind,
        words,
        voice: &voice,
        model: &model,
        approval,
    };
    let manifest = notice.manifest(&sha, line.len());
    fs::create_dir(folder)?;
    fs::write(folder.join(&file), bytes)?;
    fs::write(
        folder.join("manifest.json"),
        format!("{}\n", serde_json::to_string_pretty(&manifest)?),
    )?;
    println!(
        "rendered: {} frames, {:.2} s ({}, {voice}, {model}), sha256 {sha}",
        line.len(),
        line.len() as f64 / f64::from(TTS_SAMPLE_RATE),
        config.tts_provider.name(),
    );
    println!("the words asked: {words}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wav_is_mono_16_bit_at_the_routes_rate() {
        let bytes = wav(TTS_SAMPLE_RATE, &[1, -2, 3]);
        assert_eq!(bytes.len(), 44 + 6);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 1);
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            24_000
        );
        assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16);
        assert_eq!(&bytes[44..], &[1, 0, 254, 255, 3, 0]);
    }

    #[test]
    fn silence_is_cut_at_either_end_with_50_ms_kept() {
        let mut samples = vec![0i16; 5_000];
        samples.extend([1_000i16; 10]);
        samples.extend(vec![0i16; 5_000]);
        let line = trimmed(&samples);
        assert_eq!(line.len(), 1_200 + 10 + 1_200);
        assert!(trimmed(&[0, 5, -5]).is_empty());
    }

    #[test]
    fn the_arguments_are_checked_as_render_lines_are() {
        assert!(checked("wifi_lost", "I can't reach any Wi-Fi.", "production").is_ok());
        assert!(checked("wifi-lost", "Hi.", "test").is_err());
        assert!(checked("wifi_lost", " ", "test").is_err());
        assert!(checked("wifi_lost", "a\nb", "test").is_err());
        assert!(checked("wifi_lost", "Hi.", "approved").is_err());
        assert_eq!(
            file_name("voice_service_unreachable"),
            "voice-service-unreachable.en-US.wav"
        );
    }

    #[test]
    fn the_manifest_names_the_configured_voice_and_model() {
        let value = Notice {
            kind: "back_online",
            words: "I'm back online.",
            voice: voice_name(lamp_voice::second_config::TtsProvider::ElevenLabs, None),
            model: "eleven_v4_turbo",
            approval: "production",
        }
        .manifest(&sha256(b""), 7);
        let notice = &value["notices"][0];
        assert_eq!(value["schema_version"], 1);
        assert_eq!(notice["voice"], "Rachel");
        assert_eq!(notice["voice_revision"], "eleven_v4_turbo");
        assert_eq!(notice["language"], "en-US");
        assert_eq!(notice["frames"], 7);
        assert_eq!(notice["file"], "back-online.en-US.wav");
        assert_eq!(
            notice["wav_sha256"],
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
