//! Requested audio authorization, without a playback path in Skills.
//!
//! The ordinary loader validates packages even with the switch off. A
//! runtime authority keeps at most 16 loaded audio definitions and offers.
//! Its caller scans the definitions every 2 s, outside the authority lock;
//! authorization fails closed 3 s after the last successful scan. Each scan
//! reads only two definition files (64 KiB each) and the checked WAV (at
//! most 2,884,140 bytes), for at most 16 definitions. It checks actual bytes,
//! not just size/mtime. Thus a changed file is
//! revoked on the next completed scan, or grants stop when that scan stalls.
//! A lease adds at most 500 ms to that detection bound. Voice independently
//! checks the offer's digest against the immutable bytes it will dispatch.
use crate::{Call, Skill, Step, package};
use lamp_msgs::{
    REQUESTED_AUDIO_LEASE_US, REQUESTED_AUDIO_PLAY_US, REQUESTED_AUDIO_START_US,
    RequestedAudioContext, RequestedAudioLease, RequestedAudioOffer, RequestedAudioOperation,
    RequestedAudioPhase,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

pub const ENVIRONMENT: &str = "LAMP_REQUESTED_AUDIO";
pub const MOST_DEFINITIONS: usize = 16;
pub const MOST_OFFERS: usize = 16;
pub const SCAN_FRESH_US: u64 = 3_000_000;
pub const MOST_CONFIRMATION_BYTES: usize = 240;
pub const MOST_DEFINITION_BYTES: u64 = 64 * 1024;

pub fn enabled(value: Option<&str>) -> bool {
    value == Some("on")
}

pub fn from_environment() -> bool {
    enabled(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Fixed checked asset, never selected by model arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub path: PathBuf,
    pub sha256: String,
    pub frames: u32,
    root: PathBuf,
    relative: String,
    definition_sha256: String,
    root_identity: (u64, u64),
    file_identity: (u64, u64),
}

fn definition_hash(description: &[u8], manifest: &[u8]) -> String {
    let mut hash = Sha256::new();
    for bytes in [description, manifest] {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    format!("{:x}", hash.finalize())
}

/// The package reader's no-link/nonblocking/plain-file protections, applied
/// to one already validated relative path. Only its at most four components
/// are inspected; a runtime scan does not enumerate unrelated directories.
fn plain_bytes(root: &Path, relative: &str, most: u64) -> Result<(Vec<u8>, (u64, u64)), String> {
    let path = root.join(relative);
    let mut parent = root.to_path_buf();
    let parts: Vec<_> = relative.split('/').collect();
    for part in &parts[..parts.len() - 1] {
        parent.push(part);
        let meta = fs::symlink_metadata(&parent).map_err(|_| "audio folder missing")?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err("audio folder is not a plain folder".into());
        }
    }
    let listed = fs::symlink_metadata(&path).map_err(|_| "audio file missing")?;
    if !listed.is_file() || listed.file_type().is_symlink() {
        return Err("audio file is not plain".into());
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)
        .map_err(|_| "audio file not readable")?;
    let meta = file.metadata().map_err(|_| "audio file not readable")?;
    if !meta.is_file()
        || meta.len() > most
        || (meta.dev(), meta.ino()) != (listed.dev(), listed.ino())
    {
        return Err("audio file changed or exceeds its bound".into());
    }
    let mut bytes = Vec::new();
    file.take(most + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "audio file not readable")?;
    if bytes.len() as u64 > most {
        return Err("audio file exceeds its bound".into());
    }
    Ok((bytes, (meta.dev(), meta.ino())))
}

impl Asset {
    pub fn load(root: &Path, relative: &str) -> Result<Self, String> {
        let parts: Vec<_> = relative.split('/').collect();
        if !relative.ends_with(".wav")
            || !(1..=4).contains(&parts.len())
            || parts.iter().any(|part| {
                !(1..=100).contains(&part.len())
                    || part.starts_with('.')
                    || !part.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                    })
            })
        {
            return Err(
                "requested_audio is a relative .wav path of at most four plain parts".into(),
            );
        }
        let root_meta = fs::symlink_metadata(root).map_err(|_| "audio skill folder missing")?;
        if !root_meta.is_dir() || root_meta.file_type().is_symlink() {
            return Err("audio skill is a plain folder, not a link".into());
        }
        // Reuse package's bounded, no-link/no-special-file reader. Hash the
        // definition bytes too: an in-place permission edit revokes a grant.
        let files = package::read_folder(root)?;
        if ["SKILL.md", "skill.toml"].iter().any(|name| {
            files
                .get(*name)
                .is_none_or(|bytes| bytes.len() as u64 > MOST_DEFINITION_BYTES)
        }) {
            return Err("requested audio definitions are at most 64 KiB each".into());
        }
        let bytes = files.get(relative).ok_or("requested audio asset missing")?;
        let root = fs::canonicalize(root).map_err(|_| "audio skill folder missing")?;
        let path = root.join(relative);
        let sound = lamp_voice::requested_audio::read_sound(&path)?;
        if sound.sha256 != package::sha256_hex(bytes) {
            return Err("requested audio changed while loading".into());
        }
        let definition_sha256 = definition_hash(
            files
                .get("SKILL.md")
                .ok_or("audio skill definition missing")?,
            files
                .get("skill.toml")
                .ok_or("audio skill definition missing")?,
        );
        let file_meta = fs::symlink_metadata(&path).map_err(|_| "audio asset missing")?;
        if !file_meta.is_file() || file_meta.file_type().is_symlink() {
            return Err("audio asset is not a plain file".into());
        }
        Ok(Self {
            path,
            sha256: sound.sha256,
            frames: u32::try_from(sound.samples.len()).map_err(|_| "too many audio frames")?,
            root,
            relative: relative.to_owned(),
            definition_sha256,
            root_identity: (root_meta.dev(), root_meta.ino()),
            file_identity: (file_meta.dev(), file_meta.ino()),
        })
    }

    pub(crate) fn matches_definition(&self, description: &str, manifest: &str) -> bool {
        self.definition_sha256 == definition_hash(description.as_bytes(), manifest.as_bytes())
    }

    /// Potential I/O: always called outside the authority mutex.
    pub fn unchanged(&self) -> bool {
        let check = || -> Result<bool, String> {
            let root = fs::symlink_metadata(&self.root).map_err(|_| "audio folder missing")?;
            if !root.is_dir()
                || root.file_type().is_symlink()
                || (root.dev(), root.ino()) != self.root_identity
            {
                return Ok(false);
            }
            let (description, _) = plain_bytes(&self.root, "SKILL.md", MOST_DEFINITION_BYTES)?;
            let (manifest, _) = plain_bytes(&self.root, "skill.toml", MOST_DEFINITION_BYTES)?;
            let (bytes, identity) = plain_bytes(
                &self.root,
                &self.relative,
                lamp_voice::requested_audio::MOST_BYTES,
            )?;
            Ok(identity == self.file_identity
                && definition_hash(&description, &manifest) == self.definition_sha256
                && package::sha256_hex(&bytes) == self.sha256)
        };
        check().unwrap_or(false)
    }
}

