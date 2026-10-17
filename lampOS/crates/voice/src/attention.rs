//! Whom the lamp answers: its own rule, applied before any sound is let out.
//!
//! The model decides what to say. Whether an answer is played is, for the
//! first things a new session hears, decided here.
//!
//! **What goes wrong.** A session that has just opened has no history of
//! leaving talk alone, and takes what it hears for the start of a
//! conversation with it. On lamp-4ace on 2026-10-13, over three runs of
//! new sessions that each heard two people talking to each other, 9 of 35
//! spoke words to them within their first two requests, whether the
//! session was told how the room was, given a function to decline with, or
//! neither. A session that has been running does this far less: 2 of 87
//! things heard, that afternoon.
//!
//! **The rule.** In a room the lamp knows to be busy, an answer to one of
//! a session's first [`FRESH_REQUESTS`] requests is held back, unless the
//! lamp is already in a conversation, began the turn itself, or the text
//! of what it heard shows it was called. Everywhere else the model's own
//! judgement stands, as before.
//!
//! **What the rule may not lean on.** The first form of it held back every
//! answer in a busy room unless the lamp's name was in the service's text
//! of what it heard. Played the same evening it stopped every answer to
//! other people's talk (0 of 11 sessions) and also held back both of two
//! questions that used the name: the service does not write the name. It
//! wrote "Milan, what is the capital of Italy?" for "Hey Lamp, ...",
//! dropped "Lamp," from the other, and in 310 lines heard that day wrote
//! "Lamp" not once. So the text can show that the lamp was called and can
//! never show that it was not. Here it is used the first way only, and
//! the hold is kept to the few requests where the model is known to be
//! unreliable. The cost that remains: someone who calls the lamp in a busy
//! room within its first two requests after a new session, in words the
//! text gives no sign of, is not answered until they say it again.
//!
//! What makes holding possible at all is timing: in those runs the text of
//! what was heard had arrived before the first sound of every one of 24
//! answers, by 0.32 s at the least.
//!
//! An answer held back is dropped in the provider worker like one the
//! model declined itself (docs/tools.md). Off unless the service's
//! environment says `on`. A better sign of being called (the lamp
//! spotting its name in its own sound, the eyes seeing someone face it)
//! is to take the text's place (docs/attention.md).

use crate::room_told::Room;
use std::sync::atomic::{AtomicU64, Ordering};

/// Where the coordinator reads whether the rule is applied. `on` only.
pub const ENVIRONMENT: &str = "LAMP_ATTENTION";
/// The provider worker's flag for it.
pub const FLAG: &str = "--attention";

/// Where the coordinator reads whether a request that continues one the lamp
/// ended early, within a second of that end or after an end sent early,
/// keeps what the rule knew of it (#782). `on` only.
///
/// Why: the 400 ms early end (`--endpoint smart-turn-fast`) ends a turn at a
/// pause the person goes on after, and the rest comes as a new request.
/// On lamp-4ace on 2026-10-15, 9 of 84 such ends in the played scenes fell
/// at a comma inside a line, and two more were ended by the 600 ms window
/// just after "Hey Lamp,". The name was then said 1.5 s or more before the
/// new request began, so it did not count for it, and in a busy room the
/// rest was held as not said to the lamp (as on 2026-10-14 at 13:32, four
/// times). With this, the continuation keeps the first part's name, its
/// surest score, the text heard so far and whether it was close.
pub const CUT_KEEPS_NAME_ENVIRONMENT: &str = "LAMP_CUT_KEEPS_NAME";

/// Whether the voice's environment says `LAMP_CUT_KEEPS_NAME=on`, read once.
pub fn cut_keeps_name() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| keeps_name(std::env::var(CUT_KEEPS_NAME_ENVIRONMENT).ok().as_deref()))
}

fn keeps_name(value: Option<&str>) -> bool {
    value == Some("on")
}

worker_global! {
    /// The newest request the lamp began itself, in writing: a hello, a look.
    /// Nothing was said to it, so nothing can have been said to someone else.
    pub fn own_request() -> AtomicU64 = AtomicU64::new(0);
}

/// The provider worker's argument for the switch in the environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    match value {
        Some("on") => vec![FLAG.to_owned()],
        Some("busy") => vec![FLAG.to_owned(), BUSY_FLAG.to_owned()],
        Some("busy-near") => vec![FLAG.to_owned(), BUSY_FLAG.to_owned(), NEAR_FLAG.to_owned()],
        _ => Vec::new(),
    }
}

/// The provider worker's flag, right after [`FLAG`], for the rule's busy
/// form (`LAMP_ATTENTION=busy`): in a room the lamp knows to be busy, every
/// answer is held unless something shows it was meant for the lamp, and a
/// name heard in the sound counts only with someone turned to the lamp.
///
/// Why: on lamp-4ace on 2026-10-14 a meeting at the table beside it went
/// on for twenty minutes and the lamp answered it three times, none in a
/// new session's first requests. Silent probes the same day, with the
/// session told the office was busy, still had "What is 2 + 2?" answered
/// "Four." and "How was your weekend?" answered: told is not enough. A
/// person at the next desk answers a question in a busy room when it is
/// put to them by name or by a look.
pub const BUSY_FLAG: &str = "--attention-busy";

