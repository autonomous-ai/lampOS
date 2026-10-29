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
/// A name heard at least this surely counts as a call when it is close:
/// setup's measurement uses the same bar (`lamp_near_call::SCORE`).
pub const NEAR_SCORE: f32 = lamp_near_call::SCORE;
/// Close: the room microphone this far over its quiet during the request,
/// unless this home's own bar is used ([`near_rise_db`]).
pub const NEAR_RISE_DB: f32 = lamp_near_call::DEFAULT_RISE_DB;
/// `on`: the near rule's bar is this home's, measured at setup from the
/// person's own chair (#753, `lamp_near_call::FILE`), in place of
/// [`NEAR_RISE_DB`]. Off unless set; without a fitting file the bar stays.
/// Passed to the Live and the second route's workers, which judge with it.
pub const NEAR_CALIBRATED_ENVIRONMENT: &str = "LAMP_ATTENTION_NEAR_CALIBRATED";
/// How often the file is looked at again: a new measurement is used within
/// this, with no restart.
pub const NEAR_READ_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

/// The near rule's bar now, in dB over the room's quiet: this home's with
/// [`NEAR_CALIBRATED_ENVIRONMENT`] on and a file that fits, else
/// [`NEAR_RISE_DB`]. Looks at the file at most every [`NEAR_READ_EVERY`],
/// and reads it only when it has changed. The journal says the bar in use
/// when it changes.
pub fn near_rise_db() -> f32 {
    static BAR: std::sync::OnceLock<Option<std::sync::Mutex<NearBar>>> = std::sync::OnceLock::new();
    BAR.get_or_init(|| {
        (std::env::var(NEAR_CALIBRATED_ENVIRONMENT).ok().as_deref() == Some("on"))
            .then(|| std::sync::Mutex::new(NearBar::new(lamp_near_call::FILE.into())))
    })
    .as_ref()
    .and_then(|bar| {
        bar.lock()
            .ok()
            .map(|mut bar| bar.now(std::time::Instant::now()))
    })
    .unwrap_or(NEAR_RISE_DB)
}

/// This home's bar as last read from its file ([`near_rise_db`]).
#[derive(Debug)]
pub struct NearBar {
    path: std::path::PathBuf,
    looked: Option<std::time::Instant>,
    modified: Option<Option<std::time::SystemTime>>,
    bar: f32,
}

impl NearBar {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self {
            path,
            looked: None,
            modified: None,
            bar: NEAR_RISE_DB,
        }
    }

    /// The bar at `now`: the file's, looked at again once
    /// [`NEAR_READ_EVERY`] has passed, or [`NEAR_RISE_DB`] with none that
    /// fits.
    pub fn now(&mut self, now: std::time::Instant) -> f32 {
        if self
            .looked
            .is_some_and(|looked| now.saturating_duration_since(looked) < NEAR_READ_EVERY)
        {
            return self.bar;
        }
        self.looked = Some(now);
        let modified = std::fs::metadata(&self.path)
            .and_then(|metadata| metadata.modified())
            .ok();
        if self.modified == Some(modified) {
            return self.bar;
        }
        let first = self.modified.is_none();
        self.modified = Some(modified);
        let read = lamp_near_call::read(&self.path);
        let bar = read.unwrap_or(NEAR_RISE_DB);
        if first || bar != self.bar {
            eprintln!(
                "lamp-voice: the near rule's bar is {bar:.1} dB over the room's quiet ({})",
                if read.is_some() {
                    "this home's, measured at setup"
                } else {
                    "the default: no measurement that fits"
                }
            );
        }
        self.bar = bar;
        bar
    }
}
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

/// Whether a line held for `why` was said to someone else by name, or in
/// reply to someone the room had just called by name: people talking to
/// each other. With `LAMP_NAME_OR_FOLLOW_UP`, in a room not known to be
/// busy, it is talk (`room_told::RoomTold::count_held_talk`); a line held
/// only as said to nobody ([`NOT_SPOKEN_TO`]) is talk only with someone in
/// view and not turned to the lamp (`room_told::RoomTold::seen_talking`). The
/// holds given only in a busy room, [`AFTER_QUESTION`] and
/// [`TURNED_NOT_SPOKEN_TO`], never come there, so they are not among them.
pub(crate) fn said_to_another(why: &str) -> bool {
    [ANSWERING_ANOTHER, TO_ANOTHER].contains(&why)
}

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
    addresses_someone_else_on(heard, false)
}

/// [`addresses_someone_else`], with the words written for the lamp's name
/// standing alone ([`STAND_ALONE_INS`]) taken as the lamp's, never as a
/// person's name, when `stand_alone_ins`: on the second route only
/// ([`Attention::count_stand_alone_ins`]).
pub fn addresses_someone_else_on(heard: &str, stand_alone_ins: bool) -> bool {
    let alone = stand_alone(stand_alone_ins);
    let is_name = |word: &str| {
        word.chars().next().is_some_and(char::is_uppercase)
            && word.chars().all(char::is_alphabetic)
            && !NOT_NAMES
                .iter()
                .chain(NOT_NAMES_OPENING)
                .any(|not| word.eq_ignore_ascii_case(not))
            && !STAND_INS
                .iter()
                .chain(alone)
                .any(|not| word.eq_ignore_ascii_case(not))
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

/// More words the service writes for the lamp's name, counted as its call
/// only standing alone, set off as a name is ("Hey, Lynn. How are you
/// today?", "Thank you, Lynn."), and never as someone else's name (#1712,
/// 19 Oct on lamp-4ace: "Hey Lamp, how are you today?" and "Lamp, what
/// time is it?" came back with "Lynn" and were held as said to someone
/// else; "Len", "Palin" and "Palan" on lamp-52e6 on 15 Oct; "Land" and
/// "Line" in the first builder's trials; "Lin" and "Lump" as the first
/// builder asked). "Line up the meetings" is no call.
///
/// On the second route only (#1973, the first builder at 22:38): Deepgram
/// writes "Lamp" so, Gemini's transcript doesn't, and Linh and Lin are
/// common names, so "Lin, pass me that." between people stays held on a
/// Live lamp.
///
/// "Louvre" is the word written for "Lamp," most often: 27 of the played
/// lines on lamp-4ace from 22 to 25 October, every one said as "Lamp" or
/// "Hey Lamp". 15 of them were held, 11 as said to someone it calls by
/// name ("Louvre, what did I just ask you?" five times of five). Nobody's
/// name, and lamp-name heard none of those five, so the near rule
/// (`LAMP_NAME_NEAR`) could not take it. "Lampy" too: on lamp-4ace on 26 Oct
/// (6999, 14:1x) "Lamp, what did I just ask you?" came back as "Lampy, …"
/// and was held as said to someone it calls by name.
const STAND_ALONE_INS: &[&str] = &[
    "lynn", "len", "lin", "lump", "land", "line", "palin", "palan", "louvre", "lampy",
];

/// Words a line opens with, set off, when it is said to a person, not to
/// the lamp (`LAMP_NAME_SAID_TO_A_PERSON=on`, #3945): "Ma'am, could you
/// pass me the salt please?" was heard as the lamp's name at 0.95 to 0.99
/// at its start and answered. None is the lamp's name, a word written for
/// it ([`STAND_INS`], [`STAND_ALONE_INS`]; "man" and "mom" stay those, and
/// the text calls them before a hearing matters), or one of its shape.
pub(crate) const SAID_TO_A_PERSON: &[&str] = &[
    "ma'am", "madam", "sir", "mum", "dad", "honey", "sweetie", "babe", "dude", "bro", "buddy",
    "guys",
];

/// The word on [`SAID_TO_A_PERSON`] that `heard` opens with, set off as a
/// name is ([`crate::second_provider::first_word_set_off`]), compared lower
/// case with its apostrophe, a curly one as a straight one.
pub(crate) fn said_to_a_person(heard: &str) -> Option<&'static str> {
    let heard = heard.replace('\u{2019}', "'");
    let (start, end) = crate::second_provider::first_word_set_off(&heard)?;
    let word = heard[start..end].to_lowercase();
    SAID_TO_A_PERSON.iter().copied().find(|said| *said == word)
}

/// [`STAND_ALONE_INS`] when they count, else none.
fn stand_alone(counted: bool) -> &'static [&'static str] {
    if counted { STAND_ALONE_INS } else { &[] }
}

/// Greetings that may come before the name of a call ("Hey Lamp,").
const GREETINGS: &[&str] = &["hey", "hi", "ok", "okay", "oh", "hello"];

/// Whether `word` is a greeting a call may open with ([`GREETINGS`]).
pub(crate) fn is_greeting(word: &str) -> bool {
    GREETINGS
        .iter()
        .any(|greeting| word.eq_ignore_ascii_case(greeting))
}

/// Whether `word` is the lamp's name or a word the service has written for
/// it ([`STAND_INS`], [`STAND_ALONE_INS`]): for a name a person gives, where
/// such a word opening the line called the lamp (`known_faces`).
pub(crate) fn written_for_the_name(word: &str) -> bool {
    ["lamp", "lamb"]
        .iter()
        .chain(STAND_INS)
        .chain(STAND_ALONE_INS)
        .any(|known| word.eq_ignore_ascii_case(known))
}

/// [`called`], with the name counted only where it is said to the lamp
/// (`LAMP_NAME_IN_PASSING=ignored`, #829): opening a sentence, after any
/// greeting ("Lamp, ...", "Hey Lamp, ...", "Okay. Lamp, ..."), or right
/// after a comma ("..., Lamp?", "Can you, lamp, ..."). "I left the lamp
/// on", "a new lamp", "her desk lamp" are not calls. What the service
/// writes in the name's place is read as [`called`] reads it: first, or
/// after a greeting.
pub fn called_by_name(heard: &str) -> bool {
    called_by_name_on(heard, false)
}

/// [`called_by_name`], with the words written for the lamp's name standing
/// alone ([`STAND_ALONE_INS`]) counted as its call when `stand_alone_ins`:
/// on the second route only ([`Attention::count_stand_alone_ins`]).
pub fn called_by_name_on(heard: &str, stand_alone_ins: bool) -> bool {
    name_said_to(heard, stand_alone_ins, true)
}

/// A literal "lamp" or "lamb" in a call position. The busy-room text
/// exemption has no name-listener evidence for recognizer stand-ins.
pub(crate) fn called_by_real_name(heard: &str) -> bool {
    name_said_to(heard, false, false)
}

