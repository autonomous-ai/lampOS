//! An uploaded skill's secrets (an API key, a bot token), given in the app
//! (`LAMP_SKILL_SECRETS`, off unless `on`, set in no unit; the first
//! builder's ruling, 12 Nov 19:44 on #6059: "never said aloud and never
//! kept in the skill folder. They go in through the app, into root's store
//! like service tokens, and reach the run only as an environment file read
//! once. The agent never asks for one by voice.").
//!
//! - **Names in the skill, values here.** A skill names the variables its
//!   run needs in its `skill.toml`, `secrets = ["GEMINI_API_KEY"]`
//!   ([`needed`]); the voice's upload finds them in the file ([`names_in`]).
//!   Each value is kept by this one root process beside the services'
//!   tokens, in `<store>/skill-secrets/<skill>.json` ([`DIR_NAME`]), 0600 in
//!   a 0700 folder, written whole (beside, synced, renamed).
//! - **The app's door** (the admin socket, `lamp-cloud.service` alone):
//!   [`LIST_OP`], [`SET_OP`], [`UNSET_OP`] (names only back, never a value)
//!   and [`DROP_OP`] when the skill is removed. A wipe (a sign-out, an
//!   unpairing, a reset) takes the folder with the rest of the store.
//! - **The run's door** (the tools socket, `lamp-tasks.service` alone,
//!   [`RUN_ASKER_CGROUP`]): [`RUN_OP`], the set values of the names a skill
//!   needs and the names missing, for the run's environment file.
//! - **Never a value in a log**: a journal line names a skill, a name and a
//!   value's length; [`Value`]'s `Debug` hides it, and no refusal repeats it.
//! - **Gone with its skill**: a skill's file is dropped when the app removes
//!   it, and at each ask whose skill is no longer on the lamp, or on again
//!   as another package ([`Secrets::sweep`]).
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json, json};
use std::{
    collections::BTreeMap,
    fmt, fs,
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};

/// The switch, in lamp-services, lamp-cloud, lamp-voice, lamp-skills and
/// lamp-tasks: `on` only.
pub const ENVIRONMENT: &str = "LAMP_SKILL_SECRETS";
/// The secrets' folder in the store.
pub const DIR_NAME: &str = "skill-secrets";
/// The most bytes of one value.
pub const MOST_VALUE_BYTES: usize = 4_096;
/// The most names one skill may need.
pub const MOST_NAMES: usize = 16;
/// The most bytes of a skill's `skill.toml` read for its names.
const MOST_TOML_BYTES: u64 = 64 * 1_024;
/// The most bytes of one skill's file read back.
const MOST_FILE_BYTES: u64 = 256 * 1_024;

/// What a skill needs and what is set (`{"op","skill"}`).
pub const LIST_OP: &str = "skill_secrets";
/// One value kept (`{"op","skill","name","value"}`).
pub const SET_OP: &str = "skill_secret_set";
/// One value dropped (`{"op","skill","name"}`).
pub const UNSET_OP: &str = "skill_secret_unset";
/// Every value of a skill dropped: it was removed (`{"op","skill"}`).
pub const DROP_OP: &str = "skill_secrets_drop";
/// The run's ask on the tools socket (`{"op","skill"}`): lamp-tasks alone.
pub const RUN_OP: &str = "run_secrets";
/// The voice's own call on the tools socket, never offered to its model
/// (`{"op":"call","name","args":{"skill"}}`): what an agent skill needs and
/// has, names only, before its tool runs.
pub const NAMES_CALL: &str = "skill_secrets_named";
/// Where the system's manager puts `lamp-tasks.service`
/// (`Slice=lampos-parts.slice`): the run door's one asker. A skill's run
/// (Hermes) stays in that cgroup, but its namespace shows it no `/run`, so
/// it never reaches this socket.
pub const RUN_ASKER_CGROUP: &str = "/lampos.slice/lampos-parts.slice/lamp-tasks.service";

/// A request with a field more or less than its op takes.
pub const BAD_FIELDS: &str = "the request has fields it doesn't take";
/// A skill's name that is not one.
pub const BAD_SKILL: &str =
    "skill must be a skill's name: 1 to 64 lowercase letters, digits and hyphens";
