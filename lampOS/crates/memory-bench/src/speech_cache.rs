//! Immutable shared transcripts verified against saved PCM and provider frames.
use crate::{
    Result, audio, hash, invalid, live_speech,
    model::{Call, Provenance},
    model_worker::Worker,
    protocol::{Event, EventKind},
    speech::{self, Audio, Transcript},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    time::{Duration, Instant},
};
const MAX_INPUT: u64 = 32 * 1024 * 1024;
fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(invalid("cache input must be a bounded regular file"));
    }
    let mut bytes = vec![];
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid("cache input grew beyond bound"));
    }
    Ok(bytes)
}
fn directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() {
        return Err(invalid("cache directory must not be a symlink"));
    }
    Ok(())
}
fn save(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}
fn lines<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<Vec<T>> {
    let mut rows = vec![];
    for line in bytes.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        if line.len() > 4 * 1024 * 1024 || rows.len() >= 100_000 {
            return Err(invalid("cache rows exceed bound"));
        }
        rows.push(serde_json::from_slice(line)?);
    }
    Ok(rows)
}
fn encoded_lines<T: Serialize>(rows: &[T]) -> Result<Vec<u8>> {
    encoded_lines_with_limit(rows, MAX_INPUT as usize)
}
fn encoded_lines_with_limit<T: Serialize>(rows: &[T], limit: usize) -> Result<Vec<u8>> {
    if rows.len() > 100_000 {
        return Err(invalid("too many cache rows"));
    }
    let mut bytes = vec![];
    for row in rows {
        let line = serde_json::to_vec(row)?;
        if line.len() > 4 * 1024 * 1024 || bytes.len().saturating_add(line.len() + 1) > limit {
            return Err(invalid("encoded cache exceeds read bounds"));
        }
        bytes.extend(line);
        bytes.push(b'\n');
    }
    Ok(bytes)
}
fn bounded_json(value: &impl Serialize) -> Result<Vec<u8>> {
    struct Bounded(Vec<u8>);
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.0.len().saturating_add(bytes.len()) > MAX_INPUT as usize {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "cache manifest exceeds read bound",
                ));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut out = Bounded(vec![]);
    serde_json::to_writer(&mut out, value)?;
    Ok(out.0)
}
fn valid_hash(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn audio_file(row: &Audio) -> Result<&str> {
    if !valid_hash(&row.wav_hash)
        || !valid_hash(&row.pcm_hash)
        || !valid_hash(&row.script_hash)
        || row.file.is_empty()
        || row.file.len() > 128
        || !row
            .file
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        || !row.file.ends_with(".wav")
    {
        return Err(invalid("invalid audio cache filename or digest"));
    }
    Ok(&row.file)
}
fn validate_input(row: &Audio, wav: &[u8]) -> Result<audio::Input> {
    let input = audio::Input::from_wav(wav)?;
    input.validate()?;
    if input.wav_sha256() != row.wav_hash
        || input.pcm_sha256() != row.pcm_hash
        || input.samples() * 1000 / 16000 != row.duration_ms
    {
        return Err(invalid("prepared audio differs from actual WAV/PCM"));
    }
    Ok(input)
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    request_hash: String,
    provenance_hash: String,
    provenance: Provenance,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    fixture: bool,
    config: live_speech::Config,
    events_hash: String,
    audio_manifest_hash: String,
    transcript_hash: String,
    observed_hash: String,
    model_revision: String,
    entries: BTreeMap<String, Entry>,
}
/// The caller provides its sole budgeted worker. This routine never constructs
/// a provider, reads a key or grants spending approval. A failed attempt stays.
pub fn create(
    events_file: &Path,
    audio_dir: &Path,
    provider_artifacts: &Path,
    output: &Path,
    config: live_speech::Config,
    fixture: bool,
    worker: &mut Worker,
) -> Result<Value> {
    config.validate(fixture)?;
    directory(audio_dir)?;
    directory(provider_artifacts)?;
    let source = read(events_file, MAX_INPUT)?;
    let audio_bytes = read(&audio_dir.join("audio.jsonl"), MAX_INPUT)?;
    let events: Vec<Event> = lines(&source)?;
    let audio: Vec<Audio> = lines(&audio_bytes)?;
    validate_mapping(&events, &audio)?;
    fs::DirBuilder::new().mode(0o700).create(output)?;
    File::open(
        output
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?
    .sync_all()?;
    save(&output.join("source-events.jsonl"), &source)?;
    save(&output.join("audio.jsonl"), &audio_bytes)?;
    let mut entries = BTreeMap::new();
    let mut transcripts = vec![];
    let mut model_revision = None;
    let mut known: BTreeMap<String, Transcript> = BTreeMap::new();
    for row in &audio {
        let wav = read(&audio_dir.join(audio_file(row)?), 4 * 1024 * 1024)?;
        let input = validate_input(row, &wav)?;
        if let Some(earlier) = known.get(&row.wav_hash) {
            let mut copy = earlier.clone();
            copy.record_id = row.record_id.clone();
            transcripts.push(copy);
            continue;
        }
        let dir = output.join(&row.wav_hash);
        fs::DirBuilder::new().mode(0o700).create(&dir)?;
        save(&dir.join("audio.wav"), &wav)?;
        let call = Call::Transcribe {
            audio: input.clone(),
        };
        let reply = worker.call(
            &call,
            Instant::now()
                + Duration::from_micros(input.reserve_micros())
                + Duration::from_secs(45),
        )?;
        let account = worker.account()?;
        let receipt = account
            .receipts
            .last()
            .ok_or_else(|| invalid("transcription has no accounting receipt"))?;
        if account.stopped
            || !["accounted", "usage_unknown"].contains(&receipt.outcome.as_str())
            || receipt.request_hash != call.request_hash()?
        {
            return Err(invalid("speech receipt does not match successful request"));
        }
        let p = receipt
            .provenance
            .as_ref()
            .ok_or_else(|| invalid("speech receipt omitted raw provenance"))?;
        if p.artifact.len() != 6 || !p.artifact.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid("invalid speech artifact directory"));
        }
        let original = provider_artifacts.join(&p.artifact);
        directory(&original)?;
        let request = read(&original.join("request.json"), 4 * 1024 * 1024)?;
        let response = read(&original.join("response.jsonl"), 4 * 1024 * 1024)?;
        let provenance_bytes = read(&original.join("provenance.json"), 65536)?;
        let stored: Provenance = serde_json::from_slice(&provenance_bytes)?;
        if serde_json::to_value(&stored)? != serde_json::to_value(p)? {
            return Err(invalid("saved speech receipt differs from accounting"));
        }
        let text = live_speech::verify_saved(&config, fixture, &input, &request, &response, p)?;
        if reply.content.as_ref() != Some(&text) {
            return Err(invalid("speech reply differs from raw final frames"));
        }
        let evidence = p
            .audio
            .as_ref()
            .ok_or_else(|| invalid("speech evidence missing"))?;
        let revision = evidence
            .model_revision
            .as_ref()
            .ok_or_else(|| invalid("speech revision missing"))?;
        if model_revision.as_ref().is_some_and(|old| old != revision) {
            return Err(invalid("speech model revision changed across corpus"));
        }
        model_revision = Some(revision.clone());
        save(&dir.join("request.json"), &request)?;
        save(&dir.join("response.jsonl"), &response)?;
        save(&dir.join("provenance.json"), &provenance_bytes)?;
        File::open(&dir)?.sync_all()?;
        let transcript = Transcript {
            record_id: row.record_id.clone(),
            audio_hash: row.wav_hash.clone(),
            request_hash: p.request_sha256.clone(),
            response_hash: p.response_sha256.clone(),
            request_id: p
                .provider_request_id
                .clone()
                .ok_or_else(|| invalid("speech request ID missing"))?,
            model_revision: Some(revision.clone()),
            text,
            confidence: evidence.confidence,
            mode: if fixture { "recorded" } else { "live" }.into(),
        };
        known.insert(row.wav_hash.clone(), transcript.clone());
        transcripts.push(transcript);
        entries.insert(
            row.wav_hash.clone(),
            Entry {
                request_hash: call.request_hash()?,
                provenance_hash: hash(&provenance_bytes),
                provenance: p.clone(),
            },
        );
    }
    let observed = speech::observed(&events, &audio, &transcripts, !fixture)?;
    let transcript_bytes = encoded_lines(&transcripts)?;
    let observed_bytes = encoded_lines(&observed)?;
    save(&output.join("transcripts.jsonl"), &transcript_bytes)?;
    save(&output.join("observed-events.jsonl"), &observed_bytes)?;
    let manifest = Manifest {
        version: 1,
        fixture,
        config,
        events_hash: hash(&source),
        audio_manifest_hash: hash(&audio_bytes),
        transcript_hash: hash(&transcript_bytes),
        observed_hash: hash(&observed_bytes),
        model_revision: model_revision.ok_or_else(|| invalid("empty speech corpus"))?,
        entries,
    };
    let bytes = bounded_json(&manifest)?;
    // Publication is last: a crash leaves evidence, never a usable cache.
    save(&output.join("manifest.json"), &bytes)?;
    File::open(output)?.sync_all()?;
    Ok(
        json!({"manifest_sha256":hash(&bytes),"unique_audio":manifest.entries.len(),"utterances":audio.len(),"fixture":fixture,"scores":null,"status":"frozen speech cache"}),
    )
}
fn validate_mapping(events: &[Event], audio: &[Audio]) -> Result<()> {
    let mut ids = BTreeSet::new();
    for e in events {
        e.validate()?;
        if !ids.insert(e.record_id.as_str()) {
            return Err(invalid("duplicate source event"));
        }
    }
    let utterances: BTreeMap<_, _> = events
        .iter()
        .filter(|e| e.kind == EventKind::Utterance)
        .map(|e| (e.record_id.as_str(), e))
        .collect();
    let mut seen = BTreeSet::new();
    for a in audio {
        audio_file(a)?;
        if !seen.insert(a.record_id.as_str())
            || utterances
                .get(a.record_id.as_str())
                .is_none_or(|e| a.script_hash != hash(e.heard.as_bytes()))
        {
            return Err(invalid("audio manifest differs from source script"));
        }
    }
    if seen.len() != utterances.len() || audio.is_empty() {
        return Err(invalid("missing audio manifest rows"));
    }
    Ok(())
}
/// Recheck every input and provider byte before using a pinned shared cache.
pub fn load(
    output: &Path,
    source_events: &Path,
    expected_manifest_hash: &str,
    require_live: bool,
) -> Result<Vec<Event>> {
    directory(output)?;
    if fs::metadata(output)?.permissions().mode() & 0o077 != 0 {
        return Err(invalid("speech cache is not private"));
    }
    let bytes = read(&output.join("manifest.json"), MAX_INPUT)?;
    if !valid_hash(expected_manifest_hash) || hash(&bytes) != expected_manifest_hash {
        return Err(invalid("speech cache differs from pinned manifest"));
    }
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    if manifest.version != 1 || require_live && manifest.fixture {
        return Err(invalid("speech cache mode cannot qualify this run"));
    }
    let source = read(source_events, MAX_INPUT)?;
    let preserved = read(&output.join("source-events.jsonl"), MAX_INPUT)?;
    let audio_bytes = read(&output.join("audio.jsonl"), MAX_INPUT)?;
    let transcript_bytes = read(&output.join("transcripts.jsonl"), MAX_INPUT)?;
    let observed_bytes = read(&output.join("observed-events.jsonl"), MAX_INPUT)?;
    if source != preserved
        || hash(&source) != manifest.events_hash
        || hash(&audio_bytes) != manifest.audio_manifest_hash
        || hash(&transcript_bytes) != manifest.transcript_hash
        || hash(&observed_bytes) != manifest.observed_hash
    {
        return Err(invalid("speech cache content differs from manifest"));
    }
    let events: Vec<Event> = lines(&source)?;
    let audio: Vec<Audio> = lines(&audio_bytes)?;
    let transcripts: Vec<Transcript> = lines(&transcript_bytes)?;
    validate_mapping(&events, &audio)?;
    let mut verified: BTreeMap<&str, String> = BTreeMap::new();
    for row in &audio {
        let entry = manifest
            .entries
            .get(&row.wav_hash)
            .ok_or_else(|| invalid("cache lacks provider evidence"))?;
        let evidence = entry
            .provenance
            .audio
            .as_ref()
            .ok_or_else(|| invalid("cache lacks audio identity"))?;
        if evidence.pcm_sha256 != row.pcm_hash || evidence.samples_sent / 16 != row.duration_ms {
            return Err(invalid(
                "duplicate audio metadata differs from verified identity",
            ));
        }
        if verified.contains_key(row.wav_hash.as_str()) {
            continue;
        }
        let dir = output.join(&row.wav_hash);
        directory(&dir)?;
        let wav = read(&dir.join("audio.wav"), 4 * 1024 * 1024)?;
        let input = validate_input(row, &wav)?;
        if (Call::Transcribe {
            audio: input.clone(),
        })
        .request_hash()?
            != entry.request_hash
        {
            return Err(invalid("cached audio call hash differs"));
        }
        let request = read(&dir.join("request.json"), 4 * 1024 * 1024)?;
        let response = read(&dir.join("response.jsonl"), 4 * 1024 * 1024)?;
        let provenance_bytes = read(&dir.join("provenance.json"), 65536)?;
        if hash(&provenance_bytes) != entry.provenance_hash {
            return Err(invalid("cached provenance bytes differ from manifest"));
        }
        let p: Provenance = serde_json::from_slice(&provenance_bytes)?;
        if serde_json::to_value(&p)? != serde_json::to_value(&entry.provenance)? {
            return Err(invalid("cached receipt differs from manifest"));
        }
        let text = live_speech::verify_saved(
            &manifest.config,
            manifest.fixture,
            &input,
            &request,
            &response,
            &p,
        )?;
        if p.audio.as_ref().and_then(|a| a.model_revision.as_deref())
            != Some(&manifest.model_revision)
        {
            return Err(invalid("cached corpus has mixed speech revisions"));
        }
        verified.insert(row.wav_hash.as_str(), text);
    }
    if verified.len() != manifest.entries.len() {
        return Err(invalid("unexpected cached provider entries"));
    }
    for row in &transcripts {
        let entry = manifest
            .entries
            .get(&row.audio_hash)
            .ok_or_else(|| invalid("transcript lacks provider entry"))?;
        let p = &entry.provenance;
        if verified.get(row.audio_hash.as_str()) != Some(&row.text)
            || row.request_hash != p.request_sha256
            || row.response_hash != p.response_sha256
            || Some(&row.request_id) != p.provider_request_id.as_ref()
            || row.model_revision.as_ref() != Some(&manifest.model_revision)
            || row.confidence != p.audio.as_ref().and_then(|a| a.confidence)
            || row.mode != if manifest.fixture { "recorded" } else { "live" }
        {
            return Err(invalid(
                "transcript differs from verified provider evidence",
            ));
        }
    }
    let observed = speech::observed(&events, &audio, &transcripts, require_live)?;
    if encoded_lines(&observed)? != observed_bytes {
        return Err(invalid(
            "observed event cache differs from verified transcripts",
        ));
    }
    Ok(observed)
}

#[cfg(test)]
mod tests {
    use super::encoded_lines_with_limit;
    #[test]
    fn aggregate_cache_rows_stop_before_the_read_limit() {
        assert_eq!(
            encoded_lines_with_limit(&["a", "b"], 8).unwrap(),
            b"\"a\"\n\"b\"\n"
        );
        assert!(encoded_lines_with_limit(&["a", "b"], 7).is_err());
        assert!(encoded_lines_with_limit(&vec![""; 100_001], 32 * 1024 * 1024).is_err());
    }
}
