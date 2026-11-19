//! A voice make asked while another is made waits its turn (#7679,
//! `LAMP_MAKE_QUEUE`, exactly `on`; off, a second make is refused as
//! before, byte for byte).
//!
//! On lamp-9adf (stock 14083, Fourteen's run) a person made one skill, then
//! 47 s later asked for another ("check the gold price every evening at
//! six"), and heard "The lamp's agent is busy; ask me again in a minute."
//! (`made_skill::AGENT_BUSY_TRY`, lamp-tasks' `busy`). The lamp handed its
//! own queue to the person, and the second ask was lost unless they
//! remembered it.
//!
//! With the switch on, the second route's worker makes one voice make at a
//! time, in the order asked:
//!
//! - **Idle:** a make runs at once, in its own turn, as today, and holds the
//!   one slot ([`admit`]).
//! - **Busy:** a new make waits, at most [`MOST_WAITING`] of them, first in
//!   first out, and the model is told at once that it is queued
//!   ([`queued_told`]), so it says one line in the conversation's language
//!   ("I'll make the gold one next"). The enqueue is one lock and a push;
//!   who asked is kept with it ([`Asker`]: the person, the owner and the
//!   line heard, so a busy room can't push that line out of the
//!   thread's history before it runs). A make past that bound gets
//!   [`QUEUE_FULL_LINE`], honest about the wait.
//! - **Running:** the slot is held until the make's line was given to its
//!   turn or said ([`said`]) and the say-back's yes wait
//!   (`skill_write_yes`, which holds one wait and would overwrite it) is
//!   answered, cleared or past `skill_write_yes::YES_WITHIN`. Then the
//!   front make runs with no turn, its line said through the say service
//!   as a cut make's is, so its yes is forced as usual ([`step`]). A queued
//!   make says no waiting line: nobody waits on its turn.
//! - **Cancel:** while any wait, each later line's note lists their labels
//!   and ids ([`note`]); `make_skill` with [`CANCEL`] alone drops that one
//!   ([`cancel`]), only for whoever asked for it or the lamp's owner.
//! - **Dropped:** a make that waited past [`WAITS_AT_MOST`], or still
//!   waiting when the quiet hours begin, is told as not made
//!   ([`QUEUE_DROPPED_LINE`]) and never made later. Across a voice restart
//!   only the labels are kept ([`QUEUE_FILE`]), read once at start, said
//!   and removed ([`told_after_restart`]).
//!
//! lamp-tasks stays one at a time (`skill_make`'s `busy`) as the backstop.
//! A make behind a Harness or one-session task is #7595's, not this one's.
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// The switch: exactly `on`; anything else, today's refusal.
pub const ENVIRONMENT: &str = "LAMP_MAKE_QUEUE";
/// How many makes may wait behind the one being made.
pub const MOST_WAITING: usize = 2;
/// How long a make may wait for its turn before it is told as not made.
pub const WAITS_AT_MOST: Duration = Duration::from_secs(6 * 60);
/// How long the slot is held at most, whatever its make and yes do: a make
/// that never ended (a thread lost) never holds the queue for good.
pub const RUNS_AT_MOST: Duration = Duration::from_secs(10 * 60);
/// Where the labels of the makes still waiting are kept for a voice
/// restart: the voice's own runtime folder (`RuntimeDirectory=lampos-voice`,
/// 0700, `RuntimeDirectoryPreserve=yes`), as `skill_write_yes`'s
/// `CODE_AWAITED_FILE`. Labels only, never a request's words.
pub const QUEUE_FILE: &str = "/run/lampos-voice/make-queue.json";
/// The `make_skill` argument that drops a waiting make, by its id. Not
/// `cancel`: a clash's `choice` already takes the value `"cancel"`
/// (`made_clash::CHOICES`), and one word for two things misleads the model.
pub const CANCEL: &str = "cancel_waiting";
/// The `make_skill` argument with `request`: the model's two to four words
/// for the skill, said in the queued line and listed in the note.
pub const LABEL: &str = "label";
/// The longest label kept, in characters.
const LABEL_MOST: usize = 40;
/// How often the queue is looked at for a slot freed or a wait run out.
const STEP_EVERY: Duration = Duration::from_millis(500);
/// How long after a voice restart the makes it dropped wait at most for the
/// say service to answer ([`ready_within`]): it starts beside the worker.
pub(crate) const SAY_READY_WITHIN: Duration = Duration::from_secs(30);
/// How often the say service is tried while it isn't there yet.
pub(crate) const SAY_READY_EVERY: Duration = Duration::from_millis(250);

/// A make past [`MOST_WAITING`]: what the person hears, honest about the
/// wait (in each language, `language::FIXED_LINES`).
pub const QUEUE_FULL_LINE: &str = "Two skills are already waiting for me to make them, so I can't take another one yet. Ask me again once they're done.";
/// A waiting make dropped (past [`WAITS_AT_MOST`], the quiet hours, or a
/// restart): what the person hears, followed by the labels
/// ([`dropped_line`]).
pub const QUEUE_DROPPED_LINE: &str = "I didn't get to make what you asked for while I was busy, so it wasn't made. Ask me again if you still want it:";

/// A waiting make's work: the make itself, with no turn and no scope (the
/// turn that asked for it is over).
pub(crate) type Work = Box<dyn FnOnce() -> crate::tools::ToolAnswer + Send>;

