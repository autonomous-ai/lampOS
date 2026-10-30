//! A cut-in made plain (#3997's cut-in proposal, `lamp-voice probe-cut-in
//! PROVIDER_CONFIG SECOND_CONFIG [RUNS]`): the words the second route tells
//! a line said over its answer ([`crate::cut_in_told::NOTE`]) and the mark
//! after an answer cut and kept as heard ([`crate::cut_in_told::CUT_HERE`]),
//! asked of the route's chat as it asks them. Each line is a fresh session,
//! asked with each arm's words:
//!
//! - `before`: the note as it was ([`BEFORE_NOTE`]), no mark after a cut.
//! - `now`: the note and the mark as the route tells them now.
//!
//! The lines:
//!
//! - **stop**: "Wait.", "Hold on.", "Pause." or "Stop." over a story: right
//!   when the reply is a short stop alone (at most [`STOP_MOST_WORDS`]
//!   words, the story not gone on with). With `LAMP_SPOKEN_STOP_ENDS=on`
//!   the lamp answers "Stop." itself ([`crate::spoken_stop`]), so that row
//!   measures the note alone.
//! - **ack**: a question over the story: right when the reply opens with a
//!   few words that show it was heard ([`opens_with_ack`]) and answers it.
//! - **resume**: "go on with the story" after the story was cut and a
//!   question answered: right when its first words say where it was ("where
//!   was I", or the cut's last moment) and it doesn't start over.
//! - **plain**: no cut at all: right with no acknowledgement first.
//!
//! Counts only: an outcome for each request. Never the character, a key or
//! what the model said.
use super::super::net::{self, Chat, ChatEvent, Message};
use super::tools::NONE;
use std::{collections::BTreeMap, io, path::Path, sync::Arc};

/// The cut-in note before #3997's proposal.
pub const BEFORE_NOTE: &str = "(For you to know, not to answer: they spoke while you were speaking, and you stopped there. Answer what they said; go back to what you were saying only if they ask you to.)";

/// The most words a short stop has: "Okay, I'll stop there." is four.
pub const STOP_MOST_WORDS: usize = 8;

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

    fn note(self) -> &'static str {
        match self {
            Self::Before => BEFORE_NOTE,
            Self::Now => crate::cut_in_told::NOTE,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Stop,
    Ack,
    Resume,
    Plain,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Ack => "ack",
            Self::Resume => "resume",
            Self::Plain => "plain",
        }
    }
}

pub struct Line {
    pub name: &'static str,
    pub kind: Kind,
    pub said: &'static str,
    /// For an `ack` line, words of which one in the reply answers it.
    pub answer: &'static [&'static str],
}

const ASKED: &str = "Tell me a story about a lighthouse keeper.";
/// The story as played up to the cut, kept as heard with its dash.
const PLAYED: &str = "Once there was a lighthouse keeper named Tom who lived alone on a rocky island. Every night he climbed the hundred stairs to light the lamp. One stormy night, as he reached the top, he saw a small boat";
/// Words of the cut's last moment, any of which says where it was.
const WHERE: &[&str] = &[
    "where was i",
    "where were we",
    "boat",
    "top of",
    "stairs",
    "reached the top",
];
/// Words of the story's start, which a reply that starts over says again.
const START: &[&str] = &["once there was", "once upon", "named tom who lived"];
/// Words only the story has: a stop reply that says them went on with it.
const STORY: &[&str] = &["boat", "storm", "tom ", "island", "waves", "keeper"];

const CHAT: &[(&str, &str)] = &[
    ("Hey Lamp, are you there?", "I'm here."),
    (
        "It's been a long day.",
        "Sounds like it. Want to take a minute?",
    ),
];

pub const LINES: [Line; 11] = [
    Line {
        name: "stop-wait",
        kind: Kind::Stop,
        said: "Wait.",
        answer: &[],
    },
    Line {
        name: "stop-hold-on",
        kind: Kind::Stop,
        said: "Hold on.",
        answer: &[],
    },
    Line {
        name: "stop-pause",
        kind: Kind::Stop,
        said: "Pause.",
        answer: &[],
    },
    Line {
        name: "stop-stop",
        kind: Kind::Stop,
        said: "Stop.",
        answer: &[],
    },
    Line {
        name: "ack-sum",
        kind: Kind::Ack,
        said: "What is two plus two?",
        answer: &["four", "4"],
    },
    Line {
        name: "ack-wait-what",
        kind: Kind::Ack,
        said: "Wait, what does a lighthouse keeper do?",
        answer: &["light", "lamp", "ship", "safe", "look after", "keeps"],
    },
    Line {
        name: "ack-spell",
        kind: Kind::Ack,
        said: "How do you spell lighthouse?",
        answer: &["l-i-g", "l i g", "l, i, g", "l. i. g"],
    },
    Line {
        name: "resume-go-on",
        kind: Kind::Resume,
        said: "Okay, go on with the story.",
        answer: &[],
    },
    Line {
        name: "resume-continue",
        kind: Kind::Resume,
        said: "Continue.",
        answer: &[],
    },
    Line {
        name: "plain-sum",
        kind: Kind::Plain,
        said: "What is two plus two?",
        answer: &[],
    },
    Line {
        name: "plain-joke",
        kind: Kind::Plain,
        said: "Tell me a short joke.",
        answer: &[],
    },
];

