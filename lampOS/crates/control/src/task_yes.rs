//! The owner's yes to a skill's task (#6071 PR 2, #6305; `LAMP_SKILL_TASKS`,
//! off unless `on`), kept here and nowhere else (the first builder's ruling
//! on #6059: "The yes record goes to lamp-control, not lamp-skills' state").
//! It pays off once #6304 mints the one-run token here, from this record.
//!
//! - **Kept in `--state-dir`** ([`FILE`], `/var/lib/lampos-control`, this
//!   unit's own `StateDirectory=`, 0700, the file 0600): each yes is the
//!   skill's name, the grants said back, the SHA-256 of its folder as it was
//!   said back (`lamp_skills::task::folder_sha256`) and when it was given
//!   ([`Yes`]). Every other part runs as `lampos` too, so what keeps them
//!   from writing it by hand is each unit's: `lamp-skills` and the other
//!   `ProtectSystem=strict` parts have no write path here, and `lamp-tasks`
//!   (where a task's script runs, #6306), `lamp-body` and `lamp-eyes` hide
//!   the folder (`InaccessiblePaths=-/var/lib/lampos-control`;
//!   `lamp-update`'s `no_other_lampos_unit_can_write_lamp_controls_yes`
//!   holds every `lampos` unit to one of the two). The file is read and
//!   written never through a link.
//! - **Recorded by the voice alone** ([`RECORD`] on the owner's socket,
//!   `admin.sock`): only a peer in the voice's cgroup ([`VOICE`], by its
//!   `SO_PEERCRED` pid) may record, after the person's spoken yes to the
//!   say-back. Any other peer, `lamp-skills` included, is refused
//!   ([`NOT_THE_VOICE`]).
//! - **Read by anyone on that socket** ([`CHECK`]): agreed, waiting (no yes
//!   of that name) or changed (a yes for other bytes or other grants).
//! - **Taken back** ([`FORGET`]): by a remove of the skill (its manage
//!   helper). Taking a yes back grants nothing, so any peer may.
//! - **The one-run token** (#6304, [`KeptYes`]): lamp-control mints it
//!   from [`yes_for_token`] alone, for the folder hashed at that run.
use crate::protocol::Grant;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::Path;
use std::sync::Mutex;

/// The switch, as in `lamp-voice`, `lamp-skills` and its manage helper.
/// `on` only.
pub const ENVIRONMENT: &str = "LAMP_SKILL_TASKS";

/// Whether a skill's tasks are on in this process.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// What a yes request is told with the switch off.
pub const OFF: &str = "a skill's tasks are off on this lamp (LAMP_SKILL_TASKS is not on)";

/// In `--state-dir`: the owner's yeses to skills' tasks.
pub const FILE: &str = "task-yes.json";
/// The most yeses kept, every skill's together.
pub const MOST: usize = 50;

/// `{"task_yes_record": {"name", "sha256", "grants"}}`: the voice's, after
/// the person's spoken yes; answered `{"recorded": true}`.
pub const RECORD: &str = "task_yes_record";
/// `{"task_yes_check": {"name", "sha256", "grants"}}`: answered `{"agreed":
/// true}`, or `{"agreed": false, "why": "waiting" | "changed"}`.
pub const CHECK: &str = "task_yes_check";
/// `{"task_yes_forget": NAME}`: answered `{"forgot": BOOL}`.
pub const FORGET: &str = "task_yes_forget";

/// `lamp-voice`, where the system's manager puts it: the one peer that may
/// record a yes (as `lamp_skills::package::VOICE_LINK`).
pub const VOICE: &str = "/lampos.slice/lampos-voice.slice/lamp-voice.service";

/// What a record from any peer but the voice is told.
pub const NOT_THE_VOICE: &str = "only the voice records the person's yes to a skill's task";

/// One yes, as [`FILE`] keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Yes {
    /// The skill's name.
    pub name: String,
    /// The SHA-256 of its folder as said back, lowercase hex.
    pub sha256: String,
    /// The grants said back, lamp-control's names.
    pub grants: Vec<String>,
    /// When the person said yes, milliseconds since the epoch.
    pub agreed_unix_ms: u64,
}

/// What a record or a check names: a skill, its folder's SHA-256 and its
/// grants.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asked {
    pub name: String,
    pub sha256: String,
    pub grants: Vec<String>,
}

/// What the yeses kept say of a skill's task as it is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// A yes for exactly this folder and these grants.
    Agreed(Yes),
    /// No yes of this name.
    Waiting,
    /// A yes of this name, for other bytes or other grants.
    Changed,
}

/// The writers' lock: one record or forget at a time.
static WRITING: Mutex<()> = Mutex::new(());

