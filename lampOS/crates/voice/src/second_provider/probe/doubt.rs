//! A keep the model's own words doubt (#7739, `probe-tools ... --doubt`,
//! `LAMP_MEMORY_UNSURE_NOT_KEPT`): speech can't make that doubt on a lamp
//! on demand, so this injects it and asks the real model for what follows.
//!
//! One line, "Remember that my favourite flower is a renunculus.", in two
//! cases, each a fresh session as [`super::kept_earlier`] builds one:
//!
//! - `own-round`: the model's round said "I may have misheard that" beside
//!   its `remember`, so the call is not run and its answer is the product's
//!   not-kept note (`remember::doubted::not_kept_note`'s shape). The next
//!   round is asked.
//! - `round-after`: a bare `remember` was kept, then the next round doubted
//!   it beside a `recall` (finding nothing), so the keep is taken back and
//!   the model is told the take-back note before its next round, which is
//!   asked. A doubting round with no other call ends in the lamp's own
//!   take-back line, written by the code, not the model, so not asked here.
//!
//! Each wording ([`Wording`]) declares `remember` with its description and
//! answers with its notes: `new`, this head's; `old`, `ac50913437`'s; `off`,
//! the switch off (asked to repeat, held for a yes).
//!
//! Each reply is judged by the same model in a second request with no
//! functions ([`JUDGE`]): whether it says the thing was not kept, whether
//! it asks the person anything, and whether it says it was kept. A literal
//! `?` is counted too, as a crude extra, and a `remember` the reply calls
//! again. Counts only, never the words.
use super::super::net::{self, Chat, Message, ToolCall};
use serde_json::{Value, json};

/// What the person said.
pub const SAID: &str = "Remember that my favourite flower is a renunculus.";
/// What the model's `remember` would keep.
pub const KEPT: &str = "Favourite flower is renunculus.";
/// The words of the `own-round` round, beside its `remember`.
pub const OWN_ROUND_WORDS: &str = "Renunculus, I may have misheard that.";
/// The words of the `round-after` round, beside its `recall`, with no
/// question of their own.
pub const ROUND_AFTER_WORDS: &str = "I want to be sure I heard that right.";

/// `REMEMBER_DESCRIBED_UNSURE` at `ac50913437`.
pub const OLD_REMEMBER_DESCRIBED_UNSURE: &str = "Call this when someone asks you to remember something or keep it in mind for later. \
                          Give what to keep as one short sentence that will still make sense weeks from now, \
                          with their own words for names and numbers. If you are not sure you heard a key word \
                          right (a name, a number, or the thing itself), do not call this: say in a few words, \
                          with no question, that you didn't catch it well enough to keep it. Else say back \
                          in a few words what you kept, so they can put you right if you misheard; \
                          never also ask whether to keep it.";
/// `NOT_KEPT_NO_ASK` at `ac50913437`.
pub const OLD_NOT_KEPT_NO_ASK: &str = "Not kept: in this same reply you doubted what you heard, so nothing is kept. Tell them so in a few words, with no question, for example \"I didn't catch that well enough to keep it.\" Never ask \"did you say ...?\" or ask them to say it again, and never say it was kept.";
/// `UNDONE_NO_ASK_NOTE` at `ac50913437`.
pub const OLD_UNDONE_NO_ASK_NOTE: &str = "(For you to know: what you kept just before is taken back, since your words doubt what you heard, and it stays not kept. Tell them so plainly in a few words, with no question, for example \"Sorry, I didn't keep that after all.\" Ask nothing more about it, and never say it is kept.)";

/// Which words the model is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wording {
    New,
    Old,
    Off,
}

impl Wording {
    pub const ALL: [Self; 3] = [Self::New, Self::Old, Self::Off];

