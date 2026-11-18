//! The person's yes to a made skill that sends or writes (#6066, the
//! owner's lift of rule (b), 10 Nov; `LAMP_SKILL_WRITES`).
//!
//! A made skill with a write is put on as any made skill is, and its
//! say-back ends on [`crate::made_skill::WAITS_FOR_YES`]: nothing is sent
//! until the person agrees. Its writes then wait here ([`wait_for_yes`]),
//! in memory, one skill at a time, for [`YES_WITHIN`], with an id the
//! voice's model is given (`approve_id`), never the writes themselves.
//!
//! A made skill with code of its own (#6307, `LAMP_MADE_SCRIPTS`) waits
//! here the same way, with every guard below, but is not on until then:
//! its say-back ends on [`crate::made_skill::RUNS_AFTER_YES`], and the yes
//! puts on the very package said back, by its SHA-256
//! ([`crate::made_skill::put_on_agreed`]), so a changed script asks again.
//!
//! - **The yes** is the model's to judge, in any words, as #6009 has a
//!   waiting mail's send judged: on an agreement it calls `make_skill` again
//!   with only that id. The voice itself then calls `lamp-services`'
//!   `skill_write_approve` ([`crate::services::skill_write_approve`]) with
//!   the skill's name, its writes' exact contents and the SHA-256 of the
//!   package said back, after the skill is on: `lamp-services` approves
//!   nothing when another package of its name is installed by then (App
//!   Only's P2 on #6142). A decline, a wait or a change calls nothing:
//!   nothing is approved, and a change is a new make.
//! - **Said whole first** (App Only's P1 on #6142): the lamp's own words in
//!   one request from the make's on, as played and not cut ([`lamp_said`],
//!   [`lamp_cut`], told by the thread as a waiting write's read-back is),
//!   must hold every word of each write's say-back
//!   ([`crate::made_skill::write_said`]) in order
//!   ([`crate::services::read_back_said`]). A yes before that is not taken,
//!   and the model is told to say it again whole ([`NOT_SAID_WHOLE`]); the
//!   wait goes on.
//! - **Only a later heard line** of the second route, the one with the echo
//!   judge ([`crate::services::sends_here`]): never the make's own request,
//!   a turn nobody spoke, or the Live route.
//! - **The echo hold.** While a yes is awaited, a line made only of the
//!   lamp's own words, begun over its voice or within the echo tail
//!   (`second_provider::own_echo::yes_echo`), is held even in the private
//!   room, so the say-back's own last words heard back never reach the
//!   model; such a line's request is also kept here ([`echo_held`]), and an
//!   approval in it is refused.
//! - **No identity check** (the owner, 10 Nov): whoever is at the lamp is
//!   its owner (`LAMP_PRIVATE_ROOM`).
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How long a made skill's writes wait for the yes: as a prepared write
/// waits for its send (`crate::services::PENDING_FOR`).
pub const YES_WITHIN: Duration = crate::services::PENDING_FOR;
/// The `make_skill` argument that carries the yes.
pub const APPROVE_ID: &str = "approve_id";
/// How many of the lamp's answers a wait keeps, each at most
/// [`SAID_MOST`] bytes.
const SAID_KEPT: usize = 6;
const SAID_MOST: usize = 6_000;

/// What a made skill's yes would approve.
#[derive(Clone, Debug, PartialEq)]
pub struct Asks {
    pub skill: String,
    /// The package made, put on and said back.
    pub sha256: String,
    pub writes: Vec<Value>,
    /// What the lamp must have said whole before the yes: each write's
    /// plain words ([`crate::made_skill::write_said`]); for code of its
    /// own, what its code may use ([`crate::made_skill::code_said`]).
    pub say_back: String,
    /// For a skill's `do.task` (#6305, [`crate::skill_task_yes`]): the
    /// grants said back, which the yes covers with `sha256`, then the
    /// skill's folder's; `writes` is empty. None for a made skill's writes.
    pub grants: Option<Vec<String>>,
    /// It runs code of its own (#6307, `LAMP_MADE_SCRIPTS`), with no
    /// write: the yes puts that package on by its SHA-256
    /// ([`crate::made_skill::put_on_agreed`]), then keeps its task for the
    /// folder as it went on and the `grants` said
    /// ([`crate::make_skill_local::code_approve`]); never approves a send.
    pub code: bool,
    /// A made skill the person asked to remove by voice (#6960,
    /// [`crate::made_remove`]): the yes removes it, by its name and
    /// `sha256`; `say_back` is the one question asked, nothing else is
    /// approved.
    pub remove: bool,
}

/// A made skill's writes waiting for the yes.
#[derive(Clone, Debug, PartialEq)]
pub struct Waiting {
    pub id: String,
    pub asks: Asks,
    /// The request the skill was made in: its yes comes in a later one.
    pub request: u64,
    pub at: Instant,
    /// Later requests whose line was held as the lamp's own echo.
    pub echoes: Vec<u64>,
    /// The lamp's words in each request from the make's on, and whether
    /// that answer was cut.
    pub said: VecDeque<(u64, String, bool)>,
    /// A wait the voice's model began with no call of its own (an app's
    /// make with code, #6307, [`wait_for_yes_noted`]). Every wait's id is
    /// told in [`waiting_note`] alike, and every wait's yes is the owner's
    /// only (`make_skill_local::not_owners_yes_refused`).
    pub noted: bool,
    /// The person's answer to it was read as unclear once ([`answered`]):
    /// a second unclear keeps nothing and ends the wait.
    pub unclear: bool,
    /// How many heard turns past [`YES_WITHIN`] have been guarded against
    /// saying it is on (#7270, [`late_guarded`]).
    pub late_guarded: u8,
    /// Later requests whose line, as the model was asked it, was only the
    /// lamp's name or no words at all ([`line_heard`]), at most
    /// [`SAID_KEPT`]: no answer is taken in one, and the wait goes on.
    pub name_only: Vec<u64>,
    /// The newest later request whose line the second route asks of the
    /// model ([`line_heard`]): its round is not forced to answer when that
    /// line was only the lamp's name ([`approve_forced_now`]).
    pub newest_line: u64,
    /// When each request's words from the make's on were played to their
    /// end, in monotonic µs ([`played_until`]): where the carry of a yes
    /// counts from (`LAMP_SKILL_YES_CARRY`, #7148, [`asked_until`]).
    pub played: VecDeque<(u64, u64)>,
    /// A remove's (#6960) answer line ([`line_heard`]): the first later
    /// line that was more than the lamp's name, its request and its words
    /// (in memory only). Only that line may confirm the remove, and only as
    /// a plain yes ([`not_a_plain_yes`]); a later one drops the wait. The
    /// question asked again (an unclear answer, or a yes before it was
    /// heard whole) clears it: the line after that answers.
    pub answer_line: Option<(u64, String)>,
    /// Later requests whose rounds called anything but a wait's answer
    /// ([`round_called`]), at most [`SAID_KEPT`]: no remove is confirmed in
    /// one.
    pub other_calls: Vec<u64>,
    /// For a skill's task's wait (#6305): later requests whose heard line
    /// named a skill other than the waiting one ([`other_skill_named`],
    /// #7634), with that name as said, at most [`SAID_KEPT`]. A yes in one
    /// is never this wait's ([`answered`]).
    pub other_named: Vec<(u64, String)>,
}

/// A skill's task, or a made skill's code, just agreed to (#6749 A): its
/// name, the request the yes came in, and when. Told on the lines after it
/// ([`waiting_note`]), as this route's history keeps no call's answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Agreed {
    pub skill: String,
    pub request: u64,
    pub at: Instant,
}

#[cfg(not(test))]
mod kept {
    use super::{Agreed, Waiting};
    use std::sync::Mutex;
    static WAITING: Mutex<Option<Waiting>> = Mutex::new(None);
    static AGREED: Mutex<Option<Agreed>> = Mutex::new(None);
    pub fn waiting<R>(f: impl FnOnce(&mut Option<Waiting>) -> R) -> R {
        let mut waiting = WAITING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        super::mirrored(&mut waiting, f)
    }
    pub fn agreed<R>(f: impl FnOnce(&mut Option<Agreed>) -> R) -> R {
        f(&mut AGREED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()))
    }
}

/// Tests: each test thread has its own.
#[cfg(test)]
mod kept {
    use super::{Agreed, Waiting};
    use std::cell::RefCell;
    thread_local! {
        static WAITING: RefCell<Option<Waiting>> = const { RefCell::new(None) };
        static AGREED: RefCell<Option<Agreed>> = const { RefCell::new(None) };
    }
    pub fn waiting<R>(f: impl FnOnce(&mut Option<Waiting>) -> R) -> R {
        WAITING.with(|cell| super::mirrored(&mut cell.borrow_mut(), f))
    }
    pub fn agreed<R>(f: impl FnOnce(&mut Option<Agreed>) -> R) -> R {
        AGREED.with(|cell| f(&mut cell.borrow_mut()))
    }
}

/// The skill `asks` names, made and put on in `request`, waits for the
/// yes to its writes: any other waiting is dropped (one at a time). Its
/// id, for the model.
pub fn wait_for_yes(asks: Asks, request: u64, now: Instant) -> String {
    wait_for_yes_with(asks, request, now, false)
}

/// [`wait_for_yes`] for a say-back the lamp was given to say, with no call
/// of the model's that it answers (an app's make with code of its own,
/// #6307, `make_skill_local::app_code_asked`): its id is told in a note to
/// each later heard line while it waits ([`waiting_note`]), as the history
/// keeps no call's answer.
pub fn wait_for_yes_noted(asks: Asks, request: u64, now: Instant) -> String {
    wait_for_yes_with(asks, request, now, true)
}

/// The wait a voice make of `request` began, whose turn was cut while it
/// ran, made [`Waiting::noted`]: its say-back is said through the say
/// service (`second_provider::worker::made_running`), as an app's make's
/// is, so the person's yes answers no call of the model's either, and the
/// next line is forced to answer it ([`approve_force`]). On c0a4 (17 Nov
/// 19:43, build 13838, #7245) a 116 s OpenClaw make outlived its turn, its
/// say-back was said so, and the "yes" after it was told the approve_id but
/// called nothing, so the skill was never kept.
///
/// For a make with code of its own (or a task for the lamp's agent), the
/// line the say service is given in place of the cut turn's own
/// ([`said_back_line`]): its say-back word for word, as an app's make's is,
/// so the next line is forced once it was heard whole. On c0a4 (17 Nov
/// 22:39, the combo build, #7245) the cut line ("ask me for it again") went
/// as a note in the conversation's language, the model said it in 136
/// chars, the say-back was never heard whole, and the "yes" after it was
/// told to hear it again: nothing was forced or kept. Only in place of
/// the cut turn's code line `cut` (`made_skill::cut_code_line`): a code
/// make that also sends keeps `made_skill::cut_write_line`, which says it
/// sends nothing yet, never a say-back silent on what it sends. None for
/// no wait of `request`'s, or one whose line stays the cut turn's own.
pub fn said_by_the_say_service(request: u64, cut: &str) -> Option<String> {
    let code_line = cut.contains(crate::made_skill::CUT_ASK_AGAIN)
        || cut.ends_with(crate::made_skill::CUT_CODE_UNSAID);
    kept::waiting(|waiting| {
        let waiting = waiting
            .as_mut()
            .filter(|waiting| waiting.request == request)?;
        waiting.noted = true;
        eprintln!(
            "lamp-voice: request {request}: its made skill's say-back goes through the say service: the next line must answer its wait"
        );
        (code_line && waiting.asks.code && !waiting.asks.remove)
            .then(|| said_back_line(&waiting.asks))
    })
}

/// The line a cut make's code wait is said back in through the say
/// service ([`said_by_the_say_service`]): its name, then its say-back
/// whole, ending as the say-back ends, which `app_make::say_note` tells
/// the model to say word for word (`made_skill::RUNS_AFTER_YES`, or
/// `made_skill::RUNS_ONCE_AGREED` for a task for the lamp's agent).
fn said_back_line(asks: &Asks) -> String {
    let name = crate::made_skill::spoken_name(&asks.skill);
    let say_back = asks.say_back.trim();
    if say_back.ends_with(crate::made_skill::RUNS_ONCE_AGREED.trim()) {
        format!("I can have the lamp's agent do {name}. {say_back}")
    } else {
        format!("I made {name}. {say_back}")
    }
}

fn wait_for_yes_with(asks: Asks, request: u64, now: Instant, noted: bool) -> String {
    let id = format!(
        "yes-{:08x}",
        rand_u32(&asks.skill, request, now) // not a secret: only matched here
    );
    let count = asks.writes.len();
    let (code, remove) = (asks.code, asks.remove);
    kept::agreed(|agreed| *agreed = None);
    kept::waiting(|waiting| {
        *waiting = Some(Waiting {
            id: id.clone(),
            asks,
            request,
            at: now,
            echoes: Vec::new(),
            said: VecDeque::new(),
            noted,
            unclear: false,
            late_guarded: 0,
            name_only: Vec::new(),
            newest_line: 0,
            played: VecDeque::new(),
            answer_line: None,
            other_calls: Vec::new(),
            other_named: Vec::new(),
        });
    });
    if remove {
        eprintln!(
            "lamp-voice: request {request}: a made skill's remove waits for the person's yes (#6960)"
        );
    } else if code {
        eprintln!(
            "lamp-voice: request {request}: a made skill with code of its own waits for the person's yes (#6307)"
        );
    } else {
        eprintln!(
            "lamp-voice: request {request}: a made skill's {count} write(s) wait for the person's yes (#6066)"
        );
    }
    id
}

/// A few bits that differ between makes, for an id the model can't guess
/// from another.
fn rand_u32(skill: &str, request: u64, now: Instant) -> u32 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write(skill.as_bytes());
    hasher.write_u64(request);
    hasher.write_u128(now.elapsed().as_nanos());
    hasher.finish() as u32
}

/// The note a later heard line `request` is told at `now` of the wait:
/// the skill said back waits for the person's yes, and how to take it
/// ([`then_wait`]'s words). Every wait, as the second route's history keeps
/// no call's answer, so the id a make or a read-back told is gone by the
/// next line (#6223's row 4 on 9adf, 12 Nov: "a made skill's yes found
/// nothing waiting" three times). None in the wait's own request, none
/// waiting, or past [`YES_WITHIN`]; the yes keeps every guard of
/// [`approve`]. With nothing waiting, a skill's task or code just agreed
/// to is noted instead ([`agreed_note`], #6749 A).
pub fn waiting_note(request: u64, now: Instant) -> Option<String> {
    waiting_now(request, now).or_else(|| agreed_note(request, now))
}

/// The note a later heard line `request` is told at `now` of a skill's task
/// or a made skill's code just agreed to (#6749 A): the yes ran nothing,
/// and an ask for it now is a new run, so the model calls the skill. On
/// lamp-52e6 (13 Nov) the model, told the task "will now" run, said on the
/// next "desk check" that it was "already approved and is running" and
/// called nothing. Within [`YES_WITHIN`] of the yes, never in its own
/// request.
fn agreed_note(request: u64, now: Instant) -> Option<String> {
    kept::agreed(|agreed| {
        let agreed = agreed.as_ref().filter(|agreed| {
            request > agreed.request && now.saturating_duration_since(agreed.at) <= YES_WITHIN
        })?;
        Some(format!(
            "(For you to know, not to answer: the person just agreed to the skill {}. Their yes \
             ran nothing; it runs at its next trigger. If they ask for it now, call that skill, \
             as for any skill: each ask runs it again, and it isn't already running.)",
            crate::made_skill::spoken_name(&agreed.skill)
        ))
    })
}

