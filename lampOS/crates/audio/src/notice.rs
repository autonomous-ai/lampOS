//! Spoken failure notices, loaded from a provisioned directory and never made.
//!
//! When the provider cannot be reached there is no voice to ask for one, so
//! what Lamp can say about it must already be on the device: a short
//! recording per notice, in the approved voice, named by a manifest that
//! carries its transcript, format, length and hash. This module reads that
//! directory once, validates every asset it names, and answers one question
//! afterwards: is this notice available, and if not, why not.
//!
//! It never synthesizes, downloads, resamples, pads or substitutes. A notice
//! that is missing or fails any check is reported unavailable, and the
//! caller stays silent. A recording marked as a test asset is not a
//! production voice and is refused unless the caller asks for test assets.
//!
//! Nothing here plays audio or grants the right to. The samples go through
//! the same guarded speaker path, under a current output owner, as any reply.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Cursor, Read},
    path::Path,
};

/// Directory entry the loader reads first.
pub const MANIFEST: &str = "manifest.json";
pub const SCHEMA_VERSION: u8 = 1;
pub const MAX_MANIFEST_BYTES: u64 = 16 * 1024;
pub const MAX_NOTICES: usize = 8;
pub const RATE: u32 = 24_000;
/// A notice is a sentence, not a reply: 0.3 s to 10 s.
pub const MIN_FRAMES: u32 = RATE * 3 / 10;
pub const MAX_FRAMES: u32 = RATE * 10;
const MAX_WAV_BYTES: u64 = 44 + 2 * MAX_FRAMES as u64 + 4096;
const MAX_TRANSCRIPT_BYTES: usize = 240;
const MAX_NAME_BYTES: usize = 64;

/// What a notice is about. One recording per kind and language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    /// The voice service cannot be reached; nothing can be answered for now.
    ProviderUnavailable,
    /// An accepted question was lost; it has to be asked again.
    TurnLost,
    /// The service refused: a usage limit or a rejected credential.
    ServiceRefused,
    /// The internet is back, said only if the lamp said it could not reach
    /// it (#830).
    BackOnline,
    /// The internet answers but the voice service does not (a relay
    /// outage), and nothing else can answer (#830, `LAMP_SERVICE_DOWN_LINE`).
    VoiceServiceUnreachable,
    /// Neither the new Wi-Fi nor the old one could be joined after a change
    /// from the app (#1959): no network, so no cloud voice to say so.
    WifiLost,
}

/// Who may hear it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Approval {
    /// Recorded in the approved voice and accepted for the product.
    Production,
    /// A placeholder for tests. Never spoken by a production runtime.
    Test,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u8,
    notices: Vec<Entry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    kind: NoticeKind,
    language: String,
    transcript: String,
    voice: String,
    voice_revision: String,
    approval: Approval,
    file: String,
    wav_sha256: String,
    frames: u32,
}

/// Everything known about one loaded notice except its samples.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NoticeInfo {
    pub kind: NoticeKind,
    /// BCP 47 tag exactly as provisioned, for example `en-US`.
    pub language: String,
    /// The words in the recording. Never sent anywhere to be spoken.
    pub transcript: String,
    pub voice: String,
    pub voice_revision: String,
    pub approval: Approval,
    pub wav_sha256: String,
    pub pcm_sha256: String,
    /// Mono frames at [`RATE`], 16-bit.
    pub frames: u32,
    pub duration_ms: u32,
}

pub struct Notice {
    pub info: NoticeInfo,
    samples: Vec<i16>,
}
impl Notice {
    /// Mono 24 kHz PCM16, exactly as provisioned.
    pub fn samples(&self) -> &[i16] {
        &self.samples
    }
}

/// Why a notice cannot be spoken. Fixed categories; no path or file content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Missing {
    /// No directory was configured, or it does not exist.
    NoDirectory,
    NoManifest,
    /// The manifest is not a bounded regular file of the expected shape.
    InvalidManifest,
    /// The manifest names no recording of this kind in this language.
    NotProvisioned,
    /// It is provisioned only as a test asset.
    NotApproved,
    /// The entry itself is malformed: name, transcript, hash or length.
    InvalidEntry,
    AssetMissing,
    /// Not a bounded regular file, or changed while it was read.
    AssetUnreadable,
    AssetHashMismatch,
    /// Not complete mono 24 kHz PCM16 of the stated length, or all silence.
    AssetFormat,
}

