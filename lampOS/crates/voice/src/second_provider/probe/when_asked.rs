//! The day, only when asked when (#4753, `probe-tools ... --when`): recall
//! told the model to answer "without dates or times", and each entry said
//! only "3 days ago" or "last week", so "when did I tell you about my tea?"
//! could not be answered with a day. Each line here is a fresh session as
//! `quality-today` asks it, the model's `recall` already made and answered
//! as the voice answers it ([`crate::remember::answer_for`]), asked twice:
//! `before`, with recall's old words and entries telling only how long
//! ago; `now`, as the voice tells them now, the day beside it.
//!
//! - **when** lines ask when something was said: right only when the reply
//!   names a day (a weekday, a month or a date).
//! - **plain** lines ask about the same things without asking when: right
//!   only when the reply names no day, since a date recited unasked is
//!   what a machine does (CLAUDE.md rule 8).
//!
//! Counts only: never the character, a key or what the model said.
use super::super::net::{self, Chat, Message, ToolCall};
use crate::remember::{RECALL, RECALL_WHEN};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io, path::Path, sync::Arc};

/// What recall's description said in place of [`RECALL_WHEN`] before
/// #4753.
pub const BEFORE_WHEN: &str = ", without dates or times.";

/// The probe's now: Monday 26 October 2026, 09:00 in Ho Chi Minh City, as
/// [`super::cache::changing`] says ("a Monday in October").
pub const NOW: u64 = 1_792_980_000_000;
/// Ho Chi Minh City's offset, in minutes.
const ZONE: lamp_local_time::LocalTime = lamp_local_time::LocalTime::Offset(420);
const DAY: u64 = 86_400_000;

/// Which words a request is told.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Arm {
    Before,
    Now,
}

impl Arm {
    pub const ALL: [Self; 2] = [Self::Before, Self::Now];

    pub fn name(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::Now => "now",
        }
    }
}

/// One line: what the person asks, the recall the model made for it, what
/// the memory found, and whether the reply is to name a day.
pub struct Line {
    pub name: &'static str,
    pub said: &'static str,
    pub about: &'static str,
    /// How many days before [`NOW`] it was said.
    pub days_ago: u64,
    /// `Some(text)` for a kept thing, else an exchange of `heard`.
    pub kept: Option<&'static str>,
    pub heard: &'static str,
    pub when: bool,
}

const TEA: &str = "I've switched to green tea in the mornings.";
const DENTIST: &str = "Has a dentist at the clinic on Main Street.";
const SISTER: &str = "My sister is coming to visit from Hanoi.";
const RUNNING: &str = "I started running in the park before work.";

pub const LINES: [Line; 8] = [
    Line {
        name: "when-tea",
        said: "When did I last tell you about my tea?",
        about: "tea",
        days_ago: 2,
        kept: None,
        heard: TEA,
        when: true,
    },
    Line {
        name: "when-dentist",
        said: "On which day did I tell you about my dentist?",
        about: "dentist",
        days_ago: 5,
        kept: Some(DENTIST),
        heard: "",
        when: true,
    },
    Line {
        name: "when-sister",
        said: "When did we talk about my sister's visit?",
        about: "sister visit",
        days_ago: 10,
        kept: None,
        heard: SISTER,
        when: true,
    },
    Line {
        name: "when-running",
        said: "What day did I say I started running?",
        about: "running",
        days_ago: 3,
        kept: None,
        heard: RUNNING,
        when: true,
    },
    Line {
        name: "plain-tea",
        said: "What tea am I drinking these days?",
        about: "tea",
        days_ago: 2,
        kept: None,
        heard: TEA,
        when: false,
    },
    Line {
        name: "plain-dentist",
        said: "Where's my dentist again?",
        about: "dentist",
        days_ago: 5,
        kept: Some(DENTIST),
        heard: "",
        when: false,
    },
    Line {
        name: "plain-sister",
        said: "Who did I say is coming to visit?",
        about: "visit",
        days_ago: 10,
        kept: None,
        heard: SISTER,
        when: false,
    },
    Line {
        name: "plain-running",
        said: "What did I tell you about running?",
        about: "running",
        days_ago: 3,
        kept: None,
        heard: RUNNING,
        when: false,
    },
];

