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
//!   person's make never counts. While their conversation lasts (no gap of
//!   [`KEPT_FOR`] between their lines, as the thread's own
//!   `thread::REMEMBERED_FOR`), each heard line of theirs is told one short
//!   note ([`note`]): what the lamp made or turned with them in it, newest
//!   first, by name. Which one a line means is the model's to judge, never
//!   a list of words (the owner's ruling, 13 Nov): the note says the newest
//!   that fits, and to ask which, naming them, when more than one was made
//!   in this conversation. In the per-turn notes, never in `make_skill`'s
//!   description, which stays within its bytes.
//! - **Candidates, this conversation's first.** A turn whose name found none
//!   offers theirs (`made_turn::CANDIDATES`); those made or turned in this
//!   conversation come first, each marked [`IN_THIS_CONVERSATION`], and the
//!   model is told so ([`candidates`]). With none made or turned in this
//!   conversation, it is told that too, and to ask which they mean rather
//!   than pick one: a line that means one made just now never gets a guess.
//!
//! No file, no round trip: a few names in this process's memory, and the
//! made skills' index read only to leave out one no longer kept. The
//! journal names no skill.
use crate::made_skill;
use crate::made_turn::{CANDIDATES, TURN_OFF, TURN_ON};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

/// How long a conversation lasts with no line of the person's: as the
/// thread's own (`thread::REMEMBERED_FOR`), the second route's
/// conversation quiet this long is over.
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
    last: Instant,
    /// Oldest first.
    events: Vec<Event>,
}

/// Each person's conversation: what the lamp made and turned in it.
#[derive(Default)]
pub struct Conversations {
    talks: Vec<Talk>,
}

impl Conversations {
    /// Conversations quiet longer than [`KEPT_FOR`] at `now`, dropped.
    fn ended(&mut self, now: Instant) {
        self.talks
            .retain(|talk| now.saturating_duration_since(talk.last) <= KEPT_FOR);
    }