impl Line {
    /// A fresh session as the route asks it: the spoken words and the
    /// turn's changing part, the exchanges before, the notes, the line.
    pub fn messages(&self, character: &str, arm: Arm) -> Vec<Message> {
        let system = format!(
            "{}\n\n{}",
            super::super::worker::spoken_character(character, false, true),
            super::cache::changing(0)
        );
        let mut messages = vec![Message::system(system)];
        let story = |messages: &mut Vec<Message>| {
            messages.push(Message::user(ASKED));
            messages.push(Message::assistant(format!(
                "{PLAYED}{}",
                crate::said_as_heard::CUT
            )));
            if arm == Arm::Now {
                messages.push(Message::system(crate::cut_in_told::CUT_HERE));
            }
        };
        match self.kind {
            Kind::Stop | Kind::Ack => {
                story(&mut messages);
                messages.push(Message::system(arm.note()));
            }
            Kind::Resume => {
                story(&mut messages);
                // The question that cut it, answered; this line is not a
                // cut-in, so it is told no note.
                messages.push(Message::user("What is two plus two?"));
                messages.push(Message::assistant("Four."));
            }
            Kind::Plain => {
                for (heard, answered) in CHAT {
                    messages.push(Message::user(*heard));
                    messages.push(Message::assistant(*answered));
                }
            }
        }
        messages.push(Message::user(self.said));
        messages
    }

    /// The reply's outcome: [`RIGHT`], or what it did instead.
    pub fn outcome(&self, reply: &Reply) -> String {
        let text = match reply {
            Reply::Called(name) => return format!("called-{name}"),
            Reply::Said(text) => text,
        };
        let lower = text.to_lowercase();
        let words = text.split_whitespace().count();
        let wrong = |why: &str| why.to_owned();
        match self.kind {
            Kind::Stop if any(&lower, STORY) => wrong("went-on"),
            Kind::Stop if words > STOP_MOST_WORDS => wrong("long"),
            Kind::Stop => RIGHT.to_owned(),
            Kind::Ack if apologises(&lower) => wrong("apology"),
            Kind::Ack if !any(&lower, self.answer) => wrong("no-answer"),
            Kind::Ack if !opens_with_ack(text) => wrong("no-ack"),
            Kind::Ack => RIGHT.to_owned(),
            Kind::Resume if any(&lower, START) => wrong("restarted"),
            Kind::Resume if !any(&first_words(&lower, 25), WHERE) => wrong("no-where"),
            Kind::Resume => RIGHT.to_owned(),
            Kind::Plain if opens_with_ack(text) => wrong("acked"),
            Kind::Plain => RIGHT.to_owned(),
        }
    }
}

pub const RIGHT: &str = "right";

fn any(text: &str, words: &[&str]) -> bool {
    words.iter().any(|word| text.contains(word))
}

fn first_words(text: &str, most: usize) -> String {
    text.split_whitespace()
        .take(most)
        .collect::<Vec<_>>()
        .join(" ")
}

fn apologises(lower: &str) -> bool {
    lower.contains("apolog") || lower.contains("sorry for") || lower.contains("sorry about")
}

/// Words an acknowledgement opens with.
const ACK: &[&str] = &[
    "oh",
    "sure",
    "okay",
    "ok",
    "of course",
    "right",
    "yes",
    "yeah",
    "ah",
    "got it",
    "alright",
    "all right",
    "good question",
    "mm",
    "hmm",
    "sorry",
    "oops",
    "absolutely",
    "certainly",
];

/// Whether `text` opens with a few words that show the line was heard: a
/// first clause (to the first comma, dash, stop or exclamation) of at most
/// four words that begins with one of [`ACK`]. A speech tag before it
/// (`[warmly]`) is passed over.
pub fn opens_with_ack(text: &str) -> bool {
    let mut text = text.trim_start();
    while let Some(rest) = text.strip_prefix('[')
        && let Some(end) = rest.find(']')
    {
        text = rest[end + 1..].trim_start();
    }
    let text = text
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    let clause = text
        .split([',', '—', '–', '-', '.', '!', '…', ':', ';'])
        .next()
        .unwrap_or_default()
        .trim();
    clause.split_whitespace().count() <= 4
        && ACK.iter().any(|ack| {
            clause == *ack
                || clause
                    .strip_prefix(ack)
                    .is_some_and(|rest| rest.starts_with(' '))
        })
}