/// A skill's name as lamp-skills takes one: 1 to 64 lowercase letters,
/// digits and `-`.
fn name_ok(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// A SHA-256 as 64 lowercase hex digits.
fn sha256_ok(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// `grants` as a script may hold them: each one of lamp-control's grants,
/// once each, never `skills`.
fn grants_ok(grants: &[String]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for grant in grants {
        match serde_json::from_value::<Grant>(Value::String(grant.clone())) {
            Ok(Grant::Skills) => return Err("grants: skills is never a script's".into()),
            Ok(known) if seen.insert(known) => {}
            Ok(_) => return Err(format!("grants: {grant} is listed twice")),
            Err(_) => return Err(format!("grants: {grant:?} is not a grant")),
        }
    }
    Ok(())
}

impl Asked {
    /// Why these fields name no skill's task, if they don't.
    fn refuses(&self) -> Option<String> {
        if !name_ok(&self.name) {
            return Some(format!("{:?} is not a skill's name", self.name));
        }
        if !sha256_ok(&self.sha256) {
            return Some("sha256: 64 lowercase hex digits".into());
        }
        grants_ok(&self.grants).err()
    }
}

/// The yeses kept in `state`; none when there are none, or they can't be
/// read (no yes is ever read from a damaged file, or through a link).
pub fn yeses(state: &Path) -> Vec<Yes> {
    use std::os::unix::fs::OpenOptionsExt as _;
    const MOST_BYTES: u64 = 64 * 1024;
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(state.join(FILE))
    else {
        return Vec::new();
    };
    if !file.metadata().is_ok_and(|meta| meta.is_file()) {
        return Vec::new();
    }
    let mut bytes = Vec::new();
    if file.take(MOST_BYTES + 1).read_to_end(&mut bytes).is_err() || bytes.len() as u64 > MOST_BYTES
    {
        return Vec::new();
    }
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// What the yeses in `state` say of the skill `name`, its folder hashed
/// `sha256` now and declaring `grants` (in any order).
pub fn verdict(state: &Path, name: &str, sha256: &str, grants: &[String]) -> Verdict {
    let set = |grants: &[String]| grants.iter().cloned().collect::<BTreeSet<_>>();
    match yeses(state).into_iter().find(|yes| yes.name == name) {
        None => Verdict::Waiting,
        Some(yes) if yes.sha256 == sha256 && set(&yes.grants) == set(grants) => {
            Verdict::Agreed(yes)
        }
        Some(_) => Verdict::Changed,
    }
}

/// The yes #6304's one-run token is minted from, and nothing else: the
/// owner's yes in `state` for exactly the skill `name`, its folder as
/// hashed at that run (`sha256`) and `grants`; none means no token.
pub fn yes_for_token(state: &Path, name: &str, sha256: &str, grants: &[String]) -> Option<Yes> {
    match verdict(state, name, sha256, grants) {
        Verdict::Agreed(yes) => Some(yes),
        Verdict::Waiting | Verdict::Changed => None,
    }
}

/// The lamp's skills, one folder per name (`lamp_tasks::skill_task::SKILLS`):
/// where a skill's run is hashed at its mint, or, for a name not there, the
/// pushed skills beside it ([`PUSHED_NAME`]); never another place.
pub const SKILLS: &str = "/var/lib/lampOS/skills";
/// The folder beside [`SKILLS`] the owner's own paired computer pushes
/// skills into (`lamp_skills::PUSHED_DIR`, `/var/lib/lampOS/skills-dev`).
pub const PUSHED_NAME: &str = "skills-dev";

/// The folder of the skill `name` (a plain name) a mint hashes: under
/// `skills` when anything of that name is there (as lamp-skills, a pushed
/// skill never takes the place of one of the lamp's), else the pushed
/// skills' folder beside it. The one rule lamp-skills and lamp-tasks hold
/// (`lamp_skills::task::lamp_has_skill_named`).
fn folder_of(skills: &Path, name: &str) -> std::path::PathBuf {
    if lamp_skills::task::lamp_has_skill_named(skills, name) {
        return skills.join(name);
    }
    skills.with_file_name(PUSHED_NAME).join(name)
}

/// What a mint for a skill with no yes is told.
pub const NO_YES: &str = "the owner hasn't said yes to this skill's task";
/// What a mint for a skill whose folder or grants changed since the yes is
/// told.
pub const CHANGED: &str =
    "this skill's script or grants changed since the owner's yes, so it asks again";

/// The owner's yes as the one-run token reads it (#6304, `runs::SkillYes`):
/// at each mint lamp-control hashes the skill's folder in `skills`, or a
/// pushed one's beside it ([`folder_of`]), itself
/// (`lamp_skills::task::folder_sha256`, never through a link), reads the
/// grants it declares now, and names them only if [`yes_for_token`] holds a
/// yes in `state` for exactly that hash and those grants. With `on` false
/// ([`ENVIRONMENT`] not on in lamp-control), nothing is minted ([`OFF`]).
pub struct KeptYes {
    pub state: std::path::PathBuf,
    pub skills: std::path::PathBuf,
    pub on: bool,
    /// `LAMP_SKILL_WRITE_AGENT=on` ([`WRITE_AGENT_ENVIRONMENT`]): a skill
    /// with no yes here whose only step is a fixed write
    /// (`lamp_skills::service_write::fixed_write_of`) is minted its one
    /// grant, its service's `-write` ([`KeptYes::fixed_write_grant`]).
    pub write_agent: bool,
}

/// A made skill's fixed write run through the lamp's agent (the owner's
/// ruling, 13 Nov): `on` only, here and in lamp-skills, lamp-tasks and
/// lamp-services; on in no product unit.
pub const WRITE_AGENT_ENVIRONMENT: &str = lamp_skills::service_write::AGENT_ENVIRONMENT;

/// Whether [`WRITE_AGENT_ENVIRONMENT`] is on in this process.
pub fn write_agent_on() -> bool {
    std::env::var(WRITE_AGENT_ENVIRONMENT).ok().as_deref() == Some("on")
}

impl KeptYes {
    /// The one grant of `skill`'s fixed write's run, with
    /// [`KeptYes::write_agent`] and no task yes kept for it: its service's
    /// `-write`, read from its folder ([`folder_of`]). No yes is read here:
    /// the owner's yes to that write is lamp-services', kept in root's store
    /// and matched there on its exact content at the send, once a run.
    fn fixed_write_grant(&self, skill: &str) -> Option<Grant> {
        if !self.write_agent
            || !name_ok(skill)
            || yeses(&self.state).iter().any(|yes| yes.name == skill)
        {
            return None;
        }
        let folder = folder_of(&self.skills, skill);
        if !std::fs::symlink_metadata(&folder).is_ok_and(|meta| meta.is_dir()) {
            return None;
        }
        let write = lamp_skills::service_write::fixed_write_of(&folder)?;
        crate::protocol::ServiceGrant::of(&write.service, crate::protocol::Access::Write)
            .map(Grant::Service)
    }
}

impl crate::runs::SkillYes for KeptYes {
    fn grants(&self, skill: &str) -> Result<BTreeSet<Grant>, String> {
        if !self.on {
            return Err(OFF.into());
        }
        if !name_ok(skill) {
            return Err(format!("{skill:?} is not a skill's name"));
        }
        if !yeses(&self.state).iter().any(|yes| yes.name == skill) {
            return match self.fixed_write_grant(skill) {
                Some(grant) => Ok(BTreeSet::from([grant])),
                None => Err(NO_YES.into()),
            };
        }
        let folder = folder_of(&self.skills, skill);
        if !std::fs::symlink_metadata(&folder).is_ok_and(|meta| meta.is_dir()) {
            return Err("this skill is not on the lamp".into());
        }
        let sha256 = lamp_skills::task::folder_sha256(&folder)
            .map_err(|why| format!("this skill's folder can't be read: {why}"))?;
        let declared = lamp_skills::load_with(&folder, false)
            .map_err(|why| format!("this skill doesn't load: {why}"))?
            .grants;
        let yes = yes_for_token(&self.state, skill, &sha256, &declared)
            .ok_or_else(|| CHANGED.to_owned())?;
        grants_ok(&yes.grants)?;
        yes.grants
            .iter()
            .map(|grant| {
                serde_json::from_value::<Grant>(Value::String(grant.clone()))
                    .map_err(|_| format!("grants: {grant:?} is not a grant"))
            })
            .collect()
    }

    fn fixed_write(&self, skill: &str) -> bool {
        self.on && self.fixed_write_grant(skill).is_some()
    }
}

/// The yeses in `state` written whole: beside it, synced, renamed over;
/// 0600.
fn keep(state: &Path, kept: &[Yes]) -> Result<(), String> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let path = state.join(FILE);
    let temporary = state.join(format!("{FILE}.new"));
    let failed = |error: std::io::Error| format!("{}: {error}", path.display());
    let mut json = serde_json::to_vec_pretty(kept).map_err(|error| error.to_string())?;
    json.push(b'\n');
    // A file left beside it by a cut write goes first (a link is removed,
    // never followed); then a new one, never through a link.
    match std::fs::remove_file(&temporary) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(failed(error)),
        _ => {}
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o600)
        .open(&temporary)
        .map_err(failed)?;
    file.write_all(&json).map_err(failed)?;
    file.sync_all().map_err(failed)?;
    std::fs::rename(&temporary, &path).map_err(failed)?;
    std::fs::File::open(state)
        .and_then(|folder| folder.sync_all())
        .map_err(failed)
}

/// The person's yes to `asked` kept in `state` at `now_ms`, in the place of
/// any of its name.
pub fn record(state: &Path, asked: &Asked, now_ms: u64) -> Result<(), String> {
    if let Some(why) = asked.refuses() {
        return Err(why);
    }
    let _held = WRITING.lock().unwrap_or_else(|poison| poison.into_inner());
    let mut kept: Vec<Yes> = yeses(state)
        .into_iter()
        .filter(|yes| yes.name != asked.name)
        .collect();
    if kept.len() >= MOST {
        return Err(format!("at most {MOST} skills' tasks are agreed on a lamp"));
    }
    kept.push(Yes {
        name: asked.name.clone(),
        sha256: asked.sha256.clone(),
        grants: asked.grants.clone(),
        agreed_unix_ms: now_ms,
    });
    keep(state, &kept)
}

/// `name`'s yes taken back from `state`; whether there was one.
pub fn forget(state: &Path, name: &str) -> Result<bool, String> {
    let _held = WRITING.lock().unwrap_or_else(|poison| poison.into_inner());
    let all = yeses(state);
    if !all.iter().any(|yes| yes.name == name) {
        return Ok(false);
    }
    let kept: Vec<Yes> = all.into_iter().filter(|yes| yes.name != name).collect();
    keep(state, &kept).map(|()| true)
}

/// Whether `/proc/<pid>/cgroup`'s text puts the process in the voice's
/// unit (or a cgroup under it): the unified hierarchy's line (`0::…`), or
/// systemd's own on a hybrid one (`…:name=systemd:…`).
pub fn from_voice(cgroup: &str) -> bool {
    cgroup.lines().any(|line| {
        let mut fields = line.splitn(3, ':');
        let (Some(_), Some(controllers), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            return false;
        };
        (controllers.is_empty() || controllers == "name=systemd")
            && path
                .strip_prefix(VOICE)
                .is_some_and(|below| below.is_empty() || below.starts_with('/'))
    })
}

/// Whether the peer with `pid` (its `SO_PEERCRED`) is the voice, by its
/// cgroup; never on a system without `/proc`.
pub fn peer_is_voice(pid: Option<i32>) -> bool {
    pid.filter(|pid| *pid > 0)
        .and_then(|pid| std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok())
        .is_some_and(|cgroup| from_voice(&cgroup))
}

/// The answer on the owner's socket to a yes request in `asked`, kept in
/// `state`, from a peer that is the voice or not, with the switch `on`, at
/// `now_ms`; none for any other request.
pub fn answer(
    state: Option<&Path>,
    asked: &Map<String, Value>,
    from_the_voice: bool,
    on: bool,
    now_ms: u64,
) -> Option<Value> {
    let fields = |value: &Value| serde_json::from_value::<Asked>(value.clone());
    let no_state = || json!({"error": "no state folder for the owner's yeses"});
    if let Some(value) = asked.get(FORGET) {
        // Taking a yes back grants nothing: any peer, the switch on or off.
        let Value::String(name) = value else {
            return Some(json!({"error": "task_yes_forget: a skill's name"}));
        };
        let Some(state) = state else {
            return Some(no_state());
        };
        return Some(match forget(state, name) {
            Ok(forgot) => {
                if forgot {
                    eprintln!("lamp-control: the owner's yes to {name}'s task is taken back");
                }
                json!({"forgot": forgot})
            }
            Err(why) => json!({"error": why}),
        });
    }
    let (key, value) = [RECORD, CHECK]
        .into_iter()
        .find_map(|key| asked.get(key).map(|value| (key, value)))?;
    if !on {
        return Some(json!({"error": OFF}));
    }
    let Some(state) = state else {
        return Some(no_state());
    };
    let Ok(asked) = fields(value) else {
        return Some(json!({"error": format!("{key}: {{\"name\", \"sha256\", \"grants\"}}")}));
    };
    if let Some(why) = asked.refuses() {
        return Some(json!({"error": why}));
    }
    if key == CHECK {
        return Some(
            match verdict(state, &asked.name, &asked.sha256, &asked.grants) {
                Verdict::Agreed(_) => json!({"agreed": true}),
                Verdict::Waiting => json!({"agreed": false, "why": "waiting"}),
                Verdict::Changed => json!({"agreed": false, "why": "changed"}),
            },
        );
    }
    if !from_the_voice {
        eprintln!(
            "lamp-control: a yes to {}'s task refused: not from the voice",
            asked.name
        );
        return Some(json!({"error": NOT_THE_VOICE}));
    }
    Some(match record(state, &asked, now_ms) {
        Ok(()) => {
            eprintln!(
                "lamp-control: the owner's yes to {}'s task kept ({} grant(s))",
                asked.name,
                asked.grants.len()
            );
            json!({"recorded": true})
        }
        Err(why) => json!({"error": why}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(key: &str, value: Value) -> Map<String, Value> {
        let mut asked = Map::new();
        asked.insert(key.to_owned(), value);
        asked
    }

    /// A fresh folder of this test's own.
    fn folder(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lamp-control-task-yes-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn booth(sha256: &str, grants: Value) -> Value {
        json!({"name": "photo-booth", "sha256": sha256, "grants": grants})
    }

    /// Only the voice records; anyone may check; the yes holds for its
    /// folder's SHA-256 and grants in any order, and is kept with its time.
    #[test]
    fn only_the_voice_records_and_the_yes_holds_for_its_sha256_and_grants() {
        let dir = folder("holds");
        let state = Some(dir.as_path());
        let sha256 = "ab".repeat(32);
        let grants = json!(["camera", "light"]);
        let check = |value: Value| answer(state, &asked(CHECK, value), false, true, 0).unwrap();
        assert_eq!(
            check(booth(&sha256, grants.clone())),
            json!({"agreed": false, "why": "waiting"})
        );
        let refused = answer(
            state,
            &asked(RECORD, booth(&sha256, grants.clone())),
            false,
            true,
            5,
        )
        .unwrap();
        assert_eq!(refused["error"], NOT_THE_VOICE);
        assert!(yeses(dir.as_path()).is_empty());
        let kept = answer(
            state,
            &asked(RECORD, booth(&sha256, grants.clone())),
            true,
            true,
            5,
        )
        .unwrap();
        assert_eq!(kept, json!({"recorded": true}));
        assert_eq!(yeses(dir.as_path())[0].agreed_unix_ms, 5);
        assert_eq!(
            check(booth(&sha256, json!(["light", "camera"]))),
            json!({"agreed": true})
        );
        assert_eq!(
            check(booth(&sha256, json!(["camera"]))),
            json!({"agreed": false, "why": "changed"})
        );
        assert_eq!(
            check(booth(&"cd".repeat(32), grants.clone())),
            json!({"agreed": false, "why": "changed"})
        );
        let grant_list = ["camera".to_owned(), "light".to_owned()];
        assert!(yes_for_token(dir.as_path(), "photo-booth", &sha256, &grant_list).is_some());
        assert!(
            yes_for_token(dir.as_path(), "photo-booth", &"cd".repeat(32), &grant_list).is_none()
        );
        // The file is lamp-control's alone.
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(dir.as_path().join(FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        // Taken back by any peer.
        assert_eq!(
            answer(state, &asked(FORGET, json!("photo-booth")), false, true, 0),
            Some(json!({"forgot": true}))
        );
        assert_eq!(
            check(booth(&sha256, grants)),
            json!({"agreed": false, "why": "waiting"})
        );
    }

    /// A skill folder with a `do.task` and `grants`, as lamp-skills loads one.
    fn task_skill(skills: &Path, name: &str, grants: &str) -> std::path::PathBuf {
        let folder = skills.join(name);
        std::fs::create_dir_all(folder.join("scripts")).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Takes a photo for me.\n---\n"),
        )
        .unwrap();
        std::fs::write(
            folder.join("skill.toml"),
            format!(
                "version = \"1.0.0\"\ngrants = {grants}\ntool = \"photo_booth\"\n\
                 [[do]]\ntask = {{ task = \"Take a photo\", run = \"scripts/main.py\" }}\n"
            ),
        )
        .unwrap();
        std::fs::write(folder.join("scripts/main.py"), "print('cheese')\n").unwrap();
        folder
    }

    /// LAMP_AGENT_SERVICES (#6432): the owner's yes to a connector skill
    /// covers its service grants as any other: kept, held to exactly the
    /// grants declared, and named as `Grant::Service` for the token; an
    /// unknown service's grant is never recorded.
    #[test]
    fn the_yes_covers_a_skills_service_grants() {
        let dir = folder("services");
        let state = dir.join("state");
        std::fs::create_dir_all(&state).unwrap();
        let sha256 = "ab".repeat(32);
        let record = |grants: Value| {
            answer(
                Some(state.as_path()),
                &asked(
                    RECORD,
                    json!({"name": "morning-mail", "sha256": sha256, "grants": grants}),
                ),
                true,
                true,
                5,
            )
            .unwrap()
        };
        assert_eq!(
            record(json!(["gmail-read", "gmail-write"])),
            json!({"recorded": true})
        );
        let declared = ["gmail-write".to_owned(), "gmail-read".to_owned()];
        let yes = yes_for_token(&state, "morning-mail", &sha256, &declared).unwrap();
        let named: BTreeSet<Grant> = yes
            .grants
            .iter()
            .map(|grant| serde_json::from_value(Value::String(grant.clone())).unwrap())
            .collect();
        assert_eq!(named.len(), 2);
        assert!(named.iter().all(|grant| matches!(grant, Grant::Service(_))));
        // A yes for the read alone names no token for a skill that also sends.
        assert_eq!(record(json!(["gmail-read"])), json!({"recorded": true}));
        assert!(yes_for_token(&state, "morning-mail", &sha256, &declared).is_none());
        // An unknown service, or a second code, is never a grant.
        for bad in ["myspace-read", "gmail-app-read"] {
            assert!(record(json!([bad]))["error"].is_string(), "{bad}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #6304: a mint gets exactly the yes's grants for the folder as
    /// lamp-control hashes it; no yes, a changed script, more grants
    /// declared, a folder that is a link, or the switch off: refused.
    #[test]
    fn a_token_is_minted_only_from_the_yes_for_the_folder_as_it_is() {
        use crate::runs::SkillYes as _;
        let dir = folder("kept");
        let state = dir.join("state");
        let skills = dir.join("skills");
        std::fs::create_dir_all(&state).unwrap();
        let booth = task_skill(&skills, "photo-booth", r#"["camera", "light"]"#);
        let kept = KeptYes {
            state: state.clone(),
            skills: skills.clone(),
            on: true,
            write_agent: false,
        };
        assert_eq!(kept.grants("photo-booth").unwrap_err(), NO_YES);
        let sha256 = lamp_skills::task::folder_sha256(&booth).unwrap();
        let said = |grants: Value| {
            answer(
                Some(state.as_path()),
                &asked(
                    RECORD,
                    json!({"name": "photo-booth", "sha256": sha256, "grants": grants}),
                ),
                true,
                true,
                5,
            )
            .unwrap()
        };
        assert_eq!(said(json!(["light", "camera"])), json!({"recorded": true}));
        assert_eq!(
            kept.grants("photo-booth").unwrap(),
            BTreeSet::from([Grant::Camera, Grant::Light])
        );
        // Off: nothing, whatever is kept.
        let off = KeptYes {
            state: state.clone(),
            skills: skills.clone(),
            on: false,
            write_agent: false,
        };
        assert_eq!(off.grants("photo-booth").unwrap_err(), OFF);
        // A yes for fewer grants than the folder declares names none of them.
        assert_eq!(said(json!(["light"])), json!({"recorded": true}));
        assert_eq!(kept.grants("photo-booth").unwrap_err(), CHANGED);
        assert_eq!(said(json!(["camera", "light"])), json!({"recorded": true}));
        // The script changed since the yes: asks again.
        std::fs::write(booth.join("scripts/main.py"), "print('smile')\n").unwrap();
        assert_eq!(kept.grants("photo-booth").unwrap_err(), CHANGED);
        std::fs::write(booth.join("scripts/main.py"), "print('cheese')\n").unwrap();
        assert!(kept.grants("photo-booth").is_ok());
        // More grants declared since the yes: asks again.
        task_skill(&skills, "photo-booth", r#"["camera", "light", "body"]"#);
        assert_eq!(kept.grants("photo-booth").unwrap_err(), CHANGED);
        task_skill(&skills, "photo-booth", r#"["camera", "light"]"#);
        assert!(kept.grants("photo-booth").is_ok());
        // A folder that is a link is never hashed, nor a skill not there.
        let elsewhere = dir.join("elsewhere");
        std::fs::rename(&booth, &elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &booth).unwrap();
        assert_eq!(
            kept.grants("photo-booth").unwrap_err(),
            "this skill is not on the lamp"
        );
        std::fs::remove_file(&booth).unwrap();
        assert_eq!(
            kept.grants("photo-booth").unwrap_err(),
            "this skill is not on the lamp"
        );
        assert!(kept.grants("../photo-booth").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// LAMP_SKILL_WRITE_AGENT (the owner's ruling, 13 Nov): a skill whose
    /// only step is a fixed write, with no task yes here, is minted its one
    /// grant, its service's `-write`, and marked a fixed write's; off, or
    /// for a skill with any other step, nothing.
    #[test]
    fn a_fixed_writes_run_is_minted_its_one_write_grant() {
        use crate::runs::SkillYes as _;
        let dir = folder("fixed");
        let state = dir.join("state");
        let skills = dir.join("skills");
        std::fs::create_dir_all(&state).unwrap();
        let arrive = skills.join("arrive-mail");
        std::fs::create_dir_all(&arrive).unwrap();
        std::fs::write(
            arrive.join("SKILL.md"),
            "---\nname: arrive-mail\ndescription: Mails my boss.\n---\n",
        )
        .unwrap();
        std::fs::write(
            arrive.join("skill.toml"),
            "version = \"1.0.0\"\n[permissions]\nservices = { gmail-app = \"write\" }\n\
             [[do]]\nwrite = { service = \"gmail-app\", action = \"send_mail\", \
             to = [\"boss@work.test\"], subject = \"In\", text = \"I'm at my desk.\" }\n",
        )
        .unwrap();
        let kept = |on: bool, write_agent: bool| KeptYes {
            state: state.clone(),
            skills: skills.clone(),
            on,
            write_agent,
        };
        let gmail_write = Grant::Service(
            crate::protocol::ServiceGrant::of("gmail", crate::protocol::Access::Write).unwrap(),
        );
        assert_eq!(
            kept(true, true).grants("arrive-mail").unwrap(),
            BTreeSet::from([gmail_write])
        );
        assert!(kept(true, true).fixed_write("arrive-mail"));
        // Off, or the skills' tasks off: nothing, as before.
        assert_eq!(kept(true, false).grants("arrive-mail").unwrap_err(), NO_YES);
        assert!(!kept(true, false).fixed_write("arrive-mail"));
        assert_eq!(kept(false, true).grants("arrive-mail").unwrap_err(), OFF);
        assert!(!kept(false, true).fixed_write("arrive-mail"));
        // A task's skill is never a fixed write's: its own yes, as before.
        task_skill(&skills, "photo-booth", r#"["camera"]"#);
        assert_eq!(kept(true, true).grants("photo-booth").unwrap_err(), NO_YES);
        assert!(!kept(true, true).fixed_write("photo-booth"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A skill the owner's computer pushed (`skills-dev/<name>`, beside the
    /// lamp's skills): minted from the yes for its folder as lamp-control
    /// hashes it there, with the same checks. A changed script asks again,
    /// a lamp skill of the same name is the one hashed, and a link or a
    /// folder anywhere else is never hashed.
    #[test]
    fn a_pushed_skills_token_is_minted_from_the_yes_for_its_own_folder() {
        use crate::runs::SkillYes as _;
        let dir = folder("pushed");
        let state = dir.join("state");
        let skills = dir.join("skills");
        let pushed = dir.join(PUSHED_NAME);
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(&skills).unwrap();
        let booth = task_skill(&pushed, "photo-booth", r#"["camera", "light"]"#);
        std::fs::write(booth.join(".pushed.json"), r#"{"computer":"desk"}"#).unwrap();
        let kept = KeptYes {
            state: state.clone(),
            skills: skills.clone(),
            on: true,
            write_agent: false,
        };
        assert_eq!(folder_of(&skills, "photo-booth"), booth);
        assert_eq!(kept.grants("photo-booth").unwrap_err(), NO_YES);
        let sha256 = lamp_skills::task::folder_sha256(&booth).unwrap();
        let recorded = answer(
            Some(state.as_path()),
            &asked(
                RECORD,
                json!({"name": "photo-booth", "sha256": sha256, "grants": ["camera", "light"]}),
            ),
            true,
            true,
            5,
        );
        assert_eq!(recorded, Some(json!({"recorded": true})));
        assert_eq!(
            kept.grants("photo-booth").unwrap(),
            BTreeSet::from([Grant::Camera, Grant::Light])
        );
        // The script changed since the yes: asks again.
        std::fs::write(booth.join("scripts/main.py"), "print('smile')\n").unwrap();
        assert_eq!(kept.grants("photo-booth").unwrap_err(), CHANGED);
        std::fs::write(booth.join("scripts/main.py"), "print('cheese')\n").unwrap();
        assert!(kept.grants("photo-booth").is_ok());
        // A lamp skill of that name is the one hashed, never the pushed one.
        let own = task_skill(&skills, "photo-booth", r#"["camera", "light"]"#);
        std::fs::write(own.join("scripts/main.py"), "print('lamp')\n").unwrap();
        assert_eq!(folder_of(&skills, "photo-booth"), own);
        assert_eq!(kept.grants("photo-booth").unwrap_err(), CHANGED);
        std::fs::remove_dir_all(&own).unwrap();
        assert!(kept.grants("photo-booth").is_ok());
        // A pushed folder that is a link is never hashed.
        let elsewhere = dir.join("elsewhere");
        std::fs::rename(&booth, &elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &booth).unwrap();
        assert_eq!(
            kept.grants("photo-booth").unwrap_err(),
            "this skill is not on the lamp"
        );
        // Nor one outside both: not there, or named out of them.
        std::fs::remove_file(&booth).unwrap();
        assert_eq!(
            kept.grants("photo-booth").unwrap_err(),
            "this skill is not on the lamp"
        );
        assert!(kept.grants("../elsewhere").is_err());
        assert!(kept.grants("skills-dev/photo-booth").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A yes file that is a link is never read, and a write never follows
    /// a link left where its new file goes.
    #[test]
    fn the_yes_file_is_never_read_or_written_through_a_link() {
        let dir = folder("link");
        let elsewhere = dir.join("elsewhere.json");
        let yes = json!([{"name": "photo-booth", "sha256": "ab".repeat(32),
            "grants": ["light"], "agreed_unix_ms": 1}]);
        std::fs::write(&elsewhere, yes.to_string()).unwrap();
        std::os::unix::fs::symlink(&elsewhere, dir.join(FILE)).unwrap();
        assert!(yeses(&dir).is_empty());
        std::fs::remove_file(dir.join(FILE)).unwrap();
        let target = dir.join("target.json");
        std::os::unix::fs::symlink(&target, dir.join(format!("{FILE}.new"))).unwrap();
        let asked = Asked {
            name: "photo-booth".into(),
            sha256: "ab".repeat(32),
            grants: vec!["light".into()],
        };
        record(&dir, &asked, 2).unwrap();
        assert!(!target.exists());
        assert_eq!(yeses(&dir)[0].agreed_unix_ms, 2);
    }

    /// Off, nothing is recorded or checked; bad fields and grants are
    /// refused by name; other requests are not these.
    #[test]
    fn off_or_bad_fields_record_nothing() {
        let dir = folder("off");
        let state = Some(dir.as_path());
        let sha256 = "ab".repeat(32);
        let off = answer(
            state,
            &asked(RECORD, booth(&sha256, json!(["light"]))),
            true,
            false,
            0,
        )
        .unwrap();
        assert_eq!(off["error"], OFF);
        for (value, why) in [
            (booth("ab", json!(["light"])), "sha256"),
            (booth(&sha256, json!(["skills"])), "never a script's"),
            (booth(&sha256, json!(["root"])), "is not a grant"),
            (booth(&sha256, json!(["light", "light"])), "listed twice"),
            (
                json!({"name": "../x", "sha256": sha256, "grants": []}),
                "not a skill's name",
            ),
            (json!({"name": "x"}), "task_yes_record"),
        ] {
            let refused = answer(state, &asked(RECORD, value), true, true, 0).unwrap();
            let refused = refused["error"].as_str().unwrap().to_owned();
            assert!(refused.contains(why), "{refused}");
        }
        assert!(yeses(dir.as_path()).is_empty());
        assert!(answer(state, &asked("clients", json!(true)), true, true, 0).is_none());
    }

    /// The voice and lamp-skills ask with these very names, and the voice's
    /// cgroup is the one lamp-skills' manage helper knows it by.
    #[test]
    fn the_askers_use_these_names() {
        use lamp_voice::skill_task_yes as voice;
        assert_eq!(
            (voice::RECORD, voice::CHECK, voice::FORGET),
            (RECORD, CHECK, FORGET)
        );
        assert_eq!(voice::ENVIRONMENT, ENVIRONMENT);
        assert_eq!(voice::OFF, OFF);
        assert_eq!(VOICE, lamp_skills::package::VOICE_LINK);
        assert!(voice::CONTROL_SOCKET.ends_with("/admin.sock"));
        let grants: Vec<&str> = voice::GRANTS.iter().map(|(grant, _)| *grant).collect();
        assert!(grants_ok(&grants.iter().map(|g| g.to_string()).collect::<Vec<_>>()).is_ok());
    }

    /// The voice's cgroup, by the line systemd writes, and nothing beside it.
    #[test]
    fn the_voice_is_known_by_its_cgroup() {
        assert!(from_voice(&format!("0::{VOICE}\n")));
        assert!(from_voice(&format!("1:name=systemd:{VOICE}/child\n")));
        assert!(!from_voice(
            "0::/lampos.slice/lampos-parts.slice/lamp-skills.service\n"
        ));
        assert!(!from_voice(&format!("0::{VOICE}-not\n")));
        assert!(!from_voice(&format!("4:cpu:{VOICE}\n")));
        assert!(!peer_is_voice(None));
    }
}