/// Whether the switch is on: [`ENVIRONMENT`] exactly `on`.
pub fn on() -> bool {
    on_with(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Whether `value` turns the switch on: `on`, and nothing else.
fn on_with(value: Option<&str>) -> bool {
    value == Some("on")
}

/// A make waiting its turn.
pub struct Entry {
    /// What [`CANCEL`] names it by: `q1`, `q2`, … in this process.
    pub id: String,
    /// The request it was asked in.
    pub request: u64,
    /// Who asked, and the line heard for it, kept at the ask ([`Asker`]).
    pub asker: Asker,
    /// The model's two to four words for it ([`label_of`]).
    pub label: String,
    /// When it was asked.
    pub at: Instant,
    work: Work,
}

/// Who asked for a waiting make, kept at the ask: by the time it runs, its
/// turn and memory scope are over, and a busy room may have pushed its line
/// out of the thread's twelve exchanges.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Asker {
    /// Whose line it was (the line's person in the memory scope); none for
    /// a line no one known said.
    pub speaker: Option<String>,
    /// The lamp's owner whose line it was (`services::owner_of`), none for
    /// anyone else's: a make with code of its own is put on, or replaces
    /// the owner's skill it clashes with, only on the owner's ask
    /// (`LAMP_MAKE_NO_ASKS`), and by the time it runs its scope is gone
    /// (`make_skill_local::with_asker_kept`).
    pub owner: Option<String>,
    /// The line heard for it (`Thread::heard_in_own_line`, else
    /// `heard_for`), which the make is then held to
    /// (`make_skill_local::with_asker_kept`).
    pub heard: Option<String>,
}

impl Entry {
    /// The make run now, on this thread, with the line heard and the owner
    /// read at its ask. One that failed with no line of its own to say (a
    /// brief refused, an unclear ask with `LAMP_MAKE_NO_ASKS`, the part not
    /// running) leaves its [`failed_line`] to be said: no turn tells the
    /// model of it, and the person was told it comes next.
    pub(crate) fn run(self) -> crate::tools::ToolAnswer {
        let (work, label) = (self.work, self.label);
        let answer =
            crate::make_skill_local::with_asker_kept(self.asker.heard, self.asker.owner, work);
        crate::make_skill_local::failed_unless_said(|| failed_line(&label, &answer.told));
        answer
    }
}

/// The one make under way, and its request.
struct Slot {
    request: u64,
    /// Its line given to its turn, said, or nothing to say ([`said`]).
    said: bool,
    at: Instant,
}

/// The queue: the make under way, and those waiting behind it.
pub struct Queue {
    running: Option<Slot>,
    waiting: VecDeque<Entry>,
    /// The last id given.
    next: u32,
    /// The waiting labels changed since they were last kept ([`QUEUE_FILE`]).
    changed: bool,
}

impl Queue {
    const EMPTY: Self = Self {
        running: None,
        waiting: VecDeque::new(),
        next: 0,
        changed: false,
    };
}

#[cfg(not(test))]
mod kept {
    use super::Queue;
    use std::sync::Mutex;
    static QUEUE: Mutex<Queue> = Mutex::new(Queue::EMPTY);
    pub fn queue<R>(f: impl FnOnce(&mut Queue) -> R) -> R {
        f(&mut QUEUE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()))
    }
}

/// Tests: each test thread has its own.
#[cfg(test)]
mod kept {
    use super::Queue;
    use std::cell::RefCell;
    thread_local! {
        static QUEUE: RefCell<Queue> = const { RefCell::new(Queue::EMPTY) };
    }
    pub fn queue<R>(f: impl FnOnce(&mut Queue) -> R) -> R {
        QUEUE.with(|cell| f(&mut cell.borrow_mut()))
    }
}

/// Whether a `make_skill` call's `args` is a new make: a `request` with
/// words, and none of the function's other uses (a yes, a clash's choice,
/// a remove, a turn-off or -on, a list, a run, a read-back, a cancel).
/// Only a new make waits its turn; every other use runs as today.
pub fn is_new_make(args: &Value) -> bool {
    let other = [
        crate::skill_write_yes::APPROVE_ID,
        crate::made_clash::CLASH_ID,
        crate::made_clash::CHOICE,
        crate::made_remove::REMOVE,
        crate::made_turn::TURN_OFF,
        crate::made_turn::TURN_ON,
        crate::made_list::LIST,
        crate::made_run::RUN,
        crate::make_skill_local::READ_BACK,
        CANCEL,
    ];
    args["request"]
        .as_str()
        .is_some_and(|request| !request.trim().is_empty())
        && other
            .iter()
            .all(|key| args.get(key).is_none_or(Value::is_null))
}

/// Whether a `make_skill` call's `args` drops a waiting make: its id.
pub fn cancel_of(args: &Value) -> Option<&str> {
    args[CANCEL].as_str().map(str::trim)
}

/// The label kept for a make: the model's own words (`label`), else the
/// request's first four words; one line, no quotes, at most [`LABEL_MOST`]
/// characters.
pub fn label_of(args: &Value) -> String {
    let words = |text: &str, most: usize| -> String {
        text.split_whitespace()
            .take(most)
            .collect::<Vec<_>>()
            .join(" ")
    };
    let label = args[LABEL]
        .as_str()
        .map(|label| words(label, 8))
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| words(args["request"].as_str().unwrap_or_default(), 4));
    label
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '"' | '“' | '”' | '\\'))
        .take(LABEL_MOST)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// What became of a new make at [`admit`].
pub enum Admitted {
    /// Nothing under way and none waiting: it runs now, in its own turn,
    /// and holds the slot until [`said`] and its yes.
    Run,
    /// Another is under way: it waits, at `place` (1 is next), by `id`.
    Queued { place: usize, id: String },
    /// [`MOST_WAITING`] already wait: refused, with [`full_told`].
    Full,
}

/// A new make of `request`, asked by `asker` at `now`: run now, queued
/// with its `work` for later, or refused as full. One lock and a push; no
/// I/O, so the person hears at once that it waits.
pub(crate) fn admit(
    request: u64,
    asker: Asker,
    label: String,
    now: Instant,
    work: Work,
) -> Admitted {
    kept::queue(|queue| {
        if queue.running.is_none() && queue.waiting.is_empty() {
            queue.running = Some(Slot {
                request,
                said: false,
                at: now,
            });
            return Admitted::Run;
        }
        if queue.waiting.len() >= MOST_WAITING {
            return Admitted::Full;
        }
        queue.next += 1;
        let id = format!("q{}", queue.next);
        queue.waiting.push_back(Entry {
            id: id.clone(),
            request,
            asker,
            label,
            at: now,
            work,
        });
        queue.changed = true;
        Admitted::Queued {
            place: queue.waiting.len(),
            id,
        }
    })
}

