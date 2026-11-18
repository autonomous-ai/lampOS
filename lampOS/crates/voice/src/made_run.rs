//! A skill the person made, run once now by voice (lamp-9adf, 15 Nov at
//! 15:49 and 15:52: "Run my unread mail skill now, please." and "Run my
//! read newest email when sitting skill now." were each taken for a
//! read-back, and the lamp said only "unread mail doesn't send or write
//! anything": a made skill has a trigger and no tool, so `use_skill` can't
//! run it, and nothing else could).
//!
//! - **One plain path, the model's to choose.** `make_skill` takes [`RUN`],
//!   the name as heard, given alone: whether a line asks to run a skill now
//!   or to hear what it sends is the model's judgment, never a list of
//!   words (the owner's ruling, 13 Nov), as for a remove
//!   (`crate::made_remove`).
//! - **The owner's alone, heard.** A turn nobody spoke runs nothing
//!   ([`NOT_ASKED_ALOUD`]); a line not the owner's, as a read-back or a
//!   remove holds it (`services::owner_hears`), runs nothing and names no
//!   skill ([`NOT_OWNER`]). The name is matched to the skills they made as
//!   a remove's is (`made_remove::found`); one they didn't make, several as
//!   near, or none, are said so.
//! - **Exactly a trigger's bounds.** One that is not on, whose writes wait
//!   for the owner's yes (`services::skill_writes_approved`), or whose task
//!   waits for its yes (`skill_task_yes::asks_by`), is said so and nothing
//!   runs. Otherwise lamp-skills is asked, on its call socket's `run` op
//!   ([`OP`], the voice's alone), to run it once now as its trigger would
//!   (`lamp_skills::Engine::run_now`): a task's runs in 24 hours
//!   (`PER_SKILL`, `ALL_SKILLS`), one at a time, a read's once a day, a
//!   write stopped, each refused there in plain words before anything is
//!   made, and each step still held to the real gate where it is made (the
//!   task's hand-over and the owner's yes, the read's room, the services'
//!   own tokens). Its trigger's cooldown, day's most and group's hour are
//!   neither asked nor spent, as for a tool's run: a person asked.
//!
//! No extra round trip but the checks a read-back makes: the match reads
//! two local folders, then one line to lamp-skills, at most
//! [`crate::tools::ASK_DEADLINE`].
use crate::made_remove::{self, Found};
use crate::made_skill::{self, Keeping};
use crate::skill_write_yes::Asks;
use serde_json::{Value, json};
use std::path::Path;

/// The `make_skill` argument that runs a skill the person made, now: its
/// name as heard, given alone.
pub const RUN: &str = "run";

/// lamp-skills' call socket op that runs a made skill now:
/// `{"op":"run","skill":NAME}`, answered `{"ok":true,"result":{"ran":NAME}}`
/// or `{"ok":false,"error":WHY}`.
pub const OP: &str = "run";

/// What lamp-skills answers a `run` from any peer but the voice.
pub const NOT_THE_VOICE: &str = "only the voice may run a skill now";

/// What the model is told of a run asked in a turn nobody spoke.
pub const NOT_ASKED_ALOUD: &str = "a skill is run by voice only when the person asks out loud";

/// What the model is told of a run asked by anyone but the owner.
pub const NOT_OWNER: &str = "only the lamp's owner can run a skill they made by voice";

/// What the model is told of a skill on the lamp the person didn't make.
pub const NOT_MADE: &str = "only a skill the person made is run this way";

/// What the model is told when lamp-skills didn't answer.
pub const NOT_ANSWERED: &str = "my skills didn't answer, so it may not have started";

/// Whether a run's answer `told` says the skill ran now and speaks its
/// own words ([`ran_by`]'s `says`): a run its heard words started in code
/// then ends with no line of the lamp's own, so the skill's say is the
/// first thing heard (lamp-9adf, 16 Nov 15:36: "Lamp, moon time." was
/// answered "Moon time is running now." first, and the skill's own line
/// came 20 s after the call, the say service's quiet after that turn).
/// One turned on for the ask ([`told_turned_on`]) never does: its undo is
/// said first.
pub fn speaks_itself(told: &Value) -> bool {
    told["ran"] == true && told["says"] == true && told.get(TURNED_ON).is_none()
}

/// How long after its run now a made skill's exact line, asked as the
/// answer to that conversation (the say service's `answers`), is said as
/// one: the conversation still going on (`crate::thread::GOING_ON_FOR`).
/// lamp-skills asks it so only within this of the ask
/// (`lamp_skills::task_sure`, the first builder's 21:25 on #7363).
pub const ANSWERS_WITHIN: std::time::Duration = crate::thread::GOING_ON_FOR;

