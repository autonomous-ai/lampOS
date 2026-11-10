//! A skill's reminder (#6066, the first builder's item 2): a `[[do]]` step
//! that sets one timer or reminder in the lamp's schedule, through the
//! actions part's own `set_timer` or `set_reminder` call, the same call the
//! voice makes when a person asks. `remind = { at = "22:00", words = "..." }`
//! sets one at the next 22:00; `remind = { in = "1h", words = "..." }` one an
//! hour from the run.
//!
//! - **The words are the skill's**, fixed when it loads, with the limits a
//!   made skill's say has (`lamp_voice::made_skill::say_text`): at most
//!   [`MOST_WORDS_CHARS`] characters, one or two sentences, no web address,
//!   no quotes, braces or backticks, and no control or format character.
//!   Never a model's words at run time.
//! - **Manners.** A skill sets at most [`MOST_A_DAY`] in any 24 hours
//!   ([`Tally`]), and never stacks one: words already set and not yet said
//!   (the schedule's own list) mean nothing new is set.
//! - **The read-back** is the schedule's: a set counts as set only when the
//!   actions part answers `committed`, with the local `clock` it is due.
//! - **Privacy.** The call names no caller, so it is the room's: said to
//!   whoever is at the lamp when it comes due, as one set by no one. It reads
//!   no memory and writes nothing beyond the schedule's entry.
//! - Only in a skill a trigger starts: a person who asks for a reminder asks
//!   the voice, which sets it and says its time back.
use crate::Permissions;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// The most a skill's reminders are set in any 24 hours.
pub const MOST_A_DAY: usize = 3;
/// The window [`MOST_A_DAY`] counts in.
pub const DAY_US: u64 = 86_400_000_000;
/// The longest reminder's words, as a made skill's say
/// (`lamp_voice::made_skill::MOST_SAY_TEXT_CHARS`).
pub const MOST_WORDS_CHARS: usize = 120;
/// The shortest `in`: a minute.
pub const LEAST_IN_SECS: u64 = 60;
/// The longest `in`: a day.
pub const MOST_IN_SECS: u64 = 86_400;
/// How long each call to the actions part may take, each way.
const ACTIONS_TIMEOUT: Duration = Duration::from_secs(2);
/// The most pages of the schedule's list read for the same words: 8 items
/// a page. Past them, nothing is set (a duplicate can't be ruled out).
const MOST_PAGES: usize = 4;
/// The longest answer read from the actions part, as a light's.
const MOST_ANSWER_BYTES: u64 = 8_193;

/// `remind = { ... }`, as the skill's folder writes it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemindFile {
    /// A local clock time, `"HH:MM"`: the next one from the run.
    at: Option<String>,
    /// A duration from the run, `"20m"`, 1 m to 24 h.
    #[serde(rename = "in")]
    after: Option<String>,
    /// What is said when it comes due: fixed text ([`plain_words`]).
    words: String,
}

/// When a reminder comes due.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum When {
    /// At the next local `HH:MM` (`set_reminder`).
    At(String),
    /// This many seconds from the run (`set_timer`).
    In(u64),
}

/// A remind step, as loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remind {
    pub when: When,
    pub words: String,
}

impl RemindFile {
    /// The step, or why it is refused.
    pub fn step(self) -> Result<Remind, String> {
        let words = plain_words(&self.words).ok_or_else(|| {
            format!(
                "remind.words: its exact words, at most {MOST_WORDS_CHARS} characters in one or two \
                 sentences, with no web address, quotes, braces, backticks or control characters"
            )
        })?;
        let when = match (self.at, self.after) {
            (Some(at), None) => {
                if !clock_time(&at) {
                    return Err(format!(
                        "remind.at {at:?} is not a time of day: HH:MM, from 00:00 to 23:59"
                    ));
                }
                When::At(at)
            }
            (None, Some(after)) => {
                let us = crate::duration_us(&after).map_err(|why| format!("remind.in: {why}"))?;
                let seconds = us / 1_000_000;
                if us % 1_000_000 != 0 || !(LEAST_IN_SECS..=MOST_IN_SECS).contains(&seconds) {
                    return Err(format!(
                        "remind.in {after:?}: whole seconds, from 1m to 24h"
                    ));
                }
                When::In(seconds)
            }
            _ => return Err("remind names one of at or in, not both".into()),
        };
        Ok(Remind { when, words })
    }
}

