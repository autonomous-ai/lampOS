//! Recall before "I don't know" (#4339, `probe-tools ... --recall`): on
//! lamp-06c1's third long run (30 Oct) the second route called `recall` 3
//! times in 96 turns, and said it had nothing about the person's own life
//! four times while it was kept, observed or in a talk note. Each line here
//! is a fresh session as `quality-today` asks it, asked twice: `before`,
//! with the words the route had before
//! [`super::super::worker::RECALL_BEFORE_NOT_KNOWING`] and recall's wider
//! description; `now`, as the route tells it now.
//!
//! - **own** lines ask about the person's own life, with the answer only in
//!   memory: the first call is right only as `recall`.
//! - **plain** lines are a joke, a fact, the time, small talk and a quiz
//!   answer: right with any first call but `recall`, since a recall there
//!   would put a round before the first audio (CLAUDE.md rule 7).
//!
//! Counts only: the first call each request made. Never the character, a
//! key or what the model said.
use super::super::net::{self, Chat, ChatEvent, Message};
use super::tools::{DAYS_NOTE, NONE};
use crate::remember::{RECALL, RECALL_NOT_FOR, RECALL_OWN_PREFERENCE};
use serde_json::json;
use std::collections::BTreeMap;

/// What recall's description said in place of [`RECALL_OWN_PREFERENCE`]
/// before #4339's rule.
pub const BEFORE_OWN: &str =
    " For their own preference ('my favourite drink?'), call it before saying you do not remember.";
/// What it said in place of [`RECALL_NOT_FOR`].
pub const BEFORE_NOT_FOR: &str = "Not for this conversation.";

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

/// One line: what the person says after a short talk, and whether its first
/// call is to be `recall`.
pub struct Line {
    pub name: &'static str,
    /// The talk before it: (person, lamp), ours, short.
    pub before: &'static [(&'static str, &'static str)],
    pub said: &'static str,
    pub recall: bool,
}

const CHAT: &[(&str, &str)] = &[
    ("Hey Lamp, are you there?", "I'm here."),
    (
        "It's been a long day.",
        "Sounds like it. Want to take a minute?",
    ),
];

pub const LINES: [Line; 10] = [
    Line {
        name: "own-train",
        before: CHAT,
        said: "What time is Lily's train tomorrow?",
        recall: true,
    },
    Line {
        name: "own-talked",
        before: CHAT,
        said: "What did we talk about earlier?",
        recall: true,
    },
    Line {
        name: "own-hotel",
        before: CHAT,
        said: "Which hotel did we book for the trip?",
        recall: true,
    },
    Line {
        name: "own-project",
        before: CHAT,
        said: "What's my project at work called again?",
        recall: true,
    },
    Line {
        name: "own-dinner",
        before: CHAT,
        said: "Who am I having dinner with on Friday?",
        recall: true,
    },
    Line {
        name: "plain-joke",
        before: CHAT,
        said: "Tell me a short joke.",
        recall: false,
    },
    Line {
        name: "plain-fact",
        before: CHAT,
        said: "What's the capital of Australia?",
        recall: false,
    },
    Line {
        name: "plain-time",
        before: CHAT,
        said: "What time is it?",
        recall: false,
    },
    Line {
        name: "plain-small-talk",
        before: CHAT,
        said: "How are you doing today?",
        recall: false,
    },
    Line {
        name: "plain-quiz-answer",
        before: &[
            (
                "Let's do a quiz. Ask me a capital.",
                "Okay! What's the capital of Canada?",
            ),
            ("Hmm, let me think.", "Take your time."),
        ],
        said: "Is it Ottawa?",
        recall: false,
    },
];

/// The spoken turn's words as `arm` tells them: `before` without
/// [`super::super::worker::RECALL_BEFORE_NOT_KNOWING`].
pub fn spoken(character: &str, arm: Arm) -> String {
    let now = super::super::worker::spoken_character(character, false, true);
    match arm {
        Arm::Now => now,
        Arm::Before => now.replace(super::super::worker::RECALL_BEFORE_NOT_KNOWING, ""),
    }
}