    pub fn name(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Old => "old",
            Self::Off => "off",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|wording| wording.name() == name)
    }

    /// `remember`'s description's own part.
    pub fn described(self) -> &'static str {
        match self {
            Self::New => crate::remember::REMEMBER_DESCRIBED_UNSURE,
            Self::Old => OLD_REMEMBER_DESCRIBED_UNSURE,
            Self::Off => crate::remember::REMEMBER_DESCRIBED,
        }
    }

    /// What a `remember` its own round doubts is answered.
    pub fn not_kept(self) -> &'static str {
        match self {
            Self::New => crate::remember::doubted::NOT_KEPT_NO_ASK,
            Self::Old => OLD_NOT_KEPT_NO_ASK,
            Self::Off => crate::remember::doubted::NOT_KEPT,
        }
    }

    /// What the model is told before the round after a take-back.
    pub fn undone(self) -> &'static str {
        match self {
            Self::New => crate::remember::doubted::UNDONE_NO_ASK_NOTE,
            Self::Old => OLD_UNDONE_NO_ASK_NOTE,
            Self::Off => crate::remember::doubted::UNDONE_NOTE,
        }
    }

    /// `tools` with `remember`'s own part of its description this
    /// wording's, whichever it was declared with; the rest as they are.
    pub fn declared(self, mut tools: Vec<lamp_gemini::Tool>) -> Vec<lamp_gemini::Tool> {
        for tool in tools
            .iter_mut()
            .filter(|tool| tool.name == crate::remember::REMEMBER)
        {
            for was in [
                crate::remember::REMEMBER_DESCRIBED_UNSURE,
                crate::remember::REMEMBER_DESCRIBED,
            ] {
                if tool.description.contains(was) {
                    tool.description = tool.description.replacen(was, self.described(), 1);
                    break;
                }
            }
        }
        tools
    }
}

/// Where the doubt is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    OwnRound,
    RoundAfter,
}

impl Case {
    pub const ALL: [Self; 2] = [Self::OwnRound, Self::RoundAfter];

    pub fn name(self) -> &'static str {
        match self {
            Self::OwnRound => "own-round",
            Self::RoundAfter => "round-after",
        }
    }

    /// The request for the round that follows the doubt, as the second
    /// route builds it.
    pub fn messages(self, wording: Wording, character: &str) -> Vec<Message> {
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
        let remember = ToolCall {
            id: "call_0".into(),
            name: crate::remember::REMEMBER.into(),
            args: json!({"text": KEPT}),
            extra: None,
        };
        let mut messages = vec![Message::system(system), Message::user(SAID)];
        match self {
            Self::OwnRound => {
                messages.push(Message::assistant_calls(
                    Some(OWN_ROUND_WORDS.into()),
                    std::slice::from_ref(&remember),
                ));
                messages.push(Message::tool(
                    remember.id.clone(),
                    json!({"remembered": false, "note": wording.not_kept()}).to_string(),
                ));
            }
            Self::RoundAfter => {
                messages.push(Message::assistant_calls(
                    None,
                    std::slice::from_ref(&remember),
                ));
                messages.push(Message::tool(
                    remember.id.clone(),
                    super::kept_earlier::kept_answer().to_string(),
                ));
                let recall = ToolCall {
                    id: "call_1".into(),
                    name: crate::remember::RECALL.into(),
                    args: json!({"about": "favourite flower"}),
                    extra: None,
                };
                messages.push(Message::assistant_calls(
                    Some(ROUND_AFTER_WORDS.into()),
                    std::slice::from_ref(&recall),
                ));
                messages.push(Message::tool(
                    recall.id.clone(),
                    json!({"found": []}).to_string(),
                ));
                messages.push(Message::system(wording.undone()));
            }
        }
        messages
    }

    /// What the lamp said before the reply, for the judge.
    fn said_before(self) -> &'static str {
        match self {
            Self::OwnRound => OWN_ROUND_WORDS,
            Self::RoundAfter => ROUND_AFTER_WORDS,
        }
    }
}

/// The judge's instruction.
pub const JUDGE: &str = "You read one reply a desk lamp's voice assistant said to a person who asked it to remember something. \
Judge only the reply, by its meaning in any language. Answer with JSON only, no other words: \
{\"not_kept\": true or false, \"asks\": true or false, \"kept\": true or false}. \
not_kept: the reply tells them the thing was not kept, saved or remembered, or not caught well enough to keep. \
asks: the reply asks the person anything or asks them to do anything about it: a question, or to repeat, spell, confirm or say more. \
kept: the reply says or implies the thing is now kept, saved, noted or remembered.";

/// The judge's request for one reply.
pub fn judge_messages(case: Case, reply: &str) -> Vec<Message> {
    vec![
        Message::system(JUDGE),
        Message::user(format!(
            "The person said: {SAID}\nThe lamp said first: {}\nThe reply to judge: {reply}",
            case.said_before()
        )),
    ]
}

/// The judge's three answers, or `None` when its answer isn't them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Judged {
    pub not_kept: bool,
    pub asks: bool,
    pub kept: bool,
}

pub fn judged(answer: &str) -> Option<Judged> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    let value: Value = serde_json::from_str(answer.get(start..=end)?).ok()?;
    let flag = |name: &str| value.get(name).and_then(Value::as_bool);
    Some(Judged {
        not_kept: flag("not_kept")?,
        asks: flag("asks")?,
        kept: flag("kept")?,
    })
}

