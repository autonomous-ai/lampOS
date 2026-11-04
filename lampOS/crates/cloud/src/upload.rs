//! A skill added from a file in the owner's app (`LAMP_SKILL_UPLOAD=on`,
//! off by default; the first builder on the Queue, 3 Nov 14:33, with
//! Backend3's rules and contract). The person picks a file on their phone,
//! a package as `lamp-skills pack` makes it or a bare `SKILL.md`, and the
//! app uploads it to the backend. The lamp is told by the kind
//! `skills.upload.<upload_id>` ([`KIND_PREFIX`]; `upload_id` the first 16
//! lowercase hex of the file's SHA-256) with no file inline, as V1's
//! privacy kinds and `connector.set`: it answers `starting` at once, which
//! keeps the file on the backend, then fetches
//! `GET <base>/devices/get-message?kind=skills.upload.<upload_id>` with its
//! own key ([`fetch_message`]), whose `data` carries the file itself:
//! `{upload_id, name, version, format, sha256, size, content_base64}`. The
//! lamp decodes it, at most [`MOST_BYTES`], checks its size, its SHA-256 and
//! its `upload_id` against the kind ([`decoded`]), and reads it
//! ([`files_of`]):
//!
//! - **Never as the Store's**: a package carrying the Store's signature is
//!   refused, [`FROM_THE_STORE`]; the Store has its own route.
//! - **No code**: a `run.py`, or any file but `skill.toml` and `SKILL.md`.
//! - **A name the lamp holds** (core, Store, pushed; lamp-voice adds those
//!   made or added here) is refused, never replaced.
//!
//! Then the two files go to lamp-voice's app-make socket
//! (`lamp_voice::upload_skill`), which holds them to a made skill's checks
//! and its own (no trigger, the cap, at most five), keeps it, and asks
//! aloud; only a spoken yes at the lamp puts it on. The kind's second
//! reply, which lets the backend delete the file, waits for that: `success`
//! once a spoken yes put it on, `failure` with a plain reason on any check
//! that fails, or when it was left off ([`Uploads::settle`]).
//!
//! The file's words and its name are never in the journal.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{Value, json};

use crate::settings::Outcome;

/// `on` answers the kind, with the skills page (`LAMP_SKILLS_PAGE`).
pub const ENVIRONMENT: &str = "LAMP_SKILL_UPLOAD";
/// The kind's family, as `system.info`'s `can` names it.
pub const KIND: &str = "skills.upload";
/// The kind the backend sends: this, then the `upload_id`.
pub const KIND_PREFIX: &str = "skills.upload.";
/// The backend's route for a kind's data, above its base with `/v1` cut
/// off, as V1's privacy kinds (`privacy_fetch.go`).
pub const FETCH_PATH: &str = "/devices/get-message";
/// The most bytes of the file, decoded: the backend's own 1 MiB.
pub const MOST_BYTES: u64 = 1_024 * 1_024;
/// The most bytes of the backend's answer: the file in base64 (about
/// 1.4 MiB at most) and its fields.
pub const MOST_MESSAGE_BYTES: u64 = 1_536 * 1_024 + 64 * 1_024;
/// The longest the fetch takes.
pub const WITHIN: Duration = Duration::from_secs(30);
/// The longest lamp-voice is waited for: its check (`lamp-skills pack`,
/// at most 20 s) and its keep.
pub const VOICE_WAIT: Duration = Duration::from_secs(25);
/// How often lamp-voice is asked what became of a kept skill.
pub const SETTLE_EVERY: Duration = Duration::from_secs(5);
/// The longest a kept skill is waited for: its ask may wait an hour for a
/// good moment (the say service's longest), then 120 s for the yes.
pub const SETTLE_WITHIN: Duration = Duration::from_secs(2 * 3_600);

/// The most bytes of each of the two files, lamp-voice's own bound
/// (`lamp_voice::made_skill`'s 8 KiB).
pub const MOST_FILE_BYTES: usize = 8 * 1_024;

pub const TOO_LONG: &str = "one of its files is longer than 8 KiB";
pub const FROM_THE_STORE: &str =
    "this skill carries the Skills Store's signature: add it from the Skill Store";