/// The functions as `arm` declares them: `before` with recall's description
/// as it was.
pub fn declared(mut tools: Vec<lamp_gemini::Tool>, arm: Arm) -> Vec<lamp_gemini::Tool> {
    if arm == Arm::Before {
        for tool in tools.iter_mut().filter(|tool| tool.name == RECALL) {
            tool.description = tool
                .description
                .replace(RECALL_OWN_PREFERENCE, BEFORE_OWN)
                .replace(RECALL_NOT_FOR, BEFORE_NOT_FOR);
        }
    }
    tools
}

impl Line {
    /// A fresh session: the spoken words, the turn's changing part, the
    /// memory note with a snapshot that doesn't hold the answer, the talk
    /// before, and the line.
    pub fn messages(&self, character: &str, arm: Arm) -> Vec<Message> {
        let snapshot = json!({
            "scope": "said in this room; speaker unknown",
            "untrusted": true,
            "facts": [{"id": 7, "kind": "kept", "text": "Likes green tea in the afternoon."}],
        });
        let system = format!(
            "{}\n\n{}{DAYS_NOTE}{snapshot}\n",
            spoken(character, arm),
            super::cache::changing(0)
        );
        let mut messages = vec![Message::system(system)];
        for (heard, answered) in self.before {
            messages.push(Message::user(*heard));
            messages.push(Message::assistant(*answered));
        }
        messages.push(Message::user(self.said));
        messages
    }

    /// `right`, or the first call it made instead (`called-NAME`).
    pub fn outcome(&self, called: &str) -> String {
        if (called == RECALL) == self.recall {
            RIGHT.to_owned()
        } else {
            format!("called-{called}")
        }
    }
}

pub const RIGHT: &str = "right";

/// For each arm and line: how many requests came out each way.
pub type Counts = BTreeMap<(Arm, usize), BTreeMap<String, u32>>;

/// The first function a request called, or [`NONE`].
async fn asked(chat: &Chat, messages: &[Message]) -> net::Result<String> {
    let mut stream = chat.send(messages).await?;
    let mut called = None;
    while let Some(event) = stream.next().await? {
        if let ChatEvent::ToolCalls(calls) = event
            && called.is_none()
        {
            called = calls.first().map(|call| call.name.clone());
        }
    }
    Ok(called.unwrap_or_else(|| NONE.to_owned()))
}

/// Each arm asks each line `runs` times, `before` on `chats[0]` and `now`
/// on `chats[1]`, each declaring its own functions.
pub async fn ask(chats: [&Chat; 2], character: &str, runs: u32) -> Counts {
    let mut counts = Counts::new();
    for _ in 0..runs {
        for (arm, chat) in Arm::ALL.into_iter().zip(chats) {
            for (at, line) in LINES.iter().enumerate() {
                match asked(chat, &line.messages(character, arm)).await {
                    Ok(called) => {
                        *counts
                            .entry((arm, at))
                            .or_default()
                            .entry(line.outcome(&called))
                            .or_default() += 1;
                    }
                    Err(error) => println!(
                        "recall-first arm={} line={} failed: {error}",
                        arm.name(),
                        line.name
                    ),
                }
            }
        }
    }
    counts
}