/// No skill of that name on the lamp.
pub const NO_SKILL: &str = "no skill of that name is on this lamp";
/// A name the skill doesn't need.
pub const NOT_NEEDED: &str = "that skill doesn't need a key of that name";
/// A value out of its bounds.
pub const BAD_VALUE: &str = "the value must be 1 to 4096 bytes on one line";
/// The store not written.
pub const NOT_KEPT: &str = "the key could not be kept";
/// The store not changed on an unset or a drop.
pub const NOT_REMOVED: &str = "the key could not be removed";
/// The switch off in lamp-services.
pub const OFF: &str = "not on this lamp: a skill's keys (LAMP_SKILL_SECRETS)";

/// Whether the switch's value is on: `on` exactly.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The parts of a name that say it is a secret, between its `_`s.
const SECRET_PARTS: [&str; 12] = [
    "KEY",
    "KEYS",
    "TOKEN",
    "TOKENS",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "PASS",
    "PAT",
    "CREDENTIAL",
    "CREDENTIALS",
    "AUTH",
];
/// Names a run's environment never takes from a skill: the shell's, the
/// system's and the loader's own, and every one a runtime reads to change
/// what it runs or trusts.
const RESERVED: [&str; 52] = [
    "PATH",
    "HOME",
    "USER",
    "SHELL",
    "PWD",
    "OLDPWD",
    "LANG",
    "LANGUAGE",
    "TERM",
    "IFS",
    "ENV",
    "TMPDIR",
    "TMP",
    "TEMP",
    "HOSTNAME",
    "LOGNAME",
    "MAIL",
    "CDPATH",
    "PS1",
    "PS2",
    "PS4",
    "UID",
    "EUID",
    "PPID",
    "SHELLOPTS",
    "BASHOPTS",
    "PROMPT_COMMAND",
    "HISTFILE",
    "EDITOR",
    "VISUAL",
    "PAGER",
    "BROWSER",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "PERL5LIB",
    "PERL5OPT",
    "PERLLIB",
    "RUBYOPT",
    "RUBYLIB",
    "CLASSPATH",
    "JAVA_TOOL_OPTIONS",
    "OPENSSL_CONF",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "CURL_CA_BUNDLE",
    "REQUESTS_CA_BUNDLE",
    "GCONV_PATH",
    "GLIBC_TUNABLES",
    "LOCPATH",
    "NLSPATH",
    "HOSTALIASES",
    "RES_OPTIONS",
];
/// The beginnings of names reserved the same way.
const RESERVED_HEADS: [&str; 16] = [
    "LAMP_",
    "HERMES_",
    "LD_",
    "PYTHON",
    "XDG_",
    "LC_",
    "SSH_",
    "BASH_",
    "SUDO_",
    "SYSTEMD_",
    "DBUS_",
    "GIT_",
    "NODE_",
    "NPM_CONFIG_",
    "MALLOC_",
    "DYLD_",
];

/// Whether `name` is one a run's environment takes from a skill.
pub fn reserved(name: &str) -> bool {
    RESERVED.contains(&name)
        || RESERVED_HEADS.iter().any(|head| name.starts_with(head))
        || name.ends_with("_PROXY")
        || name == "TZ"
}

/// Whether `name` is a variable's name a skill may need:
/// `^[A-Z][A-Z0-9_]{2,63}$`, and not [`reserved`].
pub fn name_ok(name: &str) -> bool {
    (3..=64).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_uppercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && !reserved(name)
}

/// Whether a value is one kept: 1 to [`MOST_VALUE_BYTES`] bytes, no
/// control character (no NUL, no line break), so it is one line of the
/// run's environment file.
pub fn value_ok(value: &str) -> bool {
    (1..=MOST_VALUE_BYTES).contains(&value.len()) && !value.chars().any(char::is_control)
}

/// Whether one of `name`'s parts says it is a secret ([`SECRET_PARTS`], or
/// `API`).
fn secret_named(name: &str) -> bool {
    name.split('_')
        .any(|part| part == "API" || part == "APIKEY" || SECRET_PARTS.contains(&part))
}