/// Whether a make is under way or waits: a new one would wait, so who asked
/// is read for it ([`Asker`]). One lock; an idle make reads nothing more.
pub fn busy() -> bool {
    kept::queue(|queue| queue.running.is_some() || !queue.waiting.is_empty())
}

/// The make of `request` under way gave its line to its turn, said it, or
/// had nothing to say: once its yes wait is over the next may run.
pub fn said(request: u64) {
    kept::queue(|queue| {
        if let Some(slot) = queue
            .running
            .as_mut()
            .filter(|slot| slot.request == request)
        {
            slot.said = true;
        }
    });
}

/// What the model is told of a make queued at `place` as `label`: not made
/// yet, and one short line to say, in the conversation's language.
pub fn queued_told(place: usize, id: &str, label: &str) -> Value {
    let when = if place == 1 {
        "next"
    } else {
        "right after the one waiting before it"
    };
    json!({
        "ok": true,
        "queued": true,
        "place": place,
        "id": id,
        "note": format!(
            "Not made yet: another skill is being made now, so this one waits its turn \
             (place {place}). It is made and said back on its own once the one before it is \
             done, with no call of yours. Say only one short line, in the conversation's \
             language, that you'll make {label} {when}. Ask nothing, never say it is made, and \
             call nothing else for it."
        )
    })
}

/// What the model is told of a make past [`MOST_WAITING`]: not made, and
/// [`QUEUE_FULL_LINE`] in the owner's language to say.
pub fn full_told() -> Value {
    json!({
        "ok": false,
        "error": "queue_full",
        "next": crate::made_skill::not_made_in_their_language(crate::language::fixed_in(
            crate::language::at_start(),
            QUEUE_FULL_LINE
        ))
    })
}

/// The note each later line is given while makes wait: their labels and
/// ids, in turn, and how to drop one. None while none wait (always, with
/// the switch off).
pub fn note() -> Option<String> {
    kept::queue(|queue| {
        if queue.waiting.is_empty() {
            return None;
        }
        let waiting = queue
            .waiting
            .iter()
            .map(|entry| format!("\"{}\" ({CANCEL} \"{}\")", entry.label, entry.id))
            .collect::<Vec<_>>()
            .join(", ");
        Some(format!(
            "(For you to know, not to answer: waiting to be made after the skill being made now, \
             in turn: {waiting}. Each is made and said back on its own, with no call of yours, so \
             never ask for one of them again. If the person drops one (\"never mind the gold \
             one\"), call make_skill with only {CANCEL} set to its id.)"
        ))
    })
}

/// A line by `speaker` (the lamp's owner or not, `owner`) dropping the
/// waiting make `id`: dropped only for whoever asked for it or the owner;
/// what the model is told.
pub fn cancel(id: &str, speaker: Option<&str>, owner: bool) -> Value {
    let found = kept::queue(|queue| {
        let at = queue.waiting.iter().position(|entry| entry.id == id)?;
        let entry = &queue.waiting[at];
        let theirs = speaker.is_some_and(|speaker| entry.asker.speaker.as_deref() == Some(speaker));
        if !(theirs || owner) {
            return Some(Err(entry.label.clone()));
        }
        let entry = queue.waiting.remove(at)?;
        queue.changed = true;
        Some(Ok((entry.request, entry.label)))
    });
    match found {
        Some(Ok((request, label))) => {
            eprintln!(
                "lamp-voice: request {request}: a waiting make was dropped by the person's line: never made"
            );
            json!({
                "ok": true,
                "cancelled": label,
                "next": format!(
                    "Say only, in the conversation's language, that you won't make {label} then. \
                     Call nothing else for it."
                )
            })
        }
        Some(Err(label)) => {
            eprintln!(
                "lamp-voice: a waiting make wasn't dropped: the line isn't its asker's or the owner's"
            );
            json!({
                "ok": false,
                "error": "not_theirs",
                "next": format!(
                    "Say in a few words, in the conversation's language, that only whoever asked \
                     for {label} can drop it, so it still waits its turn. Call nothing else."
                )
            })
        }
        None => json!({
            "ok": false,
            "error": "not_waiting",
            "next": "Say in a few words, in the conversation's language, that it isn't waiting \
                     any more: it is being made now or already done. Call nothing else."
        }),
    }
}

/// A person's "forget everything about me", marked by the second route's
/// guard: the makes they asked for that still wait are dropped, never made
/// later from their words, nor told after (the forget's own line answers).
/// Another's, an unmarked one, or one whose memory was not emptied, leaves
/// them.
pub fn forget_with_everything(
    told: (Value, bool),
    lease: &crate::memory_scope::ScopeLease,
) -> (Value, bool) {
    if !lease.everything_marked() {
        return told;
    }
    forget_if(&told, lease.admitted_person());
    told
}

/// [`forget_with_everything`] once its mark is judged: the waiting makes
/// `admitted` asked for dropped when `told` says their memory was emptied.
/// How many were dropped.
pub fn forget_if(told: &(Value, bool), admitted: Option<&str>) -> usize {
    let emptied =
        told.0["forgotten"] == true || told.0["note"] == json!("I was keeping nothing of them.");
    let Some(admitted) = admitted.filter(|_| emptied) else {
        return 0;
    };
    let dropped = kept::queue(|queue| {
        let before = queue.waiting.len();
        queue
            .waiting
            .retain(|entry| entry.asker.speaker.as_deref() != Some(admitted));
        let dropped = before - queue.waiting.len();
        if dropped > 0 {
            queue.changed = true;
        }
        dropped
    });
    if dropped > 0 {
        eprintln!(
            "lamp-voice: {dropped} waiting make(s) dropped with everything about the person who asked: never made"
        );
    }
    // A make of theirs written down for a restart of its route (#7224,
    // fault 5) is never made again after it.
    let resumable =
        crate::make_resume::forget_person(std::path::Path::new(crate::make_resume::FILE), admitted);
    if resumable > 0 {
        eprintln!(
            "lamp-voice: {resumable} make(s) under way dropped from the restart file with everything about the person who asked"
        );
    }
    dropped
}