fn waiting_now(request: u64, now: Instant) -> Option<String> {
    kept::waiting(|waiting| {
        let waiting = waiting.as_ref().filter(|waiting| {
            request > waiting.request && now.saturating_duration_since(waiting.at) <= YES_WITHIN
        })?;
        // Only the lamp's name ([`line_heard`]): no answer to it, so no
        // approve_id either, and the wait goes on for the next line.
        if name_only_line(waiting, request) {
            return Some(format!(
                "(For you to know, not to answer: the lamp still waits for their answer to what \
                 it just said back about the skill {}. This line was only your name, which \
                 answers nothing: answer the call as you would, and don't call make_skill now.)",
                crate::made_skill::spoken_name(&waiting.asks.skill)
            ));
        }
        // A remove (#6960): the lamp asked its one question, and the
        // person's answer is the call's, whatever it is.
        if waiting.asks.remove {
            return Some(format!(
                "(For you to know, not to answer: the lamp just asked them \"{}\" and waits for \
                 their answer, approve_id \"{}\". Call make_skill with only that approve_id, \
                 answer: \"yes\" only if they agree, \"no\" if they decline or wait, \"unclear\" \
                 if you can't tell, \"new_ask\" if their line answers nothing asked but asks \
                 something new (another skill named, another thing asked), and named \"{spoken}\" \
                 if their line names this skill, another skill's name if it names another, or \
                 \"none\". A line naming {spoken} answers this question, never new_ask. Nothing \
                 is removed but on their yes to this very question, for this very skill.)",
                waiting.asks.say_back,
                waiting.id,
                spoken = crate::made_skill::spoken_name(&waiting.asks.skill)
            ));
        }
        // Cut, and not heard whole since (#6715, the first builder's
        // choice 1): no yes is taken yet, so the note neither says it
        // waits for one nor gives the approve_id, which steered the model
        // to send it on a repeated ask and lose a refused round; it hands
        // the say-back to say again whole first. A no keeps it off and
        // doesn't bring the say-back back (Thirteen's P3 on #7038).
        if cut_unsaid(waiting, request) {
            return Some(format!(
                "(For you to know, not to answer: the say-back of the skill {} was cut, so it \
                 was not heard whole, and no yes to it is taken yet. If they agree to it or ask for \
                 it, say it again, word for word, before any yes, and call nothing for it now. If \
                 they decline or wait, say only that it stays off, never how to start it, and call \
                 nothing for it. {} End as it ends, asking nothing. {})",
                crate::made_skill::spoken_name(&waiting.asks.skill),
                say_whole(&waiting.asks.say_back),
                crate::make_skill_local::NEW_ASK_OWN_CALL
            ));
        }
        let before = if waiting.asks.code {
            "before it runs any code"
        } else if waiting.asks.grants.is_some() {
            "before its task runs"
        } else {
            "before it sends anything"
        };
        // A voice make's own code, not on until the yes (lamp-9adf, 17 Nov:
        // "No, don't turn it on." was answered with a question, "Maybe
        // later." with how to start it): a no or a wait drops it, judged by
        // the model and told by its answer, never a word list.
        let declined = if waiting.asks.code && !answer_forced(waiting) {
            " If they decline or put it off, call make_skill with that approve_id and answer \
             \"no\": it is dropped. If they change this skill, don't call it. Until their yes, \
             never say it is on or how to start it."
        } else {
            " If they decline, wait or change it, don't call it: it stays off."
        };
        Some(format!(
            "(For you to know, not to answer: the skill {} that the lamp just said back waits \
             for their yes {before}, approve_id \"{}\". If they agree, in any words, call \
             make_skill with only that approve_id.{declined}{answer}{new_ask})",
            crate::made_skill::spoken_name(&waiting.asks.skill),
            waiting.id,
            // A forced answer has its own new_ask; else a new ask is told to
            // be made by its own call (lamp-9adf, 18 Nov).
            new_ask = if answer_forced(waiting) {
                String::new()
            } else {
                format!(" {}", crate::make_skill_local::NEW_ASK_OWN_CALL)
            },
            answer = if answer_forced(waiting) {
                format!(
                    " For this one, always give answer with that approve_id: \"yes\" only if \
                     they agree, \"no\" if they decline or wait, \"unclear\" if you can't tell, \
                     \"new_ask\" if their line answers nothing asked but asks something new; \
                     left out, it is read as unclear. Give named too: \"{spoken}\" if their line \
                     names this skill, another skill's name if it names another, or \"none\"; a \
                     line naming {spoken} answers it, never new_ask, and a yes counts only for \
                     this skill.",
                    spoken = crate::made_skill::spoken_name(&waiting.asks.skill)
                )
            } else {
                String::new()
            }
        ))
    })
}

/// Nothing waits on this test's thread.
#[cfg(test)]
pub fn clear_for_test() {
    kept::waiting(|waiting| *waiting = None);
    kept::agreed(|agreed| *agreed = None);
}

/// Whether the wait `id` is the one waiting: a voice make's, a read-back's,
/// a task's or an app make's code alike.
pub fn waits(id: &str) -> bool {
    kept::waiting(|waiting| waiting.as_ref().is_some_and(|waiting| waiting.id == id))
}

/// Whether the wait `id` is the one waiting and a made skill's remove
/// (#6960).
pub fn removes(id: &str) -> bool {
    kept::waiting(|waiting| {
        waiting
            .as_ref()
            .is_some_and(|waiting| waiting.id == id && waiting.asks.remove)
    })
}

/// The wait `id` dropped: its yes was not the owner's (Heron's P1 on
/// #6558, as #6307's for the app's code make). Whether one was.
pub fn drop_wait(id: &str) -> bool {
    kept::waiting(|waiting| {
        let waits = waiting.as_ref().is_some_and(|waiting| waiting.id == id);
        if waits {
            *waiting = None;
        }
        waits
    })
}

/// The remove waiting for the made skill `skill`'s yes (#6960) dropped,
/// any other wait kept: the person turned that skill off or on by voice
/// instead (#7445, Twelve's P2 on #7453: "Remove coffee time?", "no, just
/// turn it off" left the remove waiting, and a later yes could take it).
/// Whether one was.
pub fn drop_remove_of(skill: &str) -> bool {
    kept::waiting(|waiting| {
        let removes = waiting
            .as_ref()
            .is_some_and(|waiting| waiting.asks.remove && waiting.asks.skill == skill);
        if removes {
            *waiting = None;
        }
        removes
    })
}

/// Where the second route's worker keeps the made skill with code whose
/// spoken yes it awaits ([`code_yes_awaited_told`]): the app's make status
/// is answered by the outer voice process (`app_make::status`, its socket
/// for lamp-cloud), and the wait lives in the worker's own memory, so on a
/// lamp the status never saw it (sim-2's HOLD on #7130). A file in the
/// voice's own runtime folder (`RuntimeDirectory=lampos-voice`, 0700),
/// 0600, as [`crate::skill_task_yes::ASKED_FILE`] crosses the same two.
pub const CODE_AWAITED_FILE: &str = "/run/lampos-voice/code-yes-awaited.json";

#[cfg(not(test))]
fn code_awaited_file() -> Option<std::path::PathBuf> {
    Some(CODE_AWAITED_FILE.into())
}

