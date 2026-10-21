//! Requested audio authorization, without a playback path in Skills.
//!
//! The ordinary loader validates packages even with the switch off. A
//! runtime authority keeps at most 16 loaded audio definitions and offers.
//! Its caller scans the definitions every 2 s, outside the authority lock;
//! authorization fails closed 3 s after the last successful scan. Each scan
//! opens three plain files per definition and checks their identities and
//! metadata, then hashes all bounded payload bytes with SHA-256. Equal
//! metadata never skips content verification: filesystem timestamps can
//! repeat across writes, including later stores through a writable mapping.
//! A changed file is revoked on the next completed scan, or grants stop when
//! that scan stalls.
//! A lease adds at most 500 ms to that detection bound. Voice independently
//! checks the offer's digest against the immutable bytes it will dispatch.
use crate::{Call, Skill, Step, package};
use lamp_msgs::{
    REQUESTED_AUDIO_LEASE_US, REQUESTED_AUDIO_PLAY_US, REQUESTED_AUDIO_START_US,
    RequestedAudioContext, RequestedAudioLease, RequestedAudioOffer, RequestedAudioOperation,
    RequestedAudioPhase,
};
use ring::digest::{Context, Digest, SHA256, digest};
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
    parent_identities: Vec<(u64, u64)>,
}

/// A before/after observation detects file replacement and ordinary writes
/// during a read. Equal stamps do not prove equal contents, so every scan
/// also hashes the actual bytes. Access time is excluded because reads can
/// update it themselves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
    mode: u32,
    uid: u32,
    gid: u32,
}

impl Stamp {
    fn of(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            size: metadata.len(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
            mode: metadata.mode(),
            uid: metadata.uid(),
            gid: metadata.gid(),
        }
    }
}

struct PlainFile {
    file: fs::File,
    path: PathBuf,
    stamp: Stamp,
    most: u64,
    parents: Vec<(PathBuf, (u64, u64))>,
    stamp_of: fn(&fs::Metadata) -> Stamp,
}

impl PlainFile {
    fn stable(&self) -> Result<(), String> {
        for (path, identity) in &self.parents {
            let meta = fs::symlink_metadata(path).map_err(|_| "audio folder missing")?;
            if !meta.is_dir()
                || meta.file_type().is_symlink()
                || (meta.dev(), meta.ino()) != *identity
            {
                return Err("audio folder changed while checking".into());
            }
        }
        let opened = self
            .file
            .metadata()
            .map_err(|_| "audio file not readable")?;
        let listed = fs::symlink_metadata(&self.path).map_err(|_| "audio file missing")?;
        if !opened.is_file()
            || !listed.is_file()
            || listed.file_type().is_symlink()
            || (self.stamp_of)(&opened) != self.stamp
            || (self.stamp_of)(&listed) != self.stamp
        {
            return Err("audio file changed while checking".into());
        }
        Ok(())
    }

    fn bytes(&mut self, read: &mut u64) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        (&mut self.file)
            .take(self.most + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "audio file not readable")?;
        *read += bytes.len() as u64;
        if bytes.len() as u64 > self.most {
            return Err("audio file exceeds its bound".into());
        }
        self.stable()?;
        Ok(bytes)
    }
}