/// What a request came back with: its words, or the first function it
/// called.
pub enum Reply {
    Said(String),
    Called(String),
}

pub type Counts = BTreeMap<(Arm, usize), BTreeMap<String, u32>>;

async fn asked(chat: &Chat, messages: &[Message]) -> net::Result<Reply> {
    let mut stream = chat.send(messages).await?;
    let (mut said, mut called) = (String::new(), None);
    while let Some(event) = stream.next().await? {
        match event {
            ChatEvent::Text(text) => said.push_str(&text),
            ChatEvent::ToolCalls(calls) if called.is_none() => {
                called = calls.first().map(|call| call.name.clone());
            }
            _ => {}
        }
    }
    Ok(match called {
        Some(name) if said.trim().is_empty() => Reply::Called(name),
        _ if said.trim().is_empty() => Reply::Called(NONE.to_owned()),
        _ => Reply::Said(said),
    })
}

/// Each arm asks each line `runs` times.
pub async fn ask(chat: &Chat, character: &str, runs: u32) -> Counts {
    let mut counts = Counts::new();
    for _ in 0..runs {
        for arm in Arm::ALL {
            for (at, line) in LINES.iter().enumerate() {
                match asked(chat, &line.messages(character, arm)).await {
                    Ok(reply) => {
                        *counts
                            .entry((arm, at))
                            .or_default()
                            .entry(line.outcome(&reply))
                            .or_default() += 1;
                    }
                    Err(error) => println!(
                        "cut-in arm={} line={} failed: {error}",
                        arm.name(),
                        line.name
                    ),
                }
            }
        }
    }
    counts
}

/// One line an arm and line, then each arm's total for each kind.
pub fn verdict(counts: &Counts) -> Vec<String> {
    let mut lines = Vec::new();
    for arm in Arm::ALL {
        let mut kinds: BTreeMap<&str, (u32, u32)> = BTreeMap::new();
        for (at, line) in LINES.iter().enumerate() {
            let outcomes = counts.get(&(arm, at)).cloned().unwrap_or_default();
            let asked: u32 = outcomes.values().sum();
            let right = outcomes.get(RIGHT).copied().unwrap_or_default();
            let kind = kinds.entry(line.kind.name()).or_default();
            *kind = (kind.0 + right, kind.1 + asked);
            let others: Vec<String> = outcomes
                .iter()
                .filter(|(outcome, _)| outcome.as_str() != RIGHT)
                .map(|(outcome, n)| format!("{outcome}={n}"))
                .collect();
            lines.push(format!(
                "cut-in arm={} line={} right={right}/{asked}{}",
                arm.name(),
                line.name,
                if others.is_empty() {
                    String::new()
                } else {
                    format!(": {}", others.join(" "))
                }
            ));
        }
        let totals: Vec<String> = ["stop", "ack", "resume", "plain"]
            .iter()
            .map(|kind| {
                let (right, asked) = kinds.get(kind).copied().unwrap_or_default();
                format!("{kind}={right}/{asked}")
            })
            .collect();
        lines.push(format!("cut-in arm={} {}", arm.name(), totals.join(" ")));
    }
    lines
}

