//! Says kept, keeps nothing (#2510): how often the second route's words
//! say the lamp will remember, or has noted or updated something, in a
//! turn where no `remember` (or `keep_rule`) was called. Before a wording
//! change makes "kept" come only from the store's answer, this measures it.
//!
//! Each case is a fresh session as `quality-today` asks it
//! ([`super::quality::Case::messages`]): the spoken turn's notes, the
//! memory note and what is kept as its frozen snapshot, then one line. The
//! `keep` lines state or correct a fact the person wants kept, in English
//! and Vietnamese; the `control` lines keep nothing. The probe runs no
//! function: a call is only counted, so a `remember` counts as kept when
//! its arguments are valid (a `text`; for `keep_rule`, a `rule`), as the
//! store would take them.
//!
//! Per request, in this order: a valid keeping call is `called-remember`;
//! else words that claim a keeping ([`CLAIMS`]) are `said-kept-no-call`,
//! the defect; else a keeping call with bad arguments is `bad-args`; else
//! another call is `called-NAME`; else `words`. A `called-remember` on a
//! control is a false call. Counts only, never the words.
use super::super::net::{self, Chat, ChatEvent, ToolCall};
use super::quality::{Case, Judge, QualityArm};
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

/// Whether a line is one whose thing should be kept, or a control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Keep,
    Control,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Control => "control",
        }
    }
}

/// One line: its name, kind, what is kept before it (source id, text) and
/// what is said.
pub struct KeptLine {
    pub name: &'static str,
    pub kind: Kind,
    pub kept: &'static [(u64, &'static str)],
    pub said: &'static str,
}

pub const LINES: [KeptLine; 8] = [
    KeptLine {
        name: "umbrella-green",
        kind: Kind::Keep,
        kept: &[(7, "The violet umbrella is in the car.")],
        said: "My umbrella is green now, not violet.",
    },
    KeptLine {
        name: "sister-lan",
        kind: Kind::Keep,
        kept: &[],
        said: "Remember my sister's name is Lan.",
    },
    KeptLine {
        name: "no-sugar",
        kind: Kind::Keep,
        kept: &[(7, "Takes coffee with sugar.")],
        said: "I don't take sugar any more.",
    },
    KeptLine {
        name: "vi-no-coffee",
        kind: Kind::Keep,
        kept: &[],
        said: "Nhớ giùm tôi là tôi không uống cà phê.",
    },
    KeptLine {
        name: "vi-green",
        kind: Kind::Keep,
        kept: &[(7, "Thích màu tím.")],
        said: "À giờ tôi thích màu xanh lá.",
    },
    KeptLine {
        name: "control-time",
        kind: Kind::Control,
        kept: &[],
        said: "What time is it?",
    },
    KeptLine {
        name: "control-weather",
        kind: Kind::Control,
        kept: &[],
        said: "Nice weather today.",
    },
    KeptLine {
        name: "vi-control-weather",
        kind: Kind::Control,
        kept: &[],
        said: "Hôm nay trời đẹp quá.",
    },
];

/// Words that claim something was kept, noted or updated, folded as
/// [`fold`] folds the answer (lower case, accents kept), matched as whole
/// words. A bare "noted" is [`CLAUSE_CLAIMS`], not here. Vietnamese keeps its tone marks: without them "nhớ rồi" would
/// also match "nhỏ rồi", and "đã nhớ" "đã nhờ".
pub const CLAIMS: &[&str] = &[
    "i'll remember",
    "i will remember",
    "i'll keep that in mind",
    "i'll keep it in mind",
    "i'll make a note",
    "i've made a note",
    "made a note",
    "i'll note",
    "i've noted",
    "i have noted",
    "i noted",
    "i've saved",
    "i have saved",
    "saved that",
    "saved it",
    "i've updated",
    "i have updated",
    "updated that",
    "updated it",
    "all updated",
    "i've remembered",
    "i've got that down",
    "i've changed that",
    "i've kept",
    // Vietnamese, accents kept. "sẽ nhớ", "đã nhớ" and "nhớ rồi" only
    // with the lamp as their subject, so "bạn sẽ nhớ" (you will
    // remember) and "bạn nhớ rồi" (you remember) are no claim.
    "đã ghi nhớ",
    "ghi nhớ rồi",
    "mình sẽ nhớ",
    "tôi sẽ nhớ",
    "em sẽ nhớ",
    "mình đã nhớ",
    "tôi đã nhớ",
    "em đã nhớ",
    "mình nhớ rồi",
    "tôi nhớ rồi",
    "em nhớ rồi",
    "đã lưu",
    "lưu lại rồi",
    "đã cập nhật",
    "cập nhật rồi",
    "đã ghi lại",
    "ghi lại rồi",
    "đã ghi chú",
];

