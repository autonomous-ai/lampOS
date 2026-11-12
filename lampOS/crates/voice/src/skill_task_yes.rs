//! A skill's `do.task` and its `grants` said back, and the person's yes
//! to them (#6071 PR 2, #6305; `LAMP_SKILL_TASKS`, off unless `on`).
//!
//! A skill from the Skills Store, or pushed by the owner's own paired
//! computer, may carry a `scripts/` folder and one `do.task` step that
//! hands that work to the lamp's agent (`lamp_skills::task`). Its
//! `grants = [...]` name what the script may reach through the lamp's
//! control socket, each one of lamp-control's grants ([`GRANTS`]).
//!
//! - **Said back in plain words.** Asked what such a skill uses ("what does
//!   my photo booth skill use?", `make_skill`'s `read_back`), the lamp says
//!   its task, where its script came from and each grant in plain words
//!   ("it uses the camera") ([`line`]), from what the manage helper reads of
//!   the skill on the lamp now (`{"task_asks": NAME}`, [`asks_by`]), and
//!   whether lamp-control keeps a yes for exactly that ([`agreed_by`]).
//! - **The yes covers exactly that, and lamp-control keeps it.** The yes
//!   is taken as a made skill's write yes is ([`crate::skill_write_yes`]:
//!   said whole and uncut first, a later heard line, the echo held, the
//!   route with the echo judge). The voice reads the skill on the lamp
//!   again, and only while its folder still has the SHA-256 and the grants
//!   said back does it ask lamp-control to record the yes
//!   (`{"task_yes_record": {"name", "sha256", "grants"}}` on its owner's
//!   socket, [`CONTROL_SOCKET`], [`approve_by`]), which takes it from the
//!   voice's cgroup alone and keeps it in its own state folder (the first
//!   builder's ruling on #6059). No other part writes it: `lamp-skills`
//!   and the other `ProtectSystem=strict` parts have no write path there,
//!   and the other `lampos` parts hide the folder (`InaccessiblePaths=`).
//!   A script changed since asks
//!   again; `lamp-skills` hands a run over only while lamp-control says the
//!   yes holds for the folder as it is at that run (`{"task_yes_check":
//!   …}`, [`agreed_by`]).
//! - **No identity check** (the owner, 10 Nov): whoever is at the lamp is
//!   its owner, as for a write's yes.
use crate::make_skill_local::{Asking, READ_BACK_NOT_ASKED};
use crate::skill_write_yes::{self, Asks};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::{Duration, Instant};

/// The switch, in `lamp-voice`, `lamp-skills` and its manage helper. `on`
/// only.
pub const ENVIRONMENT: &str = "LAMP_SKILL_TASKS";

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

/// What a skill's tasks are told with the switch off.
pub const OFF: &str = "a skill's tasks are off on this lamp (LAMP_SKILL_TASKS is not on)";

/// lamp-control's owner's socket (`lamp-control serve --admin-socket`),
/// where the owner's yeses to skills' tasks are recorded and checked
/// (`lamp_control::task_yes`).
pub const CONTROL_SOCKET: &str = "/run/lampos-control/admin.sock";
/// The voice's request to lamp-control after the person's yes:
/// `{"task_yes_record": {"name", "sha256", "grants"}}`.
pub const RECORD: &str = "task_yes_record";
/// Anyone's question to lamp-control: `{"task_yes_check": {"name",
/// "sha256", "grants"}}`, answered `{"agreed": BOOL, "why"?}`.
pub const CHECK: &str = "task_yes_check";
/// A remove's word to lamp-control: `{"task_yes_forget": NAME}`.
pub const FORGET: &str = "task_yes_forget";
/// What a yes for a skill no longer as it was said back is told: nothing
/// is recorded.
pub const NOT_SAID_BACK: &str =
    "the skill on the lamp is not the one said back: nothing was agreed";