/// The made skills run now by the owner's voice, each with when: the say
/// service's own record, so no other asker can stand for a run's answer.
static RAN_NOW: std::sync::Mutex<Vec<(String, std::time::Instant)>> =
    std::sync::Mutex::new(Vec::new());

/// The key lamp-skills names a skill's run by in its answer to the voice's
/// call of that skill's own tool, when the run has a task it marked as
/// asked by voice (lamp-9adf, 18 Nov: "run my bitcoin price skill now"
/// called `btc_price`, not a run now): `{"task_run": NAME}` (#7363).
pub const TASK_RUN: &str = "task_run";

/// The skill a call's answer `told` says was run by voice, if any: a made
/// skill run now ([`asked`]'s `{"run": NAME, "ran": true}`) or a skill's
/// own tool whose run has a task (`{"task_run": NAME}`, [`TASK_RUN`]). The
/// worker tells the coordinator (`ProviderOutput::MadeRunNow`), which keeps
/// it ([`ran_now`]) where the say service reads it. Never kept in the
/// worker's own process, which the say service never reads (El's P1 on
/// #7363). An error is no run.
pub fn ran_of(told: &Value) -> Option<String> {
    if told.get("error").is_some() {
        return None;
    }
    (told["ran"] == true)
        .then(|| told[RUN].as_str())
        .flatten()
        .or_else(|| told[TASK_RUN].as_str())
        .filter(|skill| !skill.is_empty())
        .map(str::to_owned)
}

/// `skill` run now by the owner's voice at `at`: the coordinator's alone,
/// told by the worker ([`ran_of`]).
pub(crate) fn ran_now(skill: &str, at: std::time::Instant) {
    let mut ran = RAN_NOW.lock().unwrap_or_else(|poison| poison.into_inner());
    ran.retain(|(name, when)| {
        name != skill && at.saturating_duration_since(*when) <= ANSWERS_WITHIN
    });
    ran.push((skill.to_owned(), at));
}

/// Whether `skill`'s line may answer the conversation that ran it at `now`:
/// run now by the owner's voice within [`ANSWERS_WITHIN`]. Keyed by the
/// skill's name alone: any say-socket caller naming it then is let through
/// as its answer, with exact words only, the trust `prepare-unplug`'s lines
/// have (El's P3 on #7363).
pub fn answers_now(skill: &str, now: std::time::Instant) -> bool {
    RAN_NOW
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .iter()
        .any(|(name, at)| name == skill && now.saturating_duration_since(*at) <= ANSWERS_WITHIN)
}

/// The most of their skills' names said when none is near.
const NAMED_MOST: usize = 3;

/// What lamp-skills answers a run now of a skill turned off or snoozed
/// (`lamp_skills::Engine::run_now`), word for word.
pub const STOPPED: &str = "it is turned off or snoozed";

/// Whether a run's answer `told` is lamp-skills' refusal of a skill turned
/// off or snoozed ([`STOPPED`]).
pub fn stopped(told: &Value) -> bool {
    told["error"] == STOPPED
}

/// A made skill the owner turned off, asked to run now, with
/// `LAMP_MADE_SKILL_OFF_SAID` on (#7562's row on lamp-9adf, 18 Nov: "Lamp,
/// use my space facts skill." with space facts turned off was the model's
/// `make_skill` run, refused "it is turned off or snoozed", and the lamp
/// said "Space facts didn't run — it's turned off at the moment."): turned
/// on for the ask, as `made_turn::on_for_ask` turns one on for its
/// function's call, then run, said with one line and its undo. Nothing is
/// read of the line: the run asked is the ask. A snooze is never turned.
#[derive(Clone, Copy)]
pub struct TurnsOn<'a> {
    /// Whether the skill is turned off, by lamp-skills' `off` file (the
    /// app's switch or a voice turn-off), never a snooze.
    pub is_off: &'a dyn Fn(&str) -> bool,
    /// The skill turned on, as `make_skill`'s own `turn_on`
    /// (`made_turn::asked_here`): done, or why not.
    pub turn_on: &'a dyn Fn(&str) -> Result<(), String>,
    /// How long before each run again while lamp-skills still answers it
    /// off ([`STOPPED`]), at most `made_turn::RERUNS` times: it reads its
    /// `off` file again within 2 s.
    pub wait: std::time::Duration,
}

/// The run's answer `told` ([`asked`]'s `{"run": NAME, "ran": true}`) of a
/// skill turned on for the ask: the one line to say first, with its undo,
/// in the conversation's language, then that it is running. Still a run
/// ([`ran_of`]), never one that speaks itself first ([`speaks_itself`]):
/// the undo is said.
pub fn told_turned_on(told: &Value) -> Value {
    let made = told[RUN].as_str().unwrap_or_default();
    let spoken = made_skill::spoken_name(made);
    json!({
        RUN: made,
        "ran": true,
        "says": told["says"],
        TURNED_ON: spoken,
        "next": format!(
            "It was off, so the lamp turned it on and ran it for their ask. Say first, in the \
             conversation's language: \"{}\" Then say only, in a few words, that it is running \
             now. Never say what it found, sent or did, nor that it is done. Call nothing.",
            crate::made_turn::undo_line(&spoken)
        )
    })
}

