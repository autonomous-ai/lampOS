//! Memory's quality on the cheaper voice (#1693, the first builder at 03:05
//! on #1722): four things a friend's memory does, asked of the second
//! route's DeepSeek beside the memory's lines (`tools::MEMORY_LINES`), each
//! a fresh session told what the lamp keeps, as `told_for_session` tells
//! it. Counts only: which function each request called, and whether the
//! answer after a call carries the one word it should.
//!
//! 1. A preference kept is used in a new session with no call.
//! 2. A correction replaces the old kept thing: its first call forgets (or
//!    recalls, to find it), never a second `remember`; and once forgotten,
//!    the new one is kept.
//! 3. "What did we talk about earlier today?" calls `recall`, and the answer
//!    after it names the thing it found.
//! 4. "Forget it, never mind" with something kept forgets nothing.
use super::super::net::{self, Chat, ChatEvent, Message, ToolCall};
use super::tools::{DAYS_NOTE, NONE};
use serde_json::json;
use std::collections::BTreeMap;

/// How a case is judged, on the first call its request makes and the words
/// it answers with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Judge {
    /// The first call is one of these.
    Call(&'static [&'static str]),
    /// No call at all, and the answer carries this word.
    Says(&'static str),
    /// Anything but this call.
    Not(&'static str),
}

/// A call already made in a case's history: its name, arguments and the
/// memory's answer, then the lamp's words after it.
pub struct Made {
    pub name: &'static str,
    pub args: &'static str,
    pub result: &'static str,
    pub answer: Option<&'static str>,
}

pub struct Case {
    pub name: &'static str,
    /// What the session's frozen note shows as kept: (source id, text).
    pub kept: &'static [(u64, &'static str)],
    pub said: &'static str,
    /// A call the lamp has already made for `said`, its answer read next.
    pub made: Option<Made>,
    pub judge: Judge,
}

pub const CASES: [Case; 6] = [
    Case {
        name: "preference-used",
        kept: &[(7, "Takes coffee with milk and no sugar.")],
        said: "How do I take my coffee?",
        made: None,
        judge: Judge::Says("milk"),
    },
    Case {
        name: "correction-first",
        kept: &[(7, "Takes coffee black.")],
        said: "Actually, I take it with milk now.",
        made: None,
        judge: Judge::Call(&["forget", "recall"]),
    },
    Case {
        name: "correction-kept",
        kept: &[(7, "Takes coffee black.")],
        said: "Actually, I take it with milk now.",
        made: Some(Made {
            name: "forget",
            args: r#"{"source_id":7}"#,
            result: r#"{"ok":true,"result":{"forgotten":true}}"#,
            answer: None,
        }),
        judge: Judge::Call(&["remember"]),
    },
    Case {
        name: "earlier-today",
        kept: &[],
        said: "What did we talk about earlier today?",
        made: None,
        judge: Judge::Call(&["recall"]),
    },
    Case {
        name: "earlier-today-said",
        kept: &[],
        said: "What did we talk about earlier today?",
        made: Some(Made {
            name: "recall",
            args: r#"{"days_ago":0}"#,
            result: r#"{"ok":true,"result":{"found":[{"heard":"I need to book the dentist for Friday.","said":"Good idea, Friday it is.","when":"today","source_id":21}]}}"#,
            answer: None,
        }),
        judge: Judge::Says("dentist"),
    },
    Case {
        name: "never-mind",
        kept: &[(7, "The violet umbrella is in the car.")],
        said: "Forget it, never mind.",
        made: None,
        judge: Judge::Not("forget"),
    },
];

impl Case {
    /// The request: the spoken character with the memory's calls named, the
    /// turn's changing part, the memory note with what is kept as its frozen
    /// snapshot, the line, and a call already made with the memory's answer.
    pub fn messages(&self, character: &str) -> Vec<Message> {
        let spoken = super::super::worker::spoken_character(character, false, true);
        let snapshot = json!({
            "scope": "said in this room; speaker unknown",
            "untrusted": true,
            "facts": self.kept.iter().map(|(id, text)| json!({"id": id, "kind": "kept", "text": text})).collect::<Vec<_>>(),
        });
        let system = format!(
            "{spoken}\n\n{}{DAYS_NOTE}{snapshot}\n",
            super::cache::changing(0)
        );
        let mut messages = vec![Message::system(system), Message::user(self.said)];
        if let Some(made) = &self.made {
            let call = ToolCall {
                id: "call_0".into(),
                name: made.name.to_owned(),
                args: serde_json::from_str(made.args).unwrap_or_else(|_| json!({})),
                extra: None,
            };
            messages.push(Message::assistant_calls(None, std::slice::from_ref(&call)));
            messages.push(Message::tool(call.id.clone(), made.result.to_owned()));
            if let Some(answer) = made.answer {
                messages.push(Message::assistant(answer));
            }
        }
        messages
    }

    /// Whether one request's first call (`NONE` for none) and words are
    /// right for this case.
    pub fn right(&self, called: &str, words: &str) -> bool {
        match self.judge {
            Judge::Call(names) => names.contains(&called),
            Judge::Says(word) => {
                called == NONE
                    && words
                        .split(|c: char| !c.is_alphanumeric())
                        .any(|w| w.eq_ignore_ascii_case(word))
            }
            Judge::Not(name) => called != name,
        }
    }
}