/// The grants a `do.task`'s script may hold, as lamp-control's protocol
/// names them (`lamp_control::protocol::Grant`), each in the plain words
/// the say-back uses: the five a pushed skill's own permissions may ask
/// say what `lamp_skills::package::grants_asked` says of them. `skills`
/// is never one: a script never puts skills on the lamp ([`NEVER`]).
pub const GRANTS: [(&str, &str); 12] = [
    ("body", "moves the lamp's body"),
    ("light", "changes the light"),
    ("speak", "speaks and plays sounds"),
    ("look", "knows when someone is there, never who"),
    ("hear", "hears its own phrases, never the words around them"),
    ("sound", "hears kinds of sound, never words"),
    ("air", "reads the air sensor"),
    (
        "device",
        "reads the lamp's processor, memory, disk and temperature",
    ),
    ("camera", "uses the camera"),
    ("microphone", "uses the microphone"),
    ("notes", "keeps small notes of its own on the lamp"),
    ("speech", "reads everything the lamp says out loud"),
];

/// lamp-control's grant a script never holds: putting skills on the lamp.
pub const NEVER: &str = "skills";

/// `name`'s plain words, for a grant of [`GRANTS`].
pub fn grant_words(name: &str) -> Option<&'static str> {
    GRANTS
        .iter()
        .find(|(grant, _)| *grant == name)
        .map(|(_, words)| *words)
}

/// `name` as [`GRANTS`] has it, for a grant of it.
pub fn grant_named(name: &str) -> Option<&'static str> {
    GRANTS
        .iter()
        .find(|(grant, _)| *grant == name)
        .map(|(grant, _)| *grant)
}

/// What the script uses, every grant in plain words: "it uses the camera
/// and changes the light". A grant not in [`GRANTS`] is said by its name,
/// never left out (the manage helper gives only known ones).
pub fn uses_said(grants: &[String]) -> String {
    let words: Vec<String> = grants
        .iter()
        .map(|grant| grant_words(grant).map_or_else(|| format!("uses {grant}"), str::to_owned))
        .collect();
    match words.as_slice() {
        [] => "it uses none of the lamp's parts".to_owned(),
        [one] => format!("it {one}"),
        [rest @ .., last] => format!("it {} and {last}", rest.join(", ")),
    }
}

/// A skill on the lamp with a `do.task`, as the manage helper reads it
/// now (`{"task_asks": NAME}`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskAsks {
    pub name: String,
    /// The SHA-256 of its folder as read, `lamp_skills::task::folder_sha256`.
    pub sha256: String,
    pub grants: Vec<String>,
    /// The step's `task`, the skill file's own words.
    pub task: String,
    /// The SHA-256 of the package it went on as (its `.package.json`), none
    /// for one put on from a folder or pushed.
    pub package: Option<String>,
    /// Where its script came from: "the Skills Store", or "your computer
    /// NAME".
    pub from: String,
    /// Whether lamp-control keeps the owner's yes for exactly this folder
    /// and grants ([`agreed_by`]); the manage helper never says.
    pub approved: bool,
}

impl TaskAsks {
    /// The manage helper's `done` for `task_asks`: none for a skill with
    /// no `do.task` (`{"task": null}`), or an answer it can't read.
    pub fn of_json(done: &Value) -> Option<Self> {
        let text = |key: &str| done.get(key).and_then(Value::as_str).map(str::to_owned);
        let grants = done
            .get("grants")?
            .as_array()?
            .iter()
            .map(|grant| grant.as_str().map(str::to_owned))
            .collect::<Option<Vec<String>>>()?;
        let sha256 = text("sha256")?;
        (sha256.len() == 64).then_some(())?;
        Some(Self {
            name: text("name")?,
            sha256,
            grants,
            task: text("task")?,
            package: text("package"),
            from: text("from")?,
            approved: false,
        })
    }

    /// What the yes is asked of: what must be said whole, then kept.
    fn asks(&self) -> Asks {
        Asks {
            skill: self.name.clone(),
            sha256: self.sha256.clone(),
            writes: Vec::new(),
            say_back: said(self),
            grants: Some(self.grants.clone()),
            code: false,
        }
    }
}

/// What must be heard whole before the yes: the task, where its script came
/// from, and every grant in plain words ([`uses_said`]).
fn said(asks: &TaskAsks) -> String {
    format!(
        "{} hands one task to the lamp's agent: “{}”. Its script comes from {}, and {}.",
        crate::made_skill::spoken_name(&asks.name),
        asks.task,
        asks.from,
        uses_said(&asks.grants)
    )
}

/// The say-back of a skill whose task waits for the yes.
pub fn line(asks: &TaskAsks) -> String {
    format!("{} It runs only once you agree.", said(asks))
}