/// The key of a run's answer naming the skill turned on for the ask
/// ([`told_turned_on`]).
pub const TURNED_ON: &str = "turned_on";

/// The answer to the call `name`, `make_skill`'s run now that ran the made
/// skill `on` the model's own `turn_on` turned on in the turn (`told`),
/// said with the undo line ([`told_turned_on`]); none for any other.
pub fn ran_after_turn_on(on: Option<&str>, name: &str, told: &Value) -> Option<Value> {
    let on = on?;
    (name == made_skill::MAKE
        && told["ran"] == true
        && told[RUN] == on
        && told.get(TURNED_ON).is_none())
    .then(|| told_turned_on(told))
}

/// A run asked by the model's `make_skill` call: the name as heard, in the
/// voice's request `request` (`heard`: someone spoke it).
#[derive(Clone, Copy, Debug)]
pub struct Ask<'a> {
    pub name: &'a str,
    pub request: u64,
    pub heard: bool,
}

/// What the model is told of a skill that didn't run, and why.
fn not_run(spoken: &str, why: &str) -> Value {
    json!({
        "error": why,
        "next": format!(
            "Say plainly, in a few words, in the conversation's language, that {spoken} didn't \
             run now, and why: {why}. Never say it is running. Call nothing."
        )
    })
}

/// A run asked in `ask`, of the skills in `keeping`: only heard, and only
/// the owner's (`owner_hears`, asked first). The person's own made skill
/// [`made_remove::found`] by the name, on, its writes approved
/// (`writes_approved`, `services::skill_writes_approved`: `(on, approved)`)
/// and its task's yes given (`task_agreed`: none for a skill with no task,
/// else whether lamp-control keeps its yes), is run by `run`
/// (lamp-skills' [`OP`]: whether it speaks its own words, or why not, in
/// its words). What the model is told. With `turns` (the switch on), one
/// turned off is turned on for the ask first ([`TurnsOn`]).
pub fn asked(
    ask: Ask<'_>,
    keeping: &Keeping,
    owner_hears: impl FnOnce() -> bool,
    task_agreed: impl FnOnce(&str) -> Result<Option<bool>, String>,
    writes_approved: impl FnOnce(&Asks) -> Option<(bool, bool)>,
    run: impl FnMut(&str) -> Result<bool, String>,
) -> Value {
    asked_turning(
        ask,
        keeping,
        owner_hears,
        task_agreed,
        writes_approved,
        run,
        None,
    )
}

