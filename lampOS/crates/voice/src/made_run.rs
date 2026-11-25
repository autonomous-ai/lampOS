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
    drop(ran);
    // Under way too, for a stop that ends it (#8475).
    ran_here(skill, at);
}

/// lamp-skills' call socket op that ends the run under way:
/// `{"op":"cancel","skill":"<name>"}` for the skill the voice asked to
/// run, never another's (the Reviewer-lead's P2 on #8535), answered `{"stopped": NAME|null, "ms": N|null}` (or
/// that inside `{"ok":true,"result":...}`): the skill it stopped and how
/// long after its start, or none (#8475).
pub const CANCEL_OP: &str = "cancel";

/// How long after its start a run the voice asked is still taken as under
/// way, for a stop to end it (#8475). A known approximation: lamp-skills
/// tells the voice no run's end, so a run is under way from its start until
/// it is stopped or this long has passed, whether or not it already ended
/// on its own. A stop sent to a run already over is answered `stopped:
/// null` and does nothing.
pub const UNDER_WAY_FOR: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// The last run the voice asked, as this process knows it, with when it
/// was answered `ran`: the coordinator's (told by the worker, [`ran_now`])
/// and the second route's worker's own ([`ran_here`]), each in its own
/// process (#8475).
#[derive(Debug, Default)]
pub struct UnderWay {
    run: Option<(String, std::time::Instant)>,
}

impl UnderWay {
    pub const fn new() -> Self {
        Self { run: None }
    }

    /// `skill` answered `ran` at `at`: the newest run, under way.
    pub fn ran(&mut self, skill: &str, at: std::time::Instant) {
        self.run = Some((skill.to_owned(), at));
    }

    /// The run under way at `now`, if one is: started within
    /// [`UNDER_WAY_FOR`] and not stopped since.
    pub fn under_way(&self, now: std::time::Instant) -> Option<(&str, std::time::Instant)> {
        self.run
            .as_ref()
            .filter(|(_, at)| now.saturating_duration_since(*at) <= UNDER_WAY_FOR)
            .map(|(skill, at)| (skill.as_str(), *at))
    }

    /// The run under way at `now`, taken as stopped: never stopped twice.
    pub fn stopped(&mut self, now: std::time::Instant) -> Option<(String, std::time::Instant)> {
        let run = self.run.take()?;
        (now.saturating_duration_since(run.1) <= UNDER_WAY_FOR).then_some(run)
    }

    /// A line heard while the lamp is quiet (`text`, the lamp's name heard
    /// for it: `named`; the said-to-me rule let it out: `facing`), when it
    /// is a stop said to the lamp ([`quiet_stop_said_to_lamp`]) and a run
    /// is under way: that run, taken as stopped. None otherwise, and the
    /// run stays under way.
    pub fn quiet_stop(
        &mut self,
        text: &str,
        named: bool,
        facing: bool,
        now: std::time::Instant,
    ) -> Option<String> {
        self.under_way(now)?;
        if !quiet_stop_said_to_lamp(text, named, facing) {
            return None;
        }
        self.stopped(now).map(|(skill, _)| skill)
    }
}

/// This process's [`UnderWay`].
#[cfg(not(test))]
static UNDER_WAY: std::sync::Mutex<UnderWay> = std::sync::Mutex::new(UnderWay::new());

#[cfg(test)]
thread_local! {
    /// In tests, each test thread's own: a run one test notes is never
    /// another's run under way, as tests run side by side.
    static UNDER_WAY: std::cell::RefCell<UnderWay> = const { std::cell::RefCell::new(UnderWay::new()) };
}

/// `f` on this process's [`UnderWay`].
#[cfg(not(test))]
fn with_under_way<R>(f: impl FnOnce(&mut UnderWay) -> R) -> R {
    let mut record = UNDER_WAY
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    f(&mut record)
}

/// `f` on this test thread's [`UnderWay`].
#[cfg(test)]
fn with_under_way<R>(f: impl FnOnce(&mut UnderWay) -> R) -> R {
    UNDER_WAY.with(|record| f(&mut record.borrow_mut()))
}

/// `skill` answered `ran` at `at`, in this process: the second route's
/// worker notes the run it asked, so a quiet stop said to the lamp can be
/// told from one said to the room ([`quiet_stop_taken`], #8475). Never
/// `RAN_NOW`: the say service reads that in the coordinator alone.
pub fn ran_here(skill: &str, at: std::time::Instant) {
    with_under_way(|record| record.ran(skill, at));
}

