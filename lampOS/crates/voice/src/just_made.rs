//! The skill made, or turned off or back on, in this conversation, for this
//! person: what "the skill you just made", "that skill", "the new one" and
//! "it" mean right after a make or a turn (#7660; with
//! `LAMP_MADE_SKILL_OFF_SAID` on, `made_skill::off_said_on`). On lamp-9adf
//! (stock 14060, 18 Nov) after a make of "water reminder", "Turn off the
//! skill you just made." was answered "I found no skill of yours with a name
//! like 'water skill'"; after a make of "daily stretch reminder", "Hmm,
//! actually, turn off the skill you just made." listed all 25 of theirs; and
//! "turn it back on" got "Yes?". The model was never told which skill the
//! lamp had just made with them.
//!
//! - **Kept in code, told to the model.** A `make_skill` call that made a
//!   skill ([`after_make`]) or turned one off or on ([`after_turn`]) keeps
//!   its name, for the line's person when the line is the owner's
//!   (`services::owner_of`, as a turn is the owner's alone): another
//!   person's make never counts. For [`KEPT_FOR`] after each make or turn
//!   (never carried on by their lines), each heard line of theirs is told
//!   one short note ([`note`]): what the lamp made or turned with them,
//!   newest first, by name, and which call does what with it: `make_skill`
//!   to turn it off, on or remove it, and to use it as for any skill, its
//!   own function, `use_skill` with its name or `make_skill` `run` (as
//!   `made_turn::off_note` says, #7562), never a second make. Which one
//!   a line means is the model's to judge, never a list of words (the
//!   owner's ruling, 13 Nov), told as one quick question at most (the
//!   first builder's ruling on #7715, 23:14): the one that fits is acted
//!   on, never asked; more than one that fits is asked once, naming them so
//!   one word answers it ("The water one or the stretch one?",
//!   [`ASK_WHICH`]); none that fits goes to `make_skill`, which finds it
//!   among all of theirs or says plainly there's none like it, never a
//!   question ([`NOTE_NONE_FITS`], [`NONE_FITS`]). A line about anything
//!   else is answered as it is. In the per-turn notes, never in
//!   `make_skill`'s description, which stays within its bytes.
//! - **Candidates, this conversation's first.** A turn whose name found none
//!   offers theirs (`made_turn::CANDIDATES`); those made or turned in this
//!   conversation come first, each marked [`IN_THIS_CONVERSATION`], and the
//!   model is told so ([`candidates`]), with the same one, several and
//!   none. With none made or turned in this conversation, it is told that
//!   too: a line that means one made just now has none like that, said
//!   plainly, never a guess and never a question.
//!
//! No file, no round trip: a few names in this process's memory, and the
//! made skills' index read only to leave out one no longer kept (a skill
//! removed since, or every one once a factory reset emptied the made
//! skills' folder; the memory itself goes when `lamp-voice` restarts). The
//! owner's "forget everything" clears it ([`forget_with_everything`]). The
//! journal names no skill.
use crate::made_skill;
use crate::made_turn::{CANDIDATES, TURN_OFF, TURN_ON};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// How long a make or a turn is told after it was done, as the thread's
/// own `thread::REMEMBERED_FOR` (10 minutes): the person's lines never
/// carry it on.
pub const KEPT_FOR: Duration = crate::thread::REMEMBERED_FOR;

/// The answer's key that marks a candidate made or turned in this
/// conversation: what was done with it, and when.
pub const IN_THIS_CONVERSATION: &str = "in_this_conversation";

/// The most of a conversation's makes and turns kept for a person.
const KEPT_MOST: usize = 8;

/// The most skills a note names.
const SAID_MOST: usize = 3;

/// Whose conversations are kept at once.
const PEOPLE: usize = 4;

/// Said "just now" within this long; in minutes after.
const JUST_NOW: Duration = Duration::from_secs(90);

/// What was done with a skill in the conversation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Done {
    Made,
    Off,
    On,
}

/// A skill made or turned in the conversation: its written name, what, and
/// when.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub name: String,
    pub done: Done,
    pub at: Instant,
}

struct Talk {
    person: String,
    /// Oldest first.
    events: Vec<Event>,
}

impl Talk {
    /// When the newest of its events was done.
    fn last(&self) -> Option<Instant> {
        self.events.last().map(|event| event.at)
    }
}

/// Each person's conversation: what the lamp made and turned in it.
#[derive(Default)]
pub struct Conversations {
    talks: Vec<Talk>,
}

impl Conversations {
    /// Makes and turns done longer than [`KEPT_FOR`] before `now`, and the
    /// conversations left with none, dropped.
    fn ended(&mut self, now: Instant) {
        for talk in &mut self.talks {
            talk.events
                .retain(|event| now.saturating_duration_since(event.at) <= KEPT_FOR);
        }
        self.talks.retain(|talk| !talk.events.is_empty());
    }

    /// Every conversation's, forgotten.
    fn clear(&mut self) {
        self.talks.clear();
    }