/// [`asked`], with `turns`: a skill turned off is turned on for the ask
/// ([`TurnsOn`]) once its other bounds hold, then run.
pub fn asked_turning(
    ask: Ask<'_>,
    keeping: &Keeping,
    owner_hears: impl FnOnce() -> bool,
    task_agreed: impl FnOnce(&str) -> Result<Option<bool>, String>,
    writes_approved: impl FnOnce(&Asks) -> Option<(bool, bool)>,
    mut run: impl FnMut(&str) -> Result<bool, String>,
    turns: Option<TurnsOn<'_>>,
) -> Value {
    let Ask {
        name,
        request,
        heard,
    } = ask;
    if !heard {
        eprintln!("lamp-voice: request {request}: a skill wasn't run now: no one asked aloud");
        return json!({ "error": NOT_ASKED_ALOUD });
    }
    if !owner_hears() {
        eprintln!("lamp-voice: request {request}: a skill wasn't run now: not the owner's line");
        return json!({
            "error": NOT_OWNER,
            "next": "Say plainly, in a few words, that only the lamp's owner can run a skill they \
                     made by voice. Name no skill. Call nothing."
        });
    }
    let made = match made_remove::found(name, keeping) {
        Found::Made { name, .. } => name,
        Found::NotMade { name: other, .. } => {
            return json!({
                "error": NOT_MADE,
                "next": format!(
                    "Say plainly, in a few words, that {} isn't a skill they made, so I can't run \
                     it this way. Call nothing.",
                    made_skill::spoken_name(&other)
                )
            });
        }
        Found::Alike(names) => {
            return json!({
                "error": "more than one skill they made has a name like that",
                "next": format!(
                    "Ask them once, short, which one they mean: {}. Call nothing now.",
                    made_remove::said_or(&names[..names.len().min(NAMED_MOST)])
                )
            });
        }
        Found::Nothing => {
            let theirs = made_skill::kept_on_names(&keeping.directory);
            let named = match theirs.len() {
                0 => "and that none they made is on".to_owned(),
                count if count <= NAMED_MOST => format!(
                    "and name theirs that are on: {}",
                    theirs
                        .iter()
                        .map(|name| made_skill::spoken_name(name))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                count => format!("and that {count} they made are on"),
            };
            eprintln!(
                "lamp-voice: request {request}: a skill to run now was not found by the name asked"
            );
            return json!({
                "error": made_remove::NO_MADE,
                "next": format!(
                    "Say plainly, in a few words, that you found no skill they made by that \
                     name, {named}. Call nothing."
                )
            });
        }
    };
    let spoken = made_skill::spoken_name(&made);
    let Some(kept) = made_skill::kept_on_named(&keeping.directory, &made) else {
        eprintln!("lamp-voice: request {request}: a made skill wasn't run now: it isn't on");
        return not_run(&spoken, "it isn't on the lamp now");
    };
    // Its writes, as a read-back reads them: approved for the package on
    // the lamp, or not run (Some((false, _)): the package changed since).
    if let Some(asks) = crate::make_skill_local::asks_of(&keeping.directory, &kept) {
        match writes_approved(&asks) {
            Some((true, true)) => {}
            Some((false, _)) => {
                return not_run(&spoken, "it changed on the lamp since it was made");
            }
            Some((true, false)) => {
                eprintln!(
                    "lamp-voice: request {request}: a made skill wasn't run now: its writes wait for the owner's yes"
                );
                return not_run(&spoken, "it waits for their yes to what it sends");
            }
            None => return not_run(&spoken, "I can't tell now whether what it sends is agreed"),
        }
    }
    match task_agreed(&made) {
        Ok(None | Some(true)) => {}
        Ok(Some(false)) => {
            eprintln!(
                "lamp-voice: request {request}: a made skill wasn't run now: its task waits for the owner's yes"
            );
            return not_run(&spoken, "it waits for their yes to what it uses");
        }
        Err(_) => return not_run(&spoken, "I can't tell now whether what it uses is agreed"),
    }
    // Turned off by the owner (`LAMP_MADE_SKILL_OFF_SAID`): turned on for
    // the ask, then run; a snooze is left as it is.
    let mut turned_on = false;
    if let Some(turns) = turns
        && (turns.is_off)(&made)
    {
        if let Err(why) = (turns.turn_on)(&made) {
            eprintln!(
                "lamp-voice: request {request}: a made skill turned off wasn't run now: not turned on for the ask"
            );
            return not_run(
                &spoken,
                &format!("it is turned off, and it wasn't turned on: {why}"),
            );
        }
        eprintln!(
            "lamp-voice: request {request}: a made skill turned off asked to run now: turned on for the ask (LAMP_MADE_SKILL_OFF_SAID)"
        );
        turned_on = true;
    }
    let mut ran = run(&made);
    if let Some(turns) = turns.filter(|_| turned_on) {
        for _ in 0..crate::made_turn::RERUNS {
            if !matches!(&ran, Err(why) if why == STOPPED) {
                break;
            }
            std::thread::sleep(turns.wait);
            eprintln!(
                "lamp-voice: request {request}: a made skill still off just after its turn on: run again"
            );
            ran = run(&made);
        }
        if matches!(&ran, Err(why) if why == STOPPED) {
            return crate::made_turn::turned_on_for_ask(&spoken, &crate::made_turn::just_on());
        }
    }
    match ran {
        Ok(says) if turned_on => {
            eprintln!(
                "lamp-voice: request {request}: a made skill turned on and run now, as its owner asked"
            );
            told_turned_on(&json!({ RUN: made, "says": says }))
        }
        Ok(says) => {
            eprintln!("lamp-voice: request {request}: a made skill run now, as its owner asked");
            json!({
                RUN: made,
                "ran": true,
                // It speaks its own words ([`speaks_itself`]).
                "says": says,
                "next": format!(
                    "Say only, in a few words, in the conversation's language, that {spoken} is \
                     running now. Never say what it found, sent or did, nor that it is done. \
                     Call nothing."
                )
            })
        }
        Err(why) => {
            eprintln!("lamp-voice: request {request}: a made skill wasn't run now: {why}");
            not_run(&spoken, &why)
        }
    }
}

/// The made skill `name` run now by lamp-skills, on its call socket in
/// `folder` ([`OP`]): whether its run speaks its own words (`says`), or why
/// not, in its words, or [`NOT_ANSWERED`].
pub fn ran_by(folder: &Path, name: &str) -> Result<bool, String> {
    let answer = crate::tools::exchange(
        &folder.join(crate::tools::CALL_SOCKET),
        &json!({ "op": OP, "skill": name }),
        crate::tools::ASK_DEADLINE,
    )
    .ok_or(NOT_ANSWERED)?;
    match answer.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(answer["result"]["says"] == true),
        _ => Err(answer
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or(NOT_ANSWERED)
            .to_owned()),
    }
}

