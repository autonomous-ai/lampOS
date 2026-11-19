//! `lamp-voice probe-tools PROVIDER_CONFIG SECOND_CONFIG [RUNS] --face`:
//! does the model call `remember_my_face` for a question whether the lamp
//! knows them (`LAMP_FACE_QUESTION_NOT_ASK`, lamp-74ff, 19 Nov: "Did you
//! remember my face then?" and "Do you remember my face?", right after the
//! face was kept, each called it). No word list judges the line in the
//! product (the owner's 13 Nov rule): the description's
//! [`crate::known_faces`] `QUESTION_NOT_ASK` line does, and this scores it.
//!
//! Each line is asked as the next turn after the face was kept, as on
//! lamp-74ff: "Remember my face, I'm Ana." called `remember_my_face`, the
//! eyes kept it, and the lamp said so. Two arms, each with the names checked
//! as on the product: `before`, the description with the switch off, and
//! `now`, with it on. A question should call nothing; an ask should call
//! `remember_my_face`.
//!
//! Counts only: for each arm and kind (question or ask, English or
//! Vietnamese), how many requests called `remember_my_face`. Never the
//! character, a key, a URL or what the model said.
use super::super::net::{self, Chat, Message, ToolCall};
use crate::known_faces::{FORGET, FaceSwitches, REMEMBER, tools_for};
use crate::second_config::CREDENTIALS_ENV;
use serde_json::json;
use std::{collections::BTreeMap, io, path::Path, sync::Arc};

/// A question whether the lamp knows them, or an ask to remember them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Question,
    Ask,
}

/// One line: what the person says, what it is, and whether it is English.
pub struct Line {
    pub said: &'static str,
    pub kind: Kind,
    pub english: bool,
}

const fn question_line(said: &'static str, english: bool) -> Line {
    Line {
        said,
        kind: Kind::Question,
        english,
    }
}

const fn ask_line(said: &'static str, english: bool) -> Line {
    Line {
        said,
        kind: Kind::Ask,
        english,
    }
}

/// Made-up lines, English and Vietnamese, the two lamp-74ff lines first.
pub const LINES: [Line; 32] = [
    question_line("Okay. Did you remember my face then?", true),
    question_line("Okay. Do you remember my face?", true),
    question_line("Do you remember me?", true),
    question_line("Do you know who I am?", true),
    question_line("Do you know my face?", true),
    question_line("Do you recognise me?", true),
    question_line("Have you remembered me?", true),
    question_line("So who am I?", true),
    question_line("Did it work? Do you know me now?", true),
    question_line("Lamp, what's my name?", true),
    question_line("Bạn có nhớ mặt mình không?", false),
    question_line("Bạn có biết mình là ai không?", false),
    question_line("Bạn nhớ mình chưa?", false),
    question_line("Bạn có nhận ra mình không?", false),
    question_line("Mình tên gì nhỉ?", false),
    question_line("Vậy bạn nhớ mặt mình rồi hả?", false),
    ask_line("Remember my face, I'm Ana.", true),
    ask_line("Can you remember me as Jay?", true),
    ask_line("Could you call me Jay?", true),
    ask_line("Can you remember my face?", true),
    ask_line("Please remember my face.", true),
    ask_line("Remember me, I'm Ana.", true),
    ask_line("Do you know who I am? I'd like you to keep my face.", true),
    ask_line("I'm Ana. Please remember me.", true),
    ask_line("Learn my face, please.", true),
    ask_line("Remember me as Ana from now on.", true),
    ask_line("Nhớ mặt mình nhé.", false),
    ask_line("Nhớ mặt mình nha, mình là Ana.", false),
    ask_line("Gọi mình là Jay nhé.", false),
    ask_line("Bạn nhớ mặt mình giúp nhé.", false),
    ask_line("Hãy nhớ mình là Ana.", false),
    ask_line("Nhớ mình nhé, mình tên Ana.", false),
];

/// How an arm declares `remember_my_face`.
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

    /// The face functions as a spoken turn declares them on the product
    /// (names checked), the switch off or on.
    pub fn switches(self) -> FaceSwitches {
        FaceSwitches {
            checked: true,
            face_question: self == Self::Now,
            ..FaceSwitches::default()
        }
    }
}

/// Every function a spoken turn declares, the face ones as `arm` declares
/// them.
pub fn functions(arm: Arm) -> Vec<lamp_gemini::Tool> {
    let mut tools: Vec<_> = super::cache::functions()
        .into_iter()
        .filter(|tool| tool.name != REMEMBER && tool.name != FORGET)
        .collect();
    tools.extend(tools_for(arm.switches()));
    tools
}

/// The request for `line`: the character and the turn's changing part,
/// then the save that kept the face as on lamp-74ff, then the line.
pub fn messages(character: &str, line: &Line) -> Vec<Message> {
    let call = ToolCall {
        id: "call_0".into(),
        name: REMEMBER.to_owned(),
        args: json!({"name": "Ana"}),
        extra: None,
    };
    vec![
        Message::system(format!("{character}\n\n{}", super::cache::changing(0))),
        Message::user("Remember my face, I'm Ana."),
        Message::assistant_calls(None, std::slice::from_ref(&call)),
        Message::tool(
            call.id.clone(),
            json!({"ok": true, "remembered": "Ana"}).to_string(),
        ),
        Message::assistant("Got it, Ana. I'll know you now."),
        Message::user(line.said),
    ]
}

