//! A reply that says a light, volume or Schedule action was done, with no
//! call of that kind in its turn, is never said as it is
//! (`LAMP_ACTION_CLAIM_CHECKED`, #5215).
//!
//! On lamp-52e6 at build 9928 (4 Nov 09:38) "Lamp, set a timer to one
//! minute." was heard right, no chat round called anything, and the lamp
//! said "Timer set. It rings at 9:39 am.": no timer existed. In an earlier
//! run 4 of 14 light and volume asks got "Louder." (twice), "Brighter." or
//! "Cooler." and no call, so nothing changed. The model copies the acting
//! part's own lines, kept in the thread as the lamp's words.
//!
//! The test is strict, never a phrase list: the reply, its case, spaces and
//! curly apostrophes aside, is or opens with one of the acting part's own
//! lines, followed by its end or a sentence mark ('.', '!', ',', ';', ':',
//! a dash), and never a '?':
//! - the executive's lines after a light or volume call ([`SAID_LINES`],
//!   `lamp_actions::SAID_LINES`). Not its line for a colour made ("My light
//!   is blue now."): with `LAMP_LIGHT_TOLD` it is also how a turn answers
//!   "what colour is the light?", and kept colour calls carry their colour
//!   since #5101 (the first builder's review of #5215);
//! - the Schedule's set lines as the second route says them
//!   (`second_provider::timer_line`): "Timer set." and "I'll remind you at
//!   H:MM am." (or pm). The second is also an answer to "when will you
//!   remind me?", so a `list_reminders` call in the turn makes it stand
//!   ([`Claim::ReminderTime`]); "Timer set." needs a set.
//!
//! Such a claim stands when its turn made a call of its kind
//! ([`Claim::made_by`]); otherwise it is held, the model is asked once more
//! with [`NOTE`], and a second claim with no call is said as
//! [`NOT_DONE_LINE`]. Anything else, an answer to a question included, is
//! untouched. Off unless the environment says exactly `on`.
//!
//! **Worded claims** (`LAMP_ACTION_CLAIM_WORDED`, beside the switch above,
//! #5215's open shape): the model also words claims its own way, which the
//! strict test lets through. On 10441 (5 Nov 04:24) "Hey Lamp, keep the
//! light on until ten." got `calls=none` and "I've kept the light on until
//! ten.", and nothing held it: claimed-not-done stayed at 1. With this
//! switch a reply that opens (bare or after a lead-in such as "Okay,") with
//! one of [`WORDED`]'s short word runs is a claim too, held and judged the
//! same way. Each run is tied to one family with a call behind it: a timer,
//! reminder or alarm set ("I've set a timer …", "Timer set for one minute"),
//! the light or volume changed ("I've turned the volume up.", "Okay,
//! louder."), or the light kept on or off until a time
//! ([`Claim::LightKept`]; "for" only with a time after it). A colour said
//! ("My light is blue now.") is still no claim, nor is any sentence the run
//! opens that ends in a '?'. The worded runs judge only a turn with no call
//! at all, so work another tool did stands ("I've added a reminder to your
//! calendar." beside `service_write`), and never a line the person asked as
//! a question ([`asked_as_a_question`]), whose reply may tell a true state
//! ("The light stays on until ten."); the strict lines judge every turn
//! (the first builder's read of #5588).

/// Where the second worker reads the switch. `on` only.
pub const ENVIRONMENT: &str = "LAMP_ACTION_CLAIM_CHECKED";

/// Whether the switch is on in this process's environment.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Where the second worker reads the worded claims' switch. `on` only, and
/// it does nothing without [`ENVIRONMENT`].
pub const WORDED_ENVIRONMENT: &str = "LAMP_ACTION_CLAIM_WORDED";

