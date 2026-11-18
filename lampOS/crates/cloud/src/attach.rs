//! A photo or a text the owner sends from the app to the spoken turn
//! (`LAMP_ATTACHMENTS=on`, off by default; #7659 slice 1, the lamp side of
//! Seventh's contract with the first builder's rulings of 18 Nov 10:24).
//!
//! The app uploads it to the backend, which sends the kind `attach.<id>`
//! ([`KIND_PREFIX`]; `id` the first 16 lowercase hex of its SHA-256) with
//! nothing inline, as `skills.upload` (`crate::upload`). `lamp-cloud`
//! answers `starting` at once, then fetches
//! `GET <base>/devices/get-message?kind=attach.<id>` with its own key, whose
//! `data` is `{id, kind, mime, size, sha256, sent_at_ms, content_base64,
//! request_id?}`. It checks ([`decoded`]) the kind (`photo` or `text`), the
//! mime, the size (a JPEG of at most 184 KiB, a UTF-8 text of at most
//! 64 KiB), the SHA-256 and the id, writes it to
//! `/run/lampos-attachments/<id>` (0600, in a 0700 folder; tmpfs, so a
//! reboot empties it; only the newest is kept), and puts an
//! `attachments/arrived` event on the bus (`lamp_msgs::AttachRef`, a path,
//! never the bytes). Then the kind's second reply, `success` or `failure`,
//! lets the backend delete its copy. `request_id` ("Lamp asks for the
//! file") is reserved and ignored for now.
//!
//! The journal says the id, the kind, the bytes and the ms, never what it
//! holds ([`journal_line`]).
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use lamp_msgs::AttachRef;
use serde_json::{Value, json};

use crate::settings::Outcome;

/// `on` takes the kind and names [`CAN`] in `can`.
pub const ENVIRONMENT: &str = "LAMP_ATTACHMENTS";
/// The word `system.info`'s `can` names while the switch is on.
pub const CAN: &str = "attach";
/// The kind the backend sends: this, then the id.
pub const KIND_PREFIX: &str = "attach.";
/// The most bytes of the backend's answer: a photo in base64 (about
/// 246 KiB at most) and its fields.
pub const MOST_MESSAGE_BYTES: u64 = 320 * 1_024;

pub const NO_MESSAGE: &str = "the backend didn't give the attachment";
pub const MISMATCH: &str =
    "the attachment isn't the one uploaded: its SHA-256, size or id doesn't match";
pub const TOO_BIG: &str = "the attachment is too big: a photo is at most 184 KiB, a text 64 KiB";
pub const NOT_TAKEN: &str =
    "the attachment is neither a JPEG photo nor a UTF-8 text (text/plain or text/markdown)";
pub const NOT_KEPT: &str = "the attachment couldn't be kept on the lamp";
pub const NOT_TOLD: &str = "the lamp's voice couldn't be told of the attachment";
pub const BUSY: &str = "another attachment is still being fetched";

/// Whether the switch is on.
pub fn on_from_environment() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// The id of a kind `attach.<id>`: 16 lowercase hex.
pub fn id_of(kind: &str) -> Option<&str> {
    kind.strip_prefix(KIND_PREFIX)
        .filter(|id| AttachRef::id_ok(id))
}

/// Where an attachment is asked for, kept and told, and the results
/// waiting to go back to the app.
pub struct Attachments {
    /// The backend's base with `/v1` cut off, as the ping's.
    pub base: String,
    pub key: String,
    pub device_id: String,
    /// [`AttachRef::DIR`] on a lamp.
    pub dir: PathBuf,
    /// HTTPS only: off only in tests, against a local port.
    pub https_only: bool,
    /// The bus `attachments/arrived` goes on; none tells nobody.
    pub bus: Option<lamp_bus::Bus>,
    busy: AtomicBool,
    done: Mutex<Vec<Outcome>>,
}