/// The one line said of a waiting make, `label`, that failed with no line
/// of its own: its answer's `error`, the reason the model would have been
/// told, said as "I couldn't make the gold one: …". None for an answer
/// with no error (made, or a core skill turned on: [`line_to_say`]).
pub fn failed_line(label: &str, told: &Value) -> Option<String> {
    let why = told["error"].as_str()?.trim();
    let why = why.strip_prefix("nothing was made:").unwrap_or(why).trim();
    let why = why.trim_end_matches(['.', '!']).trim();
    if why.is_empty() {
        return None;
    }
    let mut chars = why.chars();
    let first = chars.next()?;
    let why = if why.starts_with("I ") {
        why.to_owned()
    } else {
        first.to_lowercase().chain(chars).collect()
    };
    let label = if label.is_empty() { "that" } else { label };
    Some(format!("I couldn't make {label}: {why}."))
}

/// The line a waiting make's answer gives the model to say word for word
/// when it made nothing of its own to say (a turned-off core skill that
/// already does it turned on, #7763: "Say only this line, once, …: \"…\""):
/// with no turn to say it in, the queue's thread says it. None for any
/// other answer.
pub fn line_to_say(told: &Value) -> Option<String> {
    let next = told["next"].as_str()?;
    let rest = next.strip_prefix("Say only this line")?;
    let open = rest.find('"')?;
    let close = rest.rfind('"')?;
    let line = rest.get(open + 1..close)?.trim();
    (!line.is_empty()).then(|| line.to_owned())
}

/// What one look at the queue ([`step`]) found to do.
pub struct Stepped {
    /// The labels of the makes dropped now, told as not made.
    pub dropped: Vec<String>,
    /// The next make to run now, with no turn ([`Entry::run`]).
    pub run: Option<Entry>,
    /// The waiting labels, when they changed since last kept.
    pub keep: Option<Vec<String>>,
}

/// One look at the queue at `now`: the waits past [`WAITS_AT_MOST`], or
/// every wait when the quiet hours just began (`quiet_began`), dropped;
/// the slot freed once its line was said and no yes is awaited
/// (`awaited`, `skill_write_yes::awaited`), or past [`RUNS_AT_MOST`]; and
/// then the front make taken to run.
pub fn step(now: Instant, awaited: bool, quiet_began: bool) -> Stepped {
    kept::queue(|queue| {
        let mut dropped = Vec::new();
        queue.waiting.retain(|entry| {
            let keep = !quiet_began && now.saturating_duration_since(entry.at) <= WAITS_AT_MOST;
            if !keep {
                eprintln!(
                    "lamp-voice: request {}: a waiting make was dropped ({}): told as not made, never made later",
                    entry.request,
                    if quiet_began {
                        "the quiet hours began"
                    } else {
                        "it waited too long"
                    }
                );
                dropped.push(entry.label.clone());
            }
            keep
        });
        let free = queue.running.as_ref().is_some_and(|slot| {
            let overrun = now.saturating_duration_since(slot.at) > RUNS_AT_MOST;
            if overrun {
                eprintln!(
                    "lamp-voice: request {}: the make under way held the queue past {} s: let go",
                    slot.request,
                    RUNS_AT_MOST.as_secs()
                );
            }
            overrun || (slot.said && !awaited)
        });
        if free {
            queue.running = None;
        }
        let run = if queue.running.is_none() {
            queue.waiting.pop_front().inspect(|entry| {
                queue.running = Some(Slot {
                    request: entry.request,
                    said: false,
                    at: now,
                });
            })
        } else {
            None
        };
        if !dropped.is_empty() || run.is_some() {
            queue.changed = true;
        }
        let keep = if std::mem::take(&mut queue.changed) {
            Some(labels(queue))
        } else {
            None
        };
        Stepped { dropped, run, keep }
    })
}

/// The labels waiting, in turn.
fn labels(queue: &Queue) -> Vec<String> {
    queue
        .waiting
        .iter()
        .map(|entry| entry.label.clone())
        .collect()
}

/// The line said of the makes dropped, `labels`: [`QUEUE_DROPPED_LINE`] in
/// the owner's language, then their labels.
pub fn dropped_line(labels: &[String]) -> String {
    format!(
        "{} {}.",
        crate::language::fixed_in(crate::language::at_start(), QUEUE_DROPPED_LINE),
        labels.join(", ")
    )
}

/// `labels` kept at `path` for a voice restart, or the file removed when
/// none wait. Written whole (`thread::write_file`).
pub fn keep_labels(path: &std::path::Path, labels: &[String]) {
    if labels.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    let kept = json!({ "labels": labels }).to_string();
    if let Err(error) = crate::thread::write_file(path, &kept) {
        eprintln!("lamp-voice: the waiting makes' labels aren't kept for a restart: {error}");
    }
}