#[cfg(test)]
thread_local! {
    static CODE_AWAITED_HERE: std::cell::RefCell<Option<std::path::PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Tests: where this thread's wait is mirrored; none, nowhere.
#[cfg(test)]
pub fn set_code_awaited_file_here(path: Option<std::path::PathBuf>) {
    CODE_AWAITED_HERE.with(|here| *here.borrow_mut() = path);
}

#[cfg(test)]
fn code_awaited_file() -> Option<std::path::PathBuf> {
    CODE_AWAITED_HERE.with(|here| here.borrow().clone())
}

/// The code wait `waiting` holds, as it is mirrored: its SHA-256 and when
/// its wait began; none for a wait with no code of its own, or a remove's.
fn code_wait(waiting: &Option<Waiting>) -> Option<(String, Instant)> {
    waiting
        .as_ref()
        .filter(|waiting| waiting.asks.code && !waiting.asks.remove)
        .map(|waiting| (waiting.asks.sha256.clone(), waiting.at))
}

/// `f` run on the wait, and a change of its code wait ([`code_wait`])
/// written to [`CODE_AWAITED_FILE`]: the SHA-256 and the wall-clock time
/// its [`YES_WITHIN`] ends, or the file removed once none waits. Written
/// whole (`thread::write_file`); one not written leaves the app told
/// today's `not_kept`, never a yes awaited that isn't.
fn mirrored<R>(waiting: &mut Option<Waiting>, f: impl FnOnce(&mut Option<Waiting>) -> R) -> R {
    let before = code_wait(waiting);
    let result = f(waiting);
    let after = code_wait(waiting);
    if before != after
        && let Some(path) = code_awaited_file()
    {
        match after {
            Some((sha256, at)) => {
                let left = YES_WITHIN.saturating_sub(at.elapsed());
                let until = crate::app_make::now_unix_ms() + left.as_millis() as u64;
                let told = json!({"sha256": sha256, "until_unix_ms": until});
                if let Err(error) = crate::thread::write_file(&path, &told.to_string()) {
                    eprintln!(
                        "lamp-voice: a made skill's awaited yes isn't kept for the app: {error}"
                    );
                }
            }
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    result
}

/// Whether the yes to the made skill with code whose SHA-256 is `sha256`
/// is awaited at `now_unix_ms`, as the second route's worker keeps it
/// ([`CODE_AWAITED_FILE`]), read from any of the voice's processes: the
/// app's make status (`app_make::status`, `awaits_yes`). One past its
/// [`YES_WITHIN`], unreadable or of another skill, none.
pub fn code_yes_awaited_told(sha256: &str, now_unix_ms: u64) -> bool {
    let Some(path) = code_awaited_file() else {
        return false;
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .is_some_and(|told| {
            told["sha256"] == sha256
                && told["until_unix_ms"]
                    .as_u64()
                    .is_some_and(|until| now_unix_ms <= until)
        })
}

/// Whether the person's yes to the made skill with code of its own whose
/// SHA-256 is `sha256` is awaited at `now` (#6307): its say-back was given
/// to the lamp to say, and its wait is neither dropped, answered nor past
/// [`YES_WITHIN`]. The app is told so (`app_make::status`, `awaits_yes`).
pub fn code_yes_awaited(sha256: &str, now: Instant) -> bool {
    kept::waiting(|waiting| {
        waiting.as_ref().is_some_and(|waiting| {
            waiting.asks.code
                && !waiting.asks.remove
                && waiting.asks.sha256 == sha256
                && now.saturating_duration_since(waiting.at) <= YES_WITHIN
        })
    })
}

/// The made skill with code of its own that is not on and whose yes was
/// asked before `request` (#7270, `crate::made_not_on`): its name, and
/// whether its yes is past [`YES_WITHIN`] at `now`, so no yes takes it any
/// more. Only within `within` of its wait's start; never a remove's, a
/// skill's task's or a write's wait (a skill that writes is on already),
/// and none once the wait is answered, dropped or taken by its yes.
pub fn code_not_on(request: u64, now: Instant, within: Duration) -> Option<(String, bool)> {
    kept::waiting(|waiting| {
        let waiting = waiting.as_ref().filter(|waiting| {
            waiting.asks.code
                && !waiting.asks.remove
                && request > waiting.request
                && now.saturating_duration_since(waiting.at) <= within
        })?;
        Some((
            waiting.asks.skill.clone(),
            now.saturating_duration_since(waiting.at) > YES_WITHIN,
        ))
    })
}

/// The made skill with code of its own that [`code_not_on`] names, past
/// its [`YES_WITHIN`], for the heard turn `request` at `now`: its name, for
/// the first `most` such turns only, each counted once (#7270,
/// `crate::made_not_on::Guard`). None within its wait, or past `most`.
pub fn late_guarded(request: u64, now: Instant, within: Duration, most: u8) -> Option<String> {
    kept::waiting(|waiting| {
        let waiting = waiting.as_mut().filter(|waiting| {
            waiting.asks.code
                && !waiting.asks.remove
                && request > waiting.request
                && now.saturating_duration_since(waiting.at) > YES_WITHIN
                && now.saturating_duration_since(waiting.at) <= within
                && waiting.late_guarded < most
        })?;
        waiting.late_guarded += 1;
        Some(waiting.asks.skill.clone())
    })
}

/// Whether a made skill's writes wait for the yes at `now`: the echo hold
/// is on ([`crate::second_provider`]).
pub fn awaited(now: Instant) -> bool {
    kept::waiting(|waiting| {
        waiting
            .as_ref()
            .is_some_and(|waiting| now.saturating_duration_since(waiting.at) <= YES_WITHIN)
    })
}

/// Whether a line in `request` at `now` would answer a skill's say-back
/// (#6749 C): a yes is awaited, within [`YES_WITHIN`], `request` is later
/// than the make's or the read-back's own, and the lamp's newest answer kept
/// toward it before `request` held the whole say-back uncut, so nothing the
/// lamp said since stands between them. The second route's worker then
/// counts its window from the end of that answer as played, not from its
/// start, and takes the line as an answer to the lamp's own ask: a long
/// read-back, with a repair, said in a busy room used up the thread's 45 s
/// from the answer's start, and the "Yes." 8 s after its end was held (9adf,
/// 13 Nov).
pub fn answer_awaited(request: u64, now: Instant) -> bool {
    kept::waiting(|waiting| {
        waiting
            .as_ref()
            .is_some_and(|waiting| say_back_before(waiting, request, now).is_some())
    })
}

/// The request whose answer held the whole say-back a line in `request` at
/// `now` would answer ([`answer_awaited`]); none, no yes is awaited.
fn say_back_before(waiting: &Waiting, request: u64, now: Instant) -> Option<u64> {
    if request <= waiting.request || now.saturating_duration_since(waiting.at) > YES_WITHIN {
        return None;
    }
    waiting
        .said
        .iter()
        .filter(|kept| kept.0 < request && stands_between(waiting, kept))
        .max_by_key(|(id, ..)| *id)
        .filter(|(_, said, cut)| {
            !cut && crate::services::read_back_said(&waiting.asks.say_back, said)
        })
        .map(|(id, ..)| *id)
}

/// The lamp's words in `request` play until `until_us` (monotonic µs,
/// `second_provider::Did::Said`): kept while a yes waits, for the make's
/// request and any after it, the latest end of each.
pub fn played_until(request: u64, until_us: u64) {
    kept::waiting(|waiting| {
        let Some(waiting) = waiting
            .as_mut()
            .filter(|waiting| request >= waiting.request)
        else {
            return;
        };
        match waiting.played.iter_mut().find(|(id, _)| *id == request) {
            Some((_, until)) => *until = (*until).max(until_us),
            None => {
                if waiting.played.len() == SAID_KEPT {
                    waiting.played.pop_front();
                }
                waiting.played.push_back((request, until_us));
            }
        }
    });
}

/// The lamp's question a line in `request` at `now` answers, while a
/// skill's yes is awaited ([`answer_awaited`]): the request whose answer
/// held the whole say-back, and when its words ended as played (monotonic
/// µs, [`played_until`]). The carry of a yes counts its 45 s from there
/// (`LAMP_SKILL_YES_CARRY`, #7148), as #6749 C's thread window does. None
/// when no yes is awaited or that answer's play was never told.
pub fn asked_until(request: u64, now: Instant) -> Option<(u64, u64)> {
    kept::waiting(|waiting| {
        let waiting = waiting.as_ref()?;
        let asked = say_back_before(waiting, request, now)?;
        waiting
            .played
            .iter()
            .find(|(id, _)| *id == asked)
            .map(|&(_, until)| (asked, until))
    })
}

/// The `make_skill` argument beside [`APPROVE_ID`] that says what the
/// person answered the say-back (#6835 gap 2): [`ANSWERS`]. Code decides
/// when the call is made ([`approve_force`]); the model decides what it
/// says.
pub const ANSWER: &str = "answer";
/// The [`ANSWER`] for a line that answers nothing asked but asks
/// something new: another skill named, another thing asked (lamp-9adf, 17
/// Nov 18:12: "Remove mail count?", then "Lamp, remove the joke time
/// skill", forced to a yes, no or unclear, was taken as the yes, and mail
/// count was removed). The wait ends, nothing kept, and the new ask is
/// answered ([`answered`]). Whether a line is one is the model's judgment,
/// never a list of words.
pub const NEW_ASK: &str = "new_ask";
/// What [`ANSWER`] takes. Left out, it reads as `yes`, as before.
pub const ANSWERS: [&str; 4] = ["yes", "no", "unclear", NEW_ASK];
/// The `make_skill` argument beside [`ANSWER`] that binds a yes to its
/// skill (#7545, Twelve's and El's HOLDs on #7561): the skill the person's
/// line names, its name alone as they said it, or [`NAMED_NONE`]. The
/// model fills it; code reads a yes whose name is not the waiting skill's
/// ([`crate::made_remove::same_name`], the made-skill name match) as a
/// [`NEW_ASK`], so a yes never acts on another skill's wait, whatever
/// label the model gave the line. A forced wait's yes without it reads as
/// unclear ([`answered`]).
pub const NAMED: &str = "named";
/// [`NAMED`] when the person's line names no skill ("Yes.").
pub const NAMED_NONE: &str = "none";

/// The force the second route's chat is given with a skill's tasks on and
/// `make_skill` declared (`second_provider::net::Chat::forcing`, #6835
/// gap 2, the first builder's option (b) on #6811): the round 1 of the
/// person's line right after a skill's task said back is made to call
/// `make_skill`, the call that takes its `approve_id` and the person's
/// [`ANSWER`]. Only while a task's wait, a made skill's remove (#6960), or
/// an app's make with code (never a voice make's writes or code,
/// [`answer_forced`]) holds, within [`YES_WITHIN`], and the lamp's newest
/// answer kept toward it ([`stands_between`]) held the whole say-back uncut: so one round only (the round's own answer
/// is newer, and is no say-back), never after a cut or partial read-back,
/// never with nothing waiting, and never in the say-back's own written turn
/// (no answer kept yet). On lamp-52e6 (13 Nov, row-photo2) the model,
/// after a `use_skill` it started itself and the read-back said unasked,
/// answered "Yes." with "Want me to go ahead with that?" and called
/// nothing, so no yes was kept.
pub fn approve_force() -> crate::second_provider::net::Force {
    std::sync::Arc::new(|messages: &[crate::second_provider::net::Message]| {
        messages.last().filter(|message| message.role == "user")?;
        if !approve_forced_now(Instant::now()) {
            return None;
        }
        eprintln!(
            "lamp-voice second: the line after a skill's task said back whole: its first round must call {}",
            crate::made_skill::MAKE
        );
        Some(crate::second_provider::net::Forced {
            name: crate::made_skill::MAKE,
            only: None,
            fields: Some(&APPROVE_FIELDS),
        })
    })
}

/// The only `make_skill` fields the forced approval round is told, all
/// required ([`approve_force`], #6921): the wait's id, the person's
/// answer and the skill their line names ([`NAMED`], #7545), so the
/// forced call, a task's yes or a remove's, can carry no make or edit
/// beside them, and its yes is bound to the skill asked about.
pub const APPROVE_FIELDS: [&str; 3] = [APPROVE_ID, ANSWER, NAMED];

/// Whether `waiting` is a skill's task's wait, a made skill's remove
/// (#6960), or an app's make with code of its own (`noted`, #6307): the
/// ones [`approve_force`] forces, whose call always says what the person
/// answered (never a voice make's writes or code, whose yes answers the
/// model's own call). An app's make has no call of the model's that the yes
/// answers, only the note: on lamp-9adf (15 Nov 17:24) the owner's "Hey
/// Lamp", then "Yes.", after an app's make said back, was asked as "Lamp.
/// Yes.", told the approve_id, and answered with a greeting, calling
/// nothing, so the yes was never taken.
fn answer_forced(waiting: &Waiting) -> bool {
    waiting.asks.remove
        || (waiting.asks.grants.is_some() && !waiting.asks.code)
        || (waiting.noted && waiting.asks.code)
}

/// Whether the lamp's answer `kept` toward `waiting` stands between its
/// say-back and a later line, for [`answer_awaited`] and
/// [`approve_forced_now`]: all do but a later request's answer cut before
/// any of its words were kept, which said nothing. On lamp-9adf (15 Nov
/// 17:24) the owner's "Hey Lamp" after an app's make said back was answered
/// as a call, cut before a word played by their "Yes.", and that empty cut
/// answer was the newest, so the yes was never forced. The wait's own
/// request cut with nothing kept still stands: a say-back cut off before its
/// first word (#7072).
///
/// Nor does the lamp's answer to a later line that was only its name
/// ([`line_heard`]): it answered a call, not the say-back, so the line
/// after it is still the say-back's answer, forced and awaited, as a
/// call's answer is no other request of a waiting write's
/// (`services::answered_in_place`). Else "Yes? I'm here." after "Hey
/// Lamp" stood between, and the person's "Yes." after it was neither
/// forced nor awaited.
fn stands_between(waiting: &Waiting, kept: &(u64, String, bool)) -> bool {
    let (id, said, cut) = kept;
    !(*id > waiting.request && ((*cut && said.is_empty()) || name_only_line(waiting, *id)))
}

/// Whether the line of `request` was only the lamp's name, or no words at
/// all, as the model was asked it ([`line_heard`]).
fn name_only_line(waiting: &Waiting, request: u64) -> bool {
    waiting.name_only.contains(&request)
}

/// The second route asks the model the line of `request`; `name_only`:
/// as asked, it is only the lamp's name, or no words at all
/// (`second_provider`, the same line told it was a call). While a yes is
/// awaited, such a line answers nothing (lamp-9adf, 17 Nov 13:58: an app's
/// make with code said back, then a lone "Hello?" heard as the name was
/// asked as a call, its round forced to answer, and the model's yes put
/// the Gmail-reading code on): no answer to the wait is taken in it, yes,
/// no or unclear ([`approve`], [`answered`]), its round is not forced
/// ([`approve_forced_now`]), the note gives no approve_id
/// ([`waiting_note`]), and the wait goes on for the next line. State, not
/// words: the route already judged the line a call.
///
/// `line`: the line as the model was asked it, kept in memory only and
/// never journalled. A remove's (#6960) first line that is more than the
/// name is its answer line ([`Waiting::answer_line`]), read by
/// [`plain_answer`] when the model answers yes; a later one drops the
/// remove, nothing removed: only the very next line can confirm it
/// (lamp-9adf, 18 Nov, stock 14083: a make two lines after "Remove daily
/// stretch reminder?" was taken as its yes).
///
/// For a skill's task's wait, a line that names another skill
/// ([`other_skill_named`], #7634) is kept by its request, so a yes in it
/// is never this wait's ([`answered`]).
pub fn line_heard(request: u64, name_only: bool, line: &str) {
    // The skills on the lamp are read only for a line said with "skill"
    // while a skill's task waits, before the wait's lock is taken.
    let task = kept::waiting(|waiting| {
        waiting
            .as_ref()
            .is_some_and(|waiting| request > waiting.request && task_wait(waiting))
    });
    let installed = if task && line.to_lowercase().contains("skill") {
        installed_names()
    } else {
        Vec::new()
    };
    kept::waiting(|kept| {
        let Some(waiting) = kept.as_mut().filter(|waiting| request > waiting.request) else {
            return;
        };
        waiting.newest_line = waiting.newest_line.max(request);
        if name_only {
            if !waiting.name_only.contains(&request) {
                if waiting.name_only.len() == SAID_KEPT {
                    waiting.name_only.remove(0);
                }
                waiting.name_only.push(request);
            }
            return;
        }
        if task_wait(waiting)
            && !waiting.other_named.iter().any(|(kept, _)| *kept == request)
            && let Some(other) = other_skill_named(line, &waiting.asks.skill, &installed)
        {
            if waiting.other_named.len() == SAID_KEPT {
                waiting.other_named.remove(0);
            }
            waiting.other_named.push((request, other));
        }
        if !waiting.asks.remove {
            return;
        }
        match &waiting.answer_line {
            None => waiting.answer_line = Some((request, line.to_owned())),
            Some((answer, _)) if request > *answer => {
                eprintln!(
                    "lamp-voice: request {request}: a remove's question was not confirmed on the line right after it: dropped, nothing removed (#6960)"
                );
                *kept = None;
            }
            Some(_) => {}
        }
    });
}

// A fixed set of words, by ruling: a remove (#6960) or a clash's replace
// (#6593) takes skills off the lamp, so the code itself must see a plain
// yes, as a "type DELETE to confirm" box does. The first builder ruled
// this list on #7683 (18 Nov, 12:51), after Twelve's HOLD: the owner's
// rule against yes/no word lists (13 Nov) keeps holding everywhere else,
// and here the model's own `yes` is still needed first; this is only the
// last gate, and it can only ever leave a skill on the lamp.

/// The answers that confirm a remove, each as its words: the only ones the
/// code takes ([`plain_answer`]). English and Vietnamese.
pub const YES_SAID: &[&[&str]] = &[
    &["yes"],
    &["yeah"],
    &["yep"],
    &["yup"],
    &["sure"],
    &["ok"],
    &["okay"],
    &["go", "ahead"],
    &["do", "it"],
    &["remove", "it"],
    &["delete", "it"],
    &["vâng"],
    &["ừ"],
    &["xoá", "đi"],
    &["xóa", "đi"],
    &["xoá", "nó", "đi"],
    &["xóa", "nó", "đi"],
];

/// Words that may stand beside a plain answer and add nothing: please,
/// the call's "hey" and Vietnamese "ơi", and the words around a skill's
/// name ("the moon fact skill", "my one").
const SAID_BESIDE: &[&str] = &["please", "hey", "ơi", "the", "my", "skill"];

/// Words that make a line a question, whatever else it holds.
const ASKING: &[&str] = &[
    "what", "which", "who", "whom", "whose", "why", "how", "where", "when", "gì", "nào", "sao",
    "không",
];

/// The words of `line`, lower case, cut at anything not a letter or digit
/// ("what's" is "what", "s"), the lamp's own name left out.
fn plain_words(line: &str) -> Vec<String> {
    let name = crate::second_provider::net::key_terms::NAME.to_lowercase();
    line.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .filter(|word| *word != name)
        .collect()
}

/// Whether `line`, as the code reads it, is only a plain answer: no "?",
/// no word that asks ([`ASKING`]), and every word, past the lamp's name
/// and the words said beside ([`SAID_BESIDE`]), one of `answers` (one at
/// least), `also` (words that may come with one, never alone), or a
/// skill's own name of `names`, as its folder's name is written
/// ("moon-fact": "moon fact"). "Hey Lamp, yes, remove it" and "Yes
/// please" are; "Sure, and make me a skill that …", "Is it the moon
/// one?" and "Hey Lamp, what?" are not.
pub fn plain_answer(line: &str, answers: &[&[&str]], also: &[&[&str]], names: &[String]) -> bool {
    if line.contains(['?', '？']) {
        return false;
    }
    let words: Vec<String> = plain_words(line)
        .into_iter()
        .filter(|word| !SAID_BESIDE.contains(&word.as_str()))
        .collect();
    if words.iter().any(|word| ASKING.contains(&word.as_str())) {
        return false;
    }
    let names: Vec<Vec<String>> = names
        .iter()
        .map(|name| {
            name.split(['-', '_', ' '])
                .filter(|word| !word.is_empty())
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
        })
        .filter(|name| !name.is_empty())
        .collect();
    let starts = |at: usize, phrase: &[&str]| {
        words.len() >= at + phrase.len()
            && phrase
                .iter()
                .zip(&words[at..])
                .all(|(said, word)| *said == word.as_str())
    };
    let mut answered = false;
    let mut at = 0;
    while at < words.len() {
        // The longest phrase that begins here.
        let answer = answers
            .iter()
            .filter(|phrase| starts(at, phrase))
            .map(|phrase| phrase.len())
            .max();
        let with = also
            .iter()
            .filter(|phrase| starts(at, phrase))
            .map(|phrase| phrase.len())
            .max();
        let name = names
            .iter()
            .filter(|name| {
                words.len() >= at + name.len()
                    && name.iter().zip(&words[at..]).all(|(one, word)| one == word)
            })
            .map(Vec::len)
            .max();
        let longest = [answer, with, name].into_iter().flatten().max();
        let Some(longest) = longest else {
            return false;
        };
        answered |= answer == Some(longest);
        at += longest;
    }
    answered
}

/// What may come with a remove's yes: its verb, so "Yes, remove moon
/// fact" (#7561's row) is still its yes, the skill's name read beside it.
const REMOVE_ALSO: &[&[&str]] = &[&["remove"], &["delete"], &["xoá"], &["xóa"]];

/// A round of `request` calls `calls` (each its function's name and
/// arguments), before any runs. While a made skill's remove waits (#6960),
/// a round that calls anything but a wait's answer (`make_skill` with an
/// [`APPROVE_ID`] alone of its asks) marks the request: no remove is
/// confirmed in it ([`not_a_plain_yes`]), whatever the model answered.
/// The line carried another ask, by the call it made. A clash waiting is
/// told the same (`made_clash::round_called`).
pub fn round_called<'a>(
    request: u64,
    calls: impl IntoIterator<Item = (&'a str, &'a Value)> + Clone,
) {
    crate::made_clash::round_called(request, calls.clone());
    let other = calls
        .into_iter()
        .any(|(name, args)| name != crate::made_skill::MAKE || !args[APPROVE_ID].is_string());
    if !other {
        return;
    }
    kept::waiting(|waiting| {
        let Some(waiting) = waiting
            .as_mut()
            .filter(|waiting| waiting.asks.remove && request > waiting.request)
        else {
            return;
        };
        if !waiting.other_calls.contains(&request) {
            if waiting.other_calls.len() == SAID_KEPT {
                waiting.other_calls.remove(0);
            }
            waiting.other_calls.push(request);
        }
    });
}

/// Why the yes in `request` to the remove `waiting` (#6960) is no plain
/// yes to its question, in code: not on its answer line (the first line
/// after the question, [`line_heard`]), a line that is not only a plain
/// yes ([`plain_answer`] of [`YES_SAID`]: a question, or anything else
/// asked), or a request whose rounds called anything else
/// ([`round_called`]). None when it is one. The model's own reading of
/// the line (its `answer` and `named`) is taken before this, and this only
/// ever turns a yes into nothing removed.
fn not_a_plain_yes(waiting: &Waiting, request: u64) -> Option<&'static str> {
    match &waiting.answer_line {
        Some((answer, line)) if *answer == request => {
            if !plain_answer(
                line,
                YES_SAID,
                REMOVE_ALSO,
                std::slice::from_ref(&waiting.asks.skill),
            ) {
                Some("its line is not only a plain yes")
            } else if waiting.other_calls.contains(&request) {
                Some("its turn called something else")
            } else {
                None
            }
        }
        _ => Some("not the line right after the question"),
    }
}

thread_local! {
    /// The skills on the lamp as a test of this thread sets them
    /// ([`set_installed_here`]); read only in a test build.
    static INSTALLED_HERE: std::cell::RefCell<Option<Vec<String>>> =
        const { std::cell::RefCell::new(None) };
}

/// The skills on the lamp, by name ([`crate::made_remove::names_on`]: the
/// person's made ones, and the skills and pushed folders'), for
/// [`other_skill_named`]. A test build reads only [`set_installed_here`]'s,
/// none when unset.
fn installed_names() -> Vec<String> {
    if cfg!(test) {
        return INSTALLED_HERE
            .with(|here| here.borrow().clone())
            .unwrap_or_default();
    }
    crate::made_skill::Keeping::when(true)
        .map(|keeping| crate::made_remove::names_on(&keeping))
        .unwrap_or_default()
}

/// The skills on the lamp for this test's thread alone.
#[cfg(test)]
pub fn set_installed_here(names: Option<Vec<String>>) {
    INSTALLED_HERE.with(|here| *here.borrow_mut() = names);
}

/// Whether `waiting` is a skill's task's (#6305): grants said back, not a
/// made skill's code or a remove.
fn task_wait(waiting: &Waiting) -> bool {
    waiting.asks.grants.is_some() && !waiting.asks.code && !waiting.asks.remove
}

/// Words that bound a skill's name said with "skill" ([`other_skill_named`]):
/// what comes before a name ("my look title skill", "use page title
/// skill"). A name may hold any of them inside it (`start-the-day`); only
/// its ends are bounded by one.
///
/// A yes said without its comma bounds a name too ("okay page title
/// skill", "go ahead with weather skill", "ừ đèn ngủ skill"; the
/// adversarial read's P2 on #7634), and "please" before one ("please page
/// title skill") as after it.
const BEFORE_A_NAME: &[&str] = &[
    "my", "the", "your", "our", "this", "that", "a", "an", "use", "run", "open", "try", "launch",
    "start", "play", "what", "which", "yes", "yeah", "yep", "ok", "okay", "sure", "with", "go",
    "please", "and", "then", "lamp", "có", "ừ", "vâng", "được",
];

/// Words that bound a name said after "skill", besides [`BEFORE_A_NAME`]'s
/// ("use skill page title please").
const AFTER_A_NAME: &[&str] = &["please", "now", "too", "for", "to", "is", "instead"];

/// Words before "skill" that name no skill (#7634): what is said around a
/// name, English and Vietnamese ("run my new weather skill", "Ừ, chạy cái
/// skill đó đi", "Vâng, em chạy skill đi", "great skill"). Left out, what
/// remains is read against the waiting skill's own name words
/// ([`other_skill_named`]).
const FILLERS: &[&str] = &[
    "great", "nice", "the", "that", "this", "same", "new", "old", "daily", "my", "run", "use",
    "start", "open", "chạy", "dùng", "mở", "bật", "cái", "đó", "này", "kia", "em", "anh", "chị",
    "cho", "làm",
];

/// The most words a skill's name is said in, before or after "skill".
const NAME_WORDS: usize = 6;

/// Whether `word` is "skill" or "skills".
fn skill_word(word: &str) -> bool {
    word == "skill" || word == "skills"
}

/// Whether `word` bounds a name's end ([`BEFORE_A_NAME`], [`AFTER_A_NAME`],
/// or "skill" itself).
fn bounds(word: &str) -> bool {
    BEFORE_A_NAME.contains(&word) || AFTER_A_NAME.contains(&word) || skill_word(word)
}

/// The skill other than `skill` that a heard line names (#7634, lamp-9adf
/// 18 Nov 07:20:43: "Lamp, use my look title skill." was taken as the yes
/// to `page-title`'s task), with `installed` the skills on the lamp by
/// name. Within a clause, at each "skill" or "skills":
///
/// - **Bounded runs.** The runs of at most [`NAME_WORDS`] words right
///   before "skill" or right after it whose both ends sit at the clause's
///   edge, a bounding word ([`bounds`]) or "skill": "page title backup" in
///   "use skill page title backup", never "page title" inside it. Each is
///   matched by the made-skill name match #7545 binds a yes with
///   ([`crate::made_remove::same_name`]).
/// - **The longest match wins.** A run naming an installed skill other
///   than `skill`, longer than any naming `skill`, names that other skill
///   (`page-title-backup`, not `page-title`). Else a run naming `skill`
///   names its own: "yes, use my lamp test email two skill",
///   "Có, dùng skill page title".
/// - **Else, before "skill":** the words back to a word that comes before
///   a name ([`BEFORE_A_NAME`]) or the clause's start, at most
///   [`NAME_WORDS`], fillers ([`FILLERS`]) left out. None left ("Ừ, chạy
///   cái skill đó đi", "great skill"), or only `skill`'s own name words
///   ("Yes, run my new weather skill", "the weather skill" for weather
///   report): its own. Any other word left names another skill, listed
///   or not ("my look title skill", "my backup page title skill"): safety
///   first, as a false yes runs a skill's grants without consent.
/// - **Else, after "skill":** with the lamp's skills read, words that name
///   no installed skill are nothing ("yes, run the skill again"); with
///   none read (`installed` empty), the words right after it, up to a
///   bounding word, fail closed as another's.
///
/// A line with no "skill" names none, nor does "kỹ năng". Spelling only:
/// no word is judged by what it means.
pub fn other_skill_named(heard: &str, skill: &str, installed: &[String]) -> Option<String> {
    let own = |run: &[String]| crate::made_remove::same_name(&run.join(" "), skill);
    // A word as a made skill's name is written: marks off, a number word
    // as digits ("two" is "2").
    let said_word =
        |word: &str| crate::made_skill::name_digits(&crate::made_remove::unmarked(word));
    let own_words: Vec<String> = crate::made_skill::name_digits(skill)
        .split('-')
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect();
    let another = |run: &[String]| {
        let said = run.join(" ");
        installed
            .iter()
            .any(|name| name != skill && crate::made_remove::same_name(&said, name))
    };
    for clause in heard.split([',', ';', '.', '?', '!']) {
        let words: Vec<String> = clause
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect();
        for at in (0..words.len()).filter(|&at| skill_word(&words[at])) {
            let before = (at.saturating_sub(NAME_WORDS)..at)
                .filter(|&from| from == 0 || bounds(&words[from - 1]))
                .map(|from| &words[from..at]);
            let after = (at + 2..=words.len().min(at + 1 + NAME_WORDS))
                .filter(|&to| to == words.len() || bounds(&words[to]))
                .map(|to| &words[at + 1..to]);
            let runs: Vec<&[String]> = before.chain(after).collect();
            let own_most = runs
                .iter()
                .filter(|run| own(run))
                .map(|run| run.len())
                .max()
                .unwrap_or(0);
            let other = runs
                .iter()
                .filter(|run| !own(run) && another(run))
                .max_by_key(|run| run.len());
            if let Some(other) = other
                && other.len() > own_most
            {
                // Said as the name is, without a word said before it that
                // the spelling distance lets in ("my page title backup").
                let mut other: &[String] = other;
                while other.len() > 1
                    && BEFORE_A_NAME.contains(&other[0].as_str())
                    && another(&other[1..])
                {
                    other = &other[1..];
                }
                return Some(other.join(" "));
            }
            // The words right before "skill", back to a word that comes
            // before a name or the clause's start, fillers left out. None
            // left ("Ừ, chạy cái skill đó đi"), or only the waiting skill's
            // own name words ("my new weather skill", "the weather skill"
            // for weather report): its own. Any other word left names
            // another skill, listed or not ("my look title skill", "my
            // backup page title skill"): a false refusal only lets the wait
            // go, a false yes runs a skill's grants without consent.
            let start = (0..at)
                .rev()
                .take(NAME_WORDS)
                .take_while(|&before| !BEFORE_A_NAME.contains(&words[before].as_str()))
                .last();
            if let Some(start) = start {
                let name = &words[start..at];
                let left: Vec<&String> = name
                    .iter()
                    .filter(|word| !FILLERS.contains(&word.as_str()))
                    .collect();
                if !left.is_empty() {
                    if left
                        .iter()
                        .all(|word| own_words.contains(&said_word(word.as_str())))
                    {
                        continue;
                    }
                    return Some(name.join(" "));
                }
            }
            // After "skill", its own name ("use my skill page title") or,
            // with the lamp's skills read, words naming no installed skill
            // take nothing from the yes; with none read, the words right
            // after it fail closed, as a name after "skill" can't be told
            // from "again".
            if own_most > 0 || !installed.is_empty() {
                continue;
            }
            let after: Vec<&str> = words[at + 1..]
                .iter()
                .map(String::as_str)
                .take(NAME_WORDS)
                .take_while(|word| !bounds(word))
                .collect();
            if !after.is_empty() {
                return Some(after.join(" "));
            }
        }
    }
    None
}

/// Whether [`approve_force`] forces a request at `now`.
pub fn approve_forced_now(now: Instant) -> bool {
    kept::waiting(|waiting| {
        waiting.as_ref().is_some_and(|waiting| {
            answer_forced(waiting)
                && !name_only_line(waiting, waiting.newest_line)
                && now.saturating_duration_since(waiting.at) <= YES_WITHIN
                && waiting
                    .said
                    .iter()
                    .filter(|kept| stands_between(waiting, kept))
                    .max_by_key(|(id, ..)| *id)
                    .is_some_and(|(id, said, cut)| {
                        *id >= waiting.request
                            && !cut
                            && crate::services::read_back_said(&waiting.asks.say_back, said)
                    })
        })
    })
}

/// What the model is told of an answer to a wait in a line that was only
/// the lamp's name ([`line_heard`]): nothing taken, and it still waits.
pub const NAME_ONLY_NOT_TAKEN: &str = "Not taken: that line was only your name, no answer to \
     what you said back. Nothing changed, and it still waits for their answer. Answer the call \
     only, in a few words, and call nothing.";

/// [`NAME_ONLY_NOT_TAKEN`] told, and journaled, for `skill`'s wait in
/// `request`.
fn name_only_not_taken(request: u64, skill: &str) -> Value {
    eprintln!(
        "lamp-voice: request {request}: an answer to {skill}'s say-back not taken: its line was only the lamp's name; the wait goes on (#6307)"
    );
    json!({ "approved": false, "error": NAME_ONLY_NOT_TAKEN })
}

/// What the model is told when the person declined the say-back.
pub const DECLINED_NEXT: &str = "Nothing was agreed. Say only, in the conversation's language, \
     that it stays off: nothing runs or is sent. Ask nothing, call nothing.";
/// What the model is told when the person declined a remove (#6960): it
/// stays, by its spoken name.
pub fn remove_declined_next(spoken: &str) -> String {
    format!(
        "Nothing was removed. Say only, in a few words, in the conversation's language, that \
         {spoken} stays. Ask nothing, call nothing."
    )
}
/// What the model is told when the answer to a remove was unclear, once:
/// its one question again, short.
pub fn remove_unclear_next(spoken: &str) -> String {
    format!(
        "Nothing was removed yet: their answer wasn't clear. Ask them once, short, in the \
         conversation's language: remove {spoken}, yes or no? Call nothing now."
    )
}
/// What the model is told when the line after a remove's question was a
/// new ask ([`NEW_ASK`]): nothing removed, the skill left as it is, and
/// the new ask answered as any line's.
pub fn remove_left_next(spoken: &str) -> String {
    format!(
        "Nothing was removed: their line was a new ask, not an answer to the lamp's question. \
         Say first, in a few words, in the conversation's language, that you left {spoken} as \
         it is; then do what they asked in that line, as for any line."
    )
}
/// What the model is told when a yes to a remove was no plain yes on the
/// line right after its question ([`not_a_plain_yes`]): nothing removed,
/// the skill left as it is, said first, and anything else the line asked
/// done as for any line.
pub fn remove_not_plain_next(spoken: &str) -> String {
    format!(
        "Nothing was removed: only a plain yes, right after the lamp's question, removes a \
         skill. Say first, in a few words, in the conversation's language, that you left \
         {spoken} as it is; then do anything else they asked in that line, as for any line."
    )
}
/// What the model is told when the line after a say-back was a new ask
/// ([`NEW_ASK`]): nothing agreed, and the new ask answered as any line's.
pub fn new_ask_next(spoken: &str) -> String {
    format!(
        "Nothing was agreed: their line was a new ask, not an answer to the say-back, so \
         {spoken} stays as it was and nothing runs or is sent for it. Say so first, in a few \
         words, in the conversation's language; then do what they asked in that line, as for \
         any line."
    )
}
/// What the model is told when the person's answer was unclear, once.
pub const UNCLEAR_NEXT: &str = "Nothing was agreed yet: their answer wasn't clear. Ask them \
     once, short, in the conversation's language, whether it may go ahead, yes or no. Call \
     nothing now.";

/// The person's `answer` ([`ANSWER`]) to the wait `id` in `request`, when
/// it is not a yes: a no ends the wait and keeps nothing; an unclear one
/// keeps nothing and asks once more, and a second unclear ends the wait as
/// a no; a new ask ([`NEW_ASK`]) ends the wait, keeps nothing, and lets the
/// model answer that line as any. What the model is told; none for a yes
/// bound to its skill (below; or no answer given to a voice make's), so
/// [`approve`] takes it with every guard, and none for an id not waiting.
/// Read before the owner check: anyone's no or unclear (a guest's too)
/// ends or re-asks the owner's wait, never keeping a yes. An answer
/// outside [`ANSWERS`] reads as a no. Two answers in one line are taken
/// in call order, each as its own: once the wait has ended, the next finds
/// no id waiting.
///
/// A yes is bound to its skill (#7545, Twelve's and El's HOLDs on #7561):
/// `named` ([`NAMED`]), the skill the person's line names as the model
/// gave it, must be [`NAMED_NONE`] or the waiting skill's own name
/// ([`crate::made_remove::same_name`]). A yes naming another skill is read
/// as a [`NEW_ASK`], whatever the model labelled it, so it never acts on
/// this wait; a forced wait's yes ([`answer_forced`]) with no name given
/// reads as unclear. Structure, not words: code compares the name the
/// model gave with the wait's skill, never the person's words.
pub fn answered(
    id: &str,
    request: u64,
    answer: Option<&str>,
    named: Option<&str>,
) -> Option<Value> {
    kept::waiting(|kept| {
        let waiting = kept.as_mut().filter(|waiting| waiting.id == id)?;
        // Only the lamp's name ([`line_heard`]): no answer at all, the wait
        // as it was.
        if name_only_line(waiting, request) {
            return Some(name_only_not_taken(request, &waiting.asks.skill));
        }
        // Left out: a yes for a made skill's writes or code, as before;
        // for a skill's task or an app's make with code, whose approval
        // call code forces whatever the person said ([`approve_force`]),
        // unclear, so a forced call with no answer never keeps a yes nobody
        // gave.
        let answer = match answer {
            Some(answer) => answer,
            None if answer_forced(waiting) => "unclear",
            None => "yes",
        };
        let skill = waiting.asks.skill.clone();
        let remove = waiting.asks.remove;
        let code_of_a_voice_make = waiting.asks.code && !answer_forced(waiting);
        let spoken = crate::made_skill::spoken_name(&skill);
        // A yes, bound to the skill waiting (#7545): taken by [`approve`]
        // with every guard only when the line names that skill or none.
        // For a skill's task, the heard line itself is read too (#7634):
        // one that names another skill ([`line_heard`]) is never its yes,
        // whatever skill the model named.
        let mut other = None;
        let heard_other = waiting
            .other_named
            .iter()
            .find(|(kept, _)| *kept == request)
            .map(|(_, other)| other.clone());
        let answer = if answer == "yes"
            && let Some(heard_other) = heard_other
        {
            other = Some(heard_other);
            NEW_ASK
        } else if answer == "yes" {
            match named.map(str::trim).filter(|named| !named.is_empty()) {
                Some(named)
                    if named.eq_ignore_ascii_case(NAMED_NONE)
                        || crate::made_remove::same_name(named, &skill) =>
                {
                    // A remove's yes, held in code to a plain yes on the
                    // line right after its question (lamp-9adf, 18 Nov,
                    // stock 14083: "Lamp, also make one that plays a soft
                    // chime …" was the model's yes, and daily stretch
                    // reminder was lost): else it is left as it is. A plain
                    // yes, or any other wait's, is taken by [`approve`].
                    let why = remove
                        .then(|| not_a_plain_yes(waiting, request))
                        .flatten()?;
                    *kept = None;
                    eprintln!(
                        "lamp-voice: request {request}: a yes to removing {skill} not taken: {why}: nothing removed, the wait dropped (#6960)"
                    );
                    return Some(json!({
                        "approved": false,
                        "left": skill,
                        "next": remove_not_plain_next(&spoken)
                    }));
                }
                Some(named) => {
                    other = Some(named.to_owned());
                    NEW_ASK
                }
                None if answer_forced(waiting) => {
                    eprintln!(
                        "lamp-voice: request {request}: a yes to {skill}'s wait named no skill: read as unclear"
                    );
                    "unclear"
                }
                None => return None,
            }
        } else {
            answer
        };
        // A new ask, not an answer (#7545, lamp-9adf 17 Nov), or a yes
        // naming another skill: the wait ends with nothing kept, so no
        // later yes can take it, and the line is answered as any line's.
        if answer == NEW_ASK {
            *kept = None;
            eprintln!(
                "lamp-voice: request {request}: the line after {skill}'s {} was a new ask, not an answer{}: nothing kept, the wait dropped",
                if remove { "question" } else { "say-back" },
                other
                    .map(|other| format!(" (a yes naming {other}, not {skill})"))
                    .unwrap_or_default()
            );
            let next = if remove {
                remove_left_next(&spoken)
            } else {
                new_ask_next(&spoken)
            };
            return Some(json!({ "approved": false, "left": skill, "next": next }));
        }
        if answer == "unclear" && !waiting.unclear {
            waiting.unclear = true;
            // A remove's question is asked again: the line after that is
            // its answer line now.
            waiting.answer_line = None;
            eprintln!(
                "lamp-voice: request {request}: the answer to {skill}'s say-back was unclear: nothing kept, asked once more"
            );
            let next = if remove {
                remove_unclear_next(&spoken)
            } else {
                UNCLEAR_NEXT.to_owned()
            };
            return Some(json!({ "approved": false, "next": next }));
        }
        *kept = None;
        if remove {
            eprintln!(
                "lamp-voice: request {request}: the answer to removing {skill} was {}: it stays",
                if answer == "unclear" {
                    "unclear again"
                } else {
                    "no"
                }
            );
            return Some(json!({ "approved": false, "next": remove_declined_next(&spoken) }));
        }
        eprintln!(
            "lamp-voice: request {request}: the answer to {skill}'s say-back was {}: nothing kept, it stays off",
            if answer == "unclear" {
                "unclear again"
            } else {
                "no"
            }
        );
        // A voice make's own code, never on: dropped, said plainly.
        let next = if code_of_a_voice_make {
            crate::make_skill_local::WONT_MAKE_NEXT
        } else {
            DECLINED_NEXT
        };
        Some(json!({ "approved": false, "next": next }))
    })
}

/// The line of `request` was held as the lamp's own echo while the yes was
/// awaited: no approval in it.
pub fn echo_held(request: u64) {
    kept::waiting(|waiting| {
        if let Some(waiting) = waiting.as_mut()
            && request > waiting.request
            && !waiting.echoes.contains(&request)
        {
            waiting.echoes.push(request);
        }
    });
}

/// The lamp said `words` in `request` (`thread::Thread::lamp_for`, as
/// played): kept toward the say-back while a yes waits, for the make's
/// request and any after it.
pub fn lamp_said(request: u64, words: &str) {
    kept::waiting(|waiting| {
        let Some(waiting) = waiting
            .as_mut()
            .filter(|waiting| request >= waiting.request)
        else {
            return;
        };
        match waiting.said.iter_mut().find(|(id, ..)| *id == request) {
            Some((_, said, _)) => {
                if said.len() + words.len() <= SAID_MOST {
                    said.push_str(words);
                }
            }
            None => {
                if waiting.said.len() == SAID_KEPT {
                    waiting.said.pop_front();
                }
                let words: String = words.chars().take(SAID_MOST).collect();
                waiting.said.push_back((request, words, false));
            }
        }
    });
}

/// The lamp's answer in `request` was cut (`thread::Thread::heard_only`):
/// a say-back cut is no say-back.
pub fn lamp_cut(request: u64) {
    kept::waiting(|waiting| {
        let Some(waiting) = waiting
            .as_mut()
            .filter(|waiting| request >= waiting.request)
        else {
            return;
        };
        match waiting.said.iter_mut().find(|(id, ..)| *id == request) {
            Some((_, _, cut)) => *cut = true,
            None => {
                if waiting.said.len() == SAID_KEPT {
                    waiting.said.pop_front();
                }
                waiting.said.push_back((request, String::new(), true));
            }
        }
    });
}

/// Whether one of the lamp's answers before `request`, uncut, held the
/// whole say-back.
fn said_whole_before(waiting: &Waiting, request: u64) -> bool {
    waiting.said.iter().any(|(id, said, cut)| {
        *id < request && !cut && crate::services::read_back_said(&waiting.asks.say_back, said)
    })
}

/// Whether one of the lamp's answers toward `waiting` before `request` was
/// cut and none before it held the whole say-back uncut (#6715): a cut
/// say-back still to be said again. A later line with no answer kept in
/// the wait's own request counts as cut too: the say-back was cut off
/// before its first word was kept (#7072, c0a4 14 Nov: a task's unasked
/// say-back, request 4, dropped as the "yes" of request 5 began 0.7 s
/// later, "kept: none"), so the yes over it found no cut and the note gave
/// the approve_id.
fn cut_unsaid(waiting: &Waiting, request: u64) -> bool {
    !said_whole_before(waiting, request)
        && (waiting
            .said
            .iter()
            .any(|(id, _, cut)| *id < request && *cut)
            || (request > waiting.request
                && !waiting.said.iter().any(|(id, ..)| *id == waiting.request)))
}

/// Where `said` first falls short of `say_back`, as
/// [`crate::services::read_back_said`] checks it: the 1-based place of the
/// first say-back word it does not hold in order, and the say-back's word
/// count; none when it holds all. Places only, never a word: the say-back
/// is the write's own content, the person's.
fn first_missing(say_back: &str, said: &str) -> Option<(usize, usize)> {
    let mut said = crate::services::check_words(said).into_iter();
    let wanted = crate::services::check_words(say_back);
    wanted
        .iter()
        .position(|word| !said.any(|got| &got == word))
        .map(|at| (at + 1, wanted.len()))
}

/// The journal's line of a yes in `request` not taken because the
/// say-back was not heard whole (#6715): each of the lamp's answers kept
/// toward it, by request, cut or not and its word count, and the first
/// say-back word missing from the best of them (the one before `request`
/// that holds the most of it in order, an uncut one on a tie). Counts and
/// places only, never a word of the say-back nor of what the lamp said: a
/// say-back is the person's content, and the journal holds counts and
/// outcomes only (`LAMP_SKILL_WRITES`).
fn not_said_whole_line(waiting: &Waiting, request: u64) -> String {
    let kept: Vec<String> = waiting
        .said
        .iter()
        .map(|(id, said, cut)| {
            format!(
                "r{id} cut={cut} {} words",
                crate::services::check_words(said).len()
            )
        })
        .collect();
    let held = |said: &str| {
        let wanted = crate::services::check_words(&waiting.asks.say_back);
        let mut words = crate::services::check_words(said).into_iter();
        wanted
            .iter()
            .take_while(|word| words.any(|got| &got == *word))
            .count()
    };
    let best = waiting
        .said
        .iter()
        .filter(|(id, ..)| *id < request)
        .max_by_key(|(_, said, cut)| (held(said), !cut));
    let missing = match best {
        None => "no answer kept".to_owned(),
        Some((id, said, cut)) => match first_missing(&waiting.asks.say_back, said) {
            Some((at, of)) => format!("first missing word in r{id}: {at} of {of}"),
            None if *cut => format!("r{id} held it all but was cut"),
            None => format!("r{id} held it all"),
        },
    };
    let kept = if kept.is_empty() {
        "none".to_owned()
    } else {
        kept.join("; ")
    };
    format!(
        "lamp-voice: request {request}: a made skill's yes not taken: its say-back was not heard whole (kept: {kept}; {missing})"
    )
}

/// The journal's line when the lamp's words of `request` skip the thread
/// (`second_provider::worker`, `guard` naming why) while a skill's yes
/// waits (#6715): they never count toward its say-back. None while
/// nothing waits.
pub fn skipped_line(request: u64, guard: &str) -> Option<String> {
    kept::waiting(|waiting| {
        waiting.as_ref().map(|waiting| {
            format!(
                "lamp-voice: request {request}: the lamp's words skip the thread ({guard}) while a skill's yes waits (made in request {}): they don't count toward its say-back",
                waiting.request
            )
        })
    })
}

/// What the model is told to say of a made skill whose writes wait: `line`
/// word for word, every part of it.
pub fn say_whole(line: &str) -> String {
    format!(
        "Say this word for word as written, leaving nothing out: every address, every quoted \
         word, when and how often: \"{line}\" If the conversation is in another language, say \
         it in that language too, after it."
    )
}

/// What the model is told after the say-back: ask nothing; on an
/// agreement, the call with `id`; on anything else, no call.
pub fn then_wait(id: &str) -> String {
    format!(
        "End as the line ends, asking nothing. Nothing is sent until they agree. If their next \
         words agree, in any words, call make_skill again with only approve_id \"{id}\". If they \
         decline, wait or change it, don't call it: nothing is sent, and a change is a new \
         make_skill."
    )
}

/// What the model is told when an approval is not taken: the way to a
/// yes is the read-back (`make_skill_local::read_back`), never the app,
/// which has none (Thirteen's HOLD on #6223: lamp-cloud has no kind for
/// `skill_write_approve`).
pub const NOT_APPROVED: &str = "Not approved: nothing waits for a yes now, or that was not the person's own answer. Say nothing is sent, and that they can ask me what that skill sends (\"what does my … skill send?\") to hear it again and say yes then.";
/// What the model is told when the yes came before the say-back was
/// heard whole (it waits on).
pub const NOT_SAID_WHOLE: &str =
    "Not approved yet: what it sends was not said in full, or it was cut. Nothing is sent.";
/// What the model is told when the yes came on the Live route: nothing
/// sent, and the yes taken on the lamp's other voice, never here, where
/// asking again ends at `make_skill_local::yes_not_here` (App Only's P3 on
/// #6250).
pub const NOT_HERE: &str = "Not approved: this live voice can't take that yes. Say plainly that nothing was sent, and that I take that yes only on my other voice: in a normal conversation with me, not this live one.";
/// What the model is told when a remove's yes is not taken (#6960):
/// nothing removed, and it may be asked again.
pub const REMOVE_NOT_TAKEN: &str = "Not removed: that was not the person's own answer to the lamp's question, or it came too late. Say in a few words that nothing was removed, and that they can ask me again.";
/// What the model is told when a remove's yes came on the Live route.
pub const REMOVE_NOT_HERE: &str = "Not removed: this live voice can't take that answer. Say plainly that nothing was removed, and that I remove a skill only on my other voice: in a normal conversation with me, not this live one.";
/// What the model is told once a remove is done (#6960). The rest of the
/// person's line, if it asked for more ("yes, and remove the joke one
/// too", El's P2 on #7561), is answered as any line's, never lost: a
/// further remove gets its own question and wait ([`crate::made_remove`]).
pub fn removed_next(spoken: &str) -> String {
    format!(
        "Say first, in a few words, in the conversation's language, that {spoken} is removed. \
         If their line also asked for more (another skill to remove, another thing asked), do \
         that now as for any line: another remove is asked as its own question. Else ask \
         nothing more."
    )
}
/// What the model is told once approved: it sends at its next trigger,
/// never now (#6749 A, as [`crate::skill_task_yes::APPROVED_NEXT`]).
pub const APPROVED_NEXT: &str = "Say only, in the conversation's language, that it'll send as it \
     said the next time it's triggered. Never say it's sending now: nothing is sent yet.";
/// What the model is told once a skill with code of its own is agreed to
/// and on (#6307): it runs at its next trigger, never now, and a later ask
/// for it is a new run (#6749 A).
pub const CODE_APPROVED_NEXT: &str = "Say only, in the conversation's language, that it's on now \
     and runs as it said the next time it's asked for or triggered. Never say it's running now: \
     nothing runs yet. (For you to know, not to say: each later ask for it runs it again, so \
     call that skill then, as for any skill.)";

/// What the model is told once a skill with code of its own is on but its
/// task wasn't kept (#6307): a skill's tasks off, or its folder's grants
/// not those said back, so its code runs nowhere.
pub const CODE_ON_TASK_WAITS_NEXT: &str = "Say only, in the conversation's language, that it's \
     on now, but this lamp doesn't run its code yet.";

/// `skill`'s task or code agreed to in `request` at `now`: noted on the
/// lines after it ([`agreed_note`]).
fn agreed_in(skill: &str, request: u64, now: Instant) {
    kept::agreed(|agreed| {
        *agreed = Some(Agreed {
            skill: skill.to_owned(),
            request,
            at: now,
        });
    });
}

/// Why a yes is never taken, or `None`.
fn refused(waiting: &Waiting, request: u64, heard: bool, now: Instant) -> Option<&'static str> {
    if !heard {
        Some("no one spoke it")
    } else if request <= waiting.request {
        Some("it came in the make's own request")
    } else if waiting.echoes.contains(&request) {
        Some("its line was the lamp's own words heard back")
    } else if now.saturating_duration_since(waiting.at) > YES_WITHIN {
        Some("it came too late")
    } else {
        None
    }
}

/// `make_skill` with `approve_id` `id` in `request` (`heard`: a line
/// someone spoke), at `now`: the waiting skill's writes approved through
/// `approve` (`skill_write_approve`, with its name, its package's SHA-256
/// and its writes) when it is the very one waiting, in a later heard
/// request of the route with the echo judge (`sends_here`), not a line
/// held as the lamp's echo, within [`YES_WITHIN`], once the lamp's words
/// held its whole say-back uncut. A yes before that is told to say it
/// again, and the wait goes on; any other try ends it. What the model is
/// told.
pub fn approve(
    id: &str,
    request: u64,
    heard: bool,
    sends_here: bool,
    now: Instant,
    approve: impl FnOnce(&Asks) -> Result<Value, String>,
) -> Value {
    /// Which wait is told to wait on: a made skill's writes or code, a
    /// skill's task, or a remove (#6960).
    enum Kind {
        Make,
        Task,
        Remove,
    }
    enum Took {
        Nothing,
        NameOnly(Value),
        NotSaid(String, String, Kind, String),
        Waiting(Box<Waiting>),
    }
    let took = kept::waiting(|kept| {
        let Some(waiting) = kept.as_mut().filter(|waiting| waiting.id == id) else {
            return Took::Nothing;
        };
        // Only the lamp's name ([`line_heard`]): never the yes, and the
        // wait goes on.
        if name_only_line(waiting, request) {
            return Took::NameOnly(name_only_not_taken(request, &waiting.asks.skill));
        }
        if refused(waiting, request, heard, now).is_none()
            && sends_here
            && !said_whole_before(waiting, request)
        {
            // Asked again (Twelve's P2 on #7683): the line after the
            // question said again is the remove's answer line now, as
            // after an unclear answer; a "Yes." over the question itself
            // never uses it up.
            waiting.answer_line = None;
            let kind = if waiting.asks.remove {
                Kind::Remove
            } else if waiting.asks.grants.is_some() {
                Kind::Task
            } else {
                Kind::Make
            };
            return Took::NotSaid(
                waiting.id.clone(),
                waiting.asks.say_back.clone(),
                kind,
                not_said_whole_line(waiting, request),
            );
        }
        kept.take()
            .map_or(Took::Nothing, |waiting| Took::Waiting(Box::new(waiting)))
    });
    let waiting = match took {
        Took::Nothing => {
            eprintln!("lamp-voice: request {request}: a made skill's yes found nothing waiting");
            return json!({ "error": NOT_APPROVED });
        }
        Took::NameOnly(told) => return told,
        Took::NotSaid(id, say_back, kind, line) => {
            eprintln!("{line}");
            let (error, say, wait) = match kind {
                Kind::Remove => (
                    "Not removed yet: the lamp's question was not said in full, or it was cut. Nothing is removed.",
                    crate::made_remove::say_question(&say_back),
                    crate::made_remove::then_wait(&id),
                ),
                Kind::Task => (
                    NOT_SAID_WHOLE,
                    say_whole(&say_back),
                    crate::skill_task_yes::then_wait(&id),
                ),
                Kind::Make => (NOT_SAID_WHOLE, say_whole(&say_back), then_wait(&id)),
            };
            return json!({
                "error": error,
                "next": format!("{say} {wait}"),
            });
        }
        Took::Waiting(waiting) => waiting,
    };
    let remove = waiting.asks.remove;
    if let Some(why) = refused(&waiting, request, heard, now) {
        eprintln!("lamp-voice: request {request}: a made skill's yes not taken: {why}");
        return json!({ "error": if remove { REMOVE_NOT_TAKEN } else { NOT_APPROVED } });
    }
    if !sends_here {
        eprintln!(
            "lamp-voice: request {request}: a made skill's yes not taken: this route has no echo judge"
        );
        return json!({ "error": if remove { REMOVE_NOT_HERE } else { NOT_HERE } });
    }
    match approve(&waiting.asks) {
        // A remove (#6960): the made skill taken off and dropped.
        Ok(result) if remove => {
            eprintln!(
                "lamp-voice: request {request}: a made skill removed on the person's yes (#6960){}",
                if result["still_listed"] == true {
                    ": off the lamp, still listed among the made skills"
                } else {
                    ""
                }
            );
            // Taken off but still listed (#6969): its own words, as it is.
            let next = result["next"].as_str().map_or_else(
                || removed_next(&crate::made_skill::spoken_name(&waiting.asks.skill)),
                str::to_owned,
            );
            let mut told = json!({
                "approved": true,
                "removed": waiting.asks.skill,
                "next": next,
            });
            if result["still_listed"] == true {
                told["still_listed"] = json!(true);
            }
            told
        }
        Err(why) if remove => {
            eprintln!("lamp-voice: request {request}: a made skill's remove failed: {why}");
            json!({
                "error": format!("Not removed: {why}."),
                "next": "Say it wasn't removed, and why, in a few words. Call nothing."
            })
        }
        // Code of its own (#6307): that package put on, nothing sent, and
        // its task kept when the lamp hands tasks over (#6305).
        Ok(result) if waiting.asks.code => {
            let task = result["task"] == true;
            if task {
                agreed_in(&waiting.asks.skill, request, now);
            }
            eprintln!(
                "lamp-voice: request {request}: a made skill with code of its own is on, on the person's yes (#6307); its task {}",
                if task { "kept" } else { "not kept" }
            );
            let next = if task {
                CODE_APPROVED_NEXT
            } else {
                CODE_ON_TASK_WAITS_NEXT
            };
            // Named as a make names what it put on, so a true "X is on now"
            // after it stands (`after_forget::status`, #7730).
            json!({ "approved": true, "made": waiting.asks.skill, "on": true, "task": task, "next": next })
        }
        // A skill's task (#6305): its grants and folder kept, as said.
        Ok(result) if waiting.asks.grants.is_some() => {
            agreed_in(&waiting.asks.skill, request, now);
            eprintln!(
                "lamp-voice: request {request}: a skill's task approved on the person's yes (#6305)"
            );
            // An agent skill's first run asked it: run now, as asked.
            let next = match result["call"].as_str() {
                Some(tool) => crate::upload_agent::approved_next(tool),
                None => crate::skill_task_yes::APPROVED_NEXT.to_owned(),
            };
            json!({ "approved": true, "next": next })
        }
        Ok(result) => {
            eprintln!(
                "lamp-voice: request {request}: a made skill's {} write(s) approved on the person's yes (#6066)",
                waiting.asks.writes.len()
            );
            let public: Vec<&str> = result["public"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let next = match public.as_slice() {
                [] => APPROVED_NEXT.to_owned(),
                repos => format!(
                    "{APPROVED_NEXT} Say too that anyone can see what it posts in {}.",
                    repos.join(" and ")
                ),
            };
            json!({ "approved": true, "next": next })
        }
        Err(why) => {
            // `lamp-services`' words name no recipient or text.
            eprintln!("lamp-voice: request {request}: a made skill's yes not kept: {why}");
            json!({
                "error": format!("Not approved: {why}."),
                "next": "Say nothing is sent, and why, in a few words."
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail() -> Value {
        json!({"service": "gmail", "action": "send_mail", "to": ["mai@example.com"],
               "subject": "Leaving", "text": "I'm leaving now."})
    }

    const SAY_BACK: &str =
        "mails mai@example.com from your Gmail, subject “Leaving”, saying “I'm leaving now.”";

    /// How a cut code make's own line ends (`made_skill::cut_code_line`).
    const CUT: &str = crate::made_skill::CUT_CODE_UNSAID;

    fn asks(skill: &str, writes: &[Value]) -> Asks {
        Asks {
            skill: skill.to_owned(),
            sha256: "ab".repeat(32),
            writes: writes.to_vec(),
            say_back: SAY_BACK.to_owned(),
            grants: None,
            code: false,
            remove: false,
        }
    }

    /// A wait whose say-back the lamp said whole in the make's `request`.
    fn said_back(skill: &str, writes: &[Value], request: u64, now: Instant) -> String {
        let id = wait_for_yes(asks(skill, writes), request, now);
        lamp_said(request, "I made leaving mail: when you get up, it ");
        lamp_said(request, &format!("{SAY_BACK}, at most once an hour."));
        id
    }

    fn never(_: &Asks) -> Result<Value, String> {
        panic!("approved")
    }

    /// A yes in a later heard request approves exactly the skill's writes
    /// and the package said back, once; "anyone can see it" only for a
    /// public repository.
    #[test]
    fn a_later_heard_yes_approves_the_waiting_writes_once() {
        let now = Instant::now();
        let id = said_back("leaving-mail", &[mail()], 4, now);
        assert!(awaited(now));
        let mut asked = None;
        let told = approve(&id, 5, true, true, now, |asks| {
            asked = Some(asks.clone());
            Ok(json!({"approved": 1, "public": []}))
        });
        assert_eq!(told["approved"], true, "{told}");
        assert_eq!(told["next"], APPROVED_NEXT);
        assert_eq!(asked, Some(asks("leaving-mail", &[mail()])));
        assert!(!awaited(now));
        // Once only.
        let again = approve(&id, 6, true, true, now, never);
        assert_eq!(again["error"], NOT_APPROVED);
        // A public repository is said.
        let id = said_back("standup", &[json!({"service": "github"})], 7, now);
        let told = approve(&id, 8, true, true, now, |_| {
            Ok(json!({"approved": 1, "public": ["acme/site"]}))
        });
        assert!(
            told["next"]
                .as_str()
                .unwrap()
                .contains("anyone can see what it posts in acme/site")
        );
    }

    /// #6749 C: a line answers the say-back only while the yes is awaited,
    /// after the make's own request, and while the lamp's newest answer
    /// before it held the say-back whole and uncut: an answer of the lamp's
    /// since, a cut, or the wait past [`YES_WITHIN`] ends it.
    #[test]
    fn a_line_answers_the_say_back_only_right_after_it_is_said_whole() {
        let now = Instant::now();
        clear_for_test();
        assert!(!answer_awaited(5, now), "nothing waits");
        let id = wait_for_yes(asks("leaving-mail", &[mail()]), 4, now);
        assert!(!answer_awaited(5, now), "not said yet");
        lamp_said(4, &format!("I made leaving mail: it {SAY_BACK}"));
        assert!(!answer_awaited(4, now), "the make's own request");
        assert!(answer_awaited(5, now));
        assert!(!answer_awaited(
            5,
            now + YES_WITHIN + Duration::from_secs(1)
        ));
        lamp_said(5, "Sure, it's sunny today.");
        assert!(
            !answer_awaited(6, now),
            "the lamp said something else since"
        );
        lamp_said(6, SAY_BACK);
        assert!(answer_awaited(7, now), "said again whole");
        // An answer cut before any of its words were kept said nothing
        // (9adf, 15 Nov 17:24): the say-back still holds.
        lamp_cut(7);
        assert!(answer_awaited(8, now), "cut before a word");
        lamp_said(8, &SAY_BACK[..SAY_BACK.len() / 2]);
        lamp_cut(8);
        assert!(!answer_awaited(9, now), "said again and cut");
        assert!(waits(&id));
    }

    /// #6749 A (lamp-52e6, 13 Nov): after a kept yes the model was told the
    /// task "will now" run, took it as under way, and on the next "desk
    /// check" called nothing. What it is told after a task's or code's yes
    /// says it runs at its next trigger and never that it runs now; the
    /// lines after it are told the yes ran nothing and an ask for it now is
    /// a new run, so it calls the skill. A write's yes says it sends at its
    /// next trigger; it notes nothing after.
    #[test]
    fn after_a_kept_yes_the_task_runs_at_its_next_trigger() {
        let now = Instant::now();
        for next in [crate::skill_task_yes::APPROVED_NEXT, CODE_APPROVED_NEXT] {
            assert!(
                next.contains("the next time it's asked for or triggered"),
                "{next}"
            );
            assert!(next.contains("Never say it's running now"), "{next}");
            assert!(next.contains("each later ask for it runs"), "{next}");
            assert!(next.contains("call that skill then"), "{next}");
            assert!(!next.contains("will now"), "{next}");
        }
        assert!(APPROVED_NEXT.contains("the next time it's triggered"));
        assert!(!APPROVED_NEXT.contains("will now"));
        for (code, said) in [(false, "desk check"), (true, "desk check")] {
            let task = Asks {
                grants: Some(vec!["light".to_owned()]),
                writes: Vec::new(),
                code,
                remove: false,
                ..asks("desk-check", &[])
            };
            let id = wait_for_yes(task, 4, now);
            lamp_said(4, SAY_BACK);
            let told = approve(&id, 5, true, true, now, |_| {
                Ok(json!({"approved": 1, "task": true}))
            });
            assert_eq!(told["approved"], true, "{told}");
            let next = if code {
                CODE_APPROVED_NEXT
            } else {
                crate::skill_task_yes::APPROVED_NEXT
            };
            assert_eq!(told["next"], next);
            assert!(!awaited(now));
            assert!(waiting_note(5, now).is_none(), "the yes's own request");
            let note = waiting_note(6, now).unwrap();
            assert!(note.contains(&format!("the skill {said}")), "{note}");
            for words in ["ran nothing", "call that skill", "isn't already running"] {
                assert!(note.contains(words), "{note}");
            }
            assert!(waiting_note(6, now + YES_WITHIN + Duration::from_secs(1)).is_none());
            // A new wait's note replaces it.
            let id = said_back("leaving-mail", &[mail()], 7, now);
            let note = waiting_note(8, now).unwrap();
            assert!(
                note.contains(&id) && !note.contains("ran nothing"),
                "{note}"
            );
            // A write's yes notes nothing after it.
            approve(&id, 8, true, true, now, |_| Ok(json!({"public": []})));
            assert!(waiting_note(9, now).is_none());
        }
    }

    /// #6223's row 4 on 9adf (12 Nov): a voice make's wait, whose id only
    /// the make's call answer told, is noted on each later heard line, so
    /// the next line's yes can name it; a task's and code's say which.
    #[test]
    fn every_wait_is_noted_on_the_lines_after_it() {
        let now = Instant::now();
        let id = said_back("leaving-mail", &[mail()], 4, now);
        assert!(waiting_note(4, now).is_none());
        let next = waiting_note(5, now).unwrap();
        assert!(next.contains(&format!("approve_id \"{id}\"")), "{next}");
        assert!(next.contains("before it sends anything"), "{next}");
        let told = approve(&id, 5, true, true, now, |_| {
            Ok(json!({"approved": 1, "public": []}))
        });
        assert_eq!(told["approved"], true, "{told}");
        assert!(waiting_note(6, now).is_none());
        // Past YES_WITHIN: no note.
        let id = said_back("leaving-mail", &[mail()], 7, now);
        assert!(waiting_note(8, now + YES_WITHIN + Duration::from_secs(1)).is_none());
        assert!(waiting_note(8, now).unwrap().contains(&id));
        let task = Asks {
            grants: Some(vec!["camera".to_owned()]),
            writes: Vec::new(),
            ..asks("desk-watch", &[])
        };
        wait_for_yes(task, 9, now);
        // Its say-back said in its own request (none kept reads as cut
        // before its first word, #7072).
        lamp_said(9, "It may use the camera.");
        assert!(
            waiting_note(10, now)
                .unwrap()
                .contains("before its task runs")
        );
        let code = Asks {
            code: true,
            remove: false,
            writes: Vec::new(),
            ..asks("desk-watch", &[])
        };
        wait_for_yes(code, 11, now);
        lamp_said(11, "It runs code of its own.");
        assert!(
            waiting_note(12, now)
                .unwrap()
                .contains("before it runs any code")
        );
    }

    /// On lamp-9adf (15 Nov 17:24) an app's make with code was said back,
    /// the owner's "Hey Lamp" was answered as a call and cut before a word
    /// played, then came their "Yes.": that empty cut answer stands between
    /// nothing, so the yes is still awaited and, for an app's make (noted),
    /// forced to answer its wait. A cut answer that played words still
    /// stands between, the say-back's own request cut before its first word
    /// still counts as cut (#7072), and a voice make's code is not forced.
    #[test]
    fn an_app_makes_yes_past_a_cut_call_is_forced_to_answer_its_wait() {
        let now = Instant::now();
        let code = Asks {
            code: true,
            writes: Vec::new(),
            ..asks("noon-photo", &[])
        };
        let id = wait_for_yes_noted(code.clone(), 17, now);
        lamp_said(17, SAY_BACK);
        assert!(answer_awaited(18, now));
        assert!(approve_forced_now(now));
        lamp_cut(18);
        assert!(answer_awaited(19, now), "the cut call said nothing");
        assert!(approve_forced_now(now));
        let note = waiting_note(19, now).unwrap();
        assert!(note.contains(&id), "{note}");
        assert!(note.contains("always give answer"), "{note}");
        // The forced call's no ends the wait: it stays off.
        let no = answered(&id, 19, Some("no"), None).unwrap();
        assert_eq!(no["next"], DECLINED_NEXT, "{no}");
        assert!(!awaited(now));
        // A cut answer that played words stands between.
        wait_for_yes_noted(code.clone(), 20, now);
        lamp_said(20, SAY_BACK);
        lamp_said(21, "Hey, what do");
        lamp_cut(21);
        assert!(!answer_awaited(22, now));
        assert!(!approve_forced_now(now));
        // The say-back's own request cut with nothing kept (#7072).
        wait_for_yes_noted(code.clone(), 23, now);
        lamp_cut(23);
        assert!(!answer_awaited(24, now));
        assert!(!approve_forced_now(now));
        // A voice make's code: awaited, never forced.
        wait_for_yes(code, 25, now);
        lamp_said(25, SAY_BACK);
        assert!(answer_awaited(26, now));
        assert!(!approve_forced_now(now));
        let note = waiting_note(26, now).unwrap();
        assert!(!note.contains("always give answer"), "{note}");
        // lamp-9adf, 17 Nov: "No, don't turn it on." and "Maybe later."
        // after a voice make's say-back were answered with a question and
        // with how to start it. The note has the model answer a decline or
        // a wait as a no, and the no drops it, said plainly.
        assert!(
            note.contains("If they decline or put it off, call make_skill with that approve_id"),
            "{note}"
        );
        assert!(
            note.contains("never say it is on or how to start it"),
            "{note}"
        );
        let id = kept::waiting(|waiting| waiting.as_ref().unwrap().id.clone());
        let no = answered(&id, 26, Some("no"), None).unwrap();
        assert_eq!(no["next"], crate::make_skill_local::WONT_MAKE_NEXT, "{no}");
        assert!(!awaited(now));
        // A made skill's writes, on already: a decline calls nothing, as
        // before.
        said_back("leaving-mail", &[mail()], 27, now);
        let note = waiting_note(28, now).unwrap();
        assert!(
            note.contains("If they decline, wait or change it, don't call it"),
            "{note}"
        );
    }

    /// #7245, c0a4 17 Nov 19:43: a voice make with code outlived its turn
    /// (116 s on OpenClaw), its wait began in request 1, and its say-back
    /// was said through the say service in request 2. The "yes" of request
    /// 3 answers no call of the model's, as an app's make's: forced to
    /// answer the wait, and told to give its answer. Only the cut make's
    /// own wait is so.
    #[test]
    fn a_voice_makes_yes_after_its_say_back_through_the_say_service_is_forced() {
        let now = Instant::now();
        let code = Asks {
            code: true,
            writes: Vec::new(),
            ..asks("fruit-note", &[])
        };
        let id = wait_for_yes(code.clone(), 1, now);
        assert!(
            said_by_the_say_service(2, CUT).is_none(),
            "another request's make"
        );
        assert!(said_by_the_say_service(1, CUT).is_some());
        lamp_said(2, SAY_BACK);
        assert!(answer_awaited(3, now));
        assert!(approve_forced_now(now), "the yes of request 3 is forced");
        let note = waiting_note(3, now).unwrap();
        assert!(note.contains(&id), "{note}");
        assert!(note.contains("always give answer"), "{note}");
        // A voice make whose turn waited: never forced, as before.
        wait_for_yes(code, 4, now);
        lamp_said(4, SAY_BACK);
        assert!(!approve_forced_now(now));
    }

    /// #7245, c0a4 17 Nov 22:39 (the combo build): a voice make with code
    /// (62.9 s on OpenClaw) outlived its turn, request 3, so its wait began
    /// with nothing of request 3's kept toward it. Its say-back went through
    /// the say service as request 4, a turn of its own with no heard line,
    /// and the "yes" of request 5 must be forced to answer it. The say
    /// service is given the say-back word for word, ending as it ends, so
    /// the model says it whole: never the cut turn's "ask me for it again",
    /// which went as a note in the person's language, was said in 136
    /// chars, and left the say-back unheard, the yes told to hear it again,
    /// and nothing forced or kept.
    #[test]
    fn a_cut_code_makes_say_back_through_the_say_service_forces_the_next_yes() {
        let now = Instant::now();
        let grants = vec!["camera".to_owned()];
        let code = Asks {
            say_back: crate::made_skill::code_said_as(&grants, false)
                .trim()
                .to_owned(),
            grants: Some(grants),
            code: true,
            writes: Vec::new(),
            ..asks("fruit-notes", &[])
        };
        let id = wait_for_yes(code.clone(), 3, now);
        let line = said_by_the_say_service(3, CUT).expect("a code make's say-back is said whole");
        assert!(
            line.starts_with("I made fruit notes. The lamp's model wrote its code"),
            "{line}"
        );
        assert!(line.ends_with(&code.say_back), "{line}");
        assert!(
            line.ends_with(crate::made_skill::RUNS_AFTER_YES.trim()),
            "{line}"
        );
        // Request 4, the say service's own turn with no heard line, kept
        // only a summary (the cut turn's line in the person's words, as on
        // c0a4): not forced, and the next line is told to say it again.
        lamp_said(4, "I made fruit notes. It isn't on yet, just ask me again.");
        assert!(!approve_forced_now(now), "a summary is no say-back");
        let note = waiting_note(5, now).unwrap();
        assert!(!note.contains(&id) && note.contains("was cut"), "{note}");
        // Said word for word: the yes of request 6 is forced.
        lamp_said(5, &line);
        assert!(approve_forced_now(now), "the yes of request 6 is forced");
        let note = waiting_note(6, now).unwrap();
        assert!(note.contains(&id), "{note}");
        assert!(note.contains("always give answer"), "{note}");
        // Cut by a barge-in while it was said: not forced.
        lamp_cut(5);
        assert!(!approve_forced_now(now), "a cut say-back is no say-back");
        // Said whole again, then a "no": the wait ends, nothing kept.
        lamp_said(6, &line);
        assert!(approve_forced_now(now));
        // With #7561's bound yes: the forced note asks for the name, a yes
        // naming none is taken by the approve, and one with no name given
        // is unclear, the wait kept and asked once more.
        let note = waiting_note(7, now).unwrap();
        assert!(note.contains("Give named too"), "{note}");
        assert_eq!(answered(&id, 7, Some("yes"), Some(NAMED_NONE)), None);
        assert_eq!(answered(&id, 7, Some("yes"), Some("fruit notes")), None);
        let unclear = answered(&id, 7, Some("yes"), None).unwrap();
        assert_eq!(unclear["approved"], false, "{unclear}");
        assert!(waits(&id) && approve_forced_now(now));
        let no = answered(&id, 7, Some("no"), None).unwrap();
        assert_eq!(no["approved"], false, "{no}");
        assert!(!waits(&id) && !approve_forced_now(now));
        // A second make while one waits takes the wait: the first make's
        // cut line is its own again, never the second's say-back.
        wait_for_yes(code.clone(), 8, now);
        wait_for_yes(code.clone(), 9, now);
        assert_eq!(said_by_the_say_service(8, CUT), None);
        // Past the yes window the say-back's line forces nothing.
        let late = now + YES_WITHIN + Duration::from_secs(1);
        lamp_said(10, &said_by_the_say_service(9, CUT).unwrap());
        assert!(approve_forced_now(now));
        assert!(!approve_forced_now(late));
        // A task for the lamp's agent alike, its own head.
        let agent = Asks {
            say_back: crate::made_skill::agent_said(&["camera".to_owned()])
                .trim()
                .to_owned(),
            ..code.clone()
        };
        wait_for_yes(agent, 11, now);
        let line = said_by_the_say_service(11, CUT).unwrap();
        assert!(
            line.starts_with("I can have the lamp's agent do fruit notes. The lamp's agent"),
            "{line}"
        );
        assert!(
            line.ends_with(crate::made_skill::RUNS_ONCE_AGREED.trim()),
            "{line}"
        );
        // A make's writes keep the cut turn's own line (never forced).
        wait_for_yes(asks("leaving-mail", &[]), 12, now);
        assert_eq!(said_by_the_say_service(12, CUT), None);
        // So does a code make that also sends: its cut line is
        // `cut_write_line`, never swapped for a say-back silent on what it
        // sends.
        wait_for_yes(code.clone(), 13, now);
        let write_line = format!(
            "I made fruit notes.{} {}",
            crate::made_skill::WAITS_FOR_YES,
            crate::made_skill::CUT_WRITE_UNSAID
        );
        assert_eq!(said_by_the_say_service(13, &write_line), None);
    }

    /// lamp-9adf, 18 Nov 02:24–02:26 (#7554's row): a voice make waited for
    /// its yes, its turn cut, and three new make asks each got "I'll make
    /// that now. This takes about a minute. I'm on it." with no make_skill
    /// call: the note said "call nothing" and "don't call it" with nothing
    /// for another ask. Every note of the wait, cut or heard whole, has a
    /// different skill asked for made by its own call, and no make said as
    /// begun without one.
    #[test]
    fn a_new_make_ask_beside_a_make_waiting_for_its_yes_gets_its_own_call() {
        let now = Instant::now();
        let code = Asks {
            code: true,
            writes: Vec::new(),
            ..asks("newest-mail-subject", &[])
        };
        let new_ask = [
            "a new ask, not this one: call make_skill for it with its own request",
            "Never say a skill is being made unless make_skill is called for it in this turn",
        ];
        // Its turn cut before the say-back was heard.
        wait_for_yes(code.clone(), 5, now);
        lamp_cut(5);
        let note = waiting_note(8, now).unwrap();
        assert!(note.contains("was cut"), "{note}");
        assert!(note.contains("call nothing for it now"), "{note}");
        for words in new_ask {
            assert!(note.contains(words), "{words}: {note}");
        }
        // Heard whole.
        wait_for_yes(code, 10, now);
        lamp_said(10, SAY_BACK);
        let note = waiting_note(11, now).unwrap();
        assert!(!note.contains("was cut"), "{note}");
        assert!(
            note.contains("If they change this skill, don't call it"),
            "{note}"
        );
        for words in new_ask {
            assert!(note.contains(words), "{words}: {note}");
        }
    }

    /// App Only's P1 on #6142: a say-back cut, summed up or missing a
    /// recipient approves nothing; the model is told to say it whole again,
    /// the wait goes on, and a yes after it is said whole approves.
    #[test]
    fn a_cut_or_summarised_say_back_does_not_approve() {
        let now = Instant::now();
        for said in [
            None,
            Some("I made leaving mail: it mails Mai when you leave."),
            Some("it mails from your Gmail, subject “Leaving”, saying “I'm leaving now.”"),
            Some("it mails mai@example.com from your Gmail, subject “Leaving”"),
        ] {
            let id = wait_for_yes(asks("leaving-mail", &[mail()]), 4, now);
            if let Some(said) = said {
                lamp_said(4, said);
            }
            let told = approve(&id, 5, true, true, now, never);
            assert_eq!(told["error"], NOT_SAID_WHOLE, "{said:?}");
            let next = told["next"].as_str().unwrap();
            assert!(next.contains(SAY_BACK) && next.contains(&id), "{next}");
            assert!(awaited(now), "the wait goes on");
        }
        // Said whole but cut (a line or a Stop over it): not taken either.
        let id = said_back("leaving-mail", &[mail()], 4, now);
        lamp_cut(4);
        assert_eq!(
            approve(&id, 5, true, true, now, never)["error"],
            NOT_SAID_WHOLE
        );
        // Said again whole in that answer, a later yes approves.
        lamp_said(5, SAY_BACK);
        let told = approve(&id, 6, true, true, now, |_| Ok(json!({"approved": 1})));
        assert_eq!(told["approved"], true, "{told}");
        // Said whole only in the yes's own answer: not before it.
        let id = wait_for_yes(asks("leaving-mail", &[mail()]), 7, now);
        lamp_said(8, SAY_BACK);
        assert_eq!(
            approve(&id, 8, true, true, now, never)["error"],
            NOT_SAID_WHOLE
        );
    }

    /// #6715: a yes refused for a say-back not heard whole journals each
    /// answer kept toward it (request, cut, word count) and the first
    /// say-back word missing from the best one, and no more of the
    /// say-back; the words skipping the thread are journalled only while a
    /// yes waits.
    #[test]
    fn a_refused_yes_journals_the_answers_kept_and_the_first_word_missing() {
        let now = Instant::now();
        assert_eq!(
            skipped_line(3, "the lease's request"),
            None,
            "nothing waits"
        );
        let id = wait_for_yes(asks("leaving-mail", &[mail()]), 1, now);
        let waiting = || kept::waiting(|waiting| waiting.clone().unwrap());
        assert_eq!(
            not_said_whole_line(&waiting(), 2),
            "lamp-voice: request 2: a made skill's yes not taken: its say-back was not heard whole (kept: none; no answer kept)"
        );
        lamp_said(
            1,
            "it mails mai@example.com from your Gmail, subject “Leaving”",
        );
        lamp_said(2, "I need to read it back in full first.");
        lamp_cut(2);
        let line = not_said_whole_line(&waiting(), 3);
        assert_eq!(
            line,
            "lamp-voice: request 3: a made skill's yes not taken: its say-back was not heard whole (kept: r1 cut=false 10 words; r2 cut=true 9 words; first missing word in r1: 10 of 14)"
        );
        // No word of the say-back reaches the journal: places and counts only.
        let line_words = crate::services::check_words(&line);
        for word in crate::services::check_words(SAY_BACK) {
            if word.len() > 2 {
                assert!(!line_words.contains(&word), "{word} in {line}");
            }
        }
        // Held whole but cut: said so.
        lamp_said(3, SAY_BACK);
        lamp_cut(3);
        assert!(
            not_said_whole_line(&waiting(), 4).ends_with("r3 held it all but was cut)"),
            "{}",
            not_said_whole_line(&waiting(), 4)
        );
        assert_eq!(
            approve(&id, 4, true, true, now, never)["error"],
            NOT_SAID_WHOLE
        );
        assert_eq!(
            skipped_line(5, "no scope").as_deref(),
            Some(
                "lamp-voice: request 5: the lamp's words skip the thread (no scope) while a skill's yes waits (made in request 1): they don't count toward its say-back"
            )
        );
    }

    /// The say-back's own last words heard back within the tail are held
    /// (`own_echo::yes_echo`) and their request kept: an approval in it is
    /// refused, and so is one in the make's own request, a turn nobody
    /// spoke, another id, one too late, or one on the Live route.
    #[test]
    fn an_echo_or_an_unheard_or_stale_yes_approves_nothing() {
        let now = Instant::now();
        for (request, heard, sends, at, id_ok) in [
            (5, true, true, now, true),
            (3, true, true, now, true),
            (6, false, true, now, true),
            (6, true, true, now, false),
            (
                6,
                true,
                true,
                now + YES_WITHIN + Duration::from_secs(1),
                true,
            ),
        ] {
            let id = said_back("leaving-mail", &[mail()], 3, now);
            echo_held(5);
            let id = if id_ok { id } else { "yes-other".into() };
            let told = approve(&id, request, heard, sends, at, never);
            assert_eq!(told["error"], NOT_APPROVED, "{request} {heard}");
        }
        let id = said_back("leaving-mail", &[mail()], 3, now);
        assert_eq!(approve(&id, 4, true, false, now, never)["error"], NOT_HERE);
        // A refusal by lamp-services is told, nothing approved.
        let id = said_back("leaving-mail", &[mail()], 3, now);
        let told = approve(&id, 4, true, true, now, |_| {
            Err("the skill on the lamp is not the one said back: nothing was approved".into())
        });
        assert!(
            told["error"]
                .as_str()
                .unwrap()
                .contains("not the one said back")
        );
    }

    /// A decline or a change calls nothing: the wait lapses, and a new make
    /// replaces it, its old id approving nothing.
    #[test]
    fn a_decline_or_a_change_approves_nothing() {
        let now = Instant::now();
        let old = said_back("leaving-mail", &[mail()], 3, now);
        assert!(!awaited(now + YES_WITHIN + Duration::from_secs(1)));
        let _changed = said_back("leaving-mail-2", &[mail()], 5, now);
        let told = approve(&old, 6, true, true, now, never);
        assert_eq!(told["error"], NOT_APPROVED);
    }

    /// A skill's task said back, then a line the model judges a new ask
    /// (`new_ask`), not an answer to it: the wait ends with nothing agreed,
    /// so no later yes takes it, and the model answers the new ask.
    #[test]
    fn a_new_ask_ends_a_tasks_wait_agreeing_nothing() {
        clear_for_test();
        let now = Instant::now();
        let id = wait_for_yes(
            Asks {
                skill: "photo-booth".into(),
                sha256: "a".repeat(64),
                writes: Vec::new(),
                say_back: "It uses the camera.".into(),
                grants: Some(vec!["camera".into()]),
                code: false,
                remove: false,
            },
            1,
            now,
        );
        lamp_said(1, "It uses the camera.");
        assert!(approve_forced_now(now));
        let told = answered(&id, 2, Some("new_ask"), None).unwrap();
        assert_eq!(told["approved"], false, "{told}");
        assert_eq!(told["left"], "photo-booth", "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("photo booth"), "{next}");
        assert!(next.contains("what they asked"), "{next}");
        assert!(!next.to_lowercase().contains("call nothing"), "{next}");
        assert!(!awaited(now));
        assert_eq!(
            approve(&id, 3, true, true, now, never)["error"],
            NOT_APPROVED
        );
    }

    /// The first builder's ruling on #7683 (18 Nov, 12:51): a remove is
    /// confirmed only by a plain yes the code reads, never a question or a
    /// line asking anything else, whatever the model answered. Each line
    /// as heard, read alone.
    #[test]
    fn only_a_plain_yes_the_code_reads_confirms_a_remove() {
        let skill = ["rain-joke".to_owned()];
        let plain = |line: &str| plain_answer(line, YES_SAID, REMOVE_ALSO, &skill);
        for line in [
            "Hey Lamp, yes, remove it",
            "yeah, do it",
            "Yes please",
            "ok",
            "OK.",
            "Okay, go ahead.",
            "Lamp, yes.",
            "Yep!",
            "Sure.",
            "Yes, remove rain joke.",
            "Yes, remove the rain joke skill.",
            "Please delete it",
            "Vâng.",
            "Ừ, xoá đi.",
            "Lamp ơi, xóa nó đi",
        ] {
            assert!(plain(line), "{line}");
        }
        for line in [
            "What's the time?",
            "Is it the moon one?",
            "Sure, and make me a skill that tells a joke at noon.",
            "Hey Lamp, what?",
            "Hey Lamp, what",
            "Yes?",
            "Sure, play some music.",
            "Yes, play music.",
            "Lamp, also make one that plays a soft chime when I come back to my desk.",
            "Which of my skills use the internet",
            "Yes, remove the evening dim skill.",
            "Remove it later.",
            "Do",
            "please",
            "Hey Lamp",
            "",
            "No.",
            "Không.",
        ] {
            assert!(!plain(line), "{line}");
        }
    }

    /// The same reader, held to the line right after the question: an
    /// unclear answer asks again, and the line after that is the answer
    /// line; a second line that is more than the name drops the wait; a
    /// round that calls anything else beside the answer takes no remove.
    #[test]
    fn a_removes_yes_counts_only_as_a_plain_yes_on_the_next_line() {
        let rain = || Asks {
            skill: "rain-joke".into(),
            sha256: "b".repeat(64),
            writes: Vec::new(),
            say_back: "Remove rain joke?".into(),
            grants: None,
            code: false,
            remove: true,
        };
        clear_for_test();
        let now = Instant::now();
        // An unclear answer on the next line, then a plain yes on the line
        // after the question asked again: taken.
        let id = wait_for_yes(rain(), 1, now);
        lamp_said(1, "Remove rain joke?");
        line_heard(2, false, "Hmm, maybe.");
        assert!(answered(&id, 2, Some("unclear"), Some(NAMED_NONE)).is_some());
        line_heard(3, true, "Lamp");
        line_heard(4, false, "Yes.");
        assert_eq!(answered(&id, 4, Some("yes"), Some(NAMED_NONE)), None);
        // A question, the model's yes all the same: left, the wait dropped.
        let id = wait_for_yes(rain(), 5, now);
        line_heard(6, false, "Is it the moon one?");
        let told = answered(&id, 6, Some("yes"), Some(NAMED_NONE)).unwrap();
        assert_eq!(told["left"], "rain-joke", "{told}");
        let next = told["next"].as_str().unwrap();
        assert!(next.contains("left rain joke as it is"), "{next}");
        assert!(!waits(&id));
        // A round of a plain "Yes." that calls a play beside the answer:
        // left.
        let id = wait_for_yes(rain(), 7, now);
        line_heard(8, false, "Yes.");
        let answer = json!({ APPROVE_ID: id });
        let play = json!({"query": "music"});
        round_called(8, [(crate::made_skill::MAKE, &answer)]);
        assert!(waits(&id), "the answer alone marks nothing");
        round_called(
            8,
            [(crate::made_skill::MAKE, &answer), ("play_music", &play)],
        );
        let told = answered(&id, 8, Some("yes"), Some(NAMED_NONE)).unwrap();
        assert_eq!(told["left"], "rain-joke", "{told}");
        // A second line drops it; a yes in no line heard is no yes.
        let id = wait_for_yes(rain(), 9, now);
        line_heard(10, false, "What time is it?");
        line_heard(11, false, "Yes.");
        assert!(!waits(&id));
        let id = wait_for_yes(rain(), 12, now);
        assert!(answered(&id, 13, Some("yes"), Some(NAMED_NONE)).is_some());
        assert!(!waits(&id));
        // Another wait's (a task's) yes is never held to it.
        let id = wait_for_yes(
            Asks {
                remove: false,
                grants: Some(vec!["camera".into()]),
                ..rain()
            },
            14,
            now,
        );
        line_heard(15, false, "Sure, and what does it do with the camera");
        assert_eq!(answered(&id, 15, Some("yes"), Some(NAMED_NONE)), None);
        clear_for_test();
    }

    /// Twelve's P2 on #7683: a "Yes." said while "Remove rain joke?" still
    /// plays is told to hear the question again, and the "Yes." to the
    /// question said again removes it: the re-ask clears the answer line,
    /// as an unclear answer does.
    #[test]
    fn a_yes_over_the_question_is_asked_again_and_the_next_yes_removes() {
        clear_for_test();
        let now = Instant::now();
        let id = wait_for_yes(
            Asks {
                skill: "rain-joke".into(),
                sha256: "b".repeat(64),
                writes: Vec::new(),
                say_back: "Remove rain joke?".into(),
                grants: None,
                code: false,
                remove: true,
            },
            1,
            now,
        );
        lamp_said(1, "Remove rain");
        lamp_cut(1);
        line_heard(2, false, "Yes.");
        assert_eq!(answered(&id, 2, Some("yes"), Some(NAMED_NONE)), None);
        let again = approve(&id, 2, true, true, now, |_| panic!("removed"));
        assert!(
            again["next"]
                .as_str()
                .unwrap()
                .contains("Remove rain joke?"),
            "{again}"
        );
        assert!(waits(&id));
        lamp_said(2, "Remove rain joke?");
        line_heard(3, false, "Yes.");
        assert_eq!(answered(&id, 3, Some("yes"), Some(NAMED_NONE)), None);
        let mut removed = false;
        let told = approve(&id, 3, true, true, now, |asks| {
            removed = true;
            Ok(json!({ "removed": asks.skill }))
        });
        assert!(removed, "{told}");
        assert_eq!(told["approved"], true, "{told}");
        clear_for_test();
    }

    /// #7561's row on lamp-9adf (17 Nov 23:4x): "Remove rain joke?", then
    /// "Yes, remove rain joke." was answered `new_ask` and the wait was
    /// dropped. A line naming the very skill asked about answers it: the
    /// note tells the model so, with that skill's name to copy into
    /// `named`, and a yes naming it, in any case, with hyphens,
    /// underscores or spaces, is its yes. Another skill's name is still a
    /// new ask.
    #[test]
    fn a_yes_naming_the_asked_skill_is_its_yes_never_a_new_ask() {
        let rain = || Asks {
            skill: "rain-joke".into(),
            sha256: "b".repeat(64),
            writes: Vec::new(),
            say_back: "Remove rain joke?".into(),
            grants: None,
            code: false,
            remove: true,
        };
        clear_for_test();
        let now = Instant::now();
        let id = wait_for_yes(rain(), 1, now);
        lamp_said(1, "Remove rain joke?");
        line_heard(2, false, "Yes, remove rain joke.");
        let note = waiting_note(2, now).unwrap();
        assert!(note.contains("named \"rain joke\""), "{note}");
        assert!(note.contains("never new_ask"), "{note}");
        for named in ["rain joke", "Rain-Joke", "rain_joke", "RAIN JOKE skill"] {
            assert_eq!(answered(&id, 2, Some("yes"), Some(named)), None, "{named}");
            assert!(waits(&id), "{named}");
        }
        let told = answered(&id, 2, Some("yes"), Some("cloud joke")).unwrap();
        assert_eq!(told["left"], "rain-joke", "{told}");
        assert!(!waits(&id));
        // A task's or an app's make's forced wait is told the same.
        let id = wait_for_yes(
            Asks {
                remove: false,
                grants: Some(vec!["camera".into()]),
                skill: "photo-booth".into(),
                say_back: "It uses the camera.".into(),
                ..rain()
            },
            1,
            now,
        );
        lamp_said(1, "It uses the camera.");
        let note = waiting_note(2, now).unwrap();
        assert!(
            note.contains("named too: \"photo booth\" if their line names this skill"),
            "{note}"
        );
        assert!(note.contains("never new_ask"), "{note}");
        assert!(waits(&id));
        clear_for_test();
    }

    /// #7634: what a heard line names before "skill", read against the
    /// waiting skill by the made-skill name match.
    #[test]
    fn a_line_names_another_skill_only_by_a_name_said_with_skill() {
        let other = |heard| other_skill_named(heard, "page-title", &[]);
        let on_lamp = |heard, installed: &[&str]| {
            let installed: Vec<String> = installed.iter().map(|name| name.to_string()).collect();
            other_skill_named(heard, "page-title", &installed)
        };
        assert_eq!(
            other("Lamp, use my look title skill."),
            Some("look title".into())
        );
        assert_eq!(
            other("Yes, and use my weather skill."),
            Some("weather".into())
        );
        assert_eq!(other("yes run joke time skill"), Some("joke time".into()));
        assert_eq!(
            other("Yes, and use my weather skills."),
            Some("weather".into())
        );
        // A name after "skill" is another's only as an installed skill's.
        let look = ["look-title", "page-title"];
        assert_eq!(
            on_lamp("run skill look title", &look),
            Some("look title".into())
        );
        assert_eq!(
            on_lamp("use the skill look title please", &look),
            Some("look title".into())
        );
        // The longest installed name wins over the waiting one inside it
        // (the re-read's P1 on #7634).
        let backup = ["page-title", "page-title-backup"];
        assert_eq!(
            on_lamp("yes, use skill page title backup", &backup),
            Some("page title backup".into())
        );
        assert_eq!(
            on_lamp("yes, use my page title backup skill", &backup),
            Some("page title backup".into())
        );
        assert_eq!(on_lamp("yes, use my page title skill", &backup), None);
        // With the lamp's skills read, only an installed skill's name is
        // another's (the pre-push read's P2s on #7634): describing words
        // before the waiting skill's name, words in another order, and
        // Vietnamese words around "skill" take nothing from the yes.
        let read = |heard, skill, installed: &[&str]| {
            let installed: Vec<String> = installed.iter().map(|name| name.to_string()).collect();
            other_skill_named(heard, skill, &installed)
        };
        let weather = ["weather", "timer", "page-title"];
        for (heard, skill, installed) in [
            ("Yes, run my new weather skill", "weather", &weather[..]),
            ("the daily weather skill", "weather", &weather[..]),
            (
                "yes, the weather skill",
                "weather-report",
                &["weather-report", "timer"][..],
            ),
            ("Ừ, chạy cái skill đó đi", "page-title", &backup[..]),
            ("Có, cái skill đó", "page-title", &backup[..]),
            ("Vâng, em chạy skill đi", "page-title", &backup[..]),
            ("Yes, great skill.", "page-title", &backup[..]),
        ] {
            assert_eq!(read(heard, skill, installed), None, "{heard}");
        }
        // Safety first (the coordinator's ruling on #7634): any other word
        // before "skill" names another skill, listed or not.
        for (heard, skill, installed, named) in [
            ("okay timer skill", "weather", &weather[..], "timer"),
            (
                "Lamp, use my look title skill.",
                "page-title",
                &backup[..],
                "look title",
            ),
            (
                "yes, use my backup page title skill",
                "page-title",
                &backup[..],
                "backup page title",
            ),
            ("Yes, awesome skill.", "page-title", &backup[..], "awesome"),
        ] {
            assert_eq!(read(heard, skill, installed), Some(named.into()), "{heard}");
        }
        assert_eq!(
            read("Lamp, use my look title skill.", "page-title", &look),
            Some("look title".into())
        );
        // With none read, the same, and the words after "skill" fail closed.
        for heard in ["Yes, run my new weather skill", "the daily weather skill"] {
            assert_eq!(other_skill_named(heard, "weather", &[]), None, "{heard}");
        }
        assert_eq!(other("Ừ, chạy cái skill đó đi"), Some("đó đi".into()));
        // A yes said without its comma still bounds another skill's name.
        assert_eq!(
            other_skill_named("okay timer skill", "weather", &[]),
            Some("timer".into())
        );
        assert_eq!(
            other_skill_named("yeah đèn ngủ skill", "weather", &[]),
            Some("đèn ngủ".into())
        );
        // A word after "skill" that names no installed skill is no name
        // (the re-read's P2 on #7634), but with no skill on the lamp read,
        // it fails closed (the final read's P3).
        for after in [
            "yes the skill thanks",
            "yes, run the skill again",
            "Có, chạy skill đi",
            "run skill look title",
        ] {
            assert_eq!(on_lamp(after, &backup), None, "{after}");
            assert!(other(after).is_some(), "{after}");
        }
        // A waiting skill's own name may hold the words that bound
        // another's: only a run's ends are bounded.
        for (heard, skill) in [
            ("yes, use my lamp test email two skill", "lamp-test-email-2"),
            ("yes, run the play time skill", "play-time"),
            ("yes, use my start the day skill", "start-the-day"),
            ("yes, the what now skill", "what-now"),
            ("yes, use my my diary skill", "my-diary"),
            ("yes, use skill lamp test email two", "lamp-test-email-2"),
            ("yes, use my page title backup skill", "page-title-backup"),
            // A yes said without its comma (the adversarial read's P2 on
            // #7634), and a Vietnamese line against the name as a made
            // skill's is written, without marks.
            ("okay page title skill", "page-title"),
            ("yeah weather skill", "weather"),
            ("go ahead with weather skill", "weather"),
            ("sure page title skill", "page-title"),
            ("please page title skill", "page-title"),
            ("ừ skill thời tiết", "thoi-tiet"),
            ("ừ đèn ngủ skill", "den-ngu"),
            ("vâng, dùng skill đèn ngủ", "den-ngu"),
            ("được đèn ngủ skill", "den-ngu"),
        ] {
            assert_eq!(other_skill_named(heard, skill, &[]), None, "{heard}");
            let installed = vec![skill.to_owned(), "page-title".to_owned()];
            assert_eq!(other_skill_named(heard, skill, &installed), None, "{heard}");
        }
    }

    /// #7634's scope: the heard line is read for another skill on a skill's
    /// task's wait alone ([`task_wait`]). A remove's wait keeps its own
    /// rule (#7683): "Yes, and remove the joke time skill too." after
    /// "Remove moon fact?" is not a plain yes, so nothing is removed and the
    /// lamp says so (`remove_not_plain_next`), never #7634's new ask.
    #[test]
    fn a_removes_wait_keeps_its_own_rule_for_a_line_naming_another_skill() {
        clear_for_test();
        let now = Instant::now();
        let id = wait_for_yes(
            Asks {
                skill: "moon-fact".into(),
                sha256: "b".repeat(64),
                writes: Vec::new(),
                say_back: "Remove moon fact?".into(),
                grants: None,
                code: false,
                remove: true,
            },
            1,
            now,
        );
        lamp_said(1, "Remove moon fact?");
        line_heard(2, false, "Yes, and remove the joke time skill too.");
        let told = answered(&id, 2, Some("yes"), Some("moon fact")).unwrap();
        assert_eq!(told["next"], remove_not_plain_next("moon fact"), "{told}");
        clear_for_test();
    }

    /// #7545 on a task's wait too: photo booth's task said back, then a
    /// line the model labels `yes` but whose named skill is another
    /// (weather): bound to its skill, that yes agrees to nothing for photo
    /// booth, ends the wait as a new ask, and no later yes takes it. Its
    /// own name, or none, is its yes.
    #[test]
    fn a_yes_naming_another_skill_agrees_nothing_on_a_tasks_wait() {
        let booth = || Asks {
            skill: "photo-booth".into(),
            sha256: "a".repeat(64),
            writes: Vec::new(),
            say_back: "It uses the camera.".into(),
            grants: Some(vec!["camera".into()]),
            code: false,
            remove: false,
        };
        clear_for_test();
        let now = Instant::now();
        let id = wait_for_yes(booth(), 1, now);
        lamp_said(1, "It uses the camera.");
        let told = answered(&id, 2, Some("yes"), Some("weather")).unwrap();
        assert_eq!(told["approved"], false, "{told}");
        assert_eq!(told["left"], "photo-booth", "{told}");
        assert!(!awaited(now));
        assert_eq!(
            approve(&id, 3, true, true, now, never)["error"],
            NOT_APPROVED
        );
        for named in ["photo booth", NAMED_NONE] {
            clear_for_test();
            let id = wait_for_yes(booth(), 1, now);
            lamp_said(1, "It uses the camera.");
            assert_eq!(answered(&id, 2, Some("yes"), Some(named)), None, "{named}");
        }
        clear_for_test();
    }

    /// Thirteen's HOLD on #6223 (App Only confirming): the app has no yes
    /// for a made skill's writes (lamp-cloud has no kind for
    /// `skill_write_approve`, the app nothing), so no line the voice's
    /// model is told of a skill's writes, here, in the made-skill lines or
    /// in the read-back's, names the app; and no line outside the tests
    /// of those files sends the person to the app for the yes.
    #[test]
    fn no_line_sends_the_person_to_the_app_for_a_yes() {
        let lines = [
            NOT_APPROVED.to_owned(),
            NOT_SAID_WHOLE.to_owned(),
            NOT_HERE.to_owned(),
            APPROVED_NEXT.to_owned(),
            then_wait("ab12"),
            say_whole("I made leaving mail."),
            crate::made_skill::WAITS_FOR_YES.to_owned(),
            crate::made_skill::APPROVED_ALREADY.to_owned(),
            crate::made_skill::CUT_WRITE_UNSAID.to_owned(),
            crate::make_skill_local::READ_BACK_TOLD.to_owned(),
            crate::make_skill_local::READ_BACK_NOT_ASKED.to_owned(),
            crate::make_skill_local::sends_unsaid("leaving mail"),
            crate::make_skill_local::yes_not_here("leaving mail"),
            crate::make_skill_local::changed_since_made("leaving mail"),
            crate::skill_task_yes::then_wait("ab12"),
            crate::skill_task_yes::APPROVED_NEXT.to_owned(),
            CODE_APPROVED_NEXT.to_owned(),
            CODE_ON_TASK_WAITS_NEXT.to_owned(),
            crate::skill_task_yes::yes_not_here("photo booth"),
            crate::skill_task_yes::unread("photo booth"),
            crate::make_skill_local::TASKS_READ_BACK_TOLD.to_owned(),
            crate::make_skill_local::BOTH_READ_BACK_TOLD.to_owned(),
            crate::make_skill_local::REMOVE_TOLD.to_owned(),
            REMOVE_NOT_TAKEN.to_owned(),
            REMOVE_NOT_HERE.to_owned(),
            removed_next("moon fact"),
            remove_declined_next("moon fact"),
            remove_unclear_next("moon fact"),
            remove_left_next("moon fact"),
            new_ask_next("photo booth"),
        ];
        for line in &lines {
            let words: Vec<String> = line
                .split(|c: char| !c.is_alphanumeric() && c != '\'')
                .map(str::to_lowercase)
                .collect();
            assert!(
                !words.iter().any(|word| word == "app" || word == "app's"),
                "{line}"
            );
        }
        // The way to a yes now: asking what the skill sends; on the Live
        // route, which can't take one, the lamp's other voice, never
        // asking again there (App Only's P3 on #6250).
        assert!(
            NOT_APPROVED.contains("what does my … skill send?"),
            "{NOT_APPROVED}"
        );
        assert!(!NOT_HERE.contains("what does my"), "{NOT_HERE}");
        for words in ["nothing was sent", "my other voice", "not this live one"] {
            assert!(NOT_HERE.contains(words), "{NOT_HERE}");
        }
        let app_yes = [
            "yes in the app",
            "app's yes",
            "yes from the app",
            "agree in the app",
            "approve in the app",
            "approve it in the app",
        ];
        for (file, source) in [
            ("skill_write_yes.rs", include_str!("skill_write_yes.rs")),
            ("made_skill.rs", include_str!("made_skill.rs")),
            ("make_skill_local.rs", include_str!("make_skill_local.rs")),
            ("skill_task_yes.rs", include_str!("skill_task_yes.rs")),
        ] {
            let code = source.split("mod tests {").next().unwrap();
            for (number, line) in code.lines().enumerate() {
                let line = line.to_lowercase();
                assert!(
                    !app_yes.iter().any(|said| line.contains(said)),
                    "{file}:{}: {line}",
                    number + 1
                );
            }
        }
    }
}