/// The say-back of a skill whose task is agreed already.
pub fn approved_line(asks: &TaskAsks) -> String {
    format!("{} You agreed to that already.", said(asks))
}

/// What the model is told after a task's say-back: ask nothing; on an
/// agreement, the call with `id`; on anything else, no call.
pub fn then_wait(id: &str) -> String {
    format!(
        "End as the line ends, asking nothing. Nothing runs until they agree. If their next \
         words agree, in any words, call make_skill again with only approve_id \"{id}\". If \
         they decline or wait, don't call it: nothing runs."
    )
}

/// What the model is told once a task's yes is kept.
pub const APPROVED_NEXT: &str =
    "Say only, in the conversation's language, that it will now do its task as it said.";

/// What the model is told of a task skill read back on a route that can't
/// take a yes (the Live route): it waits, and no wait starts.
pub fn yes_not_here(spoken: &str) -> String {
    format!(
        "Say plainly, in the conversation's language, that {spoken} waits for their yes before \
         its task runs, and that I can't take that yes on this voice. Ask nothing, call nothing."
    )
}

/// What the model is told when the skill's task can't be read now.
pub fn unread(spoken: &str) -> String {
    format!(
        "Say plainly, in the conversation's language, that I can't read back what {spoken} uses \
         right now, and that they can ask me again in a moment. Ask nothing, call nothing."
    )
}

/// A skill on the lamp with a `do.task`, read back by voice by `name`,
/// asked by `asking`, at `now`, from `asks` (the manage helper's
/// `task_asks`, [`asks_by`]): none when no skill of that name has a task,
/// so the made skills' read-back goes on. Agreed already, it is said with
/// [`approved_line`] and nothing is asked. Otherwise it is said back
/// ([`line`]) and its yes awaited through the write yes's own wait
/// ([`skill_write_yes::wait_for_yes`]), with every guard of it; on a route
/// that can't take a yes, said to wait and no wait started. What the model
/// is told.
pub fn read_back(
    name: &str,
    asking: Asking,
    mut asks: impl FnMut(&str) -> Result<Option<TaskAsks>, String>,
    now: Instant,
) -> Option<Value> {
    let request = asking.request;
    if !asking.heard {
        return Some(json!({ "error": READ_BACK_NOT_ASKED }));
    }
    let asked = crate::made_skill::name_asked(name);
    // A number said in words ("store test two") is asked again as its
    // digits (`store-test-2`) when the name as said has no task
    // (`made_skill::name_digits`, #6223's rows).
    let digits = crate::made_skill::name_digits(&asked);
    let (asked, found) = match asks(&asked) {
        Ok(None) if digits != asked => {
            let found = asks(&digits);
            (digits, found)
        }
        found => (asked, found),
    };
    let task = match found {
        Ok(Some(task)) => task,
        Ok(None) => return None,
        Err(why) => {
            // The helper's words name no script, only why.
            eprintln!("lamp-voice: request {request}: a skill's task wasn't read back: {why}");
            return Some(json!({
                "skill": asked,
                "error": "its task can't be read now",
                "next": unread(&crate::made_skill::spoken_name(&asked)),
            }));
        }
    };
    let spoken = crate::made_skill::spoken_name(&task.name);
    if task.approved {
        eprintln!("lamp-voice: request {request}: a skill's task read back: agreed already");
        return Some(json!({
            "skill": task.name,
            "task": true,
            "approved": true,
            "next": format!(
                "{} Ask nothing, call nothing: it is agreed already.",
                skill_write_yes::say_whole(&approved_line(&task))
            ),
        }));
    }
    if !asking.sends_here {
        eprintln!(
            "lamp-voice: request {request}: a skill's task read back: its yes can't be taken on this route"
        );
        return Some(json!({
            "skill": task.name,
            "task": true,
            "error": "this voice can't take a yes",
            "next": yes_not_here(&spoken),
        }));
    }
    let id = skill_write_yes::wait_for_yes(task.asks(), request, now);
    eprintln!(
        "lamp-voice: request {request}: a skill's task read back: it waits for the person's yes ({} grant(s))",
        task.grants.len()
    );
    Some(json!({
        "skill": task.name,
        "task": true,
        "task_waits_for_yes": true,
        skill_write_yes::APPROVE_ID: id,
        "next": format!("{} {}", skill_write_yes::say_whole(&line(&task)), then_wait(&id)),
    }))
}