/// One line an arm and line, then each arm's own and plain totals.
pub fn verdict(counts: &Counts) -> Vec<String> {
    let mut lines = Vec::new();
    for arm in Arm::ALL {
        let (mut own, mut own_asked, mut plain, mut plain_asked) = (0, 0, 0, 0);
        for (at, line) in LINES.iter().enumerate() {
            let outcomes = counts.get(&(arm, at)).cloned().unwrap_or_default();
            let asked: u32 = outcomes.values().sum();
            let right = outcomes.get(RIGHT).copied().unwrap_or_default();
            if line.recall {
                (own, own_asked) = (own + right, own_asked + asked);
            } else {
                (plain, plain_asked) = (plain + right, plain_asked + asked);
            }
            let others: Vec<String> = outcomes
                .iter()
                .filter(|(outcome, _)| outcome.as_str() != RIGHT)
                .map(|(outcome, n)| format!("{outcome}={n}"))
                .collect();
            lines.push(format!(
                "recall-first arm={} line={} right={right}/{asked}{}",
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
            "recall-first arm={} own recalled={own}/{own_asked} plain without recall={plain}/{plain_asked}",
            arm.name()
        ));
    }
    lines
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

    /// `before` is the route's words without the rule and recall as it was
    /// described; `now` is the route's own, rule and all.
    #[test]
    fn before_takes_out_exactly_the_rule_and_recall_s_new_words() {
        let now = text(&LINES[0].messages("C", Arm::Now)[0]);
        let before = text(&LINES[0].messages("C", Arm::Before)[0]);
        let rule = super::super::super::worker::RECALL_BEFORE_NOT_KNOWING;
        assert!(now.contains(rule) && !before.contains(rule));
        assert_eq!(before, now.replace(rule, ""));
        assert!(before.contains(super::super::super::worker::MEMORY_CALLS_NOTE));
        assert!(now.contains(DAYS_NOTE) && now.contains("green tea"));
        let tools = super::super::cache::functions();
        let recall = |tools: &[lamp_gemini::Tool]| {
            tools
                .iter()
                .find(|tool| tool.name == RECALL)
                .unwrap()
                .description
                .clone()
        };
        let then = recall(&declared(tools.clone(), Arm::Before));
        assert!(then.contains(BEFORE_OWN) && then.contains(BEFORE_NOT_FOR));
        assert!(!then.contains(RECALL_OWN_PREFERENCE) && !then.contains(RECALL_NOT_FOR));
        assert_eq!(recall(&declared(tools.clone(), Arm::Now)), recall(&tools));
        assert!(recall(&tools).contains(RECALL_OWN_PREFERENCE));
        // The others are as they are.
        let others = |tools: Vec<lamp_gemini::Tool>| {
            tools
                .into_iter()
                .filter(|tool| tool.name != RECALL)
                .map(|tool| tool.description)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            others(declared(tools.clone(), Arm::Before)),
            others(tools.clone())
        );
        // The talk before comes first, then the line.
        let quiz = LINES
            .iter()
            .find(|l| l.name == "plain-quiz-answer")
            .unwrap();
        let asked = quiz.messages("C", Arm::Now);
        assert_eq!(asked.len(), 6);
        assert_eq!(asked[5], Message::user("Is it Ottawa?"));
        assert_eq!(LINES.iter().filter(|line| line.recall).count(), 5);
    }

    #[test]
    fn own_lines_want_recall_and_plain_ones_anything_else() {
        assert_eq!(LINES[0].outcome("recall"), RIGHT);
        assert_eq!(LINES[0].outcome(NONE), "called-none");
        let plain = &LINES[5];
        assert_eq!(plain.outcome(NONE), RIGHT);
        assert_eq!(plain.outcome("the_time"), RIGHT);
        assert_eq!(plain.outcome("recall"), "called-recall");
        let mut counts = Counts::new();
        for arm in Arm::ALL {
            for at in 0..LINES.len() {
                counts.insert((arm, at), [(RIGHT.to_owned(), 3)].into());
            }
        }
        counts.insert(
            (Arm::Before, 0),
            [(RIGHT.to_owned(), 1), ("called-none".to_owned(), 2)].into(),
        );
        let lines = verdict(&counts);
        assert_eq!(
            lines[0],
            "recall-first arm=before line=own-train right=1/3: called-none=2"
        );
        assert!(lines.contains(
            &"recall-first arm=before own recalled=13/15 plain without recall=15/15".to_owned()
        ));
        assert_eq!(
            lines.last().unwrap(),
            "recall-first arm=now own recalled=15/15 plain without recall=15/15"
        );
    }
}