/// The words on a line that say its names are variables a run is given.
const LINE_WORDS: [&str; 10] = [
    "env",
    "environment",
    "variable",
    "variables",
    "export",
    "set",
    "secret",
    "key",
    "token",
    "token:",
];

/// How a variable is read in code, just before its name: `$`, `${`,
/// `os.environ["`, `os.environ.get("`, `getenv("`, `process.env.`,
/// `process.env["`, `ENV["`, lowercased and with the quotes and brackets
/// between left out.
const READ_FORMS: [&str; 6] = [
    "environ",
    "environ.get",
    "getenv",
    "process.env",
    "env",
    "env.get",
];

/// The variables' names `text` says its run needs, in their first order,
/// once each, at most [`MOST_NAMES`] (one more says there are too many,
/// for the caller to refuse): each [`name_ok`], and either read as a
/// variable in code (`os.environ["X"]`, `getenv("X")`, `process.env.X`),
/// named one in words ("the environment variable X", "env var X",
/// "export X", a line `X=...`), or a secret's name with a `_` in it
/// (`…_KEY`, `…_TOKEN`, `…_SECRET`, an `API`, …) written `$X` or on a
/// line that says env, set, export, key, token or secret. Never a value.
pub fn names_in(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in text.lines() {
        let lower = line.to_lowercase();
        let line_says = lower
            .split(|c: char| !c.is_ascii_alphanumeric() && c != ':')
            .any(|word| LINE_WORDS.contains(&word))
            || lower.contains("env var")
            || lower.contains("environment var");
        let bytes = line.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            let starts = bytes[at].is_ascii_uppercase()
                && (at == 0 || !(bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_'));
            if !starts {
                at += 1;
                continue;
            }
            let mut end = at;
            while end < bytes.len()
                && (bytes[end].is_ascii_uppercase()
                    || bytes[end].is_ascii_digit()
                    || bytes[end] == b'_')
            {
                end += 1;
            }
            // A run of letters that goes on in lowercase is a word, not a name.
            if end < bytes.len() && bytes[end].is_ascii_lowercase() {
                at = end;
                continue;
            }
            let name = &line[at..end];
            let before = &line[..at];
            let dollar = before.ends_with('$') || before.ends_with("${");
            let read = {
                let trimmed = before
                    .to_lowercase()
                    .trim_end_matches(['"', '\'', '[', '(', '`', ' ', '.'])
                    .to_owned();
                READ_FORMS.iter().any(|form| {
                    trimmed.ends_with(form)
                        && !trimmed[..trimmed.len() - form.len()]
                            .ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
                })
            };
            let named = {
                let words = before
                    .to_lowercase()
                    .trim_end_matches(['"', '\'', '`', ' ', ':', '*'])
                    .to_owned();
                [
                    "environment variable",
                    "environment variables",
                    "env var",
                    "env vars",
                    "export",
                ]
                .iter()
                .any(|form| words.ends_with(form))
            };
            let assigned = line[end..].starts_with('=') && {
                let head = before.trim();
                head.is_empty() || head == "export"
            };
            // A secret's word alone in prose ("the API key", "a TOKEN") is
            // no name: one read this way has a `_` in it.
            let counts = read
                || named
                || assigned
                || (name.contains('_') && secret_named(name) && (dollar || line_says));
            if counts && name_ok(name) && !found.iter().any(|kept| kept == name) {
                found.push(name.to_owned());
                if found.len() > MOST_NAMES {
                    return found;
                }
            }
            at = end;
        }
    }
    found
}

/// The names the skill `skill` in `skills` needs: its `skill.toml`'s
/// `secrets`, each [`name_ok`], once, in its order; none when it names
/// none. [`NO_SKILL`] when no folder of that name is there (a link is
/// none).
pub fn needed(skills: &Path, skill: &str) -> Result<Vec<String>, &'static str> {
    if !crate::skill_writes::skill_name_ok(skill) {
        return Err(BAD_SKILL);
    }
    let folder = skills.join(skill);
    if !fs::symlink_metadata(&folder).is_ok_and(|meta| meta.is_dir()) {
        return Err(NO_SKILL);
    }
    let path = folder.join("skill.toml");
    if !fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file()) {
        return Ok(Vec::new());
    }
    let mut text = String::new();
    fs::File::open(&path)
        .and_then(|file| file.take(MOST_TOML_BYTES).read_to_string(&mut text))
        .map_err(|_| NO_SKILL)?;
    Ok(needed_of_toml(&text))
}