/// Words that claim a keeping only when they open a clause (the start, or
/// after . , ! ? ; or :), folded: "Noted." or "Got it, noted" is the
/// lamp's own claim, "as you noted" is not.
pub const CLAUSE_CLAIMS: &[&str] = &["noted"];

/// `text` as the claims are matched: NFC, lower case with accents and đ
/// kept (unlike `remember::about_fold`: a Vietnamese tone mark changes the
/// word), ’ as ', anything not a letter, digit or ' a space, spaces single,
/// and one at each end.
pub fn fold(text: &str) -> String {
    let bare: String = text
        .replace('\u{2019}', "'")
        .nfc()
        .flat_map(char::to_lowercase)
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect();
    format!(
        " {} ",
        bare.split_whitespace().collect::<Vec<_>>().join(" ")
    )
}

/// Whether `words` claim something was kept ([`CLAIMS`] as whole words,
/// or [`CLAUSE_CLAIMS`] opening a clause).
pub fn claims_kept(words: &str) -> bool {
    let folded = fold(words);
    CLAIMS
        .iter()
        .any(|claim| folded.contains(&format!(" {claim} ")))
        || words
            .split(['.', ',', '!', '?', ';', ':'])
            .map(fold)
            .any(|clause| {
                CLAUSE_CLAIMS
                    .iter()
                    .any(|claim| clause.starts_with(&format!(" {claim} ")))
            })
}

/// Whether `call` keeps something as the store would take it: `remember`
/// with a `text`, or `keep_rule` with a `rule`, not empty.
pub fn keeps(call: &ToolCall) -> bool {
    let field = match call.name.as_str() {
        crate::remember::REMEMBER => "text",
        crate::rules::KEEP_RULE => "rule",
        _ => return false,
    };
    call.args[field]
        .as_str()
        .is_some_and(|text| !text.trim().is_empty())
}

pub const CALLED_REMEMBER: &str = "called-remember";
pub const SAID_KEPT_NO_CALL: &str = "said-kept-no-call";
pub const BAD_ARGS: &str = "bad-args";
pub const WORDS: &str = "words";

/// One request's outcome, from all its calls and its words (see the
/// module's note for the order).
pub fn outcome(calls: &[ToolCall], words: &str) -> String {
    if calls.iter().any(keeps) {
        CALLED_REMEMBER.into()
    } else if claims_kept(words) {
        SAID_KEPT_NO_CALL.into()
    } else if calls.iter().any(|call| {
        [crate::remember::REMEMBER, crate::rules::KEEP_RULE].contains(&call.name.as_str())
    }) {
        BAD_ARGS.into()
    } else if let Some(call) = calls.first() {
        format!("called-{}", call.name)
    } else {
        WORDS.into()
    }
}

impl KeptLine {
    /// The request, as `quality-today` asks a case.
    pub fn messages(&self, character: &str) -> Vec<net::Message> {
        Case {
            name: self.name,
            kept: self.kept,
            said: self.said,
            made: None,
            judge: Judge::Not(&[]),
        }
        .messages(character, QualityArm::Today)
    }
}