/// Whether the worded claims' switch is on in this process's environment.
pub fn worded_on() -> bool {
    std::env::var(WORDED_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The executive's own lines after a light or volume call
/// (`lamp_actions::short_say` and `already_say`): a test holds this equal
/// to `lamp_actions::SAID_LINES` (the voice links lamp-actions only in its
/// tests).
pub const SAID_LINES: [&str; 14] = [
    "Brighter.",
    "Dimmer.",
    "Warmer.",
    "Cooler.",
    "Louder.",
    "Quieter.",
    "Done.",
    "I'm already as bright as I go.",
    "I'm already as dim as I go.",
    "I'm already as warm as I go.",
    "I'm already as cool as I go.",
    "I'm already as loud as I go.",
    "I'm already as quiet as I go.",
    "It already is.",
];

/// Of [`SAID_LINES`], the two any call may end in ("Done." for calls that
/// differ, `second_provider::SHORT_DONE_TOGETHER`): any call of the turn
/// answers them.
const ANY_CALL_LINES: [&str; 2] = ["Done.", "It already is."];

/// The Schedule's set line for a timer, as `second_provider::timer_line`
/// says it, to its first sentence's end.
const TIMER_SET: &str = "Timer set.";
/// The Schedule's set line for a timer asked as a reminder
/// (`second_provider::timer_line`), up to its clock.
const REMIND_AT: &str = "I'll remind you at ";

/// What follows a claim's words for it to be one: its end, or a sentence
/// mark. Never a '?': "Brighter?" asks.
const MARKS: [char; 7] = ['.', '!', ',', ';', ':', '\u{2014}', '\u{2013}'];
/// Dashes that mark a claim's end after one space too ("Done \u{2014} it's red.").
const DASHES: [char; 3] = ['\u{2014}', '\u{2013}', '-'];

/// Told before the round asked again after a claim with no call.
pub const NOTE: &str = "(For you to know: your last reply said something was done, but no function was called in this turn, so nothing was done, and it was not said. If they asked for a change, a timer or a reminder, call its function now; the lamp says the result itself. If you cannot do it, say plainly that you didn't. If they only asked a question, answer it in words without saying that anything was done.)";

/// What the next round is told when a claim was held beside calls of
/// another kind, which did run (Thirteen's P2 on #5253): never "no function
/// was called", so the model doesn't call those again (a second timer, a
/// light made brighter twice), and only the claimed kind is asked for.
pub fn beside_calls_note(kind: Claim) -> String {
    format!(
        "(For you to know: your last reply said {} was done, but no function for it was called, so it was not done, and those words were not said. The other functions in that reply did run: do not call them again. If they asked for {}, call its function now; the lamp says the result itself. If not, or if you cannot, say plainly that it wasn't done.)",
        kind.what(),
        kind.what()
    )
}

/// Said when the round asked again claims again with no call.
pub const NOT_DONE_LINE: &str = "Sorry, I didn't do that. Could you ask me again?";

/// The kind of action a reply claims.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Claim {
    /// A light or volume line ("Louder.").
    LightOrVolume,
    /// "Done." or "It already is.": any call's line.
    AnyCall,
    /// A Schedule's set line ("Timer set.").
    Schedule,
    /// "I'll remind you at H:MM am.": a set line, or what a list answers.
    ReminderTime,
    /// The light kept on or off until a time, in other words ("I've kept
    /// the light on until ten."): a light call, a Schedule set or a kept
    /// rule makes it.
    LightKept,
}

impl Claim {
    /// Whether a call of `name` makes this claim true.
    pub fn made_by(self, name: &str) -> bool {
        match self {
            Self::LightOrVolume => crate::tools::is_light_or_volume(name),
            Self::AnyCall => true,
            Self::Schedule => matches!(
                name,
                "set_timer" | "set_reminder" | "repeat_reminder" | "snooze_reminder"
            ),
            Self::ReminderTime => Self::Schedule.made_by(name) || name == "list_reminders",
            Self::LightKept => {
                Self::LightOrVolume.made_by(name)
                    || Self::Schedule.made_by(name)
                    || name == crate::rules::KEEP_RULE
            }
        }
    }

    /// What it claims, in the words a note tells the model.
    pub fn what(self) -> &'static str {
        match self {
            Self::LightOrVolume => "a change to the light or the volume",
            Self::AnyCall => "an action",
            Self::Schedule => "a timer or a reminder",
            Self::ReminderTime => "a reminder",
            Self::LightKept => "keeping the light on or off until a time",
        }
    }

    /// Its name in the journal.
    pub fn label(self) -> &'static str {
        match self {
            Self::LightOrVolume => "light or volume",
            Self::AnyCall => "done",
            Self::Schedule => "timer or reminder",
            Self::ReminderTime => "reminder time",
            Self::LightKept => "light kept",
        }
    }
}

/// `text` as the claim test reads it: lower case, straight apostrophes,
/// each run of spaces one, none before.
fn normalised(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.trim_start().chars() {
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.extend(if c == '\u{2019}' {
            '\''.to_lowercase()
        } else {
            c.to_lowercase()
        });
    }
    if space && !out.is_empty() {
        out.push(' ');
    }
    out
}

/// A line's words as [`normalised`] reads them, its closing '.' or '!' off.
fn stem(line: &str) -> String {
    normalised(line).trim_end_matches(['.', '!']).to_owned()
}

/// Whether `text` opens with `stem` and then its end or a mark: `None` while
/// more text could still decide it.
fn opens_with(text: &str, stem: &str, ended: bool) -> Option<bool> {
    match text.strip_prefix(stem) {
        Some(rest) => marked(rest, ended),
        None if !ended && stem.starts_with(text) => None,
        None => Some(false),
    }
}

/// Whether what follows a claim's words ends them: nothing more, a mark, or
/// one space and a dash. `None` while undecided.
fn marked(rest: &str, ended: bool) -> Option<bool> {
    let mut chars = rest.chars();
    match chars.next() {
        None if ended => Some(true),
        None => None,
        Some(' ') => match chars.next() {
            None if ended => Some(true),
            None => None,
            Some(c) => Some(DASHES.contains(&c)),
        },
        Some(c) => Some(MARKS.contains(&c)),
    }
}

/// Whether `rest` opens with a clock as the lamp says one ("9:39 am",
/// "12:05 pm") and then its end or a mark: `None` while undecided.
fn opens_with_a_clock(rest: &str, ended: bool) -> Option<bool> {
    let mut undecided = false;
    for shape in ["0:00 xm", "00:00 xm"] {
        let mut chars = rest.chars();
        let mut fits = true;
        let mut short = false;
        for want in shape.chars() {
            let Some(c) = chars.next() else {
                short = true;
                break;
            };
            let ok = match want {
                '0' => c.is_ascii_digit(),
                'x' => c == 'a' || c == 'p',
                want => c == want,
            };
            if !ok {
                fits = false;
                break;
            }
        }
        if !fits {
            continue;
        }
        if short {
            undecided |= !ended;
            continue;
        }
        match marked(chars.as_str(), ended) {
            Some(true) => return Some(true),
            None => undecided = true,
            Some(false) => {}
        }
    }
    if undecided { None } else { Some(false) }
}

