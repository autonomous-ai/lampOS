//! A skill made on this lamp, sent to the Skills Store so other owners can
//! add it (fourteen-P13; the first builder on Queue #3997, 30 Oct 15:5x).
//! The owner's act from the app only, never by voice: `skills.share` packs
//! the made skill that is on (`lamp_skills::package::pack`, unsigned) and
//! uploads it to the Store's owner upload with the lamp's own key; a
//! reviewer reads it there before it is listed, and only approval signs it
//! with the Store's key. `skills.shared` reads back where each sent skill
//! is, and `skills.unshare` takes one back. Off unless `LAMP_SKILLS_SHARE=on`
//! with the skills page (`docs/cloud.md`, "A made skill sent to the Store").
//!
//! Only the skill's own files leave the lamp: its `skill.toml`, `SKILL.md`
//! and anything else in its folder that is not a dot file, with `made: here`
//! taken out of `SKILL.md` (on another lamp it is not made there). Nothing
//! else of the lamp is sent: no memory, faces, names, recordings or
//! settings. The Store's routes (Backend3, Queue comment 6516962994):
//! `POST /api/v1/skills/publish`, `GET /api/v1/skills/mine` and
//! `DELETE /api/v1/skills/:id`, above the backend's base as the ping's.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::account::Account;

/// `on` answers the three kinds, with the skills page (`LAMP_SKILLS_PAGE`).
pub const ENVIRONMENT: &str = "LAMP_SKILLS_SHARE";

/// The kinds this answers, in `can`'s order.
pub const KINDS: [&str; 3] = ["skills.share", "skills.shared", "skills.unshare"];

/// The longest `author`, in characters: a first name, never more.
pub const AUTHOR_MOST: usize = 32;

/// The longest any one exchange with the Store takes: a package is at most a
/// few megabytes, sent once.
pub const WITHIN: Duration = Duration::from_secs(30);

/// The longest a reason from the Store is passed on, in characters.
const REASON_MOST: usize = 300;

pub const NOT_MADE_HERE: &str = "only a skill made on this lamp can be shared";
pub const NOT_ON: &str = "turn the skill on before sharing it";
pub const BAD_AUTHOR: &str =
    "the name shown with it is a first name: at most 32 letters, on one line";
pub const BAD_ID: &str = "that is not a shared skill's id";
pub const UNKNOWN_LAMP: &str = "the Store didn't know this lamp";
pub const UNREACHED: &str = "the Skills Store can't be reached now";

/// Whether the switch is on.
pub fn on_from_environment() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// Where the Store is, and the lamp's key for it.
#[derive(Clone, PartialEq, Eq)]
pub struct Store {
    /// The scheme and host of the account's base, nothing after: the
    /// Store's routes sit at the host's root, not under the ping's base,
    /// which ends in `/api/v1/ai/v1` (Backend3 on #4570).
    pub base: String,
    pub key: String,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("base", &self.base)
            .field("key", &"<hidden>")
            .finish()
    }
}

impl Store {
    /// The account's base and the lamp's key; none without either.
    pub fn of(account: &Account, key: &str) -> Option<Self> {
        let base = reqwest::Url::parse(account.llm_base_url.trim())
            .ok()
            .filter(|url| url.has_host())?
            .origin()
            .ascii_serialization();
        let key = key.trim();
        (!key.is_empty()).then(|| Self {
            base,
            key: key.to_owned(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1/skills/{path}", self.base)
    }
}

/// `data`'s skill name, as `skills.remove` takes one.
fn name(data: &Value) -> Result<String, String> {
    data.get("name")
        .and_then(Value::as_str)
        .filter(|name| lamp_skills::package::skill_name_ok(name))
        .map(str::to_owned)
        .ok_or_else(|| crate::skills::NOT_A_NAME.to_owned())
}

/// `data`'s `author`, trimmed; none when it is missing or empty.
pub fn author(data: &Value) -> Result<Option<String>, String> {
    let Some(author) = data.get("author") else {
        return Ok(None);
    };
    let author = author.as_str().ok_or_else(|| BAD_AUTHOR.to_owned())?.trim();
    if author.is_empty() {
        return Ok(None);
    }
    if author.chars().count() > AUTHOR_MOST
        || author
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return Err(BAD_AUTHOR.to_owned());
    }
    Ok(Some(author.to_owned()))
}

/// The folder of `name` when it is a skill made on this lamp and on, as
/// `skills.list` shows it; else why not, in plain words.
pub fn made_here_and_on(places: &crate::skills::Places, name: &str) -> Result<PathBuf, String> {
    let page = crate::skills::list(places)?;
    let row = page["skills"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["name"] == name))
        .ok_or_else(|| crate::skills::NOT_A_NAME.to_owned())?;
    if row["group"] != "made_here" {
        return Err(NOT_MADE_HERE.to_owned());
    }
    if row["off"] != false {
        return Err(NOT_ON.to_owned());
    }
    Ok(places.folder.join(name))
}

/// `SKILL.md` with `made: here` taken out of its front matter.
fn not_made_here(card: &str) -> String {
    let Some(rest) = card.strip_prefix("---\n") else {
        return card.to_owned();
    };
    let Some((front, after)) = rest.split_once("\n---") else {
        return card.to_owned();
    };
    let kept: Vec<&str> = front
        .lines()
        .filter(|line| {
            line.split_once(':').is_none_or(|(key, value)| {
                key.trim() != "made" || value.trim() != crate::skills::MADE_HERE
            })
        })
        .collect();
    format!("---\n{}\n---{after}", kept.join("\n"))
}

/// Copies `from`'s plain files and folders, never a dot file, into `to`.
fn copy_skill(from: &Path, to: &Path) -> Result<(), String> {
    let failed = |error: std::io::Error| format!("the skill could not be read: {error}");
    fs::create_dir_all(to).map_err(failed)?;
    for entry in fs::read_dir(from).map_err(failed)? {
        let entry = entry.map_err(failed)?;
        let file_name = entry.file_name();
        if file_name.to_string_lossy().starts_with('.') {
            continue;
        }
        let kind = entry.file_type().map_err(failed)?;
        let there = to.join(&file_name);
        if kind.is_dir() {
            copy_skill(&entry.path(), &there)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &there).map_err(failed)?;
        } else {
            return Err("the skill holds something that is not a plain file".to_owned());
        }
    }
    Ok(())
}