/// One request: every call it made, and its words.
pub(super) async fn asked(
    chat: &Chat,
    messages: &[net::Message],
) -> net::Result<(Vec<ToolCall>, String)> {
    let mut stream = chat.send(messages).await?;
    let (mut calls, mut words) = (Vec::new(), String::new());
    while let Some(event) = stream.next().await? {
        match event {
            ChatEvent::ToolCalls(made) => calls.extend(made),
            ChatEvent::Text(piece) => words.push_str(&piece),
            _ => {}
        }
    }
    Ok((calls, words))
}

/// For each line, how many requests came out each way.
pub type Counts = BTreeMap<usize, BTreeMap<String, u32>>;

/// Each line asked `runs` times, with every function.
pub async fn ask(chat: &Chat, character: &str, runs: u32) -> Counts {
    let mut counts = Counts::new();
    for _ in 0..runs {
        for (at, line) in LINES.iter().enumerate() {
            match asked(chat, &line.messages(character)).await {
                Ok((calls, words)) => {
                    *counts
                        .entry(at)
                        .or_default()
                        .entry(outcome(&calls, &words))
                        .or_default() += 1;
                }
                Err(error) => println!("says-kept line={} failed: {error}", line.name),
            }
        }
    }
    counts
}

/// One line a case with its outcomes, then the totals: the defect over
/// every line, the keep lines that called `remember`, and the controls that
/// did.
pub fn verdict(counts: &Counts) -> Vec<String> {
    let mut lines = Vec::new();
    let (mut defect, mut called, mut keep_asked, mut false_calls, mut control_asked) =
        (0, 0, 0, 0, 0);
    for (at, line) in LINES.iter().enumerate() {
        let outcomes = counts.get(&at).cloned().unwrap_or_default();
        let asked: u32 = outcomes.values().sum();
        let of = |name: &str| outcomes.get(name).copied().unwrap_or_default();
        defect += of(SAID_KEPT_NO_CALL);
        match line.kind {
            Kind::Keep => {
                called += of(CALLED_REMEMBER);
                keep_asked += asked;
            }
            Kind::Control => {
                false_calls += of(CALLED_REMEMBER);
                control_asked += asked;
            }
        }
        let shown: Vec<String> = outcomes
            .iter()
            .map(|(outcome, n)| format!("{outcome}={n}"))
            .collect();
        lines.push(format!(
            "says-kept line={} kind={} asked={asked}{}",
            line.name,
            line.kind.name(),
            if shown.is_empty() {
                String::new()
            } else {
                format!(": {}", shown.join(" "))
            }
        ));
    }
    lines.push(format!(
        "says-kept totals: said-kept-no-call={defect} called-remember={called}/{keep_asked} control-false-calls={false_calls}/{control_asked}"
    ));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "call_0".into(),
            name: name.into(),
            args,
            extra: None,
        }
    }

    #[test]
    fn claims_are_found_folded_and_as_whole_words() {
        for words in [
            "Got it, I've noted that your umbrella is green.",
            "Noted!",
            "Got it, noted.",
            "Noted, your sister is Lan.",
            "I noted that.",
            "I’ll remember that.",
            "I will remember your sister is Lan.",
            "All updated: no sugar.",
            "Okay, I've updated it.",
            "Được rồi, mình đã ghi nhớ.",
            "ĐÃ GHI NHỚ",
            "Mình nhớ rồi nhé.",
            "Vâng, tôi đã nhớ.",
            // Decomposed accents are composed first.
            "Đa\u{0303} ghi nho\u{031b}\u{0301}.",
            "Mình sẽ nhớ là bạn không uống cà phê.",
            "Vâng, tôi đã cập nhật rồi.",
            "Đã lưu nhé.",
        ] {
            assert!(claims_kept(words), "{words}");
        }
        for words in [
            "Got it, green it is.",
            "It's half past three.",
            "I can't remember that one.",
            "Lovely weather indeed.",
            "Annotated maps are fun.",
            "Màu xanh lá đẹp lắm.",
            // Other words once the tone marks are gone: small, asked.
            "Cái này nhỏ rồi.",
            "Tôi đã nhờ chị Lan.",
            "Bạn sẽ nhớ mà.",
            // "noted" and "nhớ" said of the person are not the lamp's claim.
            "As you noted, it's green.",
            "Bạn đã nhớ mà.",
            "Bạn nhớ rồi.",
            "Nhớ rồi nhé.",
            // Tone-stripped spellings are not the claim.
            "da ghi nho",
            "nho roi",
            "",
        ] {
            assert!(!claims_kept(words), "{words}");
        }
        assert_eq!(fold("  Đã  GHI-nhớ! "), " đã ghi nhớ ");
    }

    #[test]
    fn the_defect_is_a_claim_with_no_valid_keeping_call() {
        let remember = call("remember", json!({"text": "Sister is Lan."}));
        let rule = call("keep_rule", json!({"rule": "No sugar."}));
        assert_eq!(
            outcome(std::slice::from_ref(&remember), "Noted."),
            CALLED_REMEMBER
        );
        assert_eq!(outcome(&[rule], ""), CALLED_REMEMBER);
        // A forget then a remember in one turn keeps.
        let forget = call("forget", json!({"source_id": 7}));
        assert_eq!(outcome(&[forget.clone(), remember], ""), CALLED_REMEMBER);
        assert_eq!(outcome(&[], "Got it, I've noted that."), SAID_KEPT_NO_CALL);
        assert_eq!(
            outcome(std::slice::from_ref(&forget), "Updated it."),
            SAID_KEPT_NO_CALL
        );
        let empty = call("remember", json!({"text": " "}));
        assert_eq!(
            outcome(std::slice::from_ref(&empty), "Noted."),
            SAID_KEPT_NO_CALL
        );
        assert_eq!(outcome(&[empty], ""), BAD_ARGS);
        assert_eq!(outcome(&[call("remember", json!({}))], ""), BAD_ARGS);
        assert_eq!(outcome(&[forget], ""), "called-forget");
        assert_eq!(outcome(&[], "Got it, green."), WORDS);
    }

    #[test]
    fn each_line_is_a_fresh_session_as_quality_today() {
        let green = LINES[0].messages("C");
        assert_eq!(green.len(), 2);
        assert_eq!(
            green[1],
            net::Message::user("My umbrella is green now, not violet.")
        );
        let system = green[0].content.as_ref().and_then(|c| c.as_str()).unwrap();
        assert!(
            system.contains("The violet umbrella is in the car.") && system.contains("\"id\":7")
        );
        assert!(system.contains(super::super::tools::DAYS_NOTE));
        assert!(!system.contains(super::super::quality::MEMORY_USE_NOTE));
        let keep = LINES.iter().filter(|line| line.kind == Kind::Keep).count();
        assert_eq!((keep, LINES.len() - keep), (5, 3));
        assert!(LINES.iter().any(|line| line.said.contains("cà phê")));
    }

    #[test]
    fn the_verdict_counts_each_line_and_the_totals() {
        let mut counts = Counts::new();
        counts.insert(
            0,
            [
                (CALLED_REMEMBER.to_owned(), 2),
                (SAID_KEPT_NO_CALL.to_owned(), 1),
            ]
            .into(),
        );
        counts.insert(3, [(SAID_KEPT_NO_CALL.to_owned(), 3)].into());
        counts.insert(
            5,
            [
                (CALLED_REMEMBER.to_owned(), 1),
                ("called-the_time".to_owned(), 2),
            ]
            .into(),
        );
        counts.insert(6, [(WORDS.to_owned(), 3)].into());
        let lines = verdict(&counts);
        assert_eq!(
            lines[0],
            "says-kept line=umbrella-green kind=keep asked=3: called-remember=2 said-kept-no-call=1"
        );
        assert_eq!(
            lines[5],
            "says-kept line=control-time kind=control asked=3: called-remember=1 called-the_time=2"
        );
        assert_eq!(lines[1], "says-kept line=sister-lan kind=keep asked=0");
        assert_eq!(
            lines.last().unwrap(),
            "says-kept totals: said-kept-no-call=4 called-remember=2/6 control-false-calls=1/6"
        );
    }
}
