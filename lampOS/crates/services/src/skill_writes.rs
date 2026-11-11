//! A made skill's write (#6066, the owner's lift of rule (b), 10 Nov): a
//! skill that starts itself may send a mail, add a calendar event or leave
//! a comment or an issue on a linked service with nobody asking at that
//! moment. Off unless `LAMP_SKILL_WRITES=on` ([`ENVIRONMENT`]), here and in
//! `lamp-skills`.
//!
//! - **Fixed in the file.** Every field of the write is the skill file's
//!   own: nothing is filled at run time. [`check`] holds a write to one of
//!   [`KINDS`] and its own keys, checked as the voice's write is
//!   (`writes::prepare`).
//! - **Approved by the owner's yes, not by install.** Putting a skill on
//!   approves nothing. The voice's yes path calls [`APPROVE`] on the tools
//!   socket (the voice's cgroup alone, never a function its model is
//!   offered), or the app through `lamp-cloud` on the admin socket, with
//!   the skill's name and each write's exact content; never `lamp-skills`.
//!   Who said yes is not checked (the owner, 10 Nov: whoever is at the lamp
//!   is its owner). This service keeps the
//!   SHA-256 of each ([`hash`] of [`canonical`]) with the link it was
//!   approved under ([`link_of`]: the account, or the link's own id, never
//!   a token) in its own store folder ([`APPROVED_FILE`], root's, 0700),
//!   which `lamp-skills` can't write.
//! - **Bound to the installed package.** Each approval also keeps the
//!   SHA-256 of the skill's installed package ([`installed_package`], its
//!   `.package.json` under `/var/lib/lampOS/skills`, root's), which holds
//!   its trigger, its manners and its write together: a skill updated,
//!   removed and put on again, or another package of its name, is waiting
//!   for a new yes, and its old approvals are dropped. Unlinking a service
//!   drops every approval for it ([`Approvals::revoke_service`]).
//! - **Public said at the yes.** A GitHub write's repository is read with
//!   the link's token at the yes (one it can't read counts as public) and
//!   the yes's answer names the public ones, for the voice to say "anyone
//!   can see it". One approved as private is read again at each send: made
//!   public since, or unreadable, it waits for a new yes.
//! - **Checked at the send.** `lamp-skills` sends the content and the
//!   skill's name ([`FUNCTION`]); it goes only if the content's hash is one
//!   approved for that skill and the service is still linked under the
//!   same link. No approval: "waiting for the owner's yes". Another
//!   content: refused, journalled by count. Unlinked or linked again as
//!   another account: refused as [`STOPPED`].
//! - **Bounds** ([`Ledger`]): at most [`PER_SKILL`] sends a skill and
//!   [`ALL_SKILLS`] across every skill in any 24 hours.
//! - **The owner's sent-log** ([`SentLog`]): the last [`SENT_MOST`] sends,
//!   each a skill, a service, a time and who it went to (an address's
//!   local part, never the whole address), never a text, for the voice to
//!   tell the owner ([`SENT`]).
//!
//! Every file is in the store folder, so a wipe (a sign-out, an unpairing,
//! a reset) deletes them with the tokens.
use crate::store::Entry;
use crate::writes;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

/// The switch, here and in `lamp-skills`: `on` only.
pub const ENVIRONMENT: &str = "LAMP_SKILL_WRITES";
/// The function `lamp-skills` calls on the tools socket.
pub const FUNCTION: &str = "skill_write";
/// The voice's call, on its owner's yes: approve a skill's writes.
pub const APPROVE: &str = "skill_write_approve";
/// The voice's call: the owner's sent-log.
pub const SENT: &str = "skill_writes_sent";
/// The voice's call, to read a waiting skill back: whether a skill's
/// writes are approved for its installed package under the link it has now
/// (`serve::Services::skill_writes_approved`). It approves nothing.
pub const APPROVED: &str = "skill_writes_approved";
/// In the store folder: each approved write's hash, skill and link.
pub const APPROVED_FILE: &str = "skill-writes-approved.json";
/// In the store folder: when each skill's write was sent, for the bounds.
pub const LEDGER_FILE: &str = "skill-writes.json";
/// In the store folder: the owner's sent-log.
pub const SENT_FILE: &str = "skill-writes-sent.json";
/// The most sends a skill makes in any 24 hours.
pub const PER_SKILL: usize = 3;
/// The most sends every skill together makes in any 24 hours.
pub const ALL_SKILLS: usize = 10;
/// The window the bounds count in, seconds.
pub const DAY_S: i64 = 86_400;
/// The most writes approved on the lamp, every skill's together.
pub const APPROVED_MOST: usize = 50;
/// The most writes one skill has approved.
pub const STEPS_MOST: usize = 4;
/// The most sends the owner's sent-log keeps.
pub const SENT_MOST: usize = 20;
/// The start of a refusal that stops the skill's writes: its link is gone
/// or another's.
pub const STOPPED: &str = "stopped: ";
/// What a write with no approval for its skill is told.
pub const WAITING: &str = "not sent: waiting for the owner's yes";
/// What a write whose content is not the approved one is told.
pub const NOT_APPROVED: &str = "not sent: this write is not the one the owner said yes to";