/// The package of the skill in `folder`, as the Store takes it: its own
/// files, `made: here` taken out, packed unsigned in a fresh folder under
/// `scratch` that is removed after, whatever happens. Its file name and
/// bytes.
pub fn pack(folder: &Path, scratch: &Path) -> Result<(String, Vec<u8>), String> {
    let name = folder
        .file_name()
        .ok_or_else(|| crate::skills::NOT_A_NAME.to_owned())?;
    let work = scratch.join(format!("lamp-share-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    let packed = (|| {
        let copy = work.join("skill").join(name);
        copy_skill(folder, &copy)?;
        let card = copy.join("SKILL.md");
        if let Ok(text) = fs::read_to_string(&card) {
            fs::write(&card, not_made_here(&text))
                .map_err(|error| format!("the skill could not be read: {error}"))?;
        }
        let tar = lamp_skills::package::pack(&copy, &work.join("out"))?;
        let bytes = fs::read(&tar).map_err(|error| format!("the package: {error}"))?;
        let file_name = tar
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok((file_name, bytes))
    })();
    let _ = fs::remove_dir_all(&work);
    packed
}

/// A `multipart/form-data` body: `file` (the package), then `author` when
/// given. The boundary and the body.
pub fn multipart(file_name: &str, tar: &[u8], author: Option<&str>) -> (String, Vec<u8>) {
    let boundary = format!("lamp-share-{:016x}", seed(tar));
    let mut body = Vec::with_capacity(tar.len() + 512);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: application/x-tar\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(tar);
    body.extend_from_slice(b"\r\n");
    if let Some(author) = author {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"author\"\r\n\r\n{author}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (boundary, body)
}

/// A boundary that can't be in a package's bytes by chance: their hash.
fn seed(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// What a refusal from the Store says, never the key: `401` is the lamp
/// unknown to it; else its own reason, the most telling first
/// (`error_message`, then `error_code`, then the generic `message`), cut
/// short, or its status.
pub fn reason(status: u16, body: &[u8]) -> String {
    if status == 401 {
        return UNKNOWN_LAMP.to_owned();
    }
    let said = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            ["error_message", "error_code", "message", "error", "reason"]
                .iter()
                .find_map(|key| value.get(key).and_then(Value::as_str).map(str::to_owned))
                .or_else(|| {
                    value
                        .get("error")
                        .and_then(|error| error.get("message"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
        });
    match said
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
    {
        Some(text) => format!(
            "the Skills Store said: {}",
            text.chars().take(REASON_MOST).collect::<String>()
        ),
        None => format!("the Skills Store said no ({status})"),
    }
}

/// One shared skill as the app reads it: its id, name, version, the Store's
/// `review_status` and, when there is one, its `review_note`.
pub fn row(entry: &Value) -> Option<Value> {
    let text = |key: &str| {
        entry
            .get(key)
            .and_then(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| value.as_u64().map(|n| n.to_string()))
            })
            .filter(|text| !text.is_empty())
    };
    let mut row = json!({
        "id": text("id")?,
        "name": text("name")?,
        "version": text("version").unwrap_or_default(),
        "review_status": text("review_status")?,
    });
    if let Some(note) = text("review_note") {
        row["review_note"] = json!(note.chars().take(REASON_MOST).collect::<String>());
    }
    Some(row)
}

/// The rows in `GET /skills/mine`'s answer, once [`answered`] has taken
/// its `data`: the list under `data` (the Store's page, with `total`), or a
/// list itself.
pub fn rows(answer: &Value) -> Vec<Value> {
    let list = answer.as_array().or_else(|| {
        ["skills", "items", "data"]
            .iter()
            .find_map(|key| answer.get(key)?.as_array())
    });
    list.map(|list| list.iter().filter_map(row).collect())
        .unwrap_or_default()
}

/// An id as the Store gives one: 1 to 64 letters, digits, `-` or `_`.
fn id(data: &Value) -> Result<String, String> {
    data.get("id")
        .and_then(|id| {
            id.as_str()
                .map(str::to_owned)
                .or_else(|| id.as_u64().map(|n| n.to_string()))
        })
        .filter(|id| {
            (1..=64).contains(&id.len())
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        .ok_or_else(|| BAD_ID.to_owned())
}

/// One exchange with the Store: method, URL, an optional body with its
/// content type. Its status and body, or [`UNREACHED`].
type Exchange = (reqwest::Method, String, Option<(String, Vec<u8>)>);

/// Runs `exchange` on a thread of its own with its own runtime, so the
/// app's request waits on it as `skills.install` waits on the manage
/// helper, never longer than [`WITHIN`].
fn exchange(store: &Store, (method, url, body): Exchange) -> Result<(u16, Vec<u8>), String> {
    let key = store.key.clone();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| UNREACHED.to_owned())?;
        runtime.block_on(async move {
            let client = reqwest::Client::builder()
                .tls_backend_preconfigured(crate::beat::tls()?)
                .https_only(!cfg!(test))
                .redirect(reqwest::redirect::Policy::none())
                .timeout(WITHIN)
                .build()
                .map_err(|_| UNREACHED.to_owned())?;
            let mut request = client
                .request(method, &url)
                .header("authorization", format!("Bearer {key}"));
            if let Some((content_type, bytes)) = body {
                request = request.header("content-type", content_type).body(bytes);
            }
            let mut response = request.send().await.map_err(|_| UNREACHED.to_owned())?;
            let status = response.status().as_u16();
            let mut read = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| UNREACHED.to_owned())? {
                read.extend_from_slice(&chunk);
                if read.len() > 1_048_576 {
                    break;
                }
            }
            Ok((status, read))
        })
    })
    .join()
    .unwrap_or_else(|_| Err(UNREACHED.to_owned()))
}