fn hex_digest(hash: Digest) -> String {
    hash.as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn audio_hash(bytes: &[u8]) -> String {
    hex_digest(digest(&SHA256, bytes))
}

fn definition_hash(description: &[u8], manifest: &[u8]) -> String {
    let mut hash = Context::new(&SHA256);
    for bytes in [description, manifest] {
        hash.update(&(bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    hex_digest(hash.finish())
}

/// The package reader's no-link/nonblocking/plain-file protections, applied
/// to one already validated relative path. Only its at most four components
/// are inspected; a runtime scan does not enumerate unrelated directories.
fn plain_file(root: &Path, relative: &str, most: u64) -> Result<PlainFile, String> {
    plain_file_with(root, relative, most, Stamp::of)
}

fn plain_file_with(
    root: &Path,
    relative: &str,
    most: u64,
    stamp_of: fn(&fs::Metadata) -> Stamp,
) -> Result<PlainFile, String> {
    let path = root.join(relative);
    let mut parent = root.to_path_buf();
    let mut parents = Vec::new();
    let parts: Vec<_> = relative.split('/').collect();
    for part in &parts[..parts.len() - 1] {
        parent.push(part);
        let meta = fs::symlink_metadata(&parent).map_err(|_| "audio folder missing")?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err("audio folder is not a plain folder".into());
        }
        parents.push((parent.clone(), (meta.dev(), meta.ino())));
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
    if !meta.is_file() || meta.len() > most || stamp_of(&meta) != stamp_of(&listed) {
        return Err("audio file changed or exceeds its bound".into());
    }
    Ok(PlainFile {
        file,
        path,
        stamp: stamp_of(&meta),
        most,
        parents,
        stamp_of,
    })
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
        if sound.sha256 != audio_hash(bytes) {
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
        // Check descriptor images against the validated package, with path
        // and parent identity checks bracketing their bounded content reads.
        let mut description = plain_file(&root, "SKILL.md", MOST_DEFINITION_BYTES)?;
        let mut manifest = plain_file(&root, "skill.toml", MOST_DEFINITION_BYTES)?;
        let mut recording = plain_file(&root, relative, lamp_voice::requested_audio::MOST_BYTES)?;
        let mut read = 0;
        if definition_hash(&description.bytes(&mut read)?, &manifest.bytes(&mut read)?)
            != definition_sha256
            || audio_hash(&recording.bytes(&mut read)?) != sound.sha256
        {
            return Err("requested audio changed while loading".into());
        }
        description.stable()?;
        manifest.stable()?;
        recording.stable()?;
        let asset = Self {
            path,
            sha256: sound.sha256.clone(),
            frames: u32::try_from(sound.samples.len()).map_err(|_| "too many audio frames")?,
            root,
            relative: relative.to_owned(),
            definition_sha256,
            root_identity: (root_meta.dev(), root_meta.ino()),
            file_identity: (file_meta.dev(), file_meta.ino()),
            parent_identities: recording.parents.iter().map(|(_, id)| *id).collect(),
        };
        if !asset.root_unchanged()
            || (recording.stamp.device, recording.stamp.inode) != asset.file_identity
        {
            return Err("requested audio changed while loading".into());
        }
        Ok(asset)
    }

    pub(crate) fn matches_definition(&self, description: &str, manifest: &str) -> bool {
        self.definition_sha256 == definition_hash(description.as_bytes(), manifest.as_bytes())
    }

    /// Potential I/O: always called outside the authority mutex.
    pub fn unchanged(&self) -> bool {
        self.checked(&mut 0).is_ok()
    }

    fn root_unchanged(&self) -> bool {
        fs::symlink_metadata(&self.root).is_ok_and(|root| {
            root.is_dir()
                && !root.file_type().is_symlink()
                && (root.dev(), root.ino()) == self.root_identity
        })
    }

    fn checked(&self, read: &mut u64) -> Result<(), String> {
        self.checked_with(read, Stamp::of)
    }

    fn checked_with(
        &self,
        read: &mut u64,
        stamp_of: fn(&fs::Metadata) -> Stamp,
    ) -> Result<(), String> {
        if !self.root_unchanged() {
            return Err("audio folder changed".into());
        }
        let mut description =
            plain_file_with(&self.root, "SKILL.md", MOST_DEFINITION_BYTES, stamp_of)?;
        let mut manifest =
            plain_file_with(&self.root, "skill.toml", MOST_DEFINITION_BYTES, stamp_of)?;
        let mut recording = plain_file_with(
            &self.root,
            &self.relative,
            lamp_voice::requested_audio::MOST_BYTES,
            stamp_of,
        )?;
        if (recording.stamp.device, recording.stamp.inode) != self.file_identity {
            return Err("audio file replaced".into());
        }
        if !recording
            .parents
            .iter()
            .map(|(_, id)| id)
            .eq(self.parent_identities.iter())
        {
            return Err("audio folder replaced".into());
        }
        if definition_hash(&description.bytes(read)?, &manifest.bytes(read)?)
            != self.definition_sha256
        {
            return Err("audio definition changed".into());
        }
        if audio_hash(&recording.bytes(read)?) != self.sha256 {
            return Err("audio file changed".into());
        }
        description.stable()?;
        manifest.stable()?;
        recording.stable()?;
        if !self.root_unchanged() {
            return Err("audio folder changed".into());
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct Scan {
    generation: u64,
    assets: BTreeMap<String, Asset>,
    bytes_read: u64,
}

impl Scan {
    /// Hash three bounded plain files per definition on every scan.
    /// Call without the authority lock, then return the result to `scanned`.
    pub fn unchanged(&mut self) -> bool {
        self.bytes_read = 0;
        for asset in self.assets.values() {
            if asset.checked(&mut self.bytes_read).is_err() {
                return false;
            }
        }
        true
    }

    /// Payload bytes read by the most recent check, for scan-cost evidence.
    pub fn bytes_read(&self) -> u64 {
        self.bytes_read
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
            assets: self.definitions.clone(),
            bytes_read: 0,
        }
    }

    /// An outdated scan can never freshen a replacement generation.
    /// Returns whether this generation remained accepted.
    pub fn scanned(&mut self, scan: &Scan, unchanged: bool, now_us: u64) -> bool {
        if scan.generation != self.generation {
            return false;
        }
        // A failed scan clears the inventory without changing generation.
        // A later stale result must never restore that revoked inventory.
        let unchanged = unchanged && self.definitions.keys().eq(scan.assets.keys());
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

#[cfg(test)]
mod file_checks {
    use super::*;

    fn coarse_stamp(metadata: &fs::Metadata) -> Stamp {
        // Model equal timestamp observations for distinct file images. All
        // identities, sizes, modes and owner fields still come from the file.
        Stamp {
            modified: (1, 0),
            changed: (1, 0),
            ..Stamp::of(metadata)
        }
    }

    #[test]
    fn equal_stamps_cannot_hide_changed_audio_or_permission_bytes() {
        for permission in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            fs::create_dir(dir.path().join("sounds")).unwrap();
            fs::write(dir.path().join("SKILL.md"), b"fixed fixture\n").unwrap();
            fs::write(dir.path().join("skill.toml"), b"speak = true\n").unwrap();
            let mut wav = hound::WavWriter::create(
                dir.path().join("sounds/fixture.wav"),
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 24_000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for _ in 0..240 {
                wav.write_sample(100_i16).unwrap();
            }
            wav.finalize().unwrap();
            let asset = Asset::load(dir.path(), "sounds/fixture.wav").unwrap();
            let payload = ["SKILL.md", "skill.toml", "sounds/fixture.wav"]
                .into_iter()
                .map(|name| fs::metadata(dir.path().join(name)).unwrap().len())
                .sum::<u64>();
            for _ in 0..3 {
                let mut read = 0;
                asset.checked_with(&mut read, coarse_stamp).unwrap();
                assert_eq!(
                    read, payload,
                    "even repeated equal stamps require full reads"
                );
            }
            let path = if permission {
                dir.path().join("skill.toml")
            } else {
                asset.path.clone()
            };
            let before = coarse_stamp(&fs::metadata(&path).unwrap());
            let changed = if permission {
                b"speak =false\n".to_vec()
            } else {
                let mut bytes = fs::read(&path).unwrap();
                let last = bytes.len() - 2;
                bytes[last..].copy_from_slice(&200_i16.to_le_bytes());
                bytes
            };
            fs::write(&path, &changed).unwrap();
            assert_eq!(coarse_stamp(&fs::metadata(&path).unwrap()), before);
            assert_eq!(fs::read(&path).unwrap(), changed);
            let mut read = 0;
            assert_eq!(
                asset.checked_with(&mut read, coarse_stamp).unwrap_err(),
                if permission {
                    "audio definition changed"
                } else {
                    "audio file changed"
                },
                "permission={permission}"
            );
            assert!(
                read > 0,
                "the changed image must be read, not inferred from metadata"
            );
        }
    }

    #[test]
    fn sha256_keeps_the_existing_lowercase_and_length_prefix_encoding() {
        assert_eq!(
            audio_hash(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        for (description, manifest) in [
            (b"ab".as_slice(), b"c".as_slice()),
            (b"a", b"bc"),
            (b"", b""),
        ] {
            let mut encoded = Vec::new();
            for bytes in [description, manifest] {
                encoded.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
                encoded.extend_from_slice(bytes);
            }
            assert_eq!(
                definition_hash(description, manifest),
                package::sha256_hex(&encoded)
            );
        }
        assert_ne!(definition_hash(b"ab", b"c"), definition_hash(b"a", b"bc"));
    }

    #[test]
    fn a_replaced_path_cannot_supply_the_stamp_for_already_opened_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skill.toml");
        fs::write(&path, b"original").unwrap();
        let mut opened = plain_file_with(dir.path(), "skill.toml", 64, coarse_stamp).unwrap();
        fs::rename(&path, dir.path().join("old.toml")).unwrap();
        fs::write(&path, b"original").unwrap();
        // Renaming can change the old inode's ctime. Hold observed times
        // equal so only the listed path's replacement can refuse the read.
        assert_eq!(coarse_stamp(&opened.file.metadata().unwrap()), opened.stamp);
        assert_ne!(coarse_stamp(&fs::metadata(&path).unwrap()), opened.stamp);
        assert!(opened.bytes(&mut 0).is_err());
    }

    #[test]
    fn a_replaced_parent_is_refused_even_when_the_leaf_inode_is_the_same() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sounds")).unwrap();
        fs::create_dir(dir.path().join("replacement")).unwrap();
        fs::write(dir.path().join("sounds/fixture.wav"), b"original").unwrap();
        fs::hard_link(
            dir.path().join("sounds/fixture.wav"),
            dir.path().join("replacement/fixture.wav"),
        )
        .unwrap();
        let mut opened = plain_file(dir.path(), "sounds/fixture.wav", 64).unwrap();
        fs::rename(dir.path().join("sounds"), dir.path().join("previous")).unwrap();
        fs::rename(dir.path().join("replacement"), dir.path().join("sounds")).unwrap();
        assert_eq!(
            Stamp::of(&fs::metadata(dir.path().join("sounds/fixture.wav")).unwrap()),
            opened.stamp
        );
        assert!(opened.bytes(&mut 0).is_err());
    }
}