    /// `name` `done` in `person`'s conversation at `now`.
    pub fn keep(&mut self, person: &str, name: &str, done: Done, now: Instant) {
        self.ended(now);
        let at = match self.talks.iter().position(|talk| talk.person == person) {
            Some(at) => at,
            None => {
                if self.talks.len() >= PEOPLE
                    && let Some(oldest) =
                        (0..self.talks.len()).min_by_key(|at| self.talks[*at].last())
                {
                    self.talks.remove(oldest);
                }
                self.talks.push(Talk {
                    person: person.to_owned(),
                    events: Vec::new(),
                });
                self.talks.len() - 1
            }
        };
        let talk = &mut self.talks[at];
        talk.events
            .retain(|event| !(event.name == name && event.done == done));
        talk.events.push(Event {
            name: name.to_owned(),
            done,
            at: now,
        });
        if talk.events.len() > KEPT_MOST {
            talk.events.remove(0);
        }
    }

    /// What was made and turned in `person`'s conversation within
    /// [`KEPT_FOR`] of `now`, oldest first. Reading it carries nothing on.
    pub fn of(&mut self, person: &str, now: Instant) -> Vec<Event> {
        self.ended(now);
        self.talks
            .iter()
            .find(|talk| talk.person == person)
            .map(|talk| talk.events.clone())
            .unwrap_or_default()
    }
}

static CONVERSATIONS: std::sync::Mutex<Conversations> =
    std::sync::Mutex::new(Conversations { talks: Vec::new() });

#[cfg(test)]
std::thread_local! {
    /// A test's own, so parallel tests never share them.
    static FOR_TEST: std::cell::RefCell<Conversations> =
        std::cell::RefCell::new(Conversations::default());
}

/// This process's conversations, shared by every thread of the second
/// worker: a tool call's thread keeps, a later line's thread reads.
fn shared<R>(each: impl FnOnce(&mut Conversations) -> R) -> R {
    let mut kept = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    each(&mut kept)
}

/// Tests: how many things were made or turned in `person`'s conversation,
/// on this test's thread.
#[cfg(test)]
pub(crate) fn kept_for_test(person: &str) -> usize {
    with(|kept| kept.of(person, Instant::now()).len())
}

/// This process's conversations ([`shared`]; a test's own thread's in a
/// test, so parallel tests never share them).
fn with<R>(each: impl FnOnce(&mut Conversations) -> R) -> R {
    #[cfg(test)]
    {
        FOR_TEST.with(|kept| each(&mut kept.borrow_mut()))
    }
    #[cfg(not(test))]
    {
        shared(each)
    }
}

/// How long ago `at` was at `now`, as said.
fn ago(at: Instant, now: Instant) -> String {
    let since = now.saturating_duration_since(at);
    if since < JUST_NOW {
        return "just now".to_owned();
    }
    match (since.as_secs() + 30) / 60 {
        1 => "1 minute ago".to_owned(),
        minutes => format!("{minutes} minutes ago"),
    }
}

/// What was done with `event`'s skill, as said: "made water reminder".
fn what(event: &Event) -> String {
    let spoken = made_skill::spoken_name(&event.name);
    match event.done {
        Done::Made => format!("made {spoken}"),
        Done::Off => format!("turned {spoken} off"),
        Done::On => format!("turned {spoken} back on"),
    }
}

/// Each skill once, by the last done with it, newest first.
fn newest(events: &[Event]) -> Vec<&Event> {
    let mut newest: Vec<&Event> = Vec::new();
    for event in events.iter().rev() {
        if !newest.iter().any(|kept| kept.name == event.name) {
            newest.push(event);
        }
    }
    newest
}

/// What the model is told when more than one of the skills it was told
/// could be the one a line means: one quick question, naming them so one
/// word answers it (the first builder's ruling on #7715, 23:14: "keep the
/// one question, worded to be quick").
pub const ASK_WHICH: &str = "If more than one fits and their line doesn't say which, ask once, \
     short, naming only those that fit so one word answers it, as in \"The water one or the \
     stretch one?\", and call nothing.";

/// What the model is told when none of the skills it was told fits the
/// line: no such skill, said plainly, never a question (the same ruling).
pub const NONE_FITS: &str = "If none fits, say plainly, in a few words, that they have no skill \
     like that, and ask nothing: never which one they mean.";

/// What the model is told by the note when none of the skills it names fits
/// the line: the note lists only this conversation's, so the line may mean
/// another of theirs, which `make_skill` finds among all of them (and says
/// so when none is like it); never "no skill like that" from the note
/// (Thirteen's P2 on #7715).
pub const NOTE_NONE_FITS: &str = "If none of these fits, the line is about another of theirs or \
     something else: call make_skill with what they said, as for any skill, and never say from \
     this note that they have no skill like that.";

