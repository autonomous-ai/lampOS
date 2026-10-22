//! Test people's lines, each in a voice other than the lamp's, rendered
//! through the voice's own speech route (the second route's `second.json`:
//! ElevenLabs through the relay), for played tests: the lamp hears people
//! who sound like people, never itself. It is `render_line_second` for many
//! lines in many voices. It runs only when named.
//!
//! ```sh
//! LAMP_SECOND_SPEECH_MODEL=eleven_v4_turbo \
//! cargo run --locked -p lamp-voice --example render_people_lines -- \
//!     /etc/lampOS/voice/second.json LINES.tsv OUT_FOLDER VOICES.tsv
//! ```
//!
//! The file is read as the voice reads it, with the voice unit's
//! `LAMP_SECOND_SPEECH*` from the environment when set; only the voice is
//! changed, line by line. LINES.tsv is `id<TAB>voice<TAB>kind<TAB>text`,
//! `#` lines skipped; a `{N}` in the text is N seconds of silence between
//! the pieces either side, each piece its own request ([`Tts::speak`]) with
//! the silence at either end of its reply cut. VOICES.tsv is
//! `name<TAB>ElevenLabs voice id`. A voice that is the lamp's own, by name
//! or id, is refused before anything is asked.
//!
//! Each line becomes `OUT_FOLDER/<id>.wav`, 24 kHz mono 16-bit as the lamp
//! plays it, with 0.25 s of silence at either end; a line whose WAV is
//! there is not asked again. `OUT_FOLDER/index.tsv` lists every WAV there
//! is: id, voice, voice id, kind, the text as written, seconds, sha256 (a
//! kept WAV under the voice its line names now). It prints one line per
//! WAV, never a credential or a URL, and opens no microphone or speaker.
use lamp_voice::second_config::{SecondConfig, TtsProvider, load_from_path};
use lamp_voice::second_provider::net::{TTS_SAMPLE_RATE, Tts, default_model, voice_name};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    path::Path,
    time::Duration,
};

type Outcome<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Below this a sample is silence, for the cut at either end (about -50 dBFS).
const QUIET: u16 = 100;
/// The silence at either end of a line, in seconds.
const EDGE_S: f64 = 0.25;
/// The longest pause a `{N}` may ask for, in seconds.
const LONGEST_PAUSE_S: f64 = 10.0;
/// The longest a piece's whole reply may take.
const PIECE_WITHIN: Duration = Duration::from_secs(40);

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

/// The seconds of audio in a WAV that [`wav`] wrote.
fn seconds_of(bytes: &[u8]) -> f64 {
    bytes.len().saturating_sub(44) as f64 / 2.0 / f64::from(TTS_SAMPLE_RATE)
}

/// The samples from the first above [`QUIET`] to the last: a reply with
/// the silence at either end cut, so a pause is the `{N}` asked and no more.
fn trimmed(samples: &[i16]) -> &[i16] {
    let loud = |sample: &i16| sample.unsigned_abs() > QUIET;
    match (
        samples.iter().position(loud),
        samples.iter().rposition(loud),
    ) {
        (Some(first), Some(last)) => &samples[first..=last],
        _ => &[],
    }
}

/// `seconds` of silence at the route's rate.
fn silence(seconds: f64) -> Vec<i16> {
    vec![0; (seconds * f64::from(TTS_SAMPLE_RATE)).round() as usize]
}

/// The pieces said, each after the pause before it, with [`EDGE_S`] at
/// either end: `pauses` has one fewer than `pieces`.
fn joined(pieces: &[Vec<i16>], pauses: &[f64]) -> Vec<i16> {
    let mut samples = silence(EDGE_S);
    for (index, piece) in pieces.iter().enumerate() {
        if let Some(pause) = index.checked_sub(1).and_then(|before| pauses.get(before)) {
            samples.extend(silence(*pause));
        }
        samples.extend_from_slice(piece);
    }
    samples.extend(silence(EDGE_S));
    samples
}