/// One case and wording's counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub asked: u32,
    pub not_kept: u32,
    pub asks: u32,
    pub question_marks: u32,
    pub kept: u32,
    pub kept_again: u32,
    pub unjudged: u32,
}

impl Tally {
    /// One reply counted: its calls, its words and the judge's answer.
    pub fn add(&mut self, calls: &[ToolCall], words: &str, judge: Option<Judged>) {
        self.asked += 1;
        self.question_marks += u32::from(words.contains('?') || words.contains('\u{ff1f}'));
        self.kept_again +=
            u32::from(calls.iter().any(|call| {
                call.name == crate::remember::REMEMBER && super::says_kept::keeps(call)
            }));
        match judge {
            Some(judge) => {
                self.not_kept += u32::from(judge.not_kept);
                self.asks += u32::from(judge.asks);
                self.kept += u32::from(judge.kept);
            }
            None => self.unjudged += 1,
        }
    }

    pub fn line(&self, wording: Wording, case: Case) -> String {
        format!(
            "doubt wording={} case={} asked={} says-not-kept={} asks={} question-marks={} says-kept={} remember-again={} unjudged={}",
            wording.name(),
            case.name(),
            self.asked,
            self.not_kept,
            self.asks,
            self.question_marks,
            self.kept,
            self.kept_again,
            self.unjudged
        )
    }
}

/// One case and wording asked `runs` times, each reply judged.
pub async fn ask(
    chat: &Chat,
    judge: &Chat,
    character: &str,
    wording: Wording,
    case: Case,
    runs: u32,
) -> Tally {
    let mut tally = Tally::default();
    let messages = case.messages(wording, character);
    for _ in 0..runs {
        let (calls, words) = match super::says_kept::asked(chat, &messages).await {
            Ok(answer) => answer,
            Err(error) => {
                println!(
                    "doubt wording={} case={} failed: {error}",
                    wording.name(),
                    case.name()
                );
                continue;
            }
        };
        let judge = if words.trim().is_empty() {
            // Nothing said: nothing told, asked or claimed.
            Some(Judged::default())
        } else {
            super::says_kept::asked(judge, &judge_messages(case, &words))
                .await
                .ok()
                .and_then(|(_, answer)| judged(&answer))
        };
        tally.add(&calls, &words, judge);
    }
    tally
}