impl std::fmt::Debug for Attachments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Attachments")
            .field("base", &self.base)
            .field("key", &"<hidden>")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl Attachments {
    /// The product's, from the account's base and the lamp's key; none
    /// without either.
    pub fn product(
        account: &crate::account::Account,
        key: &str,
        bus: Option<lamp_bus::Bus>,
    ) -> Option<Arc<Self>> {
        let base = account.llm_base_url.trim().trim_end_matches('/');
        let base = base.strip_suffix("/v1").unwrap_or(base);
        let key = key.trim();
        (!base.is_empty() && !key.is_empty()).then(|| {
            let mut attachments =
                Self::new(base, key, &account.device_id, Path::new(AttachRef::DIR));
            attachments.bus = bus;
            Arc::new(attachments)
        })
    }

    pub fn new(base: &str, key: &str, device_id: &str, dir: &Path) -> Self {
        Self {
            base: base.to_owned(),
            key: key.to_owned(),
            device_id: device_id.to_owned(),
            dir: dir.into(),
            https_only: true,
            bus: None,
            busy: AtomicBool::new(false),
            done: Mutex::new(Vec::new()),
        }
    }

    /// `kind` taken (`attach.<id>`): fetched, checked, kept and told on a
    /// thread of its own, one at a time ([`BUSY`]); its result kept for
    /// [`Self::take`].
    pub fn start(self: &Arc<Self>, kind: &str) -> Result<(), String> {
        let id = id_of(kind)
            .ok_or_else(|| crate::wire::not_here(kind))?
            .to_owned();
        if self.busy.swap(true, Ordering::AcqRel) {
            return Err(BUSY.into());
        }
        let attachments = Arc::clone(self);
        let kind = kind.to_owned();
        let spawned = std::thread::Builder::new()
            .name("lamp-cloud-attach".into())
            .spawn(move || {
                let began = Instant::now();
                let result = attachments.run(&id);
                attachments.busy.store(false, Ordering::Release);
                eprintln!(
                    "lamp-cloud: {}",
                    journal_line(&id, &result, began.elapsed())
                );
                attachments.keep(Outcome {
                    kind,
                    result: result.map(|attach| {
                        Some(json!({"id": attach.id, "kind": attach.kind, "bytes": attach.bytes}))
                    }),
                    request_id: None,
                    code: None,
                });
            })
            .is_ok();
        if !spawned {
            self.busy.store(false, Ordering::Release);
            return Err(NOT_KEPT.into());
        }
        Ok(())
    }

    /// The fetch, the checks, the file and the bus, blocking: what was
    /// told, or why not. Nothing is left in the folder on a failure.
    pub fn run(&self, id: &str) -> Result<AttachRef, String> {
        let data = crate::upload::fetch_kind(
            (&self.base, &self.key, &self.device_id),
            self.https_only,
            &format!("{KIND_PREFIX}{id}"),
            MOST_MESSAGE_BYTES,
        )
        .map_err(|why| {
            if why == crate::upload::TOO_BIG {
                TOO_BIG.to_owned()
            } else {
                NO_MESSAGE.to_owned()
            }
        })?;
        let (kind, mime, bytes, sent_at) = decoded(&data, id)?;
        let path = self.write(id, &bytes)?;
        let attach = AttachRef {
            path: path.clone(),
            id: id.to_owned(),
            kind,
            mime,
            bytes: bytes.len() as u64,
            sent_at,
        };
        if let Err(why) = self.tell(&attach) {
            let _ = std::fs::remove_file(path);
            return Err(why);
        }
        Ok(attach)
    }

    /// `bytes` as `<dir>/<id>`, a new file (0600), synced; any other
    /// attachment in the folder goes first, so only the newest is kept.
    fn write(&self, id: &str, bytes: &[u8]) -> Result<String, String> {
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        let path = self.dir.join(id);
        let written = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()));
        if written.is_err() {
            let _ = std::fs::remove_file(&path);
            return Err(NOT_KEPT.into());
        }
        path.to_str()
            .map(str::to_owned)
            .ok_or_else(|| NOT_KEPT.to_owned())
    }

    /// `attachments/arrived` on the bus.
    fn tell(&self, attach: &AttachRef) -> Result<(), String> {
        attach.check().map_err(|_| NOT_KEPT.to_owned())?;
        let bus = self.bus.as_ref().ok_or(NOT_TOLD)?;
        let mut out = bus
            .publisher_declared(
                AttachRef::TOPIC,
                &AttachRef::declaration(),
                crate::lamp::SOURCE,
            )
            .map_err(|_| NOT_TOLD.to_owned())?;
        out.publish_json(lamp_ipc::monotonic_us(), attach)
            .map(|_| ())
            .map_err(|_| NOT_TOLD.to_owned())
    }

    fn keep(&self, outcome: Outcome) {
        if let Ok(mut done) = self.done.lock() {
            done.push(outcome);
        }
    }

    /// The results ready, oldest first, to go to the app.
    pub fn take(&self) -> Vec<Outcome> {
        self.done
            .lock()
            .map(|mut done| std::mem::take(&mut *done))
            .unwrap_or_default()
    }

    /// Results not handed to the broker: first again.
    pub fn owe(&self, again: Vec<Outcome>) {
        if again.is_empty() {
            return;
        }
        if let Ok(mut done) = self.done.lock() {
            let later = std::mem::take(&mut *done);
            done.extend(again);
            done.extend(later);
        }
    }
}