#[derive(Clone)]
pub struct Scan {
    generation: u64,
    assets: Vec<Asset>,
}

impl Scan {
    /// Three bounded plain-file reads per definition; no authority lock.
    pub fn unchanged(&self) -> bool {
        self.assets.iter().all(Asset::unchanged)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Offered,
    Playing { until_us: u64 },
    Terminal,
}

struct Entry {
    offer: RequestedAudioOffer,
    state: State,
}

/// The only authority for a Skills process. Restart creates another boot;
/// replace revokes all prior generations before installing new definitions.
pub struct Authority {
    enabled: bool,
    boot: String,
    generation: u64,
    next: u64,
    definitions: BTreeMap<String, Asset>,
    offers: Vec<Entry>,
    scanned_us: Option<u64>,
}

impl Authority {
    pub fn new(enabled: bool, boot: String) -> Self {
        Self {
            enabled,
            boot,
            generation: 0,
            next: 0,
            definitions: BTreeMap::new(),
            offers: Vec::new(),
            scanned_us: None,
        }
    }

    /// No I/O. Call only after the loaded generation is accepted, including
    /// any cap batch. Disabled inventory entries never grant execution.
    pub fn replace(&mut self, skills: &[Skill], now_us: u64) -> Result<(), String> {
        self.definitions.clear();
        self.scanned_us = None;
        for entry in &mut self.offers {
            entry.state = State::Terminal;
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("audio generation exhausted")?;
        if !self.enabled {
            return Ok(());
        }
        let mut definitions = BTreeMap::new();
        for skill in skills.iter().filter(|skill| skill.enabled) {
            if let Some(asset) = skill.requested_audio().filter(|_| skill.permissions.speak) {
                if definitions.len() >= MOST_DEFINITIONS || definitions.contains_key(&skill.name) {
                    return Err("too many or duplicate requested audio definitions".into());
                }
                definitions.insert(skill.name.clone(), asset.clone());
            }
        }
        self.definitions = definitions;
        self.scanned_us = Some(now_us);
        Ok(())
    }

    pub fn scan(&self) -> Scan {
        Scan {
            generation: self.generation,
            assets: self.definitions.values().cloned().collect(),
        }
    }

    /// An outdated scan can never freshen a replacement generation.
    /// Returns whether this generation remained accepted.
    pub fn scanned(&mut self, scan: &Scan, unchanged: bool, now_us: u64) -> bool {
        if scan.generation != self.generation {
            return false;
        }
        if unchanged {
            self.scanned_us = Some(now_us);
        } else {
            self.scanned_us = None;
            self.definitions.clear();
            for entry in &mut self.offers {
                entry.state = State::Terminal;
            }
        }
        unchanged
    }

    fn fresh(&self, now_us: u64) -> bool {
        self.enabled
            && self
                .scanned_us
                .is_some_and(|at| now_us >= at && now_us - at < SCAN_FRESH_US)
    }

    pub fn offer(
        &mut self,
        skill: &str,
        context: RequestedAudioContext,
        now_us: u64,
    ) -> Result<RequestedAudioOffer, String> {
        context.check().map_err(|e| e.to_string())?;
        if !self.fresh(now_us)
            || context.issued_us > now_us
            || now_us - context.issued_us >= REQUESTED_AUDIO_START_US
        {
            return Err("requested audio is disabled, stale or too late".into());
        }
        self.offers
            .retain(|entry| entry.offer.finish_before_us > now_us);
        if self
            .offers
            .iter()
            .any(|entry| entry.offer.context.same_call(&context))
        {
            return Err("requested audio call already consumed".into());
        }
        if self.offers.len() >= MOST_OFFERS {
            return Err("requested audio offer table full".into());
        }
        let asset = self
            .definitions
            .get(skill)
            .ok_or("requested audio definition not granted")?;
        self.next = self
            .next
            .checked_add(1)
            .ok_or("audio offer identifiers exhausted")?;
        let offer = RequestedAudioOffer {
            skills_boot: self.boot.clone(),
            definition_generation: self.generation,
            offer_id: self.next,
            skill: skill.to_owned(),
            asset: asset
                .path
                .to_str()
                .ok_or("audio path is not UTF-8")?
                .to_owned(),
            sha256: asset.sha256.clone(),
            frames: asset.frames,
            issued_us: context.issued_us,
            start_before_us: context.issued_us + REQUESTED_AUDIO_START_US,
            finish_before_us: context.issued_us
                + REQUESTED_AUDIO_START_US
                + REQUESTED_AUDIO_PLAY_US,
            context,
        };
        offer.check().map_err(|e| e.to_string())?;
        self.offers.push(Entry {
            offer: offer.clone(),
            state: State::Offered,
        });
        Ok(offer)
    }

    pub fn authorize(
        &mut self,
        offer: &RequestedAudioOffer,
        phase: RequestedAudioPhase,
        now_us: u64,
    ) -> Result<RequestedAudioLease, String> {
        offer.check().map_err(|e| e.to_string())?;
        let fresh = self.fresh(now_us)
            && offer.skills_boot == self.boot
            && offer.definition_generation == self.generation
            && self.definitions.contains_key(&offer.skill);
        let entry = self
            .offers
            .iter_mut()
            .find(|entry| entry.offer == *offer)
            .ok_or("audio offer identity not found")?;
        let allowed = fresh
            && now_us >= offer.issued_us
            && now_us < offer.finish_before_us
            && match (phase, entry.state) {
                (RequestedAudioPhase::Start, State::Offered) => now_us < offer.start_before_us,
                (RequestedAudioPhase::Renew, State::Playing { until_us }) => now_us < until_us,
                _ => false,
            };
        if !allowed {
            entry.state = State::Terminal;
            return Err("requested audio authorization ended".into());
        }
        let until_us = now_us
            .saturating_add(REQUESTED_AUDIO_LEASE_US)
            .min(offer.finish_before_us);
        entry.state = State::Playing { until_us };
        Ok(RequestedAudioLease {
            skills_boot: self.boot.clone(),
            definition_generation: self.generation,
            offer_id: offer.offer_id,
            until_us,
        })
    }

    pub fn finish(&mut self, offer: &RequestedAudioOffer) -> Result<(), String> {
        offer.check().map_err(|e| e.to_string())?;
        let entry = self
            .offers
            .iter_mut()
            .find(|entry| entry.offer == *offer)
            .ok_or("audio offer identity not found")?;
        if entry.state == State::Terminal {
            return Err("requested audio already ended".into());
        }
        entry.state = State::Terminal;
        Ok(())
    }

    pub fn operation(
        &mut self,
        op: RequestedAudioOperation,
        now_us: u64,
    ) -> Result<serde_json::Value, String> {
        op.check().map_err(|e| e.to_string())?;
        match op {
            RequestedAudioOperation::AudioAuthorize { offer, phase } => self
                .authorize(&offer, phase, now_us)
                .map(|lease| serde_json::json!(lease)),
            RequestedAudioOperation::AudioFinish { offer, outcome: _ } => self
                .finish(&offer)
                .map(|()| serde_json::json!({"finished":true})),
        }
    }
}

impl Skill {
    pub fn requested_audio(&self) -> Option<&Asset> {
        self.steps.iter().find_map(|(_, step)| match step {
            Step::Call(Call::RequestedAudio(asset)) => Some(asset),
            _ => None,
        })
    }
}