/// One entry that was named and could not be loaded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Refused {
    pub kind: NoticeKind,
    pub language: String,
    pub reason: Missing,
}

/// The notices this process can speak, decided once at load.
pub struct Notices {
    loaded: Vec<Notice>,
    refused: Vec<Refused>,
    /// A failure of the directory or manifest as a whole.
    unavailable: Option<Missing>,
}

/// What the runtime can and cannot say, for a status report or a log line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Capability {
    pub available: Vec<NoticeInfo>,
    pub refused: Vec<Refused>,
    pub unavailable: Option<Missing>,
}

impl Notices {
    /// No directory configured: every notice is unavailable.
    pub fn none() -> Self {
        Self {
            loaded: Vec::new(),
            refused: Vec::new(),
            unavailable: Some(Missing::NoDirectory),
        }
    }

    /// Read and validate `directory`. This never fails: whatever cannot be
    /// used is recorded as unavailable, and one bad entry does not take the
    /// others with it. `accept` is the least approval the caller may speak.
    pub fn load(directory: &Path, accept: Approval) -> Self {
        let manifest = match read_manifest(directory) {
            Ok(manifest) => manifest,
            Err(reason) => {
                return Self {
                    loaded: Vec::new(),
                    refused: Vec::new(),
                    unavailable: Some(reason),
                };
            }
        };
        let (mut loaded, mut refused): (Vec<Notice>, Vec<Refused>) = (Vec::new(), Vec::new());
        for entry in manifest.notices {
            let (kind, language) = (entry.kind, entry.language.clone());
            match load_entry(directory, entry, accept) {
                Ok(notice) => loaded.push(notice),
                Err(reason) => refused.push(Refused {
                    kind,
                    language,
                    reason,
                }),
            }
        }
        Self {
            loaded,
            refused,
            unavailable: None,
        }
    }

    /// The notice of this kind in exactly this language, or why there is none.
    pub fn get(&self, kind: NoticeKind, language: &str) -> Result<&Notice, Missing> {
        if let Some(reason) = self.unavailable {
            return Err(reason);
        }
        let wanted = |info_kind: NoticeKind, info_language: &str| {
            info_kind == kind && info_language == language
        };
        if let Some(notice) = self
            .loaded
            .iter()
            .find(|notice| wanted(notice.info.kind, &notice.info.language))
        {
            return Ok(notice);
        }
        Err(self
            .refused
            .iter()
            .find(|refused| wanted(refused.kind, &refused.language))
            .map_or(Missing::NotProvisioned, |refused| refused.reason))
    }

    pub fn capability(&self) -> Capability {
        Capability {
            available: self
                .loaded
                .iter()
                .map(|notice| notice.info.clone())
                .collect(),
            refused: self.refused.clone(),
            unavailable: self.unavailable,
        }
    }
}

