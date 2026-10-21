//! A correction of a thing kept earlier in the same session (#2510, arm (c),
//! `LAMP_MEMORY_REPLACES`): "remember my favourite colour is violet" is
//! kept in one turn, and "actually it's green now" follows in the next. The
//! note holds nothing, so the only id the model can name as `replaces` is
//! the one `remember`'s answer gave it. That answer is built as the voice
//! builds it ([`crate::remember::answer_for`], [`crate::remember::told_kept`])
//! for the store's `{"ok":true,"id":8}`, so the probe measures the code.
//!
//! Per line, the requests that called `remember` (as the store would take
//! it), that gave `replaces` 8, that gave any `replaces`, and that claimed a
//! keeping with no call. Counts only, never the words.
use super::super::net::{Chat, Message, ToolCall};
use serde_json::{Value, json};

/// The id the store gave the thing kept in the first turn.
pub const KEPT_ID: u64 = 8;

/// One line: the first turn's ask, what was kept, the lamp's words for it,
/// and the correction that follows.
pub struct Line {
    pub name: &'static str,
    pub asked: &'static str,
    pub kept: &'static str,
    pub answered: &'static str,
    pub said: &'static str,
}

pub const LINES: [Line; 2] = [
    Line {
        name: "kept-earlier",
        asked: "Remember my favourite colour is violet.",
        kept: "Favourite colour is violet.",
        answered: "Violet, got it.",
        said: "Actually it's green now, not violet.",
    },
    Line {
        name: "vi-kept-earlier",
        asked: "Nhớ giùm tôi là màu yêu thích của tôi là màu tím.",
        kept: "Màu yêu thích là màu tím.",
        answered: "Màu tím, mình nhớ rồi.",
        said: "À thật ra giờ là màu xanh lá rồi, không phải màu tím nữa.",
    },
];

/// What the model is told for the first turn's `remember`, as the voice
/// tells it now.
pub fn kept_answer() -> Value {
    let told = crate::remember::answer_for(
        crate::remember::REMEMBER,
        &json!({"ok": true, "id": KEPT_ID}),
        0,
        None,
    )
    .unwrap_or_default();
    crate::remember::told_kept(told, KEPT_ID, crate::remember::replaces_on())
}

impl Line {
    /// A fresh session as `quality-today` asks it, with an empty note, then
    /// the first turn (its `remember` and that answer) and the correction.
    pub fn messages(&self, character: &str) -> Vec<Message> {
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
        let call = ToolCall {
            id: "call_0".into(),
            name: crate::remember::REMEMBER.into(),
            args: json!({"text": self.kept}),
            extra: None,
        };
        vec![
            Message::system(system),
            Message::user(self.asked),
            Message::assistant_calls(None, std::slice::from_ref(&call)),
            Message::tool(call.id.clone(), kept_answer().to_string()),
            Message::assistant(self.answered),
            Message::user(self.said),
        ]
    }
}

/// One line's counts.
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub struct Tally {
    pub asked: u32,
    pub remember: u32,
    pub replaces_kept: u32,
    pub replaces_any: u32,
    pub claim_no_call: u32,
}

impl Tally {
    /// One request's calls and words counted.
    pub fn add(&mut self, calls: &[ToolCall], words: &str) {
        let remembers: Vec<&ToolCall> = calls
            .iter()
            .filter(|call| call.name == crate::remember::REMEMBER && super::says_kept::keeps(call))
            .collect();
        fn replaces(call: &ToolCall) -> Option<&Value> {
            call.args.get("replaces").filter(|id| !id.is_null())
        }
        self.asked += 1;
        self.remember += u32::from(!remembers.is_empty());
        self.replaces_kept += u32::from(
            remembers
                .iter()
                .any(|call| replaces(call).and_then(Value::as_u64) == Some(KEPT_ID)),
        );
        self.replaces_any += u32::from(remembers.iter().any(|call| replaces(call).is_some()));
        self.claim_no_call += u32::from(
            super::says_kept::outcome(calls, words) == super::says_kept::SAID_KEPT_NO_CALL,
        );
    }
}

/// Each line asked `runs` times, with every function.
pub async fn ask(chat: &Chat, character: &str, runs: u32) -> Vec<Tally> {
    let mut tallies = vec![Tally::default(); LINES.len()];
    for _ in 0..runs {
        for (line, tally) in LINES.iter().zip(tallies.iter_mut()) {
            match super::says_kept::asked(chat, &line.messages(character)).await {
                Ok((calls, words)) => tally.add(&calls, &words),
                Err(error) => println!("kept-earlier line={} failed: {error}", line.name),
            }
        }
    }
    tallies
}

/// One line a case.
pub fn verdict(tallies: &[Tally]) -> Vec<String> {
    LINES
        .iter()
        .zip(tallies)
        .map(|(line, tally)| {
            format!(
                "kept-earlier line={} asked={} remember={} replaces={KEPT_ID}:{} replaces-any={} said-kept-no-call={}",
                line.name,
                tally.asked,
                tally.remember,
                tally.replaces_kept,
                tally.replaces_any,
                tally.claim_no_call
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(args: Value) -> ToolCall {
        ToolCall {
            id: "call_1".into(),
            name: crate::remember::REMEMBER.into(),
            args,
            extra: None,
        }
    }

    #[test]
    fn the_first_turn_is_kept_and_answered_as_the_voice_answers_it() {
        let messages = LINES[0].messages("C");
        assert_eq!(messages.len(), 6);
        let system = messages[0]
            .content
            .as_ref()
            .and_then(|c| c.as_str())
            .unwrap();
        assert!(system.contains("\"facts\":[]") && !system.contains("violet"));
        assert_eq!(
            messages[3],
            Message::tool("call_0", kept_answer().to_string())
        );
        assert_eq!(
            messages[5],
            Message::user("Actually it's green now, not violet.")
        );
        // What the voice tells for the store's answer, with the switch as set.
        let told = kept_answer();
        assert_eq!(told["remembered"], true);
        assert_eq!(
            told.get("id").is_some(),
            crate::remember::replaces_on(),
            "{told}"
        );
        assert!(LINES[1].said.contains("xanh lá"));
    }

    #[test]
    fn a_tally_counts_the_kept_id_any_replaces_and_a_claim_with_no_call() {
        let mut tally = Tally::default();
        tally.add(&[call(json!({"text":"Green now.","replaces":8}))], "");
        tally.add(&[call(json!({"text":"Green now.","replaces":3}))], "");
        tally.add(&[call(json!({"text":"Green now.","replaces":null}))], "");
        tally.add(&[call(json!({"text":"Green now."}))], "");
        tally.add(&[], "Got it, I've updated that.");
        tally.add(&[call(json!({"text":" ","replaces":8}))], "");
        assert_eq!(
            tally,
            Tally {
                asked: 6,
                remember: 4,
                replaces_kept: 1,
                replaces_any: 2,
                claim_no_call: 1,
            }
        );
        assert_eq!(
            verdict(&[tally, Tally::default()]),
            [
                "kept-earlier line=kept-earlier asked=6 remember=4 replaces=8:1 replaces-any=2 said-kept-no-call=1",
                "kept-earlier line=vi-kept-earlier asked=0 remember=0 replaces=8:0 replaces-any=0 said-kept-no-call=0",
            ]
        );
    }
}