/// The Store's answer inside its envelope, `{"status": 1, "data": ...}`;
/// a refusal, by its HTTP status or by `"status": 0`, as [`reason`] says it.
fn answered(status: u16, body: &[u8]) -> Result<Value, String> {
    if !(200..300).contains(&status) {
        return Err(reason(status, body));
    }
    let answer: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    if answer.get("status").and_then(Value::as_i64) == Some(0) {
        return Err(reason(status, body));
    }
    Ok(match answer {
        Value::Object(mut fields) if fields.contains_key("data") => {
            fields.remove("data").unwrap_or(Value::Null)
        }
        other => other,
    })
}

/// `skills.share {"name", "author"?}`: `{"sent": {"id", "name", "version",
/// "review_status"}}`, or why not.
pub fn share(
    store: &Store,
    places: &crate::skills::Places,
    data: &Value,
    scratch: &Path,
) -> Result<Value, String> {
    let name = name(data)?;
    let author = author(data)?;
    let folder = made_here_and_on(places, &name)?;
    let (file_name, tar) = pack(&folder, scratch)?;
    let (boundary, body) = multipart(&file_name, &tar, author.as_deref());
    let (status, reply) = exchange(
        store,
        (
            reqwest::Method::POST,
            store.url("publish"),
            Some((format!("multipart/form-data; boundary={boundary}"), body)),
        ),
    )?;
    let answer = answered(status, &reply)?;
    let sent = answer.get("skill").unwrap_or(&answer);
    let mut sent = row(sent).unwrap_or_else(
        || json!({"id": "", "name": name, "version": "", "review_status": "pending"}),
    );
    if sent["name"] != json!(name) {
        sent["name"] = json!(name);
    }
    Ok(json!({ "sent": sent }))
}

/// `skills.shared {}`: `{"shares": [...]}`, this lamp's sent skills.
pub fn shared(store: &Store) -> Result<Value, String> {
    let (status, reply) = exchange(store, (reqwest::Method::GET, store.url("mine"), None))?;
    Ok(json!({ "shares": rows(&answered(status, &reply)?) }))
}

/// `skills.unshare {"id"}`: the sent skill taken back from the Store.
pub fn unshare(store: &Store, data: &Value) -> Result<Value, String> {
    let id = id(data)?;
    let (status, reply) = exchange(store, (reqwest::Method::DELETE, store.url(&id), None))?;
    answered(status, &reply)?;
    Ok(json!({ "unshared": id }))
}

#[cfg(test)]
mod tests;
