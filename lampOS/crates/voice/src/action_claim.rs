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
//! ([`Claim::LightKept`]; "for" only with a time after it). GO-159
//! (lamp-52e6, 11091) found the kept light's claim still said 7 of 8 times
//! in shapes the runs missed, so it is also the light said as "it" ("I'll
//! keep it on until ten.", "I'll leave it on until ten."), "keeping" said
//! bare and closing at its time ("Got it, keeping the light on until
//! ten."), and the state told as a reply with a time after "until", "till"
//! or "for" ("The light is on until ten.", "Your light will be on until 10
//! pm.", "The light's staying on until ten.", "It'll stay on until ten."):
//! "It stays on until you say otherwise.", "Keeping it on until ten would
//! waste power." and "It's on until ten." (whose "it" may be anything) are
//! none. For bare keeping/leaving, clock and decimal punctuation stays
//! inside the time; a comma is not a claim's end. Advice after the time
//! and a comma-led question are none. A trailing numeric period waits for
//! lookahead or model completion because it may still begin a decimal;
//! other word runs keep their existing streaming decisions. A colour said
//! ("My light is blue now.") is still no claim, nor is any sentence the run
//! opens that ends in a '?'. Beside any call a worded claim stands, so work
//! another tool did stands ("I've added a reminder to your calendar."
//! beside `service_write`), unless it says a limit: a clock after "until",
//! "till" or "at", or a length after "for" or "in" ([`worded_limit`]). A
//! claimed time the call can't hold counts as unbacked (the first builder's
//! ruling on #5215): on lamp-52e6 "keep the light on until ten." got a
//! `keep_rule`, which holds always, and "I'll keep my light on until ten.".
//! Such a claim stands only beside a call of its kind that carried the
//! limit (a `seconds`, or a `time`, `from` or `to`; for a timer or reminder
//! claim, a clock beside `set_timer` or `snooze_reminder`, whose ring is a
//! length from now, never a kept light's; a reminder's time beside
//! `list_reminders` only on a line worded as a question,
//! [`worded_as_a_question`]; [`stands`]); held beside other calls,
//! the next round is told [`LIMIT_NOTE`]. A kept light's claim
//! ([`Claim::LightKept`]), whose every run says a limit, is never carried:
//! no call keeps the light until a time, so it is held beside any call,
//! whatever follows its hour ("until ten and then dim."). A known gap: "can
//! you", "could you" or "would you remind me at ten" and "remind me at
//! ten?" are worded as questions, so a set claim beside only
//! `list_reminders` stands. Only the reply's first limit is
//! judged, and while the reply streams only one said by its first
//! sentence's end: a later one is not checked. The worded runs never judge a
//! line the person asked as a question ([`asked_as_a_question`]), whose
//! reply may tell a true state ("The light stays on until ten."); the
//! strict lines judge every turn (the first builder's read of #5588).

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

/// What the next round is told when a worded claim said a limit no call of
/// the turn carried ([`worded_limit`], the first builder's ruling on
/// #5215): the calls that ran did run, and are not to be made again.
pub const LIMIT_NOTE: &str = "(For you to know: your last reply said something would hold until a time or for a length of time, but no function called in this turn carries that time, so it will not, and those words were not said. The functions that did run: do not call them again. If a function can do it at that time, call it now; the lamp says the result itself. If not, say plainly what was done, without that time.)";

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

/// Whether `kind`, which `reply` claims as [`claim_as`] found it, stands
/// beside the turn's `calls`, each a name and its arguments. A claim in the
/// acting part's own lines needs a call of its kind; a worded one any call,
/// and with a limit said ([`worded_limit`]) a call of its kind that carried
/// that limit (the first builder's ruling on #5215): "I'll keep my light on
/// until ten." beside a `keep_rule`, which holds always, does not stand,
/// nor does any kept light's claim beside any call ([`Claim::LightKept`]).
/// `read`: the person's line asks to read something back
/// ([`worded_as_a_question`]), so a `list_reminders` may back a time said.
pub fn stands<'a>(
    kind: Claim,
    reply: &str,
    read: bool,
    calls: impl IntoIterator<Item = (&'a str, &'a serde_json::Value)>,
) -> bool {
    let mut calls = calls.into_iter().peekable();
    if matches!(claim(reply, true), Some(Some(_))) {
        return calls.any(|(name, _)| kind.made_by(name));
    }
    // A kept light's every run says a limit ("until", "till", "for" and a
    // time), and no call keeps the light until a time: a light call or a
    // kept rule carries none, and a Schedule call keeps no light. Held
    // beside any call, whatever follows the hour ("until ten and then dim.";
    // the Reviewer-lead's P2s on #5737 at 2dffedb30b).
    if kind == Claim::LightKept {
        return false;
    }
    match worded_limit(reply) {
        None => calls.peek().is_some(),
        Some(limit) => {
            calls.any(|(name, args)| kind.made_by(name) && limit.carried_by(kind, read, name, args))
        }
    }
}