const CHAT: &[(&str, &str)] = &[
    ("Hey Lamp, are you there?", "I'm here."),
    (
        "It's been a long day.",
        "Sounds like it. Want to take a minute?",
    ),
];

/// The functions as `arm` declares them: `before` with recall's old words.
pub fn declared(mut tools: Vec<lamp_gemini::Tool>, arm: Arm) -> Vec<lamp_gemini::Tool> {
    if arm == Arm::Before {
        for tool in tools.iter_mut().filter(|tool| tool.name == RECALL) {
            tool.description = tool.description.replace(RECALL_WHEN, BEFORE_WHEN);
        }
    }
    tools
}

/// The relative part alone ("3 days ago"), as `when` was before #4753.
fn relative(when: &str) -> String {
    when.split(", ").next().unwrap_or(when).to_owned()
}

impl Line {
    /// What the memory found, as the store answers it.
    fn found(&self) -> Value {
        let at = NOW - self.days_ago * DAY;
        let entry = match self.kept {
            Some(text) => json!({"kind":"kept","id":7,"at_unix_ms":at,"text":text}),
            None => {
                json!({"kind":"exchange","id":7,"at_unix_ms":at,"heard":self.heard,"said":"Good to know."})
            }
        };
        json!({"ok":true,"entries":[entry]})
    }

    /// The recall's answer as the voice tells it to `arm`.
    pub fn told(&self, arm: Arm) -> Value {
        let mut told = crate::remember::answer_for(RECALL, &self.found(), NOW, Some(&ZONE), None)
            .unwrap_or_default();
        if arm == Arm::Before
            && let Some(found) = told["found"].as_array_mut()
        {
            for entry in found {
                let when = relative(entry["when"].as_str().unwrap_or_default());
                entry["when"] = json!(when);
            }
        }
        told
    }

    /// A fresh session: the spoken words, the turn's changing part, an
    /// empty memory note, the talk before, the line, and its recall made
    /// and answered.
    pub fn messages(&self, character: &str, arm: Arm) -> Vec<Message> {
        let spoken = super::super::worker::spoken_character(character, false, true);
        let snapshot = json!({
            "scope": "said in this room; speaker unknown",
            "untrusted": true,
            "facts": [],
        });
        let system = format!(
            "{spoken}\n\n{}{}{snapshot}\n",
            super::cache::changing(0),
            super::tools::DAYS_NOTE,
        );
        let mut messages = vec![Message::system(system)];
        for (heard, answered) in CHAT {
            messages.push(Message::user(*heard));
            messages.push(Message::assistant(*answered));
        }
        messages.push(Message::user(self.said));
        let call = ToolCall {
            id: "call_0".into(),
            name: RECALL.into(),
            args: json!({"about": self.about}),
            extra: None,
        };
        messages.push(Message::assistant_calls(None, std::slice::from_ref(&call)));
        messages.push(Message::tool(call.id, self.told(arm).to_string()));
        messages
    }

    /// `right`, or what the reply did instead.
    pub fn outcome(&self, words: &str) -> String {
        match (self.when, names_a_day(words)) {
            (true, true) | (false, false) => RIGHT.to_owned(),
            (true, false) => "no-day".to_owned(),
            (false, true) => "day-recited".to_owned(),
        }
    }
}

pub const RIGHT: &str = "right";

/// Weekdays and months; not "may", a word of its own too.
const DAY_WORDS: [&str; 18] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "january",
    "february",
    "march",
    "april",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// Whether words name a day: a weekday, a month, or a date ("the 24th").