/// The journal's line for an attachment: its id, kind, bytes and ms, or
/// its id, one of this module's fixed reasons and ms. Never what it holds.
pub fn journal_line(id: &str, result: &Result<AttachRef, String>, took: Duration) -> String {
    let ms = took.as_millis();
    match result {
        Ok(attach) => format!(
            "an attachment from the app: id {id}, {}, {} bytes, kept and told in {ms} ms",
            attach.kind, attach.bytes
        ),
        Err(why) => {
            const FIXED: [&str; 7] = [
                NO_MESSAGE, MISMATCH, TOO_BIG, NOT_TAKEN, NOT_KEPT, NOT_TOLD, BUSY,
            ];
            let why = FIXED
                .into_iter()
                .find(|fixed| *fixed == why.as_str())
                .unwrap_or(NO_MESSAGE);
            format!("an attachment from the app: id {id}, not kept: {why}, in {ms} ms")
        }
    }
}

/// The attachment in `data` (`{id, kind, mime, size, sha256, sent_at_ms,
/// content_base64}`), decoded: its kind, its mime, its bytes and when it was
/// sent, only when the kind is `photo` (a JPEG, `image/jpeg`) or `text`
/// (UTF-8, `text/plain` or `text/markdown`), it is within its kind's bound,
/// and its size, its SHA-256 and its id (the SHA-256's first 16 hex, the
/// kind's) are what was said.
pub fn decoded(data: &Value, id: &str) -> Result<(String, String, Vec<u8>, u64), String> {
    let kind = data["kind"].as_str().unwrap_or_default();
    let most = AttachRef::most_bytes(kind).ok_or(NOT_TAKEN)?;
    let size = data["size"].as_u64().ok_or(NO_MESSAGE)?;
    if size > most {
        return Err(TOO_BIG.into());
    }
    let content = data["content_base64"].as_str().ok_or(NO_MESSAGE)?;
    // Base64 is four characters for three bytes: anything longer than the
    // bound's is refused before it is decoded.
    if content.len() as u64 > most.div_ceil(3) * 4 + 4 {
        return Err(TOO_BIG.into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(content.trim())
        .map_err(|_| MISMATCH.to_owned())?;
    let sha256 = data["sha256"]
        .as_str()
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let actual = lamp_skills::package::sha256_hex(&bytes);
    if bytes.is_empty()
        || bytes.len() as u64 != size
        || actual != sha256
        || data["id"].as_str() != Some(id)
        || !actual.starts_with(id)
    {
        return Err(MISMATCH.into());
    }
    let mime = data["mime"]
        .as_str()
        .and_then(|mime| mime.split(';').next())
        .map(|mime| mime.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let taken = match kind {
        AttachRef::PHOTO => mime == "image/jpeg" && bytes.starts_with(&[0xFF, 0xD8]),
        _ => {
            matches!(mime.as_str(), "text/plain" | "text/markdown")
                && std::str::from_utf8(&bytes).is_ok()
        }
    };
    if !taken {
        return Err(NOT_TAKEN.into());
    }
    let sent_at = data["sent_at_ms"].as_u64().unwrap_or(0);
    Ok((kind.to_owned(), mime, bytes, sent_at))
}

#[cfg(test)]
mod tests;
