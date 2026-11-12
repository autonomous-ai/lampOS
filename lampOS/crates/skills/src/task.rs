//! A skill's `do.task` step and its `grants` (#6071 PR 2, #6305): a skill
//! that reacts at once with its own steps and hands the slower part to the
//! lamp's agent, a script in its own `scripts/` folder. Off unless
//! `LAMP_SKILL_TASKS=on` ([`ENVIRONMENT`]), here, in the manage helper and
//! in the voice.
//!
//! - **The step**: `[[do]] task = { task = "<what to do>", run =
//!   "scripts/main.py" }`, alone in its `[[do]]` table, one a skill
//!   ([`TaskFile::step`]): `run` a plain file inside the skill's own
//!   `scripts/`, never a link or a path out of it.
//! - **Its grants**: `grants = [...]` at the top of `skill.toml`, each one
//!   of lamp-control's grants (`lamp_voice::skill_task_yes::GRANTS`), once
//!   each, never `skills` ([`grants_of`]); only beside a `do.task`. What
//!   the script may reach through lamp-control's `control.sock` with its
//!   one-run token (#6304), nothing more.
//! - **First kind only** ([`origin_refuses`]): a skill whose script came
//!   from the Skills Store (backend-reviewed, its `.package.json`'s
//!   `from_store`) or was pushed by the owner's own paired computer (the
//!   developer folder's). A skill made or added on the lamp (`made: here`),
//!   or put on by hand from a folder or a file, is not loaded with one,
//!   but for one the lamp's own model made, put on as a package, with
//!   `LAMP_MADE_SCRIPTS=on` ([`made_with_code`], #6307).
//! - **The yes** (kept by lamp-control, `lamp_control::task_yes`, never
//!   here: the first builder's ruling on #6059; this unit's
//!   `ProtectSystem=strict` gives it no write path to lamp-control's
//!   folder, and lamp-control records a yes from the voice's cgroup
//!   alone): recorded on the voice's yes
//!   (`lamp_voice::skill_task_yes::approve_by`) with the grants said back
//!   and the SHA-256 of the skill's folder ([`folder_sha256`]). At each run
//!   the folder is hashed again and lamp-control asked ([`yes_refuses`]):
//!   no yes, a yes for other bytes or grants, or lamp-control not reached
//!   hands nothing over, and the person is asked again.
//! - **Limits**: one `do.task` at a time a skill, and at most [`PER_SKILL`]
//!   a skill and [`ALL_SKILLS`] across every skill in any 24 hours
//!   ([`Runs`]).
//! - **The hand-over** ([`HandOver::line`]): one line to lamp-tasks
//!   (`--tasks-socket`, #6306), answered when the run ends; its result
//!   comes back as Task news through lamp-tasks. The other steps of the
//!   skill (light, cue, say) run at once beside it.
//! - **Off**, a `do.task` loads and hands nothing over, and the journal
//!   says why ([`OFF`]); the skill's other steps still run.
use crate::package;
use lamp_voice::skill_task_yes::Agreement;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::{Component, Path},
    sync::Mutex,
    time::Duration,
};

/// The switch, here, in the manage helper and in the voice. `on` only.
pub const ENVIRONMENT: &str = lamp_voice::skill_task_yes::ENVIRONMENT;

#[cfg(test)]
thread_local! {
    /// The switch as a test of this thread sets it.
    static ON_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether a skill's tasks are on in this process.
pub fn on() -> bool {
    #[cfg(test)]
    if let Some(on) = ON_HERE.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The switch for this test's thread alone.
#[cfg(test)]
pub fn set_on_here(on: Option<bool>) {
    ON_HERE.with(|here| here.set(on));
}

/// What a `do.task` is told with the switch off: in the journal.
pub const OFF: &str =
    "do.task: not handed over: a skill's tasks are off on this lamp (LAMP_SKILL_TASKS is not on)";
/// The folder a `do.task`'s `run` is in.
pub const SCRIPTS: &str = "scripts";
/// The most bytes of a `do.task`'s `task`.
pub const MOST_TASK_BYTES: usize = 500;
/// The most runs a skill hands over in any 24 hours.
pub const PER_SKILL: usize = 3;
/// The most runs every skill together hands over in any 24 hours.
pub const ALL_SKILLS: usize = 10;
/// The window the bounds count in, milliseconds.
pub const DAY_MS: u64 = 86_400_000;
/// The longest a run is waited for: lamp-tasks answers the hand-over line
/// when the run ends.
pub const RUN_MOST: Duration = Duration::from_secs(15 * 60);
/// In `--state-dir`: when each skill's task was handed over, for the bounds.
pub const RUNS_FILE: &str = "task-runs.json";
/// What a task with no yes for its skill is told.
pub const WAITING: &str = "do.task: not handed over: waiting for the owner's yes (asked what the skill uses, the lamp says it back)";
/// What a task whose folder or grants changed since the yes is told.
pub const CHANGED: &str = "do.task: not handed over: its script or grants changed since the owner's yes, so it asks again";
/// What a task is told when lamp-control, which keeps the owner's yes,
/// can't say.
pub const NO_CONTROL: &str =
    "do.task: not handed over: lamp-control, which keeps the owner's yes, can't be asked";

/// `task = { … }` in a `[[do]]` table, as the folder writes it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskFile {
    task: String,
    run: String,
}

/// A `do.task` step as loaded: its words, and its script's path in the
/// skill's folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskStep {
    pub task: String,
    pub run: String,
}