fn name_said_to(heard: &str, stand_alone_ins: bool, recognizer_stand_ins: bool) -> bool {
    let alone = stand_alone(stand_alone_ins);
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
        // The name, or a word written for it set off as a name is: nothing
        // after it but a comma, a stop or the line's end.
        let standing_alone = is(word, alone)
            && matches!(
                tokens.get(at + 1),
                None | Some(Token::Comma) | Some(Token::Stop)
            );
        if !(is(word, &["lamp", "lamb"]) || standing_alone) {
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
    said_to || (recognizer_stand_ins && called_by_stand_in(&words))
}

/// Whether what was heard is the lamp's name alone, or a word written for
/// it, with greetings at most: "Lamp.", "Hey Lamp,", "Hey, Lynn." A call cut
/// off at a pause, the rest still to come. For the second route's carry, so
/// read as that route reads a call: [`called_by_name_on`] with the words
/// written for the name standing alone (#1973).
pub fn only_the_name(heard: &str) -> bool {
    let is = |word: &str, list: &[&str]| list.iter().any(|known| word.eq_ignore_ascii_case(known));
    let words: Vec<&str> = heard
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    words.iter().any(|word| !is(word, GREETINGS))
        && words.iter().all(|word| {
            is(word, GREETINGS)
                || is(word, &["lamp", "lamb"])
                || is(word, STAND_INS)
                || is(word, STAND_ALONE_INS)
        })
        && called_by_name_on(heard, true)
}

/// Whether what was heard opens with the lamp's name set off as a call:
/// "Lamp" or "Lamb" after a greeting ("Hey Lamp …"), or followed by a
/// pause ("Lamp, …", "Lamp."). Never a stand-in ([`called`]'s "man",
/// "plan", "mom" are everyday words), and not "Lamp is broken, …" or
/// "Lamb for dinner tonight?" (#2191, the reviewer's P2 on #2210).
pub fn begins_with_the_name(heard: &str) -> bool {
    let is = |word: &str, list: &[&str]| list.iter().any(|known| word.eq_ignore_ascii_case(known));
    let mut rest = heard;
    let mut greeted = false;
    loop {
        let start = rest.find(char::is_alphanumeric);
        let Some(start) = start else { return false };
        rest = &rest[start..];
        let end = rest
            .find(|c: char| !c.is_alphanumeric())
            .unwrap_or(rest.len());
        let (word, after) = rest.split_at(end);
        if is(word, GREETINGS) {
            greeted = true;
            rest = after;
            continue;
        }
        let set_off = after
            .trim_start()
            .chars()
            .next()
            .is_none_or(|c| matches!(c, ',' | '.' | '?' | '!' | ';' | ':'));
        return (greeted || set_off) && is(word, &["lamp", "lamb"]);
    }
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

/// The calls that withdraw the person's consent, done even when the rule
/// holds their line's answer (the first builder on #1722, 16:44; #1683):
/// today only [`crate::reports_stop::NAME`]. Each one only reduces what
/// leaves the lamp and never adds to it. Done for a colleague's line, its
/// cost is all of a stop's (`cloud.md`, "The person's choice"): every
/// report waiting is deleted, one `stopped` is sent, and sharing stays off
/// until the person turns it back on in the app. Since the answer is held,
/// nobody is told at the time. If it were
/// not done, a person who asks to stop sharing and is held would have to
/// ask twice. Asked again at this cost (Third's read of #1880), the first
/// builder kept it and made it visible (#1722, 18:44): `lamp-cloud` keeps
/// the stop as the voice's, held, and when, for the app to show, and the
/// voice tells it only when asked about reports
/// ([`crate::reports_stop::STATE_NAME`]). A call that adds to what
/// leaves the lamp, or changes the lamp, is never put here: it stays held
/// with its answer. The answer itself stays held either way.
pub const DONE_THOUGH_HELD: &[&str] = &[crate::reports_stop::NAME];

/// Whether a call for this function is done though its line's answer is
/// held ([`DONE_THOUGH_HELD`]).
pub fn done_though_held(function: &str) -> bool {
    DONE_THOUGH_HELD.contains(&function)
}

/// The words a held line must have, as whole words in any case, to be
/// asked about [`DONE_THOUGH_HELD`] at all (the first builder on #1925,
/// 21:32). A held line is talk the lamp judged not for it, in a busy room
/// people talking to each other (37 such lines in 44 minutes on the first
/// arm of 19 October), and asking means sending its words to the chat and
/// paying a round. A stop names what is sent: "stop sending reports",
/// "stop sharing", "don't send my data". A line with none of these is not
/// asked and is sent nowhere, as before; a line with one is asked once and
/// the model decides, so "report to your manager" is asked and the model
/// calls nothing. Checked on the heard words before anything goes.
pub const HELD_ASKED_WORDS: &[&str] = &["report", "reports", "sharing", "sending", "data"];

/// Whether a held line's heard words have one of [`HELD_ASKED_WORDS`]
/// as a whole word, in any case.
pub fn held_line_names_reports(said: &str) -> bool {
    said.split(|c: char| !c.is_alphanumeric()).any(|word| {
        HELD_ASKED_WORDS
            .iter()
            .any(|w| word.eq_ignore_ascii_case(w))
    })
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

/// With `LAMP_NAME_SECOND_SIGN=on`, a hearing of the name that began no
/// later than this into the line's sound (from [`NAME_BEFORE_US`] before
/// it) is the name at the line's start, a call ("Hey Lamp, …", "Lamp, …"),
/// and is its own second sign ([`Attention::weigh_the_name`]). Both times
/// are the voice's: the line's sound opens with its first block, the
/// detector's pre-roll (`activity::PRE_ROLL_BLOCKS`, 0.3 s, dated when each
/// block was read), and a hearing is dated from when it began, the time
/// `lamp-name` said it less its `ago_ms`, never from when it was said
/// (`lamp-name` says one once its score falls, up to 0.8 s on). But
/// `lamp-name`'s "began" is when its score reached the bar, which is as the
/// name ends and a little after, not as the name is first said. On
/// lamp-4ace on 2026-10-21 at 08:06 (b0802) "Hey Lamp, good morning!",
/// played at 08:06:33.988, was heard from about 08:06:35.05, about 1.06 s
/// on, and was dropped with 0.7 s here (the first builder at 09:12 on
/// #1722). Taking the line's sound as opening when it was played (the
/// pre-roll against the speaker's and the detector's delays, not
/// measured), "Lamp" ends about 0.8 s in at 0.3 s a word, so the hearing
/// began about a quarter of a second after the name's end. A passing mention
/// comes a few words in ("Yes, the lamp looked at me…", "I left the lamp
/// on…"), its hearing from about 1.55 s in. On 03:38 the same greeting,
/// its words written "Mainland. Good morning.", was dropped without the
/// sign at all (the first builder at 03:54 on #1722).
// ponytail: one played greeting measured, between it and the earliest
// passing mention; set it again from the journal's "began … ms into the
// line" once more played tests are read.
pub const NAME_AT_THE_START_US: u64 = 1_300_000;

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

pub(crate) const TURNED: &str = "someone is turned to the lamp";
pub(crate) const GOING_ON: &str = "a conversation with the lamp is going on";
/// Two first reasons that, with `--spoken-to`, let nothing out alone: a line
/// let out so had another sign as well (the second route names it, #1712).
pub(crate) const TRUSTED: &str = "the session has heard enough to be trusted";
pub(crate) const ROOM_NOT_BUSY: &str = "the room is not known to be busy";
/// The strict busy rule's holds ([`decide_busy`]).
pub(crate) const BUSY_NAMED_UNTURNED: &str =
    "a busy room: its name was heard in the sound, and nobody is turned to the lamp";
pub(crate) const BUSY_UNCALLED: &str =
    "a busy room, and nothing shows it was called or that anyone is turned to the lamp";
/// With `LAMP_NAME_OR_FOLLOW_UP` ([`Attention::face_shows`]): the strict
/// busy rule's hold of a line whose only sign is a face turned to the lamp.
pub(crate) const BUSY_TURNED_ALONE: &str =
    "a busy room: someone is turned to the lamp, but nothing else shows it was called";
/// The same with `--spoken-to`, in any room.
pub(crate) const TURNED_NOT_SPOKEN_TO: &str = "nobody spoke to the lamp: someone is turned to it, but no name, no call, no conversation with it going on";
/// The text calling the lamp, as the rule says it.
pub(crate) const CALLED_IN_TEXT: &str = "the text shows it was called";
const SOUND_A_WORD: &str = "a short sound, heard as a short word said to the lamp";
const SOUND_NEAR: &str = "a short sound, said close to the lamp";
const SOUND_TURNED: &str = "a short sound, with someone turned to the lamp";
const SOUND_ANSWERS: &str = "a short sound with a word in it, right after the lamp's own question, in a conversation going on";
const SOUND_HELD: &str = "a short sound, and nothing shows it was said to the lamp: not a short word, not close, nobody turned to it";
/// The same for a short sound with someone turned to the lamp, with
/// `LAMP_NAME_OR_FOLLOW_UP` ([`Attention::face_shows`]).
const SOUND_TURNED_ALONE: &str = "a short sound, with someone turned to the lamp, but nothing else shows it was said to the lamp: not a short word, not close, not its name";
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
        true,
    )
}

/// [`decide_busy`] with whether the text shows a call already read, and
/// whether a face turned to the lamp lets a line out alone
/// (`turned_alone`): it does unless `LAMP_NAME_OR_FOLLOW_UP`
/// ([`Attention::face_shows`]), and then it only confirms the name heard.
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
    turned_alone: bool,
) -> Verdict {
    if own {
        Verdict::Play("the lamp began this turn itself")
    } else if !(fresh || strict && room == Room::Busy) {
        Verdict::Play(TRUSTED)
    } else if room != Room::Busy {
        Verdict::Play(ROOM_NOT_BUSY)
    } else if going_on {
        Verdict::Play(GOING_ON)
    } else if called_in_text {
        Verdict::Play(CALLED_IN_TEXT)
    } else if turned == Some(true) && (turned_alone || named) {
        Verdict::Play(TURNED)
    } else if named && !strict {
        Verdict::Play(NAMED)
    } else if strict && named {
        Verdict::Hold(BUSY_NAMED_UNTURNED)
    } else if strict && turned == Some(true) {
        Verdict::Hold(BUSY_TURNED_ALONE)
    } else if strict {
        Verdict::Hold(BUSY_UNCALLED)
    } else {
        Verdict::Hold("a new session in a busy room, and nothing shows it was called")
    }
}

/// What the worker keeps for the request in hand.
#[derive(Default)]
pub struct Attention {
    /// The words written for the lamp's name standing alone count as its
    /// call ([`STAND_ALONE_INS`]): the second route's rule only.
    stand_alone_ins: bool,
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
    /// When the request's usual start was: [`Self::began_at_led`]'s time
    /// with the soft start's lead after it, the start the name at the line's
    /// start is dated from with `LAMP_NAME_START_USUAL=on`
    /// ([`Self::date_the_start_from_the_usual_start`], #4226).
    usual_began_us: Option<u64>,
    /// When the lamp's name last began to be heard in the room's sound.
    named_us: Option<u64>,
    /// The name was heard for the request in hand: as it began, or in its
    /// sound.
    named: bool,
    /// When the name was last heard at [`SURE_SCORE`] or more, and whether
    /// such a hearing counts for the request in hand.
    named_sure_us: Option<u64>,
    named_sure: bool,
    /// Whether a hearing that counts for the request in hand began at the
    /// line's start ([`NAME_AT_THE_START_US`]).
    named_at_start: bool,
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
    /// The text's call narrowed ([`Self::narrow_the_call`]).
    call_narrowed: bool,
    /// A remark to the room is not a reply to the lamp in a busy room
    /// (`LAMP_REMARKS=on`, [`crate::remarks`], #829).
    remarks: bool,
    /// In a busy room, a line let out only as the conversation going on
    /// waits for the Policy's decision (`LAMP_GOING_ON_ASKS=on`,
    /// [`crate::going_on_asks`], #1522).
    going_on_asks: bool,
    /// What the Policy said of a request, given before it is judged: by the
    /// request's number, and `None` inside when nothing came in time.
    policy_says: Option<(u64, Option<crate::going_on_asks::Says>)>,
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
    /// With `spoken_to`: the request in hand was let out only as the
    /// conversation going on ([`Self::played_on_the_conversation_alone`]).
    conversation_alone: bool,
    /// The same, with or without `spoken_to`
    /// ([`Self::played_only_as_going_on`]).
    going_on_alone: bool,
    /// The request in hand was judged with a conversation going on, and
    /// nobody else's line before it.
    went_on: bool,
    /// The name heard only in the room's sound counts only with a second
    /// sign (`LAMP_NAME_SECOND_SIGN=on`, [`Self::weigh_the_name`]).
    name_second_sign: bool,
    /// With `name_second_sign`, a hearing at the line's start is no second
    /// sign of its own when the line's first word, set off, is one said to
    /// a person ([`SAID_TO_A_PERSON`], `LAMP_NAME_SAID_TO_A_PERSON=on`,
    /// [`Self::hold_a_start_said_to_a_person`]).
    name_said_to_a_person: bool,
    /// A face turned to the lamp lets no line out alone in a busy room
    /// (`LAMP_NAME_OR_FOLLOW_UP=on`, [`Self::face_shows`]), and the room the
    /// request in hand was judged in, for [`Self::sign`].
    name_or_follow_up: bool,
    judged_room: Option<Room>,
    /// A hearing of the name inside a line's sound counts for that line
    /// only (`LAMP_NAME_STAYS_IN_ITS_LINE=on`, [`Self::keep_the_name_in_its_line`]).
    name_stays: bool,
    /// The name at the line's start is dated from the line's usual start,
    /// not its soft start's lead (`LAMP_NAME_START_USUAL=on`,
    /// [`Self::date_the_start_from_the_usual_start`]).
    name_start_usual: bool,
    /// The second signs that let the name heard in the room's sound count
    /// for the line last weighed ([`Self::weigh_the_name`]), for the
    /// journal (#3945).
    name_signs: Vec<&'static str>,
}

