//! What the service's transcript of a spoken clip says, with and without a
//! language hint (#926: English said with a Vietnamese accent came back in
//! Devanagari, and the lamp held it as heard in another script).
//!
//! ```sh
//! cargo run --locked -p lamp-gemini --example transcript_probe -- \
//!     /absolute/private/provider.json /path/to/clip.wav [LANGUAGE]
//! ```
//!
//! The clip is spoken once in each of three sessions: with no hint, with
//! `LANGUAGE` (`en-US` unless given) as the speech language
//! (`speechConfig.languageCode`), and with it as the transcript's
//! (`inputAudioTranscription.languageCodes`). For each it prints one JSON
//! line: whether the session was taken, the input transcript, its letters
//! counted as Latin or not, and the model's answer. It prints the transcript
//! and the answer, so speak only words fit to appear in a log. It prints no
//! credential and opens no microphone or speaker.
use lamp_gemini::{Event, RequestId, Resumption, SessionConfig, SessionId};
use std::{
    error::Error,
    path::Path,
    time::{Duration, Instant},
};

#[path = "shared/provider.rs"]
mod provider;

use provider::{session, speech, trim};

type Outcome<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Latin letters as `not_speech` counts them: ASCII and the Latin
/// supplements and extensions.
fn latin(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c as u32, 0x00C0..=0x024F | 0x1E00..=0x1EFF)
}

async fn spoken(config: SessionConfig, clip: &[i16], hint: &str) -> serde_json::Value {
    let started = Instant::now();
    let mut connection = match lamp_gemini::connect(
        config.resumption(Resumption::Retain),
        SessionId::new(1).unwrap(),
    )
    .await
    {
        Ok(connection) => connection,
        Err(error) => {
            return serde_json::json!({"hint": hint, "taken": false, "error": format!("{error:?}")});
        }
    };
    let request = RequestId::new(1).unwrap();
    let input = connection.input();
    let mut sent = input.try_start(request).map(|_| ());
    for (sequence, block) in clip.chunks(160).enumerate() {
        if sent.is_err() {
            break;
        }
        sent = input
            .try_audio(request, sequence as u64, Instant::now(), block)
            .map(|_| ());
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    if sent.is_ok() {
        sent = input.try_end(request).map(|_| ());
    }
    let (mut heard, mut answer, mut ended) = (String::new(), String::new(), "window");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        match tokio::time::timeout_at(deadline, connection.next_event()).await {
            Err(_) => break,
            Ok(None) => {
                ended = "connection_ended";
                break;
            }
            Ok(Some(Event::UncorrelatedInputTranscript { text, .. })) => heard.push_str(&text),
            Ok(Some(Event::OutputTranscript { text, .. })) => answer.push_str(&text),
            Ok(Some(Event::TurnComplete { idle: true, .. })) => {
                ended = "turn_complete";
                break;
            }
            Ok(Some(_)) => {}
        }
    }
    let letters = heard.chars().filter(|c| c.is_alphabetic()).count();
    let other = heard
        .chars()
        .filter(|c| c.is_alphabetic() && !latin(*c))
        .count();
    serde_json::json!({
        "hint": hint,
        "taken": ended != "connection_ended" || !heard.is_empty() || !answer.is_empty(),
        "sent": sent.err().map(|error| format!("{error:?}")),
        "ended": ended,
        "state": format!("{:?}", connection.state()),
        "heard": heard.trim(),
        "letters": letters,
        "not_latin": other,
        "answer": answer.trim(),
        "seconds": started.elapsed().as_secs_f64(),
    })
}

fn main() -> Outcome<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Outcome<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (provider, wav, language) = match arguments.as_slice() {
        [provider, wav] => (provider, wav, "en-US"),
        [provider, wav, language] => (provider, wav, language.as_str()),
        _ => {
            return Err("usage: transcript_probe PRIVATE_PROVIDER.json CLIP.wav [LANGUAGE]".into());
        }
    };
    let decoded = speech(Path::new(wav))?;
    let clip = trim(&decoded);
    if clip.len() < 8_000 || clip.len() > 16_000 * 30 {
        return Err("the clip must hold between 0.5 and 30 seconds of speech".into());
    }
    let base = || session(Path::new(provider));
    let runs = [
        ("none".to_owned(), base()?),
        (
            format!("speechConfig.languageCode={language}"),
            base()?.language_code(language)?,
        ),
        (
            format!("inputAudioTranscription.languageCodes=[{language}]"),
            base()?.transcription_language(language)?,
        ),
    ];
    for (hint, config) in runs {
        println!("{}", spoken(config, clip, &hint).await);
    }
    Ok(())
}