/// A limit a worded claim says ([`worded_limit`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Limit {
    /// A clock after "until", "till" or "at": its hour on a 12-hour face
    /// (twelve is 0), its minute, and whether pm was said (`None`: neither
    /// am nor pm, so either holds it).
    Clock {
        hour: u32,
        minute: u32,
        pm: Option<bool>,
    },
    /// A length after "for" or "in", in seconds.
    Length(u64),
}

impl Limit {
    /// Whether a call of `name` with `args` carries this limit, claimed as
    /// `kind`: `seconds` of the same length (`set_timer`,
    /// `snooze_reminder`), or a `time`, `from` or `to` ("HH:MM") on the same
    /// clock (`set_reminder`, `repeat_reminder`). A timer's or a snooze's
    /// ring is a length from now, so a clock said beside one stands ("Your
    /// timer will ring at 9:39."), but only for a Schedule claim: never a
    /// kept light's, which a timer beside its rule does not keep. A
    /// `list_reminders` reads back what is set: it backs a reminder's time
    /// only on a line that asked to read one (`read`), never after "Remind
    /// me at ten." (the Reviewer-lead's two P2s on #5737). A light call or a
    /// kept rule (`keep_rule` holds always) carries none, and a kept light's
    /// claim is never carried ([`stands`]).
    fn carried_by(self, kind: Claim, read: bool, name: &str, args: &serde_json::Value) -> bool {
        let schedule = matches!(kind, Claim::Schedule | Claim::ReminderTime);
        match (self, name) {
            (_, "list_reminders") => kind == Claim::ReminderTime && read,
            (Self::Length(seconds), _) => args["seconds"].as_u64() == Some(seconds),
            (Self::Clock { .. }, "set_timer" | "snooze_reminder") => schedule,
            (Self::Clock { hour, minute, pm }, _) => {
                ["time", "from", "to"].into_iter().any(|key| {
                    args[key]
                        .as_str()
                        .and_then(|time| time.split_once(':'))
                        .and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m.parse::<u32>().ok()?)))
                        .is_some_and(|(h, m)| {
                            h % 12 == hour && m == minute && pm.is_none_or(|pm| pm == (h >= 12))
                        })
                })
            }
        }
    }
}

/// The limit a worded claim says: the first clock after "until", "till",
/// "at" or "for" ("ten", "10:30 pm", "10.30", "ten thirty-five", "a quarter
/// past ten", "ten to ten", "noon"), or length after "for" or "in" ("one
/// minute", "5 mins", "twenty-five minutes", "an hour and a half", "one
/// hour and thirty minutes"), in the whole reply. Only the first: a second
/// limit in the reply is not judged. A bare hour is a clock only before the
/// end, a mark, "am", "pm", "o'clock" or a day word ("until ten.", "for ten
/// tonight"), so "at 20", "at 10 brightness", "at ten percent" and "at one
/// point" are none. None for a claim in the acting part's own lines, whose
/// times are a call's own result ("Timer set. It rings at 9:39 am."), and
/// none for words read as neither ("until you're back"), which keep the
/// worded runs' rule.
pub fn worded_limit(reply: &str) -> Option<Limit> {
    if matches!(claim(reply, true), Some(Some(_))) {
        return None;
    }
    let text = normalised(reply);
    let tokens = tokens(&text, true);
    (0..tokens.len()).find_map(|at| match tokens[at] {
        Token::Word("until" | "till" | "at") => said_clock(&tokens[at + 1..]),
        Token::Word("for") => {
            said_length(&tokens[at + 1..]).or_else(|| said_clock(&tokens[at + 1..]))
        }
        Token::Word("in") => said_length(&tokens[at + 1..]),
        _ => None,
    })
}

/// The number words up to nineteen, then the tens.
const ONES: [&str; 19] = [
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const TENS: [&str; 8] = [
    "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

/// A number said as digits or as one word ("ten", "forty-five").
fn said_number(word: &str) -> Option<u32> {
    let one = |word: &str| {
        ONES.iter()
            .position(|one| *one == word)
            .map(|at| at as u32 + 1)
    };
    let ten = |word: &str| {
        TENS.iter()
            .position(|ten| *ten == word)
            .map(|at| at as u32 * 10 + 20)
    };
    if let Some((tens, ones)) = word.split_once('-') {
        return Some(ten(tens)? + one(ones).filter(|ones| *ones < 10)?);
    }
    word.parse()
        .ok()
        .or_else(|| one(word))
        .or_else(|| ten(word))
}

/// The number `words` open with, and how many words it took: one word, or
/// a ten and a one apart ("twenty five").
fn said_count(words: &[Option<&str>]) -> Option<(u32, usize)> {
    let first = said_number(words.first().copied()??)?;
    let ones = words.get(1).copied().flatten().and_then(said_number);
    match ones {
        Some(ones) if TENS.iter().any(|ten| words[0] == Some(*ten)) && ones < 10 => {
            Some((first + ones, 2))
        }
        _ => Some((first, 1)),
    }
}

/// The words of `after`, a mark as `None`.
fn words_of<'a>(after: &[Token<'a>]) -> Vec<Option<&'a str>> {
    after
        .iter()
        .map(|token| match token {
            Token::Word(word) | Token::Part(word) => Some(*word),
            Token::Mark(_, _) => None,
        })
        .collect()
}