impl TaskFile {
    /// The step of the skill in `dir`, or why the folder's words are not one.
    pub fn step(self, dir: &Path) -> Result<TaskStep, String> {
        let task = self.task.trim();
        if task.is_empty() || task.len() > MOST_TASK_BYTES || task.chars().any(char::is_control) {
            return Err(format!(
                "do.task: task is 1 to {MOST_TASK_BYTES} bytes of words, with no control characters"
            ));
        }
        run_ok(dir, &self.run)?;
        Ok(TaskStep {
            task: task.to_owned(),
            run: self.run,
        })
    }
}

/// Whether `run` is a plain file inside `dir`'s `scripts/`: relative, each
/// part a plain name (no `..`, no hidden one), at most four parts, and no
/// part of it a link.
fn run_ok(dir: &Path, run: &str) -> Result<(), String> {
    let not_inside = || {
        format!(
            "do.task: run {run:?} is a file inside the skill's own {SCRIPTS}/, like \"{SCRIPTS}/main.py\""
        )
    };
    let path = Path::new(run);
    let parts: Vec<&str> = path
        .components()
        .map(|part| match part {
            Component::Normal(name) => name.to_str().filter(|name| !name.starts_with('.')),
            _ => None,
        })
        .collect::<Option<_>>()
        .ok_or_else(not_inside)?;
    if parts.len() < 2 || parts.len() > 4 || parts[0] != SCRIPTS || run.contains('\\') {
        return Err(not_inside());
    }
    let mut at = dir.to_path_buf();
    for part in &parts {
        at.push(part);
        let meta = fs::symlink_metadata(&at)
            .map_err(|_| format!("do.task: run {run:?} is not in the skill's folder"))?;
        if meta.file_type().is_symlink() {
            return Err(format!("do.task: run {run:?}: a link, never followed"));
        }
    }
    if !fs::symlink_metadata(&at).is_ok_and(|meta| meta.is_file()) {
        return Err(format!("do.task: run {run:?} is not a plain file"));
    }
    Ok(())
}

/// `grants = [...]` checked: each one of lamp-control's grants a script may
/// hold (`lamp_voice::skill_task_yes::GRANTS`), once each; never `skills`.
pub fn grants_of(grants: &[String]) -> Result<Vec<String>, String> {
    let mut kept: Vec<String> = Vec::new();
    for grant in grants {
        if grant == lamp_voice::skill_task_yes::NEVER {
            return Err(
                "grants: \"skills\" is never a script's: it never puts skills on the lamp".into(),
            );
        }
        if lamp_voice::skill_task_yes::grant_named(grant).is_none() {
            let known: Vec<&str> = lamp_voice::skill_task_yes::GRANTS
                .iter()
                .map(|(name, _)| *name)
                .collect();
            return Err(format!(
                "grants: {:?} is not a grant: one of {known:?}",
                grant.chars().take(32).collect::<String>()
            ));
        }
        if kept.contains(grant) {
            return Err(format!("grants: {grant:?} is listed twice"));
        }
        kept.push(grant.clone());
    }
    Ok(kept)
}

/// A skill made by the lamp's own model with code of its own (#6307): its
/// `do.task` loads only with this on, here and in the manage helper. `on`
/// only.
pub const MADE_ENVIRONMENT: &str = lamp_voice::made_skill::SCRIPTS_ENVIRONMENT;