/// Whether `skill`, answered `ran` at `ran_at` or just before it was noted,
/// is still this process's run under way at `now`: not stopped, nor another
/// run noted since.
pub fn still_under_way(skill: &str, ran_at: std::time::Instant, now: std::time::Instant) -> bool {
    with_under_way(|record| {
        record
            .under_way(now)
            .is_some_and(|(name, at)| name == skill && at >= ran_at)
    })
}

/// [`UnderWay::quiet_stop`] on this process's record.
pub fn quiet_stop_taken(
    text: &str,
    named: bool,
    facing: bool,
    now: std::time::Instant,
) -> Option<String> {
    with_under_way(|record| record.quiet_stop(text, named, facing, now))
}

/// Words that soften a stop and say nothing else: "okay, stop", "please
/// stop", "stop now".
const STOP_SOFT: &[&str] = &[
    "okay", "ok", "please", "hey", "oh", "alright", "right", "just", "so", "now",
];

/// What may follow a bare "stop" in a short stop line: "stop it", "stop
/// that".
const STOP_WHAT: &[&str] = &["it", "that", "this", "there"];

/// The most words a stop said to the lamp by its name or face has, as a
/// spoken stop over its answer (`spoken_stop::is_stop_request`).
const QUIET_STOP_MOST_WORDS: usize = 6;

/// Whether a line heard while the lamp is NOT speaking is a stop said to
/// the lamp, so it ends a run under way (#8475). Over the lamp's own voice
/// the spoken stop's rules stand as they are; this is the quiet case only.
///
/// - **A short stop line on its own:** the whole line, soft words and the
///   lamp's name aside, is "stop", "stop it", "stop that" ("Stop.", "Okay,
///   stop.", "Please stop.", "Stop it.", "Lamp, stop.").
/// - **Named** (`named`, or the lamp's name in its words) **or said facing
///   it** (`facing`: the said-to-me rule let it out): a short ask to stop
///   that opens with "stop" past the name and soft words ("Lamp, stop the
///   photo booth."), never a question nor "don't stop".
/// - Never a "stop" inside a longer sentence ("Don't stop now.", "I can't
///   stop laughing.", "Tom, stop it." said to someone else).
pub fn quiet_stop_said_to_lamp(text: &str, named: bool, facing: bool) -> bool {
    if text.contains('?') {
        return false;
    }
    let name = crate::second_provider::net::key_terms::NAME.to_ascii_lowercase();
    let words = crate::not_speech::plain_words(text);
    let named = named || words.contains(&name);
    let own: Vec<&str> = words
        .iter()
        .map(String::as_str)
        .filter(|word| *word != name && !STOP_SOFT.contains(word))
        .collect();
    let short = match own.as_slice() {
        [] => false,
        ["stop", what] => STOP_WHAT.contains(what) || *what == "stop",
        all => all.iter().all(|word| *word == "stop"),
    };
    if short {
        return true;
    }
    (named || facing)
        && words.len() <= QUIET_STOP_MOST_WORDS
        && own.first() == Some(&"stop")
        && crate::spoken_stop::is_stop_request(text)
}

/// The journal's line for a run lamp-skills stopped at the voice's ask:
/// grepped to count the runs stopped within 10 s (#8475).
pub fn stopped_run_line(skill: &str, ms: u64) -> String {
    format!("lamp-voice: stopped run {skill} {ms} ms after start (#8475)")
}

/// The run lamp-skills says its `cancel` stopped ([`CANCEL_OP`]), and how
/// long after its start: its own `ms`, or `elapsed_ms` (the voice's count
/// from the `ran` answer) when it gives none. None when it stopped none.
pub fn stopped_of(answer: &Value, elapsed_ms: u64) -> Option<(String, u64)> {
    let told = match answer.get("result") {
        Some(result) if result.is_object() => result,
        _ => answer,
    };
    let skill = told["stopped"].as_str().filter(|skill| !skill.is_empty())?;
    Some((skill.to_owned(), told["ms"].as_u64().unwrap_or(elapsed_ms)))
}