/// One request: its first call's name, and its words.
async fn asked(chat: &Chat, messages: &[Message]) -> net::Result<(String, String)> {
    let mut stream = chat.send(messages).await?;
    let (mut called, mut words) = (None, String::new());
    while let Some(event) = stream.next().await? {
        match event {
            ChatEvent::ToolCalls(calls) if called.is_none() => {
                called = calls.first().map(|call| call.name.clone());
            }
            ChatEvent::Text(piece) => words.push_str(&piece),
            _ => {}
        }
    }
    Ok((called.unwrap_or_else(|| NONE.to_owned()), words))
}

/// For each case: right and asked.
pub type Counts = BTreeMap<usize, (u32, u32)>;

/// Each case asked `runs` times, with every function.
pub async fn ask(chat: &Chat, character: &str, runs: u32) -> Counts {
    let mut counts = Counts::new();
    for _ in 0..runs {
        for (at, case) in CASES.iter().enumerate() {
            match asked(chat, &case.messages(character)).await {
                Ok((called, words)) => {
                    let entry = counts.entry(at).or_default();
                    entry.1 += 1;
                    entry.0 += u32::from(case.right(&called, &words));
                }
                Err(error) => println!("quality case={} failed: {error}", case.name),
            }
        }
    }
    counts
}

/// One line a case, then the bar: every case right every time.
pub fn verdict(counts: &Counts) -> Vec<String> {
    let mut lines = Vec::new();
    let mut all = true;
    for (at, case) in CASES.iter().enumerate() {
        let (right, asked) = counts.get(&at).copied().unwrap_or_default();
        all &= asked > 0 && right == asked;
        lines.push(format!("quality case={} right={right}/{asked}", case.name));
    }
    lines.push(format!(
        "quality verdict: {}",
        if all {
            "every case right every time"
        } else {
            "not every case right every time"
        }
    ));
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

    /// Each case is a fresh session: the note shows what is kept, with its
    /// source id; a call already made comes with the memory's answer.
    #[test]
    fn each_case_is_a_session_told_what_is_kept() {
        let used = CASES[0].messages("CHARACTER");
        assert_eq!(used.len(), 2);
        let system = text(&used[0]);
        assert!(system.starts_with("CHARACTER"), "{system}");
        assert!(system.contains(DAYS_NOTE));
        assert!(
            system.contains("Takes coffee with milk and no sugar.") && system.contains("\"id\":7")
        );
        assert!(system.contains(super::super::super::worker::MEMORY_CALLS_NOTE));
        assert_eq!(used[1], Message::user("How do I take my coffee?"));
        let kept = CASES[2].messages("C");
        assert_eq!(kept.len(), 4);
        assert_eq!(kept[2].tool_calls[0]["function"]["name"], json!("forget"));
        assert_eq!(kept[3].role, "tool");
        let said = CASES[4].messages("C");
        assert!(text(&said[3]).contains("dentist"));
        assert!(!text(&CASES[3].messages("C")[0]).contains("\"text\""));
    }

    /// The judges: a preference used in words and no call; a correction
    /// that forgets or recalls first, never a second remember; the new one
    /// kept once forgotten; recall for earlier today, and its answer naming
    /// what it found; never-mind forgetting nothing.
    #[test]
    fn each_case_is_judged_on_its_call_and_words() {
        let case = |name: &str| CASES.iter().find(|case| case.name == name).unwrap();
        let used = case("preference-used");
        assert!(used.right(NONE, "With milk and no sugar."));
        assert!(!used.right("recall", "With milk."));
        assert!(!used.right(NONE, "Black, I think."));
        assert!(!used.right(NONE, "Buttermilk?"));
        let first = case("correction-first");
        assert!(first.right("forget", "") && first.right("recall", ""));
        assert!(!first.right("remember", "") && !first.right(NONE, "Noted."));
        assert!(case("correction-kept").right("remember", ""));
        assert!(!case("correction-kept").right(NONE, "Got it."));
        assert!(case("earlier-today").right("recall", ""));
        assert!(case("earlier-today-said").right(NONE, "You were going to book the dentist."));
        assert!(!case("earlier-today-said").right(NONE, "Nothing much."));
        let never = case("never-mind");
        assert!(never.right(NONE, "Okay.") && never.right("recall", ""));
        assert!(!never.right("forget", ""));
    }

    #[test]
    fn the_verdict_says_each_case_and_the_bar() {
        let mut counts = Counts::new();
        for at in 0..CASES.len() {
            counts.insert(at, (3, 3));
        }
        let lines = verdict(&counts);
        assert_eq!(lines[0], "quality case=preference-used right=3/3");
        assert_eq!(
            lines.last().unwrap(),
            "quality verdict: every case right every time"
        );
        counts.insert(1, (2, 3));
        assert_eq!(
            verdict(&counts).last().unwrap(),
            "quality verdict: not every case right every time"
        );
        assert_eq!(
            verdict(&Counts::new()).last().unwrap(),
            "quality verdict: not every case right every time"
        );
    }
}