/// How long the manage helper, or lamp-control, is waited for.
const HELPER_WAIT: Duration = Duration::from_secs(8);

/// The skill `name` on the lamp as the manage helper at `manage` reads it
/// (`{"task_asks": NAME}`), without lamp-control's word: none for one with
/// no `do.task`, or none of that name; why not when the helper refused or
/// couldn't be reached.
fn read_by(manage: &Path, name: &str) -> Result<Option<TaskAsks>, String> {
    let answer = crate::tools::reached(manage, &json!({ "task_asks": name }), HELPER_WAIT)
        .ok_or("the lamp's skills couldn't be reached")?
        .ok_or("the lamp's skills didn't answer in time")?;
    if let Some(why) = answer.get("refused").and_then(Value::as_str) {
        return Err(why.to_owned());
    }
    let done = answer.get("done").ok_or("the lamp's skills said nothing")?;
    if done.get("task").is_some_and(Value::is_null) {
        return Ok(None);
    }
    TaskAsks::of_json(done)
        .map(Some)
        .ok_or_else(|| "the lamp's skills' answer can't be read".to_owned())
}

/// The skill `name` on the lamp ([`read_by`] the manage helper at
/// `manage`), approved only when lamp-control at `control` says the
/// owner's yes holds for exactly its folder and grants ([`agreed_by`]);
/// lamp-control not reached reads as not agreed.
pub fn asks_by(manage: &Path, control: &Path, name: &str) -> Result<Option<TaskAsks>, String> {
    let Some(mut asks) = read_by(manage, name)? else {
        return Ok(None);
    };
    asks.approved = agreed_by(control, &asks.name, &asks.sha256, &asks.grants)
        .is_ok_and(|agreed| agreed == Agreement::Agreed);
    Ok(Some(asks))
}

/// What lamp-control says of the owner's yes to a skill's task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agreement {
    /// A yes for exactly that folder and those grants.
    Agreed,
    /// No yes of that name.
    Waiting,
    /// A yes of that name, for other bytes or other grants.
    Changed,
}

/// Whether lamp-control at `control` keeps the owner's yes for the skill
/// `name`, its folder hashed `sha256` and declaring `grants`
/// (`{"task_yes_check": …}`); why not when it refused or couldn't be
/// reached. lamp-skills asks this at each run, and never writes a yes.
pub fn agreed_by(
    control: &Path,
    name: &str,
    sha256: &str,
    grants: &[String],
) -> Result<Agreement, String> {
    let line = json!({ CHECK: {"name": name, "sha256": sha256, "grants": grants} });
    let answer = crate::tools::reached(control, &line, HELPER_WAIT)
        .ok_or("lamp-control couldn't be reached")?
        .ok_or("lamp-control didn't answer in time")?;
    if let Some(why) = answer.get("error").and_then(Value::as_str) {
        return Err(why.to_owned());
    }
    match (
        answer.get("agreed").and_then(Value::as_bool),
        answer.get("why").and_then(Value::as_str),
    ) {
        (Some(true), _) => Ok(Agreement::Agreed),
        (Some(false), Some("changed")) => Ok(Agreement::Changed),
        (Some(false), _) => Ok(Agreement::Waiting),
        (None, _) => Err("lamp-control's answer can't be read".to_owned()),
    }
}

/// The owner's yes to the skill `name` taken back by lamp-control at
/// `control` (`{"task_yes_forget": NAME}`), as a remove takes the camera's.
pub fn forget_by(control: &Path, name: &str) -> Result<(), String> {
    let answer = crate::tools::reached(control, &json!({ FORGET: name }), HELPER_WAIT)
        .ok_or("lamp-control couldn't be reached")?
        .ok_or("lamp-control didn't answer in time")?;
    match answer.get("error").and_then(Value::as_str) {
        Some(why) => Err(why.to_owned()),
        None => Ok(()),
    }
}