/// The note a heard line of the person's is told at `now` of what the lamp
/// made and turned in their conversation (`events`, oldest first): none
/// when nothing was. Which of these a line means is the model's to judge:
/// one that fits is acted on, more than one asked once ([`ASK_WHICH`],
/// only when it names more than one), none left to `make_skill` ([`NOTE_NONE_FITS`]).
pub fn note(events: &[Event], now: Instant) -> Option<String> {
    let newest = newest(events);
    if newest.is_empty() {
        return None;
    }
    let done: Vec<String> = newest
        .iter()
        .take(SAID_MOST)
        .map(|event| format!("{} ({})", what(event), ago(event.at, now)))
        .collect();
    let which = if newest.len() > 1 {
        format!(" {ASK_WHICH}")
    } else {
        String::new()
    };
    Some(format!(
        "(For you to know, not to answer: in this conversation with them, newest first, the \
         lamp {}. When their line means a skill of theirs without saying its name, judge which \
         of these fits what they ask. If only one fits, it means that one, as if they had said \
         its name: act on it and never ask. To turn it off, turn it on or remove it, call \
         make_skill with turn_off, turn_on or remove and that name; to use or run it, call it as \
         for any skill, by its own function, by use_skill with that name, or by make_skill with \
         run and that name, never a second make of it. Never list their skills for it.{which} \
         {NOTE_NONE_FITS} A line about anything else, the light, music or a timer, is answered as it \
         is: no question about skills, and no skill turned on for it.)",
        done.join("; ")
    ))
}

/// A turn's answer `told` with its candidates (`made_turn::CANDIDATES`),
/// those made or turned in the conversation (`events`, oldest first)
/// first, each marked [`IN_THIS_CONVERSATION`], and the model told so: one
/// that fits is called, more than one marked asked once ([`ASK_WHICH`]),
/// none said plainly ([`NONE_FITS`]); with nothing made or turned in it,
/// told they have none like that, never a question and never a pick. Any
/// other answer as it is.
pub fn candidates(mut told: Value, events: &[Event], now: Instant) -> Value {
    let Some(offered) = told.get_mut(CANDIDATES).and_then(Value::as_array_mut) else {
        return told;
    };
    let newest = newest(events);
    let mut first: Vec<Value> = Vec::new();
    for event in &newest {
        let spoken = made_skill::spoken_name(&event.name);
        if let Some(at) = offered
            .iter()
            .position(|offered| offered["name"].as_str() == Some(spoken.as_str()))
        {
            let mut candidate = offered.remove(at);
            candidate[IN_THIS_CONVERSATION] = json!(format!(
                "{} {}",
                match event.done {
                    Done::Made => "made",
                    Done::Off => "turned off",
                    Done::On => "turned back on",
                },
                ago(event.at, now)
            ));
            first.push(candidate);
        }
    }
    let marked = first.len();
    first.append(offered);
    *offered = first;
    let added = if marked > 0 {
        let which = if marked > 1 {
            format!(" {ASK_WHICH}")
        } else {
            String::new()
        };
        format!(
            " Those with {IN_THIS_CONVERSATION} were made or turned off or on in this \
             conversation, newest first. When their line means one of those without saying its \
             name: if only one fits, it is that one: call again with its name and never ask.\
             {which} {NONE_FITS}"
        )
    } else if newest.is_empty() {
        " None of theirs was made, turned off or turned on in this conversation: when their \
         line means one made or changed just now without saying its name, they have none like \
         that: say so plainly, in a few words, ask nothing and call nothing; never choose one \
         for them."
            .to_owned()
    } else {
        String::new()
    };
    if let Some(next) = told["next"].as_str() {
        told["next"] = json!(format!("{next}{added}"));
    }
    told
}

/// The name a `make_skill` answer `told` made, if it made one.
fn made_by(told: &Value) -> Option<&str> {
    told["made"]
        .as_str()
        .filter(|_| told.get("error").is_none())
}

/// The name a `make_skill` turn's answer `told` turned, and whether off.
fn turned_by(told: &Value) -> Option<(&str, bool)> {
    let off = told["off"].as_bool()?;
    let name = told[if off { TURN_OFF } else { TURN_ON }].as_str()?;
    Some((name, off))
}

/// A make's answer `told`, kept for `person` (the owner's line, else
/// none) at `now`.
pub fn kept_from_make(told: &Value, person: Option<&str>, now: Instant) {
    if let (Some(person), Some(name)) = (person, made_by(told)) {
        with(|kept| kept.keep(person, name, Done::Made, now));
    }
}

/// A turn's answer `told`, kept for `person` (the owner's line, else none)
/// at `now`, and given back with this conversation's [`candidates`].
pub fn kept_from_turn(told: Value, person: Option<&str>, now: Instant) -> Value {
    let Some(person) = person else {
        return candidates(told, &[], now);
    };
    if let Some((name, off)) = turned_by(&told) {
        let done = if off { Done::Off } else { Done::On };
        with(|kept| kept.keep(person, name, done, now));
        return told;
    }
    let events = with(|kept| kept.of(person, now));
    candidates(told, &events, now)
}

/// [`kept_from_make`] on the lamp, the line's memory scope `lease`: with
/// the switch on.
pub fn after_make(told: &Value, request: u64, lease: Option<&crate::memory_scope::ScopeLease>) {
    after_make_by(told, request, || crate::services::owner_of(lease));
}

/// [`after_make`] with the line's owner read by `owner`: a make that
/// waited its turn (`LAMP_MAKE_QUEUE`, #7679) has no lease when it runs,
/// and gives the owner read at its ask.
pub fn after_make_by(told: &Value, request: u64, owner: impl FnOnce() -> Option<String>) {
    if !made_skill::off_said_on() || made_by(told).is_none() {
        return;
    }
    let person = owner();
    if person.is_some() {
        eprintln!("lamp-voice: request {request}: a skill made in the owner's conversation, kept");
    }
    kept_from_make(told, person.as_deref(), Instant::now());
}