pub const NO_CODE: &str = "it carries code (run.py), which a skill added from a file can't";
pub const OTHER_FILES: &str =
    "it carries files other than skill.toml and SKILL.md, which a skill added from a file can't";
pub const NOT_A_SKILL: &str = "the file isn't a skill's package or a SKILL.md";
pub const MISMATCH: &str = "the file isn't the one uploaded: its SHA-256, size or id doesn't match";
pub const TOO_BIG: &str = "the file is bigger than 1 MiB, too big for a skill";
pub const NO_MESSAGE: &str = "the backend didn't give the file to add";
pub const BUSY: &str = "a skill from a file is still being added";
pub const UNCHECKED: &str = "a signed package can't be checked on this lamp now";
pub const VOICE_UNREACHED: &str = "adding a skill can't be reached on the lamp now";
pub const LEFT_OFF: &str = "it wasn't put on: there was no spoken yes at the lamp";
pub const NEVER_ASKED: &str = "it was never asked about aloud, so it isn't on";
pub const LET_GO: &str = "it is no longer kept on the lamp";

/// Whether the switch is on.
pub fn on_from_environment() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// The `upload_id` of a kind `skills.upload.<upload_id>`: 16 lowercase hex.
pub fn id_of(kind: &str) -> Option<&str> {
    kind.strip_prefix(KIND_PREFIX)
        .filter(|id| id.len() == 16 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// Where the file is asked for and handed on, and the results waiting to
/// go back to the app.
pub struct Uploads {
    /// The backend's base with `/v1` cut off, as the ping's.
    pub base: String,
    pub key: String,
    pub device_id: String,
    /// `lamp-skills`' folder and the pushed skills' folder: a name there is
    /// held.
    pub skills: PathBuf,
    pub pushed: PathBuf,
    /// The Store's key file, for [`FROM_THE_STORE`].
    pub store_key: PathBuf,
    /// lamp-voice's app-make socket.
    pub socket: PathBuf,
    /// HTTPS only: off only in tests, against a local port.
    pub https_only: bool,
    /// [`SETTLE_EVERY`] and [`SETTLE_WITHIN`], shorter in tests.
    pub settle_every: Duration,
    pub settle_within: Duration,
    busy: AtomicBool,
    done: Mutex<Vec<Outcome>>,
}

impl std::fmt::Debug for Uploads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Uploads")
            .field("base", &self.base)
            .field("key", &"<hidden>")
            .finish_non_exhaustive()
    }
}

impl Uploads {
    /// The product's, from the account's base and the lamp's key; none
    /// without either.
    pub fn product(account: &crate::account::Account, key: &str) -> Option<Arc<Self>> {
        let base = account.llm_base_url.trim().trim_end_matches('/');
        let base = base.strip_suffix("/v1").unwrap_or(base);
        let key = key.trim();
        (!base.is_empty() && !key.is_empty()).then(|| {
            Arc::new(Self::new(
                base,
                key,
                &account.device_id,
                Path::new("/var/lib/lampOS/skills"),
                Path::new(lamp_skills::PUSHED_DIR),
                Path::new(lamp_skills::package::STORE_KEY),
                Path::new(crate::make_skill::SOCKET),
            ))
        })
    }

    pub fn new(
        base: &str,
        key: &str,
        device_id: &str,
        skills: &Path,
        pushed: &Path,
        store_key: &Path,
        socket: &Path,
    ) -> Self {
        Self {
            base: base.to_owned(),
            key: key.to_owned(),
            device_id: device_id.to_owned(),
            skills: skills.into(),
            pushed: pushed.into(),
            store_key: store_key.into(),
            socket: socket.into(),
            https_only: true,
            settle_every: SETTLE_EVERY,
            settle_within: SETTLE_WITHIN,
            busy: AtomicBool::new(false),
            done: Mutex::new(Vec::new()),
        }
    }