/// Ask lamp-skills, on its call socket in `folder`, to stop the run under
/// way ([`CANCEL_OP`]), on a thread of its own, so the stop that asks it
/// waits for nothing: the stop's own reply cut is made before and without
/// it. `skill` and `started` are the voice's record of the run; `why` says
/// what stopped it. The thread journals the run stopped
/// ([`stopped_run_line`]) and gives it back. None if no thread could start.
pub fn cancel_run(
    folder: &Path,
    skill: String,
    started: std::time::Instant,
    why: &'static str,
) -> Option<std::thread::JoinHandle<Option<(String, u64)>>> {
    let socket = folder.join(crate::tools::CALL_SOCKET);
    let spawned = std::thread::Builder::new()
        .name("lamp-voice-run-stop".into())
        .spawn(move || {
            eprintln!("lamp-voice: {why}: the run of {skill} under way is asked to stop (#8475)");
            let answer = crate::tools::exchange(
                &socket,
                &json!({ "op": CANCEL_OP, "skill": skill }),
                crate::tools::ASK_DEADLINE,
            );
            let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let stopped = answer
                .as_ref()
                .and_then(|answer| stopped_of(answer, elapsed_ms));
            match (&answer, &stopped) {
                (_, Some((stopped, ms))) => eprintln!("{}", stopped_run_line(stopped, *ms)),
                (Some(_), None) => eprintln!(
                    "lamp-voice: {why}: lamp-skills had no run of {skill} to stop (#8475)"
                ),
                (None, None) => eprintln!(
                    "lamp-voice: {why}: lamp-skills didn't answer the stop of {skill}'s run (#8475)"
                ),
            }
            stopped
        });
    match spawned {
        Ok(handle) => Some(handle),
        Err(error) => {
            eprintln!("lamp-voice: {why}: the run's stop wasn't sent: {error} (#8475)");
            None
        }
    }
}

/// A stop (the button's, or a spoken one) with this process's run under
/// way at `now`: that run taken as stopped and lamp-skills asked to stop
/// it, on lamp-skills' call socket in `folder` ([`cancel_run`]). None when
/// no run is under way.
pub fn stop_under_way(
    folder: &Path,
    now: std::time::Instant,
    why: &'static str,
) -> Option<std::thread::JoinHandle<Option<(String, u64)>>> {
    let (skill, started) = with_under_way(|record| record.stopped(now))?;
    cancel_run(folder, skill, started, why)
}

/// How long after a run's `ran` answer its name line waits, so a run whose
/// own first sound, or the lamp's reply, starts within it says no more
/// (#8475).
pub const NAME_AFTER: std::time::Duration = std::time::Duration::from_secs(1);

/// The latest after its `ran` answer a run's name line is still said, when
/// a turn is in hand all that while: then the turn speaks for it.
pub const NAME_LATEST: std::time::Duration = std::time::Duration::from_secs(5);

/// The few words naming a run the voice asked, from the skill's own folder
/// name, never the model's text: "Running photo booth." (#8475).
pub fn name_line(skill: &str) -> String {
    format!("Running {}.", made_skill::spoken_name(skill))
}

/// Whether a run's name line is said with the lamp speaking `language`:
/// only in English, since its words are English and, holding a skill's name,
/// are none of the fixed lines each language has words for (#4473). In
/// another language no line is said, never English in its place.
pub fn names_runs_in(language: Option<crate::language::Language>) -> bool {
    language.is_none()
}

/// What a run's name line does on a pass ([`NameLine::step`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameStep {
    /// Not yet.
    Wait,
    /// Say these words now, once.
    Say(String),
    /// Never said, and why.
    Drop(&'static str),
}

/// A run's name line, owed from its `ran` answer (#8475): said once,
/// [`NAME_AFTER`] after it, unless the lamp began speaking (the run's own
/// say, or a reply's audio) meanwhile, or the run was stopped.
#[derive(Clone, Debug)]
pub struct NameLine {
    skill: String,
    ran_at: std::time::Instant,
    ran_us: u64,
    spoke: bool,
    done: bool,
}

impl NameLine {
    /// `skill` answered `ran` at `ran_at` (`ran_us` on the voice's
    /// monotonic clock).
    pub fn after_ran(skill: &str, ran_at: std::time::Instant, ran_us: u64) -> Self {
        Self {
            skill: skill.to_owned(),
            ran_at,
            ran_us,
            spoke: false,
            done: false,
        }
    }

    pub fn skill(&self) -> &str {
        &self.skill
    }

    pub fn ran_at(&self) -> std::time::Instant {
        self.ran_at
    }

    /// When the `ran` answer came, on the voice's monotonic clock: speech
    /// begun at or after it counts as the lamp speaking.
    pub fn ran_us(&self) -> u64 {
        self.ran_us
    }