/// [`kept_from_turn`] on the lamp, the line's memory scope `lease`.
pub fn after_turn(
    told: Value,
    request: u64,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Value {
    let person = crate::services::owner_of(lease);
    let told = kept_from_turn(told, person.as_deref(), Instant::now());
    if told[CANDIDATES].as_array().is_some_and(|offered| {
        offered
            .iter()
            .any(|one| one.get(IN_THIS_CONVERSATION).is_some())
    }) {
        eprintln!(
            "lamp-voice: request {request}: a turn's candidates begin with those of this conversation"
        );
    }
    told
}

/// The made skills' folder whose index a note is held to: the product's
/// (a test's own, or none, in a test).
fn made_directory() -> Option<std::path::PathBuf> {
    #[cfg(test)]
    {
        MADE_HERE.with(|here| here.borrow().clone())
    }
    #[cfg(not(test))]
    {
        Some(std::path::PathBuf::from(made_skill::DIRECTORY))
    }
}

#[cfg(test)]
std::thread_local! {
    /// [`made_directory`] as a test of this thread sets it.
    static MADE_HERE: std::cell::RefCell<Option<std::path::PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// `events` with those of a skill no longer in the made skills' index in
/// `directory` left out: one removed since, or all of them once a factory
/// reset emptied the folder (no index, none kept).
fn still_kept(mut events: Vec<Event>, directory: &std::path::Path) -> Vec<Event> {
    let names: Vec<String> = made_skill::entries(directory)
        .iter()
        .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
        .collect();
    events.retain(|event| names.contains(&event.name));
    events
}

/// The [`note`] the owner's heard line `request` (its memory scope
/// `lease`) is told at `now`: none with the switch off, for anyone else's
/// line, or with nothing made or turned within [`KEPT_FOR`]. A skill no
/// longer in the made skills' index is left out ([`still_kept`]).
pub fn note_here(
    request: u64,
    lease: Option<&crate::memory_scope::ScopeLease>,
    now: Instant,
) -> Option<String> {
    if !made_skill::off_said_on() {
        return None;
    }
    let person = crate::services::owner_of(lease)?;
    let mut events = with(|kept| kept.of(&person, now));
    if events.is_empty() {
        return None;
    }
    if let Some(directory) = made_directory() {
        events = still_kept(events, &directory);
    }
    let note = note(&events, now)?;
    eprintln!(
        "lamp-voice: request {request}: told what was made or turned in this conversation ({} skills)",
        newest(&events).len()
    );
    Some(note)
}

/// The owner's "forget everything about me", marked by the second route's
/// guard: what the lamp made and turned with them goes too, as their voice
/// print does. Another person's forget, an unmarked one, or one whose
/// memory was not emptied, leaves it; `told` is as it was either way.
pub fn forget_with_everything(
    told: (Value, bool),
    lease: &crate::memory_scope::ScopeLease,
) -> (Value, bool) {
    if !lease.everything_marked() {
        return told;
    }
    forget_if(told, lease.admitted_person(), &crate::person::owner_ids())
}

/// [`forget_with_everything`] once its mark is judged: every conversation
/// kept is cleared when `admitted`, the line's person, is one of `owners`
/// and `told` says their memory was emptied.
pub(crate) fn forget_if(
    told: (Value, bool),
    admitted: Option<&str>,
    owners: &[String],
) -> (Value, bool) {
    let emptied =
        told.0["forgotten"] == true || told.0["note"] == json!("I was keeping nothing of them.");
    let theirs = admitted.is_some_and(|admitted| owners.iter().any(|owner| owner == admitted));
    if emptied && theirs {
        with(Conversations::clear);
        eprintln!(
            "lamp-voice: the skills made or turned in the conversation: forgotten with everything"
        );
    }
    told
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::made_remove::tests::made_here;
    use crate::made_turn::{self, Ask};
    use crate::make_skill_local::tests::keeping;

    const OWNER: &str = "owner-7660";

    /// A make's answer, as `make_skill_local::told` gives it.
    fn made_told(name: &str) -> Value {
        json!({
            "made": name,
            "on": true,
            "description": "Reminds you to stretch.",
            "uses": "",
            "next": "Say: I made it, and it's on."
        })
    }

    fn ask(name: &str, off: bool) -> Ask<'_> {
        Ask {
            name,
            off,
            request: 7660,
            heard: true,
            state: None,
        }
    }

    fn heard(person: &str, now: Instant) -> Option<String> {
        note(&with(|kept| kept.of(person, now)), now)
    }

    /// The row of #7660: a make of daily stretch reminder, then "turn off
    /// the skill you just made": the next line is told that the lamp made
    /// it just now, by name, so the model calls `turn_off` with that name;
    /// the call turns it off, and is kept as the newest.
    #[test]
    fn a_make_then_the_skill_you_just_made_is_told_by_name_and_turned_off() {
        let (root, keeping) = keeping("just-made-off");
        made_here(
            &keeping,
            &[
                ("coffee-time", true),
                ("moon-fact", true),
                ("daily-stretch-reminder", true),
            ],
        );
        let now = Instant::now();
        kept_from_make(&made_told("daily-stretch-reminder"), Some(OWNER), now);
        let told = heard(OWNER, now + Duration::from_secs(20)).expect("a note");
        assert!(
            told.contains("the lamp made daily stretch reminder (just now)"),
            "{told}"
        );
        assert!(
            told.contains(
                "To turn it off, turn it on or remove it, call make_skill with turn_off, turn_on \
                 or remove and that name"
            ),
            "{told}"
        );
        assert!(
            told.contains(
                "to use or run it, call it as for any skill, by its own function, by use_skill \
                 with that name, or by make_skill with run and that name, never a second make \
                 of it"
            ),
            "{told}"
        );
        assert!(told.contains("Never list their skills"), "{told}");
        // One told: acted on, never asked; none fitting is said plainly.
        assert!(
            told.contains("If only one fits, it means that one") && told.contains("never ask"),
            "{told}"
        );
        assert!(!told.contains(ASK_WHICH), "{told}");
        assert!(told.contains(NOTE_NONE_FITS), "{told}");
        assert!(!told.contains(NONE_FITS), "{told}");
        // "It" about the light is no skill's: never a question about skills.
        assert!(
            told.contains(
                "A line about anything else, the light, music or a timer, is answered as it is"
            ),
            "{told}"
        );
        assert!(!told.contains("coffee"), "{told}");
        // The model's call with that name turns it off.
        let mut helper = Vec::new();
        let answer = made_turn::asked(
            ask("daily stretch reminder", true),
            &keeping,
            || true,
            |name, off| {
                helper.push((name.to_owned(), off));
                Ok(())
            },
        );
        let answer = kept_from_turn(answer, Some(OWNER), now + Duration::from_secs(25));
        assert_eq!(helper, [("daily-stretch-reminder".to_owned(), true)]);
        assert_eq!(answer[TURN_OFF], "daily-stretch-reminder", "{answer}");
        let told = heard(OWNER, now + Duration::from_secs(30)).unwrap();
        assert!(
            told.contains("the lamp turned daily stretch reminder off (just now)"),
            "{told}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// "Turn it back on" right after: the newest is the one turned off, and
    /// a turn on by a name that finds none ("it") offers it first, marked
    /// as turned off in this conversation; with that name it turns on.
    #[test]
    fn turn_it_back_on_right_after_means_the_one_just_turned_off() {
        let (root, keeping) = keeping("just-made-on");
        made_here(
            &keeping,
            &[
                ("water-reminder", true),
                ("coffee-time", true),
                ("moon-fact", true),
            ],
        );
        let now = Instant::now();
        kept_from_make(&made_told("water-reminder"), Some(OWNER), now);
        let off = json!({ TURN_OFF: "water-reminder", "off": true, "next": "Say it is off." });
        let _ = kept_from_turn(off, Some(OWNER), now + Duration::from_secs(10));
        let told = heard(OWNER, now + Duration::from_secs(15)).unwrap();
        assert!(
            told.starts_with(
                "(For you to know, not to answer: in this conversation with them, newest first, \
                 the lamp turned water reminder off (just now)."
            ),
            "{told}"
        );
        // A name the model passes that finds none: theirs offered, the one
        // of this conversation first.
        let answer = made_turn::asked(ask("it", false), &keeping, || true, |_, _| panic!("turned"));
        assert!(answer[CANDIDATES].is_array(), "{answer}");
        let answer = kept_from_turn(answer, Some(OWNER), now + Duration::from_secs(16));
        let offered = answer[CANDIDATES].as_array().unwrap();
        assert_eq!(offered[0]["name"], "water reminder", "{answer}");
        assert_eq!(
            offered[0][IN_THIS_CONVERSATION], "turned off just now",
            "{answer}"
        );
        assert!(
            offered[1..]
                .iter()
                .all(|one| one.get(IN_THIS_CONVERSATION).is_none())
        );
        let next = answer["next"].as_str().unwrap();
        assert!(
            next.contains("if only one fits, it is that one: call again with its name"),
            "{next}"
        );
        assert!(!next.contains(ASK_WHICH), "{next}");
        // Its name turns it on.
        let mut helper = Vec::new();
        let answer = made_turn::asked(
            ask("water reminder", false),
            &keeping,
            || true,
            |name, off| {
                helper.push((name.to_owned(), off));
                Ok(())
            },
        );
        let _ = kept_from_turn(answer, Some(OWNER), now + Duration::from_secs(20));
        assert_eq!(helper, [("water-reminder".to_owned(), false)]);
        let told = heard(OWNER, now + Duration::from_secs(21)).unwrap();
        assert!(
            told.contains("turned water reminder back on (just now)"),
            "{told}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Two makes in the conversation: the note names both and, when both
    /// fit, asks once, naming them so one word answers it; a turn's
    /// candidates say the same.
    #[test]
    fn two_makes_in_the_conversation_ask_which_naming_both() {
        let now = Instant::now();
        kept_from_make(&made_told("water-reminder"), Some(OWNER), now);
        kept_from_make(
            &made_told("daily-stretch-reminder"),
            Some(OWNER),
            now + Duration::from_secs(200),
        );
        let told = heard(OWNER, now + Duration::from_secs(230)).unwrap();
        assert!(
            told.contains(
                "the lamp made daily stretch reminder (just now); made water reminder (4 minutes \
                 ago)"
            ),
            "{told}"
        );
        assert!(told.contains(ASK_WHICH), "{told}");
        assert!(
            told.contains("\"The water one or the stretch one?\""),
            "{told}"
        );
        let answer = json!({
            "error": crate::made_remove::NO_MADE,
            CANDIDATES: [
                {"name": "coffee time"},
                {"name": "water reminder"},
                {"name": "daily stretch reminder"}
            ],
            "next": "No skill they made or added has that name."
        });
        let answer = kept_from_turn(answer, Some(OWNER), now + Duration::from_secs(231));
        let names: Vec<&str> = answer[CANDIDATES]
            .as_array()
            .unwrap()
            .iter()
            .map(|one| one["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            ["daily stretch reminder", "water reminder", "coffee time"]
        );
        let next = answer["next"].as_str().unwrap();
        assert!(next.contains(ASK_WHICH), "{next}");
        assert!(next.contains(NONE_FITS), "{next}");
    }

    /// No make in the conversation: nothing is told, and a turn that finds
    /// none by the name is told they have none like that, said plainly,
    /// never a question and never a pick (the newest of theirs is never a
    /// guess at "the one you just made").
    #[test]
    fn no_make_in_the_conversation_asks_and_never_guesses() {
        let (root, keeping) = keeping("just-made-none");
        made_here(&keeping, &[("coffee-time", true), ("water-reminder", true)]);
        let now = Instant::now();
        assert_eq!(heard(OWNER, now), None);
        let answer = made_turn::asked(
            ask("the skill you just made", true),
            &keeping,
            || true,
            |_, _| panic!("turned"),
        );
        let answer = kept_from_turn(answer, Some(OWNER), now);
        let offered = answer[CANDIDATES].as_array().expect("theirs offered");
        assert!(
            offered
                .iter()
                .all(|one| one.get(IN_THIS_CONVERSATION).is_none())
        );
        let next = answer["next"].as_str().unwrap();
        assert!(
            next.contains("None of theirs was made, turned off or turned on in this conversation"),
            "{next}"
        );
        assert!(
            next.contains(
                "they have none like that: say so plainly, in a few words, ask nothing and call \
                 nothing"
            ),
            "{next}"
        );
        assert!(next.contains("never choose one for them"), "{next}");
        assert!(!next.contains("ask which"), "{next}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The candidates of a turn whose name found none: water reminder,
    /// daily stretch reminder and coffee time, as `made_turn` offers them.
    fn offered_three() -> Value {
        json!({
            "error": crate::made_remove::NO_MADE,
            CANDIDATES: [
                {"name": "coffee time"},
                {"name": "water reminder"},
                {"name": "daily stretch reminder"}
            ],
            "next": "No skill they made or added has that name."
        })
    }

    /// The first builder's ruling on #7715, 23:14 (a): more than one that
    /// fits is asked once, naming them so one word answers it. A make of
    /// water reminder and a turn off of daily stretch reminder (one made,
    /// two told) are both named, and the question is told once, in the note
    /// and in a turn's candidates.
    #[test]
    fn more_than_one_fitting_is_asked_once_naming_them_for_a_one_word_answer() {
        let now = Instant::now();
        kept_from_make(&made_told("water-reminder"), Some(OWNER), now);
        let off =
            json!({ TURN_OFF: "daily-stretch-reminder", "off": true, "next": "Say it is off." });
        let _ = kept_from_turn(off, Some(OWNER), now + Duration::from_secs(30));
        let told = heard(OWNER, now + Duration::from_secs(40)).unwrap();
        assert!(
            told.contains(
                "the lamp turned daily stretch reminder off (just now); made water reminder (just \
                 now)"
            ),
            "{told}"
        );
        assert_eq!(told.matches(ASK_WHICH).count(), 1, "{told}");
        assert!(ASK_WHICH.contains("ask once"), "{ASK_WHICH}");
        assert!(
            ASK_WHICH.contains("naming only those that fit so one word answers it"),
            "{ASK_WHICH}"
        );
        assert!(
            ASK_WHICH.contains("\"The water one or the stretch one?\""),
            "{ASK_WHICH}"
        );
        assert!(ASK_WHICH.contains("call nothing"), "{ASK_WHICH}");
        // Still one that fits is acted on, and none said plainly.
        assert!(told.contains("If only one fits"), "{told}");
        assert!(told.contains(NOTE_NONE_FITS), "{told}");
        let answer = kept_from_turn(offered_three(), Some(OWNER), now + Duration::from_secs(41));
        let marked = answer[CANDIDATES]
            .as_array()
            .unwrap()
            .iter()
            .filter(|one| one.get(IN_THIS_CONVERSATION).is_some())
            .count();
        assert_eq!(marked, 2, "{answer}");
        let next = answer["next"].as_str().unwrap();
        assert_eq!(next.matches(ASK_WHICH).count(), 1, "{next}");
        assert!(next.contains(NONE_FITS), "{next}");
    }

    /// The same ruling (b): with only one candidate, it isn't asked: that
    /// one is acted on. One make told: the note says to act on the one that
    /// fits and never ask, with no question; a turn's candidates with that
    /// one marked say to call it with its name, never ask.
    #[test]
    fn one_candidate_is_acted_on_never_asked() {
        let now = Instant::now();
        kept_from_make(&made_told("water-reminder"), Some(OWNER), now);
        let told = heard(OWNER, now + Duration::from_secs(5)).unwrap();
        assert!(
            told.contains(
                "If only one fits, it means that one, as if they had said its name: act on it and \
                 never ask."
            ),
            "{told}"
        );
        assert!(!told.contains(ASK_WHICH), "{told}");
        assert!(!told.contains("ask once"), "{told}");
        assert!(!told.contains("ask which"), "{told}");
        let answer = kept_from_turn(offered_three(), Some(OWNER), now + Duration::from_secs(6));
        let offered = answer[CANDIDATES].as_array().unwrap();
        assert_eq!(offered[0]["name"], "water reminder", "{answer}");
        assert!(
            offered[1..]
                .iter()
                .all(|one| one.get(IN_THIS_CONVERSATION).is_none())
        );
        let next = answer["next"].as_str().unwrap();
        assert!(
            next.contains(
                "if only one fits, it is that one: call again with its name and never ask."
            ),
            "{next}"
        );
        assert!(!next.contains(ASK_WHICH), "{next}");
        assert!(!next.contains("ask which"), "{next}");
    }

    /// The same ruling (c): if none fits, the lamp says it has no skill like
    /// that, never "which". The note and a turn's marked candidates tell it
    /// plainly, asking nothing; with nothing made or turned in the
    /// conversation, a turn's candidates say they have none like that.
    #[test]
    fn none_fitting_says_no_skill_like_that_never_which() {
        assert!(
            NONE_FITS.contains("say plainly, in a few words, that they have no skill like that"),
            "{NONE_FITS}"
        );
        assert!(NONE_FITS.contains("ask nothing"), "{NONE_FITS}");
        let now = Instant::now();
        // Nothing made or turned: no note, and a turn's candidates say
        // they have none like that, no question.
        assert_eq!(heard(OWNER, now), None);
        let answer = kept_from_turn(offered_three(), Some(OWNER), now);
        let next = answer["next"].as_str().unwrap();
        assert!(
            next.contains("they have none like that: say so plainly"),
            "{next}"
        );
        assert!(next.contains("ask nothing and call nothing"), "{next}");
        assert!(!next.contains("ask which"), "{next}");
        assert!(!next.contains(ASK_WHICH), "{next}");
        // One made: the note sends a line none of its skills fits to
        // make_skill, which looks among all of theirs (Thirteen's P2 on
        // #7715: "turn off the morning one" may mean yesterday's); only the
        // candidates, all of theirs, say none like that plainly.
        kept_from_make(&made_told("water-reminder"), Some(OWNER), now);
        let told = heard(OWNER, now + Duration::from_secs(5)).unwrap();
        assert!(told.contains(NOTE_NONE_FITS), "{told}");
        assert!(
            NOTE_NONE_FITS.contains("call make_skill with what they said"),
            "{NOTE_NONE_FITS}"
        );
        assert!(!told.contains(NONE_FITS), "{told}");
        assert!(!told.contains("ask which"), "{told}");
        let answer = kept_from_turn(offered_three(), Some(OWNER), now + Duration::from_secs(6));
        let next = answer["next"].as_str().unwrap();
        assert!(next.contains(NONE_FITS), "{next}");
        assert!(!next.contains("ask which"), "{next}");
    }

    /// Another person's make never counts for the owner: kept for them
    /// alone, and a line that isn't the owner's keeps nothing.
    #[test]
    fn another_persons_make_doesnt_count() {
        let now = Instant::now();
        kept_from_make(&made_told("guest-song"), Some("guest-7660"), now);
        kept_from_make(&made_told("room-song"), None, now);
        assert_eq!(heard(OWNER, now + Duration::from_secs(5)), None);
        let told = heard("guest-7660", now + Duration::from_secs(5)).unwrap();
        assert!(
            told.contains("guest song") && !told.contains("room song"),
            "{told}"
        );
        // A turn's candidates for a line that isn't the owner's are not
        // marked as this conversation's.
        let answer = json!({
            CANDIDATES: [{"name": "guest song"}],
            "next": "No skill they made or added has that name."
        });
        let answer = kept_from_turn(answer, Some(OWNER), now + Duration::from_secs(6));
        assert!(
            answer[CANDIDATES][0].get(IN_THIS_CONVERSATION).is_none(),
            "{answer}"
        );
    }

    /// Heron's P3 on #7715: a make is told for [`KEPT_FOR`] after it, and
    /// the owner's lines in between never carry it on.
    #[test]
    fn a_make_is_told_for_ten_minutes_after_it_never_carried_on_by_lines() {
        let now = Instant::now();
        kept_from_make(&made_told("water-reminder"), Some(OWNER), now);
        for minute in 1..10 {
            assert!(
                heard(OWNER, now + Duration::from_secs(60 * minute)).is_some(),
                "{minute}"
            );
        }
        assert_eq!(heard(OWNER, now + KEPT_FOR + Duration::from_secs(1)), None);
        // A turn later keeps the turn for its own ten minutes, never the
        // make's.
        let off = json!({ TURN_OFF: "water-reminder", "off": true, "next": "Say it is off." });
        let _ = kept_from_turn(off, Some(OWNER), now + Duration::from_secs(300));
        let told = heard(OWNER, now + KEPT_FOR + Duration::from_secs(60)).unwrap();
        assert!(told.contains("turned water reminder off"), "{told}");
        assert!(!told.contains("made water reminder"), "{told}");
        assert_eq!(
            heard(
                OWNER,
                now + Duration::from_secs(300) + KEPT_FOR + Duration::from_secs(1)
            ),
            None
        );
    }

    /// A failed make keeps nothing, and the switch off tells nothing.
    #[test]
    fn a_failed_make_keeps_nothing_and_the_switch_off_tells_nothing() {
        let now = Instant::now();
        kept_from_make(
            &json!({"error": "the skill couldn't be made: no", "next": "Say so."}),
            Some(OWNER),
            now,
        );
        assert_eq!(heard(OWNER, now), None);
        made_skill::set_off_said_here(Some(false));
        crate::person::set_private_room_for_test(Some(true));
        kept_from_make(
            &made_told("water-reminder"),
            crate::services::owner_of(None).as_deref(),
            now,
        );
        assert_eq!(note_here(1, None, now), None);
        made_skill::set_off_said_here(Some(true));
        assert!(note_here(1, None, now).is_some());
        made_skill::set_off_said_here(None);
        crate::person::set_private_room_for_test(None);
    }

    /// Heron's P3 on #7715: a skill removed since its make is never named,
    /// and with the made skills' folder emptied (a factory reset), none is.
    #[test]
    fn a_skill_removed_since_is_not_named() {
        let (root, keeping) = keeping("just-made-removed");
        made_here(&keeping, &[("coffee-time", true), ("water-reminder", true)]);
        let now = Instant::now();
        kept_from_make(&made_told("daily-stretch-reminder"), Some(OWNER), now);
        kept_from_make(&made_told("water-reminder"), Some(OWNER), now);
        let events = still_kept(with(|kept| kept.of(OWNER, now)), &keeping.directory);
        let told = note(&events, now).unwrap();
        assert!(told.contains("made water reminder"), "{told}");
        assert!(!told.contains("daily stretch"), "{told}");
        // Through the note the line is told.
        made_skill::set_off_said_here(Some(true));
        crate::person::set_private_room_for_test(Some(true));
        let owner = crate::services::owner_of(None).unwrap();
        kept_from_make(&made_told("daily-stretch-reminder"), Some(&owner), now);
        MADE_HERE.with(|here| *here.borrow_mut() = Some(keeping.directory.clone()));
        assert_eq!(note_here(1, None, now), None);
        kept_from_make(&made_told("water-reminder"), Some(&owner), now);
        let told = note_here(1, None, now).unwrap();
        assert!(
            told.contains("made water reminder") && !told.contains("daily stretch"),
            "{told}"
        );
        // The folder emptied: none named.
        std::fs::remove_dir_all(&keeping.directory).unwrap();
        assert_eq!(note_here(1, None, now), None);
        MADE_HERE.with(|here| *here.borrow_mut() = None);
        made_skill::set_off_said_here(None);
        crate::person::set_private_room_for_test(None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The product's conversations are one for the whole worker: what one
    /// thread keeps (a tool call's), another reads (a later line's).
    #[test]
    fn what_one_thread_keeps_another_reads() {
        let person = format!("cross-thread-{}", std::process::id());
        let now = Instant::now();
        let keeper = person.clone();
        std::thread::spawn(move || {
            shared(|kept| kept.keep(&keeper, "water-reminder", Done::Made, now));
        })
        .join()
        .unwrap();
        let reader = person.clone();
        let read = std::thread::spawn(move || shared(|kept| kept.of(&reader, now)))
            .join()
            .unwrap();
        assert_eq!(read.len(), 1, "{read:?}");
        assert_eq!(read[0].name, "water-reminder");
        assert_eq!(read[0].done, Done::Made);
        shared(|kept| kept.talks.retain(|talk| talk.person != person));
    }

    /// The owner's "forget everything" clears what the lamp made and turned
    /// with them; another person's, or one that emptied nothing, leaves it.
    #[test]
    fn forget_everything_clears_it() {
        let now = Instant::now();
        kept_from_make(&made_told("water-reminder"), Some(OWNER), now);
        let emptied = || (json!({"forgotten": true}), true);
        let owners = [OWNER.to_owned()];
        let _ = forget_if(emptied(), Some("guest-7660"), &owners);
        let _ = forget_if((json!({"forgotten": false}), true), Some(OWNER), &owners);
        assert!(heard(OWNER, now).is_some());
        let told = forget_if(emptied(), Some(OWNER), &owners);
        assert_eq!(told, emptied());
        assert_eq!(heard(OWNER, now), None);
    }
}