/// `probe-tools PROVIDER_CONFIG SECOND_CONFIG --doubt [--wording
/// new|old|off] [--times N]`: the route's DeepSeek, chosen as
/// [`super::tools::run`] chooses it, declaring the spoken turn's functions.
pub async fn run(
    provider_config: &std::path::Path,
    second_config: &std::path::Path,
    runs: u32,
    wordings: &[Wording],
) -> std::io::Result<()> {
    use std::{io, sync::Arc};
    crate::remember::switch_replaces(
        std::env::var(crate::remember::REPLACES_ENVIRONMENT).as_deref() == Ok("on"),
    );
    let provider = crate::config::ProviderConfig::load(provider_config)?;
    let character = provider.instruction(false)?;
    let config = crate::second_config::load_from_path(second_config)?
        .with_environment(|name| std::env::var_os(name))?;
    let credentials = std::env::var_os(crate::second_config::CREDENTIALS_ENV);
    let (base, key) = super::tools::deepseek_route(&config, credentials.as_deref())?;
    let model = &config.hosted.deepseek_model;
    let actions = std::env::var_os(crate::tools::ACTIONS_ENVIRONMENT)
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| "/run/lampos-actions".into(), std::path::PathBuf::from);
    let tools = super::tools::functions(Some(actions));
    let count = tools.len();
    let chat = |tools: Vec<lamp_gemini::Tool>| {
        let declare: net::Declare = Arc::new(move || tools.clone());
        Chat::deepseek_at(&base, model, key.expose(), declare, false).map_err(io::Error::other)
    };
    let judge = chat(Vec::new())?;
    println!(
        "tools: a keep the model's own words doubt (#7739), DeepSeek's {model} {}, thinking off, {count} functions, {runs} runs of each case and wording, judged by the same model",
        super::super::worker::deepseek_where(&config.hosted),
    );
    for &wording in wordings {
        let asking = chat(wording.declared(tools.clone()))?;
        for case in Case::ALL {
            let tally = ask(&asking, &judge, &character, wording, case, runs).await;
            println!("{}", tally.line(wording, case));
        }
    }
    println!(
        "doubt: a round-after doubt with no other call ends in the lamp's own take-back line, written by the code, so it is not asked here"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(message: &Message) -> &str {
        message.words().unwrap_or_default()
    }

    #[test]
    fn the_injected_words_are_a_doubt_the_product_acts_on_and_ask_nothing() {
        for words in [OWN_ROUND_WORDS, ROUND_AFTER_WORDS] {
            assert!(
                crate::remember::doubted::doubts_what_was_heard(words),
                "{words}"
            );
            assert!(!words.contains('?'), "{words}");
        }
    }

    #[test]
    fn each_wording_answers_with_its_own_notes() {
        assert_eq!(
            Wording::New.not_kept(),
            crate::remember::doubted::not_kept_note(true)
        );
        assert_eq!(
            Wording::Off.not_kept(),
            crate::remember::doubted::not_kept_note(false)
        );
        assert_ne!(Wording::Old.not_kept(), Wording::New.not_kept());
        assert_ne!(Wording::Old.undone(), Wording::New.undone());
        assert_ne!(Wording::Old.described(), Wording::New.described());
        for wording in Wording::ALL {
            assert_eq!(Wording::parse(wording.name()), Some(wording));
            let own = Case::OwnRound.messages(wording, "C");
            assert_eq!(own.len(), 4);
            assert_eq!(text(&own[1]), SAID);
            assert_eq!(text(&own[2]), OWN_ROUND_WORDS);
            assert_eq!(own[2].tool_calls.len(), 1);
            let told: Value = serde_json::from_str(text(&own[3])).unwrap();
            assert_eq!(told["remembered"], false);
            assert_eq!(told["note"], wording.not_kept());
            let after = Case::RoundAfter.messages(wording, "C");
            assert_eq!(after.len(), 7);
            assert!(after[2].content.is_none());
            assert_eq!(
                after[2].tool_calls[0]["function"]["name"],
                crate::remember::REMEMBER
            );
            let kept: Value = serde_json::from_str(text(&after[3])).unwrap();
            assert_eq!(kept["remembered"], true);
            assert_eq!(text(&after[4]), ROUND_AFTER_WORDS);
            assert_eq!(
                after[4].tool_calls[0]["function"]["name"],
                crate::remember::RECALL
            );
            assert_eq!(after[6].role, "system");
            assert_eq!(text(&after[6]), wording.undone());
        }
        assert_eq!(Wording::parse("both"), None);
    }

    #[test]
    fn remember_alone_takes_the_wordings_description() {
        let tools = super::super::cache::functions();
        assert!(
            tools
                .iter()
                .any(|tool| tool.name == crate::remember::REMEMBER)
        );
        for wording in Wording::ALL {
            let declared = wording.declared(tools.clone());
            assert_eq!(declared.len(), tools.len());
            for (was, now) in tools.iter().zip(&declared) {
                if was.name == crate::remember::REMEMBER {
                    assert!(now.description.contains(wording.described()));
                    assert!(now.description.len() <= 1024);
                } else {
                    assert_eq!(was.description, now.description, "{}", was.name);
                }
            }
        }
    }

    #[test]
    fn the_judge_is_read_from_its_json_and_counted() {
        assert_eq!(
            judged("```json\n{\"not_kept\": true, \"asks\": false, \"kept\": false}\n```"),
            Some(Judged {
                not_kept: true,
                asks: false,
                kept: false
            })
        );
        assert_eq!(judged("{\"not_kept\": true}"), None);
        assert_eq!(judged("no"), None);
        let mut tally = Tally::default();
        let remember = ToolCall {
            id: "1".into(),
            name: crate::remember::REMEMBER.into(),
            args: json!({"text": KEPT}),
            extra: None,
        };
        tally.add(
            &[],
            "I didn't catch that well enough to keep it.",
            judged("{\"not_kept\":true,\"asks\":false,\"kept\":false}"),
        );
        tally.add(
            &[remember],
            "Could you say it again?",
            judged("{\"not_kept\":false,\"asks\":true,\"kept\":true}"),
        );
        tally.add(&[], "Hm.", None);
        assert_eq!(
            tally.line(Wording::New, Case::OwnRound),
            "doubt wording=new case=own-round asked=3 says-not-kept=1 asks=1 question-marks=1 says-kept=1 remember-again=1 unjudged=1"
        );
        let asked = judge_messages(Case::RoundAfter, "Sorry.");
        assert_eq!(text(&asked[0]), JUDGE);
        assert!(text(&asked[1]).contains(ROUND_AFTER_WORDS) && text(&asked[1]).ends_with("Sorry."));
    }
}