    /// One pass at `now`: `spoke_now`, the lamp is heard speaking since the
    /// `ran` answer; `busy`, a turn is in hand; `under_way`, the run is not
    /// stopped. Says or drops it once; [`NameStep::Wait`] after that.
    pub fn step(
        &mut self,
        now: std::time::Instant,
        spoke_now: bool,
        busy: bool,
        under_way: bool,
    ) -> NameStep {
        if self.done {
            return NameStep::Wait;
        }
        self.spoke |= spoke_now;
        let since = now.saturating_duration_since(self.ran_at);
        let step = if !under_way {
            NameStep::Drop("the run was stopped")
        } else if self.spoke {
            NameStep::Drop("the lamp began speaking")
        } else if since < NAME_AFTER || (busy && since <= NAME_LATEST) {
            // Too soon, or a turn in hand that may still end within the latest.
            NameStep::Wait
        } else if busy {
            NameStep::Drop("a turn was still in hand")
        } else {
            NameStep::Say(name_line(&self.skill))
        };
        self.done = step != NameStep::Wait;
        step
    }
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
    /// The skill turned off again, as the app's switch
    /// (`made_turn::turned_by_helper`), when its run after the turn on
    /// failed: the owner's state as it was (Heron's P2 on #7562).
    pub turn_off: &'a dyn Fn(&str) -> Result<(), String>,
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

/// The key of a run's refusal naming the made skill that didn't run
/// ([`asked_turning`]): so a failed run of the skill the model's own
/// `turn_on` turned on in the turn is known, and that skill turned off
/// again (`made_turn::failed_after_turn_on`).
pub const NOT_RAN: &str = "not_ran";

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
        // Turned on for the ask, then refused (it runs only at its own
        // moment, or lamp-skills didn't answer): turned off again, so the
        // owner's state is as it was, and said plainly (Heron's P2 on
        // #7562).
        Err(why) if turned_on => {
            let restored = turns.map_or(Err(String::new()), |turns| (turns.turn_off)(&made));
            if restored.is_ok() {
                eprintln!(
                    "lamp-voice: request {request}: a made skill turned on for the ask didn't run now: turned off again: {why}"
                );
                crate::made_turn::not_run_still_off(&spoken, &why)
            } else {
                eprintln!(
                    "lamp-voice: request {request}: a made skill turned on for the ask didn't run now, and wasn't turned off again: {why}"
                );
                crate::made_turn::not_run_left_on(&spoken, &why)
            }
        }
        Err(why) => {
            eprintln!("lamp-voice: request {request}: a made skill wasn't run now: {why}");
            let mut told = not_run(&spoken, &why);
            told[NOT_RAN] = json!(made);
            told
        }
    }
}

/// Of a `use_skill` call's `skill` (`named`), which names no skill's
/// function, the owner's own made skill it names ([`made_remove::found`]),
/// when that one is turned off (`turned_off`, lamp-skills' `off` file;
/// never a snooze): its name. A made skill has a trigger and no function,
/// so `use_skill` can't name it: on lamp-9adf, 18 Nov (#7562's row),
/// "Lamp, tell me a space fact with my space facts skill." with space facts
/// turned off was the model's `use_skill`, told only that no skill has that
/// name. Which skill the line asks for stays the model's call: no word of
/// the line is read.
pub fn off_named(
    named: &str,
    keeping: &Keeping,
    turned_off: &std::collections::BTreeSet<String>,
) -> Option<String> {
    match made_remove::found(named, keeping) {
        Found::Made { name, .. } if turned_off.contains(&name) => Some(name),
        _ => None,
    }
}

/// A `use_skill` call `call` that named no skill's function (its refusal
/// came first), on the lamp, with `LAMP_MADE_SKILL_OFF_SAID` on and a heard
/// line: when it names a made skill turned off ([`off_named`]), the run now
/// of [`asked_here`], as `make_skill`'s run of it: the owner's alone, turned
/// on for the ask, run, and said with its undo (turned off again if the run
/// fails). None otherwise, and with the switch off: the refusal as before.
pub fn use_skill_off_here(
    call: &lamp_gemini::ToolCall,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
    skills: Option<&Path>,
) -> Option<Value> {
    if !heard || !made_skill::off_said_on() {
        return None;
    }
    let named = call.args.get("skill").and_then(Value::as_str)?;
    let keeping = Keeping::when(true)?;
    let turned_off =
        crate::skill_about::turned_off(Path::new(crate::skill_about::STATE_DIR)).ok()?;
    let made = off_named(named, &keeping, &turned_off)?;
    eprintln!(
        "lamp-voice: request {request}: use_skill named a made skill turned off: its run now (LAMP_MADE_SKILL_OFF_SAID)"
    );
    Some(asked_here(&made, request, heard, lease, skills))
}

/// Whether one `make_skill` call's `turn_on` (`on`) and run now (`run`)
/// name one skill: the same words as a skill's name is written
/// ([`made_remove::apart`]), or the same made skill of theirs found by each
/// ([`made_remove::found`], with `keeping`).
pub fn one_skill(on: &str, run: &str, keeping: Option<&Keeping>) -> bool {
    if made_remove::apart(on, run) == 0 && made_remove::apart(run, on) == 0 {
        return true;
    }
    keeping.is_some_and(|keeping| {
        match (
            made_remove::found(on, keeping),
            made_remove::found(run, keeping),
        ) {
            (Found::Made { name: a, .. }, Found::Made { name: b, .. }) => a == b,
            _ => false,
        }
    })
}