/// Whether `reply` claims an action as the acting part's own lines say
/// one (the module's rule): `Some(Some(kind))`, a claim; `Some(None)`, not
/// one; `None`, not yet known from the text so far. `ended`: no more text
/// comes.
pub fn claim(reply: &str, ended: bool) -> Option<Option<Claim>> {
    let mut text = normalised(reply);
    if ended {
        text.truncate(text.trim_end().len());
    }
    if text.is_empty() {
        return if ended { Some(None) } else { None };
    }
    let mut undecided = false;
    let mut judge = |found: Option<bool>, kind: Claim| -> Option<Claim> {
        match found {
            Some(true) => Some(kind),
            None => {
                undecided = true;
                None
            }
            Some(false) => None,
        }
    };
    let lines = SAID_LINES
        .iter()
        .map(|line| {
            let kind = if ANY_CALL_LINES.contains(line) {
                Claim::AnyCall
            } else {
                Claim::LightOrVolume
            };
            (stem(line), kind)
        })
        .chain(std::iter::once((stem(TIMER_SET), Claim::Schedule)));
    for (stem, kind) in lines {
        if let Some(kind) = judge(opens_with(&text, &stem, ended), kind) {
            return Some(Some(kind));
        }
    }
    let remind = normalised(REMIND_AT);
    let found = match text.strip_prefix(remind.as_str()) {
        Some(rest) => opens_with_a_clock(rest, ended),
        None if !ended && remind.starts_with(text.as_str()) => None,
        None => Some(false),
    };
    if let Some(kind) = judge(found, Claim::ReminderTime) {
        return Some(Some(kind));
    }
    if undecided { None } else { Some(None) }
}

/// [`claim`], and with `worded` ([`WORDED_ENVIRONMENT`]) the worded claims
/// too ([`WORDED`]): as [`claim`] says it. A claim in the acting part's own
/// words is judged first; without `worded` this is [`claim`] exactly.
pub fn claim_as(reply: &str, ended: bool, worded: bool) -> Option<Option<Claim>> {
    let strict = claim(reply, ended);
    if !worded || matches!(strict, Some(Some(_))) {
        return strict;
    }
    let mut text = normalised(reply);
    if ended {
        text.truncate(text.trim_end().len());
    }
    match (strict, worded_claim(&text, ended)) {
        (_, Some(Some(kind))) => Some(Some(kind)),
        (Some(None), Some(None)) => Some(None),
        _ => None,
    }
}

/// The words a line the person asked as a question opens with
/// ([`asked_as_a_question`]), after any [`ADDRESS`].
const QUESTION_WORDS: [&str; 21] = [
    "how", "is", "are", "do", "does", "did", "what", "when", "will", "can", "has", "have", "was",
    // The first builder's P3 on #5604: the split keeps a contraction whole.
    "what's", "how's", "where", "where's", "who", "who's", "why", "which",
];

/// Words that only address the lamp before a line's own words ("Hey Lamp,").
const ADDRESS: [&str; 6] = ["hey", "hi", "hello", "ok", "okay", "lamp"];

/// How a request asked as a question opens, after any [`ADDRESS`] ("Lamp,
/// can you set a timer?"): never a status question ([`asked_as_a_question`]).
const REQUEST_OPENINGS: [&[&str]; 6] = [
    &["can", "you"],
    &["could", "you"],
    &["will", "you"],
    &["would", "you"],
    &["can", "i", "have"],
    &["please"],
];

/// Whether the person asked `said` as a status question: a question that is
/// not a request. A question ends with a '?', or its first word after any
/// [`ADDRESS`] is one of [`QUESTION_WORDS`] ("Lamp, how long will the light
/// stay on?", "is my timer still set"). A request opens with one of
/// [`REQUEST_OPENINGS`] or says "please" anywhere ("Lamp, can you set a
/// timer for one minute?"), and a claim with no call answering it is
/// #5215's own bug, so it is still judged. A status question's reply may
/// tell the state ("The light stays on until ten.", "Your timer is set for
/// one minute."), so the worded claims leave its turn alone (the first
/// builder's P2 on #5588, as Twelve refined it); the strict lines still
/// judge it. The second route also judges worded claims only in a turn with
/// no call at all, so work done by another tool ("I've added a reminder to
/// your calendar." beside `service_write`) stands.
pub fn asked_as_a_question(said: &str) -> bool {
    let text = normalised(said);
    let words: Vec<&str> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .collect();
    if words.contains(&"please") {
        return false;
    }
    let own = words
        .iter()
        .position(|word| !ADDRESS.contains(word))
        .map_or(&[][..], |at| &words[at..]);
    if REQUEST_OPENINGS
        .iter()
        .any(|opening| own.starts_with(opening))
    {
        return false;
    }
    text.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ')'))
        .ends_with('?')
        || own
            .first()
            .is_some_and(|word| QUESTION_WORDS.contains(word))
}