/// The person's yes to `asks` recorded by lamp-control at `control`
/// (`{"task_yes_record": {"name", "sha256", "grants"}}`), only while the
/// skill on the lamp, read again by the manage helper at `manage`, still
/// has the SHA-256 and the grants said back; else [`NOT_SAID_BACK`] and
/// nothing is recorded.
pub fn approve_by(manage: &Path, control: &Path, asks: &Asks) -> Result<Value, String> {
    let grants = asks.grants.as_ref().ok_or("not a skill's task")?;
    let set = |grants: &[String]| grants.iter().cloned().collect::<BTreeSet<String>>();
    match read_by(manage, &asks.skill)? {
        Some(now) if now.sha256 == asks.sha256 && set(&now.grants) == set(grants) => {}
        _ => return Err(NOT_SAID_BACK.to_owned()),
    }
    let line = json!({ RECORD: {"name": asks.skill, "sha256": asks.sha256, "grants": grants} });
    let answer = crate::tools::reached(control, &line, HELPER_WAIT)
        .ok_or("lamp-control couldn't be reached")?
        .ok_or("lamp-control didn't answer in time")?;
    match (
        answer.get("recorded").and_then(Value::as_bool),
        answer.get("error").and_then(Value::as_str),
    ) {
        (Some(true), _) => Ok(json!({ "approved": 1 })),
        (_, Some(why)) => Err(why.to_owned()),
        _ => Err("lamp-control said nothing".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn booth(approved: bool) -> TaskAsks {
        TaskAsks {
            name: "photo-booth".into(),
            sha256: "ab".repeat(32),
            grants: vec!["camera".into(), "light".into()],
            task: "Take a photo and send it to my phone".into(),
            package: None,
            from: "the Skills Store".into(),
            approved,
        }
    }

    fn asking(request: u64) -> Asking {
        Asking {
            request,
            heard: true,
            owner_hears: true,
            sends_here: true,
        }
    }

    /// Every grant is said in plain words, in order, and none is left out.
    #[test]
    fn the_say_back_names_each_grant_in_plain_words() {
        assert_eq!(uses_said(&["camera".into()]), "it uses the camera");
        assert_eq!(
            uses_said(&["camera".into(), "light".into(), "speak".into()]),
            "it uses the camera, changes the light and speaks and plays sounds"
        );
        assert_eq!(uses_said(&[]), "it uses none of the lamp's parts");
        let said = line(&booth(false));
        for words in [
            "photo booth",
            "“Take a photo and send it to my phone”",
            "from the Skills Store",
            "it uses the camera and changes the light",
            "only once you agree",
        ] {
            assert!(said.contains(words), "{said}");
        }
        // Every grant has its words, and `skills` is none of them.
        assert!(GRANTS.iter().all(|(_, words)| !words.is_empty()));
        assert!(grant_words(NEVER).is_none());
        assert_eq!(grant_named("camera"), Some("camera"));
    }

    /// Read back, a task skill waits for the yes; a yes in a later heard
    /// request, after the say-back was heard whole, approves exactly its
    /// grants and its folder's SHA-256, and the next line is a task's.
    #[test]
    fn a_task_read_back_waits_and_a_later_yes_approves_its_grants_and_sha256() {
        let now = Instant::now();
        let told = read_back(
            "my photo booth skill",
            asking(4),
            |name| {
                assert_eq!(name, "photo-booth");
                Ok(Some(booth(false)))
            },
            now,
        )
        .unwrap();
        assert_eq!(told["task_waits_for_yes"], true, "{told}");
        let id = told[skill_write_yes::APPROVE_ID]
            .as_str()
            .unwrap()
            .to_owned();
        let next = told["next"].as_str().unwrap();
        assert!(
            next.contains("it uses the camera") && next.contains(&id),
            "{next}"
        );
        // Not said whole yet: not taken.
        let early = skill_write_yes::approve(&id, 5, true, true, now, |_| panic!("approved"));
        assert_eq!(early["error"], skill_write_yes::NOT_SAID_WHOLE);
        skill_write_yes::lamp_said(5, &line(&booth(false)));
        let mut kept = None;
        let told = skill_write_yes::approve(&id, 6, true, true, now, |asks| {
            kept = Some(asks.clone());
            Ok(json!({"approved": 1}))
        });
        assert_eq!(told["approved"], true, "{told}");
        assert_eq!(told["next"], APPROVED_NEXT);
        let kept = kept.unwrap();
        assert_eq!(kept.sha256, "ab".repeat(32));
        assert_eq!(kept.grants, Some(vec!["camera".into(), "light".into()]));
        assert!(kept.writes.is_empty());
    }

    /// Agreed already for this folder, it is said and nothing is asked; a
    /// changed folder reads as not agreed (the helper's `approved` is for
    /// the SHA-256 now), so it is said back and asks again.
    #[test]
    fn an_agreed_task_asks_nothing_and_a_changed_one_asks_again() {
        let now = Instant::now();
        let told = read_back("photo booth", asking(8), |_| Ok(Some(booth(true))), now).unwrap();
        assert_eq!(told["approved"], true);
        assert!(told.get(skill_write_yes::APPROVE_ID).is_none());
        let mut changed = booth(false);
        changed.sha256 = "cd".repeat(32);
        let told = read_back("photo booth", asking(9), |_| Ok(Some(changed.clone())), now).unwrap();
        assert_eq!(told["task_waits_for_yes"], true);
    }

    /// Unheard, or on a route with no echo judge, nothing waits; a name with
    /// no task falls through to the made skills' read-back.
    #[test]
    fn unheard_or_live_or_no_task_starts_no_wait() {
        let now = Instant::now();
        let mut unheard = asking(10);
        unheard.heard = false;
        let told = read_back("photo booth", unheard, |_| panic!("asked"), now).unwrap();
        assert_eq!(told["error"], READ_BACK_NOT_ASKED);
        let mut live = asking(11);
        live.sends_here = false;
        let told = read_back("photo booth", live, |_| Ok(Some(booth(false))), now).unwrap();
        assert!(
            told["next"]
                .as_str()
                .unwrap()
                .contains("can't take that yes")
        );
        assert!(read_back("evening dim", asking(12), |_| Ok(None), now).is_none());
        let told = read_back("photo booth", asking(13), |_| Err("busy".into()), now).unwrap();
        assert!(told["next"].as_str().unwrap().contains("can't read back"));
    }

    /// A number said in words is asked again as its digits when the name as
    /// said has no task (#6223's rows: "two" for a `2`); a name with no
    /// number in words is asked once.
    #[test]
    fn a_skill_named_with_a_digit_is_read_back_by_the_number_said() {
        let now = Instant::now();
        let mut asked = Vec::new();
        let told = read_back(
            "photo booth two",
            asking(14),
            |name| {
                asked.push(name.to_owned());
                Ok((name == "photo-booth-2").then(|| TaskAsks {
                    name: "photo-booth-2".into(),
                    ..booth(true)
                }))
            },
            now,
        )
        .unwrap();
        assert_eq!(asked, ["photo-booth-two", "photo-booth-2"]);
        assert!(told.get("error").is_none(), "{told}");
        let mut asked = Vec::new();
        let told = read_back(
            "evening dim",
            asking(15),
            |name| {
                asked.push(name.to_owned());
                Ok(None)
            },
            now,
        );
        assert!(told.is_none());
        assert_eq!(asked, ["evening-dim"], "asked once");
    }

    /// A skill made or added here carrying `grants` or a `task` step is
    /// refused by the made cap, before and as it is put on, with a made
    /// skill's code off (`LAMP_MADE_SCRIPTS`, #6307).
    #[test]
    fn a_made_skill_carrying_a_task_or_grants_is_refused() {
        crate::made_skill::set_scripts_here(Some(false));
        let base = "kind = \"installable\"\nversion = \"1.0.0\"\n[permissions]\nlight = true\n\
                    topics = [\"voice/state\"]\n[trigger]\ntopic = \"voice/state\"\n\
                    field = \"state\"\nequals = \"resting\"\n[[do]]\nlight = \"warm\"\n";
        assert!(crate::made_skill::put_on_cap(base).is_ok());
        for text in [
            format!("grants = [\"camera\"]\n{base}"),
            format!("{base}[[do]]\ntask = {{ task = \"x\", run = \"scripts/main.py\" }}\n"),
        ] {
            for refused in [
                crate::made_skill::put_on_cap(&text),
                crate::made_skill::capped(&text),
            ] {
                let why = format!("{:?}", refused.unwrap_err());
                // Made here: what the lamp can't do yet (#6307's switch).
                assert!(why.contains("running code of its own"), "{why}");
            }
        }
        crate::made_skill::set_scripts_here(None);
    }

    /// With the switch on, `make_skill` offers the read-back and the yes
    /// for a skill's task, within the link's bounds, writes on or off.
    #[test]
    fn the_read_back_is_offered_with_tasks_within_the_bounds() {
        for writes in [false, true] {
            crate::made_skill::set_writes_here(Some(writes));
            set_on_here(Some(true));
            let tool = crate::make_skill_local::tool();
            assert!(
                tool.description.len() <= 1_024,
                "{}",
                tool.description.len()
            );
            assert!(!tool.description.contains("yes"), "{}", tool.description);
            assert!(tool.description.contains("what a skill on the lamp uses"));
            let parameters = tool.parameters.unwrap();
            assert!(parameters.to_string().len() <= 4_096);
            assert!(parameters["properties"][crate::make_skill_local::READ_BACK].is_object());
            assert!(parameters["properties"][skill_write_yes::APPROVE_ID].is_object());
            set_on_here(Some(false));
            let tool = crate::make_skill_local::tool();
            assert!(!tool.description.contains("what a skill on the lamp uses"));
        }
        crate::made_skill::set_writes_here(None);
        set_on_here(None);
    }

    /// A socket at `path` answering each of `answers` in turn, one a
    /// connection: the lines it was asked.
    fn answering(path: &Path, answers: Vec<Value>) -> std::thread::JoinHandle<Vec<Value>> {
        use std::io::{BufRead as _, Write as _};
        let _ = std::fs::remove_file(path);
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

    /// The yes goes to lamp-control, never the manage helper, and only for
    /// the folder and grants said back: a skill changed since records
    /// nothing. lamp-control's check is read as agreed, waiting or changed.
    #[test]
    fn the_yes_is_recorded_by_lamp_control_only_for_the_folder_said_back() {
        let dir = std::env::temp_dir().join(format!("lamp-voice-task-yes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (manage, control) = (dir.join("manage.sock"), dir.join("control.sock"));
        let asks = booth(false).asks();
        let read = json!({"done": {"name": "photo-booth", "sha256": "ab".repeat(32),
            "grants": ["light", "camera"], "task": "Take a photo", "from": "the Skills Store"}});
        let helper = answering(&manage, vec![read.clone()]);
        let lamp_control = answering(&control, vec![json!({"recorded": true})]);
        assert_eq!(
            approve_by(&manage, &control, &asks).unwrap(),
            json!({"approved": 1})
        );
        assert_eq!(
            helper.join().unwrap(),
            [json!({"task_asks": "photo-booth"})]
        );
        let recorded = lamp_control.join().unwrap();
        assert_eq!(recorded[0][RECORD]["sha256"], "ab".repeat(32));
        assert_eq!(recorded[0][RECORD]["grants"], json!(["camera", "light"]));
        // Changed since the say-back: lamp-control is never asked.
        let mut changed = read;
        changed["done"]["sha256"] = json!("cd".repeat(32));
        let helper = answering(&manage, vec![changed]);
        assert_eq!(
            approve_by(&manage, &control, &asks).unwrap_err(),
            NOT_SAID_BACK
        );
        helper.join().unwrap();
        // lamp-control's word on a run.
        let grants = ["camera".to_owned()];
        let checks = answering(
            &control,
            vec![
                json!({"agreed": true}),
                json!({"agreed": false, "why": "changed"}),
                json!({"agreed": false, "why": "waiting"}),
                json!({"error": OFF}),
            ],
        );
        let sha256 = "ab".repeat(32);
        let check = || agreed_by(&control, "photo-booth", &sha256, &grants);
        assert_eq!(check(), Ok(Agreement::Agreed));
        assert_eq!(check(), Ok(Agreement::Changed));
        assert_eq!(check(), Ok(Agreement::Waiting));
        assert_eq!(check(), Err(OFF.to_owned()));
        assert_eq!(checks.join().unwrap()[0][CHECK]["name"], "photo-booth");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The helper's answer is read only whole.
    #[test]
    fn the_helpers_answer_is_read_only_whole() {
        let done = json!({"name": "photo-booth", "sha256": "ab".repeat(32),
            "grants": ["camera"], "task": "Take a photo", "from": "the Skills Store",
            "approved": false});
        assert_eq!(TaskAsks::of_json(&done).unwrap().grants, ["camera"]);
        let mut short = done.clone();
        short["sha256"] = json!("ab");
        assert!(TaskAsks::of_json(&short).is_none());
        let mut wrong = done;
        wrong["grants"] = json!([1]);
        assert!(TaskAsks::of_json(&wrong).is_none());
    }
}
