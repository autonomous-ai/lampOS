//! The model an op runs, fetched from the Store, checked and kept (#7415,
//! the owner's decision and the first builder's four rules of 16:55).
//!
//! The image carries no model. A model is three files in the Store under
//! `<base>/<op>/<board>/`: [`MANIFEST`] (its id, op, head, board target,
//! file, size, SHA-256 and licence), [`SIGNATURE`] (the Store's Ed25519
//! signature over the manifest's exact bytes, the format of an app's
//! `lamp-skill.sig`, checked by `lamp_skills::package::check_signature`
//! with the Store's keys as the image ships them,
//! `lamp_skills::package::store_keys`; never a key the manifest names) and
//! the model file itself.
//!
//! In order, and nothing is written before the last step: the base must be
//! https on the pinned host; the manifest and signature are fetched into
//! memory, the signature checked before the manifest is read, the manifest
//! checked (this op, a head this build reads, this board, at most
//! [`MOST_MODEL_BYTES`]); the file is fetched into memory, at most its
//! manifest's size, and its size and SHA-256 checked. Only then are the
//! three written, into a staging folder beside the kept one, synced, and
//! swapped in by renames. A failed step leaves nothing behind.
//!
//! What is kept is checked again, signature and SHA-256, every time it is
//! loaded: a file changed on the disk is removed and never run.
use lamp_update::fetch::{Channel, Fetch};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub const MANIFEST: &str = "lamp-model.json";
pub const SIGNATURE: &str = "lamp-model.sig";
/// The manifest's format.
pub const FORMAT: u32 = 1;
/// The manifest's kind.
pub const KIND: &str = "npu-model";
/// The largest model kept: a pose model is 2 to 10 MB.
pub const MOST_MODEL_BYTES: u64 = 20 * 1024 * 1024;
/// The largest manifest read.
pub const MOST_MANIFEST_BYTES: u64 = 4 * 1024;
/// The largest signature read (`lamp_skills::package`'s own bound is 1 KiB).
pub const MOST_SIGNATURE_BYTES: u64 = 1024;

/// A model's manifest, `lamp-model.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelManifest {
    pub format: u32,
    pub kind: String,
    /// The model's own id and version, said in `status` (e.g.
    /// `movenet-lightning-4`).
    pub id: String,
    /// The op it answers: `pose`.
    pub op: String,
    /// How its outputs are read (`crate::head::HEADS`).
    pub head: String,
    /// The NPU it was converted for (`VIP9000NANODI_PID0X1000003B` on the
    /// A733).
    pub board: String,
    /// Its file's name beside the manifest.
    pub file: String,
    pub size: u64,
    pub sha256: String,
    /// Its licence's SPDX id, said in `status`.
    pub licence: String,
}

