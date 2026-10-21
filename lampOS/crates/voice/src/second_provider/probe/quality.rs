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
//!    after it names the thing found of theirs and nothing of another
//!    person's said in the room.
//! 4. "Forget it, never mind" with something kept forgets nothing, and
//!    keeps nothing.
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
    /// No call at all, the answer carries this word, and none of these.
    Says(&'static str, &'static [&'static str]),
    /// Anything but these calls.
    Not(&'static [&'static str]),
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

/// How a quality arm asks: `today`, the spoken turn as the route tells it
/// now; `fix`, the same with [`MEMORY_USE_NOTE`], a proposed note on using
/// what is kept (the first builder's 05:23 run on lamp-4ace: remember and
/// forget 10/10, using what's kept 0 to 5 of 10); `marked`, the fix with
/// each of recall's things not marked theirs marked on itself as not theirs
/// to hear ([`NOT_THEIRS`]), as the voice's recall answer would carry it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum QualityArm {
    Today,
    Fix,
    Marked,
}

impl QualityArm {
    pub const ALL: [Self; 3] = [Self::Today, Self::Fix, Self::Marked];

    pub fn name(self) -> &'static str {
        match self {
            Self::Today => "quality-today",
            Self::Fix => "quality-fix",
            Self::Marked => "quality-marked",
        }
    }
}

/// What `marked` puts on each of recall's things not marked theirs: the
/// rule beside the words it is about, not only in the note (the first
/// builder at 07:11 on #2063: with the note alone, 8 of 10 still repeated
/// the room's secret).
pub const NOT_THEIRS: &str =
    "said in the room, speaker unknown: not theirs to hear, never repeat it or its subject";

/// The proposed note (`fix`), after the memory's calls. Read against what
/// the model is told today, each case's failure has its cause:
/// - preference-used: the memory note and `MEMORY_CALLS_NOTE` both say
///   "earlier conversations: call recall", and nothing says the snapshot
///   already holds the answer;
/// - correction-first and -kept: nothing says what a correction is; the
///   only rule is "asked to remember: call remember";
/// - earlier-today-said: nothing says that of what recall gives, another
///   person's words said in the room are not to be repeated.
///
/// The first wording, on lamp-4ace (the first builder at 06:18 on #2063),
/// took correction-kept to 8/8, correction-first to 5/9 (4 called nothing),
/// earlier-today-said to 4/8 (4 repeated the room's) and preference-used to
/// 7/9. So a correction is now told by its words and its first call, never
/// answered in words alone; the room's are never repeated, named or hinted
/// at; and the snapshot's answer is said as it holds it.
///
/// That wording, at `74341122c` (07:11): both corrections 10/10, but
/// preference-used 5/9 and earlier-today-said 2/10 (the room's secret said
/// 8 times). "Say only those marked theirs" read as all memory, and the
/// snapshot here is the room's, speaker unknown, so the coffee went unsaid:
/// it is now about what recall gives, and the snapshot's answer uses its own
/// word. The room's secret is told apart in the answer too, by `marked`.
pub const MEMORY_USE_NOTE: &str = " When someone says a thing your memory holds is different now (\"actually\", \"now\", \"not any more\"), that corrects it: your first call is forget with that thing's source_id, before any words, then remember the corrected one. Never keep the old and the new, and never answer a correction in words alone. When recall quotes what they said today, answer only from those quotes, word for word, and nothing else; if it says nothing was said today, say you didn't hear one. Any other thing recall gives was said in the room by someone else: never repeat it, name it or hint at it, even in part, so leave it out.";

