//! A shared spoken stop's scope, read from the person's own words (#7473).
//!
//! On lamp-06c1 (19 Nov, the ROW on #7473) the model called the shared
//! wellbeing stop (`wellbeing_nudges`, its `how` snooze, off or on) for
//! "stop the water nudges" and "stop the nudges" with no `kind` and no
//! `all`, 0 of 7, so the last nudge alone was stopped, and the reply said
//! more than was. The owner's call (19 Nov): **the lamp reads the words.**
//! When a shared stop offering kinds is called with snooze, off or on, the
//! line the person said for that turn sets its scope when it gives a clear
//! one, read strictly so an ordinary sentence never picks one (the HOLD on
//! #7473, c7007369316):
//!
//! - **What the nudges are about** ([`ABOUT_WORDS`]): a kind's word
//!   directly after "about", "about my" or "for my" ("stop the nudges about
//!   drinking water"), read first. A bare "for" is a time ("for this
//!   evening").
//! - **The nudges in general** ([`ALL_WORDS`]): every one, `all`, whatever
//!   other kind word is near ("snooze everything, I need to focus").
//! - **A kind naming the nudges** ([`KIND_WORDS`]): a kind's word directly
//!   before "nudge", "reminder" or "one" ([`NOUNS`]), or before another
//!   kind's that is ("the water and eye nudges"): those kinds only, each
//!   one of the function's own kinds (its enabled skills' `[stop] kind`). A
//!   kind word anywhere else ("while I eat", "this evening", "I can't stand
//!   these") names nothing.
//! - **Anything else**, "it", "that one" and "this one" among it, or a
//!   kind left out ("all the nudges except water", [`BUT_WORDS`]): the
//!   model's call as it sent it (`kind`, `all`, or neither, the last). So
//!   the model's `all` stays all unless a kind names the nudges.
//!
//! Only kinds naming the nudges are read, so room talk merged into a Live
//! line (`Thread::hold_line` falls back to the newest words heard) adds a
//! kind only as "the dinner nudges" would; the journal line names it.
//!
//! A clear scope replaces the model's; a call the person did not speak is
//! as the model made it. Whole words, any case, **English only for now**: a
//! line in another language reads as nothing, so a Vietnamese lamp keeps
//! the model's call until the owner has reviewed the map and its words are
//! added. `lamp-skills` does what the call then carries.

use lamp_gemini::{Tool, ToolCall};
use serde_json::{Value, json};

/// Each kind, and the words or phrases naming it: the owner's list (19 Nov,
/// on #7473), the only words read. A kind is read only when the function
/// offers it (an enabled skill's `[stop] kind`), and only before one of
/// [`NOUNS`].
pub const KIND_WORDS: [(&str, &[&str]); 9] = [
    ("water", &["water", "drink", "drinking", "sip"]),
    ("eyes", &["eye", "eyes", "screen"]),
    ("stand-up", &["stand", "standing", "stand up"]),
    ("break", &["break", "breaks"]),
    ("sit-back", &["sit back", "lean"]),
    ("posture", &["posture", "slouch"]),
    ("wind-down", &["wind down", "bedtime", "evening"]),
    ("meal", &["meal", "eat", "food", "lunch", "dinner"]),
    ("focus", &["focus"]),
];

/// The words a kind's word must come directly before to name the nudges
/// ("the water nudges", "the eye reminders", "the stand-up one").
pub const NOUNS: [&str; 6] = ["nudge", "nudges", "reminder", "reminders", "one", "ones"];

/// The words that join two kinds before one noun ("water and eye nudges").
const JOINS: [&str; 2] = ["and", "or"];

/// Phrases naming the nudges in general: every one of them, beating any
/// kind word near them. "the nudges" counts only with no kind's word
/// before "nudges" (then it is "the water nudges", not this).
pub const ALL_WORDS: [&str; 6] = [
    "the nudges",
    "all nudges",
    "all the nudges",
    "all of them",
    "everything",
    "all reminders",
];