    /// `name` `done` in `person`'s conversation at `now`.
    pub fn keep(&mut self, person: &str, name: &str, done: Done, now: Instant) {
        self.ended(now);
        let at = match self.talks.iter().position(|talk| talk.person == person) {
            Some(at) => at,
            None => {
                if self.talks.len() >= PEOPLE
                    && let Some(oldest) =
                        (0..self.talks.len()).min_by_key(|at| self.talks[*at].last)
                {
                    self.talks.remove(oldest);
                }
                self.talks.push(Talk {
                    person: person.to_owned(),
                    last: now,
                    events: Vec::new(),
                });
                self.talks.len() - 1
            }
        };
        let talk = &mut self.talks[at];
        talk.last = now;
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

    /// `person`'s line heard at `now`: their conversation goes on, and what
    /// was made and turned in it, oldest first (none once it ended).
    pub fn heard(&mut self, person: &str, now: Instant) -> Vec<Event> {
        self.ended(now);
        let Some(talk) = self.talks.iter_mut().find(|talk| talk.person == person) else {
            return Vec::new();
        };
        talk.last = now;
        talk.events.clone()
    }

    /// What was made and turned in `person`'s conversation at `now`, oldest
    /// first, without carrying it on.
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

/// This process's conversations (a test's own thread's in a test).
fn with<R>(each: impl FnOnce(&mut Conversations) -> R) -> R {
    #[cfg(test)]
    {
        let _ = &CONVERSATIONS;
        FOR_TEST.with(|kept| each(&mut kept.borrow_mut()))
    }
    #[cfg(not(test))]
    {
        let mut kept = CONVERSATIONS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        each(&mut kept)
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

/// The skills made in the conversation, newest first, each once.
fn made(events: &[Event]) -> Vec<String> {
    let mut made: Vec<String> = Vec::new();
    for event in events.iter().rev().filter(|event| event.done == Done::Made) {
        if !made.contains(&event.name) {
            made.push(event.name.clone());
        }
    }
    made
}

/// What the model is told to do when more than one skill was made in the
/// conversation (`made`, newest first).
fn ask_which(made: &[String]) -> String {
    format!(
        "More than one skill was made in this conversation: when their line means one made in \
         it without saying which, ask once which they mean, naming them ({}), and call nothing.",
        crate::made_remove::said_or(&made[..made.len().min(SAID_MOST)])
    )
}

/// The note a heard line of the person's is told at `now` of what the lamp
/// made and turned in their conversation (`events`, oldest first): none
/// when nothing was.
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
    let made = made(events);
    let which = if made.len() > 1 {
        format!(" {}", ask_which(&made))
    } else {
        String::new()
    };
    Some(format!(
        "(For you to know, not to answer: in this conversation with them, newest first, the \
         lamp {}. When their line means a skill of theirs without saying its name, it means the \
         newest of these that fits what they ask: call make_skill with that name, as if they \
         had said it, and never list their skills for it.{which} When none of these fits, ask \
         which skill they mean.)",
        done.join("; ")
    ))
}

/// A turn's answer `told` with its candidates (`made_turn::CANDIDATES`),
/// those made or turned in the conversation (`events`, oldest first)
/// first, each marked [`IN_THIS_CONVERSATION`], and the model told so; with
/// nothing made or turned in it, told to ask rather than pick one. Any
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
    let marked = !first.is_empty();
    first.append(offered);
    *offered = first;
    let added = if marked {
        let made = made(events);
        let which = if made.len() > 1 {
            format!(" {}", ask_which(&made))
        } else {
            String::new()
        };
        format!(
            " Those with {IN_THIS_CONVERSATION} were made or turned off or on in this \
             conversation, newest first: when their line means one of those without saying its \
             name, choose the newest that fits.{which}"
        )
    } else if newest.is_empty() {
        " None of theirs was made, turned off or turned on in this conversation: when their \
         line means one made or changed just now without saying its name, ask which skill they \
         mean and call nothing; never choose one for them."
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
    if !made_skill::off_said_on() || made_by(told).is_none() {
        return;
    }
    let person = crate::services::owner_of(lease);
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

/// The [`note`] the owner's heard line `request` (its memory scope
/// `lease`) is told at `now`, their conversation carried on: none with the
/// switch off, for anyone else's line, or with nothing made or turned in
/// it. A skill no longer in the made skills' index is left out.
pub fn note_here(
    request: u64,
    lease: Option<&crate::memory_scope::ScopeLease>,
    now: Instant,
) -> Option<String> {
    if !made_skill::off_said_on() {
        return None;
    }
    let person = crate::services::owner_of(lease)?;
    let mut events = with(|kept| kept.heard(&person, now));
    if events.is_empty() {
        return None;
    }
    if let Some(keeping) = made_skill::Keeping::when(true)
        && keeping.directory.join(made_skill::INDEX).is_file()
    {
        let names: Vec<String> = made_skill::entries(&keeping.directory)
            .iter()
            .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
            .collect();
        events.retain(|event| names.contains(&event.name));
    }
    let note = note(&events, now)?;
    eprintln!(
        "lamp-voice: request {request}: told what was made or turned in this conversation ({} skills)",
        newest(&events).len()
    );
    Some(note)
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
        note(&with(|kept| kept.heard(person, now)), now)
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
        assert!(told.contains("call make_skill with that name"), "{told}");
        assert!(told.contains("never list their skills"), "{told}");
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
        assert!(next.contains("choose the newest that fits"), "{next}");
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

    /// Two makes in the conversation: the note names both and asks which,
    /// naming both, when a line means one made without saying which; a
    /// turn's candidates say the same.
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
        assert!(
            told.contains("ask once which they mean, naming them (daily stretch reminder or water reminder), and call nothing"),
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
        assert!(
            next.contains("naming them (daily stretch reminder or water reminder)"),
            "{next}"
        );
    }

    /// No make in the conversation: nothing is told, and a turn that finds
    /// none by the name is told to ask which they mean, never to pick one
    /// (the newest of theirs is never a guess at "the one you just made").
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
            next.contains("ask which skill they mean and call nothing"),
            "{next}"
        );
        assert!(next.contains("never choose one for them"), "{next}");
        let _ = std::fs::remove_dir_all(&root);
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

    /// A conversation quiet for [`KEPT_FOR`] is over: nothing is told after
    /// it; lines within it carry it on.
    #[test]
    fn a_conversation_quiet_too_long_is_over() {
        let now = Instant::now();
        kept_from_make(&made_told("water-reminder"), Some(OWNER), now);
        let later = now + KEPT_FOR - Duration::from_secs(1);
        assert!(heard(OWNER, later).is_some());
        assert!(heard(OWNER, later + KEPT_FOR - Duration::from_secs(1)).is_some());
        assert_eq!(
            heard(OWNER, later + KEPT_FOR * 2 + Duration::from_secs(2)),
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
}