/// A regular file opened without following a link, read whole, unchanged in
/// size while it was read.
fn read_regular(
    path: &Path,
    limit: u64,
    absent: Missing,
    unreadable: Missing,
) -> Result<Vec<u8>, Missing> {
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .map_err(|error| {
        if error == rustix::io::Errno::NOENT {
            absent
        } else {
            unreadable
        }
    })?;
    let mut file = File::from(fd);
    let read = |file: &mut File| -> io::Result<Option<Vec<u8>>> {
        let before = file.metadata()?;
        if !before.is_file() || before.len() > limit {
            return Ok(None);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(before.len() as usize)
            .map_err(io::Error::other)?;
        file.take(limit + 1).read_to_end(&mut bytes)?;
        let unchanged =
            bytes.len() as u64 == before.len() && file.metadata()?.len() == before.len();
        Ok(unchanged.then_some(bytes))
    };
    read(&mut file).ok().flatten().ok_or(unreadable)
}

fn read_manifest(directory: &Path) -> Result<Manifest, Missing> {
    if !directory.is_dir() {
        return Err(Missing::NoDirectory);
    }
    let bytes = read_regular(
        &directory.join(MANIFEST),
        MAX_MANIFEST_BYTES,
        Missing::NoManifest,
        Missing::InvalidManifest,
    )?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| Missing::InvalidManifest)?;
    let mut seen = Vec::new();
    let distinct = manifest.notices.iter().all(|entry| {
        let key = (entry.kind, entry.language.as_str());
        let new = !seen.contains(&key);
        seen.push(key);
        new
    });
    if manifest.schema_version != SCHEMA_VERSION
        || manifest.notices.len() > MAX_NOTICES
        || !distinct
    {
        return Err(Missing::InvalidManifest);
    }
    Ok(manifest)
}

fn plain(text: &str, max: usize) -> bool {
    !text.is_empty() && text.len() <= max && !text.chars().any(char::is_control)
}

fn load_entry(directory: &Path, entry: Entry, accept: Approval) -> Result<Notice, Missing> {
    // A bare file name in this directory: never a path, never hidden.
    let name_ok = plain(&entry.file, MAX_NAME_BYTES)
        && entry.file.ends_with(".wav")
        && !entry.file.starts_with('.')
        && entry
            .file
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'));
    let language_ok = plain(&entry.language, 35)
        && entry
            .language
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    let hash_ok = entry.wav_sha256.len() == 64
        && entry
            .wav_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !name_ok
        || !language_ok
        || !hash_ok
        || !plain(&entry.transcript, MAX_TRANSCRIPT_BYTES)
        || !plain(&entry.voice, MAX_NAME_BYTES)
        || !plain(&entry.voice_revision, MAX_NAME_BYTES)
        || !(MIN_FRAMES..=MAX_FRAMES).contains(&entry.frames)
    {
        return Err(Missing::InvalidEntry);
    }
    if entry.approval == Approval::Test && accept == Approval::Production {
        return Err(Missing::NotApproved);
    }
    let bytes = read_regular(
        &directory.join(&entry.file),
        MAX_WAV_BYTES,
        Missing::AssetMissing,
        Missing::AssetUnreadable,
    )?;
    if format!("{:x}", Sha256::digest(&bytes)) != entry.wav_sha256 {
        return Err(Missing::AssetHashMismatch);
    }
    let samples = decode(&bytes, entry.frames).ok_or(Missing::AssetFormat)?;
    let mut pcm = Sha256::new();
    for sample in &samples {
        pcm.update(sample.to_le_bytes());
    }
    Ok(Notice {
        info: NoticeInfo {
            kind: entry.kind,
            language: entry.language,
            transcript: entry.transcript,
            voice: entry.voice,
            voice_revision: entry.voice_revision,
            approval: entry.approval,
            wav_sha256: entry.wav_sha256,
            pcm_sha256: format!("{:x}", pcm.finalize()),
            frames: entry.frames,
            duration_ms: entry.frames * 1000 / RATE,
        },
        samples,
    })
}

/// A complete RIFF/WAVE file of exactly `frames` mono 24 kHz PCM16 frames
/// that is not all silence. No conversion of any kind.
fn decode(bytes: &[u8], frames: u32) -> Option<Vec<i16>> {
    if bytes.len() < 44
        || &bytes[..4] != b"RIFF"
        || &bytes[8..12] != b"WAVE"
        || u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize + 8 != bytes.len()
    {
        return None;
    }
    let mut reader = hound::WavReader::new(Cursor::new(bytes)).ok()?;
    let spec = reader.spec();
    if spec.sample_rate != RATE
        || spec.channels != 1
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
        || reader.len() != frames
    {
        return None;
    }
    let samples: Vec<i16> = reader.samples().collect::<Result<_, _>>().ok()?;
    (samples.len() == frames as usize && samples.iter().any(|&sample| sample != 0))
        .then_some(samples)
}