pub fn names_a_day(words: &str) -> bool {
    let words = words.to_lowercase();
    words.split(|c: char| !c.is_alphanumeric()).any(|word| {
        let digits = word.trim_end_matches(char::is_alphabetic);
        let suffix = &word[digits.len()..];
        DAY_WORDS.contains(&word)
            || (!digits.is_empty()
                && digits.len() <= 2
                && digits.chars().all(|c| c.is_ascii_digit())
                && matches!(suffix, "st" | "nd" | "rd" | "th"))
    })
}

/// For each arm and line: how many requests came out each way.
pub type Counts = BTreeMap<(Arm, usize), BTreeMap<String, u32>>;

/// Each arm asks each line `runs` times, `before` on `chats[0]` and `now`
/// on `chats[1]`, each declaring its own functions.
pub async fn ask(chats: [&Chat; 2], character: &str, runs: u32) -> Counts {
    let mut counts = Counts::new();
    for _ in 0..runs {
        for (arm, chat) in Arm::ALL.into_iter().zip(chats) {
            for (at, line) in LINES.iter().enumerate() {
                match super::says_kept::asked(chat, &line.messages(character, arm)).await {
                    Ok((_, words)) => {
                        *counts
                            .entry((arm, at))
                            .or_default()
                            .entry(line.outcome(&words))
                            .or_default() += 1;
                    }
                    Err(error) => println!(
                        "when-asked arm={} line={} failed: {error}",
                        arm.name(),
                        line.name
                    ),
                }
            }
        }
    }
    counts
}

/// One line an arm and line, then each arm's when and plain totals.
pub fn verdict(counts: &Counts) -> Vec<String> {
    let mut lines = Vec::new();
    for arm in Arm::ALL {
        let (mut when, mut when_asked, mut plain, mut plain_asked) = (0, 0, 0, 0);
        for (at, line) in LINES.iter().enumerate() {
            let outcomes = counts.get(&(arm, at)).cloned().unwrap_or_default();
            let asked: u32 = outcomes.values().sum();
            let right = outcomes.get(RIGHT).copied().unwrap_or_default();
            if line.when {
                (when, when_asked) = (when + right, when_asked + asked);
            } else {
                (plain, plain_asked) = (plain + right, plain_asked + asked);
            }
            let others: Vec<String> = outcomes
                .iter()
                .filter(|(outcome, _)| outcome.as_str() != RIGHT)
                .map(|(outcome, n)| format!("{outcome}={n}"))
                .collect();
            lines.push(format!(
                "when-asked arm={} line={} right={right}/{asked}{}",
                arm.name(),
                line.name,
                if others.is_empty() {
                    String::new()
                } else {
                    format!(": {}", others.join(" "))
                }
            ));
        }
        lines.push(format!(
            "when-asked arm={} when named a day={when}/{when_asked} plain without a day={plain}/{plain_asked}",
            arm.name()
        ));
    }
    lines
}