/// The `secrets` a `skill.toml`'s text names, as [`needed`] keeps them.
pub fn needed_of_toml(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let table: toml::Table = toml::from_str(text).unwrap_or_default();
    for name in table
        .get("secrets")
        .and_then(toml::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        if let Some(name) = name.as_str().filter(|name| name_ok(name))
            && !names.iter().any(|kept| kept == name)
            && names.len() < MOST_NAMES
        {
            names.push(name.to_owned());
        }
    }
    names
}

/// A kept value: never shown by `Debug`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Value(String);

impl Value {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<hidden, {} bytes>", self.0.len())
    }
}

/// One skill's file.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct File {
    skill: String,
    /// Its installed package's SHA-256 when the first value was kept
    /// (`skill_writes::installed_package`): the same name on again as
    /// another package keeps none of these.
    package: Option<String>,
    values: BTreeMap<String, Value>,
}

/// One request of the app's door, its value hidden from `Debug`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    List(String),
    Set {
        skill: String,
        name: String,
        value: Value,
    },
    Unset {
        skill: String,
        name: String,
    },
    Drop(String),
}

impl Ask {
    /// The admin line's object for `op`, checked whole; the refusal's
    /// words otherwise, which never repeat the line.
    pub fn parse(op: &str, object: &Map<String, Json>) -> Result<Self, &'static str> {
        let fields: &[&str] = match op {
            LIST_OP | DROP_OP => &["op", "skill"],
            SET_OP => &["op", "skill", "name", "value"],
            UNSET_OP => &["op", "skill", "name"],
            _ => return Err(BAD_FIELDS),
        };
        if object.len() != fields.len() || object.keys().any(|key| !fields.contains(&key.as_str()))
        {
            return Err(BAD_FIELDS);
        }
        let skill = object
            .get("skill")
            .and_then(Json::as_str)
            .filter(|skill| crate::skill_writes::skill_name_ok(skill))
            .ok_or(BAD_SKILL)?
            .to_owned();
        let name = || {
            object
                .get("name")
                .and_then(Json::as_str)
                .filter(|name| name_ok(name))
                .map(str::to_owned)
                .ok_or(NOT_NEEDED)
        };
        Ok(match op {
            LIST_OP => Self::List(skill),
            DROP_OP => Self::Drop(skill),
            UNSET_OP => Self::Unset {
                skill,
                name: name()?,
            },
            _ => {
                let name = name()?;
                let value = object
                    .get("value")
                    .and_then(Json::as_str)
                    .filter(|value| value_ok(value))
                    .ok_or(BAD_VALUE)?;
                Self::Set {
                    skill,
                    name,
                    value: Value::new(value.to_owned()),
                }
            }
        })
    }
}

/// The store's secrets: its folder and the installed skills.
pub struct Secrets {
    /// `<store>/skill-secrets`.
    pub dir: PathBuf,
    /// The installed skills, where each one's needs are read.
    pub skills: PathBuf,
    /// Held from a file's read to its rename.
    write: Mutex<()>,
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secrets")
            .field("dir", &self.dir)
            .field("skills", &self.skills)
            .finish_non_exhaustive()
    }
}

impl Secrets {
    /// In the store `store`, for the skills in `skills`.
    pub fn new(store: &Path, skills: &Path) -> Self {
        Self {
            dir: store.join(DIR_NAME),
            skills: skills.to_path_buf(),
            write: Mutex::new(()),
        }
    }

    fn path(&self, skill: &str) -> PathBuf {
        self.dir.join(format!("{skill}.json"))
    }