/// The worded claims (`LAMP_ACTION_CLAIM_WORDED`, #5215): a reply that opens
/// with one of these, bare or after one of [`LEAD_INS`] and a mark, is a
/// claim of its kind. Each is a run of words: `a|b` is either word, `*` up
/// to [`ANY_WORDS`] words, `#` a time ([`Slot::Time`]), and `$` the end or
/// a mark that is not a '?'.
/// After the run, the sentence it opens must not end in a '?'. Each run is
/// tied to a family with a call behind it, and never a colour said ("My
/// light is blue now." also answers a colour question).
const WORDED: [(&str, Claim); 53] = [
    // "Timer set for one minute.", "Reminder set."
    ("timer|reminder|alarm set|started", Claim::Schedule),
    ("timer's|reminder's|alarm's set|started", Claim::Schedule),
    // "Your one-minute timer is set.": also how a status answer may open,
    // so a `list_reminders` in the turn makes it stand.
    (
        "your|the|a * timer|reminder|alarm is set|started",
        Claim::ReminderTime,
    ),
    (
        "your|the|a * timer|reminder|alarm has been set|started",
        Claim::ReminderTime,
    ),
    (
        "your|the * timer's|reminder's|alarm's set|started",
        Claim::ReminderTime,
    ),
    // "I've set a timer for one minute."
    (
        "i've|i set|started|added * timer|reminder|alarm|timers|reminders|alarms",
        Claim::Schedule,
    ),
    (
        "i have set|started|added * timer|reminder|alarm|timers|reminders|alarms",
        Claim::Schedule,
    ),
    // "I'll remind you in ten minutes.": a set, never an answer about one
    // already kept ("I'll remind you at nine." is left as it is).
    ("i'll remind you in", Claim::Schedule),
    ("i will remind you in", Claim::Schedule),
    // "Okay, louder.", "Louder now."
    (
        "louder|quieter|softer|brighter|dimmer|warmer|cooler $",
        Claim::LightOrVolume,
    ),
    (
        "louder|quieter|softer|brighter|dimmer|warmer|cooler now",
        Claim::LightOrVolume,
    ),
    ("volume up|down $", Claim::LightOrVolume),
    ("light|lights on|off|up|down $", Claim::LightOrVolume),
    // "I've turned the volume up.", "I turned off the light."
    (
        "i've|i turned|switched|made|set|dimmed|brightened|raised|lowered * light|lights|lamp|volume|sound",
        Claim::LightOrVolume,
    ),
    (
        "i have turned|switched|made|set|dimmed|brightened|raised|lowered * light|lights|lamp|volume|sound",
        Claim::LightOrVolume,
    ),
    // "I've turned it up.", "Turned it down."
    (
        "i've|i turned|switched it|that up|down|on|off",
        Claim::LightOrVolume,
    ),
    (
        "turned|switched it|that up|down|on|off",
        Claim::LightOrVolume,
    ),
    (
        "turned|switched the|my|your light|lights|volume up|down|on|off",
        Claim::LightOrVolume,
    ),
    (
        "i've|i made it|that louder|quieter|softer|brighter|dimmer|warmer|cooler",
        Claim::LightOrVolume,
    ),
    // "I've kept the light on until ten." (10441, 5 Nov 04:24, no call).
    // "For" only with a time after it ([`Slot::Time`]): "I left a light on
    // for you." is a persona line (the first builder's P3 on #5588).
    (
        "i've|i kept|left * light|lights|lamp on|off until|till",
        Claim::LightKept,
    ),
    (
        "i've|i kept|left * light|lights|lamp on|off for #",
        Claim::LightKept,
    ),
    (
        "i'll|i'm keep|keeping|leave|leaving * light|lights|lamp on|off until|till",
        Claim::LightKept,
    ),
    (
        "i'll|i'm keep|keeping|leave|leaving * light|lights|lamp on|off for #",
        Claim::LightKept,
    ),
    (
        "i will keep|leave * light|lights|lamp on|off until|till",
        Claim::LightKept,
    ),
    (
        "i will keep|leave * light|lights|lamp on|off for #",
        Claim::LightKept,
    ),
    (
        "the|my|your light|lights|lamp stays|stay on|off until|till",
        Claim::LightKept,
    ),
    (
        "the|my|your light|lights|lamp stays|stay on|off for #",
        Claim::LightKept,
    ),
    (
        "the|my|your light|lights|lamp will stay on|off until|till",
        Claim::LightKept,
    ),
    (
        "the|my|your light|lights|lamp will stay on|off for #",
        Claim::LightKept,
    ),
    (
        "the|my|your light|lights|lamp is|are staying on|off until|till",
        Claim::LightKept,
    ),
    (
        "the|my|your light|lights|lamp is|are staying on|off for #",
        Claim::LightKept,
    ),
    // Said as it is done (#5215's rest): "Turning it up.", "Setting a timer
    // for one minute." Each closes at the sentence's end; a timer's "for"
    // only with a time, and a unit, after it: "Turning it off and on again
    // usually helps.", "Setting a timer now would be smart." and "Starting a
    // reminder now isn't possible." are talk (the first builder's HOLD on
    // #5628). "Raising the lamp." is the arm, so only the volume is raised.
    (
        "i'm turning|switching it|that up|down|on|off $",
        Claim::LightOrVolume,
    ),
    (
        "turning|switching it|that up|down|on|off $",
        Claim::LightOrVolume,
    ),
    (
        "i'm turning|switching the|my|your light|lights|volume up|down|on|off $",
        Claim::LightOrVolume,
    ),
    (
        "turning|switching the|my|your light|lights|volume up|down|on|off $",
        Claim::LightOrVolume,
    ),
    (
        "i'm turning|switching up|down|on|off the|my|your light|lights|volume $",
        Claim::LightOrVolume,
    ),
    (
        "turning|switching up|down|on|off the|my|your light|lights|volume $",
        Claim::LightOrVolume,
    ),
    (
        "i'm dimming|brightening * light|lights|lamp $",
        Claim::LightOrVolume,
    ),
    (
        "i'm raising|lowering * volume|sound $",
        Claim::LightOrVolume,
    ),
    (
        "dimming|brightening * light|lights|lamp $",
        Claim::LightOrVolume,
    ),
    ("raising|lowering * volume|sound $", Claim::LightOrVolume),
    (
        "i'm making it|that louder|quieter|softer|brighter|dimmer|warmer|cooler $",
        Claim::LightOrVolume,
    ),
    (
        "making it|that louder|quieter|softer|brighter|dimmer|warmer|cooler $",
        Claim::LightOrVolume,
    ),
    (
        "i'm setting|starting * timer|reminder|alarm $",
        Claim::Schedule,
    ),
    (
        "i'm setting|starting * timer|reminder|alarm now $",
        Claim::Schedule,
    ),
    (
        "i'm setting|starting * timer|reminder|alarm for # $",
        Claim::Schedule,
    ),
    (
        "i'm setting|starting * timer|reminder|alarm for # second|seconds|minute|minutes|hour|hours|am|pm $",
        Claim::Schedule,
    ),
    (
        "i'm setting|starting * timer|reminder|alarm for # a|an minute|hour $",
        Claim::Schedule,
    ),
    ("setting|starting * timer|reminder|alarm $", Claim::Schedule),
    (
        "setting|starting * timer|reminder|alarm now $",
        Claim::Schedule,
    ),
    (
        "setting|starting * timer|reminder|alarm for # $",
        Claim::Schedule,
    ),
    (
        "setting|starting * timer|reminder|alarm for # second|seconds|minute|minutes|hour|hours|am|pm $",
        Claim::Schedule,
    ),
    (
        "setting|starting * timer|reminder|alarm for # a|an minute|hour $",
        Claim::Schedule,
    ),
];