impl Remind {
    /// The actions part's function that sets it, which `[permissions]
    /// actions` must list.
    pub fn action(&self) -> &'static str {
        match self.when {
            When::At(_) => "set_reminder",
            When::In(_) => "set_timer",
        }
    }

    /// Its arguments to [`Self::action`], as the actions part's catalogue
    /// takes them.
    pub fn args(&self) -> Value {
        match &self.when {
            When::At(time) => json!({ "text": self.words, "time": time }),
            When::In(seconds) => json!({ "text": self.words, "seconds": seconds }),
        }
    }

    /// The step, as a test or the journal names it.
    pub fn named(&self) -> Value {
        match &self.when {
            When::At(time) => json!({ "at": time, "words": self.words }),
            When::In(seconds) => json!({ "in_s": seconds, "words": self.words }),
        }
    }

    /// When it comes due, in a few words, without its words.
    fn when_said(&self) -> String {
        match &self.when {
            When::At(time) => format!("remind at {time}"),
            When::In(seconds) => format!("remind in {seconds} s"),
        }
    }
}

/// `"22:00"`: two digits, a colon, two digits, 00:00 to 23:59.
fn clock_time(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 5
        && bytes[2] == b':'
        && [0, 1, 3, 4].iter().all(|&at| bytes[at].is_ascii_digit())
        && text[..2].parse::<u8>().is_ok_and(|hours| hours < 24)
        && text[3..].parse::<u8>().is_ok_and(|minutes| minutes < 60)
}