/// The preference line in `fix` and `marked`: first in the use note, ahead
/// of [`MEMORY_USE_NOTE`]'s correction lines, which stay as they are (the
/// first builder at 09:49 and 12:11 on #1722). `19096d05d` had it first in
/// the memory section instead, after those lines and beside the snapshot;
/// `74341122c` had an earlier wording of it first in the note (5/9).
/// Friday's lamp-06c1 run at 06:43 on #2176: with it, earlier-today fell to
/// 7/10 (called-none 3): "answer from it with no call" was taken for a
/// question about the talk too. So it says the snapshot never holds what
/// was said, and that talk is recall's.
pub const PREFERENCE_LINE: &str = " What your memory snapshot already holds you know: when it is asked about, answer from it with no call, saying the thing it holds with its own word for it, not another; call recall only for what it doesn't hold. It never holds what was said in your conversations: for what you talked about, today or before, call recall.";

/// What `fix` and `marked` give, in a recall of today's answer, as what
/// they said today, when nothing of theirs is found.
pub const NOTHING_TODAY: &str = "nothing said today";

pub const CASES: [Case; 6] = [
    Case {
        name: "preference-used",
        kept: &[(7, "Takes coffee with milk and no sugar.")],
        said: "How do I take my coffee?",
        made: None,
        judge: Judge::Says("milk", &[]),
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
            // As recall answers someone known: theirs marked, then the
            // room's, whose speaker is unknown and may be someone else's
            // (`remember::BOTH_SCOPE`). The room's here is another person's
            // secret, which a friend doesn't repeat (the peer read's P2).
            result: r#"{"ok":true,"result":{"found":[{"heard":"I need to book the dentist for Friday.","said":"Good idea, Friday it is.","when":"today","source_id":21,"theirs":true},{"heard":"Don't tell Minh, his surprise party is on Sunday.","said":"My lips are sealed.","when":"today","source_id":22}],"untrusted":true,"scope":"marked theirs: the person at the lamp now, whose face you know; the rest said in this room, speaker unknown"}}"#,
            answer: None,
        }),
        judge: Judge::Says("dentist", &["party", "surprise"]),
    },
    Case {
        name: "never-mind",
        kept: &[(7, "The violet umbrella is in the car.")],
        said: "Forget it, never mind.",
        made: None,
        // Nothing forgotten, and nothing kept: a friend lets it go (the peer
        // read's P3).
        judge: Judge::Not(&["forget", "remember"]),
    },
];

/// A recall answer as `fix` and `marked` give it (the first builder at
/// 09:49 on #2063): a recall of today (`days_ago` 0) with what they said
/// today quoted as `said_today`, their heard lines, or [`NOTHING_TODAY`]
/// when none is theirs; in `marked`, each found thing not marked theirs
/// also marked `"theirs":false` and [`NOT_THEIRS`] on itself. Any other
/// answer as it is.
fn shaped(args: &str, result: &str, mark: bool) -> String {
    let (Ok(args), Ok(mut answer)) = (
        serde_json::from_str::<serde_json::Value>(args),
        serde_json::from_str::<serde_json::Value>(result),
    ) else {
        return result.to_owned();
    };
    let Some(found) = answer["result"]["found"].as_array_mut() else {
        return result.to_owned();
    };
    let theirs: Vec<serde_json::Value> = found
        .iter()
        .filter(|thing| thing["theirs"] == true)
        .map(|thing| thing["heard"].clone())
        .collect();
    if mark {
        for thing in found.iter_mut().filter(|thing| thing["theirs"] != true) {
            thing["theirs"] = json!(false);
            thing["not_theirs"] = json!(NOT_THEIRS);
        }
    }
    if args["days_ago"] == 0 {
        answer["result"]["said_today"] = if theirs.is_empty() {
            json!(NOTHING_TODAY)
        } else {
            json!(theirs)
        };
    }
    answer.to_string()
}