/// `probe-tools PROVIDER_CONFIG SECOND_CONFIG [RUNS] --when`: the voice's
/// own functions, on the second route's DeepSeek model as the lamp reaches
/// it, thinking off.
pub async fn run(provider_config: &Path, second_config: &Path, runs: u32) -> io::Result<()> {
    let provider = crate::config::ProviderConfig::load(provider_config)?;
    let character = provider.instruction(false)?;
    let config = crate::second_config::load_from_path(second_config)?
        .with_environment(|name| std::env::var_os(name))?;
    let credentials = std::env::var_os(crate::second_config::CREDENTIALS_ENV);
    let (base, key) = super::tools::deepseek_route(&config, credentials.as_deref())?;
    let model = &config.hosted.deepseek_model;
    let functions = super::cache::functions();
    let chat = |arm| {
        let tools = declared(functions.clone(), arm);
        let declare: net::Declare = Arc::new(move || tools.clone());
        Chat::deepseek_at(&base, model, key.expose(), declare, false).map_err(io::Error::other)
    };
    let (before, now) = (chat(Arm::Before)?, chat(Arm::Now)?);
    println!(
        "tools: the day only when asked when (#4753), DeepSeek's {model} {}, thinking off, {} lines, {runs} runs of each arm",
        super::super::worker::deepseek_where(&config.hosted),
        LINES.len()
    );
    for line in verdict(&ask([&before, &now], &character, runs).await) {
        println!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(message: &Message) -> String {
        message
            .content
            .as_ref()
            .and_then(|content| content.as_str())
            .unwrap_or_default()
            .to_owned()
    }

    /// `now` is told the day beside how long ago, `before` only how long
    /// ago; recall's words differ only in [`RECALL_WHEN`].
    #[test]
    fn before_is_told_only_how_long_ago_and_the_old_words() {
        let tea = &LINES[0];
        assert_eq!(
            tea.told(Arm::Now)["found"][0]["when"],
            "2 days ago, Saturday 24 October"
        );
        assert_eq!(tea.told(Arm::Before)["found"][0]["when"], "2 days ago");
        assert_eq!(
            LINES[2].told(Arm::Now)["found"][0]["when"],
            "last week, Friday 16 October"
        );
        assert_eq!(
            LINES[1].told(Arm::Now)["found"][0]["asked_to_remember"],
            DENTIST
        );
        let asked = tea.messages("C", Arm::Now);
        assert_eq!(asked.len(), 8);
        assert_eq!(asked[5], Message::user(tea.said));
        assert!(text(&asked[7]).contains("Saturday 24 October"));
        assert!(!text(&tea.messages("C", Arm::Before)[7]).contains("Saturday"));
        let tools = super::super::cache::functions();
        let recall = |tools: Vec<lamp_gemini::Tool>| {
            tools
                .into_iter()
                .find(|tool| tool.name == RECALL)
                .unwrap()
                .description
        };
        let then = recall(declared(tools.clone(), Arm::Before));
        assert!(then.contains("without dates or times") && !then.contains(RECALL_WHEN));
        assert_eq!(
            then,
            recall(tools.clone()).replace(RECALL_WHEN, BEFORE_WHEN)
        );
        assert_eq!(recall(declared(tools.clone(), Arm::Now)), recall(tools));
        assert_eq!(LINES.iter().filter(|line| line.when).count(), 4);
    }

    #[test]
    fn a_when_line_wants_a_day_and_a_plain_one_none() {
        assert!(names_a_day("On Saturday, the 24th."));
        assert!(names_a_day("That was on the 24th."));
        assert!(names_a_day("Back in October."));
        assert!(!names_a_day("A couple of days ago."));
        assert!(!names_a_day("You're on green tea in the mornings."));
        assert!(!names_a_day("It may be green tea, 2 cups."));
        assert_eq!(LINES[0].outcome("On Saturday."), RIGHT);
        assert_eq!(LINES[0].outcome("Two days ago."), "no-day");
        assert_eq!(LINES[4].outcome("Green tea."), RIGHT);
        assert_eq!(
            LINES[4].outcome("Green tea, since Saturday."),
            "day-recited"
        );
        let mut counts = Counts::new();
        for arm in Arm::ALL {
            for at in 0..LINES.len() {
                counts.insert((arm, at), [(RIGHT.to_owned(), 3)].into());
            }
        }
        counts.insert(
            (Arm::Before, 0),
            [(RIGHT.to_owned(), 1), ("no-day".to_owned(), 2)].into(),
        );
        let lines = verdict(&counts);
        assert_eq!(
            lines[0],
            "when-asked arm=before line=when-tea right=1/3: no-day=2"
        );
        assert!(lines.contains(
            &"when-asked arm=before when named a day=10/12 plain without a day=12/12".to_owned()
        ));
    }
}