/// Words said before a worded claim, then a mark ("Okay, louder."). Never a
/// '?'.
const LEAD_INS: [&str; 5] = [
    "okay|ok|sure|alright|there|right|done",
    "all right",
    "there you go",
    "got it",
    "you got it",
];

/// How many words a `*` takes at most.
const ANY_WORDS: usize = 3;

/// One piece of a reply as the worded claims read it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Token<'a> {
    Word(&'a str),
    /// The last word while more text may come: it may yet grow.
    Part(&'a str),
    Mark(char),
}

/// `text` (already [`normalised`]) as words and marks.
fn tokens(text: &str, ended: bool) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (at, c) in text.char_indices() {
        if c.is_alphanumeric() || c == '\'' || (c == '-' && start.is_some()) {
            start.get_or_insert(at);
            continue;
        }
        if let Some(from) = start.take() {
            out.push(Token::Word(&text[from..at]));
        }
        if !c.is_whitespace() {
            out.push(Token::Mark(c));
        }
    }
    if let Some(from) = start {
        out.push(if ended {
            Token::Word(&text[from..])
        } else {
            Token::Part(&text[from..])
        });
    }
    out
}

/// One slot of a worded claim.
#[derive(Clone, Copy)]
enum Slot<'a> {
    /// One of these words, '|' between them.
    Words(&'a str),
    /// Up to [`ANY_WORDS`] words.
    Any,
    /// The end, or a mark that is not a '?'.
    End,
    /// A mark that is not a '?' (after a lead-in).
    Mark,
    /// A time: a word that opens with a digit, one of [`NUMBER_WORDS`],
    /// "tonight", or "the night".
    Time,
}

/// The number words a [`Slot::Time`] takes ("for ten minutes", "for half
/// an hour").
const NUMBER_WORDS: [&str; 18] = [
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
    "twelve", "fifteen", "twenty", "thirty", "forty", "ninety", "half",
];

fn slots(pattern: &str) -> Vec<Slot<'_>> {
    pattern
        .split(' ')
        .map(|slot| match slot {
            "*" => Slot::Any,
            "$" => Slot::End,
            "#" => Slot::Time,
            words => Slot::Words(words),
        })
        .collect()
}

/// `Some(true)` if any of `found` is, else `None` if any is, else
/// `Some(false)`.
fn any_of(found: impl IntoIterator<Item = Option<bool>>) -> Option<bool> {
    let mut undecided = false;
    for one in found {
        match one {
            Some(true) => return Some(true),
            None => undecided = true,
            Some(false) => {}
        }
    }
    if undecided { None } else { Some(false) }
}

/// Whether `tokens[at..]` opens with `slots`, and the sentence they open
/// ends with no '?': `None` while more text could still decide it.
fn fits(slots: &[Slot<'_>], tokens: &[Token<'_>], at: usize, more: bool) -> Option<bool> {
    let ran_out = if more { None } else { Some(false) };
    let Some((slot, rest)) = slots.split_first() else {
        for token in &tokens[at..] {
            match token {
                Token::Mark('?') => return Some(false),
                Token::Mark('.' | '!') => return Some(true),
                _ => {}
            }
        }
        return if more { None } else { Some(true) };
    };
    match slot {
        Slot::Words(words) => match tokens.get(at) {
            None => ran_out,
            Some(Token::Word(word)) if words.split('|').any(|one| one == *word) => {
                fits(rest, tokens, at + 1, more)
            }
            Some(Token::Part(part)) if words.split('|').any(|one| one.starts_with(part)) => None,
            Some(_) => Some(false),
        },
        Slot::Any => any_of((0..=ANY_WORDS).map(|taken| {
            let taking = &tokens[at..tokens.len().min(at + taken)];
            if taking.iter().any(|token| matches!(token, Token::Mark(_))) {
                Some(false)
            } else if taking.iter().any(|token| matches!(token, Token::Part(_)))
                || taking.len() < taken
            {
                ran_out
            } else {
                fits(rest, tokens, at + taken, more)
            }
        })),
        Slot::End => match tokens.get(at) {
            None if more => None,
            None => Some(true),
            Some(Token::Mark('?') | Token::Word(_) | Token::Part(_)) => Some(false),
            Some(Token::Mark(_)) => Some(true),
        },
        Slot::Mark => match tokens.get(at) {
            None => ran_out,
            Some(Token::Mark('?') | Token::Word(_) | Token::Part(_)) => Some(false),
            Some(Token::Mark(_)) => fits(rest, tokens, at + 1, more),
        },
        Slot::Time => {
            let one = |word: &str| {
                word.starts_with(|c: char| c.is_ascii_digit())
                    || word == "tonight"
                    || NUMBER_WORDS.contains(&word)
            };
            let could = |part: &str| {
                part.starts_with(|c: char| c.is_ascii_digit())
                    || "tonight".starts_with(part)
                    || "the".starts_with(part)
                    || NUMBER_WORDS.iter().any(|word| word.starts_with(part))
            };
            match (tokens.get(at), tokens.get(at + 1)) {
                (None, _) => ran_out,
                (Some(Token::Word("the")), Some(Token::Word("night"))) => {
                    fits(rest, tokens, at + 2, more)
                }
                (Some(Token::Word("the")), Some(Token::Part(part)))
                    if "night".starts_with(part) =>
                {
                    None
                }
                (Some(Token::Word("the")), None) => ran_out,
                (Some(Token::Word(word)), _) if one(word) => fits(rest, tokens, at + 1, more),
                (Some(Token::Part(part)), _) if could(part) => None,
                _ => Some(false),
            }
        }
    }
}