/// A plain name: 1 to 64 of letters, digits, `.`, `_` and `-`, not
/// starting with a dot.
fn plain(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 64
        && !text.starts_with('.')
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl ModelManifest {
    /// Its own rules, and that it is the model for `op` on `board`.
    pub fn check(&self, op: &str, board: &str) -> Result<(), String> {
        if self.format != FORMAT || self.kind != KIND {
            return Err(format!(
                "{MANIFEST}: format {} kind {:?} is not a model this lamp reads ({FORMAT}, {KIND:?})",
                self.format, self.kind
            ));
        }
        for (field, value) in [
            ("id", &self.id),
            ("op", &self.op),
            ("head", &self.head),
            ("board", &self.board),
            ("file", &self.file),
            ("licence", &self.licence),
        ] {
            if !plain(value) {
                return Err(format!("{MANIFEST}: {field} is not a plain name"));
            }
        }
        if self.op != op {
            return Err(format!("{MANIFEST}: a model for {:?}, not {op:?}", self.op));
        }
        if !crate::head::HEADS.contains(&self.head.as_str()) {
            return Err(format!(
                "{MANIFEST}: head {:?} is not one this lamp reads ({})",
                self.head,
                crate::head::HEADS.join(", ")
            ));
        }
        if self.board != board {
            return Err(format!(
                "{MANIFEST}: a model for {}, and this lamp's NPU is {board}",
                self.board
            ));
        }
        if self.file == MANIFEST || self.file == SIGNATURE {
            return Err(format!("{MANIFEST}: file is not the model's own"));
        }
        if self.size == 0 || self.size > MOST_MODEL_BYTES {
            return Err(format!(
                "{MANIFEST}: size {} is not 1 to {MOST_MODEL_BYTES} bytes",
                self.size
            ));
        }
        if !lamp_skills::package::sha256_ok(&self.sha256) {
            return Err(format!("{MANIFEST}: sha256 is not 64 lowercase hex"));
        }
        Ok(())
    }
}

/// The manifest's bytes read, after the signature over them is checked
/// against the Store's `keys`; then its own rules for `op` on `board`.
pub fn verified(
    manifest: &[u8],
    signature: &[u8],
    keys: &[[u8; 32]],
    op: &str,
    board: &str,
) -> Result<ModelManifest, String> {
    lamp_skills::package::check_signature(manifest, Some(signature), keys)
        .map_err(|why| why.replace("lamp-skill.sig", SIGNATURE))?;
    let parsed: ModelManifest =
        serde_json::from_slice(manifest).map_err(|error| format!("{MANIFEST}: {error}"))?;
    parsed.check(op, board)?;
    Ok(parsed)
}

/// Where models come from: an https base on the pinned host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub base: String,
}

impl Source {
    /// `base` if it is https on `host` exactly (no user, no port of its
    /// own, printable), as `lamp_skills::package::store_url_ok` and one host.
    pub fn new(base: &str, host: &str) -> Result<Self, String> {
        let base = base.trim_end_matches('/');
        if !lamp_skills::package::store_url_ok(base) {
            return Err(format!("{base:?} is not an https URL"));
        }
        let authority = base
            .strip_prefix("https://")
            .and_then(|rest| rest.split('/').next())
            .unwrap_or_default();
        if host.is_empty() || authority != host {
            return Err(format!(
                "{base:?} is not on the Store's host {host:?}: models come only from there"
            ));
        }
        Ok(Self {
            base: base.to_owned(),
        })
    }

    /// The URL of `name` for `op` on `board`.
    pub fn url(&self, op: &str, board: &str, name: &str) -> String {
        format!("{}/{op}/{board}/{name}", self.base)
    }
}

/// `url` read into memory, at most `most` bytes.
fn fetched(channel: &mut dyn Channel, url: &str, most: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    channel
        .fetch(url, most, &mut bytes)
        .map_err(|error| match error {
            Fetch::Missing => format!("the Store has no {}", url.rsplit('/').next().unwrap_or(url)),
            other => other.to_string(),
        })?;
    if bytes.len() as u64 > most {
        return Err(format!("{url}: more than {most} bytes"));
    }
    Ok(bytes)
}

/// The model kept for each op, under the service's state folder:
/// `<root>/<op>/` holds the three files.
#[derive(Clone, Debug)]
pub struct Store {
    pub root: PathBuf,
}

/// A model the Store offers: its manifest's exact bytes, their signature
/// (checked) and the manifest read from them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offered {
    pub bytes: Vec<u8>,
    pub signature: Vec<u8>,
    pub manifest: ModelManifest,
}