/// Asked of DeepSeek as the route reaches it (through the relay with its
/// credential, or its own API with the unit's `deepseek.key`), thinking
/// off, declaring the voice's own functions.
pub async fn run(provider_config: &Path, second_config: &Path, runs: u32) -> io::Result<()> {
    let provider = crate::config::ProviderConfig::load(provider_config)?;
    let character = provider.instruction(false)?;
    let config = crate::second_config::load_from_path(second_config)?
        .with_environment(|name| std::env::var_os(name))?;
    let credentials = std::env::var_os(crate::second_config::CREDENTIALS_ENV);
    let (base, key) = super::tools::deepseek_route(&config, credentials.as_deref())?;
    let model = &config.hosted.deepseek_model;
    let tools = super::cache::functions();
    let declare: net::Declare = Arc::new(move || tools.clone());
    let chat =
        Chat::deepseek_at(&base, model, key.expose(), declare, false).map_err(io::Error::other)?;
    println!(
        "cut-in: #3997's cut-in proposal, DeepSeek's {model} {}, thinking off, {} lines, {runs} runs of each arm",
        super::super::worker::deepseek_where(&config.hosted),
        LINES.len()
    );
    for line in verdict(&ask(&chat, &character, runs).await) {
        println!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(text: &str) -> Reply {
        Reply::Said(text.to_owned())
    }

    fn line(name: &str) -> &'static Line {
        LINES.iter().find(|line| line.name == name).unwrap()
    }

    #[test]
    fn the_arms_differ_only_in_the_note_and_the_mark() {
        let now = line("ack-sum").messages("C", Arm::Now);
        let before = line("ack-sum").messages("C", Arm::Before);
        let here = Message::system(crate::cut_in_told::CUT_HERE);
        assert!(now.contains(&Message::system(crate::cut_in_told::NOTE)) && now.contains(&here));
        assert!(before.contains(&Message::system(BEFORE_NOTE)) && !before.contains(&here));
        assert_eq!(now.len(), before.len() + 1);
        assert_eq!(now.last(), Some(&Message::user("What is two plus two?")));
        // A resume is no cut-in: no note, the mark only now.
        let resume = line("resume-go-on").messages("C", Arm::Now);
        assert!(resume.contains(&here));
        assert!(!resume.contains(&Message::system(crate::cut_in_told::NOTE)));
        assert!(
            !line("resume-go-on")
                .messages("C", Arm::Before)
                .contains(&here)
        );
        // A plain line has no cut at all.
        let plain = line("plain-sum").messages("C", Arm::Now);
        assert!(
            !plain.contains(&here) && !plain.contains(&Message::system(crate::cut_in_told::NOTE))
        );
        assert_ne!(BEFORE_NOTE, crate::cut_in_told::NOTE);
    }

    #[test]
    fn outcomes_count_a_short_stop_an_ack_a_clear_resume_and_no_ack_unasked() {
        let stop = line("stop-wait");
        assert_eq!(stop.outcome(&said("Okay, I'll stop.")), RIGHT);
        assert_eq!(
            stop.outcome(&said(
                "Okay, I'll stop there. Just let me know whenever you want me to carry on."
            )),
            "long"
        );
        assert_eq!(
            stop.outcome(&said("Sure. The boat drifted closer.")),
            "went-on"
        );
        assert_eq!(
            stop.outcome(&Reply::Called("decline".into())),
            "called-decline"
        );
        let ack = line("ack-sum");
        assert_eq!(ack.outcome(&said("Oh, sure — it's four.")), RIGHT);
        assert_eq!(ack.outcome(&said("[warmly] Sure! Four.")), RIGHT);
        assert_eq!(ack.outcome(&said("Two plus two is four.")), "no-ack");
        assert_eq!(ack.outcome(&said("Oh, sure — I'm not sure.")), "no-answer");
        assert_eq!(
            ack.outcome(&said("I apologise for rambling on. It's four.")),
            "apology"
        );
        let resume = line("resume-go-on");
        assert_eq!(
            resume.outcome(&said(
                "Where was I… Tom had just seen a small boat in the storm."
            )),
            RIGHT
        );
        assert_eq!(
            resume.outcome(&said("Once there was a lighthouse keeper named Tom.")),
            "restarted"
        );
        assert_eq!(
            resume.outcome(&said(
                "The waves crashed and the wind howled all night long."
            )),
            "no-where"
        );
        let plain = line("plain-joke");
        assert_eq!(
            plain.outcome(&said("Why did the lamp go to school? To get brighter.")),
            RIGHT
        );
        assert_eq!(
            plain.outcome(&said("Oh, sure — why did the lamp...")),
            "acked"
        );
        assert!(!opens_with_ack(
            "Okay so here is a much longer opening clause, then more"
        ));
        assert!(opens_with_ack("Of course! Four."));
        assert!(!opens_with_ack("Oho, four."));
    }

    #[test]
    fn the_verdict_counts_each_kind() {
        let mut counts = Counts::new();
        for arm in Arm::ALL {
            for at in 0..LINES.len() {
                counts.insert((arm, at), [(RIGHT.to_owned(), 3)].into());
            }
        }
        counts.insert(
            (Arm::Before, 0),
            [(RIGHT.to_owned(), 1), ("long".to_owned(), 2)].into(),
        );
        let lines = verdict(&counts);
        assert_eq!(
            lines[0],
            "cut-in arm=before line=stop-wait right=1/3: long=2"
        );
        assert!(
            lines.contains(&"cut-in arm=before stop=10/12 ack=9/9 resume=6/6 plain=6/6".to_owned())
        );
        assert_eq!(
            lines.last().unwrap(),
            "cut-in arm=now stop=12/12 ack=9/9 resume=6/6 plain=6/6"
        );
    }
}