    /// `kind` taken (`skills.upload.<upload_id>`): one fetch and check at a
    /// time, on a thread of its own, then the wait for the spoken yes on
    /// the same thread, the next upload free to start; its result kept for
    /// [`Self::take`].
    pub fn start(self: &Arc<Self>, kind: &str) -> Result<(), String> {
        let id = id_of(kind)
            .ok_or_else(|| crate::wire::not_here(kind))?
            .to_owned();
        if self.busy.swap(true, Ordering::AcqRel) {
            return Err(BUSY.into());
        }
        let uploads = Arc::clone(self);
        let kind = kind.to_owned();
        let spawned = std::thread::Builder::new()
            .name("lamp-cloud-upload".into())
            .spawn(move || {
                let (kept, journal) = uploads.run(&id);
                uploads.busy.store(false, Ordering::Release);
                for line in journal {
                    eprintln!("lamp-cloud: {line}");
                }
                let result = match kept {
                    Ok(kept) => uploads.settle(kept),
                    Err(why) => Err(why),
                };
                eprintln!(
                    "lamp-cloud: a skill from a file: {}",
                    match &result {
                        Ok(_) => "put on by a spoken yes".to_owned(),
                        Err(why) => format!("not on: {why}"),
                    }
                );
                uploads.keep(Outcome {
                    kind,
                    result,
                    request_id: None,
                    code: None,
                });
            });
        if spawned.is_err() {
            self.busy.store(false, Ordering::Release);
            return Err(VOICE_UNREACHED.into());
        }
        Ok(())
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

    /// Results not handed to the broker, or lost before its ack: first again.
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

    /// The fetch, the checks and the hand-over, blocking: what lamp-voice
    /// kept, and the journal's lines, none of which names the file's words
    /// or name.
    pub fn run(&self, id: &str) -> (Result<Value, String>, Vec<String>) {
        let mut journal = Vec::new();
        let result = self.kept(id, &mut journal);
        journal.push(match &result {
            Ok(_) => "a skill from a file: kept; the lamp asks about it aloud".to_owned(),
            Err(why) => format!("a skill from a file: not added: {why}"),
        });
        (result, journal)
    }

    fn kept(&self, id: &str, journal: &mut Vec<String>) -> Result<Value, String> {
        let data = fetch_message(self, id)?;
        let (bytes, format) = decoded(&data, id)?;
        journal.push(format!(
            "a skill from a file: {} bytes, its SHA-256 and id match",
            bytes.len()
        ));
        let (toml, card) = files_of(&bytes, &self.skills, &self.store_key)?;
        if (format == "skill_md") != toml.is_none() {
            return Err(NOT_A_SKILL.into());
        }
        // lamp-voice's own bound on each file, so the line it reads is whole.
        if card.len() > MOST_FILE_BYTES
            || toml
                .as_ref()
                .is_some_and(|toml| toml.len() > MOST_FILE_BYTES)
        {
            return Err(TOO_LONG.into());
        }
        if let Some(why) = held(&card, &self.skills, &self.pushed) {
            return Err(why);
        }
        let mut upload = json!({"skill_md": card});
        if let Some(toml) = toml {
            upload["skill_toml"] = json!(toml);
        }
        let reply = exchange(&self.socket, &json!({"upload": upload}))?;
        if let Some(why) = reply.get("refused").and_then(Value::as_str) {
            return Err(why.to_owned());
        }
        let kept = reply.get("kept").ok_or(VOICE_UNREACHED)?;
        let uses = &kept["uses"];
        let permissions = lamp_skills::Permissions {
            light: uses["light"] == true,
            body: uses["body"] == true,
            topics: uses["topics"]
                .as_array()
                .map(|topics| {
                    topics
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            ..Default::default()
        };
        Ok(json!({
            "upload_id": id,
            "added": "file",
            "uploaded": true,
            "name": kept["name"],
            "description": kept["description"],
            "version": kept["version"],
            "sha256": kept["sha256"],
            "may_use": crate::skills::may_use(&permissions),
        }))
    }

    /// What became of the skill `kept`, asked of lamp-voice every
    /// [`Self::settle_every`] for at most [`Self::settle_within`]: on by a
    /// spoken yes is `success` (with `"state": "on"`), left off, let go or
    /// never asked is `failure` with why.
    pub fn settle(&self, mut kept: Value) -> Result<Option<Value>, String> {
        let sha256 = kept["sha256"].as_str().unwrap_or_default().to_owned();
        let until = Instant::now() + self.settle_within;
        loop {
            let state = exchange(&self.socket, &json!({"added": {"sha256": sha256}}))
                .ok()
                .and_then(|told| told["state"].as_str().map(str::to_owned));
            match state.as_deref() {
                Some("on") => {
                    kept["state"] = json!("on");
                    return Ok(Some(kept));
                }
                Some("left_off") => return Err(LEFT_OFF.into()),
                Some("none") => return Err(LET_GO.into()),
                _ if Instant::now() >= until => return Err(NEVER_ASKED.into()),
                _ => std::thread::sleep(self.settle_every),
            }
        }
    }
}

/// One request on a runtime of its own, never longer than [`WITHIN`]: no
/// redirect followed, HTTPS only unless `https_only` is off (tests). The
/// status and at most `most` bytes of the body, or why not.
fn get(
    url: &str,
    headers: Vec<(&'static str, String)>,
    https_only: bool,
    most: u64,
) -> Result<(u16, Vec<u8>), String> {
    let url = url.to_owned();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| NO_MESSAGE.to_owned())?;
        runtime.block_on(async move {
            let client = reqwest::Client::builder()
                .tls_backend_preconfigured(crate::beat::tls()?)
                .https_only(https_only)
                .redirect(reqwest::redirect::Policy::none())
                .timeout(WITHIN)
                .build()
                .map_err(|_| NO_MESSAGE.to_owned())?;
            let mut request = client.get(&url);
            for (name, value) in headers {
                request = request.header(name, value);
            }
            let mut response = request.send().await.map_err(|_| NO_MESSAGE.to_owned())?;
            let status = response.status().as_u16();
            let mut read = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| NO_MESSAGE.to_owned())? {
                read.extend_from_slice(&chunk);
                if read.len() as u64 > most {
                    return Err(TOO_BIG.to_owned());
                }
            }
            Ok((status, read))
        })
    })
    .join()
    .unwrap_or_else(|_| Err(NO_MESSAGE.to_owned()))
}