/// What a made skill may write: a fixed-shape write of the services crate,
/// with the keys it takes. No MCP tool call (its shape is the caller's), no
/// public post, no draft.
pub const KINDS: &[(&str, &str, &[&str])] = &[
    ("gmail", "send_mail", &["to", "cc", "subject", "text"]),
    ("gmail-app", "send_mail", &["to", "cc", "subject", "text"]),
    (
        "google_calendar",
        "create_event",
        &["title", "start", "end", "calendar", "attendees", "text"],
    ),
    ("github", "create_issue", &["repo", "title", "text"]),
    ("github", "comment", &["repo", "number", "text"]),
    ("linear", "create_issue", &["team_id", "title", "text"]),
    (
        "asana",
        "create_task",
        &["project", "workspace", "title", "text"],
    ),
    ("figma", "comment", &["id", "text"]),
    ("figma-api", "comment", &["id", "text"]),
];

/// Whether a made skill may write to `code` at all.
pub fn writable(code: &str) -> bool {
    KINDS.iter().any(|(service, _, _)| *service == code)
}

/// A write's content checked: an object with `service` and `action` one of
/// [`KINDS`], only that kind's keys, and every field as the service's write
/// takes it. Its refusals name keys, never a value, but for an address the
/// write's own check names.
pub fn check(write: &Value) -> Result<(), String> {
    let object = write
        .as_object()
        .ok_or("a write is a table of its fields")?;
    let text = |key: &str| object.get(key).and_then(Value::as_str).unwrap_or_default();
    let (service, action) = (text("service"), text("action"));
    let Some((_, _, keys)) = KINDS
        .iter()
        .find(|(code, kind, _)| *code == service && *kind == action)
    else {
        return Err(format!(
            "a skill writes only {}",
            KINDS
                .iter()
                .map(|(code, kind, _)| format!("{code} {kind}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    };
    if let Some(key) = object
        .keys()
        .find(|key| !["service", "action"].contains(&key.as_str()) && !keys.contains(&key.as_str()))
    {
        return Err(format!("{service} {action} takes no {key}"));
    }
    // The same checks as the voice's write, on a stand-in link: the
    // fields' shapes and bounds, nothing of the account.
    let stand_in = Entry {
        connector: service.to_owned(),
        auth_type: "pat".into(),
        api_key: "stand-in".into(),
        user_email: "stand-in@lamp.invalid".into(),
        ..Entry::default()
    };
    writes::prepare(service, write, &stand_in).map(|_| ())
}

/// `value` as one line of JSON, each object's keys sorted, so the same
/// content gives the same bytes in `lamp-skills` and here.
pub fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            let fields: Vec<String> = keys
                .into_iter()
                .map(|key| format!("{}:{}", Value::from(key.as_str()), canonical(&object[key])))
                .collect();
            format!("{{{}}}", fields.join(","))
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        other => other.to_string(),
    }
}

/// The SHA-256 of a write's [`canonical`] content, as hex.
pub fn hash(write: &Value) -> String {
    ring::digest::digest(&ring::digest::SHA256, canonical(write).as_bytes())
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The installed skills, root's: where each one's package hash is read.
pub const SKILLS_DIR: &str = "/var/lib/lampOS/skills";
/// In an installed skill's folder: its package's record (`lamp-skills`'
/// `package::INSTALLED_FROM`), written by the installer.
pub const PACKAGE_FILE: &str = ".package.json";
/// The most bytes of [`PACKAGE_FILE`] read.
const PACKAGE_FILE_MOST: u64 = 65_536;

/// The SHA-256 of `skill`'s installed package in `skills`, as its
/// installer recorded it; `None` for a skill put on with no package (a
/// folder by hand, a developer's push) or not on the lamp.
pub fn installed_package(skills: &Path, skill: &str) -> Option<String> {
    use std::io::Read;
    if !skill_name_ok(skill) {
        return None;
    }
    let path = skills.join(skill).join(PACKAGE_FILE);
    fs::symlink_metadata(&path)
        .ok()
        .filter(fs::Metadata::is_file)?;
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .ok()?
        .take(PACKAGE_FILE_MOST)
        .read_to_end(&mut bytes)
        .ok()?;
    let record: Value = serde_json::from_slice(&bytes).ok()?;
    record["sha256"]
        .as_str()
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(str::to_owned)
}

/// A GitHub write's repository, `owner/name`, when `write` is one.
pub fn github_repo(write: &Value) -> Option<&str> {
    (write["service"] == "github")
        .then(|| write["repo"].as_str())
        .flatten()
}

/// A step's content: `service` and `action` beside its fields.
pub fn content(service: &str, action: &str, fields: &Map<String, Value>) -> Value {
    let mut content = fields.clone();
    content.insert("service".into(), Value::from(service));
    content.insert("action".into(), Value::from(action));
    Value::Object(content)
}

/// Which link a write is approved under: the account it names, else the
/// link's own id; never a token. `None` for a link with neither (one kept
/// before the id), which is linked again before a skill may write to it.
pub fn link_of(entry: &Entry) -> Option<String> {
    let account = entry.account();
    if !account.is_empty() {
        return Some(format!("account:{account}"));
    }
    (!entry.link_id.is_empty()).then(|| format!("link:{}", entry.link_id))
}

/// Who a write goes to, for the owner's sent-log: a mail's first address
/// and how many more, an event's guests, else the place it goes. Never a
/// text, a subject or a title.
pub fn sent_to(write: &Value) -> String {
    let list = |key: &str| -> Vec<String> {
        match write.get(key) {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            Some(Value::String(text)) => text
                .split(',')
                .map(|one| one.trim().to_owned())
                .filter(|one| !one.is_empty())
                .collect(),
            _ => Vec::new(),
        }
    };
    // An address's local part alone: a write's addresses are plain, with
    // no display name to say instead.
    let local = |address: String| -> String {
        address
            .split_once('@')
            .map_or(address.clone(), |(local, _)| local.to_owned())
    };
    let people = |people: Vec<String>, none: &str| match people
        .into_iter()
        .map(local)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => none.to_owned(),
        [one] => one.clone(),
        [one, rest @ ..] => format!("{one} and {} more", rest.len()),
    };
    let text = |key: &str| write.get(key).and_then(Value::as_str).unwrap_or_default();
    match (text("service"), text("action")) {
        (_, "send_mail") => {
            let mut to = list("to");
            to.extend(list("cc"));
            people(to, "nobody")
        }
        ("google_calendar", _) => people(list("attendees"), "your calendar"),
        ("github", _) => text("repo").to_owned(),
        ("linear", _) => format!("team {}", text("team_id")),
        ("asana", _) => match text("project") {
            "" => format!("workspace {}", text("workspace")),
            project => format!("project {project}"),
        },
        ("figma" | "figma-api", _) => format!("file {}", text("id")),
        _ => String::new(),
    }
}

/// A skill's name as the skills folder takes it: 1 to 64 lowercase letters,
/// digits and hyphens.
pub fn skill_name_ok(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// One file of the store folder, read whole or empty.
fn read<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// One file of the store folder written whole: beside, 0600, synced, then
/// renamed over.
fn keep<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let folder = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(folder)?;
    let beside = folder.join(format!(".{name}.new"));
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&beside)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&beside, path)
}

/// One approved write.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Approval {
    pub skill: String,
    pub sha256: String,
    pub service: String,
    /// [`link_of`] when the owner said yes.
    pub link_of: String,
    /// The skill's installed package when the owner said yes
    /// ([`installed_package`]).
    pub package: String,
    /// Its destination was public at the yes (a GitHub repository anyone
    /// can see, said so): a private one made public since needs a new yes.
    #[serde(default)]
    pub public: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ApprovedFile {
    approved: Vec<Approval>,
}

/// What [`Approvals::find`] finds for a skill's write.
#[derive(Debug, PartialEq, Eq)]
pub enum Found {
    /// No write of this skill is approved.
    Waiting,
    /// Its writes are approved, and this content is none of them.
    Other,
    /// This content, approved.
    Approved(Approval),
}

/// The approved writes, in `<store>/`[`APPROVED_FILE`].
#[derive(Debug)]
pub struct Approvals {
    path: PathBuf,
}

impl Approvals {
    pub fn new(store: &Path) -> Self {
        Approvals {
            path: store.join(APPROVED_FILE),
        }
    }

    /// `skill`'s approvals replaced by `approved` (each a write's content
    /// the link it is approved under and whether its destination is
    /// public), for its installed `package`; none takes them away.
    pub fn approve(
        &self,
        skill: &str,
        package: &str,
        approved: &[(Value, String, bool)],
    ) -> Result<(), String> {
        if approved.len() > STEPS_MOST {
            return Err(format!("a skill has at most {STEPS_MOST} writes"));
        }
        let mut file: ApprovedFile = read(&self.path);
        file.approved.retain(|one| one.skill != skill);
        if file.approved.len() + approved.len() > APPROVED_MOST {
            return Err(format!(
                "the lamp keeps at most {APPROVED_MOST} approved writes"
            ));
        }
        file.approved
            .extend(approved.iter().map(|(write, link_of, public)| Approval {
                skill: skill.to_owned(),
                sha256: hash(write),
                service: write["service"].as_str().unwrap_or_default().to_owned(),
                link_of: link_of.clone(),
                package: package.to_owned(),
                public: *public,
            }));
        keep(&self.path, &file)
            .map_err(|error| format!("the approval could not be kept ({})", error.kind()))
    }

    /// What is approved for `skill`'s `write`, its installed package now
    /// `package`. Approvals for another package of the skill (updated,
    /// removed and put on again, or none on now) are dropped.
    pub fn find(&self, skill: &str, package: Option<&str>, write: &Value) -> Found {
        let mut file: ApprovedFile = read(&self.path);
        let stale = |one: &Approval| one.skill == skill && Some(one.package.as_str()) != package;
        if file.approved.iter().any(stale) {
            file.approved.retain(|one| !stale(one));
            if keep(&self.path, &file).is_err() {
                eprintln!("lamp-services: a changed skill's approvals were not dropped");
            }
        }
        let wanted = hash(write);
        let mut mine = file
            .approved
            .into_iter()
            .filter(|one| one.skill == skill && Some(one.package.as_str()) == package)
            .peekable();
        if mine.peek().is_none() {
            return Found::Waiting;
        }
        mine.find(|one| one.sha256 == wanted)
            .map_or(Found::Other, Found::Approved)
    }

    /// Every approval for a write to `code` dropped: the service unlinked.
    pub fn revoke_service(&self, code: &str) -> io::Result<()> {
        let mut file: ApprovedFile = read(&self.path);
        let before = file.approved.len();
        file.approved.retain(|one| one.service != code);
        if file.approved.len() == before {
            return Ok(());
        }
        keep(&self.path, &file)
    }
}

/// One send, as the ledger keeps it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Sent {
    skill: String,
    at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LedgerFile {
    sent: Vec<Sent>,
}

/// The sends of the last day, kept in `<store>/`[`LEDGER_FILE`] so a
/// restart doesn't forget them: a skill's name and a time each.
#[derive(Debug)]
pub struct Ledger {
    path: PathBuf,
}

impl Ledger {
    pub fn new(store: &Path) -> Self {
        Ledger {
            path: store.join(LEDGER_FILE),
        }
    }

    /// One send for `skill` at `now` taken, when both bounds allow it: the
    /// skill's sends and every skill's in the last day, after it. Kept
    /// before the send (the caller holds one lock across it), so two at
    /// once never pass a bound together; a send that then fails still
    /// counts. Refused when the ledger can't be kept.
    pub fn take(&self, skill: &str, now: i64) -> Result<(usize, usize), String> {
        // Unreadable, or a send after now (the clock went back): full,
        // never empty, until the clock passes it again.
        let full = || {
            Err(format!(
                "the day's sends can't be counted, so none goes (at most {ALL_SKILLS} a day)"
            ))
        };
        let mut sent = match fs::read(&self.path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(_) => return full(),
            Ok(bytes) => match serde_json::from_slice::<LedgerFile>(&bytes) {
                Ok(file) => file.sent,
                Err(_) => return full(),
            },
        };
        if sent.iter().any(|one| one.at > now) {
            return full();
        }
        sent.retain(|one| one.at > now - DAY_S);
        let mine = sent.iter().filter(|one| one.skill == skill).count();
        if mine >= PER_SKILL {
            return Err(format!(
                "a skill sends at most {PER_SKILL} times a day, and this one has"
            ));
        }
        if sent.len() >= ALL_SKILLS {
            return Err(format!(
                "the lamp's skills send at most {ALL_SKILLS} times a day together, and they have"
            ));
        }
        sent.push(Sent {
            skill: skill.to_owned(),
            at: now,
        });
        let all = sent.len();
        keep(&self.path, &LedgerFile { sent })
            .map_err(|error| format!("the day's sends could not be kept ({})", error.kind()))?;
        Ok((mine + 1, all))
    }
}

/// One send in the owner's sent-log.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Told {
    pub skill: String,
    pub service: String,
    /// Unix seconds.
    pub at: i64,
    /// [`sent_to`].
    pub to: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SentFile {
    sent: Vec<Told>,
}

/// The owner's sent-log, `<store>/`[`SENT_FILE`]: the last [`SENT_MOST`]
/// sends, oldest first, never a text.
#[derive(Debug)]
pub struct SentLog {
    path: PathBuf,
}

impl SentLog {
    pub fn new(store: &Path) -> Self {
        SentLog {
            path: store.join(SENT_FILE),
        }
    }

    pub fn add(&self, told: Told) -> io::Result<()> {
        let mut file: SentFile = read(&self.path);
        file.sent.push(told);
        let over = file.sent.len().saturating_sub(SENT_MOST);
        file.sent.drain(..over);
        keep(&self.path, &file)
    }

    /// The log, for the voice: `{"sent": [{skill, service, at, to}]}`.
    pub fn list(&self) -> Value {
        json!({"sent": read::<SentFile>(&self.path).sent})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail() -> Value {
        json!({"service":"gmail","action":"send_mail","to":["boss@work.test"],"subject":"In","text":"I'm at my desk."})
    }

    #[test]
    fn the_same_content_hashes_alike_whatever_its_order() {
        let other: Value = serde_json::from_str(
            r#"{"text":"I'm at my desk.","subject":"In","to":["boss@work.test"],"action":"send_mail","service":"gmail"}"#,
        )
        .unwrap();
        assert_eq!(hash(&mail()), hash(&other));
        let mut changed = mail();
        changed["text"] = json!("I'm at my desk!");
        assert_ne!(hash(&mail()), hash(&changed));
        assert_eq!(hash(&mail()).len(), 64);
    }

    #[test]
    fn a_skill_writes_only_its_kinds_and_their_keys() {
        assert_eq!(check(&mail()), Ok(()));
        let event = json!({"service":"google_calendar","action":"create_event","title":"Focus","start":"2026-11-11T09:00:00+07:00","end":"2026-11-11T10:00:00+07:00"});
        assert_eq!(check(&event), Ok(()));
        for (bad, why) in [
            (
                json!({"service":"facebook","action":"page_post","text":"hi"}),
                "a skill writes only",
            ),
            (
                json!({"service":"gmail","action":"save_draft","to":["a@b.test"],"subject":"s","text":"t"}),
                "a skill writes only",
            ),
            (
                json!({"service":"slack","action":"call","tool":"post","arguments":{}}),
                "a skill writes only",
            ),
            (
                json!({"service":"gmail","action":"send_mail","to":["a@b.test"],"subject":"s","text":"t","send_id":"x"}),
                "takes no send_id",
            ),
            (
                json!({"service":"gmail","action":"send_mail","to":["not an address"],"subject":"s","text":"t"}),
                "is not an email address",
            ),
            (
                json!({"service":"gmail","action":"send_mail","to":[],"subject":"s","text":"t"}),
                "at least one address",
            ),
        ] {
            let refused = check(&bad).unwrap_err();
            assert!(refused.contains(why), "{bad}: {refused}");
        }
    }

    const PACKAGE: &str = "aa";

    #[test]
    fn approvals_are_by_skill_package_content_and_link() {
        let dir = tempfile::tempdir().unwrap();
        let approvals = Approvals::new(dir.path());
        let find = |skill: &str, write: &Value| approvals.find(skill, Some(PACKAGE), write);
        assert_eq!(find("arrive-mail", &mail()), Found::Waiting);
        approvals
            .approve(
                "arrive-mail",
                PACKAGE,
                &[(mail(), "account:me@home.test".into(), false)],
            )
            .unwrap();
        let Found::Approved(approval) = find("arrive-mail", &mail()) else {
            panic!("not approved");
        };
        assert_eq!(approval.link_of, "account:me@home.test");
        let mut changed = mail();
        changed["to"] = json!(["someone@else.test"]);
        assert_eq!(find("arrive-mail", &changed), Found::Other);
        assert_eq!(find("other", &mail()), Found::Waiting);
        // Approved again: replaced, never added to.
        approvals.approve("arrive-mail", PACKAGE, &[]).unwrap();
        assert_eq!(find("arrive-mail", &mail()), Found::Waiting);
        let kept = fs::read_to_string(dir.path().join(APPROVED_FILE)).unwrap();
        assert!(!kept.contains("desk"), "{kept}");
        // Another package of the skill: waiting, and the old yes dropped.
        approvals
            .approve(
                "arrive-mail",
                PACKAGE,
                &[(mail(), "account:a".into(), false)],
            )
            .unwrap();
        assert_eq!(
            approvals.find("arrive-mail", Some("bb"), &mail()),
            Found::Waiting
        );
        assert_eq!(find("arrive-mail", &mail()), Found::Waiting);
        approvals
            .approve(
                "arrive-mail",
                PACKAGE,
                &[(mail(), "account:a".into(), false)],
            )
            .unwrap();
        assert_eq!(approvals.find("arrive-mail", None, &mail()), Found::Waiting);
        // Unlinking the service drops its approvals.
        approvals
            .approve(
                "arrive-mail",
                PACKAGE,
                &[(mail(), "account:a".into(), false)],
            )
            .unwrap();
        approvals.revoke_service("gmail").unwrap();
        assert_eq!(find("arrive-mail", &mail()), Found::Waiting);
    }

    #[test]
    fn the_installed_package_is_read_from_its_record() {
        let dir = tempfile::tempdir().unwrap();
        let hash = "ab".repeat(32);
        fs::create_dir_all(dir.path().join("arrive-mail")).unwrap();
        fs::write(
            dir.path().join("arrive-mail").join(PACKAGE_FILE),
            json!({"sha256": hash, "manifest": {}}).to_string(),
        )
        .unwrap();
        assert_eq!(installed_package(dir.path(), "arrive-mail"), Some(hash));
        assert_eq!(installed_package(dir.path(), "other"), None);
        assert_eq!(installed_package(dir.path(), "../arrive-mail"), None);
    }

    /// An unreadable ledger, or one with a send after now (the clock went
    /// back), counts as full.
    #[test]
    fn the_ledger_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = Ledger::new(dir.path());
        ledger.take("a", 5_000).unwrap();
        assert!(
            ledger
                .take("b", 4_000)
                .unwrap_err()
                .contains("can't be counted")
        );
        assert!(ledger.take("b", 5_000).is_ok());
        fs::write(dir.path().join(LEDGER_FILE), "not json").unwrap();
        assert!(
            ledger
                .take("b", 6_000)
                .unwrap_err()
                .contains("can't be counted")
        );
    }

    #[test]
    fn the_ledger_holds_each_skill_and_every_skill_to_its_bound() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = Ledger::new(dir.path());
        for n in 1..=PER_SKILL {
            assert_eq!(ledger.take("a", 1_000).unwrap().0, n);
        }
        assert!(ledger.take("a", 1_000).unwrap_err().contains("at most 3"));
        // A day later it may again.
        assert!(ledger.take("a", 1_000 + DAY_S).is_ok());
        for skill in ["b", "c", "d"] {
            for _ in 0..PER_SKILL {
                ledger.take(skill, 2_000 + DAY_S).unwrap();
            }
        }
        // 1 + 9 = 10 in the window: every skill's bound.
        assert!(
            ledger
                .take("e", 2_000 + DAY_S)
                .unwrap_err()
                .contains("at most 10")
        );
    }

    #[test]
    fn the_sent_log_keeps_the_last_twenty_and_no_text() {
        let dir = tempfile::tempdir().unwrap();
        let log = SentLog::new(dir.path());
        for at in 0..25 {
            log.add(Told {
                skill: "arrive-mail".into(),
                service: "gmail".into(),
                at,
                to: sent_to(&mail()),
            })
            .unwrap();
        }
        let list = log.list();
        let sent = list["sent"].as_array().unwrap();
        assert_eq!(sent.len(), SENT_MOST);
        assert_eq!(sent[0]["at"], 5);
        assert_eq!(sent[0]["to"], "boss");
        assert!(!list.to_string().contains("desk"));
        let event = json!({"service":"google_calendar","action":"create_event","title":"Secret","attendees":["a@b.test","c@d.test"]});
        assert_eq!(sent_to(&event), "a and 1 more");
        assert!(!list.to_string().contains("work.test"));
    }
}