/// For each arm and line, how many requests called `remember_my_face`
/// and how many were asked.
pub type Counts = BTreeMap<(Arm, usize), (u32, u32)>;

/// Each arm asks each line `runs` times.
pub async fn ask(chats: [&Chat; 2], character: &str, runs: u32) -> Counts {
    let mut counts = Counts::new();
    for _ in 0..runs {
        for arm in Arm::ALL {
            let chat = chats[usize::from(arm == Arm::Now)];
            for (at, line) in LINES.iter().enumerate() {
                let called = match super::tools::asked(chat, &messages(character, line)).await {
                    Ok(name) => name == REMEMBER,
                    Err(error) => {
                        println!("face arm={} line={} failed: {error}", arm.name(), at + 1);
                        continue;
                    }
                };
                let entry = counts.entry((arm, at)).or_default();
                entry.0 += u32::from(called);
                entry.1 += 1;
            }
        }
    }
    counts
}

/// One line per arm and line, then per arm and kind and language: how
/// many requests called `remember_my_face`. A question right calls
/// nothing; an ask right calls it.
pub fn verdict(counts: &Counts) -> Vec<String> {
    let mut lines = Vec::new();
    for arm in Arm::ALL {
        let mut kinds: BTreeMap<(Kind, bool), (u32, u32)> = BTreeMap::new();
        for (at, line) in LINES.iter().enumerate() {
            let (called, asked) = counts.get(&(arm, at)).copied().unwrap_or_default();
            let kind = kinds.entry((line.kind, line.english)).or_default();
            kind.0 += called;
            kind.1 += asked;
            lines.push(format!(
                "face arm={} line={} {} {}: called={called}/{asked}",
                arm.name(),
                at + 1,
                if line.kind == Kind::Question {
                    "question"
                } else {
                    "ask"
                },
                if line.english { "en" } else { "vi" },
            ));
        }
        for ((kind, english), (called, asked)) in kinds {
            let right = if kind == Kind::Question {
                asked - called
            } else {
                called
            };
            lines.push(format!(
                "face verdict arm={} {} {}: called={called}/{asked} right={right}/{asked}",
                arm.name(),
                if kind == Kind::Question {
                    "questions"
                } else {
                    "asks"
                },
                if english { "en" } else { "vi" },
            ));
        }
    }
    lines
}

/// Asks both arms on DeepSeek, routed as the live second route is
/// ([`super::tools::deepseek_route`]: through the relay with its credential
/// when the unit's base URL is set), and prints the counts.
pub async fn run(provider_config: &Path, second_config: &Path, runs: u32) -> io::Result<()> {
    let provider = crate::config::ProviderConfig::load(provider_config)?;
    let character = provider.instruction(false)?;
    let config = crate::second_config::load_from_path(second_config)?
        .with_environment(|name| std::env::var_os(name))?;
    let credentials = std::env::var_os(CREDENTIALS_ENV);
    let (base, key) = super::tools::deepseek_route(&config, credentials.as_deref())?;
    let model = &config.hosted.deepseek_model;
    let chat = |arm| {
        let tools = functions(arm);
        let declare: net::Declare = Arc::new(move || tools.clone());
        Chat::deepseek_at(&base, model, key.expose(), declare, false).map_err(io::Error::other)
    };
    let (before, now) = (chat(Arm::Before)?, chat(Arm::Now)?);
    println!(
        "face: remember_my_face for a question whether known (LAMP_FACE_QUESTION_NOT_ASK), DeepSeek's {model} {}, thinking off, {} lines, {runs} runs of each arm",
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

    /// The arms differ only in the question line of the description.
    #[test]
    fn the_arms_differ_only_in_the_question_line() {
        let description = |arm: Arm| {
            functions(arm)
                .into_iter()
                .find(|tool| tool.name == REMEMBER)
                .map(|tool| tool.description)
                .unwrap()
        };
        let (before, now) = (description(Arm::Before), description(Arm::Now));
        assert!(now.starts_with(&before) && now.len() > before.len());
        assert_eq!(
            functions(Arm::Before)
                .iter()
                .filter(|tool| tool.name == REMEMBER)
                .count(),
            1
        );
        let questions = LINES.iter().filter(|line| line.kind == Kind::Question);
        let asks = LINES.iter().filter(|line| line.kind == Kind::Ask);
        assert_eq!((questions.count(), asks.count()), (16, 16));
    }

    #[test]
    fn the_verdict_counts_a_question_right_when_nothing_is_called() {
        let mut counts = Counts::new();
        counts.insert((Arm::Now, 0), (0, 2));
        counts.insert((Arm::Now, 16), (2, 2));
        let verdict = verdict(&counts);
        assert!(
            verdict.contains(&"face verdict arm=now questions en: called=0/2 right=2/2".to_owned())
        );
        assert!(verdict.contains(&"face verdict arm=now asks en: called=2/2 right=2/2".to_owned()));
    }
}
