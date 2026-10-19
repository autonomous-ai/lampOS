//! One line rendered in the lamp's own voice, at build time, for a
//! recording the lamp plays when it cannot reach its voice (#1959: the
//! Wi-Fi helper's "I can't reach any Wi-Fi" line).
//!
//! ```sh
//! cargo run --locked -p lamp-gemini --example render_line -- \
//!     /absolute/private/provider.json NEW_FOLDER KIND "The words to say." production|test
//! ```
//!
//! The provider file is the voice's own (`deploy/product/settings/voice-provider.json`
//! with its credential file beside it, mode 0600): the same route, model and
//! voice the lamp speaks with. The line goes as one written turn, as the
//! say service's `text` does ("Say exactly these words, in your own voice,
//! and nothing else:"), and the reply's audio is kept as it came, 24 kHz
//! mono 16-bit, with the silence before and after it cut, as
//! `KIND.en-US.wav` (`wifi_lost` gives `wifi-lost.en-US.wav`) in
//! NEW_FOLDER, which it makes and which must not exist. Beside it, it writes
//! the failure-notice manifest (`docs/failure-notices.md`): the kind, the
//! words as the transcript, the provider file's voice and model (as
//! `voice_revision`), the approval, the file, its SHA-256 and its frames.
//! It prints what it rendered and the service's own transcript of what it
//! said, to be compared with the words by whoever approves the recording.
//! It prints no credential and opens no microphone or speaker.
//! `tools/render-wifi-line.sh` runs it and then the loader's own check.
use lamp_gemini::{Event, OUTPUT_RATE, RequestId, Resumption, SessionId};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{error::Error, fs, path::Path, time::Duration};

// Only `session` is used here; the shared file serves the probes too.
#[allow(dead_code)]
#[path = "shared/provider.rs"]
mod provider;

type Outcome<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// As the say service's `text` (`lamp_voice::say::AS_WRITTEN`).
const AS_WRITTEN: &str = "Say exactly these words, in your own voice, and nothing else:";
/// The session's instruction: a lamp that says what it is given.
const INSTRUCTION: &str = "You are a small desk lamp that talks. When asked to say words, say \
     exactly those words, calmly and kindly, and nothing else.";
/// The recording's language, as the voice loads it (`--failure-notice-language`).
const LANGUAGE: &str = "en-US";

/// The provider file's fields the manifest names; the rest is the session's.
#[derive(Deserialize)]
struct Named {
    voice: String,
    model: String,
}

/// Below this a sample is silence, for the cut at either end (about -50 dBFS).
const QUIET: i16 = 100;

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
    let keep = OUTPUT_RATE as usize / 20;
    let loud = |sample: &i16| sample.unsigned_abs() > QUIET as u16;
    let (Some(first), Some(last)) = (
        samples.iter().position(loud),
        samples.iter().rposition(loud),
    ) else {
        return &[];
    };
    &samples[first.saturating_sub(keep)..(last + keep + 1).min(samples.len())]
}

fn main() -> Outcome<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Outcome<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [provider, folder, kind, words, approval] = arguments.as_slice() else {
        return Err(
            "usage: render_line PRIVATE_PROVIDER.json NEW_FOLDER KIND WORDS production|test".into(),
        );
    };
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
    if !matches!(approval.as_str(), "production" | "test") {
        return Err("the approval is production or test".into());
    }
    let folder = Path::new(folder);
    if !folder.is_absolute() || folder.exists() {
        return Err(
            "NEW_FOLDER is an absolute path that is not there; nothing is overwritten".into(),
        );
    }
    let named: Named = serde_json::from_str(&provider::private(Path::new(provider), 16_384)?)
        .map_err(|_| "invalid private provider configuration")?;
    let config = provider::session(Path::new(provider))?
        .instruction(INSTRUCTION)?
        .resumption(Resumption::Retain);
    let mut connection = lamp_gemini::connect(config, SessionId::new(1)?).await?;
    let request = RequestId::new(1)?;
    connection
        .input()
        .try_written(request, &format!("{AS_WRITTEN} {words}"), None)?;
    let (mut samples, mut transcript) = (Vec::new(), String::new());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(40);
    loop {
        match tokio::time::timeout_at(deadline, connection.next_event()).await {
            Err(_) => return Err("no whole reply within 40 s".into()),
            Ok(None) => return Err("the connection ended before the reply did".into()),
            Ok(Some(Event::Audio { pcm, .. })) => samples.extend_from_slice(&pcm),
            Ok(Some(Event::OutputTranscript { text, .. })) => transcript.push_str(&text),
            Ok(Some(Event::Interrupted { .. } | Event::Unanswered { .. })) => {
                return Err("the reply was cut or never came".into());
            }
            Ok(Some(Event::TurnComplete { idle: true, .. })) => break,
            Ok(Some(_)) => {}
        }
    }
    let line = trimmed(&samples);
    if line.is_empty() {
        return Err("the reply was silent".into());
    }
    let file = format!("{}.{LANGUAGE}.wav", kind.replace('_', "-"));
    let bytes = wav(OUTPUT_RATE, line);
    let sha: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let manifest = serde_json::json!({"schema_version": 1, "notices": [{
        "kind": kind, "language": LANGUAGE, "transcript": words, "voice": named.voice,
        "voice_revision": named.model, "approval": approval, "file": file,
        "wav_sha256": sha, "frames": line.len()}]});
    fs::create_dir(folder)?;
    fs::write(folder.join(&file), bytes)?;
    fs::write(
        folder.join("manifest.json"),
        format!("{}\n", serde_json::to_string_pretty(&manifest)?),
    )?;
    println!(
        "rendered: {} frames, {:.2} s ({}, {}), sha256 {sha}",
        line.len(),
        line.len() as f64 / f64::from(OUTPUT_RATE),
        named.voice,
        named.model
    );
    println!("the service heard itself say: {}", transcript.trim());
    println!("the words asked:              {words}");
    Ok(())
}