/// The shared stop's yes-or-no for every one of its skills
/// (`lamp_skills::STOP_ALL`).
pub const ALL: &str = "all";

/// What the person's words name, when they name a scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// These kinds, in [`KIND_WORDS`]' order.
    Kinds(Vec<String>),
    /// Every one of them.
    All,
}

/// `heard`'s words: lowercase, split at anything but a letter, a digit or
/// an apostrophe ("stand-up" is "stand", "up").
fn words(heard: &str) -> Vec<String> {
    heard
        .to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether `phrase`'s words come one after another in `words` at `start`.
fn at(words: &[String], start: usize, phrase: &str) -> bool {
    let phrase: Vec<&str> = phrase.split(' ').collect();
    words
        .get(start..start + phrase.len())
        .is_some_and(|run| run.iter().zip(&phrase).all(|(word, said)| word == said))
}

/// Whether `phrase`'s words come one after another anywhere in `words`.
fn has(words: &[String], phrase: &str) -> bool {
    (0..words.len()).any(|start| at(words, start, phrase))
}

/// The kind whose word (of those `kinds` offers) begins at `start`, and the
/// index just past it; the longest word first ("stand up" over "stand").
fn kind_at(words: &[String], start: usize, kinds: &[&str]) -> Option<(&'static str, usize)> {
    KIND_WORDS
        .iter()
        .filter(|(kind, _)| kinds.contains(kind))
        .flat_map(|(kind, said)| said.iter().map(move |said| (*kind, *said)))
        .filter(|(_, said)| at(words, start, said))
        .max_by_key(|(_, said)| said.split(' ').count())
        .map(|(kind, said)| (kind, start + said.split(' ').count()))
}

/// Whether the kind word ending at `end` names the nudges: a noun next, or
/// another kind's word (after "and" or "or", or straight after, as a comma
/// leaves it) that does.
fn qualifies(words: &[String], end: usize, kinds: &[&str]) -> bool {
    let Some(next) = words.get(end) else {
        return false;
    };
    if NOUNS.contains(&next.as_str()) {
        return true;
    }
    let after = if JOINS.contains(&next.as_str()) {
        end + 1
    } else {
        end
    };
    kind_at(words, after, kinds).is_some_and(|(_, past)| qualifies(words, past, kinds))
}

/// The words that leave a kind out ("all the nudges except water",
/// "everything but the water one", "not the eye ones").
pub const BUT_WORDS: [&str; 3] = ["except", "but", "not"];

/// How many words after one of [`BUT_WORDS`] a kind's word is read as left
/// out ("but the water one": two).
const BUT_REACH: usize = 3;

/// Whether one of [`BUT_WORDS`] comes within [`BUT_REACH`] words before an
/// offered kind's word: the line leaves a kind out, which the call cannot
/// say, so the model's call stands.
fn leaves_a_kind_out(words: &[String], kinds: &[&str]) -> bool {
    words.iter().enumerate().any(|(but, word)| {
        BUT_WORDS.contains(&word.as_str())
            && (but + 1..=but + BUT_REACH).any(|start| kind_at(words, start, kinds).is_some())
    })
}

/// The words a kind's word may come directly after to name the nudges'
/// subject ("the nudges about drinking water", "the reminders about my
/// eyes", "the nudges for my posture"). A bare "for" is not one: "for this
/// evening" and "for lunch" are times.
pub const ABOUT_WORDS: [&str; 3] = ["about my", "about", "for my"];

/// The kinds whose words come directly after one of [`ABOUT_WORDS`], and
/// after those through "and", "or" or straight on, a "my" between
/// ("about drinking water", "about water and my eyes").
fn kinds_about(words: &[String], kinds: &[&str]) -> Vec<&'static str> {
    let mut named: Vec<&'static str> = Vec::new();
    for start in 0..words.len() {
        let Some(said) = ABOUT_WORDS.iter().find(|said| at(words, start, said)) else {
            continue;
        };
        let mut next = start + said.split(' ').count();
        while let Some((kind, past)) = kind_at(words, next, kinds) {
            if !named.contains(&kind) {
                named.push(kind);
            }
            next = match words.get(past) {
                Some(join) if JOINS.contains(&join.as_str()) => past + 1,
                _ => past,
            };
            // "about water and my eyes": a "my" before the next kind too.
            if words.get(next).is_some_and(|word| word == "my") {
                next += 1;
            }
        }
    }
    named
}