impl Case {
    /// The request: the spoken character with the memory's calls named, the
    /// turn's changing part, the memory note with what is kept as its frozen
    /// snapshot, the line, and a call already made with the memory's answer.
    pub fn messages(&self, character: &str, arm: QualityArm) -> Vec<Message> {
        let mut spoken = super::super::worker::spoken_character(character, false, true);
        if arm != QualityArm::Today {
            spoken.push_str(PREFERENCE_LINE);
            spoken.push_str(MEMORY_USE_NOTE);
        }
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
            let result = match arm {
                QualityArm::Today => made.result.to_owned(),
                _ => shaped(made.args, made.result, arm == QualityArm::Marked),
            };
            messages.push(Message::tool(call.id.clone(), result));
            if let Some(answer) = made.answer {
                messages.push(Message::assistant(answer));
            }
        }
        messages
    }

    /// Whether one request's first call (`NONE` for none) and words are
    /// right for this case.
    pub fn right(&self, called: &str, words: &str) -> bool {
        self.outcome(called, words) == RIGHT
    }

    /// What one request did, counts only, never its words: `right`, or why
    /// not: the call it made (`called-NAME`), no call but without the word
    /// (`no-word`), or a word it must not say (`said-never`). So a run on a
    /// lamp shows each failure's cause without a word leaving it.
    pub fn outcome(&self, called: &str, words: &str) -> String {
        let has = |word: &str| {
            words
                .split(|c: char| !c.is_alphanumeric())
                .any(|w| w.eq_ignore_ascii_case(word))
        };
        let right = match self.judge {
            Judge::Call(names) => names.contains(&called),
            Judge::Says(word, never) => {
                if called != NONE {
                    false
                } else if never.iter().any(|w| has(w)) {
                    return "said-never".into();
                } else if !has(word) {
                    return "no-word".into();
                } else {
                    true
                }
            }
            Judge::Not(names) => !names.contains(&called),
        };
        if right {
            RIGHT.into()
        } else {
            format!("called-{called}")
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

/// [`Case::outcome`]'s for a right request.
pub const RIGHT: &str = "right";

/// For each arm and case: how many requests came out each way.
pub type Counts = BTreeMap<(QualityArm, usize), BTreeMap<String, u32>>;

/// Each arm asks each case `runs` times, with every function.
pub async fn ask(chat: &Chat, character: &str, runs: u32) -> Counts {
    let mut counts = Counts::new();
    for _ in 0..runs {
        for arm in QualityArm::ALL {
            for (at, case) in CASES.iter().enumerate() {
                match asked(chat, &case.messages(character, arm)).await {
                    Ok((called, words)) => {
                        *counts
                            .entry((arm, at))
                            .or_default()
                            .entry(case.outcome(&called, &words))
                            .or_default() += 1;
                    }
                    Err(error) => println!(
                        "quality arm={} case={} failed: {error}",
                        arm.name(),
                        case.name
                    ),
                }
            }
        }
    }
    counts
}

/// One line an arm and case, with how the others came out, then the bar:
/// the arms with every case right every time.
pub fn verdict(counts: &Counts) -> Vec<String> {
    let mut lines = Vec::new();
    let mut whole = Vec::new();
    for arm in QualityArm::ALL {
        let mut all = true;
        for (at, case) in CASES.iter().enumerate() {
            let outcomes = counts.get(&(arm, at)).cloned().unwrap_or_default();
            let asked: u32 = outcomes.values().sum();
            let right = outcomes.get(RIGHT).copied().unwrap_or_default();
            all &= asked > 0 && right == asked;
            let others: Vec<String> = outcomes
                .iter()
                .filter(|(outcome, _)| outcome.as_str() != RIGHT)
                .map(|(outcome, n)| format!("{outcome}={n}"))
                .collect();
            lines.push(format!(
                "quality arm={} case={} right={right}/{asked}{}",
                arm.name(),
                case.name,
                if others.is_empty() {
                    String::new()
                } else {
                    format!(": {}", others.join(" "))
                }
            ));
        }
        if all {
            whole.push(arm.name());
        }
    }
    lines.push(if whole.is_empty() {
        "quality verdict: no arm right every time on every case".to_owned()
    } else {
        format!(
            "quality verdict: every case right every time: {}",
            whole.join(", ")
        )
    });
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

    fn said_today_of(arm: QualityArm) -> String {
        text(&CASES[4].messages("C", arm)[3])
    }

    /// Each case is a fresh session: the note shows what is kept, with its
    /// source id; a call already made comes with the memory's answer. Only
    /// `fix` carries the proposed note.
    #[test]
    fn each_case_is_a_session_told_what_is_kept() {
        let used = CASES[0].messages("CHARACTER", QualityArm::Today);
        assert_eq!(used.len(), 2);
        let system = text(&used[0]);
        assert!(system.starts_with("CHARACTER"), "{system}");
        assert!(system.contains(DAYS_NOTE));
        assert!(
            system.contains("Takes coffee with milk and no sugar.") && system.contains("\"id\":7")
        );
        assert!(system.contains(super::super::super::worker::MEMORY_CALLS_NOTE));
        assert!(!system.contains(MEMORY_USE_NOTE));
        assert!(
            text(&CASES[0].messages("CHARACTER", QualityArm::Fix)[0]).contains(MEMORY_USE_NOTE)
        );
        assert_eq!(used[1], Message::user("How do I take my coffee?"));
        let kept = CASES[2].messages("C", QualityArm::Today);
        assert_eq!(kept.len(), 4);
        assert_eq!(kept[2].tool_calls[0]["function"]["name"], json!("forget"));
        assert_eq!(kept[3].role, "tool");
        // Only `marked` marks the room's thing on itself; theirs as it was.
        let marked = text(&CASES[4].messages("C", QualityArm::Marked)[3]);
        let found: serde_json::Value = serde_json::from_str(&marked).unwrap();
        let found = &found["result"]["found"];
        assert_eq!(found[0]["theirs"], json!(true));
        assert!(found[0].get("not_theirs").is_none());
        assert_eq!(found[1]["theirs"], json!(false));
        assert_eq!(found[1]["not_theirs"], json!(NOT_THEIRS));
        assert_eq!(found[1]["source_id"], json!(22));
        assert!(text(&CASES[4].messages("C", QualityArm::Marked)[0]).contains(MEMORY_USE_NOTE));
        assert!(!text(&CASES[4].messages("C", QualityArm::Fix)[3]).contains(NOT_THEIRS));
        // In `fix` and `marked`, today's recall quotes only what they said.
        for arm in [QualityArm::Fix, QualityArm::Marked] {
            let answer: serde_json::Value =
                serde_json::from_str(&text(&CASES[4].messages("C", arm)[3])).unwrap();
            assert_eq!(
                answer["result"]["said_today"],
                json!(["I need to book the dentist for Friday."])
            );
        }
        assert!(!said_today_of(QualityArm::Today).contains("said_today"));
        // Nothing of theirs today: said outright; another day: as it was.
        let room = r#"{"ok":true,"result":{"found":[{"heard":"Room words.","source_id":3}]}}"#;
        let none: serde_json::Value =
            serde_json::from_str(&shaped(r#"{"days_ago":0}"#, room, false)).unwrap();
        assert_eq!(none["result"]["said_today"], json!(NOTHING_TODAY));
        assert!(none["result"]["found"][0].get("not_theirs").is_none());
        assert_eq!(
            shaped(r#"{"days_ago":2}"#, room, false),
            serde_json::from_str::<serde_json::Value>(room)
                .unwrap()
                .to_string()
        );
        // The preference line opens the use note in `fix` and `marked`
        // only, ahead of its correction lines; the memory section as today.
        let fixed = text(&CASES[0].messages("C", QualityArm::Fix)[0]);
        let opened = format!("{PREFERENCE_LINE}{MEMORY_USE_NOTE}");
        assert!(fixed.contains(&opened), "{fixed}");
        assert!(fixed.find(PREFERENCE_LINE) < fixed.find("corrects it"));
        assert!(fixed.contains(DAYS_NOTE));
        assert!(!system.contains(PREFERENCE_LINE));
        assert!(
            PREFERENCE_LINE.contains("its own word for it")
                && PREFERENCE_LINE.contains("call recall only")
                && PREFERENCE_LINE
                    .contains("for what you talked about, today or before, call recall")
        );
        let said = CASES[4].messages("C", QualityArm::Today);
        assert!(text(&said[3]).contains("dentist") && text(&said[3]).contains("\"theirs\":true"));
        assert!(text(&said[3]).contains("surprise party"));
        assert!(!text(&CASES[3].messages("C", QualityArm::Today)[0]).contains("\"text\""));
        // What the note proposes, against each failing case's cause.
        for words in [
            "corrects",
            "source_id",
            "first call is forget",
            "Never keep the old and the new",
            "never answer a correction in words alone",
            "answer only from those quotes, word for word",
            "say you didn't hear one",
            "never repeat it",
            "leave it out",
        ] {
            assert!(MEMORY_USE_NOTE.contains(words), "{words}");
        }
    }

    /// The judges, and each wrong one's outcome: the call made, no word, or
    /// a word never to be said, counts only.
    #[test]
    fn each_case_is_judged_on_its_call_and_words() {
        let case = |name: &str| CASES.iter().find(|case| case.name == name).unwrap();
        let used = case("preference-used");
        assert!(used.right(NONE, "With milk and no sugar."));
        assert_eq!(used.outcome("recall", "With milk."), "called-recall");
        assert_eq!(used.outcome(NONE, "Black, I think."), "no-word");
        assert_eq!(used.outcome(NONE, "Buttermilk?"), "no-word");
        let first = case("correction-first");
        assert!(first.right("forget", "") && first.right("recall", ""));
        assert_eq!(first.outcome("remember", ""), "called-remember");
        assert_eq!(first.outcome(NONE, "Noted."), "called-none");
        assert!(case("correction-kept").right("remember", ""));
        assert!(!case("correction-kept").right(NONE, "Got it."));
        assert!(case("earlier-today").right("recall", ""));
        let said = case("earlier-today-said");
        assert!(said.right(NONE, "You were going to book the dentist."));
        assert_eq!(said.outcome(NONE, "Nothing much."), "no-word");
        // Another person's, said in the room, never repeated.
        assert_eq!(
            said.outcome(NONE, "The dentist, and Minh's surprise party."),
            "said-never"
        );
        assert_eq!(
            said.outcome(NONE, "The dentist, and a party on Sunday."),
            "said-never"
        );
        assert_eq!(said.outcome("recall", "The dentist."), "called-recall");
        let never = case("never-mind");
        assert!(never.right(NONE, "Okay.") && never.right("recall", ""));
        assert!(!never.right("forget", "") && !never.right("remember", ""));
    }

    #[test]
    fn the_verdict_says_each_arm_and_case_with_why_and_the_bar() {
        let mut counts = Counts::new();
        for arm in QualityArm::ALL {
            for at in 0..CASES.len() {
                counts.insert((arm, at), [(RIGHT.to_owned(), 3)].into());
            }
        }
        counts.insert(
            (QualityArm::Today, 0),
            [(RIGHT.to_owned(), 1), ("called-recall".to_owned(), 2)].into(),
        );
        let lines = verdict(&counts);
        assert_eq!(
            lines[0],
            "quality arm=quality-today case=preference-used right=1/3: called-recall=2"
        );
        assert!(
            lines.contains(&"quality arm=quality-fix case=preference-used right=3/3".to_owned())
        );
        assert_eq!(
            lines.last().unwrap(),
            "quality verdict: every case right every time: quality-fix, quality-marked"
        );
        assert_eq!(
            verdict(&Counts::new()).last().unwrap(),
            "quality verdict: no arm right every time on every case"
        );
    }
}