/// A Unicode format character (general category Cf), as a made skill's
/// say refuses one: zero-width marks, bidirectional controls and the like.
fn is_format(c: char) -> bool {
    matches!(
        c as u32,
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

/// A reminder's words, trimmed, if they are plain text a person hears as
/// written: the rules of a made skill's say
/// (`lamp_voice::made_skill::say_text`).
pub fn plain_words(text: &str) -> Option<String> {
    let text = text.trim();
    let lower = text.to_lowercase();
    let sentences = text
        .split(['.', '!', '?'])
        .filter(|part| !part.trim().is_empty())
        .count();
    let plain = !text.chars().any(|c| {
        c.is_control()
            || is_format(c)
            || matches!(
                c,
                '"' | '“' | '”' | '«' | '»' | '{' | '}' | '`' | '\u{2028}' | '\u{2029}'
            )
    });
    let web = ["://", "www.", "http"]
        .iter()
        .any(|mark| lower.contains(mark));
    (plain && !web && (1..=2).contains(&sentences) && text.chars().count() <= MOST_WORDS_CHARS)
        .then(|| text.to_owned())
}

/// The same words, as a person hears them: case and spacing aside, as the
/// actions part compares a set's words (`same_words`).
fn same_words(a: &str, b: &str) -> bool {
    let words = |text: &str| {
        text.split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
    };
    words(a) == words(b)
}

/// By skill, when each of its reminders was set within the last
/// [`DAY_US`]. Kept while the program runs, as `most_a_day` is.
#[derive(Debug, Default)]
pub struct Tally {
    sets: BTreeMap<String, VecDeque<u64>>,
}

impl Tally {
    pub const fn new() -> Self {
        Self {
            sets: BTreeMap::new(),
        }
    }

    /// How many `skill` set within the day before `now_us`.
    pub fn count(&mut self, skill: &str, now_us: u64) -> usize {
        let Some(sets) = self.sets.get_mut(skill) else {
            return 0;
        };
        while sets
            .front()
            .is_some_and(|at| now_us.saturating_sub(*at) >= DAY_US)
        {
            sets.pop_front();
        }
        sets.len()
    }

    fn mark(&mut self, skill: &str, now_us: u64) {
        self.sets
            .entry(skill.to_owned())
            .or_default()
            .push_back(now_us);
    }
}

/// One call on the actions part's socket: its answer, whole.
fn call(socket: &Path, name: &str, args: Value) -> io::Result<Value> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(ACTIONS_TIMEOUT))?;
    stream.set_write_timeout(Some(ACTIONS_TIMEOUT))?;
    writeln!(
        stream,
        "{}",
        json!({"op": "call", "name": name, "args": args})
    )?;
    let mut line = String::new();
    BufReader::new(stream.take(MOST_ANSWER_BYTES)).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// The actions part's refusal, its own fixed sentence.
fn refusal(answer: &Value) -> &str {
    answer["error"].as_str().unwrap_or("refused")
}

/// What is set and not yet said with these words, as the schedule lists it
/// for the room: its `clock`, if any is; or why that can't be known.
fn already_set(socket: &Path, words: &str) -> Result<Option<String>, String> {
    let mut after_id = 0;
    for _ in 0..MOST_PAGES {
        let answer = call(socket, "list_reminders", json!({ "after_id": after_id }))
            .map_err(|error| format!("the actions part was not reached ({error})"))?;
        if answer["ok"] != true {
            return Err(refusal(&answer).to_owned());
        }
        let page = &answer["result"];
        let items = page["items"]
            .as_array()
            .ok_or("the schedule's list had no items")?;
        if let Some(same) = items.iter().find(|item| {
            item["text"]
                .as_str()
                .is_some_and(|text| same_words(text, words))
        }) {
            return Ok(Some(
                same["clock"].as_str().unwrap_or("its time").to_owned(),
            ));
        }
        match (page["complete"].as_bool(), page["next_after_id"].as_u64()) {
            (Some(true), _) => return Ok(None),
            (Some(false), Some(next)) if next > after_id => after_id = next,
            _ => return Err("the schedule's list was not whole".into()),
        }
    }
    Err(format!(
        "more than {} are set, so the same words can't be ruled out",
        MOST_PAGES * 8
    ))
}

/// Sets `remind` for `skill`, held to its `permissions`, its
/// [`MOST_A_DAY`] (in `tally`, at `now_us`) and the schedule's list of
/// what is set; the journal's words for how it went, never its words.
pub fn set(
    socket: &Path,
    skill: &str,
    permissions: &Permissions,
    remind: &Remind,
    tally: &mut Tally,
    now_us: u64,
) -> String {
    let what = remind.when_said();
    let name = remind.action();
    if let Some(why) = permissions.refuses_action(name) {
        return format!("{what}: refused, {why}");
    }
    if tally.count(skill, now_us) >= MOST_A_DAY {
        return format!("{what}: not set, {MOST_A_DAY} set within a day already");
    }
    match already_set(socket, &remind.words) {
        Ok(Some(clock)) => {
            return format!("{what}: not set, the same words are set already, due {clock}");
        }
        Ok(None) => {}
        Err(why) => return format!("{what}: not set, what is set could not be read: {why}"),
    }
    // Counted once asked, whatever the answer: an answer lost on the way
    // may still have set it.
    tally.mark(skill, now_us);
    match call(socket, name, remind.args()) {
        Ok(answer) if answer["ok"] == true => match answer["result"]["status"].as_str() {
            Some("committed") => format!(
                "{what}: set, due {}",
                answer["result"]["clock"]
                    .as_str()
                    .unwrap_or("at a time not said")
            ),
            Some("already_set") => format!("{what}: set already, nothing new"),
            _ => format!("{what}: not known to be set, the schedule did not say committed"),
        },
        Ok(answer) => format!("{what}: not set, {}", refusal(&answer)),
        Err(error) => {
            format!("{what}: not known to be set, the actions part was not reached ({error})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_words_are_a_made_skills_say() {
        assert_eq!(
            plain_words("  Time to drink water. "),
            Some("Time to drink water.".into())
        );
        for bad in [
            "",
            "Say \"hi\".",
            "See https://example.com now.",
            "Visit www.example.com.",
            "One. Two. Three.",
            "Tab\there.",
            "Zero\u{200B}width.",
            "{note}",
            &"a".repeat(MOST_WORDS_CHARS + 1),
        ] {
            assert_eq!(plain_words(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_clock_time_is_exact() {
        for good in ["00:00", "22:00", "23:59"] {
            assert!(clock_time(good), "{good}");
        }
        for bad in ["24:00", "7:15", "+1:00", "12:60", "12-00", "12:00:00"] {
            assert!(!clock_time(bad), "{bad}");
        }
    }

    #[test]
    fn a_day_counts_the_last_24_hours() {
        let mut tally = Tally::default();
        tally.mark("water", 0);
        tally.mark("water", 10);
        assert_eq!(tally.count("water", DAY_US - 1), 2);
        assert_eq!(tally.count("water", DAY_US), 1);
        assert_eq!(tally.count("water", DAY_US + 10), 0);
        assert_eq!(tally.count("stretch", 0), 0);
    }
}