    fn folder(&self) -> io::Result<()> {
        if let Some(store) = self.dir.parent() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(store)?;
        }
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)?;
        fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))
    }

    /// `skill`'s file, or an empty one: a regular file, never a link, of
    /// at most [`MOST_FILE_BYTES`].
    fn load(&self, skill: &str) -> File {
        let path = self.path(skill);
        let plain = fs::symlink_metadata(&path)
            .is_ok_and(|meta| meta.is_file() && meta.len() <= MOST_FILE_BYTES);
        if !plain {
            return File::default();
        }
        let mut bytes = Vec::new();
        let read = fs::OpenOptions::new()
            .read(true)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(&path)
            .and_then(|file| file.take(MOST_FILE_BYTES).read_to_end(&mut bytes));
        match read {
            Ok(_) => serde_json::from_slice::<File>(&bytes)
                .ok()
                .filter(|file| file.skill == skill)
                .unwrap_or_default(),
            Err(_) => File::default(),
        }
    }

    /// `file` kept whole for `skill`: beside, 0600, synced, renamed over;
    /// an empty one removes it.
    fn keep(&self, skill: &str, file: &File) -> io::Result<()> {
        if file.values.is_empty() {
            return self.remove(skill);
        }
        self.folder()?;
        let beside = self.dir.join(format!(".{skill}.json.new"));
        let text = serde_json::to_vec(file).map_err(io::Error::other)?;
        let _ = fs::remove_file(&beside);
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&beside)?;
        out.set_permissions(fs::Permissions::from_mode(0o600))?;
        out.write_all(&text)?;
        out.sync_all()?;
        drop(out);
        fs::rename(&beside, self.path(skill))?;
        fs::File::open(&self.dir)?.sync_all()
    }

    fn remove(&self, skill: &str) -> io::Result<()> {
        match fs::remove_file(self.path(skill)) {
            Ok(()) => fs::File::open(&self.dir)?.sync_all(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// `skill`'s file as it stands for the skill on the lamp now: kept only
    /// while that skill is the package it was kept for; a file of a skill
    /// gone, or on again as another package, is removed.
    fn current(&self, skill: &str) -> File {
        let file = self.load(skill);
        if file.values.is_empty() {
            return file;
        }
        let gone = !fs::symlink_metadata(self.skills.join(skill)).is_ok_and(|meta| meta.is_dir());
        let other = file.package.is_some()
            && file.package != crate::skill_writes::installed_package(&self.skills, skill);
        if gone || other {
            if self.remove(skill).is_err() {
                eprintln!("lamp-services: skill {skill}'s keys could not be dropped");
            } else {
                eprintln!(
                    "lamp-services: skill {skill}'s keys dropped: {}",
                    if gone {
                        "it is no longer on the lamp"
                    } else {
                        "it is on again as another package"
                    }
                );
            }
            return File::default();
        }
        file
    }

    /// Every skill's file whose skill is gone, or on as another package,
    /// removed ([`Self::current`]); at start, and at each app ask.
    pub fn sweep(&self) {
        let _held = self
            .write
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(skill) = name.to_str().and_then(|name| name.strip_suffix(".json")) else {
                continue;
            };
            if crate::skill_writes::skill_name_ok(skill) {
                self.current(skill);
            }
        }
    }

    /// What `skill` needs and which of those are set, names only.
    fn view(&self, skill: &str, needed: &[String], file: &File) -> Json {
        let set: Vec<&String> = needed
            .iter()
            .filter(|name| file.values.contains_key(*name))
            .collect();
        json!({"skill": skill, "needed": needed, "set": set})
    }

    /// The app's ask answered: the view after it, or the refusal's words;
    /// with the journal's line, names and lengths only.
    pub fn answer(&self, ask: &Ask) -> (Result<Json, &'static str>, String) {
        self.sweep();
        let _held = self
            .write
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        match ask {
            Ask::Drop(skill) => match self.remove(skill) {
                Ok(()) => (
                    Ok(json!({"skill": skill})),
                    format!("skill {skill}'s keys dropped: it was removed"),
                ),
                Err(error) => (
                    Err(NOT_REMOVED),
                    format!("skill {skill}'s keys not dropped: {}", error.kind()),
                ),
            },
            Ask::List(skill) => match needed(&self.skills, skill) {
                Ok(needed) => {
                    let file = self.current(skill);
                    (
                        Ok(self.view(skill, &needed, &file)),
                        format!("skill {skill}'s keys listed"),
                    )
                }
                Err(why) => (Err(why), format!("skill keys not listed: {why}")),
            },
            Ask::Set { skill, name, value } => {
                let needed = match needed(&self.skills, skill) {
                    Ok(needed) => needed,
                    Err(why) => return (Err(why), format!("a skill key not kept: {why}")),
                };
                if !needed.contains(name) {
                    return (
                        Err(NOT_NEEDED),
                        format!("skill {skill}: {name} not kept: {NOT_NEEDED}"),
                    );
                }
                let mut file = self.current(skill);
                file.skill = skill.clone();
                if file.values.is_empty() {
                    file.package = crate::skill_writes::installed_package(&self.skills, skill);
                }
                file.values.retain(|kept, _| needed.contains(kept));
                file.values.insert(name.clone(), value.clone());
                match self.keep(skill, &file) {
                    Ok(()) => (
                        Ok(self.view(skill, &needed, &file)),
                        format!(
                            "skill {skill}: {name} kept ({} bytes)",
                            value.secret().len()
                        ),
                    ),
                    Err(error) => (
                        Err(NOT_KEPT),
                        format!("skill {skill}: {name} not kept: {}", error.kind()),
                    ),
                }
            }
            Ask::Unset { skill, name } => {
                let needed = match needed(&self.skills, skill) {
                    Ok(needed) => needed,
                    Err(why) => return (Err(why), format!("a skill key not removed: {why}")),
                };
                if !needed.contains(name) {
                    return (
                        Err(NOT_NEEDED),
                        format!("skill {skill}: {name} not removed: {NOT_NEEDED}"),
                    );
                }
                let mut file = self.current(skill);
                file.values
                    .retain(|kept, _| kept != name && needed.contains(kept));
                match self.keep(skill, &file) {
                    Ok(()) => (
                        Ok(self.view(skill, &needed, &file)),
                        format!("skill {skill}: {name} removed"),
                    ),
                    Err(error) => (
                        Err(NOT_REMOVED),
                        format!("skill {skill}: {name} not removed: {}", error.kind()),
                    ),
                }
            }
        }
    }

    /// The run's ask, `{"op":"run_secrets","skill"}`: the set values of
    /// the names the skill needs, and the names missing, `{"ok":true,
    /// "values":{NAME:VALUE},"missing":[NAME]}`; or `{"ok":false,"error"}`.
    /// With the journal's line: counts only.
    pub fn run(&self, line: &str) -> (Json, String) {
        let refused = |why: &str| json!({"ok": false, "error": why});
        let asked: Option<Map<String, Json>> = serde_json::from_str::<Json>(line.trim_end())
            .ok()
            .and_then(|value| value.as_object().cloned());
        let Some(asked) = asked.filter(|asked| {
            asked.len() == 2 && asked.get("op").and_then(Json::as_str) == Some(RUN_OP)
        }) else {
            return (
                refused(BAD_FIELDS),
                "a skill's run asked for keys wrongly".into(),
            );
        };
        let Some(skill) = asked
            .get("skill")
            .and_then(Json::as_str)
            .filter(|skill| crate::skill_writes::skill_name_ok(skill))
        else {
            return (refused(BAD_SKILL), "a skill's run named no skill".into());
        };
        let needed = match needed(&self.skills, skill) {
            Ok(needed) => needed,
            Err(why) => return (refused(why), format!("a skill's run: {why}")),
        };
        if needed.is_empty() {
            return (
                json!({"ok": true, "values": {}, "missing": []}),
                format!("a skill's run: {skill} needs no key"),
            );
        }
        let file = {
            let _held = self
                .write
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            self.current(skill)
        };
        let mut values = Map::new();
        let mut missing = Vec::new();
        for name in &needed {
            match file.values.get(name) {
                Some(value) => {
                    values.insert(name.clone(), Json::from(value.secret()));
                }
                None => missing.push(name.clone()),
            }
        }
        let said = format!(
            "a skill's run: {skill} given {} key(s), {} missing",
            values.len(),
            missing.len()
        );
        (
            json!({"ok": true, "values": values, "missing": missing}),
            said,
        )
    }
}

#[cfg(test)]
mod tests;