#[cfg(test)]
thread_local! {
    /// A made skill's code as a test of this thread sets it.
    static MADE_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether a made skill's code is on in this process.
pub fn made_on() -> bool {
    #[cfg(test)]
    if let Some(on) = MADE_HERE.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(MADE_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// A made skill's code for this test's thread alone.
#[cfg(test)]
pub fn set_made_here(on: Option<bool>) {
    MADE_HERE.with(|here| here.set(on));
}

/// Where the script of a skill whose `do.task` may run came from, as the
/// say-back says it, when it is the lamp's own model's ([`origin_refuses`]).
pub const FROM_MADE: &str = "the lamp's own model";

/// Whether the skill in `folder` is one the lamp's own model made with code
/// of its own (#6307), with [`made_on`]: its card says `made: here` and it
/// went on as a package (`.package.json`, as the manage helper's
/// `install_made` puts one on, a package said back and agreed to by its
/// SHA-256), not from the Store, nor pushed, nor put on from a folder.
pub fn made_with_code(folder: &Path, pushed: bool) -> bool {
    let card = fs::read_to_string(folder.join("SKILL.md")).unwrap_or_default();
    made_on()
        && !pushed
        && card.lines().any(|line| line.trim() == "made: here")
        && package::installed_from(folder).is_some_and(|installed| !installed.from_store)
}

/// Why the skill in `folder` may not carry a `do.task`, or none: only one
/// from the Skills Store (its `.package.json` says `from_store`, and its
/// card is not marked made or added here) or pushed by the owner's own
/// paired computer (`pushed`, the developer folder's), or one the lamp's
/// own model made with its code's switch on ([`made_with_code`], #6307).
pub fn origin_refuses(folder: &Path, pushed: bool) -> Option<String> {
    if made_with_code(folder, pushed) {
        return None;
    }
    let card = fs::read_to_string(folder.join("SKILL.md")).unwrap_or_default();
    if card.lines().any(|line| {
        matches!(
            line.trim(),
            "made: here" | lamp_voice::made_skill::ADDED_FROM_FILE
        )
    }) {
        return Some(
            "do.task: a skill made or added on the lamp can't hand a task to a script yet".into(),
        );
    }
    if pushed || package::installed_from(folder).is_some_and(|installed| installed.from_store) {
        return None;
    }
    Some(
        "do.task: only a skill from the Skills Store or pushed by the owner's own computer may carry a script"
            .into(),
    )
}

/// The lamp's own records in a skill's folder, never the skill's: left out
/// of [`folder_sha256`].
pub const LAMP_RECORDS: [&str; 3] = [
    package::INSTALLED_FROM,
    crate::PUSHED_FROM,
    package::REVOKED,
];

/// Every plain file under `dir`, hidden ones too (a script may read a
/// dotfile beside it), by its path in the folder, but the lamp's own
/// records at its top ([`LAMP_RECORDS`]); a link, a name that is not
/// UTF-8 or has a control character, or a folder past a skill's bounds
/// refuses it.
fn hashed_files(dir: &Path) -> Result<package::Files, String> {
    let mut files = package::Files::new();
    let mut total = 0u64;
    let mut folders = vec![(dir.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = folders.pop() {
        let entries =
            fs::read_dir(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("{}: {error}", folder.display()))?;
            let name = entry.file_name();
            let Some(name) = name
                .to_str()
                .filter(|name| !name.chars().any(char::is_control))
            else {
                return Err(format!("{}: not a plain name", entry.path().display()));
            };
            if prefix.is_empty() && LAMP_RECORDS.contains(&name) {
                continue;
            }
            let path = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            let meta =
                fs::symlink_metadata(entry.path()).map_err(|error| format!("{path}: {error}"))?;
            if meta.is_dir() {
                folders.push((entry.path(), path));
                continue;
            }
            if !meta.is_file() {
                return Err(format!("{path}: a link or not a plain file"));
            }
            total += meta.len();
            if total > package::MOST_BYTES || files.len() >= package::MOST_FILES {
                return Err(format!(
                    "more than {} files or {} bytes: too big for a skill",
                    package::MOST_FILES,
                    package::MOST_BYTES
                ));
            }
            let bytes = read_small(&entry.path(), meta.len())?
                .ok_or_else(|| format!("{path}: gone while read"))?;
            files.insert(path, bytes);
        }
    }
    Ok(files)
}

/// The SHA-256 of the skill folder `dir` as the yes is tied to it: every
/// file, hidden ones too, but the lamp's own records ([`hashed_files`]), by
/// its path in the folder in order, each as a line `SHA256  PATH` (as
/// `sha256sum` writes it), and those lines hashed together, lowercase hex.
/// One definition, for this program, the manage helper, the voice and
/// lamp-control's one-run token (#6304).
pub fn folder_sha256(dir: &Path) -> Result<String, String> {
    let files = hashed_files(dir)?;
    let mut listed = String::new();
    for (path, bytes) in &files {
        listed.push_str(&hex(&Sha256::digest(bytes)));
        listed.push_str("  ");
        listed.push_str(path);
        listed.push('\n');
    }
    Ok(hex(&Sha256::digest(listed.as_bytes())))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A plain file's bytes, never through a link, at most `most`; none when
/// it isn't there.
fn read_small(path: &Path, most: u64) -> Result<Option<Vec<u8>>, String> {
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let mut bytes = Vec::new();
    file.take(most + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() as u64 > most {
        return Err(format!("{}: more than {most} bytes", path.display()));
    }
    Ok(Some(bytes))
}

/// Why the skill `name`, its folder hashed `sha256` now and declaring
/// `grants`, may not hand its task over, by lamp-control's word at
/// `control` (`lamp_voice::skill_task_yes::agreed_by`), or none:
/// [`WAITING`] with no yes of its name, [`CHANGED`] with one for other
/// bytes or other grants, [`NO_CONTROL`] when lamp-control can't say. Only
/// asked: lamp-skills never writes a yes.
pub fn yes_refuses(
    control: &Path,
    name: &str,
    sha256: &str,
    grants: &[String],
) -> Option<&'static str> {
    match lamp_voice::skill_task_yes::agreed_by(control, name, sha256, grants) {
        Ok(Agreement::Agreed) => None,
        Ok(Agreement::Waiting) => Some(WAITING),
        Ok(Agreement::Changed) => Some(CHANGED),
        Err(_) => Some(NO_CONTROL),
    }
}

/// The runs under way, one a skill.
static UNDER_WAY: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
/// Without `--state-dir`, when each run was handed over, for the bounds.
static RUNS_HERE: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());

/// A skill's task under way: the skill is free for its next when it ends
/// ([`Drop`]).
#[derive(Debug)]
pub struct Runs {
    skill: String,
}

impl Drop for Runs {
    fn drop(&mut self) {
        UNDER_WAY
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(&self.skill);
    }
}

/// Every run in `runs` counted within the day before `now_ms`: past the
/// bounds for `skill`, or after now (a clock set back), is full.
fn bounded(runs: &[(String, u64)], skill: &str, now_ms: u64) -> Result<(), String> {
    if runs.iter().any(|(_, at)| *at > now_ms) {
        return Err("do.task: not handed over: its runs' ledger has one after now".into());
    }
    let today: Vec<&(String, u64)> = runs
        .iter()
        .filter(|(_, at)| now_ms.saturating_sub(*at) < DAY_MS)
        .collect();
    if today.iter().filter(|(name, _)| name == skill).count() >= PER_SKILL {
        return Err(format!(
            "do.task: not handed over: its {PER_SKILL} runs in 24 hours are spent"
        ));
    }
    if today.len() >= ALL_SKILLS {
        return Err(format!(
            "do.task: not handed over: the skills' {ALL_SKILLS} runs in 24 hours are spent"
        ));
    }
    Ok(())
}

/// `skill`'s task begun at `now_ms`, if it may be: none of its own under
/// way, and within the bounds kept in `state`'s [`RUNS_FILE`] (or, without
/// one, this program's own count); an unreadable ledger counts as full.
/// The run is counted as it begins.
pub fn begin(skill: &str, state: Option<&Path>, now_ms: u64) -> Result<Runs, String> {
    let mut under_way = UNDER_WAY
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if under_way.contains(skill) {
        return Err("do.task: not handed over: its last task is still under way".into());
    }
    let mut runs: Vec<(String, u64)> = match state {
        Some(state) => match read_small(&state.join(RUNS_FILE), 64 * 1024) {
            Ok(None) => Vec::new(),
            Ok(Some(bytes)) => serde_json::from_slice(&bytes).map_err(|_| {
                "do.task: not handed over: its runs' ledger can't be read".to_owned()
            })?,
            Err(_) => {
                return Err("do.task: not handed over: its runs' ledger can't be read".into());
            }
        },
        None => RUNS_HERE
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone(),
    };
    bounded(&runs, skill, now_ms)?;
    runs.retain(|(_, at)| now_ms.saturating_sub(*at) < DAY_MS);
    runs.push((skill.to_owned(), now_ms));
    match state {
        Some(state) => {
            let path = state.join(RUNS_FILE);
            let json = serde_json::to_vec(&runs).map_err(|error| error.to_string())?;
            lamp_update::records::write_whole(&path, &json, 0o644)
                .map_err(|error| format!("do.task: not handed over: its runs' ledger: {error}"))?;
        }
        None => {
            *RUNS_HERE
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()) = runs;
        }
    }
    under_way.insert(skill.to_owned());
    Ok(Runs {
        skill: skill.to_owned(),
    })
}

/// One `do.task` handed to lamp-tasks (#6306): what it receives on its
/// socket, as one line, and answers when the run ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandOver<'a> {
    pub skill: &'a str,
    pub step: &'a TaskStep,
    /// The skill's folder on the lamp, which its script is in.
    pub folder: &'a Path,
    /// The folder's SHA-256 as hashed for this run, which the yes holds.
    pub sha256: &'a str,
    /// The grants the yes covers: lamp-control's one-run token's (#6304).
    pub grants: &'a [String],
}

impl HandOver<'_> {
    /// `{"op": "skill_task", "skill", "task", "run", "folder", "sha256",
    /// "grants"}`: the contract lamp-tasks' hand-over takes (#6306).
    pub fn line(&self) -> String {
        serde_json::json!({
            "op": "skill_task",
            "skill": self.skill,
            "task": self.step.task,
            "run": self.step.run,
            "folder": self.folder.to_string_lossy(),
            "sha256": self.sha256,
            "grants": self.grants,
        })
        .to_string()
    }
}

/// Where a skill's task is handed over: the program's `--state-dir`, where
/// the runs' ledger is, lamp-control's owner's socket, which keeps the
/// owner's yes (`lamp_voice::skill_task_yes::CONTROL_SOCKET`), and
/// lamp-tasks' socket (`--tasks-socket`, #6306).
#[derive(Clone, Copy, Debug)]
pub struct Place<'a> {
    pub state: Option<&'a Path>,
    pub control: &'a Path,
    pub socket: Option<&'a Path>,
}