/// The words between the `{N}` markers, each trimmed, and the pauses
/// between them in seconds.
fn pieces(text: &str) -> Outcome<(Vec<&str>, Vec<f64>)> {
    let (mut words, mut pauses) = (Vec::new(), Vec::new());
    let mut rest = text;
    loop {
        let (piece, after) = match rest.find('{') {
            Some(open) => (&rest[..open], Some(&rest[open + 1..])),
            None => (rest, None),
        };
        let piece = piece.trim();
        if piece.is_empty() || piece.contains('}') {
            return Err(format!("{text:?}: every piece has words and no lone }}").into());
        }
        words.push(piece);
        let Some(after) = after else {
            return Ok((words, pauses));
        };
        let close = after
            .find('}')
            .ok_or_else(|| format!("{text:?}: a {{ with no }}"))?;
        let asked = &after[..close];
        let pause = Some(asked)
            .filter(|asked| {
                !asked.is_empty() && asked.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            })
            .and_then(|asked| asked.parse::<f64>().ok())
            .filter(|pause| (0.0..=LONGEST_PAUSE_S).contains(pause))
            .ok_or_else(|| format!("{text:?}: {{{asked}}} is not a pause of 0 to 10 s"))?;
        pauses.push(pause);
        rest = &after[close + 1..];
    }
}

/// Whether a name is a file's or a voice's: a-z, 0-9, `_` and `-`.
fn plain(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// The rows of a TSV, less blank and `#` lines, each split into `fields`.
fn rows(text: &str, fields: usize) -> Outcome<Vec<Vec<&str>>> {
    text.lines()
        .filter(|row| !row.trim().is_empty() && !row.starts_with('#'))
        .map(|row| {
            let row: Vec<&str> = row.splitn(fields, '\t').collect();
            if row.len() == fields {
                Ok(row)
            } else {
                Err(format!("{row:?}: not {fields} fields").into())
            }
        })
        .collect()
}

/// VOICES.tsv: each voice's ElevenLabs id by its name.
fn voices(text: &str) -> Outcome<BTreeMap<String, String>> {
    let mut voices = BTreeMap::new();
    for row in rows(text, 2)? {
        let (name, id) = (row[0].trim(), row[1].trim());
        if !plain(name) || id.is_empty() || id.len() > 64 {
            return Err(format!("{name:?}: a voice is a plain name and an id").into());
        }
        if !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(format!("{name}: a voice id is letters and digits").into());
        }
        if voices.insert(name.to_owned(), id.to_owned()).is_some() {
            return Err(format!("{name}: named twice").into());
        }
    }
    Ok(voices)
}

/// One line to render, as LINES.tsv gives it.
struct Line<'a> {
    id: &'a str,
    voice: &'a str,
    kind: &'a str,
    text: &'a str,
}

/// LINES.tsv's lines, each with a voice VOICES.tsv names and words that
/// split into pieces.
fn lines<'a>(text: &'a str, voices: &BTreeMap<String, String>) -> Outcome<Vec<Line<'a>>> {
    let mut seen = BTreeSet::new();
    let mut lines = Vec::new();
    for row in rows(text, 4)? {
        let line = Line {
            id: row[0],
            voice: row[1],
            kind: row[2],
            text: row[3].trim(),
        };
        if !plain(line.id) || !plain(line.kind) || !seen.insert(line.id) {
            return Err(format!("{:?}: a line has a plain id of its own and kind", line.id).into());
        }
        if !voices.contains_key(line.voice) {
            return Err(format!("{}: no voice {:?} in VOICES.tsv", line.id, line.voice).into());
        }
        if line.text.len() > 600 || line.text.chars().any(char::is_control) {
            return Err(format!("{}: the text is at most 600 bytes, no control", line.id).into());
        }
        pieces(line.text)?;
        lines.push(line);
    }
    Ok(lines)
}

/// Whether the voice `name` with `id` is the lamp's (`lamp`, by name or id
/// as [`voice_name`] gives it).
fn is_lamps(lamp: &str, name: &str, id: &str) -> bool {
    lamp.eq_ignore_ascii_case(name) || lamp == id
}