/// Whether `text` (already [`normalised`]) claims an action in other words
/// ([`WORDED`]), as [`claim`] says it.
fn worded_claim(text: &str, ended: bool) -> Option<Option<Claim>> {
    let tokens = tokens(text, ended);
    let more = !ended;
    let mut undecided = false;
    for (pattern, kind) in WORDED {
        let run = slots(pattern);
        let bare = fits(&run, &tokens, 0, more);
        let led = LEAD_INS.iter().map(|lead| {
            let mut led = slots(lead);
            led.push(Slot::Mark);
            led.extend(run.iter().copied());
            fits(&led, &tokens, 0, more)
        });
        match any_of(std::iter::once(bare).chain(led)) {
            Some(true) => return Some(Some(kind)),
            None => undecided = true,
            Some(false) => {}
        }
    }
    if undecided { None } else { Some(None) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #5215: the lines are the executive's own, each once, so a line it
    /// says after a real call and one the model copies are the same words.
    #[test]
    fn the_said_lines_are_the_executives_own() {
        assert_eq!(SAID_LINES, lamp_actions::SAID_LINES);
        for line in ANY_CALL_LINES {
            assert!(SAID_LINES.contains(&line), "{line}");
        }
        assert_eq!(
            crate::second_provider::SHORT_DONE_TOGETHER,
            ANY_CALL_LINES[0]
        );
    }

    /// The Schedule's set lines are the second route's own
    /// (`second_provider::timer_line`).
    #[test]
    fn the_schedule_lines_are_the_timer_lines_own() {
        use crate::second_provider::net::ToolCall;
        let calls = [ToolCall {
            id: "c0".into(),
            name: "set_timer".into(),
            args: serde_json::json!({}),
            extra: None,
        }];
        let told = [serde_json::json!({"status": "committed", "clock": "09:39"})];
        for reminded in [false, true] {
            let line = crate::second_provider::timer_line(&calls, &told, reminded).unwrap();
            let kind = if reminded {
                Claim::ReminderTime
            } else {
                Claim::Schedule
            };
            assert_eq!(claim(&line, true), Some(Some(kind)), "{line}");
            assert!(
                line.starts_with(TIMER_SET) || line.starts_with(REMIND_AT),
                "{line}"
            );
        }
    }

    #[test]
    fn a_reply_that_is_or_opens_with_an_own_line_is_a_claim() {
        for (reply, kind) in [
            ("Louder.", Claim::LightOrVolume),
            ("louder", Claim::LightOrVolume),
            ("  Brighter! ", Claim::LightOrVolume),
            ("Brighter. Better?", Claim::LightOrVolume),
            ("Cooler, as you like.", Claim::LightOrVolume),
            ("I\u{2019}m already as loud as I go.", Claim::LightOrVolume),
            ("Done.", Claim::AnyCall),
            ("Done \u{2014} it's red.", Claim::AnyCall),
            ("Louder - there.", Claim::LightOrVolume),
            ("It already is.", Claim::AnyCall),
            ("Timer set. It rings at 9:39 am.", Claim::Schedule),
            ("Timer set.", Claim::Schedule),
            ("I'll remind you at 12:03 pm.", Claim::ReminderTime),
            ("I'll remind you at 9:39 am", Claim::ReminderTime),
        ] {
            assert_eq!(claim(reply, true), Some(Some(kind)), "{reply:?}");
        }
    }

    #[test]
    fn anything_else_is_no_claim() {
        for reply in [
            "It's 9:39.",
            "What time is it?",
            "You have one timer, for tea, at 9:39 am.",
            "Louder music helps me focus.",
            "Brighter?",
            "Done?",
            "Doneness matters for steak.",
            "Timer settings are in the app.",
            "My light is blue.",
            "My light is blue now.",
            "My light is warm white now.",
            "My light is blue now?",
            "I'll remind you at nine.",
            "I'll remind you at 9:39 tomorrow.",
            "It rings at 9:39 am.",
            "Okay, louder.",
            "",
        ] {
            assert_eq!(claim(reply, true), Some(None), "{reply:?}");
        }
    }

    /// Streamed, a claim is undecided until its words and the mark after
    /// them are in, and decided no claim at the first that fits none.
    #[test]
    fn a_reply_so_far_is_judged_as_it_comes() {
        for reply in [
            "",
            "Lou",
            "Louder",
            "Timer",
            "Timer set",
            "I'll remind you at 9:3",
        ] {
            assert_eq!(claim(reply, false), None, "{reply:?}");
        }
        assert_eq!(claim("Louder.", false), Some(Some(Claim::LightOrVolume)));
        assert_eq!(claim("Timer set.", false), Some(Some(Claim::Schedule)));
        assert_eq!(
            claim("I'll remind you at 9:39 am.", false),
            Some(Some(Claim::ReminderTime))
        );
        for reply in ["Lu", "Louder m", "The", "It's", "I'll remind you at n"] {
            assert_eq!(claim(reply, false), Some(None), "{reply:?}");
        }
    }

    #[test]
    fn a_claim_stands_beside_a_call_of_its_kind() {
        assert!(Claim::LightOrVolume.made_by("light_brighter"));
        assert!(Claim::LightOrVolume.made_by("volume_up"));
        assert!(!Claim::LightOrVolume.made_by("set_timer"));
        assert!(!Claim::LightOrVolume.made_by("volume_to"));
        assert!(Claim::Schedule.made_by("set_timer"));
        assert!(Claim::Schedule.made_by("repeat_reminder"));
        assert!(!Claim::Schedule.made_by("list_reminders"));
        assert!(Claim::ReminderTime.made_by("list_reminders"));
        assert!(Claim::ReminderTime.made_by("set_reminder"));
        assert!(!Claim::ReminderTime.made_by("light_brighter"));
        assert!(!Claim::Schedule.made_by("light_brighter"));
        assert!(Claim::AnyCall.made_by("set_timer"));
    }
    /// #5215's open shape: claims the model words its own way are claims
    /// only with the worded switch; without it, [`claim`] exactly.
    #[test]
    fn a_reply_in_other_words_is_a_claim_only_when_worded() {
        for (reply, kind) in [
            (
                "I've set a timer for one minute. It rings at 9:39 am.",
                Claim::Schedule,
            ),
            (
                "Timer set for one minute, it rings at 9:39 am.",
                Claim::Schedule,
            ),
            ("Okay, your one-minute timer is set.", Claim::ReminderTime),
            ("Reminder set for 6 pm.", Claim::Schedule),
            ("I'll remind you in ten minutes.", Claim::Schedule),
            ("Sure. I've added a reminder for the oven.", Claim::Schedule),
            ("Okay, louder.", Claim::LightOrVolume),
            ("Sure \u{2014} brighter.", Claim::LightOrVolume),
            ("Brighter now.", Claim::LightOrVolume),
            ("I've turned the volume up.", Claim::LightOrVolume),
            ("I turned off the light.", Claim::LightOrVolume),
            ("Turned it down.", Claim::LightOrVolume),
            ("I've made it warmer.", Claim::LightOrVolume),
            ("Lights off.", Claim::LightOrVolume),
            ("I\u{2019}ve kept the light on until ten.", Claim::LightKept),
            ("The light stays on until 10 tonight.", Claim::LightKept),
            ("I'll keep my light on until ten.", Claim::LightKept),
        ] {
            assert_eq!(claim_as(reply, true, true), Some(Some(kind)), "{reply:?}");
            assert_eq!(
                claim_as(reply, true, false),
                claim(reply, true),
                "{reply:?}"
            );
            assert_eq!(claim(reply, true), Some(None), "{reply:?}");
        }
        // The acting part's own lines stay claims of their own kind.
        assert_eq!(
            claim_as("Timer set. It rings at 9:39 am.", true, true),
            Some(Some(Claim::Schedule))
        );
        assert_eq!(
            claim_as("Louder.", true, true),
            Some(Some(Claim::LightOrVolume))
        );
    }

    /// #5215's rest: a claim said as it is done ("Turning it up.") is a
    /// worded claim too, and talk that opens with the same words is not.
    #[test]
    fn a_claim_said_as_it_is_done_is_a_worded_claim() {
        for (reply, kind) in [
            ("Turning it up.", Claim::LightOrVolume),
            ("Okay, turning the light off.", Claim::LightOrVolume),
            ("Turning off the light.", Claim::LightOrVolume),
            ("Dimming the light.", Claim::LightOrVolume),
            ("Raising the volume.", Claim::LightOrVolume),
            ("Making it warmer.", Claim::LightOrVolume),
            ("I'm turning it down.", Claim::LightOrVolume),
            ("Sure, I'm turning up the volume.", Claim::LightOrVolume),
            ("Setting a timer for one minute.", Claim::Schedule),
            ("I'm setting a reminder for 6 pm.", Claim::Schedule),
            ("Starting a timer now.", Claim::Schedule),
            ("Setting a timer for five minutes.", Claim::Schedule),
            ("Okay, setting an alarm for 6 pm.", Claim::Schedule),
            ("Setting a timer for half an hour.", Claim::Schedule),
            ("Setting a reminder for tonight.", Claim::Schedule),
            ("Setting a timer.", Claim::Schedule),
            ("Lowering the volume.", Claim::LightOrVolume),
        ] {
            assert_eq!(claim_as(reply, true, true), Some(Some(kind)), "{reply:?}");
            assert_eq!(claim(reply, true), Some(None), "{reply:?}");
        }
        for reply in [
            "Turning it off and on again usually helps.",
            "Setting a timer needs a time.",
            "Turning to the weather, it's sunny.",
            "Setting up takes a minute.",
            "I'm turning in for the night.",
            "Making it to ten is hard.",
            "Turning it up?",
            "Making it warmer would help?",
            // The first builder's HOLD on #5628: refusals, advice and talk.
            "Starting a reminder now isn't possible.",
            "Setting a reminder for tomorrow might help.",
            "Setting an alarm for six is a good idea.",
            "Starting a timer for pasta is the easy part.",
            "Setting a timer for ten minutes works well for tea.",
            "Setting a timer now would be smart.",
            "Setting the alarm for six means an early night.",
            // The arm, not the light (its P3).
            "Raising the lamp.",
            "Lowering the lamp.",
        ] {
            assert_eq!(claim_as(reply, true, true), Some(None), "{reply:?}");
        }
    }

    /// Ordinary talk with the same words, questions and status answers are
    /// no claims, worded or not.
    #[test]
    fn talk_with_the_same_words_is_no_worded_claim() {
        for reply in [
            "Timer settings are in the app.",
            "Is your timer set?",
            "Did I set a timer?",
            "I've set my heart on tea.",
            "I set out early today.",
            "Okay, louder music it is.",
            "Louder?",
            "Okay, brighter?",
            "I can keep my light on until ten, with a reminder.",
            "I've turned into a night owl.",
            "Your timer has been cancelled.",
            "Your timer is set, isn't it?",
            "My light is blue now.",
            "The light is warm white.",
            "I'll remind you at nine.",
            "You have one timer, for tea, at 9:39 am.",
            "Sure, what should the light be set to?",
            "Done with that?",
            "Sounds good.",
            "It's 9:39.",
            "",
        ] {
            assert_eq!(claim_as(reply, true, true), Some(None), "{reply:?}");
        }
    }

    /// Streamed, a worded claim is undecided while its words could still
    /// make one, and decided no claim at the first word that fits none.
    #[test]
    fn a_reply_in_other_words_is_judged_as_it_comes() {
        for reply in [
            "I",
            "I've",
            "I've set a",
            "I've set a timer for one",
            "Okay,",
            "Timer set for one",
            "Okay, louder",
            "The light stays on",
        ] {
            assert_eq!(claim_as(reply, false, true), None, "{reply:?}");
        }
        for reply in ["It's", "I think", "Okay, what", "Sure, here", "Sounds"] {
            assert_eq!(claim_as(reply, false, true), Some(None), "{reply:?}");
        }
        assert_eq!(
            claim_as("I've set a timer for one minute.", false, true),
            Some(Some(Claim::Schedule))
        );
    }

    /// The first builder's P2 on #5588: a line the person asked as a
    /// question ends with a '?' or opens with a question word, after the
    /// lamp's name or a greeting.
    #[test]
    fn a_line_asked_as_a_question() {
        for said in [
            "Lamp, how long will the light stay on?",
            "Lamp how long will the light stay on",
            "Hey Lamp, is my timer still set",
            "Is my timer still set?",
            "Are the lights staying on",
            "Did you set my timer",
            "What time does my reminder ring",
            "When will you remind me",
            "Will the light stay on",
            "Has my timer gone off",
            "Have you kept the light on",
            "Was that you",
            "Do you remember my timer",
            "Does the light stay on",
            "Okay, the light stays on until ten?",
            "How long will the light stay on?",
            "Can the light stay on until ten?",
            // The first builder's P3 on #5604: a contraction or another
            // question word opens a question with its '?' lost.
            "Lamp, what's left on my timer",
            "What\u{2019}s left on my timer",
            "How's my timer doing",
            "Where's my reminder",
            "Where is my timer at",
            "Who set the timer",
            "Who's keeping the light on",
            "Why is the light still on",
            "Which timer is set",
        ] {
            assert!(asked_as_a_question(said), "{said:?}");
        }
        for said in [
            "Hey Lamp, keep the light on until ten.",
            "Lamp, set a timer to one minute.",
            "Louder.",
            "Turn off the lights",
            "Lamp, howl like a wolf.",
            "",
            // A request asked as a question is still a request.
            "Lamp, can you set a timer for one minute?",
            "Lamp, could you keep the light on until ten?",
            "Will you turn it up?",
            "Hey Lamp, would you dim the light",
            "Can I have a timer for ten minutes?",
            "Please, is it possible to set a timer?",
            "Is it okay to set a timer, please?",
            "CAN YOU MAKE IT LOUDER?",
            // El's P2 on #5615: "why" opens a status question, but these
            // are requests.
            "Why don't you turn the light on",
            "Why not set a timer?",
            "Lamp, why don’t you make it louder?",
            // Opens as a request, so judged as one; a "Yes, I can hear
            // you." beside it claims no action either way.
            "Can you hear me",
        ] {
            assert!(!asked_as_a_question(said), "{said:?}");
        }
    }

    /// The first builder's P3 on #5588: a light kept "for" a time needs the
    /// time after "for" (digits, a number word, "the night" or "tonight");
    /// "I left a light on for you." is a persona line, no claim.
    #[test]
    fn a_light_kept_for_needs_a_time_after_for() {
        for reply in [
            "I left a light on for you.",
            "I've left the light on for your return.",
            "I'll keep my light on for whoever comes home.",
            "The light stays on for the cat.",
            "I'm leaving the lamp on for them, then.",
        ] {
            assert_eq!(claim_as(reply, true, true), Some(None), "{reply:?}");
        }
        for reply in [
            "I've left the light on for 10 minutes.",
            "I'll keep my light on for ten minutes.",
            "I've kept the light on for the night.",
            "The light stays on for tonight.",
            "I'll leave the lamp on for 2 hours.",
            "I'll keep the light on for half an hour.",
            "I've kept the light on till ten.",
        ] {
            assert_eq!(
                claim_as(reply, true, true),
                Some(Some(Claim::LightKept)),
                "{reply:?}"
            );
        }
        // Streamed: undecided while the word after "for" could be a time.
        for reply in [
            "I left a light on for",
            "I left a light on for t",
            "I left a light on for the",
        ] {
            assert_eq!(claim_as(reply, false, true), None, "{reply:?}");
        }
        assert_eq!(
            claim_as("I left a light on for you", false, true),
            Some(None)
        );
    }

    #[test]
    fn the_light_kept_stands_beside_a_light_call_a_set_or_a_kept_rule() {
        for name in [
            "light_on",
            "set_timer",
            "set_reminder",
            crate::rules::KEEP_RULE,
        ] {
            assert!(Claim::LightKept.made_by(name), "{name}");
        }
        for name in ["list_reminders", "forget_rule", "web_search"] {
            assert!(!Claim::LightKept.made_by(name), "{name}");
        }
    }
}