/// Take a leading `--attention-busy` off a worker's flags, after
/// [`take`].
pub fn take_busy(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == BUSY_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// The provider worker's flag, right after [`BUSY_FLAG`], for a name heard
/// close to the lamp counting as a call (`LAMP_ATTENTION=busy-near`): in a
/// busy room, a hearing of the name at [`NEAR_SCORE`] or more, while the
/// room microphone rose [`NEAR_RISE_DB`] or more over the room's quiet
/// during the request, is let out though nobody is seen turned to the lamp.
///
/// Why: in played busy-room scenes on lamp-4ace on 2026-10-14 the calls
/// that were held had their name heard at 0.97 to 0.99 with no face seen
/// (the busy rule wants 0.999 or a face). With the owner at the desk the
/// same evening, the room microphone rose 19.5 to 26.5 dB over its quiet
/// for the ten lines said to the lamp, and 9.8 to 15.1 dB for people
/// farther off (`docs/ears.md`). Someone at the desk can also be talking to
/// someone else: so the name must be heard as well.
pub const NEAR_FLAG: &str = "--attention-near";
/// A name heard at least this surely counts as a call when it is close.
pub const NEAR_SCORE: f32 = 0.95;
/// Close: the room microphone this far over its quiet during the request.
pub const NEAR_RISE_DB: f32 = 18.0;
const NAMED_NEAR: &str = "its name was heard close to the lamp";
/// The reason an answer is let out for a request said to the lamp from
/// close to it (`crate::addressed`, `LAMP_ADDRESSED=on`).
pub const ADDRESSED: &str = "someone close to the lamp and turned to it spoke";

/// Take a leading `--attention-near` off a worker's flags, after
/// [`take_busy`].
pub fn take_near(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == NEAR_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// Where the coordinator reads whether "the lamp" said in passing is no
/// call (#829). `ignored` only.
///
/// Why: [`called`] takes the word "lamp" anywhere in what was heard as the
/// lamp called, so in a busy room "I left the lamp on in the kitchen", said
/// by one person to another, lets an answer out. It was played on lamp-4ace
/// on 14 and 15 October, and both times the model declined it of its own
/// accord. With this, the name counts only when it is said to the lamp, not
/// about it ([`called_by_name`]).
pub const PASSING_ENVIRONMENT: &str = "LAMP_NAME_IN_PASSING";
/// The provider worker's flag for it, right after [`NEAR_FLAG`].
pub const PASSING_FLAG: &str = "--name-in-passing-ignored";

/// The provider worker's argument for that switch in the environment.
pub fn passing_arguments_from_environment() -> Vec<String> {
    passing_arguments(std::env::var(PASSING_ENVIRONMENT).ok().as_deref())
}

fn passing_arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("ignored") {
        vec![PASSING_FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading `--name-in-passing-ignored` off a worker's flags, after
/// [`take_near`].
pub fn take_passing(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == PASSING_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// Where the coordinator reads whether the lamp speaks only when spoken
/// to (#829, step 2). `on`, or `day-and-night` for that and the day's own
/// bars ([`BY_DAY_FLAG`]).
///
/// Why: outside a busy room, and in a session the rule trusts, every
/// answer is played, whatever was said and to whom. On lamp-4ace on
/// 16 October at 02:16, "Did you get my message about the budget?", said
/// by one person to another with no name and nobody at the lamp, was
/// answered 11 times in 12; on 15 October at 14:49 a word overheard in
/// the office's talk turned the lamp's volume up for 11 hours. With this,
/// in every room, an answer is played only when something shows the lamp
/// was spoken to: its name, a face turned to it, a conversation with it
/// going on, or a person close to it and turned to it; or the lamp began
/// the turn itself. A reply to the lamp's own news outside a busy room, and
/// a question about its own work, are played too ([`NEWS_REPLY_US`],
/// [`about_its_work`]).
/// And only talk between people makes the room busy
/// (`crate::room_told::RoomTold::count_only_talk`).
pub const SPOKEN_TO_ENVIRONMENT: &str = "LAMP_SPOKEN_TO";
/// The provider worker's flag for it, right after [`PASSING_FLAG`].
pub const SPOKEN_TO_FLAG: &str = "--spoken-to";
/// The provider worker's flag for `LAMP_SPOKEN_TO=day-and-night`, right
/// after [`SPOKEN_TO_FLAG`]: the switch, and by day, in a busy room with
/// nobody turned to the lamp, the name counts only heard surely, and close
/// to the lamp only with a face turned to it or its words calling it by
/// name (#829, the first builder at 15:23 and 17:09). With `on` the name
/// needs no face by day or at night. The day's half is its own value
/// because no lamp has played it by day (the reviewer on #1158).
pub const BY_DAY_FLAG: &str = "--spoken-to-by-day";

/// The provider worker's argument for that switch in the environment.
pub fn spoken_to_arguments_from_environment() -> Vec<String> {
    spoken_to_arguments(std::env::var(SPOKEN_TO_ENVIRONMENT).ok().as_deref())
}

fn spoken_to_arguments(value: Option<&str>) -> Vec<String> {
    match value {
        Some("on") => vec![SPOKEN_TO_FLAG.to_owned()],
        Some("day-and-night") => vec![SPOKEN_TO_FLAG.to_owned(), BY_DAY_FLAG.to_owned()],
        _ => Vec::new(),
    }
}

/// Take a leading `--spoken-to` off a worker's flags, after
/// [`take_passing`].
pub fn take_spoken_to(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == SPOKEN_TO_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// Take a leading `--spoken-to-by-day` off a worker's flags, after
/// [`take_spoken_to`].
pub fn take_by_day(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == BY_DAY_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// Why an answer is held when nothing shows the lamp was spoken to.
pub const NOT_SPOKEN_TO: &str =
    "nobody spoke to the lamp: no name, nobody turned to it, no conversation with it going on";

/// With the switch, the first thing anyone says this soon after the lamp's
/// own news (a turn it began itself: a task's news, a reminder) is a reply
/// to it, in a room not known to be busy. On lamp-4ace on 16 October at
/// 02:30, "How is that task going?" came 45.3 s after the news, past the
/// 45 s an answer keeps a conversation going, and was held; people take a
/// while to come back. In a busy room only a reply that asks about the
/// lamp's work is taken so ([`about_its_work`]): two colleagues talking a
/// minute or two after the news were answered every line (#1378).
pub const NEWS_REPLY_US: u64 = 180_000_000;
const AFTER_NEWS: &str = "the first thing said after the lamp's own news";

/// With the switch, a question about the lamp's own work
/// ([`about_its_work`]) is for it this long after the lamp's news, or after
/// it started a task, a timer or a reminder.
pub const WORK_ASKED_US: u64 = 600_000_000;
const ABOUT_ITS_WORK: &str = "a question about the lamp's own task, reminder or timer";

/// With the switch, a line that follows, by at most this long, one that
/// called someone other than the lamp by name ([`addresses_someone_else`])
/// is that person's reply, and not part of a conversation with the lamp.
/// On lamp-4ace on 16 October at 04:58 and 05:01, Samantha's "Daniel, are
/// you coming to the meeting at two?" was followed by Daniel's "Yes, give
/// me five minutes.", and as Daniel was in a conversation with the lamp it
/// set a timer for five minutes.
pub const ANOTHER_REPLY_US: u64 = 10_000_000;
const ANSWERING_ANOTHER: &str =
    "a reply to someone the room had just called by name, not said to the lamp";
/// With the switch, a line that itself calls someone other than the lamp
/// by name ("Daniel, are you coming to the meeting at two?") is said to
/// them, however it follows the lamp: the first builder's 15:40 on #829,
/// where such a line and its reply could be one request.
const TO_ANOTHER: &str = "said to someone it calls by name, not to the lamp";
/// With the switch, in a busy room, a line that follows, by at most
/// [`ANOTHER_REPLY_US`], a question the lamp left unanswered answers the
/// one who asked it (the first builder at 15:40 on #829): "Yes, give me
/// five minutes." after "Are you coming to the meeting at two?" sets no
/// timer.
const AFTER_QUESTION: &str =
    "a busy room: a reply to a question the lamp left unanswered, not said to the lamp";

/// Words that open a sentence of their own before a comma, and are no
/// one's name ("Okay, stop.", "Well, I think so.").
const NOT_NAMES: &[&str] = &[
    "okay", "ok", "well", "so", "yes", "yeah", "yep", "no", "nope", "oh", "ah", "hey", "hi",
    "hello", "right", "sure", "thanks", "thank", "please", "sorry", "now", "then", "also",
    "actually", "look", "listen", "wait", "um", "uh", "hmm", "hm", "alright", "and", "but",
    "anyway", "honestly", "really", "fine", "great", "cool", "good", "wow", "huh", "first", "lamp",
    "lamb",
];

/// Times and sentence words that open a request to the lamp, and are no
/// one's name either: "Tomorrow, remind me…", "Seriously, what time is
/// it?" (Second on #1090), and "True, but…", which held the next line as a
/// reply to someone called True (#1030's worker test).
const NOT_NAMES_OPENING: &[&str] = &[
    "today",
    "tomorrow",
    "tonight",
    "yesterday",
    "later",
    "morning",
    "afternoon",
    "evening",
    "earlier",
    "seriously",
    "basically",
    "maybe",
    "perhaps",
    "besides",
    "however",
    "otherwise",
    "meanwhile",
    "finally",
    "next",
    "again",
    "plus",
    "remember",
    "quickly",
    "true",
];

/// Whether what was heard calls someone other than the lamp by name: a name
/// set off by a comma, opening it after any greeting ("Daniel, are you
/// coming to the meeting at two?", "Hey Samantha, ...") or closing it ("Are
/// you coming, Daniel?"). A name here is a capitalised word that is not the
/// lamp's, nor a word the service writes in its place, nor one that opens a
/// sentence of its own ("Okay,").
pub fn addresses_someone_else(heard: &str) -> bool {
    let is_name = |word: &str| {
        word.chars().next().is_some_and(char::is_uppercase)
            && word.chars().all(char::is_alphabetic)
            && !NOT_NAMES
                .iter()
                .chain(NOT_NAMES_OPENING)
                .any(|not| word.eq_ignore_ascii_case(not))
            && !STAND_INS.iter().any(|not| word.eq_ignore_ascii_case(not))
    };
    let words: Vec<&str> = heard.split_whitespace().collect();
    let opening = words
        .iter()
        .find(|word| {
            !GREETINGS
                .iter()
                .any(|greeting| word.trim_end_matches(',').eq_ignore_ascii_case(greeting))
        })
        .is_some_and(|word| word.strip_suffix(',').is_some_and(is_name));
    let closing = words.len() >= 2
        && words[words.len() - 2].ends_with(',')
        && is_name(words[words.len() - 1].trim_end_matches(['?', '.', '!']));
    opening || closing
}

/// Whether what was heard speaks of the lamp's own work: a task, a
/// reminder, a timer or an alarm ("How is that task going?").
pub fn about_its_work(heard: &str) -> bool {
    const WORK: [&str; 8] = [
        "task",
        "tasks",
        "reminder",
        "reminders",
        "timer",
        "timers",
        "alarm",
        "alarms",
    ];
    heard
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| WORK.iter().any(|work| word.eq_ignore_ascii_case(work)))
}

/// Where the coordinator reads whether the model is asked to say when it
/// hears the lamp's name. `on` only. A measurement switch: it changes
/// nothing the lamp does, and exists to count whether the model's own sign
/// can stand where the heard text could not.
pub const NAME_ENVIRONMENT: &str = "LAMP_NAME_SIGNAL";
/// The provider worker's flag for it.
pub const NAME_FLAG: &str = "--name-signal";
/// The function the model calls when it hears the name.
pub const HEARD_MY_NAME: &str = "heard_my_name";

/// The provider worker's argument for that switch in the environment.
pub fn name_arguments_from_environment() -> Vec<String> {
    if std::env::var(NAME_ENVIRONMENT).ok().as_deref() == Some("on") {
        vec![NAME_FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading `--name-signal` off a worker's flags.
pub fn take_name(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == NAME_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// The function itself: silent, so that calling it neither delays the
/// answer nor adds a word to it.
pub fn name_tool() -> lamp_gemini::Tool {
    lamp_gemini::Tool {
        name: HEARD_MY_NAME.into(),
        description: "Call this every time someone says your name, Lamp, when speaking to you, \
                      whatever else they say. It makes no sound and nobody sees it. Then answer \
                      them exactly as you would have."
            .into(),
        parameters: None,
        silent: true,
    }
}

/// Take a leading `--attention` off a worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// How many of a session's first requests the rule covers. Counted in the
/// played runs: the answers to other people's talk came on a new session's
/// first or second request.
pub const FRESH_REQUESTS: u32 = 2;

/// Whether the text of what was heard shows the lamp was called. Its name,
/// or one of the words the service has written in its place at the start
/// of a called request in the lamp's own records ("Milan, what is the
/// capital of Italy?", "Man, tell me a joke.", "Lam", "Lan", "plan", "Helen,
/// what color is the sky on a clear day?", "Hey Mom", "Amen", "Weiland").
/// Each was written for "Lamp" or "Hey Lamp" at least once on lamp-4ace.
/// Positive evidence only: a text without them says nothing either way.
pub fn called(heard: &str) -> bool {
    let mut words = heard
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty());
    let is = |word: &str, list: &[&str]| list.iter().any(|known| word.eq_ignore_ascii_case(known));
    let all: Vec<&str> = words.by_ref().collect();
    if all.iter().any(|word| is(word, &["lamp", "lamb"])) {
        return true;
    }
    // A misheard name stands where the name stood: first, or after a
    // greeting.
    called_by_stand_in(&all)
}

/// Words the service has written in the lamp's name's place ([`called`]).
const STAND_INS: &[&str] = &[
    "lam", "lan", "milan", "man", "plan", "helen", "mom", "amen", "weiland",
];

/// Greetings that may come before the name of a call ("Hey Lamp,").
const GREETINGS: &[&str] = &["hey", "hi", "ok", "okay", "oh", "hello"];

/// [`called`], with the name counted only where it is said to the lamp
/// (`LAMP_NAME_IN_PASSING=ignored`, #829): opening a sentence, after any
/// greeting ("Lamp, ...", "Hey Lamp, ...", "Okay. Lamp, ..."), or right
/// after a comma ("..., Lamp?", "Can you, lamp, ..."). "I left the lamp
/// on", "a new lamp", "her desk lamp" are not calls. What the service
/// writes in the name's place is read as [`called`] reads it: first, or
/// after a greeting.
pub fn called_by_name(heard: &str) -> bool {
    // Words, and the punctuation between them that marks a sentence's start
    // or a comma.
    #[derive(Clone, Copy, PartialEq)]
    enum Token<'a> {
        Word(&'a str),
        Comma,
        Stop,
    }
    let mut tokens = Vec::new();
    let mut word_start = None;
    for (at, c) in heard.char_indices() {
        if c.is_alphanumeric() {
            word_start.get_or_insert(at);
            continue;
        }
        if let Some(start) = word_start.take() {
            tokens.push(Token::Word(&heard[start..at]));
        }
        match c {
            ',' | ';' | ':' => tokens.push(Token::Comma),
            '.' | '?' | '!' => tokens.push(Token::Stop),
            _ => {}
        }
    }
    if let Some(start) = word_start {
        tokens.push(Token::Word(&heard[start..]));
    }
    let is = |word: &str, list: &[&str]| list.iter().any(|known| word.eq_ignore_ascii_case(known));
    let said_to = tokens.iter().enumerate().any(|(at, token)| {
        let Token::Word(word) = token else {
            return false;
        };
        if !is(word, &["lamp", "lamb"]) {
            return false;
        }
        // What came before it, greetings passed over.
        let before = tokens[..at]
            .iter()
            .rev()
            .find(|token| !matches!(token, Token::Word(word) if is(word, GREETINGS)));
        matches!(before, None | Some(Token::Comma) | Some(Token::Stop))
    });
    let words: Vec<&str> = tokens
        .iter()
        .filter_map(|token| match token {
            Token::Word(word) => Some(*word),
            _ => None,
        })
        .collect();
    said_to || called_by_stand_in(&words)
}

/// What the service has written in the name's place, first or after a
/// greeting ([`called`]).
fn called_by_stand_in(words: &[&str]) -> bool {
    let is = |word: &str, list: &[&str]| list.iter().any(|known| word.eq_ignore_ascii_case(known));
    words
        .iter()
        .find(|word| !is(word, GREETINGS))
        .is_some_and(|word| is(word, STAND_INS))
}

/// Whether a call for this function is the start of an answer, to be
/// decided like one: every call but the two that only say something about
/// what was heard and do nothing.
pub fn answers(function: &str) -> bool {
    function != HEARD_MY_NAME && function != crate::tools::NOT_FOR_ME
}

/// Whether an answer is played, and why, for the record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verdict {
    Play(&'static str),
    Hold(&'static str),
}

/// The lamp's name counts for a request if it began to be heard no longer
/// than this before the request began: the name is the first thing said,
/// and the voice takes a moment to be sure someone is speaking.
pub const NAME_BEFORE_US: u64 = 1_500_000;
const NAMED: &str = "its name was heard in the room's sound";

/// A tap on the lamp's head (#711, `docs/creature.md` "Touch") counts as a
/// call for a request that begins no longer than this after it: the tap
/// asks the lamp to listen, and the person may take a breath first. It is
/// the one window the lamp shows it listens for, the ring's listening face
/// ([`crate::ring_face::TAP_LISTENS_US`] is this) and Expression's look-up:
/// as long as a call waits for the rest on lamp-4ace (#891, First on #711).
/// A request that begins inside it keeps it however long it goes on.
pub const TAPPED_BEFORE_US: u64 = 2_000_000;
const TAPPED: &str = "someone tapped its head to talk to it";
/// A request that begins this long after one that was only the lamp's name
/// (the call's wait ran out, #891) is that call's (#829, the first builder
/// at 16:32: "Lamp," then "Check my work." was held in a busy room by day),
/// if the call itself showed the lamp was called ([`Attention::bare_call`]).
pub const AFTER_CALL_US: u64 = 3_000_000;
const AFTER_CALL: &str = "it follows the lamp's name said alone";

/// The provider worker's flag for taking the head's taps (#711), given
/// with `LAMP_HEAD_TOUCH=on`. The worker reads them from the bus, so they
/// count only when it has one (`LAMP_BUS=on`), as the name heard does.
pub const HEAD_TOUCH_FLAG: &str = "--head-touch";

/// The provider worker's arguments for the head's taps.
pub fn head_touch_arguments_from_environment() -> Vec<String> {
    if crate::hand_controls::head_touch_from_environment() {
        vec![HEAD_TOUCH_FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading `--head-touch` off a worker's flags.
pub fn take_head_touch(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == HEAD_TOUCH_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// The rule. `fresh`: the request is one of its session's first
/// [`FRESH_REQUESTS`]. `heard`: the text of what was heard since the
/// request began, empty when none has arrived. `named`: the lamp's name
/// was heard in the sound itself as the request began (`docs/name.md`),
/// which like the text can speak for playing and never against.
pub fn decide(
    room: Room,
    going_on: bool,
    own: bool,
    fresh: bool,
    heard: &str,
    named: bool,
) -> Verdict {
    decide_busy(room, going_on, own, fresh, heard, named, false, None)
}

const TURNED: &str = "someone is turned to the lamp";
const SOUND_A_WORD: &str = "a short sound, heard as a short word said to the lamp";
const SOUND_NEAR: &str = "a short sound, said close to the lamp";
const SOUND_TURNED: &str = "a short sound, with someone turned to the lamp";
const SOUND_ANSWERS: &str = "a short sound with a word in it, right after the lamp's own question, in a conversation going on";
const SOUND_HELD: &str = "a short sound, and nothing shows it was said to the lamp: not a short word, not close, nobody turned to it";
const NAMED_PLAINLY: &str = "its name was heard plainly in the room's sound";

/// A hearing of the name this sure counts as a call in a busy room even
/// with nobody seen turned to the lamp. On lamp-4ace on 2026-10-14, from
/// 07:45 to 13:02 with nobody calling it, the part that listens for the
/// name heard it 17 times in people's talk, at 0.903 to 0.997; played calls
/// that afternoon scored 1.000, 1.000, 0.999 and 0.922.
pub const SURE_SCORE: f32 = 0.999;

/// The same, in its busy form when `strict` ([`BUSY_FLAG`]): in a busy room
/// every request is covered, not only a new session's first, and the name
/// heard in the sound counts only with someone turned to the lamp.
/// `turned`: whether the eyes' newest fresh look has a face turned to the
/// lamp (`crate::bus_out::someone_turned_to_lamp`), which speaks for
/// playing and never against.
#[allow(clippy::too_many_arguments)]
pub fn decide_busy(
    room: Room,
    going_on: bool,
    own: bool,
    fresh: bool,
    heard: &str,
    named: bool,
    strict: bool,
    turned: Option<bool>,
) -> Verdict {
    decide_called(
        room,
        going_on,
        own,
        fresh,
        called(heard),
        named,
        strict,
        turned,
    )
}

/// [`decide_busy`] with whether the text shows a call already read.
#[allow(clippy::too_many_arguments)]
fn decide_called(
    room: Room,
    going_on: bool,
    own: bool,
    fresh: bool,
    called_in_text: bool,
    named: bool,
    strict: bool,
    turned: Option<bool>,
) -> Verdict {
    if own {
        Verdict::Play("the lamp began this turn itself")
    } else if !(fresh || strict && room == Room::Busy) {
        Verdict::Play("the session has heard enough to be trusted")
    } else if room != Room::Busy {
        Verdict::Play("the room is not known to be busy")
    } else if going_on {
        Verdict::Play("a conversation with the lamp is going on")
    } else if called_in_text {
        Verdict::Play("the text shows it was called")
    } else if turned == Some(true) {
        Verdict::Play(TURNED)
    } else if named && !strict {
        Verdict::Play(NAMED)
    } else if strict && named {
        Verdict::Hold(
            "a busy room: its name was heard in the sound, and nobody is turned to the lamp",
        )
    } else if strict {
        Verdict::Hold(
            "a busy room, and nothing shows it was called or that anyone is turned to the lamp",
        )
    } else {
        Verdict::Hold("a new session in a busy room, and nothing shows it was called")
    }
}

/// What the worker keeps for the request in hand.
#[derive(Default)]
pub struct Attention {
    request: u64,
    /// The session the request began on, and how many requests that were
    /// heard have begun on it. A turn the lamp wrote itself is not one:
    /// the session heard nothing in it.
    session: u64,
    begun: u32,
    /// What was heard since it began, as far as the service has written it.
    heard: String,
    verdict: Option<Verdict>,
    /// When the request began, if that is known, and when its sound ended,
    /// if it has, by this process's clock.
    began_us: Option<u64>,
    ended_us: Option<u64>,
    /// When the lamp's name last began to be heard in the room's sound.
    named_us: Option<u64>,
    /// The name was heard for the request in hand: as it began, or in its
    /// sound.
    named: bool,
    /// When the name was last heard at [`SURE_SCORE`] or more, and whether
    /// such a hearing counts for the request in hand.
    named_sure_us: Option<u64>,
    named_sure: bool,
    /// When the lamp's head was last tapped, and whether that counts for the
    /// request in hand ([`TAPPED_BEFORE_US`]).
    tapped_us: Option<u64>,
    tapped: bool,
    /// The newest request that was only the lamp's name, when it ended,
    /// and, once the next request began, whether its own hearing showed the
    /// lamp was called; and whether the request in hand began within
    /// [`AFTER_CALL_US`] of one that did.
    bare_call_us: Option<(u64, u64, Option<bool>)>,
    after_call: bool,
    /// The surest the name was heard for the request in hand, and
    /// whether a voice close to the lamp was heard in it.
    best_score: f32,
    near: bool,
    /// Whether it was said to the lamp from close to it, as
    /// [`crate::addressed::judge`] found.
    addressed: bool,
    /// A short sound let through in a quiet room is held unless something
    /// shows it was said to the lamp (`LAMP_SHORT_SOUNDS=quiet-room-checked`,
    /// #800), and whether the request in hand ended as such a sound.
    sounds_checked: bool,
    /// Ask again rather than answer what makes no sense in a conversation
    /// (#926, `LAMP_OTHER_LANGUAGE=sorry`), and whether the lamp's last line
    /// was a question.
    ask_unclear: bool,
    lamp_asked: bool,
    /// The request judged unclear, and why.
    unclear: Option<(u64, &'static str)>,
    /// The request in hand ended as a cut-in over the lamp's own answer.
    cut_in: bool,
    sound: bool,
    /// Whether the request judged last was the lamp's own turn, as the
    /// judging read it, for the record ([`Attention::baseline`]).
    judged_own: bool,
    /// "The lamp" said in passing is no call ([`called_by_name`],
    /// `LAMP_NAME_IN_PASSING=ignored`).
    name_in_passing_ignored: bool,
    /// An answer is played only when the lamp was spoken to
    /// (`LAMP_SPOKEN_TO=on`, #829).
    spoken_to: bool,
    /// When the lamp's own news was last heard to begin, until something
    /// said after it has been judged; and when it last did work of its own:
    /// its news, or a task, a timer or a reminder it started. With
    /// `spoken_to` only.
    news_us: Option<u64>,
    worked_us: Option<u64>,
    /// The own turn whose sound opened `news_us`.
    news_of: Option<u64>,
    /// The request in hand follows, within [`ANOTHER_REPLY_US`], one that
    /// called someone other than the lamp by name. With `spoken_to` only.
    after_another: bool,
    /// The request in hand follows, within [`ANOTHER_REPLY_US`], a question
    /// the lamp left unanswered ([`AFTER_QUESTION`]); counted only in a busy
    /// room. With `spoken_to` only.
    after_question: bool,
    /// The request in hand follows, within [`ANOTHER_REPLY_US`], a line
    /// with words that someone said and the lamp did not answer: it follows
    /// that line, not the lamp's own last words (#1054). Whatever the
    /// switch.
    after_unanswered: bool,
    /// The day's own bars (`LAMP_SPOKEN_TO=day-and-night`, [`BY_DAY_FLAG`]).
    /// With `spoken_to` only.
    by_day: bool,
    /// It is night where the lamp stands ([`crate::local_time::night_here`]),
    /// as the caller read it before judging. Read only with `by_day`.
    night: bool,
}

/// The most text kept for one request.
const MOST_HEARD_BYTES: usize = 2_048;

impl Attention {
    /// Read the text's call with [`called_by_name`] from now on
    /// (`LAMP_NAME_IN_PASSING=ignored`, #829).
    pub fn ignore_name_in_passing(&mut self, ignored: bool) {
        self.name_in_passing_ignored = ignored;
    }

    /// Play an answer only when something shows the lamp was spoken to,
    /// in every room, from now on (`LAMP_SPOKEN_TO=on`, #829).
    pub fn speak_when_spoken_to(&mut self, on: bool) {
        self.spoken_to = on;
    }

    /// Hold the name to the day's own bars ([`BY_DAY_FLAG`]) from now on,
    /// with [`Self::speak_when_spoken_to`].
    pub fn judge_by_day(&mut self, on: bool) {
        self.by_day = on;
    }

    /// Whether the day's own bars are set: only then is the night read
    /// ([`Self::set_night`]).
    pub fn judges_by_day(&self) -> bool {
        self.spoken_to && self.by_day
    }

    /// By day, with the day's own bars: the name alone counts in a busy
    /// room only heard surely, and close to the lamp only with a face or
    /// its words calling it.
    fn day_bars(&self) -> bool {
        self.judges_by_day() && !self.night
    }

    /// Whether the rule reads the eyes for every request, whatever the
    /// room: it does when it plays only for a lamp spoken to.
    pub fn speaks_when_spoken_to(&self) -> bool {
        self.spoken_to
    }

    /// The lamp's own turn `request` began to be heard at `at_us`: its first
    /// sound was handed on to be played. Only then is it news someone can
    /// reply to (#918, the reviewer at 05:11 and 06:17): an own turn that
    /// ends silent, is cancelled before it speaks, or whose sound a
    /// cancellation drops while it waits for output credit, opens no
    /// window. Once per turn.
    pub fn spoke(&mut self, request: u64, at_us: u64) {
        if self.spoken_to
            && request == self.request
            && own_request().load(Ordering::Acquire) == request
            && self.news_of != Some(request)
        {
            self.news_of = Some(request);
            self.news_us = Some(at_us);
            self.worked(at_us);
        }
    }

    /// The lamp started a task, a timer or a reminder at `at_us`, by this
    /// process's clock.
    pub fn worked(&mut self, at_us: u64) {
        self.worked_us = Some(self.worked_us.map_or(at_us, |before| before.max(at_us)));
    }

    /// A request began on `session`, at `now_us`.
    pub fn began_at(&mut self, request: u64, session: u64, now_us: u64) {
        // The line before, if it called someone else by name and ended just
        // now (#829).
        let after_another = self.spoken_to
            && own_request().load(Ordering::Acquire) != request
            && self
                .ended_us
                .or(self.began_us)
                .is_some_and(|end| end.saturating_add(ANOTHER_REPLY_US) >= now_us)
            && addresses_someone_else(&self.heard);
        // The line before, if it was a question the lamp left unanswered
        // and ended just now (#829, 15:40).
        let after_question = self.spoken_to
            && own_request().load(Ordering::Acquire) != request
            && self
                .ended_us
                .or(self.began_us)
                .is_some_and(|end| end.saturating_add(ANOTHER_REPLY_US) >= now_us)
            && self.heard.trim_end().ends_with('?')
            && !matches!(self.verdict, Some(Verdict::Play(_)));
        // The line before, someone's, with words, ended just now and was
        // left unanswered: this one follows it, not the lamp's own last
        // words. In the first builder's two-person round on #829 (15:39),
        // "Yes, give me five minutes." after "Are you coming to the meeting
        // at two?" set a timer as a reply to the lamp's story (#1054). A
        // short sound let through and ended as one is nobody's line, even
        // heard as a word ("¿Qué?"), as the room counts it (Fourth on #1054).
        let own = own_request().load(Ordering::Acquire);
        let after_unanswered = own != request
            && own != self.request
            && self
                .ended_us
                .or(self.began_us)
                .is_some_and(|end| end.saturating_add(ANOTHER_REPLY_US) >= now_us)
            && self.heard.chars().any(char::is_alphanumeric)
            && !self.sound
            && !matches!(self.verdict, Some(Verdict::Play(_)));
        // The bare call is judged as it is left, on all it heard.
        if let Some((call, ended, None)) = self.bare_call_us
            && call == self.request
            && call != request
        {
            self.bare_call_us = Some((call, ended, Some(self.call_shown())));
        }
        self.began(request, session);
        self.after_another = after_another;
        self.after_question = after_question;
        self.after_unanswered = after_unanswered;
        self.began_us = Some(now_us);
        self.named = self
            .named_us
            .is_some_and(|named| named.saturating_add(NAME_BEFORE_US) >= now_us);
        self.named_sure = self
            .named_sure_us
            .is_some_and(|named| named.saturating_add(NAME_BEFORE_US) >= now_us);
        self.tapped = self.tapped_us.is_some_and(|tapped| {
            tapped <= now_us && tapped.saturating_add(TAPPED_BEFORE_US) >= now_us
        });
        self.after_call = self.bare_call_us.is_some_and(|(call, ended, shown)| {
            call != request
                && shown == Some(true)
                && ended <= now_us
                && ended.saturating_add(AFTER_CALL_US) >= now_us
        });
    }

    /// Whether the request in hand, a bare call, showed by its own hearing
    /// that the lamp was called, as the busy room judges a request: its
    /// name heard plainly ([`SURE_SCORE`]), or surely ([`NEAR_SCORE`]) close
    /// to the lamp, or its text a call by name; and not a call to someone
    /// else. The name's first sign (`name/rising`), which makes a bare call,
    /// comes at the name part's own 0.9 and is for timing only
    /// (`docs/name.md`): in one morning on lamp-4ace it heard the name 17
    /// times in people's talk at 0.903 to 0.997 (the reviewer on #1108).
    /// By day with the day's own bars, close to the lamp is not enough on
    /// its own, as for a request (the first builder's 17:09 sign; the
    /// reviewer on #1158). The eyes are not read for a bare call, so only
    /// its words or a sure hearing show it then.
    fn call_shown(&self) -> bool {
        !addresses_someone_else(&self.heard)
            && (self.named_sure
                || self.near && self.best_score >= NEAR_SCORE && !self.day_bars()
                || called_by_name(&self.heard))
    }

    /// A request began on `session` at `now_us` that continues the one in
    /// hand, which the lamp ended early: the same person going on with the
    /// same utterance. What the rule knew of the first part stays: its name,
    /// heard surely or not and how surely, the text heard so far, whether it
    /// was close, and whether it was said to the lamp from close to it. Its window begins where the first part's did.
    /// Anything else is as for [`Self::began_at`].
    pub fn continued_at(&mut self, request: u64, session: u64, now_us: u64) {
        let tapped = session == self.session && self.began_us.is_some() && self.tapped;
        let after_call = session == self.session && self.began_us.is_some() && self.after_call;
        let after_another = self.after_another;
        let after_question = self.after_question;
        let after_unanswered = self.after_unanswered;
        let before = (session == self.session && self.began_us.is_some()).then(|| {
            (
                self.named,
                self.named_sure,
                self.best_score,
                std::mem::take(&mut self.heard),
                self.near,
                self.addressed,
                self.began_us,
            )
        });
        self.began_at(request, session, now_us);
        if let Some((named, named_sure, best_score, heard, near, addressed, began_us)) = before {
            self.named |= named;
            self.named_sure |= named_sure;
            self.best_score = self.best_score.max(best_score);
            self.heard = heard;
            self.near |= near;
            self.addressed |= addressed;
            self.began_us = began_us;
            self.after_another = after_another;
            self.after_question = after_question;
            self.after_unanswered = after_unanswered;
        }
        self.tapped |= tapped;
        self.after_call |= after_call;
    }

    /// The request `request` was only the lamp's name, its call's wait run
    /// out, and it ended at `at_us`: what begins within [`AFTER_CALL_US`]
    /// is called (#829, 16:32), if the call showed the lamp was called
    /// ([`Self::call_shown`], judged when the next request begins, so that
    /// its text and its closeness have come).
    pub fn bare_call(&mut self, request: u64, at_us: u64) {
        self.bare_call_us = Some((request, at_us, None));
    }

    /// The sound of `request` stopped being sent at `at_us`: the person
    /// had finished. A name that begins to be heard after this was not
    /// said in the request, whoever said it.
    pub fn ended(&mut self, request: u64, at_us: u64) {
        if request == self.request && self.began_us.is_some() {
            self.ended_us = Some(at_us);
        }
    }

    /// The lamp's name began to be heard in the room's sound at `at_us`.
    /// It counts for the request in hand if it began as the request did or
    /// in its sound, and once it has counted a later hearing does not take
    /// that away.
    pub fn name_heard(&mut self, at_us: u64) {
        self.name_heard_scored(at_us, 0.0);
    }

    /// The same, with the hearing's score: one at [`SURE_SCORE`] or more is
    /// also kept as a sure one.
    pub fn name_heard_scored(&mut self, at_us: u64, score: f32) {
        self.named_us = Some(self.named_us.map_or(at_us, |before| before.max(at_us)));
        let sure = score >= SURE_SCORE;
        if sure {
            self.named_sure_us = Some(self.named_sure_us.map_or(at_us, |b| b.max(at_us)));
        }
        if let Some(began) = self.began_us
            && at_us.saturating_add(NAME_BEFORE_US) >= began
            && self.ended_us.is_none_or(|ended| at_us <= ended)
        {
            self.named = true;
            self.named_sure |= sure;
            self.best_score = self.best_score.max(score);
        }
    }

    /// The lamp's head was tapped at `at_us`: a call for a request that
    /// begins within [`TAPPED_BEFORE_US`], and for the one in hand if its
    /// sound has not ended.
    pub fn head_tapped(&mut self, at_us: u64) {
        self.tapped_us = Some(self.tapped_us.map_or(at_us, |before| before.max(at_us)));
        if self.began_us.is_some() && self.ended_us.is_none_or(|ended| at_us <= ended) {
            self.tapped = true;
        }
    }

    /// A request began on `session`.
    pub fn began(&mut self, request: u64, session: u64) {
        if session != self.session {
            self.session = session;
            self.begun = 0;
        }
        if own_request().load(Ordering::Acquire) != request {
            self.begun = self.begun.saturating_add(1);
        }
        self.request = request;
        self.heard.clear();
        self.verdict = None;
        self.began_us = None;
        self.ended_us = None;
        self.named = false;
        self.named_sure = false;
        self.tapped = false;
        self.best_score = 0.0;
        self.near = false;
        self.addressed = false;
        self.sound = false;
        self.cut_in = false;
    }

    /// Short sounds let through are checked (`--sounds-checked`, #800).
    /// Ask again ("Sorry?") rather than answer a turn of a conversation heard
    /// in another script, or as a number alone (#926).
    pub fn ask_again_when_unclear(&mut self, on: bool) {
        self.ask_unclear = on;
    }

    /// The request in hand ended as a cut-in over the lamp's answer (#926).
    pub fn ended_as_cut_in(&mut self, request: u64) {
        if request == self.request {
            self.cut_in = true;
        }
    }

    /// Whether the lamp's last line was a question, for the next judgement.
    pub fn lamp_asked(&mut self, asked: bool) {
        self.lamp_asked = asked;
    }

    /// Why `request` was held as unclear, if it was: then it is to be asked
    /// again.
    pub fn unclear(&self, request: u64) -> Option<&'static str> {
        self.unclear
            .filter(|(unclear, _)| *unclear == request)
            .map(|(_, why)| why)
    }

    pub fn check_sounds(&mut self, on: bool) {
        self.sounds_checked = on;
    }

    /// The sound of `request` was ended as a short sound let through in a
    /// quiet room (`ProviderInput::End { sound: true }`).
    pub fn ended_as_sound(&mut self, request: u64) {
        if request == self.request {
            self.sound = true;
        }
    }

    /// Whether the request in hand is a short sound to be checked: the
    /// eyes are then read for it whatever the room.
    pub fn sound_in_hand(&self) -> bool {
        self.sounds_checked && self.sound
    }

    /// When the request in hand began and, if it has, when its sound
    /// ended, by this process's clock.
    pub fn window(&self) -> Option<(u64, Option<u64>)> {
        self.began_us.map(|began| (began, self.ended_us))
    }

    /// Whether a voice close to the lamp was heard in the request in hand
    /// (the room microphone, `NEAR_RISE_DB` over its quiet).
    pub fn set_near(&mut self, near: bool) {
        self.near = near;
    }

    /// Whether it is night where the lamp stands, for the next judgement:
    /// with the day's own bars, at night the name heard in a busy room
    /// needs no face (#829), as with `LAMP_SPOKEN_TO=on` at any hour.
    pub fn set_night(&mut self, night: bool) {
        self.night = night;
    }

    /// Whether the request in hand was said to the lamp from close to it
    /// (`crate::addressed`): in a busy room, an answer the rule would hold
    /// is let out.
    pub fn set_addressed(&mut self, addressed: bool) {
        self.addressed = addressed;
    }

    /// Whether `request` follows a line with words that someone said and
    /// the lamp did not answer, within [`ANOTHER_REPLY_US`] (#1054).
    pub fn follows_an_unanswered_line(&self, request: u64) -> bool {
        self.request == request && self.after_unanswered
    }

    /// The request in hand as `crate::addressed` judges it, once it began.
    /// None for a turn the lamp wrote itself: nobody spoke in it.
    pub fn in_hand(&self) -> Option<crate::addressed::Request> {
        if own_request().load(Ordering::Acquire) == self.request {
            return None;
        }
        self.began_us.map(|began_us| crate::addressed::Request {
            session: self.session,
            request: self.request,
            began_us,
            ended_us: self.ended_us,
            name_score: self.best_score,
        })
    }

    /// The service wrote down more of what it heard.
    pub fn heard(&mut self, text: &str) {
        let room = MOST_HEARD_BYTES.saturating_sub(self.heard.len());
        let cut = text
            .char_indices()
            .map(|(index, c)| index + c.len_utf8())
            .take_while(|end| *end <= room)
            .last()
            .unwrap_or(0);
        self.heard.push_str(&text[..cut]);
    }

    /// The first of an answer to `request` has come: decide once, and say
    /// so. `None` for a request this is not following, or already decided.
    pub fn first_output(&mut self, request: u64, room: Room, going_on: bool) -> Option<Verdict> {
        self.first_output_busy(request, room, going_on, false, None)
    }

    /// The same, in the rule's busy form when `strict`, with what the eyes
    /// say of who is turned to the lamp.
    pub fn first_output_busy(
        &mut self,
        request: u64,
        room: Room,
        going_on: bool,
        strict: bool,
        turned: Option<bool>,
    ) -> Option<Verdict> {
        if request != self.request || self.verdict.is_some() {
            return None;
        }
        let own = own_request().load(Ordering::Acquire) == request;
        self.judged_own = own;
        let fresh = self.begun <= FRESH_REQUESTS;
        // #829, with the switch: a reply to someone the room had just
        // called by name is that person's conversation, not the lamp's.
        // Only the lamp's name counts for it.
        // #829, 15:40: a line that itself calls someone else by name is
        // theirs too, and in a busy room so is a reply to a question the
        // lamp left unanswered.
        let another = if !self.spoken_to || own {
            None
        } else if self.after_another {
            Some(ANSWERING_ANOTHER)
        } else if addresses_someone_else(&self.heard) && self.heard.contains('?') {
            // Only a question put to that person: a request that opens
            // with a word and a comma is the lamp's (Second on #1090).
            Some(TO_ANOTHER)
        } else if self.after_question && room == Room::Busy {
            Some(AFTER_QUESTION)
        } else {
            None
        };
        let answering_another = another.is_some();
        let going_on = going_on && !answering_another;
        // The lamp's news has one reply: the first thing judged after it,
        // whatever path judges it, takes it (the reviewer on #918 at 07:12:
        // a short "Thanks" judged as a sound had left it for later talk).
        let news_ago = match (self.spoken_to && !own, self.began_us) {
            (true, Some(began)) => self
                .news_us
                .take()
                .filter(|at| *at <= began)
                .map(|at| began - at),
            _ => None,
        };
        // In a conversation held in English, a turn heard mostly in another
        // script, or as a number alone after no question, was misheard
        // (#926): on lamp-4ace on 2026-10-16 at 05:39 a cut-in heard as
        // Hindi was answered in Hindi. It is held and asked again.
        if self.ask_unclear
            && going_on
            && !own
            && let Some(why) = crate::not_speech::unclear_in_conversation(
                &self.heard,
                self.lamp_asked,
                self.cut_in,
            )
        {
            let verdict = Verdict::Hold(why);
            self.unclear = Some((request, why));
            self.verdict = Some(verdict);
            return Some(verdict);
        }
        let called_in_text = if self.name_in_passing_ignored {
            called_by_name(&self.heard)
        } else {
            called(&self.heard)
        };
        // A short sound let through in a quiet room (#800): on lamp-52e6 on
        // 2026-10-15 at 13:03 a room sound of 0.7 s was heard as "¿Qué?" and
        // answered, nobody having spoken to the lamp. It is answered only
        // when what was heard is a short word said to the lamp, or it was
        // said close to the lamp, or someone is turned to it, or it has a
        // word in it and answers the lamp's own question in a conversation.
        if self.sound_in_hand() && !own {
            // A reply to someone the room had just called by name is not
            // the lamp's, short or not (the reviewer on #918 at 06:06): only
            // the lamp's name lets it out.
            let verdict = if let Some(why) = another.filter(|_| !(called_in_text || self.named)) {
                Verdict::Hold(why)
            } else if crate::not_speech::short_word(&self.heard) {
                Verdict::Play(SOUND_A_WORD)
            } else if self.near {
                Verdict::Play(SOUND_NEAR)
            } else if turned == Some(true) {
                Verdict::Play(SOUND_TURNED)
            } else if self.lamp_asked && going_on && self.heard.chars().any(char::is_alphanumeric) {
                // On lamp-4ace on 2026-10-17 at 16:11, "Yes. Please start
                // it." to the lamp's "Shall I start it?" was ended after
                // "Yes." and heard as "bien": held, its handing over was
                // never run. A line with a word in it, right after the
                // lamp's own question in a conversation going on, is the
                // person's answer, and the model judges it.
                Verdict::Play(SOUND_ANSWERS)
            } else {
                Verdict::Hold(SOUND_HELD)
            };
            self.verdict = Some(verdict);
            return Some(verdict);
        }
        // A tap on its head (#711) is a call for what is said, not a sound:
        // a short sound after a tap is still judged as a sound, above.
        if self.tapped && !own {
            let verdict = Verdict::Play(TAPPED);
            self.verdict = Some(verdict);
            return Some(verdict);
        }
        // So is the lamp's name said alone just before: the request after
        // it is what the person called the lamp for.
        if self.after_call && !own {
            let verdict = Verdict::Play(AFTER_CALL);
            self.verdict = Some(verdict);
            return Some(verdict);
        }
        // #829, with the switch: the first thing said after the lamp's own
        // news is a reply to it, outside a busy room; and in every room, a
        // question about its own work is for it. In a busy room a reply to
        // the news is taken only so, or as any other line is (a name, a
        // face, a conversation going on): two colleagues talking a minute
        // or two after the news were answered every line (the Second on
        // #1378), while the battery's "How is that task going?", 46.8 s
        // after it, asks about the lamp's work.
        if self.spoken_to
            && !own
            && !answering_another
            && let Some(began) = self.began_us
        {
            let since = |at: Option<u64>| at.filter(|at| *at <= began).map(|at| began - at);
            let why = if news_ago.is_some_and(|ago| ago <= NEWS_REPLY_US) && room != Room::Busy {
                Some(AFTER_NEWS)
            } else if since(self.worked_us).is_some_and(|ago| ago <= WORK_ASKED_US)
                && about_its_work(&self.heard)
            {
                Some(ABOUT_ITS_WORK)
            } else {
                None
            };
            if let Some(why) = why {
                let verdict = Verdict::Play(why);
                self.verdict = Some(verdict);
                return Some(verdict);
            }
        }
        if strict && room == Room::Busy && self.named_sure {
            let verdict = Verdict::Play(NAMED_PLAINLY);
            self.verdict = Some(verdict);
            return Some(verdict);
        }
        // #829, the first builder at 17:09: with the day's own bars, by day,
        // a name heard close to the lamp also needs a face turned to it or
        // its own words calling it by name. On lamp-4ace on 16 October at
        // 17:08, request 169 played on the near rule alone and answered
        // colleagues talking about eye drops beside it.
        let near_shows = !self.day_bars() || turned == Some(true) || called_by_name(&self.heard);
        if strict && room == Room::Busy && self.near && self.best_score >= NEAR_SCORE && near_shows
        {
            let verdict = Verdict::Play(NAMED_NEAR);
            self.verdict = Some(verdict);
            return Some(verdict);
        }
        let verdict = match decide_called(
            room,
            going_on,
            own,
            fresh,
            called_in_text,
            self.named,
            strict,
            turned,
        ) {
            Verdict::Hold(_) if room == Room::Busy && self.addressed => Verdict::Play(ADDRESSED),
            verdict => verdict,
        };
        // #829, step 2: it holds what nobody said to the lamp, in every
        // room. And a request with the name heard in its sound needs no
        // face in a busy room either: at night nobody faces the lamp
        // (lamp-4ace, 16 October at 03:28, "Hey Lamp, turn your volume down
        // a little." was held for want of one). With the day's own bars, by
        // day, in a busy room with nobody turned to it, the name counts
        // only heard surely, above: the first builder's table at 15:23 had
        // three answers to office talk let out by a hearing of 0.93 to
        // 0.997, and the name was heard 227 times in that room since 07:00.
        let spoken_to = if answering_another {
            called_in_text || self.named
        } else {
            own || going_on
                || called_in_text
                || self.named
                || turned == Some(true)
                || self.addressed
        };
        let verdict = match verdict {
            Verdict::Play(_) if self.spoken_to && !spoken_to => {
                Verdict::Hold(if let Some(why) = another {
                    why
                } else {
                    NOT_SPOKEN_TO
                })
            }
            Verdict::Hold(_)
                if self.spoken_to
                    && self.named
                    && !(self.day_bars() && strict && room == Room::Busy) =>
            {
                Verdict::Play(NAMED)
            }
            verdict => verdict,
        };
        self.verdict = Some(verdict);
        Some(verdict)
    }

    /// What the rule saw when it last judged, with the room, the
    /// conversation and the eyes it was given, as the Policy's fitting rows
    /// take it ([`crate::first_output`]).
    pub fn baseline(
        &self,
        room: Room,
        going_on: bool,
        turned: Option<bool>,
    ) -> crate::first_output::Baseline {
        crate::first_output::Baseline {
            kind: "voice_first_output",
            room: match room {
                Room::Quiet => "quiet",
                Room::Busy => "busy",
                Room::Unknown => "unknown",
            },
            own_turn: self.judged_own,
            going_on,
            named_sure: self.named_sure,
            text: self.heard.clone(),
            facing: turned,
        }
    }

    /// Whether the answer to the request in hand was let out because the
    /// name was heard, for the record: the one case the sound decided.
    pub fn played_for_its_name(&self) -> bool {
        self.verdict == Some(Verdict::Play(NAMED))
    }

    /// Whether it was let out because the name was heard close to the
    /// lamp ([`NEAR_FLAG`]), for the record.
    pub fn played_for_near(&self) -> bool {
        self.verdict == Some(Verdict::Play(NAMED_NEAR))
    }

    /// Whether `request` is the request in hand and its answer is still
    /// to be judged: the first thing [`Self::first_output_busy`] checks.
    pub fn awaits_verdict(&self, request: u64) -> bool {
        request == self.request && self.verdict.is_none()
    }

    /// Whether `request` is the request in hand and its answer has been
    /// judged.
    pub fn judged(&self, request: u64) -> bool {
        request == self.request && self.verdict.is_some()
    }

    /// The request in hand, for the journal.
    pub fn in_hand_request(&self) -> u64 {
        self.request
    }

    /// Whether `request`'s answer is being held back.
    pub fn holds(&self, request: u64) -> bool {
        request == self.request && matches!(self.verdict, Some(Verdict::Hold(_)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The calls held in the played scenes of 2026-10-14 had their name
    /// heard at 0.97 to 0.99 with no face: close to the lamp, with
    /// `--attention-near`, they are calls; from farther off, or with a
    /// weaker name, they are not; and in a room not busy nothing changes.
    /// #800: with `--sounds-checked`, a short sound let through in a quiet
    /// room is answered only when something shows it was said to the lamp.
    /// Without the switch, or for a request that did not end as such a
    /// sound, the rule is as it was.
    #[test]
    fn a_short_sound_is_answered_only_when_something_shows_it_was_said_to_the_lamp() {
        let decide = |checked: bool, sound: bool, heard: &str, near: bool, turned: Option<bool>| {
            let mut attention = Attention::default();
            attention.check_sounds(checked);
            attention.began_at(7, 1, 70_000_000);
            attention.heard(heard);
            attention.set_near(near);
            if sound {
                // Another request's sound is not this one's.
                attention.ended_as_sound(6);
                attention.ended_as_sound(7);
            }
            attention
                .first_output_busy(7, Room::Quiet, true, false, turned)
                .unwrap()
        };
        // The office sound of 13:03 on lamp-52e6.
        assert_eq!(
            decide(true, true, "¿Qué?", false, None),
            Verdict::Hold(SOUND_HELD)
        );
        assert_eq!(
            decide(true, true, "¿Qué?", false, Some(false)),
            Verdict::Hold(SOUND_HELD)
        );
        assert_eq!(
            decide(true, true, "", false, None),
            Verdict::Hold(SOUND_HELD)
        );
        // A short word, said close, or with someone turned to the lamp.
        for word in ["Yes.", "Stop.", "What?", "Yes, go ahead.", " hey Lamp "] {
            assert_eq!(
                decide(true, true, word, false, None),
                Verdict::Play(SOUND_A_WORD),
                "{word}"
            );
        }
        assert_eq!(
            decide(true, true, "Claro.", true, None),
            Verdict::Play(SOUND_NEAR)
        );
        assert_eq!(
            decide(true, true, "¿Qué?", false, Some(true)),
            Verdict::Play(SOUND_TURNED)
        );
        // Without the switch, or not a sound: as before.
        let before = decide(false, false, "¿Qué?", false, None);
        assert!(matches!(before, Verdict::Play(_)));
        assert_eq!(decide(false, true, "¿Qué?", false, None), before);
        assert_eq!(decide(true, false, "¿Qué?", false, None), before);
        // The next request forgets that the last one was a sound.
        let mut attention = Attention::default();
        attention.check_sounds(true);
        attention.began_at(7, 1, 70_000_000);
        attention.ended_as_sound(7);
        assert!(attention.sound_in_hand());
        attention.began_at(8, 1, 80_000_000);
        assert!(!attention.sound_in_hand());
    }

    /// On lamp-4ace on 2026-10-17 at 16:11 "Yes. Please start it.", said
    /// to the lamp's "Shall I start it?", was ended after "Yes." as a short
    /// sound heard as "bien", and its handing over was held. With a word in
    /// it, right after the lamp's own question, in a conversation going on,
    /// it is the person's answer; without any one of the three it is held
    /// as before, and a room sound with no question before it most of all.
    #[test]
    fn a_short_sound_with_a_word_right_after_the_lamps_question_is_its_answer() {
        let decide = |heard: &str, asked: bool, going_on: bool, room: Room| {
            let mut attention = Attention::default();
            attention.check_sounds(true);
            attention.began_at(7, 1, 70_000_000);
            attention.heard(heard);
            attention.ended_as_sound(7);
            attention.lamp_asked(asked);
            attention
                .first_output_busy(7, room, going_on, false, None)
                .unwrap()
        };
        // The scene of 16:11, in every room.
        for room in [Room::Quiet, Room::Busy, Room::Unknown] {
            assert_eq!(
                decide("bien", true, true, room),
                Verdict::Play(SOUND_ANSWERS),
                "{room:?}"
            );
        }
        assert_eq!(
            decide("¿Qué?", true, true, Room::Quiet),
            Verdict::Play(SOUND_ANSWERS)
        );
        // A room sound with no question before it, or no conversation, or
        // no word heard in it: held.
        assert_eq!(
            decide("bien", false, true, Room::Quiet),
            Verdict::Hold(SOUND_HELD)
        );
        assert_eq!(
            decide("¿Qué?", false, true, Room::Quiet),
            Verdict::Hold(SOUND_HELD)
        );
        assert_eq!(
            decide("bien", true, false, Room::Quiet),
            Verdict::Hold(SOUND_HELD)
        );
        for heard in ["", " ", "…", "¿?"] {
            assert_eq!(
                decide(heard, true, true, Room::Quiet),
                Verdict::Hold(SOUND_HELD),
                "{heard:?}"
            );
        }
        // A short word keeps its own reason.
        assert_eq!(
            decide("Yes.", true, true, Room::Quiet),
            Verdict::Play(SOUND_A_WORD)
        );
        // Not checked, or not a sound: the question changes nothing here.
        let unchecked = |sound: bool, checked: bool| {
            let mut attention = Attention::default();
            attention.check_sounds(checked);
            attention.began_at(7, 1, 70_000_000);
            attention.heard("bien");
            if sound {
                attention.ended_as_sound(7);
            }
            attention.lamp_asked(true);
            attention
                .first_output_busy(7, Room::Quiet, true, false, None)
                .unwrap()
        };
        assert_ne!(unchecked(true, false), Verdict::Play(SOUND_ANSWERS));
        assert_ne!(unchecked(false, true), Verdict::Play(SOUND_ANSWERS));
        assert_eq!(unchecked(true, false), unchecked(false, false));
    }

    #[test]
    fn the_head_touch_flag_is_taken_only_first() {
        let flags = |list: &[&str]| list.iter().map(|flag| flag.to_string()).collect::<Vec<_>>();
        let given = flags(&[HEAD_TOUCH_FLAG, "--bus-root", "/run/lampos-bus"]);
        let (on, rest) = take_head_touch(&given);
        assert!(on);
        assert_eq!(rest, &given[1..]);
        let without = flags(&["--bus-root", "/run/lampos-bus"]);
        assert_eq!(take_head_touch(&without), (false, &without[..]));
    }

    /// #711: a head tap asks the lamp to listen. A request that begins
    /// within two seconds of it, the window the lamp shows it listens for,
    /// is a call in any room, even in the strict busy form with nobody seen
    /// turned to the lamp; later ones are not.
    #[test]
    fn a_head_tap_is_a_call_for_the_two_seconds_the_lamp_shows_it_listens() {
        let decide = |tapped_at: Option<u64>, began_at: u64| {
            let mut attention = Attention::default();
            for request in 1..=2 {
                attention.began_at(request, 1, request * 1_000_000);
            }
            if let Some(at) = tapped_at {
                attention.head_tapped(at);
            }
            attention.began_at(3, 1, began_at);
            attention
                .first_output_busy(3, Room::Busy, false, true, None)
                .unwrap()
        };
        let tap = 10_000_000;
        assert_eq!(decide(Some(tap), tap + 500_000), Verdict::Play(TAPPED));
        assert_eq!(
            decide(Some(tap), tap + TAPPED_BEFORE_US),
            Verdict::Play(TAPPED)
        );
        assert!(matches!(
            decide(Some(tap), tap + TAPPED_BEFORE_US + 1),
            Verdict::Hold(_)
        ));
        assert!(matches!(decide(None, tap + 500_000), Verdict::Hold(_)));
        // After the window, a fresh utterance is not called: the face and
        // the look-up have long settled.
        assert!(matches!(
            decide(Some(tap), tap + 3_000_000),
            Verdict::Hold(_)
        ));
        // One that began inside it is, though it goes on past it and its
        // first output comes later.
        let mut attention = Attention::default();
        for request in 1..=2 {
            attention.began_at(request, 1, request * 1_000_000);
        }
        attention.head_tapped(tap);
        attention.began_at(3, 1, tap + 1_900_000);
        attention.ended(3, tap + 4_000_000);
        assert_eq!(
            attention.first_output_busy(3, Room::Busy, false, true, None),
            Some(Verdict::Play(TAPPED))
        );
        // A tap after the request began counts while its sound goes on.
        let mut attention = Attention::default();
        attention.began_at(1, 1, tap);
        attention.head_tapped(tap + 1_000_000);
        assert_eq!(
            attention.first_output_busy(1, Room::Busy, false, true, None),
            Some(Verdict::Play(TAPPED))
        );
        // Not once it has ended, and a new request forgets the old one.
        let mut attention = Attention::default();
        attention.began_at(1, 1, tap);
        attention.ended(1, tap + 2_000_000);
        attention.head_tapped(tap + 3_000_000);
        assert!(matches!(
            attention.first_output_busy(1, Room::Busy, false, true, None),
            Some(Verdict::Hold(_))
        ));
    }

    #[test]
    fn a_name_heard_close_to_the_lamp_is_a_call_in_a_busy_room() {
        let decide = |score: f32, near: bool, room: Room| {
            let mut attention = Attention::default();
            for request in 1..=3 {
                attention.began_at(request, 1, request * 10_000_000);
            }
            attention.name_heard_scored(30_000_000, score);
            attention.set_near(near);
            let verdict = attention
                .first_output_busy(3, room, false, true, None)
                .unwrap();
            (verdict, attention.played_for_near())
        };
        assert_eq!(
            decide(0.97, true, Room::Busy),
            (Verdict::Play(NAMED_NEAR), true)
        );
        assert!(matches!(
            decide(0.97, false, Room::Busy).0,
            Verdict::Hold(_)
        ));
        assert!(matches!(decide(0.93, true, Room::Busy).0, Verdict::Hold(_)));
        assert!(matches!(decide(0.0, true, Room::Busy).0, Verdict::Hold(_)));
        assert!(matches!(
            decide(0.97, true, Room::Quiet).0,
            Verdict::Play(_)
        ));
        // A new request forgets that the last one was close.
        let mut attention = Attention::default();
        attention.began_at(1, 1, 10_000_000);
        attention.set_near(true);
        attention.began_at(2, 1, 20_000_000);
        attention.name_heard_scored(20_000_000, 0.97);
        assert!(matches!(
            attention.first_output_busy(2, Room::Busy, false, true, None),
            Some(Verdict::Hold(_))
        ));
        assert_eq!(attention.window(), Some((20_000_000, None)));
    }

    #[test]
    fn said_from_close_and_facing_lets_out_an_answer_the_busy_rule_would_hold() {
        let decide = |addressed: bool, room: Room, strict: bool, request: u64| {
            let mut attention = Attention::default();
            for began in 1..=request {
                attention.began_at(began, 1, began * 10_000_000);
            }
            attention.set_addressed(addressed);
            attention
                .first_output_busy(request, room, false, strict, None)
                .unwrap()
        };
        // The busy form holds a request nothing shows was for the lamp.
        assert_eq!(decide(true, Room::Busy, true, 3), Verdict::Play(ADDRESSED));
        assert!(matches!(
            decide(false, Room::Busy, true, 3),
            Verdict::Hold(_)
        ));
        // So does the plain form, for a new session's first requests.
        assert_eq!(decide(true, Room::Busy, false, 1), Verdict::Play(ADDRESSED));
        assert!(matches!(
            decide(false, Room::Busy, false, 1),
            Verdict::Hold(_)
        ));
        // Where the rule plays anyway, its own reason stands.
        for (room, strict, request) in [(Room::Quiet, true, 3), (Room::Busy, false, 9)] {
            let verdict = decide(true, room, strict, request);
            assert!(
                matches!(verdict, Verdict::Play(why) if why != ADDRESSED),
                "{verdict:?}"
            );
        }
        // A new request forgets that the last one was said close.
        let mut attention = Attention::default();
        attention.began_at(1, 1, 10_000_000);
        attention.set_addressed(true);
        attention.began_at(2, 1, 20_000_000);
        assert!(matches!(
            attention.first_output_busy(2, Room::Busy, false, true, None),
            Some(Verdict::Hold(_))
        ));
    }

    #[test]
    fn the_request_in_hand_is_what_the_addressed_judgement_takes() {
        let mut attention = Attention::default();
        assert_eq!(attention.in_hand(), None);
        attention.began_at(4, 2, 10_000_000);
        attention.name_heard_scored(10_200_000, 0.97);
        attention.ended(4, 12_000_000);
        assert_eq!(
            attention.in_hand(),
            Some(crate::addressed::Request {
                session: 2,
                request: 4,
                began_us: 10_000_000,
                ended_us: Some(12_000_000),
                name_score: 0.97,
            })
        );
        // The lamp's own turn: nobody spoke in it.
        own_request().store(4_242, Ordering::Release);
        attention.began_at(4_242, 2, 13_000_000);
        assert_eq!(attention.in_hand(), None);
        own_request().store(0, Ordering::Release);
    }

    #[test]
    fn a_request_that_continues_an_early_ended_one_keeps_its_name_in_a_busy_room() {
        // "Hey Lamp, can you ask Claude Code ... about lamps," ended at the
        // comma, and "just in its reply" came as a new request 3.5 s after
        // the name began.
        let first_part = |attention: &mut Attention| {
            for request in 1..=3 {
                attention.began_at(request, 1, request * 10_000_000);
            }
            attention.name_heard_scored(30_100_000, 0.97);
            attention.heard("Hey Lamp, can you ask Claude Code about lamps,");
            attention.set_near(true);
            attention.ended(3, 33_000_000);
        };
        let mut continued = Attention::default();
        first_part(&mut continued);
        continued.continued_at(4, 1, 33_600_000);
        assert_eq!(continued.window(), Some((30_000_000, None)));
        assert_eq!(
            continued.first_output_busy(4, Room::Busy, false, true, None),
            Some(Verdict::Play(NAMED_NEAR))
        );
        // The same request begun afresh: the name was too long ago, and the
        // busy rule holds it, as on lamp-4ace.
        let mut afresh = Attention::default();
        first_part(&mut afresh);
        afresh.began_at(4, 1, 33_600_000);
        assert!(matches!(
            afresh.first_output_busy(4, Room::Busy, false, true, None),
            Some(Verdict::Hold(_))
        ));
        // The text heard so far goes on too: the service's "Lamp," shows
        // the call, as it would have in one request.
        let mut text = Attention::default();
        text.began_at(1, 1, 10_000_000);
        text.heard("Lamp, ");
        text.continued_at(2, 1, 12_000_000);
        text.heard("what is the time?");
        assert_eq!(
            text.first_output_busy(2, Room::Busy, false, false, None),
            Some(Verdict::Play("the text shows it was called"))
        );
    }

    #[test]
    fn a_continuation_keeps_nothing_across_sessions_or_without_a_first_part() {
        let mut other = Attention::default();
        other.began_at(1, 1, 10_000_000);
        other.name_heard_scored(10_100_000, 0.999);
        other.heard("Lamp, ");
        other.continued_at(2, 2, 12_000_000);
        assert_eq!(other.window(), Some((12_000_000, None)));
        assert!(matches!(
            other.first_output_busy(2, Room::Busy, false, true, None),
            Some(Verdict::Hold(_))
        ));
        // Said to the lamp from close to it in the first part: still so.
        let mut close = Attention::default();
        close.began_at(1, 1, 10_000_000);
        close.set_addressed(true);
        close.continued_at(2, 1, 12_000_000);
        assert_eq!(
            close.first_output_busy(2, Room::Busy, false, true, None),
            Some(Verdict::Play(ADDRESSED))
        );
        let mut none = Attention::default();
        none.continued_at(1, 1, 10_000_000);
        assert_eq!(none.window(), Some((10_000_000, None)));
        assert!(keeps_name(Some("on")));
        for value in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
            assert!(!keeps_name(value), "{value:?}");
        }
    }

    #[test]
    fn busy_near_is_the_busy_rule_with_the_near_call() {
        assert_eq!(arguments(Some("busy-near")), [FLAG, BUSY_FLAG, NEAR_FLAG]);
        assert_eq!(arguments(Some("busy")), [FLAG, BUSY_FLAG]);
        let flags = vec![NEAR_FLAG.to_owned(), "--x".to_owned()];
        assert_eq!(take_near(&flags), (true, &flags[1..]));
        assert_eq!(take_near(&flags[1..]), (false, &flags[1..]));
    }

    /// Played calls on 2026-10-14 afternoon: the name heard at 1.000 in a
    /// busy room with no face to see is a call; at 0.997, as people's talk
    /// scored that morning, it is not.
    #[test]
    fn in_a_busy_room_a_name_heard_plainly_is_a_call_without_a_face() {
        for (score, plays) in [(1.0f32, true), (0.999, true), (0.997, false)] {
            let mut attention = Attention::default();
            for request in 1..=3 {
                attention.began_at(request, 1, request * 10_000_000);
            }
            attention.name_heard_scored(30_000_000, score);
            let verdict = attention
                .first_output_busy(3, Room::Busy, false, true, None)
                .unwrap();
            assert_eq!(
                matches!(verdict, Verdict::Play(_)),
                plays,
                "{score}: {verdict:?}"
            );
        }
    }

    /// The meeting of 2026-10-14: a session long past its first requests,
    /// a busy room. The busy form holds what nothing shows was meant for the
    /// lamp, a name heard in the sound with nobody turned to it included,
    /// and lets out what was called in the text or asked with a look.
    #[test]
    fn in_its_busy_form_a_busy_room_needs_a_sign_on_every_request() {
        let busy = |heard: &str, named: bool, turned: Option<bool>| {
            decide_busy(Room::Busy, false, false, false, heard, named, true, turned)
        };
        assert!(matches!(
            busy("What is 2 + 2?", false, None),
            Verdict::Hold(_)
        ));
        assert!(matches!(busy("", false, Some(false)), Verdict::Hold(_)));
        assert_eq!(
            busy("", true, Some(false)),
            Verdict::Hold(
                "a busy room: its name was heard in the sound, and nobody is turned to the lamp"
            )
        );
        assert!(matches!(busy("", true, None), Verdict::Hold(_)));
        assert_eq!(busy("", true, Some(true)), Verdict::Play(TURNED));
        assert_eq!(busy("", false, Some(true)), Verdict::Play(TURNED));
        assert_eq!(
            busy("Lamp, what is 2 + 2?", false, None),
            Verdict::Play("the text shows it was called")
        );
        // A conversation with the lamp, and its own turns, go on.
        assert!(matches!(
            decide_busy(Room::Busy, true, false, false, "", false, true, None),
            Verdict::Play(_)
        ));
        assert!(matches!(
            decide_busy(Room::Busy, false, true, false, "", false, true, None),
            Verdict::Play(_)
        ));
        // Any other room: as before, whatever the form.
        for room in [Room::Quiet, Room::Unknown] {
            assert!(matches!(
                decide_busy(room, false, false, false, "", false, true, None),
                Verdict::Play(_)
            ));
            assert!(matches!(
                decide_busy(room, false, false, true, "", false, true, None),
                Verdict::Play(_)
            ));
        }
        // And without the busy form, nothing changes past a session's
        // first requests.
        assert!(matches!(
            decide_busy(
                Room::Busy,
                false,
                false,
                false,
                "",
                true,
                false,
                Some(false)
            ),
            Verdict::Play(_)
        ));
        assert_eq!(
            decide_busy(Room::Busy, false, false, true, "", true, false, None),
            Verdict::Play(NAMED)
        );
    }

    #[test]
    fn busy_adds_its_flag_after_the_rules_own() {
        assert_eq!(arguments(Some("busy")), [FLAG, BUSY_FLAG]);
        let flags = [FLAG.to_owned(), BUSY_FLAG.to_owned(), "rest".to_owned()];
        let (on, rest) = take(&flags);
        let (busy, rest) = take_busy(rest);
        assert!(on && busy);
        assert_eq!(rest, ["rest"]);
        let (busy, rest) = take_busy(&flags[2..]);
        assert!(!busy);
        assert_eq!(rest, ["rest"]);
    }

    #[test]
    fn only_on_turns_it_on_and_the_flag_is_taken_from_the_front() {
        assert_eq!(arguments(Some("on")), [FLAG]);
        assert!(arguments(Some("yes")).is_empty());
        assert!(arguments(None).is_empty());
        let flags = [FLAG.to_owned(), "rest".to_owned()];
        assert_eq!(take(&flags), (true, &flags[1..]));
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
    }

    #[test]
    fn the_name_in_passing_switch_is_ignored_only_and_its_flag_follows_near() {
        assert_eq!(passing_arguments(Some("ignored")), [PASSING_FLAG]);
        assert!(passing_arguments(Some("on")).is_empty());
        assert!(passing_arguments(None).is_empty());
        let flags = [PASSING_FLAG.to_owned(), "rest".to_owned()];
        assert_eq!(take_passing(&flags), (true, &flags[1..]));
        assert_eq!(take_passing(&flags[1..]), (false, &flags[1..]));
    }

    #[test]
    fn the_spoken_to_switch_is_on_or_day_and_night_and_its_flags_follow_the_passing_one() {
        assert_eq!(spoken_to_arguments(Some("on")), [SPOKEN_TO_FLAG]);
        assert_eq!(
            spoken_to_arguments(Some("day-and-night")),
            [SPOKEN_TO_FLAG, BY_DAY_FLAG]
        );
        for value in ["ignored", "by-day", "ON", ""] {
            assert!(spoken_to_arguments(Some(value)).is_empty(), "{value}");
        }
        assert!(spoken_to_arguments(None).is_empty());
        let flags = [SPOKEN_TO_FLAG.to_owned(), "rest".to_owned()];
        assert_eq!(take_spoken_to(&flags), (true, &flags[1..]));
        assert_eq!(take_spoken_to(&flags[1..]), (false, &flags[1..]));
        let both = [PASSING_FLAG.to_owned(), SPOKEN_TO_FLAG.to_owned()];
        let (passing, rest) = take_passing(&both);
        assert_eq!((passing, take_spoken_to(rest)), (true, (true, &[][..])));
        let by_day = [SPOKEN_TO_FLAG.to_owned(), BY_DAY_FLAG.to_owned()];
        let (spoken_to, rest) = take_spoken_to(&by_day);
        assert_eq!((spoken_to, take_by_day(rest)), (true, (true, &[][..])));
        assert_eq!(take_by_day(&by_day), (false, &by_day[..]));
    }

    /// #829, step 2: with the switch an answer is played, in every room,
    /// only when something shows the lamp was spoken to; without it the
    /// rule is as it was. The lines are the first builder's two-person
    /// scene and the office's talk on lamp-4ace (15 and 16 October).
    #[test]
    fn with_the_switch_only_what_was_said_to_the_lamp_is_played_in_every_room() {
        // The request in hand, with `heard`, the name heard in its sound
        // or not, and said close to the lamp or not; then the verdict.
        let judge = |spoken_to: bool,
                     room: Room,
                     going_on: bool,
                     heard: &str,
                     named: bool,
                     addressed: bool,
                     turned: Option<bool>| {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(spoken_to);
            attention.began_at(81, 9, 10_000_000);
            if named {
                attention.name_heard_scored(10_200_000, 0.97);
            }
            attention.set_addressed(addressed);
            attention.heard(heard);
            attention
                .first_output_busy(81, room, going_on, false, turned)
                .unwrap()
        };
        const BUDGET: &str = "Did you get my message about the budget?";
        const SUSHI: &str = "Voglio mangiare sushi.";
        for room in [Room::Quiet, Room::Unknown, Room::Busy] {
            for text in [BUDGET, SUSHI, "Thailand."] {
                // Nobody spoke to the lamp: held with the switch, for that
                // reason where the room is not known to be busy, and for
                // the busy room's own where it is.
                let held = if room == Room::Busy {
                    "a new session in a busy room, and nothing shows it was called"
                } else {
                    NOT_SPOKEN_TO
                };
                for turned in [None, Some(false)] {
                    assert_eq!(
                        judge(true, room, false, text, false, false, turned),
                        Verdict::Hold(held),
                        "{room:?} {text} {turned:?}"
                    );
                }
                // Each sign alone lets it out.
                for (going_on, named, addressed, turned) in [
                    (true, false, false, None),
                    (false, true, false, None),
                    (false, false, true, None),
                    (false, false, false, Some(true)),
                ] {
                    assert!(
                        matches!(
                            judge(true, room, going_on, text, named, addressed, turned),
                            Verdict::Play(_)
                        ),
                        "{room:?} {text} {going_on} {named} {addressed} {turned:?}"
                    );
                }
            }
            // The name in the text.
            assert!(matches!(
                judge(
                    true,
                    room,
                    false,
                    "Hey Lamp, what is the weather like in Hanoi today?",
                    false,
                    false,
                    None
                ),
                Verdict::Play(_)
            ));
        }
        // Without the switch nothing changes: a room not known to be busy
        // plays them all, and a busy one holds them as before.
        for room in [Room::Quiet, Room::Unknown] {
            assert_eq!(
                judge(false, room, false, BUDGET, false, false, None),
                Verdict::Play("the room is not known to be busy")
            );
        }
        assert_eq!(
            judge(false, Room::Busy, false, BUDGET, false, false, None),
            Verdict::Hold("a new session in a busy room, and nothing shows it was called")
        );
        // A session the rule trusts plays what nobody said to the lamp
        // without the switch, and holds it with.
        let trusted = |spoken_to: bool| {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(spoken_to);
            // 11 s apart: past `ANOTHER_REPLY_US`, so that no line is a
            // reply to the question before it (`AFTER_QUESTION`).
            for request in 1..=u64::from(FRESH_REQUESTS) + 1 {
                attention.began_at(request, 9, request * 11_000_000);
                attention.heard(BUDGET);
                attention.first_output(request, Room::Busy, false);
            }
            attention.verdict.unwrap()
        };
        assert_eq!(
            trusted(false),
            Verdict::Play("the session has heard enough to be trusted")
        );
        assert_eq!(trusted(true), Verdict::Hold(NOT_SPOKEN_TO));
    }

    /// #829, the first builder's day scene (16 October, 02:30), in the
    /// busy room two people had left: "How is that task going?" 45.3 s
    /// after the lamp's news was held, and so was everything after it.
    /// With the switch the first thing said within three minutes of the
    /// news is a reply to it, once; and a question about the lamp's own
    /// work is for it for ten minutes. Without it, both are held.
    #[test]
    fn with_the_switch_the_lamps_news_is_replied_to_and_its_work_asked_about() {
        const S: u64 = 1_000_000;
        let day = |spoken_to: bool| {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(spoken_to);
            // The lamp's news of the task it was given.
            own_request().store(7_829, Ordering::Release);
            attention.began_at(7_829, 4, 100 * S);
            assert_eq!(
                attention.first_output_busy(7_829, Room::Busy, false, true, None),
                Some(Verdict::Play("the lamp began this turn itself"))
            );
            attention.spoke(7_829, 100 * S);
            own_request().store(0, Ordering::Release);
            let mut judge = |request: u64, at: u64, heard: &str| {
                attention.began_at(request, 4, at * S);
                attention.heard(heard);
                attention
                    .first_output_busy(request, Room::Busy, false, true, None)
                    .unwrap()
            };
            [
                // 45.3 s after the news began to be said: in a busy room a
                // reply to the news is taken for asking about its work
                // (#1378).
                judge(1, 146, "How is that task going?"),
                // The window is spent: what follows is judged as before.
                judge(2, 150, "What's my favorite color?"),
                // Still about the lamp's own work, within ten minutes.
                judge(3, 400, "Is the timer still running?"),
                // Not about it.
                judge(4, 410, "What time is it?"),
                // About work, too long after.
                judge(5, 800, "Did the task finish?"),
            ]
        };
        let held = Verdict::Hold(
            "a busy room, and nothing shows it was called or that anyone is turned to the lamp",
        );
        assert_eq!(
            day(true),
            [
                Verdict::Play(ABOUT_ITS_WORK),
                held,
                Verdict::Play(ABOUT_ITS_WORK),
                held,
                held
            ]
        );
        assert_eq!(day(false), [held; 5]);
        // A task started is work too.
        let mut attention = Attention::default();
        attention.speak_when_spoken_to(true);
        attention.worked(10 * S);
        attention.began_at(9, 4, 300 * S);
        attention.heard("Did you set the reminder?");
        assert_eq!(
            attention.first_output(9, Room::Quiet, false),
            Some(Verdict::Play(ABOUT_ITS_WORK))
        );
        assert!(about_its_work("How is that TASK going?"));
        assert!(!about_its_work("I'll ask him about it."));
    }

    /// A busy room after the lamp's own news, with `strict` and no face, as
    /// the worker keeps the conversation: each line at `at_s` seconds after
    /// the news began; the lamp's answer goes into the thread 2 s after each
    /// line it plays.
    fn after_news(spoken_to: bool, room: Room, lines: &[(u64, &str)]) -> Vec<Verdict> {
        const S: u64 = 1_000_000;
        let start = std::time::Instant::now();
        let at = |us: u64| start + std::time::Duration::from_micros(us);
        let mut thread = crate::thread::Thread::new();
        let mut attention = Attention::default();
        attention.speak_when_spoken_to(spoken_to);
        own_request().store(9_828, Ordering::Release);
        attention.began_at(9_828, 4, 100 * S);
        assert_eq!(
            attention.first_output_busy(9_828, room, false, true, Some(false)),
            Some(Verdict::Play("the lamp began this turn itself"))
        );
        attention.spoke(9_828, 100 * S);
        thread.lamp(
            9_828,
            "Oh, Claude Code just finished the story for you.",
            at(100 * S),
        );
        own_request().store(0, Ordering::Release);
        let mut verdicts = Vec::new();
        for (request, (at_ms, line)) in (1..).zip(lines) {
            let began = 100 * S + at_ms * 1_000;
            attention.began_at(request, 4, began);
            attention.heard(line);
            thread.person(line, true, at(began));
            let going_on = thread.going_on(at(began));
            let verdict = attention
                .first_output_busy(request, room, going_on, true, Some(false))
                .unwrap();
            if matches!(verdict, Verdict::Play(_)) {
                thread.lamp(request, "Here it is.", at(began + 2 * S));
            }
            attention.ended(request, began + S);
            verdicts.push(verdict);
        }
        verdicts
    }

    const GOING_ON: Verdict = Verdict::Play("a conversation with the lamp is going on");
    const BUSY_HELD: Verdict = Verdict::Hold(
        "a busy room, and nothing shows it was called or that anyone is turned to the lamp",
    );

    /// The first builder's battery on lamp-4ace on 17 October at 19:02
    /// (#828, 19:22; the lines on #1378 at 20:41), with
    /// `LAMP_ATTENTION=busy-near`, a busy room and nobody's face seen (the
    /// camera saw the floor). The news began at 153.46 s and ended at
    /// 165.00 s. "How is that task going?" came 46.8 s after it began, just
    /// past the 45 s a conversation lasts, and was held; so was every line
    /// after it, since a held line gets no answer and starts no
    /// conversation. With the switch, that line asks about the lamp's work,
    /// and its answer makes the next three a conversation going on.
    #[test]
    fn with_the_switch_the_battery_after_the_lamps_news_does_not_go_deaf() {
        let battery = [
            (46_770, "How is that task going?"),
            (65_610, "Remind me in two minutes to stretch."),
            (82_960, "What's my favourite colour?"),
            (99_280, "What time is it?"),
        ];
        let mut answered = vec![Verdict::Play(ABOUT_ITS_WORK)];
        answered.extend([GOING_ON; 3]);
        assert_eq!(after_news(true, Room::Busy, &battery), answered);
        assert_eq!(after_news(false, Room::Busy, &battery), [BUSY_HELD; 4]);
    }

    /// The first builder's spec (#828, 19:22, and #705, 19:55): after the
    /// news, five follow-ups 17 s apart with no face and no name, all
    /// answered with the switch, the first asking about the lamp's work
    /// 100 s after the news; without it, all held. And talk between two
    /// people is still held: a line calling someone else by a made-up name,
    /// 17 s on inside the conversation, and one a minute after it, the
    /// conversation over. A first line within the news's own 45 s is
    /// answered either way, as a conversation going on; without the switch
    /// the line to someone else is answered too there.
    #[test]
    fn with_the_switch_five_follow_ups_after_the_news_are_answered_and_talk_is_held() {
        let at = |first: u64| {
            [
                (first, "Is the task done?"),
                (first + 17_000, "And the other one?"),
                (first + 34_000, "How long did it take?"),
                (first + 51_000, "Was anything left over?"),
                (first + 68_000, "Okay, what comes next?"),
                (
                    first + 85_000,
                    "Zemba, are you coming to the meeting at two?",
                ),
                (first + 145_000, "Did you get my message about the budget?"),
            ]
        };
        let mut answered = vec![Verdict::Play(ABOUT_ITS_WORK)];
        answered.extend([GOING_ON; 4]);
        answered.extend([BUSY_HELD; 2]);
        assert_eq!(after_news(true, Room::Busy, &at(100_000)), answered);
        assert_eq!(after_news(false, Room::Busy, &at(100_000)), [BUSY_HELD; 7]);
        assert_eq!(after_news(true, Room::Busy, &at(17_000)), answered);
        let mut unswitched = vec![GOING_ON; 6];
        unswitched.push(BUSY_HELD);
        assert_eq!(after_news(false, Room::Busy, &at(17_000)), unswitched);
    }

    /// The Second on #1378 (20:29): with `AFTER_NEWS` taken on timing alone,
    /// two colleagues talking to each other 120 s after the lamp's news, no
    /// name and no face, were answered every line, the first as a reply to
    /// the news and the rest as a conversation going on. In a busy room a
    /// reply to the news is now taken only when it asks about the lamp's
    /// work, or as any other line is: their talk is held, with the switch
    /// and without it. Within the news's own 45 s their talk is a
    /// conversation going on either way. Outside a busy room the first line
    /// after the news is still a reply to it, whatever it says.
    #[test]
    fn two_people_talking_soon_after_the_news_in_a_busy_room_are_held() {
        let at = |first: u64| {
            [
                (first, "Did you get my message about the budget?"),
                (first + 4_000, "Yes, I'll look at it after lunch."),
                (first + 8_000, "Can you send me the numbers first?"),
                (first + 12_000, "Sure, give me ten minutes."),
                (first + 16_000, "Thanks, that works."),
            ]
        };
        for spoken_to in [true, false] {
            assert_eq!(
                after_news(spoken_to, Room::Busy, &at(120_000)),
                [BUSY_HELD; 5],
                "{spoken_to}"
            );
            assert_eq!(
                after_news(spoken_to, Room::Busy, &at(30_000)),
                [GOING_ON; 5],
                "{spoken_to}"
            );
        }
        // The cost of the narrowing: a reply that says nothing of the work,
        // 100 s after the news, is held in a busy room and taken in a quiet
        // one.
        let reply = [(100_000, "Can you check it once more?")];
        assert_eq!(after_news(true, Room::Busy, &reply), [BUSY_HELD]);
        assert_eq!(
            after_news(true, Room::Quiet, &reply),
            [Verdict::Play(AFTER_NEWS)]
        );
    }

    /// #918, the reviewer at 05:11: the lamp's own turn opens the reply
    /// window only once its sound goes out. Ended silent, or cancelled
    /// before it spoke, it opens none, and the talk after it is judged as
    /// any other. Only the turn in hand, and only the lamp's own, opens one.
    #[test]
    fn an_own_turn_that_said_nothing_is_no_news_to_reply_to() {
        const S: u64 = 1_000_000;
        let after = |spoke: Option<u64>| {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(true);
            own_request().store(8_918, Ordering::Release);
            attention.began_at(8_918, 4, 100 * S);
            attention.first_output(8_918, Room::Quiet, false);
            if let Some(request) = spoke {
                attention.spoke(request, 101 * S);
            }
            own_request().store(0, Ordering::Release);
            attention.began_at(1, 4, 110 * S);
            attention.heard("Did you get my message about the budget?");
            attention.first_output(1, Room::Quiet, false).unwrap()
        };
        assert_eq!(after(Some(8_918)), Verdict::Play(AFTER_NEWS));
        assert_eq!(after(None), Verdict::Hold(NOT_SPOKEN_TO));
        assert_eq!(after(Some(8_917)), Verdict::Hold(NOT_SPOKEN_TO));
        // The news has one reply, however it is judged: a short "Thanks"
        // let through as a sound takes it as the same words heard as
        // speech do, and later talk is judged as any other (the reviewer on
        // #918 at 07:12).
        for sound in [true, false] {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(true);
            attention.check_sounds(true);
            own_request().store(8_919, Ordering::Release);
            attention.began_at(8_919, 4, 100 * S);
            attention.first_output(8_919, Room::Quiet, false);
            attention.spoke(8_919, 101 * S);
            own_request().store(0, Ordering::Release);
            attention.began_at(1, 4, 110 * S);
            if sound {
                attention.ended_as_sound(1);
            }
            attention.heard("Thanks.");
            assert!(matches!(
                attention.first_output(1, Room::Quiet, false),
                Some(Verdict::Play(_))
            ));
            attention.began_at(2, 4, 220 * S);
            attention.heard("Did you get my message about the budget?");
            assert_eq!(
                attention.first_output(2, Room::Quiet, false),
                Some(Verdict::Hold(NOT_SPOKEN_TO)),
                "{sound}"
            );
        }
        // A person's request that is answered is no news.
        let mut attention = Attention::default();
        attention.speak_when_spoken_to(true);
        attention.began_at(2, 4, 100 * S);
        attention.heard("Hey Lamp, what time is it?");
        attention.first_output(2, Room::Quiet, false);
        attention.spoke(2, 101 * S);
        attention.began_at(3, 4, 110 * S);
        attention.heard("Did you get my message about the budget?");
        assert_eq!(
            attention.first_output(3, Room::Quiet, false),
            Some(Verdict::Hold(NOT_SPOKEN_TO))
        );
    }

    /// #1054: a request after a line with words that someone said and the
    /// lamp left unanswered follows that line, not the lamp; after a line
    /// the lamp answered, 11 s on, a sound with no words, or a short sound
    /// let through and ended as one though heard as a word (Fourth's probe
    /// on #1054), it does not. Whatever the switch.
    #[test]
    fn a_request_after_a_line_the_lamp_left_unanswered_follows_that_line() {
        const S: u64 = 1_000_000;
        const ASKED: &str = "Are you coming to the meeting at two?";
        let follows = |asked: &str, answered: bool, sound: bool, gap_s: u64| {
            let mut attention = Attention::default();
            attention.check_sounds(true);
            attention.began_at(41, 7, 100 * S);
            attention.heard(asked);
            if answered {
                assert!(matches!(
                    attention.first_output_busy(41, Room::Quiet, true, false, None),
                    Some(Verdict::Play(_))
                ));
            }
            if sound {
                attention.ended_as_sound(41);
            }
            attention.ended(41, 103 * S);
            attention.began_at(42, 7, (103 + gap_s) * S);
            attention.heard("Yes, give me five minutes.");
            (
                attention.follows_an_unanswered_line(42),
                attention.follows_an_unanswered_line(41),
            )
        };
        assert_eq!(follows(ASKED, false, false, 1), (true, false));
        assert_eq!(follows(ASKED, true, false, 1), (false, false));
        assert_eq!(follows(ASKED, false, false, 11), (false, false));
        assert_eq!(follows("", false, false, 1), (false, false));
        assert_eq!(follows("¿Qué?", false, true, 1), (false, false));
        // Heard as the same word but not ended as a sound: someone's line.
        assert_eq!(follows("¿Qué?", false, false, 1), (true, false));
        // The first request of all follows nothing.
        let mut attention = Attention::default();
        attention.began_at(1, 7, S);
        assert!(!attention.follows_an_unanswered_line(1));
    }

    /// #829, the first builder at 15:40, the two-person round on lamp-4ace:
    /// Samantha's "Daniel, are you coming to the meeting at two?", Daniel's
    /// "Yes, give me five minutes.", and the lamp, which had just told
    /// Daniel a story, set a five-minute timer. With the switch, a line that
    /// itself calls someone else by name is theirs, even when it and the
    /// reply are one request; and in a busy room, a reply to a question the
    /// lamp left unanswered is the asker's, named or not. The lamp's name
    /// still counts, a question the lamp answered leaves the next line
    /// alone, and in a quiet room an unnamed question changes nothing.
    #[test]
    fn with_the_switch_a_line_to_someone_else_or_after_an_unanswered_question_is_not_the_lamps() {
        const S: u64 = 1_000_000;
        // Two lines: `asked`, let out if `answered` (else no output), and
        // `answer` `gap_s` later, in `room`, a conversation with the lamp
        // going on. In a busy room the reason held is the busy room's own,
        // as the conversation no longer counts.
        let reply = |asked: &str, answered: bool, gap_s: u64, answer: &str, room: Room| {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(true);
            attention.began_at(41, 7, 100 * S);
            attention.heard(asked);
            if answered {
                assert!(matches!(
                    attention.first_output_busy(41, room, true, false, None),
                    Some(Verdict::Play(_))
                ));
            }
            attention.ended(41, 103 * S);
            attention.began_at(42, 7, (103 + gap_s) * S);
            attention.heard(answer);
            attention
                .first_output_busy(42, room, true, false, None)
                .unwrap()
        };
        const ANSWER: &str = "Yes, give me five minutes.";
        // One request: the call and its reply together, in any room.
        let together = |room: Room| {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(true);
            attention.began_at(40, 7, 10 * S);
            attention
                .heard("Daniel, are you coming to the meeting at two? Yes, give me five minutes.");
            attention.first_output_busy(40, room, true, false, None)
        };
        assert_eq!(together(Room::Quiet), Some(Verdict::Hold(TO_ANOTHER)));
        assert!(matches!(together(Room::Busy), Some(Verdict::Hold(_))));
        // Second on #1090: requests to the lamp that open with a word and a
        // comma are the lamp's, in a conversation with it.
        for line in [
            "Tomorrow, remind me to call the bank.",
            "Tonight, set an alarm for seven.",
            "Seriously, what time is it?",
            "Basically, can you dim the light?",
            "Maybe, turn it up a little.",
            "Daniel's here, can you say hello?",
            "True, but can you check the client's address?",
        ] {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(true);
            attention.began_at(50, 7, 10 * S);
            attention.heard(line);
            assert!(
                matches!(
                    attention.first_output_busy(50, Room::Quiet, true, false, None),
                    Some(Verdict::Play(_))
                ),
                "{line}"
            );
        }
        // In a busy room, a reply to a question the lamp left unanswered.
        const ASKED: &str = "Are you coming to the meeting at two?";
        assert!(matches!(
            reply(ASKED, false, 1, ANSWER, Room::Busy),
            Verdict::Hold(_)
        ));
        // The lamp's name still counts.
        assert!(matches!(
            reply(ASKED, false, 1, "Lamp, give me five minutes.", Room::Busy),
            Verdict::Play(_)
        ));
        // A question the lamp answered, one long before, a line that was
        // not a question, or a quiet room: as before.
        for (asked, answered, gap_s, room) in [
            (ASKED, true, 1, Room::Busy),
            (ASKED, false, 11, Room::Busy),
            ("We are coming to the meeting at two.", false, 1, Room::Busy),
            (ASKED, false, 1, Room::Quiet),
        ] {
            assert!(
                matches!(
                    reply(asked, answered, gap_s, ANSWER, room),
                    Verdict::Play(_)
                ),
                "{asked} {answered} {gap_s} {room:?}"
            );
        }
    }

    /// #829, the first builder at 05:02, scene 4 on lamp-4ace: Samantha's
    /// "Daniel, are you coming to the meeting at two?", then Daniel's "Yes,
    /// give me five minutes." while he was in a conversation with the lamp,
    /// which set a timer. With the switch, a reply to someone the room had
    /// just called by name is not the lamp's conversation, facing it or
    /// not; the lamp's name still counts. Without it, as before.
    #[test]
    fn with_the_switch_a_reply_to_someone_called_by_name_is_not_the_lamps() {
        const S: u64 = 1_000_000;
        let reply =
            |spoken_to: bool, called: &str, gap_s: u64, answer: &str, turned: Option<bool>| {
                let mut attention = Attention::default();
                attention.speak_when_spoken_to(spoken_to);
                attention.began_at(31, 6, 100 * S);
                attention.heard(called);
                attention.ended(31, 103 * S);
                attention.began_at(32, 6, (103 + gap_s) * S);
                attention.heard(answer);
                attention
                    .first_output_busy(32, Room::Quiet, true, false, turned)
                    .unwrap()
            };
        const CALLED: &str = "Daniel, are you coming to the meeting at two?";
        const ANSWER: &str = "Yes, give me five minutes.";
        for turned in [None, Some(true)] {
            assert_eq!(
                reply(true, CALLED, 1, ANSWER, turned),
                Verdict::Hold(ANSWERING_ANOTHER)
            );
        }
        for called in ["Hey Samantha, did you see it?", "Are you coming, Daniel?"] {
            assert_eq!(
                reply(true, called, 1, ANSWER, None),
                Verdict::Hold(ANSWERING_ANOTHER),
                "{called}"
            );
        }
        // The lamp's name still counts.
        assert!(matches!(
            reply(true, CALLED, 1, "Lamp, give me five minutes.", None),
            Verdict::Play(_)
        ));
        // No one called by name, or long before: the conversation goes on.
        for (called, gap_s) in [
            ("Did you get my message about the budget?", 1),
            ("Okay, that is enough.", 1),
            ("Lamp, what time is it?", 1),
            (CALLED, 15),
        ] {
            assert!(
                matches!(reply(true, called, gap_s, ANSWER, None), Verdict::Play(_)),
                "{called} {gap_s}"
            );
        }
        // Without the switch, as before.
        assert!(matches!(
            reply(false, CALLED, 1, ANSWER, None),
            Verdict::Play(_)
        ));
        // The same reply as a short sound let through and checked (the
        // reviewer on #918 at 06:06): a short word, said close, or with a
        // face turned to the lamp, is still the other person's; the lamp's
        // name still lets it out, and without the switch it plays.
        let short = |spoken_to: bool, answer: &str, near: bool, turned: Option<bool>| {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(spoken_to);
            attention.check_sounds(true);
            attention.began_at(31, 6, 100 * S);
            attention.heard(CALLED);
            attention.ended(31, 103 * S);
            attention.began_at(32, 6, 104 * S);
            attention.ended_as_sound(32);
            attention.set_near(near);
            attention.heard(answer);
            attention
                .first_output_busy(32, Room::Quiet, true, false, turned)
                .unwrap()
        };
        for (near, turned) in [(false, None), (true, None), (false, Some(true))] {
            assert_eq!(
                short(true, "Yes.", near, turned),
                Verdict::Hold(ANSWERING_ANOTHER),
                "{near} {turned:?}"
            );
        }
        assert!(matches!(
            short(true, "Lamp?", false, None),
            Verdict::Play(_)
        ));
        assert_eq!(
            short(false, "Yes.", false, None),
            Verdict::Play(SOUND_A_WORD)
        );
    }

    #[test]
    fn someone_called_by_name_is_told_from_the_lamp_and_from_words_that_open_a_sentence() {
        for calls in [
            "Daniel, are you coming to the meeting at two?",
            "Hey Samantha, did you see it?",
            "Are you coming, Daniel?",
            "Karen, the budget is due.",
        ] {
            assert!(addresses_someone_else(calls), "{calls}");
        }
        for not in [
            "Lamp, what time is it?",
            "Hey Lamp, tell me a story.",
            "Can you explain that, Lamp?",
            "Man, tell me a short joke.",
            "Okay, stop.",
            "Well, I think so.",
            "Yes, give me five minutes.",
            "Did you get my message about the budget?",
            "I told Daniel about it.",
            "",
        ] {
            assert!(!addresses_someone_else(not), "{not}");
        }
    }

    /// #829, the first builder at 03:29: a request said with the name
    /// needs no face, in a busy room too. Without the switch the busy form
    /// holds the name heard in the sound with nobody turned to the lamp;
    /// with it, it is played. With the day's own bars (`day-and-night`) it
    /// is played at night, and by day held as without the switch (the
    /// first builder at 15:23). And the lamp's own turn is always played.
    #[test]
    fn with_the_switch_the_name_needs_no_face_and_the_lamps_own_turn_plays() {
        let named = |spoken_to: bool, by_day: bool, night: bool| {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(spoken_to);
            attention.judge_by_day(by_day);
            attention.set_night(night);
            attention.began_at(5, 3, 10_000_000);
            attention.name_heard_scored(10_100_000, 0.97);
            attention.heard("turn your volume down a little.");
            attention.first_output_busy(5, Room::Busy, false, true, Some(false))
        };
        let held = Some(Verdict::Hold(
            "a busy room: its name was heard in the sound, and nobody is turned to the lamp",
        ));
        assert_eq!(named(false, false, false), held);
        assert_eq!(named(false, true, true), held);
        // `on`, by day or at night.
        assert_eq!(named(true, false, false), Some(Verdict::Play(NAMED)));
        assert_eq!(named(true, false, true), Some(Verdict::Play(NAMED)));
        // `day-and-night`.
        assert_eq!(named(true, true, true), Some(Verdict::Play(NAMED)));
        assert_eq!(named(true, true, false), held);
        let mut attention = Attention::default();
        attention.speak_when_spoken_to(true);
        own_request().store(6_829, Ordering::Release);
        attention.began_at(6_829, 3, 20_000_000);
        assert_eq!(
            attention.first_output(6_829, Room::Quiet, false),
            Some(Verdict::Play("the lamp began this turn itself"))
        );
        own_request().store(0, Ordering::Release);
    }

    /// #829, the first builder at 15:23: lamp-4ace in the busy office by
    /// day, nobody turned to the lamp, with the switch. Every answer let out
    /// that day for the name heard in the room's sound, by lamp-name's
    /// score: four played calls and three hearings in office talk. With the
    /// day's own bars the name alone counts by day only heard surely
    /// (0.999), close to the lamp or not (the 17:09 sign). Which of the
    /// seven were close was not recorded; each is judged both ways. With a
    /// face turned, in a quiet room, or at night, a hearing at the 0.9
    /// floor still counts. With `on` all seven are let out, the three in
    /// office talk too: no score of these separates them (the reviewer on
    /// #1158).
    #[test]
    fn with_the_days_bars_in_a_busy_room_by_day_the_name_alone_counts_only_heard_surely() {
        let judge = |score: f32, near: bool, room: Room, turned: Option<bool>, night: bool| {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(true);
            attention.judge_by_day(true);
            attention.set_night(night);
            attention.began_at(9, 4, 10_000_000);
            attention.name_heard_scored(10_100_000, score);
            attention.set_near(near);
            attention.heard("What time is it?");
            let verdict = attention.first_output_busy(9, room, false, true, turned);
            matches!(verdict, Some(Verdict::Play(_)))
        };
        let by_day = |score, near| judge(score, near, Room::Busy, Some(false), false);
        // (time, score, for the lamp)
        let seven = [
            ("07:12", 0.996, true),
            ("07:14", 0.949, true),
            ("07:16", 0.998, true),
            ("09:42", 0.995, true),
            ("14:58", 0.932, false),
            ("15:12", 0.933, false),
            ("15:14", 0.997, false),
        ];
        for (at, score, _) in seven {
            for near in [false, true] {
                let mut attention = Attention::default();
                attention.speak_when_spoken_to(true);
                attention.began_at(9, 4, 10_000_000);
                attention.name_heard_scored(10_100_000, score);
                attention.set_near(near);
                attention.heard("What time is it?");
                let on = attention.first_output_busy(9, Room::Busy, false, true, Some(false));
                assert!(matches!(on, Some(Verdict::Play(_))), "{at} with on");
            }
            // Not close: none is let out by its score alone.
            assert!(!by_day(score, false), "{at}");
            // Close, by day: not on that alone either, the first builder's
            // 17:09 sign; at night, at 0.95 or more.
            assert!(!by_day(score, true), "{at}");
            assert!(
                judge(score, true, Room::Busy, Some(false), true),
                "{at} at night"
            );
        }
        assert!(by_day(SURE_SCORE, false));
        for score in [0.9, 0.932] {
            assert!(judge(score, false, Room::Busy, Some(true), false), "turned");
            assert!(
                judge(score, false, Room::Quiet, Some(false), false),
                "quiet"
            );
            assert!(judge(score, false, Room::Busy, Some(false), true), "night");
        }
    }

    /// #829, the first builder at 17:09: request 169 on lamp-4ace at 17:08
    /// played on the near rule alone, and the lamp answered colleagues
    /// talking about eye drops beside it ("I can't recommend specific eye
    /// drops."). The voice journals no transcripts, so the line here is one
    /// of that shape, not the words heard. With the day's own bars, by day,
    /// a name heard close to the lamp at 0.96 in a busy room also needs a
    /// face turned to it or its words calling it by name; at night, with
    /// `on`, or without the switch, the near rule stands as it was.
    #[test]
    fn with_the_days_bars_a_name_heard_close_by_day_also_needs_a_face_or_the_words_calling_it() {
        let near = |said: &str, turned: Option<bool>, night: bool, spoken_to: bool| {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(spoken_to);
            attention.judge_by_day(true);
            attention.set_night(night);
            attention.began_at(169, 4, 10_000_000);
            attention.name_heard_scored(10_100_000, 0.96);
            attention.set_near(true);
            attention.heard(said);
            attention.first_output_busy(169, Room::Busy, false, true, turned)
        };
        let talk = "Have you tried the eye drops from the pharmacy downstairs?";
        assert!(matches!(
            near(talk, Some(false), false, true),
            Some(Verdict::Hold(_))
        ));
        assert!(matches!(
            near(talk, None, false, true),
            Some(Verdict::Hold(_))
        ));
        assert!(matches!(
            near(talk, Some(true), false, true),
            Some(Verdict::Play(_))
        ));
        assert!(matches!(
            near(
                "Lamp, which eye drops should I use?",
                Some(false),
                false,
                true
            ),
            Some(Verdict::Play(_))
        ));
        assert_eq!(
            near(talk, Some(false), true, true),
            Some(Verdict::Play(NAMED_NEAR))
        );
        assert_eq!(
            near(talk, Some(false), false, false),
            Some(Verdict::Play(NAMED_NEAR))
        );
        // `on`, by day.
        let mut attention = Attention::default();
        attention.speak_when_spoken_to(true);
        attention.began_at(169, 4, 10_000_000);
        attention.name_heard_scored(10_100_000, 0.96);
        attention.set_near(true);
        attention.heard(talk);
        assert_eq!(
            attention.first_output_busy(169, Room::Busy, false, true, Some(false)),
            Some(Verdict::Play(NAMED_NEAR))
        );
    }

    /// #829: with the switch the name counts when it is said to the lamp,
    /// as every call the service wrote on 2026-10-13 said it, and not when
    /// the lamp is spoken of; without it every one of these is a call.
    #[test]
    fn the_name_said_to_the_lamp_is_a_call_and_the_lamp_spoken_of_is_not_with_the_switch() {
        for shows in [
            "Lamp, what is two plus two?",
            "Hey Lamp, tell me a joke.",
            "What time is it, Lamp?",
            "Hey, Lamb, how many days are there in a week?",
            "Milan, what is the capital of Italy?",
            "Man, tell me a joke.",
            "Ok Lam, stop.",
            "Hey Lan",
            "Helen, what color is the sky on a clear day?",
            "Weiland, tell me a joke.",
            "Can you, lamp, turn the light down?",
            "Hey Lamp, write a short story about a lamp who learns to sing.",
            // At the end, set off by a comma: the reviewer's on #864.
            "Can you explain that, Lamp?",
            "What is this, Lamp?",
            "Thanks, Lamp.",
            "Okay. Lamp, what time is it?",
        ] {
            assert!(called_by_name(shows), "{shows}");
            assert!(called(shows), "{shows}");
        }
        for spoken_of in [
            "I left the lamp on in the kitchen.",
            "I left the lamb on in the kitchen.",
            "Did you see the lamp turn its head this morning?",
            "Yes, the lamp looked at me when I came in.",
            "My lamp is broken again.",
            "Ask the lamp what time it is.",
            "That lamp's light is too bright.",
            // A lamp spoken of with words before it: the reviewer's on #864.
            "I bought a new lamp yesterday.",
            "The old lamp stopped working.",
            "Did you see her desk lamp?",
            "The lamp, the chair and the table are new.",
        ] {
            assert!(!called_by_name(spoken_of), "{spoken_of}");
            assert!(called(spoken_of), "{spoken_of}");
        }
        for silent in [
            "How was your weekend?",
            "The lamps in the storage room are new.",
            "",
        ] {
            assert!(!called_by_name(silent), "{silent}");
        }
    }

    /// The rule in a busy room, strict as on lamp-4ace: "I left the lamp on"
    /// lets an answer out as a call without the switch, and is held with
    /// it, as other talk is; a call by name plays either way.
    #[test]
    fn the_lamp_spoken_of_in_a_busy_room_is_held_only_with_the_switch() {
        let decide = |text: &str, ignored: bool| {
            let mut attention = Attention::default();
            attention.ignore_name_in_passing(ignored);
            attention.began_at(1, 1, 10_000_000);
            attention.heard(text);
            attention.first_output_busy(1, Room::Busy, false, true, Some(false))
        };
        for passing in [
            "I left the lamp on in the kitchen.",
            "I bought a new lamp yesterday.",
            "The old lamp stopped working.",
            "Did you see her desk lamp?",
        ] {
            assert_eq!(
                decide(passing, false),
                Some(Verdict::Play("the text shows it was called")),
                "{passing}"
            );
            assert!(
                matches!(decide(passing, true), Some(Verdict::Hold(_))),
                "{passing}"
            );
        }
        for call in [
            "Hey Lamp, what time is it?",
            "Can you explain that, Lamp?",
            "What is this, Lamp?",
        ] {
            for ignored in [false, true] {
                assert_eq!(
                    decide(call, ignored),
                    Some(Verdict::Play("the text shows it was called")),
                    "{call}"
                );
            }
        }
    }

    /// What the service really wrote for called requests in the lamp's
    /// records of 2026-10-13, and what it wrote for other people's talk.
    #[test]
    fn the_text_can_show_a_call_by_the_name_or_by_what_the_service_writes_in_its_place() {
        for shows in [
            "Lamp, what is two plus two?",
            "Hey Lamp, tell me a joke.",
            "What time is it, Lamp?",
            "Hey, Lamb, how many days are there in a week?",
            "Milan, what is the capital of Italy?",
            "Milan, how are you feeling right now?",
            "Man, tell me a joke.",
            "Ok Lam, stop.",
            "Hey Lan",
            "plan",
            "Helen, what color is the sky on a clear day?",
            "Hey Mom, what time is it?",
            "Amen.",
            "Weiland, tell me a joke.",
        ] {
            assert!(called(shows), "{shows}");
        }
        for silent in [
            // Called, and written with nothing left of the name: the text
            // cannot show it, which is why it is never read as "not called".
            "What color is the sky on a clear day?",
            "What is your favorite color?",
            // Other people's talk.
            "How was your weekend?",
            "Do you know where the new monitors went?",
            "The lamps in the storage room are new.",
            "We planned it with the man from Milan.",
            "Did you ask Helen about the report?",
            "Bây giờ mình có một vấn đề hơi khó.",
            "",
        ] {
            assert!(!called(silent), "{silent}");
        }
    }

    /// Each line of the rule. Only one case holds an answer back.
    #[test]
    fn only_a_new_sessions_first_requests_in_a_busy_room_are_held_and_every_doubt_plays() {
        let weekend = "How was your weekend?";
        assert_eq!(
            decide(Room::Busy, false, false, true, weekend, false),
            Verdict::Hold("a new session in a busy room, and nothing shows it was called")
        );
        // No text at all is held too: the text can only ever speak for
        // playing, and its absence says nothing.
        assert!(matches!(
            decide(Room::Busy, false, false, true, "", false),
            Verdict::Hold(_)
        ));
        // The same request on a session that has heard enough: played.
        assert!(matches!(
            decide(Room::Busy, false, false, false, weekend, false),
            Verdict::Play("the session has heard enough to be trusted")
        ));
        assert!(matches!(
            decide(
                Room::Busy,
                false,
                false,
                true,
                "Milan, how was your weekend?",
                false,
            ),
            Verdict::Play("the text shows it was called")
        ));
        assert!(matches!(
            decide(Room::Busy, true, false, true, weekend, false),
            Verdict::Play("a conversation with the lamp is going on")
        ));
        assert!(matches!(
            decide(Room::Busy, false, true, true, weekend, false),
            Verdict::Play("the lamp began this turn itself")
        ));
        for room in [Room::Quiet, Room::Unknown] {
            assert!(matches!(
                decide(room, false, false, true, weekend, false),
                Verdict::Play("the room is not known to be busy")
            ));
        }
        // The name heard in the sound plays what would have been held,
        // whatever the text says or does not say.
        for text in [weekend, ""] {
            assert_eq!(
                decide(Room::Busy, false, false, true, text, true),
                Verdict::Play("its name was heard in the room's sound")
            );
        }
    }

    /// #829, 17:21: output of a request that isn't the one in hand is never
    /// judged, and the journal says so; the request in hand is judged once.
    #[test]
    fn only_the_request_in_hand_is_judged_and_it_says_so() {
        let mut attention = Attention::default();
        attention.began_at(36, 4, 10_000_000);
        attention.began_at(37, 4, 11_000_000);
        assert_eq!(attention.first_output(36, Room::Quiet, false), None);
        assert!(!attention.judged(36) && attention.in_hand_request() == 37);
        assert!(attention.first_output(37, Room::Quiet, false).is_some());
        assert!(attention.judged(37));
        assert_eq!(attention.first_output(37, Room::Quiet, false), None);
    }

    /// #829, the first builder at 16:32, the owner on lamp-4ace: "Lamp,"
    /// said alone (its call's wait ran out, and the model was told only the
    /// name came), then "Check my work." as a request of its own, which
    /// was held in a busy room by day with nobody turned. It begins within
    /// `AFTER_CALL_US` of the call, and is played. Later, or the call's own
    /// request, it is judged as before.
    #[test]
    fn a_request_just_after_the_lamps_name_said_alone_is_called() {
        const S: u64 = 1_000_000;
        let after = |gap_us: u64| {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(true);
            attention.began_at(20, 3, 10 * S);
            attention.heard("Lamp.");
            attention.ended(20, 11 * S);
            attention.bare_call(20, 13 * S);
            attention.began_at(21, 3, 13 * S + gap_us);
            attention.heard("Check my work.");
            attention.first_output_busy(21, Room::Busy, false, true, Some(false))
        };
        assert_eq!(after(800_000), Some(Verdict::Play(AFTER_CALL)));
        assert_eq!(after(AFTER_CALL_US), Some(Verdict::Play(AFTER_CALL)));
        assert!(matches!(after(AFTER_CALL_US + 1), Some(Verdict::Hold(_))));
        // The call's own request is not called by itself.
        let mut attention = Attention::default();
        attention.bare_call(20, 13 * S);
        attention.began_at(20, 3, 13 * S + 500_000);
        assert!(matches!(
            attention.first_output_busy(20, Room::Busy, false, true, Some(false)),
            Some(Verdict::Hold(_))
        ));
    }

    /// The reviewer on #1108: a bare call counts only when its own hearing
    /// shows the lamp was called, as the busy room judges one. "Hey Clamp?"
    /// with the name heard at 0.92, its call's wait run out, then "Can you
    /// look at this?" 1 s later in a busy room with nobody turned: held, as
    /// it would be without the call. The same call heard at 0.999, or at
    /// 0.96 close to the lamp, lets the next request out; "Hey, Zorp?" at
    /// 0.999 calls someone else, and does not.
    #[test]
    fn a_bare_call_counts_only_when_its_own_hearing_shows_the_lamp_was_called() {
        const S: u64 = 1_000_000;
        let after = |said: &str, score: f32, near: bool| {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(true);
            attention.began_at(20, 3, 10 * S);
            attention.name_heard_scored(10 * S + 200_000, score);
            attention.heard(said);
            attention.set_near(near);
            attention.ended(20, 11 * S);
            attention.bare_call(20, 13 * S);
            attention.began_at(21, 3, 14 * S);
            attention.heard("Can you look at this?");
            attention.first_output_busy(21, Room::Busy, false, true, Some(false))
        };
        assert!(matches!(
            after("Hey Clamp?", 0.92, false),
            Some(Verdict::Hold(_))
        ));
        assert!(matches!(
            after("Hey Clamp?", 0.96, false),
            Some(Verdict::Hold(_))
        ));
        assert_eq!(
            after("Hey Clamp?", 0.999, false),
            Some(Verdict::Play(AFTER_CALL))
        );
        assert_eq!(
            after("Hey Clamp?", 0.96, true),
            Some(Verdict::Play(AFTER_CALL))
        );
        assert_eq!(after("Lamp?", 0.92, false), Some(Verdict::Play(AFTER_CALL)));
        assert!(matches!(
            after("Hey, Zorp?", 0.999, false),
            Some(Verdict::Hold(_))
        ));
        // Two lines within the 3 s after a real call are both its.
        let mut attention = Attention::default();
        attention.speak_when_spoken_to(true);
        attention.began_at(20, 3, 10 * S);
        attention.heard("Lamp?");
        attention.ended(20, 11 * S);
        attention.bare_call(20, 13 * S);
        attention.began_at(21, 3, 13 * S + 500_000);
        attention.heard("Check my work.");
        assert_eq!(
            attention.first_output_busy(21, Room::Busy, false, true, Some(false)),
            Some(Verdict::Play(AFTER_CALL))
        );
        attention.began_at(22, 3, 15 * S);
        attention.heard("And the second page.");
        assert_eq!(
            attention.first_output_busy(22, Room::Busy, false, true, Some(false)),
            Some(Verdict::Play(AFTER_CALL))
        );
    }

    /// The reviewer on #1158: request 169's shape, cut off as a bare call.
    /// With the day's own bars, by day, a bare call heard close to the lamp
    /// at 0.96 shows nothing on that alone, as for a request (the first
    /// builder's 17:09 sign), so the line after it within 3 s is not its.
    /// At night, or with `on`, closeness shows it as before; its words
    /// calling the lamp, or a sure hearing, show it by day too.
    #[test]
    fn with_the_days_bars_a_bare_call_close_by_day_needs_its_words_or_a_sure_hearing() {
        const S: u64 = 1_000_000;
        let after = |said: &str, score: f32, by_day: bool, night: bool| {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(true);
            attention.judge_by_day(by_day);
            attention.set_night(night);
            attention.began_at(20, 3, 10 * S);
            attention.name_heard_scored(10 * S + 200_000, score);
            attention.heard(said);
            attention.set_near(true);
            attention.ended(20, 11 * S);
            attention.bare_call(20, 13 * S);
            attention.began_at(21, 3, 14 * S);
            attention.heard("Can you look at this?");
            attention.first_output_busy(21, Room::Busy, false, true, Some(false))
        };
        let talk = "Have you tried the eye drops";
        assert!(matches!(
            after(talk, 0.96, true, false),
            Some(Verdict::Hold(_))
        ));
        assert_eq!(
            after(talk, 0.96, true, true),
            Some(Verdict::Play(AFTER_CALL))
        );
        assert_eq!(
            after(talk, 0.96, false, false),
            Some(Verdict::Play(AFTER_CALL))
        );
        assert_eq!(
            after("Lamp?", 0.96, true, false),
            Some(Verdict::Play(AFTER_CALL))
        );
        assert_eq!(
            after(talk, 0.999, true, false),
            Some(Verdict::Play(AFTER_CALL))
        );
    }

    /// The name is the first thing said, a little before the voice is
    /// sure someone is speaking. It counts for the request that begins
    /// then, and for no later one.
    #[test]
    fn the_name_heard_as_a_request_began_plays_it_and_is_not_carried_to_the_next() {
        let second = 1_000_000u64;
        let mut attention = Attention::default();
        // Heard 0.4 s before the request began.
        attention.name_heard(10 * second - 400_000);
        attention.began_at(1, 7, 10 * second);
        assert_eq!(
            attention.first_output(1, Room::Busy, false),
            Some(Verdict::Play("its name was heard in the room's sound"))
        );
        assert!(attention.played_for_its_name() && !attention.holds(1));
        // The next request, twenty seconds on, with no name: held.
        attention.began_at(2, 7, 30 * second);
        assert!(matches!(
            attention.first_output(2, Room::Busy, false),
            Some(Verdict::Hold(_))
        ));
        assert!(!attention.played_for_its_name() && attention.holds(2));
        // A name heard in the middle of a request counts for it too.
        let mut attention = Attention::default();
        attention.began_at(1, 8, 50 * second);
        attention.name_heard(51 * second);
        assert!(matches!(
            attention.first_output(1, Room::Busy, false),
            Some(Verdict::Play(_))
        ));
        // To the limit: 1.5 s before counts, a moment more does not.
        for (before, plays) in [(NAME_BEFORE_US, true), (NAME_BEFORE_US + 1, false)] {
            let mut attention = Attention::default();
            attention.name_heard(80 * second - before);
            attention.began_at(1, 9, 80 * second);
            let verdict = attention.first_output(1, Room::Busy, false).unwrap();
            assert_eq!(matches!(verdict, Verdict::Play(_)), plays, "{before}");
        }
        // Nothing is listening for the name, or it was not heard: as before.
        let mut attention = Attention::default();
        attention.began_at(1, 10, 90 * second);
        assert!(matches!(
            attention.first_output(1, Room::Busy, false),
            Some(Verdict::Hold(_))
        ));
        // A request that began at no known time takes no name, however
        // lately one was heard.
        let mut attention = Attention::default();
        attention.name_heard(95 * second);
        attention.began(1, 11);
        assert!(matches!(
            attention.first_output(1, Room::Busy, false),
            Some(Verdict::Hold(_))
        ));
    }

    /// A name that begins to be heard after a request's sound has ended
    /// was not said in it, and one heard in it is not lost to a later one.
    #[test]
    fn a_name_counts_for_a_request_only_if_it_began_before_the_requests_sound_ended() {
        let second = 1_000_000;
        // Someone else's "Hey Liam" a second after the person finished.
        let mut attention = Attention::default();
        attention.began_at(1, 20, 10 * second);
        attention.ended(1, 12 * second);
        attention.name_heard(13 * second);
        assert!(matches!(
            attention.first_output(1, Room::Busy, false),
            Some(Verdict::Hold(_))
        ));
        // Said at the very end of the request, and read by the worker only
        // after its sound had ended: it is dated, and counts.
        let mut attention = Attention::default();
        attention.began_at(1, 21, 10 * second);
        attention.ended(1, 12 * second);
        attention.name_heard(12 * second);
        assert!(matches!(
            attention.first_output(1, Room::Busy, false),
            Some(Verdict::Play(_))
        ));
        // Called by name, and then the other line: the call still counts.
        let mut attention = Attention::default();
        attention.name_heard(10 * second - 200_000);
        attention.began_at(1, 22, 10 * second);
        attention.ended(1, 12 * second);
        attention.name_heard(13 * second);
        assert!(matches!(
            attention.first_output(1, Room::Busy, false),
            Some(Verdict::Play(_))
        ));
        // The late name is the next request's, if that begins soon enough,
        // and another request's end changes nothing.
        attention.began_at(2, 22, 14 * second);
        attention.ended(1, 14 * second);
        attention.name_heard(15 * second);
        assert!(matches!(
            attention.first_output(2, Room::Busy, false),
            Some(Verdict::Play(_))
        ));
    }

    /// A session's first two requests are the fresh ones; the count starts
    /// again with each new session, and a request is decided once.
    #[test]
    fn the_first_two_requests_of_each_session_are_the_ones_held() {
        let mut attention = Attention::default();
        let outcome = |attention: &mut Attention, request: u64, session: u64| {
            attention.began(request, session);
            attention.heard("How was your weekend?");
            attention.first_output(request, Room::Busy, false)
        };
        assert!(matches!(
            outcome(&mut attention, 1, 5),
            Some(Verdict::Hold(_))
        ));
        assert!(attention.holds(1) && !attention.holds(2));
        // Decided once: the room changing mid-answer changes nothing.
        assert_eq!(attention.first_output(1, Room::Quiet, true), None);
        assert!(attention.holds(1));
        assert!(matches!(
            outcome(&mut attention, 2, 5),
            Some(Verdict::Hold(_))
        ));
        assert!(matches!(
            outcome(&mut attention, 3, 5),
            Some(Verdict::Play(_))
        ));
        assert!(matches!(
            outcome(&mut attention, 4, 5),
            Some(Verdict::Play(_))
        ));
        // A new session: its first two again.
        assert!(matches!(
            outcome(&mut attention, 5, 6),
            Some(Verdict::Hold(_))
        ));
        assert!(matches!(
            outcome(&mut attention, 6, 6),
            Some(Verdict::Hold(_))
        ));
        assert!(matches!(
            outcome(&mut attention, 7, 6),
            Some(Verdict::Play(_))
        ));
        // Another request's output decides nothing.
        attention.began(8, 7);
        assert_eq!(attention.first_output(7, Room::Busy, false), None);
        // A turn the lamp began itself is played whatever was heard.
        attention.began(9, 8);
        attention.heard("so did you send the report");
        own_request().store(9, Ordering::Release);
        assert!(matches!(
            attention.first_output(9, Room::Busy, false),
            Some(Verdict::Play("the lamp began this turn itself"))
        ));
        own_request().store(0, Ordering::Release);
    }

    #[test]
    fn what_is_kept_of_one_request_is_bounded_and_cut_at_a_character() {
        let mut attention = Attention::default();
        attention.began(1, 1);
        attention.heard(&"é".repeat(MOST_HEARD_BYTES));
        assert_eq!(attention.heard.len(), MOST_HEARD_BYTES);
        attention.heard("Lamp");
        assert_eq!(attention.heard.len(), MOST_HEARD_BYTES);
        assert!(attention.heard.chars().all(|c| c == 'é'));
    }
}