/// [`asked`] on the lamp, by `make_skill`'s call: the made skills' own
/// folder (`LAMP_MAKE_SKILL`), the owner's hearing as a read-back's
/// (`services::owner_hears`), the writes' approval of `lamp-services`, the
/// task's yes of lamp-control through the manage helper (with a skill's
/// tasks on, `LAMP_SKILL_TASKS`), and lamp-skills' own folder `skills`, the
/// tools' (`--skills-dir`). Not `LAMP_SKILLS_DIR`: the second route's
/// worker is started without it, and on lamp-9adf (15 Nov, 17:42) "Run my
/// read newest email when sitting skill now" was answered "that part of me
/// isn't running".
pub fn asked_here(
    name: &str,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
    skills: Option<&Path>,
) -> Value {
    let (Some(keeping), Some(skills)) = (Keeping::when(true), skills) else {
        return crate::tools::unavailable();
    };
    made_skill::drop_gone(&keeping);
    let manage = keeping.manage.clone();
    let is_off = |skill: &str| {
        crate::skill_about::turned_off(Path::new(crate::skill_about::STATE_DIR))
            .is_ok_and(|off| off.contains(skill))
    };
    let turn_on = |skill: &str| {
        let on = json!({ crate::made_turn::TURN_ON: skill });
        let told = crate::made_turn::asked_here(skill, false, request, heard, lease);
        if crate::made_turn::turned(made_skill::MAKE, &on, &told) && told["off"] == false {
            Ok(())
        } else {
            Err(told["error"]
                .as_str()
                .unwrap_or("it couldn't be turned on")
                .to_owned())
        }
    };
    let turns = made_skill::off_said_on().then_some(TurnsOn {
        is_off: &is_off,
        turn_on: &turn_on,
        wait: crate::made_turn::RERUN_AFTER,
    });
    asked_turning(
        Ask {
            name,
            request,
            heard,
        },
        &keeping,
        || crate::services::owner_hears(request, heard, lease),
        |skill| {
            if !crate::skill_task_yes::on() {
                return Ok(None);
            }
            crate::skill_task_yes::asks_by(
                &manage,
                Path::new(crate::skill_task_yes::CONTROL_SOCKET),
                skill,
            )
            .map(|asks| asks.map(|asks| asks.approved))
        },
        |asks| crate::services::skill_writes_approved(Path::new(crate::services::FOLDER), asks),
        |skill| ran_by(skills, skill),
        turns,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::made_remove::tests::made_here;
    use crate::make_skill_local::tests::keeping;
    use std::path::PathBuf;

    /// lamp-9adf, 18 Nov: "run my bitcoin price skill now" was the model's
    /// call of `btc_price`, no run now: lamp-skills names the run with a
    /// task in its answer ([`TASK_RUN`]), and the worker tells the
    /// coordinator of it as of a run now, through `use_skill`'s note too;
    /// a refusal, or an answer naming no run, is none (#7363).
    #[test]
    fn a_skill_tools_run_with_a_task_is_told_as_run_by_voice() {
        let told = json!({"next": "Say you'll get the price.", TASK_RUN: "btc-price"});
        assert_eq!(ran_of(&told).as_deref(), Some("btc-price"));
        let mut through = told.clone();
        crate::use_skill::ran("btc-price", &mut through);
        assert_eq!(ran_of(&through).as_deref(), Some("btc-price"));
        assert_eq!(ran_of(&json!({"next": "Say you'll dance."})), None);
        assert_eq!(ran_of(&json!({TASK_RUN: ""})), None);
        assert_eq!(
            ran_of(&json!({"error": "I'm still doing that.", TASK_RUN: "btc-price"})),
            None
        );
    }

    /// The owner's lamp-9adf on 15 Nov: unread-mail and
    /// read-newest-email-when-sitting made and on, evening-dim made and off.
    fn nines_lamp(test: &str) -> (PathBuf, Keeping) {
        let (root, keeping) = keeping(test);
        made_here(
            &keeping,
            &[
                ("unread-mail", true),
                ("read-newest-email-when-sitting", true),
                ("evening-dim", false),
            ],
        );
        (root, keeping)
    }

    fn ask(name: &str) -> Ask<'_> {
        Ask {
            name,
            request: 7,
            heard: true,
        }
    }

    fn owner() -> bool {
        true
    }

    fn no_task(_: &str) -> Result<Option<bool>, String> {
        Ok(None)
    }

    fn approved(_: &Asks) -> Option<(bool, bool)> {
        Some((true, true))
    }

    fn never(_: &str) -> Result<bool, String> {
        panic!("run")
    }

    /// The owner's two lines at 15:49 and 15:52, as the model passes the
    /// name it heard to `make_skill`'s `run`: each runs that made skill
    /// once, by its name as written, and the model says it is running,
    /// never what it found.
    /// On lamp-9adf (15 Nov, 17:42) the second route's worker, started
    /// without `LAMP_SKILLS_DIR`, answered a run "that part of me isn't
    /// running": `make_skill` now carries the tools' own lamp-skills folder
    /// (`--skills-dir`), and with none a run is unavailable, as before.
    #[test]
    fn make_skill_carries_the_tools_skills_folder_for_a_run() {
        use crate::tools::{Route, Settings, route};
        let folder = PathBuf::from("/run/lampos-skills");
        let on = Settings {
            make_skill: true,
            skills: Some(folder.clone()),
            ..Settings::default()
        };
        assert_eq!(route(&on, made_skill::MAKE), Route::MakeLocal(Some(folder)));
        assert_eq!(
            asked_here("unread mail", 3, true, None, None),
            crate::tools::unavailable()
        );
    }

    #[test]
    fn the_owners_two_lines_each_run_that_made_skill() {
        let (root, keeping) = nines_lamp("run-two-lines");
        for (heard, name) in [
            ("unread mail", "unread-mail"),
            (
                "my read newest email when sitting skill",
                "read-newest-email-when-sitting",
            ),
        ] {
            let mut ran = Vec::new();
            let told = asked(ask(heard), &keeping, owner, no_task, approved, |skill| {
                ran.push(skill.to_owned());
                Ok(false)
            });
            assert_eq!(ran, [name], "{heard}: {told}");
            assert_eq!(told[RUN], name, "{told}");
            assert_eq!(told["ran"], true, "{told}");
            // What the worker tells the coordinator of it (#7363).
            assert_eq!(ran_of(&told).as_deref(), Some(name), "{told}");
            assert!(!speaks_itself(&told), "{told}");
            let next = told["next"].as_str().unwrap();
            assert!(next.contains("is running now"), "{next}");
            assert!(next.contains("Never say what it found"), "{next}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A run whose skill speaks its own words (lamp-skills' `says`) is
    /// told so, for the second route to add no line of its own when the
    /// heard words started it ([`speaks_itself`]); a refusal never is.
    #[test]
    fn a_run_that_speaks_itself_is_told_so() {
        let (root, keeping) = nines_lamp("run-says");
        let told = asked(
            ask("unread mail"),
            &keeping,
            owner,
            no_task,
            approved,
            |_| Ok(true),
        );
        assert!(speaks_itself(&told), "{told}");
        let refused = asked(
            ask("unread mail"),
            &keeping,
            owner,
            no_task,
            approved,
            |_| Err("its last task is still under way".to_owned()),
        );
        assert!(!speaks_itself(&refused), "{refused}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// lamp-skills' refusal (its runs spent, its last under way) is said
    /// plainly with its why, and never as running; one whose task waits
    /// for its yes, or that is off, is not asked of lamp-skills at all.
    #[test]
    fn a_run_refused_is_said_plainly_with_why() {
        let (root, keeping) = nines_lamp("run-refused");
        for why in [
            "its 3 runs in 24 hours are spent",
            "its last task is still under way",
        ] {
            let told = asked(
                ask("unread mail"),
                &keeping,
                owner,
                no_task,
                approved,
                |_| Err(why.to_owned()),
            );
            assert_eq!(told["error"], why, "{told}");
            assert_eq!(ran_of(&told), None, "a refused run is no run");
            let next = told["next"].as_str().unwrap();
            assert!(next.contains("unread mail didn't run now"), "{next}");
            assert!(next.contains(why), "{next}");
            assert!(next.contains("Never say it is running"), "{next}");
        }
        let waits = asked(
            ask("unread mail"),
            &keeping,
            owner,
            |_| Ok(Some(false)),
            approved,
            never,
        );
        assert_eq!(
            waits["error"], "it waits for their yes to what it uses",
            "{waits}"
        );
        let unknown = asked(
            ask("unread mail"),
            &keeping,
            owner,
            |_| Err("lamp-control couldn't be reached".to_owned()),
            approved,
            never,
        );
        assert!(unknown["error"].as_str().unwrap().contains("can't tell"));
        let off = asked(
            ask("evening dim"),
            &keeping,
            owner,
            no_task,
            approved,
            never,
        );
        assert_eq!(off["error"], "it isn't on the lamp now", "{off}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Only the owner's line, heard: anyone else's runs nothing and names
    /// no skill; a turn nobody spoke runs nothing.
    #[test]
    fn only_the_owners_heard_line_runs_a_skill() {
        let (root, keeping) = nines_lamp("run-not-owner");
        let told = asked(
            ask("unread mail"),
            &keeping,
            || false,
            no_task,
            approved,
            never,
        );
        assert_eq!(told["error"], NOT_OWNER, "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(!next.contains("unread"), "{next}");
        let unheard = asked(
            Ask {
                heard: false,
                ..ask("unread mail")
            },
            &keeping,
            || panic!("asked whose"),
            no_task,
            approved,
            never,
        );
        assert_eq!(unheard, json!({ "error": NOT_ASKED_ALOUD }));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A name that matches none of theirs runs nothing and names theirs
    /// that are on, to the owner.
    #[test]
    fn a_name_that_matches_none_runs_nothing() {
        let (root, keeping) = nines_lamp("run-no-name");
        let told = asked(ask("moon fact"), &keeping, owner, no_task, approved, never);
        assert_eq!(told["error"], made_remove::NO_MADE, "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("unread mail"), "{next}");
        assert!(!next.contains("evening dim"), "{next}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// lamp-9adf, 18 Nov, #7562's row: space facts made and on the lamp,
    /// turned off by the owner (lamp-skills' `off` file).
    fn space_lamp(test: &str) -> (PathBuf, Keeping) {
        let (root, keeping) = keeping(test);
        made_here(&keeping, &[("space-facts", true), ("unread-mail", true)]);
        (root, keeping)
    }

    /// lamp-skills as the voice asks it: space facts answers [`STOPPED`]
    /// while turned off, and `late` more times after its turn on (it reads
    /// its `off` file again within 2 s); then it runs, saying its own
    /// words. What was turned on and what was run, in order.
    struct Lamp {
        on: std::cell::Cell<bool>,
        late: std::cell::Cell<usize>,
        turned: std::cell::RefCell<Vec<String>>,
        ran: std::cell::RefCell<Vec<String>>,
    }

    impl Lamp {
        fn new(late: usize) -> Self {
            Self {
                on: false.into(),
                late: late.into(),
                turned: Vec::new().into(),
                ran: Vec::new().into(),
            }
        }

        fn run(&self, skill: &str) -> Result<bool, String> {
            self.ran.borrow_mut().push(skill.to_owned());
            if !self.on.get() {
                return Err(STOPPED.to_owned());
            }
            if self.late.get() > 0 {
                self.late.set(self.late.get() - 1);
                return Err(STOPPED.to_owned());
            }
            Ok(true)
        }

        fn turn_on(&self, skill: &str) -> Result<(), String> {
            self.turned.borrow_mut().push(skill.to_owned());
            self.on.set(true);
            Ok(())
        }
    }

    /// The model's `make_skill` run, as on lamp-9adf for #7562's requests 6
    /// and 9, of space facts while the owner has it turned off.
    fn run_off(lamp: &Lamp, keeping: &Keeping, off: bool, switch: bool) -> Value {
        let is_off = move |_: &str| off;
        let turn_on = |skill: &str| lamp.turn_on(skill);
        let turns = switch.then_some(TurnsOn {
            is_off: &is_off,
            turn_on: &turn_on,
            wait: std::time::Duration::from_millis(1),
        });
        asked_turning(
            ask("space facts"),
            keeping,
            owner,
            no_task,
            approved,
            |skill| lamp.run(skill),
            turns,
        )
    }

    /// The undo line said of space facts turned on for the ask.
    const UNDO: &str = "Space facts was off, so I turned it on. Say 'turn it off' to undo.";

    /// #7562's row on lamp-9adf, 18 Nov, request 6: "Lamp, use my space
    /// facts skill." with space facts turned off was the model's
    /// `make_skill` run, not the skill's function; it was refused "it is
    /// turned off or snoozed" and said "Space facts didn't run — it's turned
    /// off at the moment." With the switch on, the run turns it on for the
    /// owner's ask, as its function's call does (`made_turn::on_for_ask`),
    /// runs it, and tells the model the one line with its undo, asking
    /// nothing; a run still, never one that speaks before its undo.
    #[test]
    fn an_off_skill_asked_to_run_now_is_turned_on_run_and_said_with_its_undo() {
        let (root, keeping) = space_lamp("run-off-on");
        let lamp = Lamp::new(0);
        let told = run_off(&lamp, &keeping, true, true);
        assert_eq!(*lamp.turned.borrow(), ["space-facts"], "{told}");
        assert_eq!(*lamp.ran.borrow(), ["space-facts"], "{told}");
        assert_eq!(told[RUN], "space-facts", "{told}");
        assert_eq!(told["ran"], true, "{told}");
        assert_eq!(told[TURNED_ON], "space facts", "{told}");
        assert_eq!(ran_of(&told).as_deref(), Some("space-facts"), "{told}");
        assert!(!speaks_itself(&told), "the undo is said first: {told}");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains(&format!("\"{UNDO}\"")), "{next}");
        assert!(next.contains("conversation's language"), "{next}");
        assert!(next.contains("running now"), "{next}");
        assert!(!next.contains("didn't run"), "{next}");
        assert!(!next.contains('?'), "nothing asked: {next}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// lamp-skills reads its `off` file again within 2 s: a run just after
    /// the turn on that still answers off is run again after a short wait;
    /// still off after every try, the model is told it was just turned on,
    /// with its undo, never that it is off.
    #[test]
    fn an_off_skill_turned_on_for_a_run_is_run_again_while_still_off() {
        let (root, keeping) = space_lamp("run-off-late");
        let lamp = Lamp::new(1);
        let told = run_off(&lamp, &keeping, true, true);
        assert_eq!(lamp.ran.borrow().len(), 2, "{told}");
        assert_eq!(told["ran"], true, "{told}");
        assert_eq!(told[TURNED_ON], "space facts", "{told}");
        let lamp = Lamp::new(100);
        let told = run_off(&lamp, &keeping, true, true);
        assert_eq!(
            lamp.ran.borrow().len(),
            1 + crate::made_turn::RERUNS,
            "{told}"
        );
        assert_eq!(told["ran"]["error"], crate::made_turn::JUST_ON, "{told}");
        assert!(told["next"].as_str().unwrap().contains(UNDO), "{told}");
        assert!(!told.to_string().contains(STOPPED), "{told}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A snoozed skill (not in the `off` file) is never turned on: its run
    /// is refused as before and said so. With the switch off, a turned-off
    /// one is as before too: "didn't run now", nothing turned on.
    #[test]
    fn a_snoozed_skill_or_the_switch_off_turns_nothing_on() {
        let (root, keeping) = space_lamp("run-off-snoozed");
        for (off, switch) in [(false, true), (true, false)] {
            let lamp = Lamp::new(0);
            let told = run_off(&lamp, &keeping, off, switch);
            assert!(lamp.turned.borrow().is_empty(), "{told}");
            assert_eq!(*lamp.ran.borrow(), ["space-facts"], "{told}");
            assert_eq!(told["error"], STOPPED, "{told}");
            assert!(stopped(&told), "{told}");
            assert_eq!(ran_of(&told), None, "{told}");
            let next = told["next"].as_str().unwrap();
            assert!(next.contains("space facts didn't run now"), "{next}");
            assert!(!next.contains("undo"), "{next}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A turn on refused (the helper's, or a guest's at `make_skill`'s own
    /// `turn_on`) runs nothing and is said plainly with why; one whose
    /// task waits for its yes is never turned on.
    #[test]
    fn an_off_skill_not_turned_on_runs_nothing() {
        let (root, keeping) = space_lamp("run-off-refused");
        let is_off = |_: &str| true;
        let turn_on = |_: &str| Err(crate::made_turn::NOT_OWNER.to_owned());
        let turns = Some(TurnsOn {
            is_off: &is_off,
            turn_on: &turn_on,
            wait: std::time::Duration::from_millis(1),
        });
        let told = asked_turning(
            ask("space facts"),
            &keeping,
            owner,
            no_task,
            approved,
            never,
            turns,
        );
        let why = told["error"].as_str().unwrap();
        assert!(why.contains("turned off") && why.contains(crate::made_turn::NOT_OWNER));
        assert!(told["next"].as_str().unwrap().contains("didn't run now"));
        let lamp = Lamp::new(0);
        let lamp_on = |skill: &str| lamp.turn_on(skill);
        let waits = asked_turning(
            ask("space facts"),
            &keeping,
            owner,
            |_| Ok(Some(false)),
            approved,
            never,
            Some(TurnsOn {
                is_off: &is_off,
                turn_on: &lamp_on,
                wait: std::time::Duration::from_millis(1),
            }),
        );
        assert_eq!(waits["error"], "it waits for their yes to what it uses");
        assert!(lamp.turned.borrow().is_empty(), "{waits}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `make_skill` is offered for a run, a parameter of the function the
    /// voice has, within the link's bounds with every switch on and off.
    #[test]
    fn make_skill_is_offered_for_a_run_within_the_bounds() {
        for on in [true, false] {
            crate::made_skill::set_writes_here(Some(on));
            crate::made_skill::set_scripts_here(Some(on));
            crate::skill_task_yes::set_on_here(Some(on));
            let tool = crate::make_skill_local::tool();
            assert!(
                tool.description
                    .contains("to run one they made now, run alone"),
                "{}",
                tool.description
            );
            assert!(
                tool.description.len() <= 1_024,
                "{}",
                tool.description.len()
            );
            assert!(!tool.description.contains("yes"), "{}", tool.description);
            let parameters = tool.parameters.unwrap();
            assert!(parameters["properties"][RUN].is_object(), "{parameters}");
            assert!(parameters.to_string().len() <= 4_096);
        }
        crate::skill_task_yes::set_on_here(None);
        crate::made_skill::set_scripts_here(None);
        crate::made_skill::set_writes_here(None);
    }
}