/// A model ready to load: its manifest and its checked file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    pub manifest: ModelManifest,
    pub path: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn folder(&self, op: &str) -> PathBuf {
        self.root.join(op)
    }

    /// Fetches `op`'s model for `board` from `source` through `channel`,
    /// checks it with `keys` and keeps it. Nothing is written unless every
    /// check passed.
    pub fn fetch(
        &self,
        channel: &mut dyn Channel,
        source: &Source,
        keys: &[[u8; 32]],
        op: &str,
        board: &str,
    ) -> Result<Kept, String> {
        let offered = self.offered(channel, source, keys, op, board)?;
        let file = self.fetch_file(channel, source, op, board, &offered)?;
        self.keep(op, &offered, &file)
    }

    /// What the Store offers for `op` on `board` now: its manifest and
    /// signature fetched into memory, the signature checked before the
    /// manifest is read. A few KiB, and nothing written: what an update
    /// check and a model the NPU refused look at before the file.
    pub fn offered(
        &self,
        channel: &mut dyn Channel,
        source: &Source,
        keys: &[[u8; 32]],
        op: &str,
        board: &str,
    ) -> Result<Offered, String> {
        let bytes = fetched(
            channel,
            &source.url(op, board, MANIFEST),
            MOST_MANIFEST_BYTES,
        )?;
        let signature = fetched(
            channel,
            &source.url(op, board, SIGNATURE),
            MOST_SIGNATURE_BYTES,
        )?;
        let manifest = verified(&bytes, &signature, keys, op, board)?;
        Ok(Offered {
            bytes,
            signature,
            manifest,
        })
    }

    /// The offered model's file, fetched into memory, at most its
    /// manifest's size, its size and SHA-256 checked.
    pub fn fetch_file(
        &self,
        channel: &mut dyn Channel,
        source: &Source,
        op: &str,
        board: &str,
        offered: &Offered,
    ) -> Result<Vec<u8>, String> {
        let manifest = &offered.manifest;
        let file = fetched(
            channel,
            &source.url(op, board, &manifest.file),
            manifest.size,
        )?;
        if file.len() as u64 != manifest.size || sha256_hex(&file) != manifest.sha256 {
            return Err(format!(
                "{}: its size or sha256 is not what {MANIFEST} says: not kept",
                manifest.file
            ));
        }
        Ok(file)
    }

    /// The offered model and its checked file kept for `op`, in place of
    /// any kept before.
    pub fn keep(&self, op: &str, offered: &Offered, file: &[u8]) -> Result<Kept, String> {
        self.put(
            op,
            &offered.bytes,
            &offered.signature,
            &offered.manifest,
            file,
        )
        .map_err(|error| format!("the model can't be kept: {error}"))?;
        Ok(Kept {
            path: self.folder(op).join(&offered.manifest.file),
            manifest: offered.manifest.clone(),
        })
    }

    fn refused_mark(&self, op: &str) -> PathBuf {
        self.root.join(format!("{op}.refused"))
    }

    /// Marks the model of this SHA-256 as refused by the NPU
    /// (`vip_create_network`, or a buffer this build can't read), so it is
    /// never fetched again: only a model of another SHA-256 is (Heron's
    /// P2 4 on #7972). Kept across restarts, beside the kept models.
    pub fn mark_refused(&self, op: &str, sha256: &str) {
        let written = fs::create_dir_all(&self.root)
            .and_then(|()| fs::write(self.refused_mark(op), sha256.as_bytes()));
        if let Err(error) = written {
            eprintln!("[lamp-npu] the refused model's mark can't be kept: {error}");
        }
    }

    /// The SHA-256 of `op`'s model the NPU last refused, if any.
    pub fn refused(&self, op: &str) -> Option<String> {
        let bytes = read_most(&self.refused_mark(op), 64).ok()?;
        let sha256 = String::from_utf8(bytes).ok()?;
        lamp_skills::package::sha256_ok(&sha256).then_some(sha256)
    }

    /// Writes the three checked files into a staging folder, syncs them,
    /// and swaps the folder in for `op`'s.
    fn put(
        &self,
        op: &str,
        manifest_bytes: &[u8],
        signature: &[u8],
        manifest: &ModelManifest,
        file: &[u8],
    ) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        let staging = self.root.join(format!(".{op}.new"));
        let old = self.root.join(format!(".{op}.old"));
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_dir_all(&old);
        fs::create_dir(&staging)?;
        let written = (|| {
            for (name, bytes) in [
                (MANIFEST, manifest_bytes),
                (SIGNATURE, signature),
                (manifest.file.as_str(), file),
            ] {
                let mut out = fs::File::create(staging.join(name))?;
                out.write_all(bytes)?;
                out.sync_all()?;
            }
            fs::File::open(&staging)?.sync_all()
        })();
        if let Err(error) = written {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        let folder = self.folder(op);
        if folder.exists() {
            fs::rename(&folder, &old)?;
        }
        if let Err(error) = fs::rename(&staging, &folder) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        let _ = fs::remove_dir_all(&old);
        fs::File::open(&self.root)?.sync_all()
    }

    /// `op`'s kept model, checked again (signature, manifest, size and
    /// SHA-256): `Ok(None)` when none is kept; `Err` when one is kept and
    /// fails, and then it is removed.
    pub fn load(&self, keys: &[[u8; 32]], op: &str, board: &str) -> Result<Option<Kept>, String> {
        let folder = self.folder(op);
        let manifest_bytes = match read_most(&folder.join(MANIFEST), MOST_MANIFEST_BYTES) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return self.refuse(op, format!("{MANIFEST}: {error}")),
        };
        let checked = (|| {
            let signature = read_most(&folder.join(SIGNATURE), MOST_SIGNATURE_BYTES)
                .map_err(|error| format!("{SIGNATURE}: {error}"))?;
            let manifest = verified(&manifest_bytes, &signature, keys, op, board)?;
            let path = folder.join(&manifest.file);
            let file = read_most(&path, manifest.size)
                .map_err(|error| format!("{}: {error}", manifest.file))?;
            if file.len() as u64 != manifest.size || sha256_hex(&file) != manifest.sha256 {
                return Err(format!(
                    "{}: changed on the disk since it was checked",
                    manifest.file
                ));
            }
            Ok(Kept { manifest, path })
        })();
        match checked {
            Ok(kept) => Ok(Some(kept)),
            Err(why) => self.refuse(op, why),
        }
    }

    fn refuse(&self, op: &str, why: String) -> Result<Option<Kept>, String> {
        self.remove(op);
        Err(format!(
            "the kept model failed its check and was removed: {why}"
        ))
    }

    /// `op`'s kept model removed.
    pub fn remove(&self, op: &str) {
        let _ = fs::remove_dir_all(self.folder(op));
    }
}

