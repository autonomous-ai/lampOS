//! File-only TTS preparation and frozen real/recorded transcription manifests.
//! Service invocation is intentionally left at the central model-client boundary.
use crate::{
    Result, files, hash, invalid,
    protocol::{Event, EventKind, write_frame},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const MOST_AUDIO: usize = 32_000 * 120;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Audio {
    pub record_id: String,
    pub script_hash: String,
    pub wav_hash: String,
    pub pcm_hash: String,
    pub file: String,
    pub duration_ms: u64,
    pub voice: String,
    pub words_per_minute: u32,
}
/// Accept only one bounded 16 kHz mono signed-16 PCM RIFF data chunk.
pub fn pcm(wav: &[u8]) -> Result<&[u8]> {
    if wav.len() > MOST_AUDIO + 65536
        || wav.len() < 12
        || &wav[..4] != b"RIFF"
        || &wav[8..12] != b"WAVE"
    {
        return Err(invalid("expected bounded RIFF WAVE"));
    }
    let number = |p: usize| u32::from_le_bytes(wav[p..p + 4].try_into().unwrap()) as usize;
    if number(4) + 8 != wav.len() {
        return Err(invalid("RIFF length mismatch"));
    }
    let mut at = 12;
    let mut format = false;
    let mut data = None;
    while at + 8 <= wav.len() {
        let length = number(at + 4);
        let end = at
            .checked_add(8)
            .and_then(|v| v.checked_add(length))
            .filter(|v| *v <= wav.len())
            .ok_or_else(|| invalid("truncated WAVE chunk"))?;
        match &wav[at..at + 4] {
            b"fmt " => {
                let f = &wav[at + 8..end];
                if format
                    || f.len() < 16
                    || f[..4] != [1, 0, 1, 0]
                    || f[4..8] != 16000u32.to_le_bytes()
                    || f[8..12] != 32000u32.to_le_bytes()
                    || f[12..16] != [2, 0, 16, 0]
                {
                    return Err(invalid("WAVE must be mono PCM16 at 16000 Hz"));
                }
                format = true;
            }
            b"data" => {
                if data.is_some() || length == 0 || length % 2 != 0 || length > MOST_AUDIO {
                    return Err(invalid("invalid bounded PCM data"));
                }
                data = Some(&wav[at + 8..end]);
            }
            _ => {}
        }
        at = end + (length % 2);
    }
    if !format || at != wav.len() {
        return Err(invalid("missing format or malformed WAVE tail"));
    }
    data.ok_or_else(|| invalid("WAVE has no data"))
}
fn create(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
pub fn prepare(events_file: &Path, output: &Path, voice: &str, rate: u32) -> Result<Value> {
    if !(80..=300).contains(&rate)
        || voice.is_empty()
        || voice.len() > 100
        || !voice.chars().all(|c| c.is_alphanumeric() || c == ' ')
    {
        return Err(invalid("invalid TTS voice or rate"));
    }
    let events: Vec<Event> = files::lines(events_file)?;
    for event in &events {
        event.validate()?;
    }
    fs::DirBuilder::new().mode(0o700).create(output)?;
    let output = fs::canonicalize(output)?;
    let mut manifest = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(output.join("audio.jsonl"))?;
    let mut made: BTreeMap<String, (String, String, u64)> = BTreeMap::new();
    let mut count = 0;
    for event in events.iter().filter(|e| e.kind == EventKind::Utterance) {
        let script_hash = hash(event.heard.as_bytes());
        let identity = hash(&serde_json::to_vec(&(&script_hash, voice, rate))?);
        let (wav_hash, pcm_hash, duration_ms) = if let Some(row) = made.get(&identity) {
            row.clone()
        } else {
            let input = output.join(format!("{identity}.txt"));
            let target = output.join(format!("{identity}.wav"));
            create(&input, event.heard.as_bytes())?;
            // Output is mandatory and selected by us. There is no speaker/device path.
            let mut child = Command::new("/usr/bin/say")
                .args([
                    "-v",
                    voice,
                    "-r",
                    &rate.to_string(),
                    "--file-format=WAVE",
                    "--data-format=LEI16@16000",
                    "--channels=1",
                    "-f",
                ])
                .arg(&input)
                .arg("-o")
                .arg(&target)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                if let Some(status) = child.try_wait()? {
                    if !status.success() {
                        return Err(invalid("file-only TTS failed"));
                    }
                    break;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(invalid("file-only TTS timed out"));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let wav = fs::read(&target)?;
            let samples = pcm(&wav)?;
            let row = (
                hash(&wav),
                hash(samples),
                (samples.len() as u64 * 1000) / 32000,
            );
            made.insert(identity.clone(), row.clone());
            row
        };
        let row = Audio {
            record_id: event.record_id.clone(),
            script_hash,
            wav_hash,
            pcm_hash,
            file: format!("{identity}.wav"),
            duration_ms,
            voice: voice.into(),
            words_per_minute: rate,
        };
        write_frame(&mut manifest, &row)?;
        manifest.sync_data()?;
        count += 1;
    }
    let request = json!({"status":"prepared; no STT service call","transport":"websocket","path":"/v1/listen or relay /ws/audio/transcriptions","model":"nova-3-general","encoding":"linear16","sample_rate":16000,"channels":1,"language":"en","smart_format":true,"interim_results":true,"endpointing":300,"vad_events":true,"keyterm":["Lamp"],"audio_frames_ms":20,"controls":[{"type":"Finalize"},{"type":"CloseStream"}],"event_file_hash":hash(&fs::read(events_file)?),"audio_manifest_hash":hash(&fs::read(output.join("audio.jsonl"))?),"tts":"macOS say, file only","tts_executable_sha256":hash(&fs::read("/usr/bin/say")?),"voice_revision":null,"host_os":std::env::consts::OS,"unique_audio":made.len(),"records":count,"auth":"environment variable only; never written here"});
    create(
        &output.join("request-plan.json"),
        &serde_json::to_vec_pretty(&request)?,
    )?;
    Ok(request)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transcript {
    pub record_id: String,
    pub audio_hash: String,
    pub request_hash: String,
    pub response_hash: String,
    pub request_id: String,
    pub model_revision: Option<String>,
    pub text: String,
    pub confidence: Option<f64>,
    pub mode: String,
}
/// Freeze observed text only from explicit cache entries. No clean-text fallback.
pub fn observed(
    events: &[Event],
    audio: &[Audio],
    transcripts: &[Transcript],
    live: bool,
) -> Result<Vec<Event>> {
    let mut rows = BTreeMap::new();
    for row in transcripts {
        if rows.insert(row.record_id.as_str(), row).is_some() {
            return Err(invalid("duplicate transcript"));
        }
    }
    let mut sounds = BTreeMap::new();
    for row in audio {
        if sounds.insert(row.record_id.as_str(), row).is_some() {
            return Err(invalid("duplicate audio"));
        }
    }
    let mut output = events.to_vec();
    let mut count = 0;
    for event in &mut output {
        if event.kind != EventKind::Utterance {
            continue;
        }
        count += 1;
        let sound = sounds
            .get(event.record_id.as_str())
            .ok_or_else(|| invalid("missing audio manifest row"))?;
        let row = rows
            .get(event.record_id.as_str())
            .ok_or_else(|| invalid("missing transcript; fallback forbidden"))?;
        if sound.script_hash != hash(event.heard.as_bytes())
            || row.audio_hash != sound.wav_hash
            || live && row.mode != "live"
            || !["live", "recorded"].contains(&row.mode.as_str())
            || row.request_id.is_empty()
            || row
                .confidence
                .is_some_and(|c| !c.is_finite() || !(0.0..=1.0).contains(&c))
        {
            return Err(invalid(
                "transcript provenance, mode or confidence mismatch",
            ));
        }
        for h in [&row.audio_hash, &row.request_hash, &row.response_hash] {
            if h.len() != 64 || !h.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err(invalid("invalid transcript content hash"));
            }
        }
        event.heard = row.text.clone();
        event.stt_confidence = row.confidence;
    }
    if count != rows.len() || count != sounds.len() {
        return Err(invalid("unexpected cache rows"));
    }
    Ok(output)
}