/// The most text kept for one request.
const MOST_HEARD_BYTES: usize = 2_048;

impl Attention {
    /// Hold a remark to the room in a busy room ([`crate::remarks`]).
    pub fn hold_remarks(&mut self, on: bool) {
        self.remarks = on;
    }

    /// Whether remarks are held ([`Self::hold_remarks`]).
    pub fn holds_remarks(&self) -> bool {
        self.remarks
    }

    /// The name heard only in the room's sound counts only with a second
    /// sign ([`Self::weigh_the_name`]).
    pub fn ask_a_second_sign_of_the_name(&mut self, on: bool) {
        self.name_second_sign = on;
    }

    /// With [`Self::ask_a_second_sign_of_the_name`], a hearing of the name
    /// at the line's start is no second sign of its own when the line's
    /// first word, set off, is said to a person ([`SAID_TO_A_PERSON`]:
    /// "Ma'am, could you pass me the salt please?", heard at 0.95 to 0.99
    /// at its start, #3945); `LAMP_NAME_SAID_TO_A_PERSON=on`.
    pub fn hold_a_start_said_to_a_person(&mut self, on: bool) {
        self.name_said_to_a_person = on;
    }

    /// The word on [`SAID_TO_A_PERSON`] that takes the line's start from the
    /// name's second signs ([`Self::hold_a_start_said_to_a_person`]): only
    /// with both switches on and the name heard at the line's start.
    pub fn start_said_to_a_person(&self) -> Option<&'static str> {
        if !(self.name_second_sign && self.name_said_to_a_person && self.named_at_start) {
            return None;
        }
        said_to_a_person(&self.heard)
    }

    /// A hearing of the name that began inside the sound of the line before
    /// (from its start to its end) counts for that line only, never for the
    /// one begun after it within [`NAME_BEFORE_US`] ([`Self::began_at`]); a
    /// call's goes on: a bare call by its own rule ([`AFTER_CALL_US`]), and a
    /// line whose words call the lamp ([`Self::called_in_its_words`]). On lamp-4ace
    /// on 2026-10-26 at 06:21 a hearing at 0.92 in "Can you pass me the
    /// charger? The white one." counted for the line begun 0.7 s after it,
    /// which was answered.
    pub fn keep_the_name_in_its_line(&mut self, on: bool) {
        self.name_stays = on;
    }

    /// A hearing of the name is at the line's start ([`NAME_AT_THE_START_US`])
    /// when it began within that of the line's usual start, the detector's
    /// 300 ms pre-roll, not of the soft start's lead before it
    /// (`LAMP_NAME_START_USUAL=on`, #4226). The bound was measured before
    /// soft starts were kept, and on a lamp every hearing then came 0.29 s
    /// later into the line (939 to 1023 ms with soft starts, 654 to 723 ms
    /// without). Only the name at the line's start is dated so; every other
    /// window keeps the line's own start.
    pub fn date_the_start_from_the_usual_start(&mut self, on: bool) {
        self.name_start_usual = on;
    }

    /// Where the name at the start of a line begun at `began_us`, its sound
    /// beginning `lead_us` before its usual start, is dated from: its usual
    /// start with [`Self::date_the_start_from_the_usual_start`], otherwise
    /// `began_us` itself.
    pub fn start_of(&self, began_us: u64, lead_us: u64) -> u64 {
        if self.name_start_usual {
            began_us.saturating_add(lead_us)
        } else {
            began_us
        }
    }

    /// Where the name at the start of the request in hand is dated from
    /// ([`Self::start_of`]).
    fn start_in_hand(&self) -> Option<u64> {
        if self.name_start_usual {
            self.usual_began_us.or(self.began_us)
        } else {
            self.began_us
        }
    }

    /// Whether the lamp's head was tapped for the request in hand.
    pub fn tapped(&self) -> bool {
        self.tapped
    }

    /// Whether the words heard of the request in hand call the lamp by name
    /// ("Lamp.", "Hey Lamp, …"), not someone else.
    pub fn called_in_its_words(&self) -> bool {
        called_by_name_on(&self.heard, self.stand_alone_ins)
    }

    /// From now on, in a busy room, a face turned to the lamp lets no line
    /// out alone; it only confirms the lamp's name heard
    /// (`LAMP_NAME_OR_FOLLOW_UP=on`, [`crate::name_or_follow_up`]).
    pub fn need_name_or_follow_up(&mut self, on: bool) {
        self.name_or_follow_up = on;
    }

    /// Whether a face turned to the lamp (`turned`, the eyes' newest fresh
    /// look) shows on its own that a line said in `room` was said to it: it
    /// does, but not in a busy room with [`Self::need_name_or_follow_up`].
    /// At the owner's desk on lamp-4ace on 22 October the owner faced the
    /// lamp's way while talking to a colleague, and 8 of the lamp's 18
    /// answers were let out by such a face. A person alone with the lamp
    /// faces it and does not say its name (the owner at 18:0x: "That's so
    /// weird if I had to say lamp a thousand times throughout the day").
    pub fn face_shows(&self, room: Room, turned: Option<bool>) -> bool {
        turned == Some(true) && !(self.name_or_follow_up && room == Room::Busy)
    }

    /// With [`Self::ask_a_second_sign_of_the_name`]: the name heard for the
    /// request in hand only in the room's sound counts for nothing alone,
    /// at any score, [`SURE_SCORE`] too. It counts with a second sign: a
    /// face turned to the lamp (`turned`), said close to the lamp, said to
    /// it from close, its words calling it, or the hearing at the line's
    /// start ([`NAME_AT_THE_START_US`]). On lamp-4ace on 2026-10-20
    /// at 18:16, 18:17 and 18:20 a hearing of 0.9, where the office's false
    /// hearings reach 0.997, opened a conversation with nobody calling the
    /// lamp, and the lines after it were answered as that conversation
    /// going on (the first builder at 21:03 on #1722). Called once a line's
    /// signs are all in, before it is judged; whether the name was dropped.
    ///
    /// With [`Self::hold_a_start_said_to_a_person`], a conversation with
    /// the lamp going on (`going_on`) is a second sign too, and the line's
    /// start is none when its first word is said to a person
    /// ([`Self::start_said_to_a_person`]).
    pub fn weigh_the_name(&mut self, turned: Option<bool>, going_on: bool) -> bool {
        let text_calls = if self.name_in_passing_ignored {
            called_by_name(&self.heard)
        } else {
            called(&self.heard)
        };
        let said_to_a_person = self.named_at_start && self.start_said_to_a_person().is_some();
        let start_counts = self.named_at_start && !said_to_a_person;
        // A conversation going on gives back only the start this switch
        // held, never a second sign for other lines (#1722's rule; the
        // Reviewer-lead's P3 on #4082).
        let going_on = going_on && said_to_a_person;
        let signs: Vec<&'static str> = [
            (turned == Some(true), "a face turned to the lamp"),
            (self.near, "the name said close to it"),
            (self.addressed, "said to the lamp from close to it"),
            (text_calls, "its words calling it"),
            (going_on, "a conversation going on"),
            (start_counts, "its hearing at the line's start"),
        ]
        .into_iter()
        .filter_map(|(shown, sign)| shown.then_some(sign))
        .collect();
        let weighed = self.name_second_sign && self.named;
        let alone = weighed && signs.is_empty();
        self.name_signs = if weighed { signs } else { Vec::new() };
        if alone {
            self.named = false;
            self.named_sure = false;
            self.best_score = 0.0;
        }
        alone
    }

    /// The second signs that let the name count for the line last weighed
    /// ([`Self::weigh_the_name`]): none when it counted for nothing, or
    /// none was asked.
    pub fn second_signs(&self) -> &[&'static str] {
        &self.name_signs
    }

    /// In a busy room, let a line out only as the conversation going on
    /// once the Policy has said it is the lamp's ([`crate::going_on_asks`]).
    pub fn ask_policy_when_going_on(&mut self, on: bool) {
        self.going_on_asks = on;
    }

    /// What the Policy said of `request` (`None`: nothing came in time),
    /// for its verdict.
    pub fn policy_said(&mut self, request: u64, says: Option<crate::going_on_asks::Says>) {
        self.policy_says = Some((request, says));
    }

    /// Whether what the Policy said of `request` has been given, or that
    /// nothing came in time ([`Self::policy_said`]).
    pub fn policy_given(&self, request: u64) -> bool {
        self.policy_says.is_some_and(|(of, _)| of == request)
    }

    /// Whether the first output of `request`, still to be judged, should
    /// wait for the Policy's decision before it is: only a line whose
    /// verdict would come to the Policy's arm of
    /// [`Self::first_output_busy`] ([`crate::going_on_asks`]). So with the
    /// switch, in a busy room, a conversation going on, and none of what
    /// lets a line out or holds it first: nothing showing it was said to
    /// the lamp (its name, a call, a face, said close or from close, a tap,
    /// a bare call before it), not a reply to the lamp's own question, not
    /// someone else's (a reply to a person called by name, a question put
    /// to one, a reply to a question the lamp left alone), not a question
    /// about the lamp's own work, and not a remark to the room with
    /// `LAMP_REMARKS` (the reviewer and Jarvis on #1538: each waited, then
    /// was judged without the Policy). The verdict itself is
    /// [`Self::first_output_busy`]'s; the worker gives the room microphone's
    /// rise and the lamp's question before it asks this.
    pub fn waits_for_policy(
        &self,
        request: u64,
        room: Room,
        going_on: bool,
        turned: Option<bool>,
    ) -> bool {
        self.going_on_asks
            && self.awaits_verdict(request)
            && !self.policy_given(request)
            && room == Room::Busy
            && going_on
            && own_request().load(Ordering::Acquire) != request
            && !self.sound_in_hand()
            && !self.called_for_itself()
            && !self.after_call
            && !self.named_sure
            && !self.face_shows(room, turned)
            && !self.near
            && !self.addressed
            && !self.lamp_asked
            && self.another(room, false).is_none()
            && !self.asks_about_its_work()
            && !(self.remarks && crate::remarks::remark(&self.heard))
    }

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

    /// The lamp's own turn (its news, a reminder) began to be heard at
    /// `at_us`, on a route that judges only spoken turns and so never sees
    /// the own turn begin (the second route, #1712): news someone can reply
    /// to, as [`Self::spoke`] marks it.
    pub fn news_told(&mut self, at_us: u64) {
        if self.spoken_to {
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
        self.began_at_led(request, session, now_us, 0);
    }

    /// [`Self::began_at`], its sound beginning `lead_us` before its usual
    /// start by a soft start's lead (#4226): with
    /// [`Self::date_the_start_from_the_usual_start`], the name at its start
    /// is dated from `now_us + lead_us`. Nothing else is.
    pub fn began_at_led(&mut self, request: u64, session: u64, now_us: u64, lead_us: u64) {
        // The line before, if it called someone else by name and ended just
        // now (#829).
        let after_another = self.spoken_to
            && own_request().load(Ordering::Acquire) != request
            && self
                .ended_us
                .or(self.began_us)
                .is_some_and(|end| end.saturating_add(ANOTHER_REPLY_US) >= now_us)
            && addresses_someone_else_on(&self.heard, self.stand_alone_ins);
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
        // The line before's sound, when a hearing in it stays with it: not
        // a call, by its words or as a bare call. A hearing at its start is
        // a call however its words were written ("Lamp." heard as "Ramp.",
        // the reviewer-lead's P2 on `da9a597de`), and goes on too.
        let line_before = (self.name_stays
            && self.request != request
            && !self.called_in_its_words()
            && self
                .bare_call_us
                .is_none_or(|(call, _, _)| call != self.request))
        .then_some(self.start_in_hand().zip(self.ended_us))
        .flatten();
        let its_own = move |named: u64| {
            line_before.is_none_or(|(began, ended)| {
                named >= now_us
                    || named <= began.saturating_add(NAME_AT_THE_START_US)
                    || named > ended
            })
        };
        // One that stays with the line before is used up there, so no later
        // line takes it either (reviewer-two's P2 on `da9a597de`).
        if !self.named_us.is_some_and(its_own) {
            self.named_us = None;
        }
        if !self.named_sure_us.is_some_and(its_own) {
            self.named_sure_us = None;
        }
        let counts = move |named: u64| named.saturating_add(NAME_BEFORE_US) >= now_us;
        self.began(request, session);
        self.after_another = after_another;
        self.after_question = after_question;
        self.after_unanswered = after_unanswered;
        self.began_us = Some(now_us);
        self.usual_began_us = Some(now_us.saturating_add(lead_us));
        let start = self.start_of(now_us, lead_us);
        self.named = self.named_us.is_some_and(counts);
        self.named_sure = self.named_sure_us.is_some_and(counts);
        self.named_at_start = self.named
            && self
                .named_us
                .is_some_and(|named| named <= start.saturating_add(NAME_AT_THE_START_US));
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
        !addresses_someone_else_on(&self.heard, self.stand_alone_ins)
            && (self.named_sure
                || self.near && self.best_score >= NEAR_SCORE && !self.day_bars()
                || called_by_name_on(&self.heard, self.stand_alone_ins))
    }

    /// A request began on `session` at `now_us` that continues the one in
    /// hand, which the lamp ended early: the same person going on with the
    /// same utterance. What the rule knew of the first part stays: its name,
    /// heard surely or not and how surely, the text heard so far, whether it
    /// was close, and whether it was said to the lamp from close to it. Its window begins where the first part's did.
    /// Anything else is as for [`Self::began_at`].
    pub fn continued_at(&mut self, request: u64, session: u64, now_us: u64) {
        self.continued_at_led(request, session, now_us, 0);
    }

    /// [`Self::continued_at`] with the continuation's soft lead
    /// ([`Self::began_at_led`]): the first part's usual start is kept, as
    /// its start is; the lead counts only when nothing goes on.
    pub fn continued_at_led(&mut self, request: u64, session: u64, now_us: u64, lead_us: u64) {
        let tapped = session == self.session && self.began_us.is_some() && self.tapped;
        let after_call = session == self.session && self.began_us.is_some() && self.after_call;
        let after_another = self.after_another;
        let after_question = self.after_question;
        let after_unanswered = self.after_unanswered;
        let before = (session == self.session && self.began_us.is_some()).then(|| {
            (
                (self.named, self.named_at_start),
                self.named_sure,
                self.best_score,
                std::mem::take(&mut self.heard),
                self.near,
                self.addressed,
                (self.began_us, self.usual_began_us),
            )
        });
        // The first part's hearings are this line's own: kept past
        // `began_at`, which lets go of one that stays with the line before.
        let hearings = (self.named_us, self.named_sure_us);
        let continuing = before.is_some();
        self.began_at_led(request, session, now_us, lead_us);
        if continuing {
            (self.named_us, self.named_sure_us) = hearings;
        }
        if let Some((
            (named, named_at_start),
            named_sure,
            best_score,
            heard,
            near,
            addressed,
            (began_us, usual_began_us),
        )) = before
        {
            self.named |= named;
            self.named_sure |= named_sure;
            self.best_score = self.best_score.max(best_score);
            self.heard = heard;
            self.near |= near;
            self.addressed |= addressed;
            self.began_us = began_us;
            self.usual_began_us = usual_began_us;
            // At the start is judged against the first part's start, not
            // the continuation's: a hearing mid-way through the first part
            // is never at the start because the line was cut there.
            let start = self.start_in_hand();
            self.named_at_start = named_at_start
                || (self.named
                    && self.named_us.is_some_and(|n| {
                        start.is_some_and(|b| n <= b.saturating_add(NAME_AT_THE_START_US))
                    }));
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
            let start = self.start_in_hand().unwrap_or(began);
            self.named_at_start |= at_us <= start.saturating_add(NAME_AT_THE_START_US);
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
        self.usual_began_us = None;
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

    /// Count the words written for the lamp's name standing alone as its
    /// call ([`STAND_ALONE_INS`]), as the second route's rule does (#1973,
    /// the first builder at 22:38). Off, as the Live worker's rule is.
    pub fn count_stand_alone_ins(&mut self, on: bool) {
        self.stand_alone_ins = on;
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
        self.judged_room = Some(room);
        self.conversation_alone = false;
        self.going_on_alone = false;
        self.went_on = false;
        let fresh = self.begun <= FRESH_REQUESTS;
        let another = self.another(room, own);
        let answering_another = another.is_some();
        let going_on = going_on && !answering_another;
        self.went_on = going_on;
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
        let called_in_text = self.called_in_text();
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
            } else if self.face_shows(room, turned) || turned == Some(true) && self.named {
                // With `LAMP_NAME_OR_FOLLOW_UP`, the face counts only with
                // the name heard in the sound.
                Verdict::Play(SOUND_TURNED)
            } else if self.lamp_asked && going_on && self.heard.chars().any(char::is_alphanumeric) {
                // On lamp-4ace on 2026-10-17 at 16:11, "Yes. Please start
                // it." to the lamp's "Shall I start it?" was ended after
                // "Yes." and heard as "bien": held, its handing over was
                // never run. A line with a word in it, right after the
                // lamp's own question in a conversation going on, is the
                // person's answer, and the model judges it.
                Verdict::Play(SOUND_ANSWERS)
            } else if turned == Some(true) {
                Verdict::Hold(SOUND_TURNED_ALONE)
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
        let near_shows = !self.day_bars()
            || turned == Some(true)
            || called_by_name_on(&self.heard, self.stand_alone_ins);
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
            self.face_shows(room, Some(true)),
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
                || self.face_shows(room, turned)
                || self.addressed
        };
        self.going_on_alone = !answering_another
            && going_on
            && !(own
                || called_in_text
                || self.named
                || self.named_sure
                || self.face_shows(room, turned)
                || self.addressed
                || self.lamp_asked);
        self.conversation_alone = self.spoken_to && self.going_on_alone;
        let verdict = match verdict {
            Verdict::Play(_) if self.spoken_to && !spoken_to => {
                Verdict::Hold(if let Some(why) = another {
                    why
                } else if turned == Some(true) {
                    // Only with `LAMP_NAME_OR_FOLLOW_UP`: else the face
                    // showed it was spoken to.
                    TURNED_NOT_SPOKEN_TO
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
            // #829, scene 3, with `LAMP_REMARKS=on`: a line let out only as
            // the conversation going on, in a busy room, that asks nothing
            // of the lamp and answers no question of its own, is a remark
            // to the room ("I should bring an umbrella then.").
            Verdict::Play(GOING_ON)
                if self.remarks
                    && room == Room::Busy
                    && !called_in_text
                    && !self.named
                    && !self.named_sure
                    && !self.face_shows(room, turned)
                    && !self.near
                    && !self.addressed
                    && !self.lamp_asked
                    && crate::remarks::remark(&self.heard) =>
            {
                Verdict::Hold(crate::remarks::REMARK)
            }
            // #1522, with `LAMP_GOING_ON_ASKS=on`: in a busy room, a line
            // let out only as the conversation going on is the lamp's only
            // if the Policy says so. A reply to the lamp's own question is
            // the person's answer, as the remark arm has it: "Yes, go
            // ahead." to "Shall I start it?" is never held for want of a
            // `follows_lamp` of 0.6 (Eighth on #1538).
            Verdict::Play(GOING_ON)
                if self.going_on_asks
                    && room == Room::Busy
                    && !called_in_text
                    && !self.named
                    && !self.named_sure
                    && !self.face_shows(room, turned)
                    && !self.near
                    && !self.addressed
                    && !self.lamp_asked =>
            {
                crate::going_on_asks::verdict(
                    self.policy_says
                        .filter(|(of, _)| *of == request)
                        .and_then(|(_, says)| says),
                )
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

    /// Whether, with the switch, the answer to the request in hand was let
    /// out only as the conversation going on: no name, nobody turned to the
    /// lamp, not said close to it, no answer to its own question. A route
    /// whose model reads only words is told so (`second_provider`, #1712:
    /// on lamp-4ace on 19 Oct, colleagues' talk after a named request was
    /// answered six times, each answer keeping the conversation going,
    /// where Gemini Live leaves such talk alone).
    /// The sign that let the request in hand out with `--spoken-to`, as
    /// the journal says it, where the rule's first reason lets nothing out
    /// alone (the session trusted, the room not known to be busy): its name
    /// heard, a tap, its words calling the lamp, someone turned to it, said
    /// close to it, or a conversation going on (#1712, the first builder at
    /// 17:49: such lines were journaled as let out by trust).
    pub fn sign(&self, turned: Option<bool>) -> Option<&'static str> {
        let called_in_text = self.called_in_text();
        if self.named {
            Some(NAMED)
        } else if self.tapped {
            Some(TAPPED)
        } else if called_in_text {
            Some(CALLED_IN_TEXT)
        } else if self.face_shows(self.judged_room.unwrap_or(Room::Unknown), turned) {
            Some(TURNED)
        } else if self.addressed {
            Some(ADDRESSED)
        } else if self.went_on {
            Some(GOING_ON)
        } else {
            None
        }
    }

    pub fn played_on_the_conversation_alone(&self) -> bool {
        self.conversation_alone && matches!(self.verdict, Some(Verdict::Play(_)))
    }

    /// Whether the request in hand was let out only as the conversation
    /// going on, whether or not `spoken_to` is on: lamp-52e6 and lamp-4ace
    /// run without it, and there [`Self::played_on_the_conversation_alone`]
    /// is never true. The going-on holds read this one
    /// (`LAMP_GOING_ON_SAME_ONE`, `LAMP_QUIET_GOING_ON_SAME_ONE`; 20 Oct at
    /// 22:4x).
    pub fn played_only_as_going_on(&self) -> bool {
        self.going_on_alone && matches!(self.verdict, Some(Verdict::Play(_)))
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

    /// Whether the request in hand called the lamp for itself: its name
    /// heard as it began or in its sound, its head tapped, or its words
    /// calling it, as the rule reads them. Let out, such a line begins a
    /// conversation of its own, whatever conversation was going on: the
    /// same-voice check takes its voice as that one's, and judges it
    /// against no earlier voice (Eighth on #829: a named call let out while
    /// another person's conversation was going on was judged against that
    /// person's voice).
    pub fn called_for_itself(&self) -> bool {
        self.named || self.tapped || self.called_in_text()
    }

    /// Whether the text shows the lamp was called, as the rule reads it
    /// ([`called_by_name_on`] with `LAMP_NAME_IN_PASSING=ignored`, else
    /// [`called`]), narrowed when [`Self::narrow_the_call`] said so.
    fn called_in_text(&self) -> bool {
        let read = if self.name_in_passing_ignored {
            called_by_name_on(&self.heard, self.stand_alone_ins)
        } else {
            called(&self.heard)
        };
        read && !(self.call_narrowed && !self.named && !begins_with_the_name(&self.heard))
    }

    /// With `LAMP_BUSY_CALLED_STRICT=on`, in a busy room with nobody turned
    /// to the lamp (the second route says so before each line): the text
    /// shows a call only as "Lamp" or "Lamb" set off as one
    /// ([`begins_with_the_name`]), or with the name heard in the sound. A
    /// stand-in ("Man, that meeting ran long.", "Mom, can you pick me up?")
    /// or "Lamp is broken, …" alone is office talk (#1712, the first
    /// builder on #1722 at 16:11).
    /// The request in hand was carried as the same speaker's conversation
    /// (`LAMP_SAME_SPEAKER_CARRIES`): a colleague's question held just
    /// before it was no question to them (the reviewer-lead on #3331).
    /// [`AFTER_QUESTION`] does not hold it.
    pub fn carried_past_a_question(&mut self) {
        self.after_question = false;
    }

    pub fn narrow_the_call(&mut self, narrowed: bool) {
        self.call_narrowed = narrowed;
    }

    /// Whether the request in hand is someone else's, and why: with the
    /// switch, a reply to someone the room had just called by name is that
    /// person's conversation, not the lamp's, and only the lamp's name
    /// counts for it (#829). A line that itself calls someone else by name
    /// is theirs too, and in a busy room so is a reply to a question the
    /// lamp left unanswered (#829, 15:40). Never the lamp's own turn.
    fn another(&self, room: Room, own: bool) -> Option<&'static str> {
        if !self.spoken_to || own {
            None
        } else if self.after_another {
            Some(ANSWERING_ANOTHER)
        } else if addresses_someone_else_on(&self.heard, self.stand_alone_ins)
            && self.heard.contains('?')
        {
            // Only a question put to that person: a request that opens
            // with a word and a comma is the lamp's (Second on #1090).
            Some(TO_ANOTHER)
        } else if self.after_question && room == Room::Busy {
            Some(AFTER_QUESTION)
        } else {
            None
        }
    }

    /// Whether the request in hand asks about the lamp's own work, done
    /// lately ([`ABOUT_ITS_WORK`]), as [`Self::first_output_busy`] lets it
    /// out.
    fn asks_about_its_work(&self) -> bool {
        self.spoken_to
            && self.began_us.is_some_and(|began| {
                self.worked_us
                    .filter(|at| *at <= began)
                    .is_some_and(|at| began - at <= WORK_ASKED_US)
            })
            && about_its_work(&self.heard)
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

    /// Whether what was heard of the request in hand names a timer, a
    /// reminder or the lamp ([`crate::going_on_asks::names_the_schedule`]).
    pub fn names_the_schedule(&self) -> bool {
        crate::going_on_asks::names_the_schedule(&self.heard)
    }

    /// Whether the request in hand answers the lamp's own question, as its
    /// first output was judged ([`Self::lamp_asked`]).
    pub fn answers_the_lamps_question(&self) -> bool {
        self.lamp_asked
    }

    /// Whether what was heard of the request in hand is a question
    /// ([`crate::remarks::questions`]).
    pub fn asks_a_question(&self) -> bool {
        crate::remarks::questions(&self.heard)
    }
}

#[cfg(test)]
mod tests {
    /// #753: this home's bar is read from setup's file once it fits, looked
    /// at again only every [`NEAR_READ_EVERY`], and the default is kept
    /// without one, or with one its own tries don't make.
    #[test]
    fn this_homes_near_bar_is_read_from_setups_file_and_the_default_kept_without_one() {
        use super::{NEAR_READ_EVERY, NEAR_RISE_DB, NearBar};
        use std::time::{Duration, Instant};
        let folder = std::env::temp_dir().join(format!("lamp-near-bar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("near-call.json");
        let mut bar = NearBar::new(path.clone());
        let at = Instant::now();
        assert_eq!(bar.now(at), NEAR_RISE_DB);
        let tries = [22.4, 25.0, 23.1].map(|rise_db| lamp_near_call::Try {
            rise_db,
            score: Some(0.98),
            heard_by: lamp_near_call::HeardBy::Name,
        });
        let measured = lamp_near_call::Calibrated::of(tries.to_vec(), 1_760_000_000).unwrap();
        lamp_near_call::write(&path, &measured).unwrap();
        // Not looked at again within the interval.
        assert_eq!(bar.now(at + Duration::from_secs(1)), NEAR_RISE_DB);
        assert_eq!(bar.now(at + NEAR_READ_EVERY), 19.4);
        assert_eq!(bar.now(at + NEAR_READ_EVERY * 2), 19.4);
        // A bar its tries don't make, then no file: the default again.
        let edited = lamp_near_call::Calibrated {
            near_rise_db: 30.0,
            ..measured
        };
        std::thread::sleep(Duration::from_millis(20));
        lamp_near_call::write(&path, &edited).unwrap();
        assert_eq!(bar.now(at + NEAR_READ_EVERY * 3), NEAR_RISE_DB);
        std::fs::remove_dir_all(&folder).unwrap();
        assert_eq!(bar.now(at + NEAR_READ_EVERY * 4), NEAR_RISE_DB);
    }

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

    /// #1522, with `LAMP_GOING_ON_ASKS=on` (the first builder's decision on
    /// #1522 at 15:14): in a busy room, a line let out only as the
    /// conversation going on waits for the Policy and is played on `answer`
    /// or on a `stay_quiet` said back to the lamp, held otherwise, and played
    /// with no decision in time. A call in its words, a face turned to the
    /// lamp, a room not known to be busy, or the switch off: as before, and
    /// nothing waits.
    #[test]
    fn with_the_switch_a_line_going_on_in_a_busy_room_is_the_lamps_only_as_the_policy_says() {
        use crate::going_on_asks::{self, Says};
        let line = |asks: bool,
                    room: Room,
                    heard: &str,
                    turned: Option<bool>,
                    says: Option<(u64, Option<Says>)>| {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(true);
            attention.ask_policy_when_going_on(asks);
            attention.began_at(7, 4, 1_000_000);
            attention.heard(heard);
            let waits = attention.waits_for_policy(7, room, true, turned);
            if let Some((request, says)) = says {
                attention.policy_said(request, says);
            }
            let verdict = attention
                .first_output_busy(7, room, true, true, turned)
                .unwrap();
            (waits, verdict)
        };
        const CAR: &str = "Okay, I will book the car for eight.";
        let busy = Room::Busy;
        assert_eq!(
            line(true, busy, CAR, None, Some((7, Some(Says::Not)))),
            (true, Verdict::Hold(going_on_asks::NOT_THE_LAMPS))
        );
        assert_eq!(
            line(true, busy, CAR, None, Some((7, Some(Says::Following)))),
            (true, Verdict::Play(going_on_asks::FOLLOWING))
        );
        assert_eq!(
            line(true, busy, CAR, None, Some((7, Some(Says::Answer)))),
            (true, Verdict::Play(going_on_asks::ANSWER))
        );
        assert_eq!(
            line(true, busy, CAR, None, Some((7, None))),
            (true, Verdict::Play(going_on_asks::NO_DECISION))
        );
        // What the Policy said of another request is nothing of this one's.
        assert_eq!(
            line(true, busy, CAR, None, Some((6, Some(Says::Not)))).1,
            Verdict::Play(going_on_asks::NO_DECISION)
        );
        // A reply to the lamp's own question neither waits nor is held
        // (Eighth on #1538).
        let mut attention = Attention::default();
        attention.speak_when_spoken_to(true);
        attention.ask_policy_when_going_on(true);
        attention.began_at(7, 4, 1_000_000);
        attention.heard("Yes, go ahead.");
        attention.lamp_asked(true);
        assert!(!attention.waits_for_policy(7, busy, true, None));
        attention.policy_said(7, Some(Says::Not));
        assert_eq!(
            attention.first_output_busy(7, busy, true, true, None),
            Some(GOING_ON)
        );
        // Nor does a line whose verdict comes before the Policy's arm (the
        // reviewer and Jarvis on #1538): a remark to the room with
        // `LAMP_REMARKS`, a question about the lamp's own work, a question
        // put to someone else by name, a line said close to the lamp. Each
        // is judged as without the switch.
        let waits = |remarks: bool, heard: &str, prepare: &dyn Fn(&mut Attention)| {
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(true);
            attention.ask_policy_when_going_on(true);
            attention.hold_remarks(remarks);
            attention.began_at(7, 4, 100_000_000);
            attention.heard(heard);
            // As the worker does, once the request has begun.
            prepare(&mut attention);
            attention.waits_for_policy(7, busy, true, None)
        };
        let nothing = |_: &mut Attention| {};
        assert!(waits(false, "I should bring an umbrella then.", &nothing));
        assert!(!waits(true, "I should bring an umbrella then.", &nothing));
        assert!(waits(false, "How is that task going?", &nothing));
        assert!(!waits(false, "How is that task going?", &|attention| {
            attention.worked(90_000_000)
        }));
        assert!(!waits(
            false,
            "Daniel, are you coming to the meeting at two?",
            &nothing
        ));
        assert!(!waits(false, "And what about tomorrow?", &|attention| {
            attention.set_near(true)
        }));
        // As before, and nothing waits: the switch off, a call in its
        // words, a face turned to the lamp, a room not known to be busy.
        let not_says = Some((7, Some(Says::Not)));
        assert_eq!(line(false, busy, CAR, None, not_says), (false, GOING_ON));
        for (room, heard, turned) in [
            (busy, "Lamp, book the car for eight.", None),
            (busy, CAR, Some(true)),
            (Room::Quiet, CAR, None),
            (Room::Unknown, CAR, None),
        ] {
            assert_eq!(
                line(true, room, heard, turned, not_says),
                (false, line(false, room, heard, turned, None).1),
                "{room:?} {heard} {turned:?}"
            );
        }
    }

    /// One line said in a busy room while a conversation with the lamp is
    /// going on, nobody's face turned to it (`strict`, as lamp-4ace runs),
    /// after the lamp's line that `asked` or not, with `LAMP_REMARKS` on or
    /// off: its verdict.
    fn said_in_a_busy_conversation(
        remarks: bool,
        asked: bool,
        line: &str,
        turned: Option<bool>,
    ) -> Verdict {
        let mut attention = Attention::default();
        attention.speak_when_spoken_to(true);
        attention.hold_remarks(remarks);
        attention.began_at(7, 4, 1_000_000);
        attention.lamp_asked(asked);
        attention.heard(line);
        attention
            .first_output_busy(7, Room::Busy, true, true, turned)
            .unwrap()
    }

    /// #829, scene 3, and the night's scripted lines on lamp-4ace, by kind
    /// (the first builder on #705, 18 October, 13:0x): with `LAMP_REMARKS`
    /// the umbrella and the car (11) are held as remarks to the room, and
    /// the 13 of ours said to the lamp still play (the 8 not ours are not
    /// judged); without it, all play as today. Scene 3's lines are given as Gemini may write them
    /// too, without their punctuation (Fourth, 13:01).
    #[test]
    fn with_remarks_a_remark_to_the_room_is_held_and_what_is_said_to_the_lamp_plays() {
        let remark = Verdict::Hold(crate::remarks::REMARK);
        for (line, asked, with) in [
            ("I should bring an umbrella then.", false, remark),
            ("i should bring an umbrella then", false, remark),
            ("Okay, I will book the car for eight.", false, remark),
            ("Yes, and we should leave a bit early.", false, remark),
            ("And what about tomorrow?", false, GOING_ON),
            ("and what about tomorrow", false, GOING_ON),
            (
                "Remember that my favourite colour is green.",
                false,
                GOING_ON,
            ),
            // The answers to the lamp's "Shall I start it?".
            ("No, not now.", true, GOING_ON),
            ("No thanks, not now.", true, GOING_ON),
            ("write me a poem about the moon", false, GOING_ON),
            ("Thanks, that's perfect.", false, GOING_ON),
            // Another language: none of the cues, never held here.
            ("Mai mình đi sớm nhé", false, GOING_ON),
        ] {
            assert_eq!(
                said_in_a_busy_conversation(true, asked, line, Some(false)),
                with,
                "{line}"
            );
            assert_eq!(
                said_in_a_busy_conversation(false, asked, line, Some(false)),
                GOING_ON,
                "{line}, off"
            );
        }
        // A bare answer plays whatever the lamp asked, even with its
        // question missed (the reviewer on #1499).
        for answer in ["No, not now.", "Yes go ahead", "Sure", "Not now"] {
            assert_eq!(
                said_in_a_busy_conversation(true, false, answer, Some(false)),
                GOING_ON,
                "{answer}"
            );
        }
        // And a remark after the lamp's question is the answer to it.
        assert_eq!(
            said_in_a_busy_conversation(
                true,
                true,
                "I should bring an umbrella then.",
                Some(false)
            ),
            GOING_ON
        );
        // The replies to the lamp's own question without its `?` that look
        // like remarks (the reviewer's second probe on #1499): its question
        // known by its words, through the thread, they play.
        for (lamps_line, reply) in [
            ("Which one would you like", "I'll take the second one"),
            (
                "What time should I set it for",
                "I think around seven thirty",
            ),
            (
                "What would you like to hear",
                "I think some jazz would be nice",
            ),
        ] {
            let mut thread = crate::thread::Thread::new();
            let now = std::time::Instant::now();
            thread.lamp(6, lamps_line, now);
            let asked = thread.lamp_asked_with(7, true);
            assert!(asked, "{lamps_line}");
            assert!(!thread.lamp_asked_with(7, false), "{lamps_line}, off");
            assert_eq!(
                said_in_a_busy_conversation(true, asked, reply, Some(false)),
                GOING_ON,
                "{reply}"
            );
        }
        // A face turned to the lamp, or none seen, decides for it as before.
        for turned in [Some(true), None] {
            let verdict = said_in_a_busy_conversation(
                true,
                false,
                "I should bring an umbrella then.",
                turned,
            );
            assert_eq!(verdict == remark, turned.is_none(), "{turned:?}");
        }
    }

    /// #1522, lamp-4ace on 18 October at 14:44, as the worker judges each
    /// line (`provider_worker`: the room and the conversation from
    /// `RoomTold` and the thread, the verdict told back with
    /// `RoomTold::judged`), in a busy room with `LAMP_SPOKEN_TO`: the
    /// battery's scene 3 after the call, then scene 4's opening, two people
    /// talking with no name, 6 s on. Without `LAMP_REMARKS` every line is
    /// let out as the conversation going on, re-armed by the lamp's own
    /// replies, as on the lamp. With it, the follow-up is answered, the two
    /// remarks are held and end the conversation, and the rest is held.
    fn scene_three_and_four(remarks: bool) -> Vec<bool> {
        scene_three_and_four_with(remarks, false)
    }

    /// The same, with `LAMP_GOING_ON_ASKS` (`asks`, #1538) as well, the
    /// Policy's decision on each line given before it is judged as lamp-4ace's
    /// were at 14:44: "And what about tomorrow?" a `stay_quiet` said back to
    /// the lamp (`follows_lamp` 0.66), every other line of the two people's
    /// `stay_quiet` or `look_up` (the first builder on #1522, 15:14).
    fn scene_three_and_four_with(remarks: bool, asks: bool) -> Vec<bool> {
        scene_three_and_four_after(remarks, asks, 6)
            .into_iter()
            .map(|(play, _)| play)
            .collect()
    }

    /// The same, the room made busy first by three lines of talk the lamp
    /// left alone, the last of them `pause_s` seconds before the call, and
    /// each line judged in the room `RoomTold` gives then (Eighth on #1594:
    /// the scene had been given a busy room whatever `RoomTold` said).
    /// Whether each line was played, and in what room.
    fn scene_three_and_four_after(remarks: bool, asks: bool, pause_s: u64) -> Vec<(bool, Room)> {
        use crate::going_on_asks::Says;
        let lines = [
            (
                0,
                "Hey Lamp, what's the weather in Hanoi today?",
                Says::Answer,
            ),
            (6, "And what about tomorrow?", Says::Following),
            (12, "I should bring an umbrella then.", Says::Not),
            (15, "Yes, and we should leave a bit early.", Says::Not),
            (18, "Okay, I will book the car for eight.", Says::Not),
            // Scene 4's opening, 6 s after scene 3 ended.
            (27, "Did you see the report from the client?", Says::Not),
            (30, "Yes, I read it this morning.", Says::Not),
            (33, "I know.", Says::Not),
            (36, "We need to cut something before Friday.", Says::Not),
        ];
        lines_after_talk(remarks, asks, pause_s, &lines)
    }

    /// `lines` (when each is said after the call, in seconds; its words; the
    /// Policy's decision on it, given with `asks`) as the worker judges
    /// them, after three lines of talk the lamp left alone, the last of them
    /// `pause_s` seconds before the first line: whether each was played, and
    /// in what room.
    fn lines_after_talk(
        remarks: bool,
        asks: bool,
        pause_s: u64,
        lines: &[(u64, &str, crate::going_on_asks::Says)],
    ) -> Vec<(bool, Room)> {
        const S: u64 = 1_000_000;
        let start = std::time::Instant::now();
        let at = |us: u64| start + std::time::Duration::from_micros(us);
        let mut thread = crate::thread::Thread::new();
        let mut room = crate::room_told::RoomTold::new();
        room.count_only_talk(true);
        room.end_busy_once_answered(asks);
        for (request, before_s) in [(1, 11), (2, 6), (3, 1)] {
            let began = (100 - pause_s - before_s) * S;
            room.heard(request, began);
            room.ended(request, began + S / 2);
            room.left_unanswered(request, began + S);
        }
        let mut attention = Attention::default();
        attention.speak_when_spoken_to(true);
        attention.hold_remarks(remarks);
        attention.ask_policy_when_going_on(asks);
        let mut played = Vec::new();
        for (request, &(at_s, line, says)) in (17..).zip(lines) {
            let began = 100 * S + at_s * S;
            room.heard(request, began);
            attention.began_at(request, 4, began);
            attention.heard(line);
            thread.person(line, true, at(began));
            room.ended(request, began + S);
            attention.ended(request, began + S);
            let going_on = !room.talked_past_the_lamp() && thread.going_on(at(began + S));
            attention.lamp_asked(thread.lamp_asked_with(request, remarks));
            if asks {
                attention.policy_said(request, Some(says));
            }
            let now = room.at(began + S);
            let verdict = attention
                .first_output_busy(request, now, going_on, true, Some(false))
                .unwrap();
            room.judged(request, Some(verdict), false);
            let play = matches!(verdict, Verdict::Play(_));
            if play {
                thread.lamp(request, "Here is what I found.", at(began + 2 * S));
                room.spoke(request, began + 2 * S);
            } else {
                room.left_unanswered(request, began + 2 * S);
            }
            played.push((play, now));
        }
        played
    }

    /// The reviewer's probe on #1594 (20:53): one of two people calls the
    /// lamp and asks a follow-up, and then they talk on, a line every 6 s
    /// for nearly two minutes, each held by the Policy, with one more
    /// question to the lamp at 30 s. Their talk keeps the room busy: every
    /// line of it is held, and both follow-ups are answered. When the lines
    /// the Policy held counted for nothing, the room ended a minute after
    /// the call, and every line from 66 s on was answered. The same with
    /// `LAMP_REMARKS`: none of the lines is a remark.
    #[test]
    fn with_the_switch_two_people_talking_on_after_a_call_keep_their_room_busy() {
        use crate::going_on_asks::Says;
        const TALK: [&str; 6] = [
            "The numbers look high this quarter.",
            "Maybe the trip to Hanoi.",
            "That sounds about right to me.",
            "The client wants to meet in person.",
            "Probably the train, not the car.",
            "Friday works for the client.",
        ];
        let mut lines = vec![
            (
                0,
                "Hey Lamp, what's the weather in Hanoi today?",
                Says::Answer,
            ),
            (6, "And what about tomorrow?", Says::Following),
        ];
        for (at_s, talk) in (12..=120).step_by(6).zip(TALK.iter().cycle()) {
            lines.push(if at_s == 30 {
                (at_s, "And what about the weekend?", Says::Following)
            } else {
                (at_s, *talk, Says::Not)
            });
        }
        for remarks in [false, true] {
            let judged = lines_after_talk(remarks, true, 6, &lines);
            let played: Vec<u64> = lines
                .iter()
                .zip(&judged)
                .filter(|(_, (play, _))| *play)
                .map(|((at_s, ..), _)| *at_s)
                .collect();
            assert_eq!(played, [0, 6, 30], "remarks {remarks}");
            assert!(
                judged.iter().all(|(_, room)| *room == Room::Busy),
                "remarks {remarks}: {judged:?}"
            );
        }
    }

    /// Eighth's probe on #1594: two people talk, pause, and one of them calls
    /// the lamp, as an office minute goes. After a pause of 20 s or of 70 s
    /// the call and "And what about tomorrow?" are answered and the other
    /// seven held, every line of scene 3 judged in a busy room. A minute from
    /// the talk alone would have ended busy at the call made 70 s after it,
    /// and answered the umbrella, "leave a bit early" and the car.
    #[test]
    fn with_both_switches_scene_three_after_a_pause_is_still_judged_in_a_busy_room() {
        for pause_s in [20, 70] {
            let judged = scene_three_and_four_after(true, true, pause_s);
            let played: Vec<bool> = judged.iter().map(|(play, _)| *play).collect();
            assert_eq!(
                played,
                [true, true, false, false, false, false, false, false, false],
                "{pause_s} s"
            );
            assert!(
                judged[..5].iter().all(|(_, room)| *room == Room::Busy),
                "{pause_s} s: {judged:?}"
            );
        }
    }

    /// The first builder on #705 (16:10): on scene 3 and scene 4's opening,
    /// does either of the two halves of #1522's fix pass a line the other
    /// holds? No: `LAMP_REMARKS` alone, `LAMP_GOING_ON_ASKS` alone with the
    /// Policy's own decisions of 14:44, and the two together all answer the
    /// call and "And what about tomorrow?" and hold the other seven. Only
    /// what the room books differs: two remarks held end the conversation,
    /// while a line the Policy holds is not talk, so each line after it is
    /// asked of the Policy again.
    #[test]
    fn on_scene_three_the_two_halves_of_the_fix_hold_the_same_lines() {
        let expected = [true, true, false, false, false, false, false, false, false];
        assert_eq!(scene_three_and_four_with(true, false), expected);
        assert_eq!(scene_three_and_four_with(false, true), expected);
        assert_eq!(scene_three_and_four_with(true, true), expected);
        assert_eq!(scene_three_and_four_with(false, false), [true; 9]);
    }

    #[test]
    fn with_remarks_two_remarks_held_end_a_conversation_its_own_replies_kept_going() {
        // As on lamp-4ace at 14:44: every line answered.
        assert_eq!(scene_three_and_four(false), [true; 9]);
        // The call and the follow-up answered; the umbrella and "leave a bit
        // early" held as remarks, which end it; the car and scene 4 held.
        assert_eq!(
            scene_three_and_four(true),
            [true, true, false, false, false, false, false, false, false]
        );
    }

    /// Outside a busy room, or let out for its name, a remark plays as
    /// before.
    #[test]
    fn with_remarks_a_remark_outside_a_busy_room_or_named_plays() {
        let mut attention = Attention::default();
        attention.speak_when_spoken_to(true);
        attention.hold_remarks(true);
        attention.began_at(7, 4, 1_000_000);
        attention.heard("I should bring an umbrella then.");
        assert_eq!(
            attention.first_output_busy(7, Room::Quiet, true, true, Some(false)),
            Some(Verdict::Play("the room is not known to be busy"))
        );
        let mut attention = Attention::default();
        attention.speak_when_spoken_to(true);
        attention.hold_remarks(true);
        attention.began_at(7, 4, 1_000_000);
        attention.heard("Lamp, I should bring an umbrella then.");
        assert_ne!(
            attention.first_output_busy(7, Room::Busy, true, true, Some(false)),
            Some(Verdict::Hold(crate::remarks::REMARK))
        );
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
        // #1712: on the second route the words written for the lamp's name
        // are not someone else's, and standing alone they call the lamp.
        for heard in [
            "Hey, Lynn. How are you today?",
            "Hey Lynn, how are you today?",
            "Lynn, what time is it?",
            "Thank you, Lynn.",
            "Thank you. Lynn.",
            "Palin, set a timer for five minutes.",
            "Len, what's the weather?",
            "Lin, what time is it?",
            "Louvre, what did I just ask you?",
            "Hey, Louvre. What time is it in London right now?",
            "Lampy, what did I just ask you?",
        ] {
            assert!(!addresses_someone_else_on(heard, true), "{heard}");
            assert!(called_by_name_on(heard, true), "{heard}");
        }
        for heard in [
            "Line up the meetings for me.",
            "We should land the plan soon.",
            "We went to the Louvre in May.",
        ] {
            assert!(!called_by_name_on(heard, true), "{heard}");
        }
        // #1973, the first builder at 22:38: on Live they are people's
        // names, as before.
        for heard in [
            "Lin, pass me that.",
            "Lynn, what time is it?",
            "Thank you, Lin.",
        ] {
            assert!(!called_by_name(heard), "{heard}");
        }
        assert!(addresses_someone_else("Lin, are you coming?"));
        assert!(!addresses_someone_else_on("Lin, are you coming?", true));
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

    /// #1973, the first builder at 22:38: on Live "Lin, pass me that."
    /// between people is held, as before, since "Lin" is a person's name
    /// there; with the second route's stand-ins, "Lin, what time is it?"
    /// calls the lamp, in a busy room too.
    #[test]
    fn the_names_stand_ins_call_the_lamp_on_the_second_route_only() {
        const S: u64 = 1_000_000;
        let judged = |heard: &str, stand_alone: bool, room: Room| {
            // As the units run it: the name in passing ignored.
            let mut attention = Attention::default();
            attention.speak_when_spoken_to(true);
            attention.ignore_name_in_passing(true);
            attention.count_stand_alone_ins(stand_alone);
            attention.began_at(60, 7, 10 * S);
            attention.heard(heard);
            attention.first_output_busy(60, room, false, false, None)
        };
        for room in [Room::Quiet, Room::Busy] {
            assert!(
                matches!(
                    judged("Lin, pass me that.", false, room),
                    Some(Verdict::Hold(_))
                ),
                "Live, {room:?}"
            );
            assert!(
                matches!(
                    judged("Lin, what time is it?", true, room),
                    Some(Verdict::Play(_))
                ),
                "the second route, {room:?}"
            );
            // 25 Oct: held five times of five on lamp-4ace as said to
            // someone it calls by name.
            assert!(
                matches!(
                    judged("Louvre, what did I just ask you?", true, room),
                    Some(Verdict::Play(_))
                ),
                "Louvre on the second route, {room:?}"
            );
            // 26 Oct: held on 6999 as said to someone it calls by name.
            assert!(
                matches!(
                    judged("Lampy, what did I just ask you?", true, room),
                    Some(Verdict::Play(_))
                ),
                "Lampy on the second route, {room:?}"
            );
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

    #[test]
    fn the_real_name_call_keeps_positions_without_recognizer_stand_ins() {
        for called in [
            "Lamp.",
            "Lamp",
            "Hey Lamp.",
            "Lamp tell me a joke.",
            "Lamb, please.",
            "Can you, lamp, stop.",
            "Thanks, Lamp.",
        ] {
            assert!(called_by_real_name(called), "{called}");
        }
        for not_called in [
            "Plan.",
            "Man.",
            "Thanks, Line.",
            "The lamp.",
            "I left the lamp on.",
            "The lamps are new.",
            "",
        ] {
            assert!(!called_by_real_name(not_called), "{not_called}");
        }
        // Recognition-supported callers retain their existing substitutes.
        assert!(called_by_name_on("Plan.", true));
        assert!(called_by_name_on("Plan.", false));
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

    /// `LAMP_NAME_STAYS_IN_ITS_LINE=on`: on lamp-4ace on 2026-10-26 at 06:21
    /// (build 6919) a hearing at 0.92 near the end of "Can you pass me the
    /// charger? The white one." counted for "Here you go. Is your phone dying
    /// again?", begun 0.7 s after it. With the switch it stays with its own
    /// line; a call's goes on, and a hearing after the line before ended,
    /// as the next begins, is the next one's.
    #[test]
    fn a_name_heard_inside_a_line_stays_with_it() {
        let second = 1_000_000u64;
        let next_named = |stays: bool, words: &str, heard_at: u64| {
            let mut attention = Attention::default();
            attention.keep_the_name_in_its_line(stays);
            attention.began_at(45, 7, 9 * second + 200_000);
            attention.heard(words);
            attention.name_heard_scored(heard_at, 0.92);
            attention.ended(45, 11 * second + 700_000);
            attention.began_at(46, 7, 12 * second + 400_000);
            attention.named
        };
        let desk = "Can you pass me the charger? The white one.";
        assert!(
            next_named(false, desk, 11 * second),
            "off: carried, as on 4ace"
        );
        assert!(!next_named(true, desk, 11 * second));
        assert!(next_named(true, "Lamp.", 11 * second), "a call's goes on");
        assert!(next_named(true, desk, 12 * second), "the next line's own");
        // A call at the line's start goes on however its words were written
        // (the reviewer-lead's P2 on `da9a597de`).
        let mut attention = Attention::default();
        attention.keep_the_name_in_its_line(true);
        attention.began_at(1, 7, 9 * second + 200_000);
        attention.heard("Ramp.");
        attention.name_heard_scored(9 * second + 500_000, 0.95);
        attention.ended(1, 9 * second + 800_000);
        attention.began_at(2, 7, 10 * second + 500_000);
        assert!(attention.named, "at its start");
        // Used up: a hearing that stays with the first line counts for no
        // later one either (reviewer-two's P2 on `da9a597de`).
        for stays in [false, true] {
            let mut attention = Attention::default();
            attention.keep_the_name_in_its_line(stays);
            attention.began_at(1, 7, 10 * second);
            attention.heard("I think the oven is still on, by the way.");
            attention.name_heard_scored(12 * second + 900_000, 0.999);
            attention.ended(1, 13 * second);
            attention.began_at(2, 7, 13 * second + 100_000);
            assert_eq!(attention.named, !stays, "the second line, stays {stays}");
            attention.ended(2, 13 * second + 600_000);
            attention.began_at(3, 7, 13 * second + 700_000);
            assert_eq!(attention.named, !stays, "the third line, stays {stays}");
        }
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

    /// A stop to sharing names what is sent, so it is asked (#1925).
    #[test]
    fn a_stop_to_sharing_names_reports() {
        for said in [
            "Stop sending reports.",
            "Lamp, stop sharing!",
            "Don't send my data",
            "STOP THE REPORTS",
        ] {
            assert!(held_line_names_reports(said), "{said}");
        }
    }

    /// Busy-room talk with none of the words is not asked, and a part of a
    /// word is not the word.
    #[test]
    fn busy_room_talk_without_the_words_is_not_asked() {
        for said in [
            "Send me the file.",
            "Can you share your screen?",
            "The reporter called back.",
            "Is the database up?",
            "",
        ] {
            assert!(!held_line_names_reports(said), "{said}");
        }
    }

    /// "report to your manager" has `report`: it is asked once, and the
    /// model decides, which is meant to call nothing.
    #[test]
    fn report_to_your_manager_is_asked_and_left_to_the_model() {
        assert!(held_line_names_reports("Report to your manager first."));
    }

    /// #2002's P1: the piece a pause after the name leaves, heard as the
    /// name alone, and nothing more.
    #[test]
    fn the_name_alone_is_told_from_a_line_with_it() {
        for alone in [
            "Lamp.",
            "lamp",
            "Hey Lamp,",
            "Hey, Lynn.",
            "Louvre.",
            "Okay. Lamp?",
            "Lamb.",
        ] {
            assert!(only_the_name(alone), "{alone}");
        }
        for not in [
            "",
            "Hey.",
            "Lamp, what time is it?",
            "Thank you, Lamp.",
            "Line up the meetings.",
            "The lamp.",
        ] {
            assert!(!only_the_name(not), "{not}");
        }
    }

    /// `LAMP_NAME_SECOND_SIGN=on`: a hearing at the line's start
    /// ([`NAME_AT_THE_START_US`]) is its own second sign, whether it was
    /// heard before the line began or in it, and it stays one for the line
    /// going on after the lamp ended it early; a later one is dropped.
    #[test]
    fn with_a_second_sign_asked_the_name_at_the_lines_start_is_one() {
        let began = 10_000_000;
        let dropped = |at: u64, before: bool, going_on: bool| {
            let mut attention = Attention::default();
            attention.ask_a_second_sign_of_the_name(true);
            if before {
                attention.name_heard_scored(at, 0.95);
            }
            attention.began_at(1, 1, began);
            if !before {
                attention.name_heard_scored(at, 0.95);
            }
            if going_on {
                attention.continued_at(2, 1, began + 3_000_000);
            }
            attention.weigh_the_name(None, false)
        };
        for going_on in [false, true] {
            for before in [false, true] {
                let at_start = began + NAME_AT_THE_START_US;
                assert!(!dropped(at_start, before, going_on), "{before} {going_on}");
                assert!(
                    dropped(at_start + 1, before, going_on),
                    "{before} {going_on}"
                );
            }
            assert!(!dropped(began - NAME_BEFORE_US, true, going_on));
        }
        // A line begun at 10.0 s, the name heard 1.6 s in (mid-line), cut
        // and continued at 11.8 s: the hearing is still mid-line, dropped.
        let mut attention = Attention::default();
        attention.ask_a_second_sign_of_the_name(true);
        attention.began_at(1, 1, began);
        attention.name_heard_scored(began + 1_600_000, 0.95);
        attention.continued_at(2, 1, began + 1_800_000);
        assert!(attention.weigh_the_name(None, false));
    }

    /// #4226, `LAMP_NAME_START_USUAL=on`: a line whose sound began 0.3 s
    /// before its usual start by the soft start's lead, the name heard 1.5 s
    /// after its sound began (1.2 s after its usual start), has the name at
    /// its start: its own second sign. Without the switch the hearing is
    /// past [`NAME_AT_THE_START_US`] and dropped, as before; a lead of 0
    /// changes nothing. Heard before the line began or in it, and for the
    /// line going on after the lamp ended it early, alike. Every other
    /// window keeps the line's own start ([`Attention::window`]).
    #[test]
    fn with_the_usual_start_the_name_at_the_lines_start_is_dated_past_the_soft_lead() {
        let began = 10_000_000;
        let lead = 300_000;
        let weighed = |usual: bool, lead: u64, at: u64, before: bool, going_on: bool| {
            let mut attention = Attention::default();
            attention.ask_a_second_sign_of_the_name(true);
            attention.date_the_start_from_the_usual_start(usual);
            if before {
                attention.name_heard_scored(at, 0.95);
            }
            attention.began_at_led(1, 1, began, lead);
            assert_eq!(attention.window(), Some((began, None)));
            if !before {
                attention.name_heard_scored(at, 0.95);
            }
            if going_on {
                attention.continued_at_led(2, 1, began + 3_000_000, 0);
            }
            let at_start = attention.named_at_start;
            (attention.weigh_the_name(None, false), at_start)
        };
        for going_on in [false, true] {
            let at = began + 1_500_000;
            assert_eq!(weighed(true, lead, at, false, going_on), (false, true));
            assert_eq!(weighed(false, lead, at, false, going_on), (true, false));
            let bound = began + lead + NAME_AT_THE_START_US;
            assert!(!weighed(true, lead, bound, false, going_on).0);
            assert!(weighed(true, lead, bound + 1, false, going_on).0);
            for before in [false, true] {
                for at in [
                    began - NAME_BEFORE_US,
                    began + NAME_AT_THE_START_US,
                    began + NAME_AT_THE_START_US + 1,
                    began + 1_500_000,
                ] {
                    if before && at > began {
                        continue;
                    }
                    let off = weighed(false, 0, at, before, going_on);
                    assert_eq!(weighed(true, 0, at, before, going_on), off, "{at}");
                    assert_eq!(weighed(false, lead, at, before, going_on), off, "{at}");
                }
            }
        }
    }

    /// #3945, `LAMP_NAME_SAID_TO_A_PERSON=on`: "Ma'am, could you pass me
    /// the salt please?", said to a person, heard as the lamp's name at
    /// 0.996 at the line's start, is no call of its own: its first word,
    /// set off, is said to a person. A face turned to the lamp, a
    /// conversation going on, said close, or the switch off still let it
    /// count; a start hearing on any other first word is still one.
    #[test]
    fn with_said_to_a_person_a_start_hearing_on_maam_is_no_second_sign() {
        let began = 10_000_000;
        let weighed = |line: &str, on: bool, turned: Option<bool>, going_on: bool, near: bool| {
            let mut attention = Attention::default();
            attention.ask_a_second_sign_of_the_name(true);
            attention.hold_a_start_said_to_a_person(on);
            attention.began_at(1, 1, began);
            attention.name_heard_scored(began + 200_000, 0.996);
            attention.heard(line);
            attention.set_near(near);
            let held = attention.start_said_to_a_person();
            (attention.weigh_the_name(turned, going_on), held)
        };
        let maam = "Ma'am, could you pass me the salt please?";
        assert_eq!(
            weighed(maam, true, None, false, false),
            (true, Some("ma'am"))
        );
        assert_eq!(
            weighed(
                "Ma\u{2019}am, could you pass me the salt please?",
                true,
                None,
                false,
                false
            ),
            (true, Some("ma'am"))
        );
        for (line, word) in [
            ("Sir, could you pass me the salt please?", "sir"),
            ("Honey, could you pass me the salt please?", "honey"),
            ("Hey guys, could you pass me the salt please?", "guys"),
        ] {
            assert_eq!(
                weighed(line, true, None, false, false),
                (true, Some(word)),
                "{line}"
            );
        }
        // Every other second sign still lets it count, and the journal
        // names it (#3945's 23:33 run, request 2: which one let it out?).
        let signs = |turned: Option<bool>, going_on: bool, near: bool, on: bool| {
            let mut attention = Attention::default();
            attention.ask_a_second_sign_of_the_name(true);
            attention.hold_a_start_said_to_a_person(on);
            attention.began_at(1, 1, began);
            attention.name_heard_scored(began + 200_000, 0.996);
            attention.heard(maam);
            attention.set_near(near);
            attention.weigh_the_name(turned, going_on);
            attention.second_signs().to_vec()
        };
        assert_eq!(
            signs(Some(true), false, false, true),
            ["a face turned to the lamp"]
        );
        assert_eq!(signs(None, true, false, true), ["a conversation going on"]);
        assert_eq!(
            signs(None, false, true, true),
            ["the name said close to it"]
        );
        assert_eq!(
            signs(None, false, false, false),
            ["its hearing at the line's start"]
        );
        assert!(signs(None, false, false, true).is_empty());
        assert!(!weighed(maam, true, Some(true), false, false).0);
        assert!(!weighed(maam, true, None, true, false).0);
        assert!(!weighed(maam, true, None, false, true).0);
        // The switch off: the start counts, as before.
        assert_eq!(weighed(maam, false, None, false, false), (false, None));
        // Without it a conversation going on is no second sign, as before.
        let mut attention = Attention::default();
        attention.ask_a_second_sign_of_the_name(true);
        attention.began_at(1, 1, began);
        attention.name_heard_scored(began + 2_000_000, 0.996);
        assert!(attention.weigh_the_name(None, true));
        // With it, a conversation going on gives back only a held start: a
        // hearing later in the line is still dropped, as before.
        let mut attention = Attention::default();
        attention.ask_a_second_sign_of_the_name(true);
        attention.hold_a_start_said_to_a_person(true);
        attention.began_at(1, 1, began);
        attention.name_heard_scored(began + 2_000_000, 0.996);
        attention.heard("Could you pass me the salt please?");
        assert!(attention.weigh_the_name(None, true));
        // Any other first word keeps the start's sign.
        for line in [
            "Lamp, what time is it in London?",
            "1. How are you today?",
            "Blake, what time is it in London?",
            "Man, what time is it in London?",
            "Could you pass me the salt please?",
            "Ma'am",
        ] {
            let (dropped, held) = weighed(line, true, None, false, false);
            if line == "Ma'am" {
                assert_eq!(held, Some("ma'am"));
                continue;
            }
            assert!(!dropped, "{line}");
            assert_eq!(held, None, "{line}");
        }
        // Never a veto: the name, a word written for it.
        for word in ["lamp", "lamb"]
            .iter()
            .chain(STAND_INS)
            .chain(STAND_ALONE_INS)
        {
            let line = format!("{word}, what time is it in London?");
            assert_eq!(said_to_a_person(&line), None, "{line}");
            let mut capital = word.to_string();
            capital[..1].make_ascii_uppercase();
            let line = format!("{capital}, what time is it in London?");
            assert_eq!(
                weighed(&line, true, None, false, false),
                (false, None),
                "{line}"
            );
        }
        // Said to a person only set off: "Dad's car" is no call to a person.
        assert_eq!(said_to_a_person("Dad's car is outside."), None);
        assert_eq!(said_to_a_person("Sir Paul sang it."), None);
    }

    /// A line for [`judged_at_the_desk`].
    #[derive(Clone, Copy, Default)]
    struct DeskLine {
        room: Option<Room>,
        strict: bool,
        spoken_to: bool,
        /// Past the session's first [`FRESH_REQUESTS`].
        trusted: bool,
        /// Ended as a short sound, checked.
        sound: bool,
        /// The name heard in its sound, at 0.95.
        named: bool,
        tapped: bool,
        going_on: bool,
    }

    /// The verdict on `heard`, said as `line` says, with `turned` from the
    /// eyes, and `LAMP_NAME_OR_FOLLOW_UP` `on` or not; and the sign the
    /// journal gives it.
    fn judged_at_the_desk(
        on: bool,
        line: DeskLine,
        heard: &str,
        turned: Option<bool>,
    ) -> (Verdict, Option<&'static str>) {
        let mut attention = Attention::default();
        attention.need_name_or_follow_up(on);
        attention.speak_when_spoken_to(line.spoken_to);
        attention.check_sounds(line.sound);
        let s = 1_000_000;
        let first = if line.trusted { 1 } else { 3 };
        for request in first..3 {
            attention.began_at(request, 1, request * 10 * s);
        }
        if line.tapped {
            attention.head_tapped(30 * s - s / 2);
        }
        attention.began_at(3, 1, 30 * s);
        if line.named {
            attention.name_heard_scored(30 * s + s / 5, 0.95);
        }
        attention.heard(heard);
        if line.sound {
            attention.ended_as_sound(3);
        }
        let verdict = attention
            .first_output_busy(
                3,
                line.room.unwrap_or(Room::Quiet),
                line.going_on,
                line.strict,
                turned,
            )
            .unwrap();
        (verdict, attention.sign(turned))
    }

    /// `LAMP_NAME_OR_FOLLOW_UP`: at the owner's desk on lamp-4ace on 22
    /// October, the owner talked to a colleague facing the lamp's way, and 8
    /// of the lamp's 18 answers were let out as "someone is turned to the
    /// lamp". With the switch, in a busy room, a face turned to the lamp
    /// lets nothing out alone: in either form, with `--spoken-to` or not, in
    /// a trusted session, or as a short sound; the journal gives it as no
    /// sign. In a room not known to be busy it is a reason as without the
    /// switch (the owner at 18:0x: a person alone with the lamp does not say
    /// its name), and given as the sign. Without the switch each is played.
    #[test]
    fn with_name_or_follow_up_a_face_turned_to_the_lamp_lets_nothing_out_alone_in_a_busy_room() {
        let talk = "Okay yeah, sure.";
        let busy = Some(Room::Busy);
        let face = Some(true);
        for (line, held) in [
            (
                DeskLine {
                    room: busy,
                    strict: true,
                    ..DeskLine::default()
                },
                BUSY_TURNED_ALONE,
            ),
            (
                DeskLine {
                    room: busy,
                    strict: true,
                    spoken_to: true,
                    ..DeskLine::default()
                },
                BUSY_TURNED_ALONE,
            ),
            (
                DeskLine {
                    room: busy,
                    ..DeskLine::default()
                },
                "a new session in a busy room, and nothing shows it was called",
            ),
            (
                DeskLine {
                    room: busy,
                    spoken_to: true,
                    trusted: true,
                    ..DeskLine::default()
                },
                TURNED_NOT_SPOKEN_TO,
            ),
            (
                DeskLine {
                    room: busy,
                    sound: true,
                    ..DeskLine::default()
                },
                SOUND_TURNED_ALONE,
            ),
        ] {
            let (verdict, sign) = judged_at_the_desk(true, line, talk, face);
            assert_eq!(verdict, Verdict::Hold(held));
            assert_eq!(sign, None, "{held}");
            let (verdict, sign) = judged_at_the_desk(false, line, talk, face);
            assert!(matches!(verdict, Verdict::Play(_)), "{held}: {verdict:?}");
            assert_eq!(sign, Some(TURNED), "{held}");
        }
        for line in [
            DeskLine {
                spoken_to: true,
                ..DeskLine::default()
            },
            DeskLine {
                room: Some(Room::Unknown),
                spoken_to: true,
                trusted: true,
                ..DeskLine::default()
            },
            DeskLine {
                sound: true,
                ..DeskLine::default()
            },
        ] {
            let on = judged_at_the_desk(true, line, talk, face);
            assert!(matches!(on.0, Verdict::Play(_)), "{:?}", on.0);
            assert_eq!(on.1, Some(TURNED));
            assert_eq!(on, judged_at_the_desk(false, line, talk, face));
        }
    }

    /// With the switch, a face turned to the lamp still confirms the name
    /// heard in the sound: in a busy room in either form, with
    /// `--spoken-to`, and as a short sound, it is played as before.
    #[test]
    fn with_name_or_follow_up_a_face_and_the_name_heard_are_played() {
        let face = Some(true);
        for line in [
            DeskLine {
                room: Some(Room::Busy),
                strict: true,
                named: true,
                ..DeskLine::default()
            },
            DeskLine {
                room: Some(Room::Busy),
                named: true,
                ..DeskLine::default()
            },
            DeskLine {
                room: Some(Room::Busy),
                strict: true,
                spoken_to: true,
                named: true,
                ..DeskLine::default()
            },
            DeskLine {
                spoken_to: true,
                named: true,
                ..DeskLine::default()
            },
        ] {
            let on = judged_at_the_desk(true, line, "What time is it?", face);
            assert!(matches!(on.0, Verdict::Play(_)), "{:?}", on.0);
            assert_eq!(
                on,
                judged_at_the_desk(false, line, "What time is it?", face)
            );
        }
        let sound = DeskLine {
            sound: true,
            named: true,
            ..DeskLine::default()
        };
        assert_eq!(
            judged_at_the_desk(true, sound, "¿Qué?", face).0,
            Verdict::Play(SOUND_TURNED)
        );
    }

    /// With the switch, a tap, the text calling the lamp and a conversation
    /// going on are played whatever the eyes see, in every room and either
    /// form, as without it. So is the name heard in the sound, but in a
    /// strict busy room with nobody turned to the lamp, which is judged as
    /// without the switch.
    #[test]
    fn with_name_or_follow_up_the_name_a_tap_the_text_and_a_follow_up_are_played() {
        let heard_as = |line: DeskLine| match (line.tapped, line.going_on) {
            (true, _) => "Check my work.",
            (_, true) => "And what about tomorrow?",
            _ => "Hey Lamp, what time is it?",
        };
        for room in [Room::Quiet, Room::Unknown, Room::Busy] {
            for strict in [false, true] {
                for spoken_to in [false, true] {
                    for turned in [None, Some(false), Some(true)] {
                        let at = DeskLine {
                            room: Some(room),
                            strict,
                            spoken_to,
                            ..DeskLine::default()
                        };
                        for line in [
                            at,
                            DeskLine { tapped: true, ..at },
                            DeskLine {
                                going_on: true,
                                ..at
                            },
                        ] {
                            let on = judged_at_the_desk(true, line, heard_as(line), turned);
                            let why = format!("{room:?} {strict} {spoken_to} {turned:?}");
                            assert!(matches!(on.0, Verdict::Play(_)), "{why}: {:?}", on.0);
                            assert_eq!(
                                on.0,
                                judged_at_the_desk(false, line, heard_as(line), turned).0,
                                "{why}"
                            );
                        }
                        let named = DeskLine { named: true, ..at };
                        let on = judged_at_the_desk(true, named, "What time is it?", turned);
                        let off = judged_at_the_desk(false, named, "What time is it?", turned);
                        if turned == Some(true) || !(strict && room == Room::Busy) {
                            assert!(matches!(on.0, Verdict::Play(_)), "{room:?} {turned:?}");
                        }
                        assert_eq!(on.0, off.0, "{room:?} {strict} {spoken_to} {turned:?}");
                    }
                }
            }
        }
    }
}