/// The folder a skill's task is handed over from: `name` under the lamp's
/// skills `root`, or, for one the owner's computer pushed (`pushed`), under
/// the developer folder `dev` (on a lamp [`crate::PUSHED_DIR`]), the one
/// other place lamp-tasks and lamp-control take. `None` for a pushed skill
/// with no developer folder, or a name that is not a plain one.
pub fn folder_of(
    root: &Path,
    dev: Option<&Path>,
    name: &str,
    pushed: bool,
) -> Option<std::path::PathBuf> {
    let mut parts = Path::new(name).components();
    if !matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(part)), None) if part == name && !name.starts_with('.')
    ) {
        return None;
    }
    Some(if pushed {
        dev?.join(name)
    } else {
        root.join(name)
    })
}

/// The `do.task` `step` of the skill `skill` in `folder`, declaring
/// `grants`, handed over at `now_ms` through `send` (the socket and the
/// line; lamp-tasks' answer), or why not: the journal's words. With the
/// switch off, nothing ([`OFF`]); with no yes for the folder as it is now
/// and these grants, nothing ([`WAITING`], [`CHANGED`]); past the bounds or
/// with its last still under way, nothing. The run is counted once it is
/// handed over, and the skill holds its one place until `send` returns.
pub fn hand(
    skill: &str,
    step: &TaskStep,
    folder: &Path,
    grants: &[String],
    place: Place<'_>,
    now_ms: u64,
    send: impl FnOnce(&Path, &str) -> Result<String, String>,
) -> String {
    if !on() {
        return OFF.into();
    }
    let Some(state) = place.state else {
        return "do.task: not handed over: no --state-dir, so its runs aren't counted".into();
    };
    let sha256 = match folder_sha256(folder) {
        Ok(sha256) => sha256,
        Err(why) => return format!("do.task: not handed over: its folder can't be read: {why}"),
    };
    if let Some(why) = yes_refuses(place.control, skill, &sha256, grants) {
        return why.into();
    }
    let Some(socket) = place.socket else {
        return "do.task: not handed over: no lamp-tasks hand-over on this lamp yet (--tasks-socket, #6306)".into();
    };
    let _runs = match begin(skill, Some(state), now_ms) {
        Ok(runs) => runs,
        Err(why) => return why,
    };
    let line = HandOver {
        skill,
        step,
        folder,
        sha256: &sha256,
        grants,
    }
    .line();
    match send(socket, &line) {
        Ok(said) => format!("do.task: handed to lamp-tasks: {said}"),
        Err(why) => format!("do.task: lamp-tasks did not take it: {why}"),
    }
}