/// What the model is told of one `make_skill` call whose `turn_on` and run
/// now name two skills (Heron's P3 on #7562: the run alone ran, the turn on
/// was dropped): nothing done, and each asked again as its own call.
pub fn two_skills(on: &str, run: &str) -> Value {
    json!({
        "error": "one call turned on one skill and ran another, so nothing was done",
        "next": format!(
            "Nothing was done yet. Call make_skill again now with turn_on and \"{on}\" alone, \
             then with run and \"{run}\" alone, saying nothing first; each answer says what to \
             say."
        )
    })
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
    // Back off as the app's switch: the owner's own state restored, so no
    // owner's check again, nor a remove's wait dropped.
    let turn_off = |skill: &str| crate::made_turn::turned_by_helper(&manage, skill, true);
    let turns = made_skill::off_said_on().then_some(TurnsOn {
        is_off: &is_off,
        turn_on: &turn_on,
        turn_off: &turn_off,
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

    /// #7874 on c0a4: OpenClaw made `my-day`, on, but "run my day" found
    /// it and then looked it up again as `day`, so it "isn't on". A skill
    /// named `desk-check` asked as "my desk check skill" still runs.
    #[test]
    fn a_made_skill_named_my_something_runs_now() {
        let (root, keeping) = keeping("run-my-day");
        made_here(&keeping, &[("my-day", true), ("desk-check", true)]);
        for (heard, name) in [
            ("my day", "my-day"),
            ("my-day", "my-day"),
            ("my desk check skill", "desk-check"),
        ] {
            let mut ran = Vec::new();
            let told = asked(ask(heard), &keeping, owner, no_task, approved, |skill| {
                ran.push(skill.to_owned());
                Ok(false)
            });
            assert_eq!(ran, [name], "{heard}: {told}");
            assert_eq!(told[RUN], name, "{heard}: {told}");
        }
        let _ = std::fs::remove_dir_all(&root);
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
    /// words, unless it refuses the run (`refuses`, in lamp-skills' words).
    /// What was turned on, turned off and run, in order; a turn off refused
    /// when `keeps_on`.
    struct Lamp {
        on: std::cell::Cell<bool>,
        late: std::cell::Cell<usize>,
        refuses: Option<&'static str>,
        keeps_on: bool,
        turned: std::cell::RefCell<Vec<String>>,
        offed: std::cell::RefCell<Vec<String>>,
        ran: std::cell::RefCell<Vec<String>>,
    }

    impl Lamp {
        fn new(late: usize) -> Self {
            Self {
                on: false.into(),
                late: late.into(),
                refuses: None,
                keeps_on: false,
                turned: Vec::new().into(),
                offed: Vec::new().into(),
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
            if let Some(why) = self.refuses {
                return Err(why.to_owned());
            }
            Ok(true)
        }

        fn turn_on(&self, skill: &str) -> Result<(), String> {
            self.turned.borrow_mut().push(skill.to_owned());
            self.on.set(true);
            Ok(())
        }

        fn turn_off(&self, skill: &str) -> Result<(), String> {
            self.offed.borrow_mut().push(skill.to_owned());
            if self.keeps_on {
                return Err("the lamp's skills couldn't be reached".to_owned());
            }
            self.on.set(false);
            Ok(())
        }
    }

    /// The model's `make_skill` run, as on lamp-9adf for #7562's requests 6
    /// and 9, of space facts while the owner has it turned off.
    fn run_off(lamp: &Lamp, keeping: &Keeping, off: bool, switch: bool) -> Value {
        let is_off = move |_: &str| off;
        let turn_on = |skill: &str| lamp.turn_on(skill);
        let turn_off = |skill: &str| lamp.turn_off(skill);
        let turns = switch.then_some(TurnsOn {
            is_off: &is_off,
            turn_on: &turn_on,
            turn_off: &turn_off,
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
        let turn_off = |_: &str| -> Result<(), String> { panic!("nothing was turned on") };
        let turns = Some(TurnsOn {
            is_off: &is_off,
            turn_on: &turn_on,
            turn_off: &turn_off,
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
                turn_off: &turn_off,
                wait: std::time::Duration::from_millis(1),
            }),
        );
        assert_eq!(waits["error"], "it waits for their yes to what it uses");
        assert!(lamp.turned.borrow().is_empty(), "{waits}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Heron's P2 on #7562: space facts turned on for the owner's ask,
    /// then its run refused (it does something only at its own moment, or
    /// lamp-skills didn't answer): turned off again, as the owner had it,
    /// and said plainly that it didn't run and is still off, with no undo
    /// and never as a run. A turn off that fails leaves it on: said so,
    /// with the undo line.
    #[test]
    fn a_run_refused_after_the_turn_on_leaves_it_off() {
        let (root, keeping) = space_lamp("run-off-then-refused");
        for why in ["it has nothing to do but at its own moment", NOT_ANSWERED] {
            let lamp = Lamp {
                refuses: Some(why),
                ..Lamp::new(0)
            };
            let told = run_off(&lamp, &keeping, true, true);
            assert_eq!(*lamp.turned.borrow(), ["space-facts"], "{told}");
            assert_eq!(*lamp.offed.borrow(), ["space-facts"], "{told}");
            assert!(!lamp.on.get(), "left off: {told}");
            assert_eq!(told["error"], why, "{told}");
            assert_eq!(ran_of(&told), None, "{told}");
            assert!(told.get(TURNED_ON).is_none(), "{told}");
            let next = told["next"].as_str().unwrap();
            assert!(next.contains("space facts didn't run now"), "{next}");
            assert!(next.contains("still off"), "{next}");
            assert!(!next.contains("undo"), "{next}");
        }
        let lamp = Lamp {
            refuses: Some("it has nothing to do but at its own moment"),
            keeps_on: true,
            ..Lamp::new(0)
        };
        let told = run_off(&lamp, &keeping, true, true);
        assert!(lamp.on.get(), "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("didn't run now"), "{next}");
        assert!(next.contains(&format!("\"{UNDO}\"")), "{next}");
        // Not turned on for the ask (the switch off, or it was on): a
        // refusal is as before, nothing turned off.
        let lamp = Lamp {
            refuses: Some("it has nothing to do but at its own moment"),
            ..Lamp::new(0)
        };
        lamp.on.set(true);
        let told = run_off(&lamp, &keeping, false, true);
        assert!(lamp.offed.borrow().is_empty(), "{told}");
        assert_eq!(told[NOT_RAN], "space-facts", "{told}");
        assert!(!told["next"].as_str().unwrap().contains("still off"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #7562's row on lamp-9adf, 18 Nov: "Lamp, tell me a space fact with
    /// my space facts skill." with space facts turned off was the model's
    /// `use_skill`, which names no made skill: the name it gave is matched
    /// to theirs as a run's is, and only one turned off is taken (a snooze,
    /// one on, or one not theirs is the refusal as before).
    #[test]
    fn use_skill_naming_a_made_skill_turned_off_finds_it() {
        let (root, keeping) = space_lamp("use-skill-off");
        let off: std::collections::BTreeSet<String> = ["space-facts".to_owned()].into();
        for named in ["space_facts", "space facts", "my space facts skill"] {
            assert_eq!(
                off_named(named, &keeping, &off).as_deref(),
                Some("space-facts"),
                "{named}"
            );
        }
        assert_eq!(off_named("unread mail", &keeping, &off), None);
        assert_eq!(off_named("dance", &keeping, &off), None);
        assert_eq!(
            off_named("space facts", &keeping, &Default::default()),
            None
        );
        let call = lamp_gemini::ToolCall {
            id: "1".into(),
            name: crate::use_skill::NAME.into(),
            args: json!({"skill": "space_facts"}),
        };
        for switch in [true, false] {
            made_skill::set_off_said_here(Some(switch));
            // Unheard, or the switch off: the refusal as before.
            assert_eq!(use_skill_off_here(&call, 7, false, None, None), None);
        }
        made_skill::set_off_said_here(Some(false));
        assert_eq!(use_skill_off_here(&call, 7, true, None, None), None);
        made_skill::set_off_said_here(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Heron's P3 on #7562: one call's `turn_on` and run now name one
    /// skill, or two; two are never one done and the other dropped.
    #[test]
    fn a_turn_on_and_a_run_name_one_skill_or_two() {
        let (root, keeping) = space_lamp("one-skill-or-two");
        assert!(one_skill("space facts", "space_facts", None));
        assert!(one_skill(
            "space facts",
            "my space facts skill",
            Some(&keeping)
        ));
        assert!(!one_skill("space facts", "unread mail", Some(&keeping)));
        assert!(!one_skill("space facts", "unread mail", None));
        let told = two_skills("space facts", "unread mail");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("turn_on and \"space facts\" alone"), "{next}");
        assert!(next.contains("run and \"unread mail\" alone"), "{next}");
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

    /// A fake lamp-skills call socket in a folder of its own, answering one
    /// line with `answer`: the folder, and the line it was asked.
    fn skills_answering(test: &str, answer: Value) -> (PathBuf, std::thread::JoinHandle<Value>) {
        use std::io::{BufRead as _, Write as _};
        let folder = std::env::temp_dir().join(format!("run-stop-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let listener =
            std::os::unix::net::UnixListener::bind(folder.join(crate::tools::CALL_SOCKET)).unwrap();
        let served = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            std::io::BufReader::new(&stream)
                .read_line(&mut line)
                .unwrap();
            writeln!(&stream, "{answer}").unwrap();
            serde_json::from_str(&line).unwrap()
        });
        (folder, served)
    }

    /// #8475: a short "stop" heard while the lamp is quiet, with a run the
    /// voice asked under way, takes that run as stopped and asks lamp-skills
    /// to end it, on its call socket's `cancel`; with no run under way, or
    /// one started over 15 min ago, nothing is sent.
    #[test]
    fn a_short_quiet_stop_with_a_run_under_way_sends_the_cancel() {
        use std::time::{Duration, Instant};
        for short in [
            "Stop.",
            "stop",
            "Stop it.",
            "Okay, stop.",
            "Stop that!",
            "Please stop.",
            "Stop, stop.",
            "Okay stop now.",
        ] {
            assert!(quiet_stop_said_to_lamp(short, false, false), "{short}");
        }
        let now = Instant::now();
        let mut runs = UnderWay::new();
        assert_eq!(runs.quiet_stop("Stop.", false, false, now), None, "no run");
        runs.ran("photo-booth", now);
        let later = now + Duration::from_secs(4);
        let skill = runs.quiet_stop("Stop.", false, false, later);
        assert_eq!(skill.as_deref(), Some("photo-booth"));
        // Taken as stopped: a second stop sends nothing more.
        assert_eq!(runs.quiet_stop("Stop.", false, false, later), None);
        let (folder, served) = skills_answering(
            "quiet",
            json!({"ok": true, "result": {"stopped": "photo-booth", "ms": 4_200}}),
        );
        let stopped = cancel_run(&folder, skill.unwrap(), now, "a test's stop")
            .expect("its thread")
            .join()
            .unwrap();
        assert_eq!(
            served.join().unwrap(),
            json!({"op": CANCEL_OP, "skill": "photo-booth"})
        );
        assert_eq!(stopped, Some(("photo-booth".to_owned(), 4_200)));
        let _ = std::fs::remove_dir_all(&folder);
        // Started more than 15 min ago: no longer taken as under way.
        let mut old = UnderWay::new();
        old.ran("photo-booth", now);
        let past = now + UNDER_WAY_FOR + Duration::from_secs(1);
        assert_eq!(old.quiet_stop("Stop.", false, false, past), None);
        // lamp-skills not there: the stop's thread ends with nothing.
        let gone = cancel_run(
            Path::new("/nonexistent-run-stop"),
            "photo-booth".to_owned(),
            now,
            "a test's stop",
        )
        .expect("its thread")
        .join()
        .unwrap();
        assert_eq!(gone, None);
    }

    /// #8475: a "stop" inside a longer sentence, heard while the lamp is
    /// quiet, is no stop of the run, named, facing or not: the run stays
    /// under way and nothing is sent.
    #[test]
    fn a_quiet_sentence_with_stop_in_it_stops_no_run() {
        let now = std::time::Instant::now();
        let mut runs = UnderWay::new();
        runs.ran("photo-booth", now);
        for sentence in [
            "Don't stop now.",
            "Don\u{2019}t stop now.",
            "I can't stop laughing.",
            "Tom, stop it.",
            "You two, stop it right now.",
            "We had to stop at the shop on the way.",
            "Why did it stop?",
            "Stop?",
        ] {
            for (named, facing) in [(false, false), (false, true), (true, false), (true, true)] {
                assert!(
                    !quiet_stop_said_to_lamp(sentence, named, facing),
                    "{sentence} named {named} facing {facing}"
                );
                assert_eq!(runs.quiet_stop(sentence, named, facing, now), None);
            }
        }
        assert_eq!(
            runs.under_way(now).map(|(skill, _)| skill),
            Some("photo-booth")
        );
        // A longer ask to stop counts only named or said facing the lamp.
        assert!(!quiet_stop_said_to_lamp(
            "Stop the photo booth.",
            false,
            false
        ));
        assert!(quiet_stop_said_to_lamp(
            "Stop the photo booth.",
            true,
            false
        ));
        assert!(quiet_stop_said_to_lamp(
            "Stop the photo booth.",
            false,
            true
        ));
    }

    /// #8475: "Lamp, stop." heard while quiet names the lamp: it stops the
    /// run under way.
    #[test]
    fn lamp_stop_said_while_quiet_stops_the_run() {
        for named in [
            "Lamp, stop.",
            "Lamp stop",
            "Stop, Lamp!",
            "Hey Lamp, stop it.",
            "Lamp, stop the photo booth.",
        ] {
            assert!(quiet_stop_said_to_lamp(named, false, false), "{named}");
        }
        let now = std::time::Instant::now();
        let mut runs = UnderWay::new();
        runs.ran("photo-booth", now);
        assert_eq!(
            runs.quiet_stop("Lamp, stop.", false, false, now).as_deref(),
            Some("photo-booth")
        );
        assert_eq!(runs.under_way(now), None);
    }

    /// #8475: a run's name line is said once, about 1 s after its `ran`
    /// answer, in words made from the skill's own folder name; with a turn
    /// in hand at 1 s it waits for that turn to end quietly.
    #[test]
    fn a_runs_name_line_is_said_once_from_its_skill_name() {
        use std::time::Duration;
        assert_eq!(name_line("photo-booth"), "Running photo booth.");
        assert_eq!(name_line("moon-time"), "Running moon time.");
        // English only: in another language no line is said.
        assert!(names_runs_in(None));
        assert!(!names_runs_in(
            crate::language::Language::parse("vi").unwrap()
        ));
        let ran = std::time::Instant::now();
        let mut line = NameLine::after_ran("photo-booth", ran, 1_000);
        assert_eq!(line.ran_us(), 1_000);
        assert_eq!(
            line.step(ran + Duration::from_millis(500), false, false, true),
            NameStep::Wait
        );
        assert_eq!(
            line.step(ran + NAME_AFTER, false, false, true),
            NameStep::Say("Running photo booth.".to_owned())
        );
        // Once only.
        assert_eq!(
            line.step(ran + Duration::from_secs(2), false, false, true),
            NameStep::Wait
        );
        let mut held = NameLine::after_ran("photo-booth", ran, 1_000);
        assert_eq!(
            held.step(ran + Duration::from_millis(1_100), false, true, true),
            NameStep::Wait
        );
        assert_eq!(
            held.step(ran + Duration::from_secs(2), false, false, true),
            NameStep::Say("Running photo booth.".to_owned())
        );
    }

    /// #8475: the lamp began speaking within the second (the run's own say,
    /// or the reply's audio): the name line is dropped and never said later;
    /// so too for a run stopped, or a turn still in hand past
    /// [`NAME_LATEST`].
    #[test]
    fn a_runs_name_line_is_dropped_when_speech_begins_within_a_second() {
        use std::time::Duration;
        let ran = std::time::Instant::now();
        let mut line = NameLine::after_ran("photo-booth", ran, 1_000);
        assert_eq!(
            line.step(ran + Duration::from_millis(300), true, true, true),
            NameStep::Drop("the lamp began speaking")
        );
        assert_eq!(
            line.step(ran + Duration::from_secs(2), false, false, true),
            NameStep::Wait
        );
        let mut stopped = NameLine::after_ran("photo-booth", ran, 1_000);
        assert_eq!(
            stopped.step(ran + NAME_AFTER, false, false, false),
            NameStep::Drop("the run was stopped")
        );
        let mut busy = NameLine::after_ran("photo-booth", ran, 1_000);
        assert_eq!(
            busy.step(ran + NAME_AFTER, false, true, true),
            NameStep::Wait
        );
        assert_eq!(
            busy.step(
                ran + NAME_LATEST + Duration::from_millis(1),
                false,
                true,
                true
            ),
            NameStep::Drop("a turn was still in hand")
        );
    }

    /// #8475: the journal's line for a run stopped, grepped to count those
    /// stopped within 10 s, from lamp-skills' answer, bare or wrapped, its
    /// own `ms` first; none when it stopped none.
    #[test]
    fn a_stopped_run_is_journaled_with_its_skill_and_ms() {
        assert_eq!(
            stopped_run_line("photo-booth", 4_200),
            "lamp-voice: stopped run photo-booth 4200 ms after start (#8475)"
        );
        assert_eq!(
            stopped_of(&json!({"stopped": "photo-booth", "ms": 4_200}), 9),
            Some(("photo-booth".to_owned(), 4_200))
        );
        assert_eq!(
            stopped_of(
                &json!({"ok": true, "result": {"stopped": "photo-booth", "ms": null}}),
                9
            ),
            Some(("photo-booth".to_owned(), 9))
        );
        assert_eq!(stopped_of(&json!({"stopped": null, "ms": null}), 9), None);
        assert_eq!(stopped_of(&json!({"ok": false, "error": "no"}), 9), None);
    }
}
