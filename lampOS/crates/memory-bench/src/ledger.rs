//! Private, exclusive runs. Interrupted and failed attempts are never overwritten.
use crate::{
    Result, hash, invalid,
    protocol::{MAX_FRAME_BYTES, valid_id},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Recorded,
    Live,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub run_id: String,
    pub mode: Mode,
    pub arm: String,
    pub implementation_revision: String,
    pub registration_revision: String,
    pub registration_hash: String,
    pub fixture_hash: String,
    pub transcript_hash: Option<String>,
    pub timezone: String,
    pub model: String,
    pub config: Value,
    pub budget_microusd: u64,
    pub seed: u64,
}
impl Manifest {
    pub fn validate(&self) -> Result<()> {
        if !valid_id(&self.run_id)
            || !valid_id(&self.arm)
            || self.budget_microusd == 0
            || self.model.trim().is_empty()
            || !self.config.is_object()
        {
            return Err(invalid("invalid run manifest"));
        }
        for revision in [&self.implementation_revision, &self.registration_revision] {
            if revision.len() != 40 || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(invalid("full commit revisions required"));
            }
        }
        for digest in [&self.registration_hash, &self.fixture_hash]
            .into_iter()
            .chain(self.transcript_hash.iter())
        {
            if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(invalid("invalid manifest content hash"));
            }
        }
        jiff::tz::TimeZone::get(&self.timezone).map_err(|_| invalid("unknown fixture timezone"))?;
        Ok(())
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub sequence: u64,
    pub previous_hash: String,
    pub kind: String,
    pub data: Value,
    pub hash: String,
}
fn digest(sequence: u64, previous: &str, kind: &str, data: &Value) -> Result<String> {
    Ok(hash(&serde_json::to_vec(&(
        sequence, previous, kind, data,
    ))?))
}
pub struct Ledger {
    file: File,
    next: u64,
    previous: String,
    finished: bool,
}
impl Ledger {
    pub fn create(path: &Path, manifest: &Manifest) -> Result<Self> {
        manifest.validate()?;
        DirBuilder::new().mode(0o700).create(path)?;
        let bytes = serde_json::to_vec(manifest)?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path.join("manifest.json"))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        let file = OpenOptions::new()
            .create_new(true)
            .append(true)
            .mode(0o600)
            .open(path.join("events.jsonl"))?;
        File::open(path)?.sync_all()?;
        let mut ledger = Self {
            file,
            next: 0,
            previous: hash(&bytes),
            finished: false,
        };
        ledger.append(
            "started",
            serde_json::json!({"manifest_hash":ledger.previous}),
        )?;
        Ok(ledger)
    }
    pub fn append(&mut self, kind: &str, data: Value) -> Result<()> {
        if self.finished || !valid_id(kind) || !data.is_object() {
            return Err(invalid("invalid ledger append or already finished"));
        }
        let digest = digest(self.next, &self.previous, kind, &data)?;
        let entry = Entry {
            sequence: self.next,
            previous_hash: self.previous.clone(),
            kind: kind.into(),
            data,
            hash: digest.clone(),
        };
        let mut bytes = serde_json::to_vec(&entry)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(invalid("ledger entry exceeds bound"));
        }
        self.file.write_all(&bytes)?;
        self.file.sync_data()?;
        self.next += 1;
        self.previous = digest;
        self.finished = matches!(kind, "completed" | "failed");
        Ok(())
    }
}
#[derive(Debug, Serialize)]
pub struct Verification {
    pub entries: u64,
    pub terminal: Option<String>,
    pub final_hash: String,
}
pub fn verify(path: &Path) -> Result<Verification> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(invalid("run directory must be private and not a symlink"));
    }
    for filename in ["manifest.json", "events.jsonl"] {
        let metadata = fs::symlink_metadata(path.join(filename))?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
            return Err(invalid("run files must be private regular files"));
        }
    }
    let bytes = fs::read(path.join("manifest.json"))?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    manifest.validate()?;
    let mut previous = hash(&bytes);
    let mut sequence = 0;
    let mut terminal = None;
    let mut reader = BufReader::new(File::open(path.join("events.jsonl"))?);
    loop {
        let mut line = Vec::new();
        let count = std::io::Read::take(&mut reader, (MAX_FRAME_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        if count > MAX_FRAME_BYTES || line.last() != Some(&b'\n') {
            return Err(invalid("truncated or oversized ledger row"));
        }
        let entry: Entry = serde_json::from_slice(&line)?;
        if terminal.is_some()
            || entry.sequence != sequence
            || entry.previous_hash != previous
            || entry.hash
                != digest(
                    entry.sequence,
                    &entry.previous_hash,
                    &entry.kind,
                    &entry.data,
                )?
        {
            return Err(invalid("broken ledger chain or rows after terminal state"));
        }
        if sequence == 0
            && (entry.kind != "started"
                || entry.data.get("manifest_hash").and_then(Value::as_str)
                    != Some(previous.as_str()))
        {
            return Err(invalid("ledger does not begin with its manifest"));
        }
        if matches!(entry.kind.as_str(), "completed" | "failed") {
            terminal = Some(entry.kind);
        }
        previous = entry.hash;
        sequence += 1;
    }
    if sequence == 0 {
        return Err(invalid("empty ledger"));
    }
    Ok(Verification {
        entries: sequence,
        terminal,
        final_hash: previous,
    })
}