/// What `heard` names, of a stop offering `kinds`: the kinds the nudges
/// are about first ([`ABOUT_WORDS`]), then the nudges in general, then the
/// kinds naming the nudges. `None` when it gives no clear
/// scope ("it", "that one" and a kind word elsewhere among it), or leaves
/// a kind out ([`BUT_WORDS`]).
pub fn read(heard: &str, kinds: &[&str]) -> Option<Scope> {
    let words = words(heard);
    if leaves_a_kind_out(&words, kinds) {
        return None;
    }
    let mut named: Vec<&str> = kinds_about(&words, kinds);
    if named.is_empty() && ALL_WORDS.iter().any(|said| has(&words, said)) {
        return Some(Scope::All);
    }
    for start in 0..words.len() {
        if let Some((kind, past)) = kind_at(&words, start, kinds)
            && qualifies(&words, past, kinds)
            && !named.contains(&kind)
        {
            named.push(kind);
        }
    }
    if named.is_empty() {
        return None;
    }
    Some(Scope::Kinds(
        KIND_WORDS
            .iter()
            .map(|(kind, _)| *kind)
            .filter(|kind| named.contains(kind))
            .map(str::to_owned)
            .collect(),
    ))
}

/// Whether `call` may be a stop's snooze, off or on: its `how` one of
/// them. Read before anything else, so another call reads no file.
pub fn may_be_stop(call: &ToolCall) -> bool {
    call.args
        .get(crate::use_skill::STOP_CHOICE)
        .and_then(Value::as_str)
        .is_some_and(|how| crate::use_skill::STOP_WAYS.contains(&how))
}

/// `call` of a shared stop among `skills` offering kinds, its scope set by
/// `heard` ([`read`]), with a journal line saying what the words set and
/// what the model sent; `None` when it is no such call, nothing was heard,
/// the words give no clear scope, or the model already sent the same.
pub fn scoped(call: &ToolCall, skills: &[Tool], heard: Option<&str>) -> Option<(ToolCall, String)> {
    if !may_be_stop(call) {
        return None;
    }
    let skill = skills.iter().find(|skill| skill.name == call.name)?;
    let (kinds, _) = crate::use_skill::kind(skill)?;
    let scope = read(heard?, &kinds)?;
    let mut args = call.args.as_object()?.clone();
    args.remove(crate::use_skill::KIND);
    args.remove(ALL);
    match &scope {
        Scope::Kinds(named) => {
            let kind = match named.as_slice() {
                [one] => json!(one),
                many => json!(many),
            };
            args.insert(crate::use_skill::KIND.to_owned(), kind);
        }
        Scope::All => {
            args.insert(ALL.to_owned(), json!(true));
        }
    }
    let args = Value::Object(args);
    if args == call.args {
        return None;
    }
    let shown = |args: &Value, key: &str| {
        args.get(key)
            .map_or_else(|| "none".to_owned(), Value::to_string)
    };
    let line = format!(
        "the heard words set kind={} all={} (the model sent kind={} all={})",
        shown(&args, crate::use_skill::KIND),
        shown(&args, ALL),
        shown(&call.args, crate::use_skill::KIND),
        shown(&call.args, ALL),
    );
    Some((
        ToolCall {
            id: call.id.clone(),
            name: call.name.clone(),
            args,
        },
        line,
    ))
}