/// The labels the last run left at `path`, read once and the file removed:
/// those makes were never made.
pub fn labels_left(path: &std::path::Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let _ = std::fs::remove_file(path);
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|kept| {
            kept["labels"].as_array().map(|labels| {
                labels
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|label| label.chars().take(LABEL_MOST).collect())
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// At the worker's start, with the switch on: the makes still waiting when
/// the voice last stopped ([`QUEUE_FILE`]) told as not made, once, by `say`,
/// on a thread of its own once the say service answers (`ready`), or after
/// [`SAY_READY_WITHIN`] all the same, journaled.
pub fn told_after_restart(say: fn(String), ready: fn() -> bool) {
    if !on() {
        return;
    }
    let left = labels_left(std::path::Path::new(QUEUE_FILE));
    if left.is_empty() {
        return;
    }
    eprintln!(
        "lamp-voice: {} make(s) waiting when the voice stopped were never made: told as not made",
        left.len()
    );
    let told = std::thread::Builder::new()
        .name("lamp-voice-make-queue-told".into())
        .spawn(move || {
            let began = Instant::now();
            if ready_within(ready, SAY_READY_WITHIN, SAY_READY_EVERY) {
                eprintln!(
                    "lamp-voice: the say service answered {} ms after the start: the makes not made are told",
                    began.elapsed().as_millis()
                );
            } else {
                eprintln!(
                    "lamp-voice: the say service didn't answer within {} s of the start: the makes not made are told all the same",
                    SAY_READY_WITHIN.as_secs()
                );
            }
            say(dropped_line(&left));
        });
    if told.is_err() {
        eprintln!("lamp-voice: the makes not made at the restart couldn't be told: no thread");
    }
}

/// Whether `ready` holds within `within`, tried every `every`: at once when
/// it already does.
pub fn ready_within(ready: impl Fn() -> bool, within: Duration, every: Duration) -> bool {
    let began = Instant::now();
    loop {
        if ready() {
            return true;
        }
        if began.elapsed() >= within {
            return false;
        }
        std::thread::sleep(every);
    }
}

/// With the switch on, the queue's own thread: every [`STEP_EVERY`] a
/// [`step`], the dropped told by `say`, the labels kept, and the next make
/// given to `run`. Started once.
pub fn start(run: fn(Entry), say: fn(String)) {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !on() || STARTED.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    eprintln!(
        "lamp-voice: a make asked while another is made waits its turn ({ENVIRONMENT}=on, at most {MOST_WAITING})"
    );
    let started = std::thread::Builder::new()
        .name("lamp-voice-make-queue".into())
        .spawn(move || {
            let mut was_quiet: Option<bool> = None;
            loop {
                std::thread::sleep(STEP_EVERY);
                let now = Instant::now();
                // The quiet hours read only while a make waits, and only
                // their start drops one: a make asked in them waits as any.
                let quiet_began = if kept::queue(|queue| queue.waiting.is_empty()) {
                    was_quiet = None;
                    false
                } else {
                    let quiet = crate::run_write_yes::quiet_hours_now();
                    let began = was_quiet == Some(false) && quiet;
                    was_quiet = Some(quiet);
                    began
                };
                let stepped = step(now, crate::skill_write_yes::awaited(now), quiet_began);
                if !stepped.dropped.is_empty() {
                    let line = dropped_line(&stepped.dropped);
                    let told = std::thread::Builder::new()
                        .name("lamp-voice-make-queue-told".into())
                        .spawn(move || say(line));
                    if told.is_err() {
                        eprintln!("lamp-voice: the makes dropped couldn't be told: no thread");
                    }
                }
                if let Some(labels) = stepped.keep {
                    keep_labels(std::path::Path::new(QUEUE_FILE), &labels);
                }
                if let Some(entry) = stepped.run {
                    eprintln!(
                        "lamp-voice: request {}: a waiting make begins: the one before it is done",
                        entry.request
                    );
                    run(entry);
                }
            }
        });
    if started.is_err() {
        eprintln!("lamp-voice: the make queue couldn't start: no thread");
    }
}

/// `make_skill`'s parameters with the queue's two: the label with a
/// request, and the id of a waiting make to drop.
pub fn with_parameters(parameters: &mut Value) {
    parameters["properties"][LABEL] = json!({
        "type": "string",
        "description": "With request: two to four words naming this skill as you'd say it to them, in the conversation's language (\"the gold one\")."
    });
    parameters["properties"][CANCEL] = json!({
        "type": "string",
        "description": "Alone: the id of a skill waiting to be made that the person drops (\"never mind the gold one\"), as the note lists it."
    });
}

/// Tests: the queue on this thread emptied.
#[cfg(test)]
pub fn clear_for_test() {
    kept::queue(|queue| *queue = Queue::EMPTY);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolAnswer;
    use std::sync::{Arc, Mutex};

    /// A make whose work, once run, notes its request in `made`.
    fn work(request: u64, made: &Arc<Mutex<Vec<u64>>>) -> Work {
        let made = made.clone();
        Box::new(move || {
            made.lock().unwrap().push(request);
            ToolAnswer::plain(json!({"made": format!("skill-{request}")}))
        })
    }

    /// Who asked: a known person's line, not the owner's flag.
    fn by(speaker: &str) -> Asker {
        Asker {
            speaker: Some(speaker.to_owned()),
            owner: (speaker == "ann").then(|| speaker.to_owned()),
            heard: Some(format!("{speaker}'s own line")),
        }
    }

    fn make(request: &str, label: &str) -> Value {
        json!({ "request": request, LABEL: label })
    }

    /// A code make's say-back waiting for its yes, as `make_with` leaves it.
    fn waits_for_yes(skill: &str, request: u64, now: Instant) -> String {
        crate::skill_write_yes::wait_for_yes(
            crate::skill_write_yes::Asks {
                skill: skill.to_owned(),
                sha256: "0".repeat(64),
                writes: Vec::new(),
                say_back: "it may use the internet.".to_owned(),
                grants: Some(vec!["net".to_owned()]),
                code: true,
                remove: false,
            },
            request,
            now,
        )
    }

    /// A step as the queue's thread takes it, the yes read from this
    /// thread's wait.
    fn step_now(now: Instant) -> Stepped {
        step(now, crate::skill_write_yes::awaited(now), false)
    }

    /// The row's case (lamp-9adf, Fourteen): a make, then 47 s later
    /// another; the second is queued, not refused, and both are made in
    /// order, each said back, the second once the first's line was said.
    /// First use and a repeat: a third make later runs at once again.
    #[test]
    fn two_makes_47s_apart_are_both_made_in_order_each_with_its_say_back() {
        clear_for_test();
        crate::skill_write_yes::clear_for_test();
        let made = Arc::new(Mutex::new(Vec::new()));
        let t0 = Instant::now();
        // The first: nothing under way, so it runs in its own turn.
        assert!(matches!(
            admit(3, by("ann"), "bitcoin price".into(), t0, work(3, &made)),
            Admitted::Run
        ));
        // 47 s later, the gold one: queued at once, place 1.
        let t47 = t0 + Duration::from_secs(47);
        let Admitted::Queued { place, id } =
            admit(4, by("ann"), "gold price".into(), t47, work(4, &made))
        else {
            panic!("the second make wasn't queued");
        };
        assert_eq!((place, id.as_str()), (1, "q1"));
        // The first still under way: nothing else runs.
        assert!(step_now(t47).run.is_none());
        // The first's line given to its turn, no yes waiting: the gold one
        // runs next, with no turn.
        said(3);
        let stepped = step_now(t47 + Duration::from_secs(10));
        let entry = stepped.run.expect("the gold one runs");
        assert_eq!((entry.request, entry.label.as_str()), (4, "gold price"));
        // Who asked, and its own line, kept from the ask.
        assert_eq!(entry.asker, by("ann"));
        let told = entry.run();
        assert_eq!(told.told["made"], "skill-4");
        assert_eq!(*made.lock().unwrap(), vec![4]);
        // Its line said through the say service, and the slot freed.
        said(4);
        assert!(step_now(t47 + Duration::from_secs(70)).run.is_none());
        assert!(kept::queue(|queue| queue.running.is_none()));
        // A repeat, later: nothing under way, it runs at once.
        assert!(matches!(
            admit(
                9,
                Asker::default(),
                "rain check".into(),
                t47 + Duration::from_secs(90),
                work(9, &made)
            ),
            Admitted::Run
        ));
    }

    /// Two wait; a third gets the full line, honest about the wait, and
    /// nothing of it is kept.
    #[test]
    fn a_third_waiting_gets_the_full_line() {
        clear_for_test();
        let made = Arc::new(Mutex::new(Vec::new()));
        let now = Instant::now();
        assert!(matches!(
            admit(1, Asker::default(), "a".into(), now, work(1, &made)),
            Admitted::Run
        ));
        assert!(matches!(
            admit(2, Asker::default(), "b".into(), now, work(2, &made)),
            Admitted::Queued { place: 1, .. }
        ));
        assert!(matches!(
            admit(3, Asker::default(), "c".into(), now, work(3, &made)),
            Admitted::Queued { place: 2, .. }
        ));
        assert!(matches!(
            admit(4, Asker::default(), "d".into(), now, work(4, &made)),
            Admitted::Full
        ));
        assert_eq!(kept::queue(|queue| queue.waiting.len()), MOST_WAITING);
        let told = full_told();
        assert_eq!(told["error"], "queue_full");
        assert!(
            told["next"].as_str().unwrap().contains(QUEUE_FULL_LINE),
            "{told}"
        );
        // Each language has its own words for it (`language::FIXED_LINES`).
        assert_eq!(
            crate::language::Language::Vietnamese.fixed(QUEUE_FULL_LINE),
            Some(
                "Mình đang có hai kỹ năng chờ làm rồi, nên chưa nhận thêm được. Xong rồi bạn nhờ lại mình nhé."
            )
        );
        assert!(made.lock().unwrap().is_empty());
    }

    /// A queued make is told at once that it waits, never the busy line,
    /// and the enqueue is quick: one lock and a push. A slow agent on the
    /// first (still under way after 100 s) doesn't drop it.
    #[test]
    fn a_queued_make_is_told_at_once_and_never_refused_busy() {
        clear_for_test();
        let made = Arc::new(Mutex::new(Vec::new()));
        let now = Instant::now();
        assert!(matches!(
            admit(1, Asker::default(), "first".into(), now, work(1, &made)),
            Admitted::Run
        ));
        let began = Instant::now();
        let admitted = admit(
            2,
            Asker::default(),
            "the gold one".into(),
            now,
            work(2, &made),
        );
        let took = began.elapsed();
        let Admitted::Queued { place, id } = admitted else {
            panic!("not queued");
        };
        // The target is 10 ms (the journal's "make queued … in X ms"); 50
        // here, so a loaded laptop doesn't flake.
        assert!(took < Duration::from_millis(50), "{took:?}");
        let told = queued_told(place, &id, "the gold one");
        assert_eq!(told["ok"], true);
        assert_eq!(told["queued"], true);
        assert_eq!(told["place"], 1);
        let note = told["note"].as_str().unwrap();
        assert!(note.contains("you'll make the gold one next"), "{note}");
        assert!(note.contains("conversation's language"), "{note}");
        assert!(!note.contains(crate::made_skill::AGENT_BUSY_TRY), "{note}");
        // The first, a slow agent, still under way at 100 s: the gold one waits.
        assert!(step_now(now + Duration::from_secs(100)).run.is_none());
        assert!(made.lock().unwrap().is_empty());
        assert_eq!(kept::queue(|queue| queue.waiting.len()), 1);
    }

    /// The first's say-back waits for its yes: the queued make waits too,
    /// so its own wait never overwrites the first's; once the yes is
    /// answered it runs. Past the yes's time it runs as well.
    #[test]
    fn a_queued_says_back_waits_for_the_first_yes() {
        clear_for_test();
        crate::skill_write_yes::clear_for_test();
        let made = Arc::new(Mutex::new(Vec::new()));
        let now = Instant::now();
        assert!(matches!(
            admit(3, Asker::default(), "btc".into(), now, work(3, &made)),
            Admitted::Run
        ));
        assert!(matches!(
            admit(4, Asker::default(), "gold".into(), now, work(4, &made)),
            Admitted::Queued { .. }
        ));
        let id = waits_for_yes("btc-price", 3, now);
        said(3);
        // Said, but its yes still awaited: the gold one waits.
        assert!(step_now(now + Duration::from_secs(5)).run.is_none());
        assert!(crate::skill_write_yes::waits(&id));
        // The yes answered (the wait cleared): the gold one runs.
        assert!(crate::skill_write_yes::drop_wait(&id));
        let entry = step_now(now + Duration::from_secs(8)).run.expect("runs");
        assert_eq!(entry.request, 4);
        // Past YES_WITHIN with no answer: the next runs as well.
        clear_for_test();
        crate::skill_write_yes::clear_for_test();
        assert!(matches!(
            admit(5, Asker::default(), "a".into(), now, work(5, &made)),
            Admitted::Run
        ));
        assert!(matches!(
            admit(6, Asker::default(), "b".into(), now, work(6, &made)),
            Admitted::Queued { .. }
        ));
        waits_for_yes("a", 5, now);
        said(5);
        let late = now + crate::skill_write_yes::YES_WITHIN + Duration::from_secs(1);
        assert!(step_now(late).run.is_some_and(|entry| entry.request == 6));
    }

    /// "Never mind the gold one": only that one is dropped, the rest keep
    /// their turn, and the note lists what still waits.
    #[test]
    fn never_mind_drops_only_that_one() {
        clear_for_test();
        let made = Arc::new(Mutex::new(Vec::new()));
        let now = Instant::now();
        assert!(note().is_none(), "none waits");
        assert!(matches!(
            admit(1, by("ann"), "btc".into(), now, work(1, &made)),
            Admitted::Run
        ));
        let Admitted::Queued { id: gold, .. } =
            admit(2, by("ann"), "gold price".into(), now, work(2, &made))
        else {
            panic!()
        };
        let Admitted::Queued { id: rain, .. } =
            admit(3, by("ann"), "rain check".into(), now, work(3, &made))
        else {
            panic!()
        };
        let listed = note().unwrap();
        assert!(
            listed.contains(&format!("\"gold price\" ({CANCEL} \"{gold}\")")),
            "{listed}"
        );
        assert!(
            listed.contains(&format!("\"rain check\" ({CANCEL} \"{rain}\")")),
            "{listed}"
        );
        let told = cancel(&gold, Some("ann"), false);
        assert_eq!(told["cancelled"], "gold price", "{told}");
        let listed = note().unwrap();
        assert!(!listed.contains("gold price"), "{listed}");
        assert!(listed.contains("rain check"), "{listed}");
        // Again: no longer waiting.
        assert_eq!(cancel(&gold, Some("ann"), false)["error"], "not_waiting");
        // The rain check keeps its turn and runs next.
        said(1);
        let entry = step(now, false, false).run.expect("runs");
        assert_eq!(entry.request, 3);
        assert!(note().is_none());
    }

    /// A person's "forget everything about me" while their make waits: it is
    /// dropped, never made later from their words, and its label no longer
    /// kept for a restart; another person's waiting make stays, and a forget
    /// that emptied nothing drops nothing.
    #[test]
    fn forget_everything_drops_only_their_waiting_makes() {
        clear_for_test();
        crate::skill_write_yes::clear_for_test();
        let made = Arc::new(Mutex::new(Vec::new()));
        let t0 = Instant::now();
        assert!(matches!(
            admit(1, by("ann"), "bitcoin price".into(), t0, work(1, &made)),
            Admitted::Run
        ));
        assert!(matches!(
            admit(2, by("bob"), "gold price".into(), t0, work(2, &made)),
            Admitted::Queued { .. }
        ));
        assert!(matches!(
            admit(3, by("ann"), "rain alert".into(), t0, work(3, &made)),
            Admitted::Queued { .. }
        ));
        let kept_nothing = (json!({"forgotten": false}), true);
        assert_eq!(forget_if(&kept_nothing, Some("bob")), 0);
        let emptied = (json!({"forgotten": true}), true);
        assert_eq!(forget_if(&emptied, None), 0);
        assert_eq!(forget_if(&emptied, Some("bob")), 1);
        // The labels kept for a restart no longer name it.
        assert_eq!(step_now(t0).keep, Some(vec!["rain alert".to_owned()]));
        said(1);
        let entry = step_now(t0 + Duration::from_secs(1))
            .run
            .expect("ann's runs");
        assert_eq!(entry.label, "rain alert");
        entry.run();
        assert_eq!(*made.lock().unwrap(), vec![3]);
    }

    /// A waiting make that turned on a core skill already doing it (#7763)
    /// has the line its answer gives to say; any other answer has none.
    #[test]
    fn a_waiting_makes_core_turn_on_line_is_read_from_its_answer() {
        let told = json!({
            "turned_on": "weather",
            "next": "Say only this line, once, in the conversation's language: \"Weather is on now. Say turn off weather to undo it.\" Call nothing."
        });
        assert_eq!(
            line_to_say(&told).as_deref(),
            Some("Weather is on now. Say turn off weather to undo it.")
        );
        for told in [
            json!({"next": "Say plainly, in a few words, that there's nothing to undo. Call nothing now."}),
            json!({"next": "Say only this line: \"\" Call nothing."}),
            json!({"ok": true}),
        ] {
            assert_eq!(line_to_say(&told), None, "{told}");
        }
    }

    /// Each kind of failure with no line of its own gives one line, named
    /// by the make's label; an answer with no error gives none.
    #[test]
    fn a_waiting_makes_failure_is_one_line_by_its_label() {
        for (told, line) in [
            (
                json!({"error": crate::make_no_asks::NOT_ACTED, "next": crate::make_no_asks::NOT_ACTED_NEXT}),
                "I couldn't make gold price: it wasn't clear what to make.",
            ),
            (
                json!({"error": "Say what the skill should do.", "next": "Tell them that in a few words."}),
                "I couldn't make gold price: say what the skill should do.",
            ),
            (
                crate::tools::unavailable(),
                "I couldn't make gold price: that part of me is not running right now, so I could not do it.",
            ),
        ] {
            assert_eq!(
                failed_line("gold price", &told).as_deref(),
                Some(line),
                "{told}"
            );
        }
        assert_eq!(
            failed_line("", &crate::tools::unavailable()).as_deref(),
            Some(
                "I couldn't make that: that part of me is not running right now, so I could not do it."
            )
        );
        for told in [
            json!({"ok": true}),
            json!({"turned_on": "weather"}),
            json!({"error": " "}),
        ] {
            assert_eq!(failed_line("gold price", &told), None, "{told}");
        }
    }

    /// A guest can't drop the owner's waiting make; the owner can drop a
    /// guest's; a line no one known said drops nothing it didn't ask for.
    #[test]
    fn a_guest_cant_cancel_the_owners() {
        clear_for_test();
        let made = Arc::new(Mutex::new(Vec::new()));
        let now = Instant::now();
        assert!(matches!(
            admit(1, by("ann"), "x".into(), now, work(1, &made)),
            Admitted::Run
        ));
        let Admitted::Queued { id: owners, .. } =
            admit(2, by("ann"), "gold price".into(), now, work(2, &made))
        else {
            panic!()
        };
        let Admitted::Queued { id: guests, .. } =
            admit(3, by("bob"), "song of the day".into(), now, work(3, &made))
        else {
            panic!()
        };
        assert_eq!(cancel(&owners, Some("bob"), false)["error"], "not_theirs");
        assert_eq!(cancel(&owners, None, false)["error"], "not_theirs");
        assert!(note().unwrap().contains("gold price"));
        // The owner (ann) drops the guest's.
        assert_eq!(cancel(&guests, Some("ann"), true)["ok"], true);
        assert_eq!(kept::queue(|queue| queue.waiting.len()), 1);
    }

    /// A make that waited past its time, or was waiting at the quiet hours'
    /// start, is dropped, told as not made in the owner's language with its
    /// label, and never run later; across a restart only the labels are
    /// kept, read once, then gone.
    #[test]
    fn a_waiting_make_past_its_time_or_restart_is_told_not_made() {
        clear_for_test();
        let made = Arc::new(Mutex::new(Vec::new()));
        let now = Instant::now();
        assert!(matches!(
            admit(1, Asker::default(), "x".into(), now, work(1, &made)),
            Admitted::Run
        ));
        assert!(matches!(
            admit(
                2,
                Asker::default(),
                "gold price".into(),
                now,
                work(2, &made)
            ),
            Admitted::Queued { .. }
        ));
        let stepped = step(now + Duration::from_secs(60), true, false);
        assert!(stepped.dropped.is_empty());
        assert_eq!(stepped.keep, Some(vec!["gold price".to_owned()]));
        let stepped = step(now + WAITS_AT_MOST + Duration::from_secs(1), true, false);
        assert_eq!(stepped.dropped, vec!["gold price".to_owned()]);
        assert_eq!(stepped.keep, Some(Vec::new()));
        // Never made later, even once the slot is free.
        said(1);
        assert!(
            step(now + WAITS_AT_MOST + Duration::from_secs(2), false, false)
                .run
                .is_none()
        );
        assert!(made.lock().unwrap().is_empty());
        // The quiet hours begin: every wait dropped at once.
        clear_for_test();
        assert!(matches!(
            admit(5, Asker::default(), "x".into(), now, work(5, &made)),
            Admitted::Run
        ));
        assert!(matches!(
            admit(6, Asker::default(), "rain".into(), now, work(6, &made)),
            Admitted::Queued { .. }
        ));
        assert_eq!(step(now, true, true).dropped, vec!["rain".to_owned()]);
        // The line: the fixed line, in Vietnamese too, then the labels.
        assert_eq!(
            dropped_line(&["gold price".to_owned()]),
            format!("{QUEUE_DROPPED_LINE} gold price.")
        );
        assert!(
            crate::language::Language::Vietnamese
                .fixed(QUEUE_DROPPED_LINE)
                .is_some_and(|line| line.ends_with(':'))
        );
        // A restart: the labels kept, read once, and the file removed.
        let dir = std::env::temp_dir().join(format!("make-queue-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("make-queue.json");
        keep_labels(&file, &["gold price".to_owned(), "rain".to_owned()]);
        assert_eq!(
            labels_left(&file),
            vec!["gold price".to_owned(), "rain".to_owned()]
        );
        assert!(!file.exists());
        assert!(labels_left(&file).is_empty());
        keep_labels(&file, &["x".to_owned()]);
        keep_labels(&file, &[]);
        assert!(!file.exists(), "none waiting: no file");
        let _ = std::fs::remove_dir_all(&dir);
        // Told once the say service answers, never on a fixed wait: at once
        // when it does, on the third try, or after the cap all the same.
        let every = Duration::from_millis(1);
        assert!(ready_within(|| true, Duration::ZERO, every));
        let tries = std::cell::Cell::new(0);
        assert!(ready_within(
            || {
                tries.set(tries.get() + 1);
                tries.get() >= 3
            },
            Duration::from_secs(5),
            every
        ));
        assert_eq!(tries.get(), 3);
        let began = Instant::now();
        assert!(!ready_within(|| false, Duration::from_millis(20), every));
        assert!(began.elapsed() >= Duration::from_millis(20));
    }

    /// Off unless exactly `on`: the calls it reads are a new make's only,
    /// and nothing waits or is noted with it off.
    #[test]
    fn off_unless_set() {
        assert!(on_with(Some("on")));
        for value in [
            None,
            Some(""),
            Some("off"),
            Some("ON"),
            Some("on "),
            Some("1"),
        ] {
            assert!(!on_with(value), "{value:?}");
        }
        clear_for_test();
        assert!(note().is_none());
        // Only a new make with words waits its turn.
        assert!(is_new_make(&make(
            "check the gold price at six",
            "gold price"
        )));
        assert!(!is_new_make(&json!({"request": "  "})));
        assert!(!is_new_make(
            &json!({crate::skill_write_yes::APPROVE_ID: "yes-1"})
        ));
        assert!(!is_new_make(
            &json!({"request": "x", crate::made_remove::REMOVE: "gold"})
        ));
        assert!(!is_new_make(&json!({crate::made_list::LIST: true})));
        assert!(!is_new_make(&json!({"request": "x", CANCEL: "q1"})));
        assert_eq!(cancel_of(&json!({CANCEL: " q2 "})), Some("q2"));
        // A label: the model's words, else the request's first four; one
        // line, no quotes, bounded. Vietnamese kept as said.
        assert_eq!(label_of(&make("x", "giá vàng")), "giá vàng");
        assert_eq!(
            label_of(&json!({"request": "check the gold price every evening at six"})),
            "check the gold price"
        );
        assert_eq!(label_of(&make("x", "the \"gold\"\none")), "the gold one");
        assert!(label_of(&make("x", &"word ".repeat(30))).chars().count() <= LABEL_MOST);
    }
}