/// lamp-tasks' answer to a hand-over line, as the journal says it: its
/// task's number, or its refusal's code and words.
pub fn answered(answer: &str) -> Result<String, String> {
    let answer: serde_json::Value = serde_json::from_str(answer.trim())
        .map_err(|_| "its answer is not one line of JSON".to_owned())?;
    let cut = |text: &str| text.chars().take(200).collect::<String>();
    match answer.get("ok").and_then(serde_json::Value::as_bool) {
        Some(true) => Ok(match answer.get("id").and_then(serde_json::Value::as_u64) {
            Some(id) => format!("its run ended, task {id}"),
            None => "its run ended".to_owned(),
        }),
        _ => Err(format!(
            "{}: {}",
            cut(answer["error"].as_str().unwrap_or("refused")),
            cut(answer["message"].as_str().unwrap_or(""))
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(dir: &Path) -> std::path::PathBuf {
        let folder = dir.join("photo-booth");
        fs::create_dir_all(folder.join(SCRIPTS)).unwrap();
        fs::write(
            folder.join("SKILL.md"),
            "---\nname: photo-booth\ndescription: Photos.\n---\n",
        )
        .unwrap();
        fs::write(folder.join("skill.toml"), "version = \"1.0.0\"\n").unwrap();
        fs::write(folder.join(SCRIPTS).join("main.py"), "print('hi')\n").unwrap();
        folder
    }

    fn step(dir: &Path, run: &str) -> Result<TaskStep, String> {
        TaskFile {
            task: "Take a photo".into(),
            run: run.into(),
        }
        .step(dir)
    }

    #[test]
    fn run_is_a_plain_file_inside_scripts() {
        let dir = tempfile::tempdir().unwrap();
        let folder = folder(dir.path());
        assert_eq!(
            step(&folder, "scripts/main.py").unwrap().run,
            "scripts/main.py"
        );
        fs::write(folder.join("main.py"), "x").unwrap();
        fs::write(folder.join(SCRIPTS).join(".hidden.py"), "x").unwrap();
        std::os::unix::fs::symlink(folder.join("main.py"), folder.join(SCRIPTS).join("link.py"))
            .unwrap();
        fs::create_dir_all(folder.join(SCRIPTS).join("lib")).unwrap();
        for (run, why) in [
            ("main.py", "inside the skill's own scripts/"),
            ("/etc/passwd", "inside the skill's own scripts/"),
            ("scripts/../main.py", "inside the skill's own scripts/"),
            ("scripts/.hidden.py", "inside the skill's own scripts/"),
            ("scripts", "inside the skill's own scripts/"),
            ("scripts/a/b/c/d.py", "inside the skill's own scripts/"),
            ("scripts/none.py", "not in the skill's folder"),
            ("scripts/link.py", "a link"),
            ("scripts/lib", "not a plain file"),
        ] {
            let refused = step(&folder, run).unwrap_err();
            assert!(refused.contains(why), "{run}: {refused}");
        }
        for task in ["", "  ", "a\nb"] {
            let refused = TaskFile {
                task: task.into(),
                run: "scripts/main.py".into(),
            }
            .step(&folder)
            .unwrap_err();
            assert!(refused.contains("task is 1 to"), "{refused}");
        }
    }

    #[test]
    fn grants_are_known_once_each_and_never_skills() {
        let list = |names: &[&str]| {
            names
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            grants_of(&list(&["camera", "light"])).unwrap(),
            list(&["camera", "light"])
        );
        assert!(grants_of(&[]).unwrap().is_empty());
        for (bad, why) in [
            (list(&["root"]), "is not a grant"),
            (list(&["camera", "camera"]), "listed twice"),
            (list(&["skills"]), "never a script's"),
        ] {
            let refused = grants_of(&bad).unwrap_err();
            assert!(refused.contains(why), "{bad:?}: {refused}");
        }
    }

    /// The folder's hash changes with any file, hidden ones too, but the
    /// lamp's own records at its top; a link refuses it.
    #[test]
    fn the_folder_hash_follows_every_file_but_the_lamps_records() {
        let dir = tempfile::tempdir().unwrap();
        let folder = folder(dir.path());
        let first = folder_sha256(&folder).unwrap();
        assert_eq!(first.len(), 64);
        for record in LAMP_RECORDS {
            fs::write(folder.join(record), "{}").unwrap();
        }
        assert_eq!(folder_sha256(&folder).unwrap(), first);
        fs::write(folder.join(SCRIPTS).join(".helper.py"), "x = 1\n").unwrap();
        let hidden = folder_sha256(&folder).unwrap();
        assert_ne!(hidden, first);
        // A record's name below the top is the skill's own file.
        fs::write(folder.join(SCRIPTS).join(".package.json"), "{}").unwrap();
        assert_ne!(folder_sha256(&folder).unwrap(), hidden);
        fs::write(folder.join(SCRIPTS).join("main.py"), "print('changed')\n").unwrap();
        assert_ne!(folder_sha256(&folder).unwrap(), first);
        std::os::unix::fs::symlink("/etc/hostname", folder.join(SCRIPTS).join(".link")).unwrap();
        assert!(folder_sha256(&folder).unwrap_err().contains("a link"));
    }

    #[test]
    fn only_a_store_or_pushed_skill_carries_a_task() {
        let dir = tempfile::tempdir().unwrap();
        let folder = folder(dir.path());
        assert!(origin_refuses(&folder, true).is_none());
        let by_hand = origin_refuses(&folder, false).unwrap();
        assert!(by_hand.contains("Skills Store"), "{by_hand}");
        fs::write(
            folder.join("SKILL.md"),
            "---\nname: photo-booth\ndescription: Photos.\nmade: here\n---\n",
        )
        .unwrap();
        set_made_here(Some(false));
        let made = origin_refuses(&folder, true).unwrap();
        assert!(made.contains("made or added on the lamp"), "{made}");
        set_made_here(None);
    }

    /// #6307: a skill the lamp's own model made carries a task only with
    /// its code's switch on, and only as it went on as a package, never
    /// from a folder, pushed, or from the Store.
    #[test]
    fn a_made_skill_carries_a_task_only_with_its_code_on_and_as_a_package() {
        let dir = tempfile::tempdir().unwrap();
        let folder = folder(dir.path());
        fs::write(
            folder.join("SKILL.md"),
            "---\nname: photo-booth\ndescription: Photos.\nmade: here\n---\n",
        )
        .unwrap();
        let record = |from_store: bool| {
            let installed = package::Installed {
                sha256: "ab".repeat(32),
                manifest: package::Manifest {
                    format: 1,
                    name: "photo-booth".into(),
                    version: "1.0.0".into(),
                    files: Vec::new(),
                },
                from_store,
            };
            fs::write(
                folder.join(package::INSTALLED_FROM),
                serde_json::to_vec(&installed).unwrap(),
            )
            .unwrap();
        };
        set_made_here(Some(true));
        // Put on from a folder: no package record.
        assert!(origin_refuses(&folder, false).is_some());
        record(false);
        assert!(made_with_code(&folder, false));
        assert!(origin_refuses(&folder, false).is_none());
        assert!(!made_with_code(&folder, true));
        set_made_here(Some(false));
        assert!(origin_refuses(&folder, false).is_some());
        set_made_here(Some(true));
        record(true);
        assert!(!made_with_code(&folder, false));
        set_made_here(None);
    }

    /// lamp-control at `path`, answering each of `answers` in turn, one a
    /// connection: the lines it was asked.
    fn control(
        path: &Path,
        answers: Vec<serde_json::Value>,
    ) -> std::thread::JoinHandle<Vec<serde_json::Value>> {
        use std::io::{BufRead as _, Write as _};
        let _ = fs::remove_file(path);
        let listener = std::os::unix::net::UnixListener::bind(path).unwrap();
        std::thread::spawn(move || {
            let mut asked = Vec::new();
            for answer in answers {
                let (stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                std::io::BufReader::new(&stream)
                    .read_line(&mut line)
                    .unwrap();
                asked.push(serde_json::from_str(&line).unwrap());
                writeln!(&stream, "{answer}").unwrap();
            }
            asked
        })
    }

    /// The yes is lamp-control's word, only asked: agreed, waiting,
    /// changed, or lamp-control not reached, which hands nothing over.
    #[test]
    fn the_yes_is_only_asked_of_lamp_control() {
        let dir = tempfile::tempdir().unwrap();
        let folder = folder(dir.path());
        let grants = vec!["camera".to_owned(), "light".to_owned()];
        let sha256 = folder_sha256(&folder).unwrap();
        let socket = dir.path().join("control.sock");
        let asked = control(
            &socket,
            vec![
                serde_json::json!({"agreed": true}),
                serde_json::json!({"agreed": false, "why": "waiting"}),
                serde_json::json!({"agreed": false, "why": "changed"}),
                serde_json::json!({"error": "a skill's tasks are off on this lamp"}),
            ],
        );
        let refuses = || yes_refuses(&socket, "photo-booth", &sha256, &grants);
        assert_eq!(refuses(), None);
        assert_eq!(refuses(), Some(WAITING));
        assert_eq!(refuses(), Some(CHANGED));
        assert_eq!(refuses(), Some(NO_CONTROL));
        let asked = asked.join().unwrap();
        let check = &asked[0][lamp_voice::skill_task_yes::CHECK];
        assert_eq!(check["name"], "photo-booth");
        assert_eq!(check["sha256"], sha256.as_str());
        assert_eq!(check["grants"], serde_json::json!(["camera", "light"]));
        // Every line it sends is a check: never a record.
        assert!(asked.iter().all(|line| {
            line.as_object()
                .is_some_and(|line| line.keys().eq([lamp_voice::skill_task_yes::CHECK]))
        }));
        fs::remove_file(&socket).unwrap();
        assert_eq!(refuses(), Some(NO_CONTROL));
    }

    /// One at a time a skill, and at most three a skill a day; an
    /// unreadable ledger, or one with a run after now, counts as full.
    #[test]
    fn one_task_at_a_time_and_a_daily_cap() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path();
        let first = begin("cap-skill", Some(state), 1_000).unwrap();
        let busy = begin("cap-skill", Some(state), 1_001).unwrap_err();
        assert!(busy.contains("still under way"), "{busy}");
        drop(first);
        drop(begin("cap-skill", Some(state), 2_000).unwrap());
        drop(begin("cap-skill", Some(state), 3_000).unwrap());
        let spent = begin("cap-skill", Some(state), 4_000).unwrap_err();
        assert!(spent.contains("3 runs in 24 hours"), "{spent}");
        // A day on, it may again.
        drop(begin("cap-skill", Some(state), 1_000 + DAY_MS).unwrap());
        // Set back: full.
        let back = begin("cap-skill", Some(state), 500).unwrap_err();
        assert!(back.contains("after now"), "{back}");
        fs::write(state.join(RUNS_FILE), "not json").unwrap();
        assert!(
            begin("cap-skill", Some(state), 9_000)
                .unwrap_err()
                .contains("can't be read")
        );
        // Across every skill: at most ten.
        let runs: Vec<(String, u64)> = (0..ALL_SKILLS)
            .map(|at| (format!("s{at}"), at as u64))
            .collect();
        assert!(
            bounded(&runs, "another", 100)
                .unwrap_err()
                .contains("10 runs")
        );
    }

    /// Off: nothing is handed over, and the journal says why. On: with no
    /// yes from lamp-control it waits; with it the line names the folder's
    /// SHA-256 and grants; a changed script asks again.
    #[test]
    fn a_task_is_handed_over_only_on_with_the_yes_for_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        fs::create_dir_all(&state).unwrap();
        let folder = folder(dir.path());
        let step = step(&folder, "scripts/main.py").unwrap();
        let grants = vec!["light".to_owned()];
        let socket = dir.path().join("tasks.sock");
        let control_socket = dir.path().join("control.sock");
        let place = Place {
            state: Some(&state),
            control: &control_socket,
            socket: Some(&socket),
        };
        let agreed = serde_json::json!({"agreed": true});
        let lamp_control = control(
            &control_socket,
            vec![
                serde_json::json!({"agreed": false, "why": "waiting"}),
                agreed.clone(),
                serde_json::json!({"agreed": false, "why": "changed"}),
                agreed.clone(),
            ],
        );
        let never = |_: &Path, _: &str| -> Result<String, String> { panic!("handed over") };
        set_on_here(Some(false));
        assert_eq!(
            hand("photo-booth", &step, &folder, &grants, place, 1, never),
            OFF
        );
        set_on_here(Some(true));
        assert_eq!(
            hand("photo-booth", &step, &folder, &grants, place, 1, never),
            WAITING
        );
        let sha256 = folder_sha256(&folder).unwrap();
        let mut sent = None;
        let said = hand(
            "photo-booth",
            &step,
            &folder,
            &grants,
            place,
            1,
            |at, line| {
                assert_eq!(at, socket.as_path());
                sent = Some(line.to_owned());
                answered("{\"ok\":true,\"id\":7}")
            },
        );
        assert_eq!(said, "do.task: handed to lamp-tasks: its run ended, task 7");
        let line: serde_json::Value = serde_json::from_str(&sent.unwrap()).unwrap();
        assert_eq!(line["op"], "skill_task");
        assert_eq!(line["sha256"], sha256.as_str());
        assert_eq!(line["grants"], serde_json::json!(["light"]));
        assert_eq!(line["run"], "scripts/main.py");
        fs::write(folder.join(SCRIPTS).join("main.py"), "print('changed')\n").unwrap();
        assert_eq!(
            hand("photo-booth", &step, &folder, &grants, place, 2, never),
            CHANGED
        );
        // Each run asked lamp-control of the folder as hashed then.
        fs::write(folder.join(SCRIPTS).join("main.py"), "print('hi')\n").unwrap();
        let refused = hand("photo-booth", &step, &folder, &grants, place, 4, |_, _| {
            answered("{\"ok\":false,\"error\":\"unknown_op\",\"message\":\"no\"}")
        });
        assert!(refused.contains("did not take it: unknown_op"), "{refused}");
        let asked = lamp_control.join().unwrap();
        let check = |at: usize| asked[at][lamp_voice::skill_task_yes::CHECK]["sha256"].clone();
        assert_eq!(check(1), sha256.as_str());
        assert_ne!(check(2), sha256.as_str());
        // lamp-control not there: nothing handed over.
        assert_eq!(
            hand("photo-booth", &step, &folder, &grants, place, 5, never),
            NO_CONTROL
        );
        // No hand-over yet on this lamp: said so.
        let lamp_control = control(&control_socket, vec![agreed]);
        let none = Place {
            state: Some(&state),
            control: &control_socket,
            socket: None,
        };
        assert!(hand("photo-booth", &step, &folder, &grants, none, 6, never).contains("#6306"));
        lamp_control.join().unwrap();
        set_on_here(None);
    }

    /// A skill the owner's computer pushed is handed over from its own
    /// folder in the developer folder (`skills-dev/<name>`), hashed there
    /// with the lamp's `.pushed.json` left out; a changed script asks
    /// again. No other folder is ever named.
    #[test]
    fn a_pushed_skills_task_is_handed_over_from_its_developer_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (root, dev) = (dir.path().join("skills"), dir.path().join("skills-dev"));
        let state = dir.path().join("state");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&state).unwrap();
        let pushed = folder(&dev);
        let unpushed = folder_sha256(&pushed).unwrap();
        fs::write(pushed.join(crate::PUSHED_FROM), r#"{"computer":"desk"}"#).unwrap();
        assert_eq!(folder_sha256(&pushed).unwrap(), unpushed);
        assert_eq!(
            folder_of(&root, Some(&dev), "photo-booth", true),
            Some(pushed.clone())
        );
        assert_eq!(
            folder_of(&root, Some(&dev), "photo-booth", false),
            Some(root.join("photo-booth"))
        );
        // No developer folder, or a name that leaves either: none.
        assert_eq!(folder_of(&root, None, "photo-booth", true), None);
        for name in ["../photo-booth", "a/b", ".", "..", "", ".hidden", "/etc"] {
            assert_eq!(folder_of(&root, Some(&dev), name, true), None, "{name}");
            assert_eq!(folder_of(&root, Some(&dev), name, false), None, "{name}");
        }
        let step = step(&pushed, "scripts/main.py").unwrap();
        let grants = vec!["light".to_owned()];
        let socket = dir.path().join("tasks.sock");
        let control_socket = dir.path().join("control.sock");
        let place = Place {
            state: Some(&state),
            control: &control_socket,
            socket: Some(&socket),
        };
        let lamp_control = control(
            &control_socket,
            vec![
                serde_json::json!({"agreed": true}),
                serde_json::json!({"agreed": false, "why": "changed"}),
            ],
        );
        set_on_here(Some(true));
        let mut sent = None;
        let said = hand(
            "pushed-booth",
            &step,
            &pushed,
            &grants,
            place,
            1,
            |_, line| {
                sent = Some(line.to_owned());
                answered("{\"ok\":true,\"id\":3}")
            },
        );
        assert_eq!(said, "do.task: handed to lamp-tasks: its run ended, task 3");
        let line: serde_json::Value = serde_json::from_str(&sent.unwrap()).unwrap();
        assert_eq!(line["folder"], pushed.to_string_lossy().as_ref());
        assert_eq!(line["sha256"], unpushed.as_str());
        fs::write(pushed.join(SCRIPTS).join("main.py"), "print('changed')\n").unwrap();
        let never = |_: &Path, _: &str| -> Result<String, String> { panic!("handed over") };
        assert_eq!(
            hand("pushed-booth", &step, &pushed, &grants, place, 2, never),
            CHANGED
        );
        let asked = lamp_control.join().unwrap();
        let check = |at: usize| asked[at][lamp_voice::skill_task_yes::CHECK]["sha256"].clone();
        assert_eq!(check(0), unpushed.as_str());
        assert_ne!(check(1), unpushed.as_str());
        set_on_here(None);
    }
}