/// A WAV's SHA-256, in hex.
fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// One line said in voice `id`: each piece its own request, its silence
/// cut, joined by the pauses asked.
async fn rendered(config: &SecondConfig, id: &str, text: &str) -> Outcome<Vec<i16>> {
    let (words, pauses) = pieces(text)?;
    let mut config = config.clone();
    config.tts_voice = Some(id.to_owned());
    let tts = Tts::new(&config)?;
    let mut said = Vec::with_capacity(words.len());
    for piece in words {
        let mut samples = Vec::new();
        let whole = async {
            let mut speech = tts.speak(piece).await?;
            while let Some(packet) = speech.next().await? {
                samples.extend_from_slice(&packet);
            }
            Ok::<_, Box<dyn Error + Send + Sync>>(())
        };
        tokio::time::timeout(PIECE_WITHIN, whole)
            .await
            .map_err(|_| "no whole reply within 40 s")??;
        let piece = trimmed(&samples);
        if piece.is_empty() {
            return Err("a piece's reply was silent".into());
        }
        said.push(piece.to_vec());
    }
    Ok(joined(&said, &pauses))
}

fn main() -> Outcome<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Outcome<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [second, lines_file, folder, voices_file] = arguments.as_slice() else {
        return Err(
            "usage: render_people_lines SECOND.json LINES.tsv OUT_FOLDER VOICES.tsv".into(),
        );
    };
    let voices = voices(&fs::read_to_string(voices_file)?)?;
    let lines_text = fs::read_to_string(lines_file)?;
    let lines = lines(&lines_text, &voices)?;
    let config =
        load_from_path(Path::new(second))?.with_environment(|name: &str| std::env::var_os(name))?;
    if config.tts_provider != TtsProvider::ElevenLabs {
        return Err("the route's speech is not ElevenLabs; its voice ids would not be".into());
    }
    let lamp = voice_name(config.tts_provider, config.tts_voice.as_deref()).to_owned();
    let model = config
        .tts_model
        .clone()
        .unwrap_or_else(|| default_model(config.tts_provider).to_owned());
    println!(
        "the lamp's voice: {lamp} ({}, {model}); {} lines in {} voices",
        config.tts_provider.name(),
        lines.len(),
        voices.len()
    );
    if let Some((name, _)) = voices.iter().find(|(name, id)| is_lamps(&lamp, name, id)) {
        return Err(format!("{name} is the lamp's own voice; give its lines another").into());
    }
    let folder = Path::new(folder);
    fs::create_dir_all(folder)?;
    let (mut made, mut kept, mut failed) = (0, 0, 0);
    let mut index = String::from("# id\tvoice\tvoice_id\tkind\ttext\tseconds\tsha256\n");
    for line in &lines {
        let id = &voices[line.voice];
        let file = folder.join(format!("{}.wav", line.id));
        let (how, bytes) = if file.exists() {
            kept += 1;
            ("kept", fs::read(&file)?)
        } else {
            match rendered(&config, id, line.text).await {
                Ok(samples) => {
                    let bytes = wav(TTS_SAMPLE_RATE, &samples);
                    let part = folder.join(format!("{}.wav.part", line.id));
                    fs::write(&part, &bytes)?;
                    fs::rename(&part, &file)?;
                    made += 1;
                    ("rendered", bytes)
                }
                Err(error) => {
                    failed += 1;
                    println!("failed {} {}: {error}", line.id, line.voice);
                    continue;
                }
            }
        };
        let seconds = seconds_of(&bytes);
        let sha = sha256(&bytes);
        println!("{how} {} {} {seconds:.2} s", line.id, line.voice);
        index.push_str(&format!(
            "{}\t{}\t{id}\t{}\t{}\t{seconds:.3}\t{sha}\n",
            line.id, line.voice, line.kind, line.text
        ));
    }
    let part = folder.join("index.tsv.part");
    fs::write(&part, index)?;
    fs::rename(&part, folder.join("index.tsv"))?;
    println!("{made} rendered, {kept} kept, {failed} failed");
    if failed > 0 {
        return Err(format!("{failed} lines failed; run again for them").into());
    }
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
        assert_eq!(seconds_of(&wav(TTS_SAMPLE_RATE, &[0; 12_000])), 0.5);
    }

    #[test]
    fn a_reply_is_cut_to_its_first_and_last_sound() {
        let mut samples = vec![50i16; 5_000];
        samples.extend([1_000i16, 0, -1_000]);
        samples.extend(vec![-50i16; 5_000]);
        assert_eq!(trimmed(&samples), &[1_000, 0, -1_000]);
        assert!(trimmed(&[0, 5, -5]).is_empty());
    }

    #[test]
    fn the_markers_split_the_words_and_give_the_pauses() {
        let (words, pauses) =
            pieces("Lamp, can you tell me {1.1} um {0.6} how long it takes?").unwrap();
        assert_eq!(words, ["Lamp, can you tell me", "um", "how long it takes?"]);
        assert_eq!(pauses, [1.1, 0.6]);
        let (words, pauses) = pieces("Why is that?").unwrap();
        assert_eq!(words, ["Why is that?"]);
        assert!(pauses.is_empty());
        for wrong in [
            "{1.0} at the start",
            "at the end {1.0}",
            "two {1.0} {0.5} together",
            "a lone } here",
            "a {1.0 open",
            "a {x} word",
            "a {} none",
            "a {11} too long",
            "a {-1} negative",
        ] {
            assert!(pieces(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn the_pieces_are_joined_by_their_pauses_with_a_quarter_second_each_end() {
        let samples = joined(&[vec![1; 3], vec![2; 2]], &[0.5]);
        let edge = 6_000;
        assert_eq!(samples.len(), edge + 3 + 12_000 + 2 + edge);
        assert!(samples[..edge].iter().all(|&s| s == 0));
        assert_eq!(&samples[edge..edge + 3], &[1, 1, 1]);
        assert!(samples[edge + 3..edge + 3 + 12_000].iter().all(|&s| s == 0));
        assert_eq!(&samples[edge + 12_003..edge + 12_005], &[2, 2]);
        assert!(samples[edge + 12_005..].iter().all(|&s| s == 0));
    }

    #[test]
    fn the_files_are_read_with_their_checks() {
        let voices =
            voices("# name\tid\nrachel\t21m00Tcm4TlvDq8ikWAM\nadam\tpNInz6obpgDQGcFmaJgB\n")
                .unwrap();
        assert_eq!(voices["adam"], "pNInz6obpgDQGcFmaJgB");
        assert!(super::voices("adam\tpNIn/z6\n").is_err());
        assert!(super::voices("adam\tA1\nadam\tB2\n").is_err());
        let text = "# id\tvoice\tkind\ttext\na01\tadam\task\tLamp, {0.8} hello.\n\n";
        let read = lines(text, &voices).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(
            (read[0].id, read[0].voice, read[0].kind, read[0].text),
            ("a01", "adam", "ask", "Lamp, {0.8} hello.")
        );
        assert!(lines("a01\tnobody\task\tHi.\n", &voices).is_err());
        assert!(lines("a01\tadam\task\tHi.\na01\tadam\task\tHo.\n", &voices).is_err());
        assert!(lines("../x\tadam\task\tHi.\n", &voices).is_err());
        assert!(lines("a01\tadam\task\tHi {x}.\n", &voices).is_err());
        assert!(lines("a01\tadam\task\n", &voices).is_err());
    }

    #[test]
    fn the_lamps_own_voice_is_known_by_name_or_id() {
        let unset = voice_name(TtsProvider::ElevenLabs, None);
        assert!(is_lamps(unset, "rachel", "anything"));
        assert!(!is_lamps(unset, "adam", "pNInz6obpgDQGcFmaJgB"));
        let set = voice_name(TtsProvider::ElevenLabs, Some("pNInz6obpgDQGcFmaJgB"));
        assert!(is_lamps(set, "anyone", "pNInz6obpgDQGcFmaJgB"));
    }
}