pub fn heard_words(request: u64, heard: bool) -> Option<String> {
    if !heard {
        return None;
    }
    #[cfg(test)]
    {
        if let Some(set) = HEARD_HERE.with(|here| here.borrow().clone()) {
            return set;
        }
    }
    crate::thread::THREAD
        .lock()
        .ok()?
        .heard_in_own_line(request)
}

#[cfg(test)]
thread_local! {
    static HEARD_HERE: std::cell::RefCell<Option<Option<String>>> =
        const { std::cell::RefCell::new(None) };
}

/// A test's heard words, on its own thread; `None` reads the thread's.
#[cfg(test)]
pub(crate) fn set_heard_here(heard: Option<Option<&str>>) {
    HEARD_HERE.with(|here| *here.borrow_mut() = heard.map(|heard| heard.map(str::to_owned)));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wellbeing stop as its tools file says it, with these kinds.
    fn stop(kinds: &[&str]) -> Tool {
        Tool {
            name: "wellbeing_nudges".into(),
            description: "When someone asks you to stop a wellbeing nudge.".into(),
            parameters: Some(json!({"type":"object","properties":{
                "all":{"type":"boolean"},
                "how":{"type":"string","enum":["snooze","off","on","just_drank"]},
                "kind":{"type":"string","enum":kinds}}})),
            silent: false,
        }
    }

    const EVERY: [&str; 9] = [
        "break",
        "eyes",
        "focus",
        "meal",
        "posture",
        "sit-back",
        "stand-up",
        "water",
        "wind-down",
    ];

    fn call(args: Value) -> ToolCall {
        ToolCall {
            id: "c1".into(),
            name: "wellbeing_nudges".into(),
            args,
        }
    }

    fn set(heard: &str, args: Value) -> Value {
        scoped(&call(args.clone()), &[stop(&EVERY)], Some(heard))
            .map_or(args, |(call, _)| call.args)
    }

    /// Each row of the word map names its kind, alone, before each noun.
    #[test]
    fn each_word_before_a_noun_names_its_kind() {
        for (kind, said) in KIND_WORDS {
            for word in said {
                for noun in NOUNS {
                    let heard = format!("Lamp, stop the {word} {noun} please.");
                    assert_eq!(
                        read(&heard, &EVERY),
                        Some(Scope::Kinds(vec![kind.to_owned()])),
                        "{heard}"
                    );
                    assert_eq!(
                        set(&heard, json!({"how":"off"})),
                        json!({"how":"off","kind":kind}),
                        "{heard}"
                    );
                }
            }
        }
        // Any case, and a hyphen as a space.
        assert_eq!(
            read("STOP THE WATER NUDGES", &EVERY),
            Some(Scope::Kinds(vec!["water".into()]))
        );
        assert_eq!(
            read("snooze the stand-up one", &EVERY),
            Some(Scope::Kinds(vec!["stand-up".into()]))
        );
    }

    /// The HOLD on #7473: a kind word not naming the nudges names nothing,
    /// and whole words only.
    #[test]
    fn a_kind_word_elsewhere_names_nothing() {
        for heard in [
            "I need to focus",
            "stop them while I eat",
            "not this evening",
            "I'm taking a break",
            "I can't stand these reminders",
            "my eyesight nudges",
            "the breakfast reminders",
            "the standard ones",
            "wind the clock down",
            "water the plants, then the reminder",
        ] {
            assert_eq!(read(heard, &EVERY), None, "{heard}");
        }
    }

    /// A kind no enabled skill has is not read.
    #[test]
    fn a_kind_not_offered_is_not_read() {
        assert_eq!(read("stop the focus nudges", &["water", "eyes"]), None);
        assert_eq!(
            read("stop the eye and focus nudges", &["water", "eyes"]),
            None
        );
    }

    /// The nudges in general: every one of them, over the model's kind.
    #[test]
    fn the_nudges_in_general_are_all() {
        for heard in [
            "stop the nudges",
            "turn off all nudges",
            "snooze all the nudges for today",
            "stop all of them",
            "no more of everything",
            "stop all reminders",
        ] {
            assert_eq!(read(heard, &EVERY), Some(Scope::All), "{heard}");
            assert_eq!(
                set(heard, json!({"how":"off"})),
                json!({"how":"off","all":true}),
                "{heard}"
            );
        }
        assert_eq!(
            set("stop the nudges", json!({"how":"snooze","kind":"eyes"})),
            json!({"how":"snooze","all":true})
        );
    }

    /// The HOLD's six lines on #7473 (c7007369316), each with its scope.
    /// "Turn it all off" reads as nothing ("all off" is no phrase of the
    /// map), so the model's call stands: its `all` kept, never taken away.
    #[test]
    fn the_holds_six_lines_get_their_scope() {
        for heard in [
            "Snooze everything for an hour, I need to focus",
            "Stop the nudges while I eat",
            "Turn off the nudges this evening",
            "I'm taking a break, stop the nudges",
        ] {
            assert_eq!(read(heard, &EVERY), Some(Scope::All), "{heard}");
            assert_eq!(
                set(heard, json!({"how":"snooze","kind":"focus"})),
                json!({"how":"snooze","all":true}),
                "{heard}"
            );
        }
        for (heard, sent) in [
            ("I can't stand these reminders", json!({"how":"off"})),
            (
                "I can't stand these reminders",
                json!({"how":"off","kind":"water"}),
            ),
            ("Turn it all off", json!({"how":"off","all":true})),
            ("Turn it all off", json!({"how":"off"})),
        ] {
            assert_eq!(read(heard, &EVERY), None, "{heard}");
            assert_eq!(set(heard, sent.clone()), sent, "{heard}");
        }
    }

    /// The second HOLD's lines on #7473 (c7007417135), each with its scope.
    #[test]
    fn the_second_holds_lines_get_their_scope() {
        for heard in [
            "stop all the nudges this evening",
            "give me a break, turn off the nudges",
            "turn off the nudges, I'm standing up to leave",
            "remind me to drink water at five and stop the nudges",
        ] {
            assert_eq!(read(heard, &EVERY), Some(Scope::All), "{heard}");
            assert_eq!(
                set(heard, json!({"how":"off"})),
                json!({"how":"off","all":true}),
                "{heard}"
            );
        }
        for heard in [
            "snooze them, I'm going to eat",
            "stop nudging me, I need to focus",
            "stop all the nudges except water",
            "everything but the water one",
            "turn off the nudges, not the eye ones",
            "Turn it all off",
            "stop all of it",
        ] {
            assert_eq!(read(heard, &EVERY), None, "{heard}");
            for sent in [
                json!({"how":"off"}),
                json!({"how":"off","all":true}),
                json!({"how":"off","kind":"water"}),
            ] {
                assert_eq!(set(heard, sent.clone()), sent, "{heard}");
            }
        }
    }

    /// The third HOLD on #7473 (c7008469040): a kind the nudges are about
    /// counts, before the nudges in general; a bare "for" is a time.
    #[test]
    fn a_kind_the_nudges_are_about_counts() {
        for (heard, kind) in [
            ("stop the nudges about drinking water", json!("water")),
            ("turn off the nudges for my posture", json!("posture")),
            ("snooze the reminders about my eyes", json!("eyes")),
            (
                "stop the nudges about water and my eyes",
                json!(["water", "eyes"]),
            ),
        ] {
            assert_eq!(
                set(heard, json!({"how":"off","all":true})),
                json!({"how":"off","kind":kind}),
                "{heard}"
            );
        }
        for heard in [
            "stop the nudges for this evening",
            "snooze the nudges for lunch",
            "stop the nudges, I'm on about the dinner plan",
        ] {
            // "about the dinner": "the" is no kind, so nothing is about it.
            assert_eq!(read(heard, &EVERY), Some(Scope::All), "{heard}");
        }
    }

    /// "it", "that one", "this one" never override the model: its `kind`,
    /// its `all`, or neither (the last) stands.
    #[test]
    fn that_one_or_it_keeps_the_models_call() {
        for heard in ["stop that one", "snooze this one", "turn it off"] {
            assert_eq!(read(heard, &EVERY), None, "{heard}");
            for sent in [
                json!({"how":"off"}),
                json!({"how":"off","all":true}),
                json!({"how":"off","kind":"eyes"}),
            ] {
                assert_eq!(set(heard, sent.clone()), sent, "{heard}");
            }
        }
    }

    /// Two kinds naming the nudges: both, as a list.
    #[test]
    fn two_kinds_named_are_both() {
        assert_eq!(
            read("snooze the water and eye nudges", &EVERY),
            Some(Scope::Kinds(vec!["water".into(), "eyes".into()]))
        );
        assert_eq!(
            set("turn off the eye, water reminders", json!({"how":"off"})),
            json!({"how":"off","kind":["water","eyes"]})
        );
        assert_eq!(
            set(
                "stop the lunch nudges and the posture ones",
                json!({"how":"off"})
            ),
            json!({"how":"off","kind":["posture","meal"]})
        );
    }

    /// Nothing clear: the model's call as it is; with neither, the last.
    #[test]
    fn no_match_is_the_models_call() {
        assert_eq!(read("just snooze them", &EVERY), None);
        assert!(
            scoped(
                &call(json!({"how":"snooze"})),
                &[stop(&EVERY)],
                Some("just snooze them")
            )
            .is_none()
        );
        assert_eq!(
            set("just snooze them", json!({"how":"snooze","all":true})),
            json!({"how":"snooze","all":true})
        );
        // Not heard (a typed line, the lamp's own): as the model sent it.
        assert!(scoped(&call(json!({"how":"off"})), &[stop(&EVERY)], None).is_none());
        // Not English: nothing read, the model's call stands.
        assert_eq!(read("tắt nhắc uống nước", &EVERY), None);
    }

    /// A clear scope replaces the model's wrong kind, or its all.
    #[test]
    fn the_words_override_the_models_kind() {
        assert_eq!(
            set("stop the water nudges", json!({"how":"off","kind":"eyes"})),
            json!({"how":"off","kind":"water"})
        );
        assert_eq!(
            set(
                "snooze the water nudges",
                json!({"how":"snooze","all":true})
            ),
            json!({"how":"snooze","kind":"water"})
        );
        let (_, line) = scoped(
            &call(json!({"how":"off","kind":"eyes"})),
            &[stop(&EVERY)],
            Some("stop the water nudges"),
        )
        .unwrap();
        assert_eq!(
            line,
            "the heard words set kind=\"water\" all=none (the model sent kind=\"eyes\" all=none)"
        );
    }

    /// Only a stop offering kinds, and only snooze, off or on: a done way,
    /// another skill, or a stop with no kinds is as the model sent it.
    #[test]
    fn only_a_kinded_stops_snooze_off_or_on_is_read() {
        let heard = Some("stop the water nudges");
        assert!(
            scoped(
                &call(json!({"how":"just_drank"})),
                &[stop(&EVERY)],
                Some("I just had some water")
            )
            .is_none()
        );
        let mut plain = stop(&EVERY);
        plain.parameters = Some(json!({"type":"object","properties":{
            "all":{"type":"boolean"},
            "how":{"type":"string","enum":["snooze","off","on"]}}}));
        assert!(scoped(&call(json!({"how":"off"})), &[plain], heard).is_none());
        let mut other = stop(&EVERY);
        other.name = "dance".into();
        assert!(scoped(&call(json!({"how":"off"})), &[other], heard).is_none());
    }
}