/// At most `most` bytes of `path`, else an error.
fn read_most(path: &Path, most: u64) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(most + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > most {
        return Err(io::Error::other(format!("more than {most} bytes")));
    }
    Ok(bytes)
}

/// The three files for a model, signed with `key_text` (an unencrypted
/// PKCS#8 Ed25519 key in base64, as `lamp-skills pack --key-file` takes):
/// what the Store's side serves (`lamp-npu pack`).
pub fn pack(key_text: &str, manifest: &ModelManifest) -> Result<(Vec<u8>, Vec<u8>), String> {
    let key = lamp_update::manifest::read_private_key(key_text).map_err(|why| why.0)?;
    let mut bytes = serde_json::to_vec_pretty(manifest).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    let signature = lamp_skills::package::signature_for(&bytes, &key)?;
    Ok((bytes, signature))
}

/// A model's manifest for `file`'s bytes.
pub fn manifest_for(
    file_name: &str,
    file: &[u8],
    id: &str,
    op: &str,
    head: &str,
    board: &str,
    licence: &str,
) -> ModelManifest {
    ModelManifest {
        format: FORMAT,
        kind: KIND.to_owned(),
        id: id.to_owned(),
        op: op.to_owned(),
        head: head.to_owned(),
        board: board.to_owned(),
        file: file_name.to_owned(),
        size: file.len() as u64,
        sha256: sha256_hex(file),
        licence: licence.to_owned(),
    }
}