/// `GET <base>/devices/get-message?kind=skills.upload.<id>` with the lamp's
/// key and id, as V1's privacy fetch and `connector.set`: `{"status": 1,
/// "data": {"kind", "data": {...}}}`, its inner `data` given back. At most
/// [`MOST_MESSAGE_BYTES`], for this kind only.
pub fn fetch_message(uploads: &Uploads, id: &str) -> Result<Value, String> {
    let kind = format!("{KIND_PREFIX}{id}");
    let url = format!("{}{FETCH_PATH}?kind={kind}", uploads.base);
    let mut headers = vec![
        ("authorization", format!("Bearer {}", uploads.key)),
        ("accept", "application/json".to_owned()),
    ];
    if !uploads.device_id.is_empty() {
        headers.push(("x-device-id", uploads.device_id.clone()));
    }
    let (status, body) =
        get(&url, headers, uploads.https_only, MOST_MESSAGE_BYTES).map_err(|why| {
            if why == TOO_BIG {
                why
            } else {
                NO_MESSAGE.to_owned()
            }
        })?;
    if status != 200 {
        return Err(NO_MESSAGE.into());
    }
    let mut answer: Value = serde_json::from_slice(&body).map_err(|_| NO_MESSAGE.to_owned())?;
    if answer["status"] != 1 {
        return Err(NO_MESSAGE.into());
    }
    let envelope = answer["data"].take();
    if envelope
        .get("kind")
        .and_then(Value::as_str)
        .is_some_and(|told| told != kind)
    {
        return Err(NO_MESSAGE.into());
    }
    match envelope.get("data") {
        Some(data @ Value::Object(_)) => Ok(data.clone()),
        _ => Err(NO_MESSAGE.into()),
    }
}