/// `word` without an "am" or "pm" it ends in, and which ("10pm").
fn meridiem(word: &str) -> (&str, Option<bool>) {
    if let Some(number) = word.strip_suffix("am") {
        (number, Some(false))
    } else if let Some(number) = word.strip_suffix("pm") {
        (number, Some(true))
    } else {
        (word, None)
    }
}

/// The words after a bare hour that make it a clock ("ten tonight"), beside
/// the end, a mark, "am", "pm" and "o'clock".
const DAY_WORDS: [&str; 6] = ["tonight", "today", "tomorrow", "this", "in", "on"];

/// The clock `after` opens with ([`Limit::Clock`]).
fn said_clock(after: &[Token<'_>]) -> Option<Limit> {
    let words = words_of(after);
    let word = |at: usize| words.get(at).copied().flatten();
    // "A quarter past ten", "half past ten", "ten to ten", "five past nine".
    let lead = usize::from(word(0) == Some("a"));
    let part = match word(lead) {
        Some("quarter") => Some((15, 1)),
        Some("half") => Some((30, 1)),
        _ => said_count(words.get(lead..).unwrap_or(&[]))
            .filter(|(minutes, _)| (1..30).contains(minutes)),
    };
    if let Some((minutes, took)) = part {
        let mut at = lead + took;
        if matches!(word(at), Some("minute" | "minutes")) {
            at += 1;
        }
        if let Some(way @ ("past" | "to")) = word(at) {
            let Limit::Clock {
                hour,
                minute: 0,
                pm,
            } = said_clock(&after[at + 1..])?
            else {
                return None;
            };
            return match way {
                "past" => Some(Limit::Clock {
                    hour,
                    minute: minutes,
                    pm,
                }),
                _ if minutes == 30 => None,
                // Back across twelve, am and pm swap: "a quarter to
                // midnight" is 11:45 pm.
                _ => Some(Limit::Clock {
                    hour: (hour + 11) % 12,
                    minute: 60 - minutes,
                    pm: if hour == 0 { pm.map(|pm| !pm) } else { pm },
                }),
            };
        }
    }
    let (hour, mut pm) = match word(0)? {
        "noon" => (0, Some(true)),
        "midnight" => (0, Some(false)),
        said => {
            let (number, pm) = meridiem(said);
            (said_number(number).filter(|hour| *hour <= 23)?, pm)
        }
    };
    let two_digits = |at: usize| {
        word(at)
            .map(meridiem)
            .filter(|(number, _)| number.len() == 2)
            .and_then(|(number, pm)| Some((number.parse::<u32>().ok().filter(|m| *m < 60)?, pm)))
    };
    let (mut minute, mut next) = (0, 1);
    let mut timed = pm.is_some();
    if pm.is_none() && matches!(after.get(1), Some(Token::Mark(':', _))) {
        (minute, pm) = two_digits(2)?;
        (next, timed) = (3, true);
    } else if pm.is_none()
        && matches!(after.get(1), Some(Token::Mark('.', _)))
        && let Some(said) = two_digits(2)
    {
        // "10.30".
        (minute, pm) = said;
        (next, timed) = (3, true);
    } else if pm.is_none()
        && let Some((said, took)) =
            said_count(words.get(1..).unwrap_or(&[])).filter(|(said, _)| (10..60).contains(said))
    {
        // "Ten thirty-five", "ten fifteen".
        (minute, next, timed) = (said, 1 + took, true);
    }
    if pm.is_none() {
        pm = match word(next) {
            Some("am") => Some(false),
            Some("pm") => Some(true),
            _ => (hour > 12).then_some(true),
        };
    }
    // A bare hour: a clock only before the end, a mark that is not a '%',
    // "am", "pm", "o'clock" or a day word, and never past twelve.
    let bare = match after.get(next) {
        None => true,
        Some(Token::Mark(mark, _)) => *mark != '%',
        Some(Token::Word(word) | Token::Part(word)) => {
            matches!(*word, "am" | "pm" | "o'clock") || DAY_WORDS.contains(word)
        }
    };
    if !(timed || (bare && hour <= 12)) {
        return None;
    }
    Some(Limit::Clock {
        hour: hour % 12,
        minute,
        pm,
    })
}

/// The length `after` opens with ([`Limit::Length`]): a count and its unit,
/// "and a half" before or after the unit, and "and" another such length
/// ("one hour and thirty minutes").
fn said_length(after: &[Token<'_>]) -> Option<Limit> {
    let words = words_of(after);
    let word = |at: usize| words.get(at).copied().flatten();
    if word(0)? == "half" && matches!(word(1), Some("a" | "an")) && word(2) == Some("hour") {
        return Some(Limit::Length(1800));
    }
    let (count, mut next) = match word(0)? {
        "a" | "an" => (1, 1),
        _ => said_count(&words)?,
    };
    // "One and a half hours".
    let and_a_half = |at: usize| {
        word(at) == Some("and") && word(at + 1) == Some("a") && word(at + 2) == Some("half")
    };
    let mut half = and_a_half(next);
    if half {
        next += 3;
    }
    let unit = match word(next)? {
        "second" | "seconds" | "sec" | "secs" => 1,
        "minute" | "minutes" | "min" | "mins" => 60,
        "hour" | "hours" | "hr" | "hrs" => 3600,
        _ => return None,
    };
    // "An hour and a half".
    half |= and_a_half(next + 1);
    let mut seconds = u64::from(count) * unit + if half { unit / 2 } else { 0 };
    // "One hour and thirty minutes".
    if !half
        && word(next + 1) == Some("and")
        && let Some(Limit::Length(more)) = said_length(&after[(next + 2).min(after.len())..])
    {
        seconds += more;
    }
    Some(Limit::Length(seconds))
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
const REQUEST_OPENINGS: [&[&str]; 8] = [
    &["can", "you"],
    &["could", "you"],
    &["will", "you"],
    &["would", "you"],
    &["can", "i", "have"],
    &["please"],
    // El's P2 on #5615: "why" opens a status question, but not these.
    &["why", "don't", "you"],
    &["why", "not"],
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
    let (request, question, _) = read_as_asked(said);
    !request && question
}

/// Whether the person's `said` asks to read something back: worded as a
/// question, a request asked as one included ("Lamp, can you tell me when
/// my timer rings?"). Its turn's `list_reminders` may back a time said
/// ([`stands`]); "Remind me at ten." asks for a set, not a read.
pub fn worded_as_a_question(said: &str) -> bool {
    let (_, question, could) = read_as_asked(said);
    question || could
}

/// `said` as [`asked_as_a_question`] reads it: whether it is a request,
/// whether it is worded as a question (a '?' at its end, or its first word
/// after any [`ADDRESS`] one of [`QUESTION_WORDS`]), and whether that word
/// is "could" or "would".
fn read_as_asked(said: &str) -> (bool, bool, bool) {
    let text = normalised(said);
    let words: Vec<&str> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .collect();
    let own = words
        .iter()
        .position(|word| !ADDRESS.contains(word))
        .map_or(&[][..], |at| &words[at..]);
    let request = words.contains(&"please")
        || REQUEST_OPENINGS
            .iter()
            .any(|opening| own.starts_with(opening));
    let question = text
        .trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ')'))
        .ends_with('?')
        || own
            .first()
            .is_some_and(|word| QUESTION_WORDS.contains(word));
    (
        request,
        question,
        matches!(own.first(), Some(&"could" | &"would")),
    )
}

/// The worded claims (`LAMP_ACTION_CLAIM_WORDED`, #5215): a reply that opens
/// with one of these, bare or after one of [`LEAD_INS`] and a mark, is a
/// claim of its kind. Each is a run of words: `a|b` is either word, `*` up
/// to [`ANY_WORDS`] words, `#` a time ([`Slot::Time`]), and `$` the end or
/// a mark that is not a '?'.
/// After the run, the sentence it opens must not end in a '?'. Each run is
/// tied to a family with a call behind it, and never a colour said ("My
/// light is blue now." also answers a colour question).
const WORDED: [(&str, Claim); 65] = [
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
        "i've|i kept|left * light|lights|lamp|it on|off until|till",
        Claim::LightKept,
    ),
    (
        "i've|i kept|left * light|lights|lamp|it on|off for #",
        Claim::LightKept,
    ),
    (
        "i'll|i'm keep|keeping|leave|leaving * light|lights|lamp|it on|off until|till",
        Claim::LightKept,
    ),
    (
        "i'll|i'm keep|keeping|leave|leaving * light|lights|lamp|it on|off for #",
        Claim::LightKept,
    ),
    (
        "i will keep|leave * light|lights|lamp|it on|off until|till",
        Claim::LightKept,
    ),
    (
        "i will keep|leave * light|lights|lamp|it on|off for #",
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
    // GO-159 (lamp-52e6, 11091): "keeping" said bare ("Got it, keeping
    // the light on until ten."), closing at its time so talk about keeping
    // it on ("Keeping it on until ten would waste power.") is none.
    // These alone use a complete numeric time and its close: a clock's
    // colon, a decimal point, or a comma must not end the claim early.
    (
        "keeping|leaving * light|lights|lamp|it on|off until|till|for @ ~",
        Claim::LightKept,
    ),
    (
        "keeping|leaving * light|lights|lamp|it on|off until|till|for @ am|pm|tonight|o'clock|minute|minutes|hour|hours ~",
        Claim::LightKept,
    ),
    (
        "keeping|leaving * light|lights|lamp|it on|off for @ a|an minute|hour ~",
        Claim::LightKept,
    ),
    // The state told as a reply ("The light is on until ten.", "Your light
    // will be on until 10 pm.", "The light's staying on until ten."), and
    // the light said as "it" ("It'll stay on until ten."): each with a time
    // after "until", "till" or "for", so "It stays on until you say
    // otherwise." and "The light is on until you turn it off." are none.
    (
        "the|my|your light|lights|lamp is|are on|off until|till|for #",
        Claim::LightKept,
    ),
    (
        "the|my|your light|lights|lamp will be|remain on|off until|till|for #",
        Claim::LightKept,
    ),
    (
        "the|my|your light's|lamp's on|off until|till|for #",
        Claim::LightKept,
    ),
    (
        "the|my|your light's|lamp's staying on|off until|till|for #",
        Claim::LightKept,
    ),
    (
        "the|my|your light'll|lamp'll stay|be|remain on|off until|till|for #",
        Claim::LightKept,
    ),
    ("it stays|remains on|off until|till|for #", Claim::LightKept),
    (
        "it'll stay|remain on|off until|till|for #",
        Claim::LightKept,
    ),
    (
        "it will stay|remain on|off until|till|for #",
        Claim::LightKept,
    ),
    // Not "it's staying": a streamed "It's" ("It's 9:39.") would wait a
    // word longer to be judged.
    ("it is staying on|off until|till|for #", Claim::LightKept),
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
    /// A mark and whether it can join adjacent numeric components: a
    /// colon/period directly after a digit, then a digit or no text yet.
    /// Other matchers ignore this flag and keep their existing rules.
    Mark(char, bool),
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
            let numeric = matches!(c, ':' | '.')
                && at > 0
                && text.as_bytes()[at - 1].is_ascii_digit()
                && text
                    .as_bytes()
                    .get(at + c.len_utf8())
                    .is_none_or(u8::is_ascii_digit);
            out.push(Token::Mark(c, numeric));
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
    /// A bare kept time's sentence end, never its comma or clock colon.
    TimeEnd,
    /// A mark that is not a '?' (after a lead-in).
    Mark,
    /// A time: a word that opens with a digit, one of [`NUMBER_WORDS`],
    /// "tonight", "noon", "midnight", or "the night".
    Time,
    /// A time whose numeric clock/decimal punctuation stays inside it.
    ClockTime,
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
            "~" => Slot::TimeEnd,
            "@" => Slot::ClockTime,
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
                Token::Mark('?', _) => return Some(false),
                Token::Mark('.' | '!', _) => return Some(true),
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
            if taking
                .iter()
                .any(|token| matches!(token, Token::Mark(_, _)))
            {
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
            Some(Token::Mark('?', _) | Token::Word(_) | Token::Part(_)) => Some(false),
            Some(Token::Mark(_, _)) => Some(true),
        },
        Slot::TimeEnd => match tokens.get(at) {
            None if more => None,
            None | Some(Token::Mark('.' | '!', _)) => Some(true),
            Some(Token::Mark(',', _)) if more && at + 1 == tokens.len() => None,
            Some(_) => Some(false),
        },
        Slot::Mark => match tokens.get(at) {
            None => ran_out,
            Some(Token::Mark('?', _) | Token::Word(_) | Token::Part(_)) => Some(false),
            Some(Token::Mark(_, _)) => fits(rest, tokens, at + 1, more),
        },
        Slot::Time | Slot::ClockTime => {
            if matches!(slot, Slot::ClockTime)
                && let Some(Token::Word(word)) = tokens.get(at)
                && word.bytes().all(|byte| byte.is_ascii_digit())
            {
                return fits_numeric_time(rest, tokens, at + 1, more);
            }
            let one = |word: &str| {
                word.starts_with(|c: char| c.is_ascii_digit())
                    || matches!(word, "tonight" | "noon" | "midnight")
                    || NUMBER_WORDS.contains(&word)
            };
            let could = |part: &str| {
                part.starts_with(|c: char| c.is_ascii_digit())
                    || ["tonight", "noon", "midnight", "the"]
                        .iter()
                        .any(|word| word.starts_with(part))
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

/// Continue after a numeric time's first component. A trailing period
/// may still begin a decimal while streaming, so wait for lookahead or
/// model completion. The scan only advances; no new timer or I/O is used.
fn fits_numeric_time(
    rest: &[Slot<'_>],
    tokens: &[Token<'_>],
    mut after: usize,
    more: bool,
) -> Option<bool> {
    while let Some(Token::Mark(mark @ (':' | '.'), true)) = tokens.get(after) {
        match tokens.get(after + 1) {
            Some(Token::Word(part)) if part.starts_with(|c: char| c.is_ascii_digit()) => {
                after += 2;
                // Keep the existing Time convention for compact suffixes
                // such as 10:30pm or 1.5hours, then close at that token.
                if !part.bytes().all(|byte| byte.is_ascii_digit()) {
                    break;
                }
            }
            Some(Token::Part(part)) if part.starts_with(|c: char| c.is_ascii_digit()) => {
                return None;
            }
            None if more => return None,
            _ if *mark == '.' => break,
            _ => return Some(false),
        }
    }
    fits(rest, tokens, after, more)
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
pub(crate) mod tests {
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

    /// GO-159 (lamp-52e6, 11091): "keep the light on until ten" with the
    /// worded runs on still got its claim 7 of 8 times and the worded guard
    /// never fired, so the model's replies opened with shapes the runs
    /// missed: the light said as "it", "will stay" or "will be", "keeping"
    /// said bare, "light's", or the state told as a reply.
    pub(crate) const KEPT_SHAPES: [&str; 12] = [
        "I'll keep it on until ten.",
        "Okay, I'll keep it on till 10.",
        "Sure! The light will stay on until ten.",
        "Got it, keeping the light on until ten.",
        "Done, the light's staying on until ten.",
        "Alright, your light will be on until 10 pm.",
        "The light is on until ten.",
        "I'll leave it on until ten.",
        "Keeping it on until 10 pm.",
        "It'll stay on until ten.",
        "Okay, it will stay on until noon.",
        "Your light'll stay on for two hours.",
    ];

    /// Each of [`KEPT_SHAPES`] is a kept light's claim on the worded runs
    /// ("Done," is also the strict "Done.", judged first), so with no call
    /// it is held.
    #[test]
    fn a_kept_light_said_as_it_or_will_stay_is_a_worded_claim() {
        let mut missed = Vec::new();
        for reply in KEPT_SHAPES {
            let text = normalised(reply);
            if worded_claim(text.trim_end(), true) != Some(Some(Claim::LightKept)) {
                missed.push(reply);
            }
            assert_eq!(
                claim_as(reply, true, false),
                claim(reply, true),
                "{reply:?}"
            );
        }
        assert!(missed.is_empty(), "missed: {missed:?}");
        for reply in KEPT_SHAPES {
            let kind = claim_as(reply, true, true).flatten();
            assert!(kind.is_some(), "{reply:?}");
            assert!(!stands(kind.unwrap(), reply, false, []), "{reply:?}");
        }
    }

    /// The widened kept-light runs keep their controls: no limit said
    /// after an "it" or a state, talk about keeping it on, persona lines,
    /// questions and colours are no claims.
    #[test]
    fn a_kept_light_without_a_limit_or_as_talk_is_no_worded_claim() {
        for reply in [
            "I left a light on for you.",
            "I'll leave it on for you.",
            "Turning it off and on again usually helps.",
            "It stays on until you say otherwise.",
            "It'll stay on until you're back.",
            "The light is on until you turn it off.",
            "The light is on.",
            "Your light will be on when you get home.",
            "Keeping it on until ten would waste power.",
            "Leaving it on for 10 hours wastes power.",
            "Keeping the light on is easy.",
            "I'll keep it on until ten?",
            "The light will stay on until ten?",
            "It'll stay on until ten?",
            "My light is blue now.",
            "It's on until ten.",
        ] {
            assert_eq!(claim_as(reply, true, true), Some(None), "{reply:?}");
        }
    }

    /// A clock's colon or a decimal point is not the end of a claim;
    /// neither is a comma before a question or an explanation.
    #[test]
    fn bare_kept_time_advice_and_questions_never_claim_at_a_prefix() {
        for reply in [
            "Keeping it on until 10:30 would waste power.",
            "Keeping it on until ten, would that help?",
            "Keeping it on until 10:30, would that help?",
            "Keeping it on until 10:30?",
            "Okay, keeping it on until 10:30 would waste power.",
            "Leaving it on for 1.5 hours wastes power.",
            "Leaving it on for 1.5 hours, would that help?",
            "Keeping it on until ten, as a habit, wastes power.",
        ] {
            assert_eq!(claim_as(reply, true, true), Some(None), "{reply:?}");
            for end in reply.char_indices().map(|(at, _)| at).chain([reply.len()]) {
                let prefix = &reply[..end];
                assert!(
                    !matches!(claim_as(prefix, false, true), Some(Some(_))),
                    "premature claim at {prefix:?} in {reply:?}"
                );
                assert_eq!(claim_as(prefix, false, false), claim(prefix, false));
            }
        }
    }

    /// A real timed claim stays undecided while its numeric punctuation
    /// or suffix is incomplete, rather than releasing its words early.
    #[test]
    fn bare_kept_clock_and_decimal_claims_keep_their_streamed_prefixes() {
        for reply in [
            "Keeping it on until 10:30.",
            "Keeping it on until 10:30 pm.",
            "Keeping it on until 10:30pm.",
            "Got it, keeping the light on until 10:30.",
            "Leaving it on for 1.5 hours.",
            "Leaving it on for 1.5hours.",
            "Keeping it on until noon.",
            "Keeping it on for the night.",
            "Leaving it on for half an hour.",
            "Keeping it on until 10. 30 minutes should be enough.",
            "Keeping it on until 10:30. 5 minutes should be enough.",
        ] {
            assert_eq!(
                claim_as(reply, true, true),
                Some(Some(Claim::LightKept)),
                "{reply:?}"
            );
            for end in reply.char_indices().map(|(at, _)| at).chain([reply.len()]) {
                let prefix = &reply[..end];
                assert_ne!(claim_as(prefix, false, true), Some(None), "{prefix:?}");
                assert_eq!(claim_as(prefix, false, false), claim(prefix, false));
            }
        }
        for prefix in [
            "Keeping it on until 10:",
            "Keeping it on until 10:3",
            "Keeping it on until 10:30.",
            "Leaving it on for 1.",
            "Leaving it on for 1.5 h",
            "Keeping it on until ten,",
        ] {
            assert_eq!(claim_as(prefix, false, true), None, "{prefix:?}");
        }
        for prefix in ["Keeping it on until 10. ", "Keeping it on until 10:30. "] {
            assert_eq!(claim_as(prefix, false, true), Some(Some(Claim::LightKept)));
        }
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

    /// The first builder's ruling on #5215: a limit a worded claim says is
    /// read as a clock or a length, and stands only beside a call that
    /// carried it; a kept rule carries none.
    #[test]
    fn a_worded_claims_limit_stands_only_beside_a_call_that_carried_it() {
        let clock = |hour, minute, pm| Some(Limit::Clock { hour, minute, pm });
        for (reply, limit) in [
            ("I'll keep my light on until ten.", clock(10, 0, None)),
            (
                "I've kept the light on till 10:30 pm.",
                clock(10, 30, Some(true)),
            ),
            (
                "I'll keep the light on until 22:00.",
                clock(10, 0, Some(true)),
            ),
            ("I'll keep my light on until noon.", clock(0, 0, Some(true))),
            ("Timer set for one minute.", Some(Limit::Length(60))),
            (
                "I'll remind you in half an hour.",
                Some(Limit::Length(1800)),
            ),
            ("I left a light on for you until nine.", clock(9, 0, None)),
            ("I'll keep my light on.", None),
            ("I'll keep my light on until you're back.", None),
            ("Timer set. It rings at 9:39 am.", None),
        ] {
            assert_eq!(worded_limit(reply), limit, "{reply:?}");
        }
        let kept =
            serde_json::json!({"rule": "Keep the light on.", "about": "light", "never": true});
        let at = |time: &str| serde_json::json!({"text": "Lights", "time": time});
        let ten = "I'll keep my light on until ten.";
        assert!(!stands(
            Claim::LightKept,
            ten,
            false,
            [("keep_rule", &kept)]
        ));
        // A reminder at the same time keeps no light.
        assert!(!stands(
            Claim::LightKept,
            ten,
            false,
            [("set_reminder", &at("22:00"))]
        ));
        assert!(!stands(
            Claim::LightKept,
            ten,
            false,
            [("set_reminder", &at("10:00"))]
        ));
        assert!(!stands(
            Claim::LightKept,
            ten,
            false,
            [("set_reminder", &at("21:00"))]
        ));
        let minute = "Timer set for one minute.";
        let seconds = |seconds: u64| serde_json::json!({"text": "Tea", "seconds": seconds});
        assert!(stands(
            Claim::Schedule,
            minute,
            false,
            [("set_timer", &seconds(60))]
        ));
        assert!(!stands(
            Claim::Schedule,
            minute,
            false,
            [("set_timer", &seconds(300))]
        ));
        // No limit: any call, as before; the strict lines: a call of their kind.
        assert!(stands(
            Claim::Schedule,
            "Reminder set.",
            false,
            [("web_search", &kept)]
        ));
        assert!(!stands(Claim::LightKept, ten, false, []));
        assert!(stands(
            Claim::Schedule,
            "Timer set. It rings at 9:39 am.",
            false,
            [("set_timer", &seconds(1))]
        ));
    }

    /// The Reviewer-lead's P2 on #5737: a clock said beside a call whose
    /// time comes as a length from now (`set_timer`) or is read back
    /// (`list_reminders`) stands, as does one an interval's window carries.
    #[test]
    fn a_clock_said_stands_beside_a_call_that_carries_no_clock() {
        let timer = serde_json::json!({"text": "Tea", "seconds": 300});
        let listed = serde_json::json!({});
        let every = serde_json::json!({"text": "Water", "every": "interval", "hours": 2, "from": "09:00", "to": "17:00"});
        let mut held = Vec::new();
        for (reply, kind, name, args) in [
            (
                "Your tea timer is set and will ring at 9:39.",
                Claim::ReminderTime,
                "set_timer",
                &timer,
            ),
            (
                "Your timer is set to ring at 9:39 am.",
                Claim::ReminderTime,
                "list_reminders",
                &listed,
            ),
            (
                "I've set a reminder every two hours from nine until five.",
                Claim::Schedule,
                "repeat_reminder",
                &every,
            ),
        ] {
            assert_eq!(claim_as(reply, true, true), Some(Some(kind)), "{reply:?}");
            assert!(worded_limit(reply).is_some(), "{reply:?}");
            if !stands(kind, reply, true, [(name, args)]) {
                held.push(reply);
            }
        }
        assert!(held.is_empty(), "held: {held:?}");
    }

    /// The Reviewer-lead's P3s on #5737: a percent or "at one point" is no
    /// clock, and the clocks and lengths are read as said.
    #[test]
    fn the_limits_said_are_read_as_said() {
        let clock = |hour, minute, pm| Some(Limit::Clock { hour, minute, pm });
        let length = |seconds| Some(Limit::Length(seconds));
        for (reply, limit) in [
            ("I've set the light at ten percent.", None),
            ("I've set the volume at 20 percent.", None),
            ("I've set the volume at 20%.", None),
            ("I'll keep the light on, at one point.", None),
            ("Timer set for an hour and a half.", length(5400)),
            ("Timer set for one and a half hours.", length(5400)),
            ("Timer set for 5 mins.", length(300)),
            ("Timer set for twenty-five minutes.", length(1500)),
            ("Timer set for twenty five minutes.", length(1500)),
            (
                "I'll keep my light on until ten thirty-five.",
                clock(10, 35, None),
            ),
            (
                "I'll keep my light on until nine twenty five pm.",
                clock(9, 25, Some(true)),
            ),
            (
                "I'll keep my light on until a quarter past ten.",
                clock(10, 15, None),
            ),
            (
                "I'll keep my light on until half past ten.",
                clock(10, 30, None),
            ),
            (
                "I'll keep my light on until quarter to ten.",
                clock(9, 45, None),
            ),
            // At 09483c9afa (the Reviewer-lead's P3s).
            (
                "I'll keep my light on until ten to ten.",
                clock(9, 50, None),
            ),
            (
                "I'll keep my light on until five past nine.",
                clock(9, 5, None),
            ),
            (
                "I'll keep my light on until a quarter to midnight.",
                clock(11, 45, Some(true)),
            ),
            (
                "I'll keep my light on until quarter to noon.",
                clock(11, 45, Some(false)),
            ),
            ("I'll keep my light on until 10.30.", clock(10, 30, None)),
            (
                "I'll keep my light on until ten o'clock.",
                clock(10, 0, None),
            ),
            (
                "I'll keep my light on until ten tonight.",
                clock(10, 0, None),
            ),
            ("Timer set for one hour and thirty minutes.", length(5400)),
            ("Your reminder is set for ten.", clock(10, 0, None)),
            ("I've set the volume at 20.", None),
            ("I've set the light at 10 brightness.", None),
        ] {
            assert_eq!(worded_limit(reply), limit, "{reply:?}");
        }
    }

    /// The Reviewer-lead's P3 on #5737, kept as it is: only the first limit
    /// in a reply is judged, so a second one stands on the first's call.
    #[test]
    fn only_the_first_limit_said_is_judged() {
        let reply = "Timer set for five minutes, and I'll keep the light on until ten.";
        assert_eq!(worded_limit(reply), Some(Limit::Length(300)));
        let timer = serde_json::json!({"text": "Tea", "seconds": 300});
        let kept =
            serde_json::json!({"rule": "Keep the light on.", "about": "light", "never": true});
        assert!(stands(
            Claim::Schedule,
            reply,
            false,
            [("set_timer", &timer), ("keep_rule", &kept)]
        ));
    }

    /// The Reviewer-lead's P2 on #5737 at 09483c9afa: a timer's clock
    /// shortcut is a Schedule claim's only, and a listing backs a time only
    /// on a line asked as a read.
    #[test]
    fn a_timer_or_a_listing_backs_a_clock_only_for_its_own_claims() {
        let timer = serde_json::json!({"text": "Tea", "seconds": 300});
        let kept =
            serde_json::json!({"rule": "Keep the light on.", "about": "light", "never": true});
        let ten = "I'll keep my light on until ten.";
        assert!(!stands(
            Claim::LightKept,
            ten,
            false,
            [("keep_rule", &kept), ("set_timer", &timer)]
        ));
        let listed = serde_json::json!({});
        let set = "Your reminder is set for ten.";
        assert!(!stands(
            Claim::ReminderTime,
            set,
            false,
            [("list_reminders", &listed)]
        ));
        assert!(stands(
            Claim::ReminderTime,
            set,
            true,
            [("list_reminders", &listed)]
        ));
        assert!(worded_as_a_question(
            "Lamp, can you tell me when my timer rings?"
        ));
        assert!(worded_as_a_question("Could you check my reminders"));
        assert!(!worded_as_a_question("Lamp, remind me at ten."));
    }
}