/// The file in `data` (`{upload_id, name, version, format, sha256, size,
/// content_base64}`), decoded, and its `format` (`package` or `skill_md`),
/// only when it is at most [`MOST_BYTES`] and its size, its SHA-256 and its
/// `upload_id` (the SHA-256's first 16 hex, the kind's `id`) are what was
/// said.
pub fn decoded(data: &Value, id: &str) -> Result<(Vec<u8>, String), String> {
    let format = match data["format"].as_str() {
        Some(format @ ("package" | "skill_md")) => format.to_owned(),
        _ => return Err(NOT_A_SKILL.into()),
    };
    let size = data["size"].as_u64().ok_or(NO_MESSAGE)?;
    if size > MOST_BYTES {
        return Err(TOO_BIG.into());
    }
    let content = data["content_base64"].as_str().ok_or(NO_MESSAGE)?;
    // Base64 is four characters for three bytes: anything longer than the
    // cap's is refused before it is decoded.
    if content.len() as u64 > MOST_BYTES.div_ceil(3) * 4 + 4 {
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
    if bytes.len() as u64 != size
        || actual != sha256
        || data["upload_id"].as_str() != Some(id)
        || !actual.starts_with(id)
    {
        return Err(MISMATCH.into());
    }
    Ok((bytes, format))
}

/// The file's `skill.toml` (none for a bare `SKILL.md`) and `SKILL.md`, or
/// why it can't be added: a package with the Store's signature, with code
/// or other files, or neither a package nor a `SKILL.md`.
pub fn files_of(
    bytes: &[u8],
    skills: &Path,
    store_key: &Path,
) -> Result<(Option<String>, String), String> {
    if bytes.starts_with(b"---\n") {
        let card = String::from_utf8(bytes.to_vec()).map_err(|_| NOT_A_SKILL.to_owned())?;
        return Ok((None, card));
    }
    let package = lamp_skills::package::read_package_bytes(bytes, None)
        .map_err(|_| NOT_A_SKILL.to_owned())?;
    if package.signature.is_some() {
        let keys = lamp_skills::package::store_keys(skills, store_key)
            .map_err(|_| UNCHECKED.to_owned())?;
        if lamp_skills::package::read_package_bytes(bytes, Some(&keys)).is_ok() {
            return Err(FROM_THE_STORE.into());
        }
    }
    if package.files.contains_key("run.py") {
        return Err(NO_CODE.into());
    }
    if package
        .files
        .keys()
        .any(|path| path != "skill.toml" && path != "SKILL.md")
    {
        return Err(OTHER_FILES.into());
    }
    let text = |path: &str| {
        package
            .files
            .get(path)
            .and_then(|bytes| String::from_utf8(bytes.clone()).ok())
            .ok_or_else(|| NOT_A_SKILL.to_owned())
    };
    Ok((Some(text("skill.toml")?), text("SKILL.md")?))
}

/// Why the skill `card` names can't be added here: a skill on the lamp by
/// that name, in `skills` (core, from the Store, made) or `pushed`.
pub fn held(card: &str, skills: &Path, pushed: &Path) -> Option<String> {
    let front = card.strip_prefix("---\n")?.split_once("\n---")?.0;
    let name = front
        .lines()
        .find_map(|line| line.strip_prefix("name:"))?
        .trim()
        .trim_matches('"');
    if !lamp_skills::package::skill_name_ok(name) {
        return None;
    }
    let there = |folder: &Path| std::fs::symlink_metadata(folder.join(name)).is_ok();
    (there(skills) || there(pushed)).then(|| {
        format!(
            "a skill called {name} is already on this lamp, and one added from a file never takes its place"
        )
    })
}

/// One line to lamp-voice, one line back, within [`VOICE_WAIT`].
fn exchange(socket: &Path, line: &Value) -> Result<Value, String> {
    use std::io::{BufRead, BufReader, Read, Write};
    let asked = || -> std::io::Result<String> {
        let mut stream = std::os::unix::net::UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(VOICE_WAIT))?;
        stream.set_write_timeout(Some(crate::make_skill::WAIT))?;
        stream.write_all(format!("{line}\n").as_bytes())?;
        let mut reply = String::new();
        BufReader::new(stream.take(8_192)).read_line(&mut reply)?;
        Ok(reply)
    };
    let reply = asked().map_err(|_| VOICE_UNREACHED.to_owned())?;
    serde_json::from_str(reply.trim()).map_err(|_| VOICE_UNREACHED.to_owned())
}

#[cfg(test)]
mod tests;
