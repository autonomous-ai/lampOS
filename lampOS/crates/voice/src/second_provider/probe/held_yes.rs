//! The yes to a held one-face forget (#8038, #8041, `probe-tools ...
//! --held-yes`): since #7960's ruling only the model's own `forget` call on
//! the yes line runs it (`second_provider::Taken::Agrees`), so what the held
//! ask tells the model decides whether a yes forgets anything. On lamp-06c1
//! (#8041's ROW, 2 of 2) the old words, "Do not call forget again for their
//! yes", left the model calling nothing.
//!
//! Each run is a fresh session, as the second route builds one: the person
//! asks to forget a thing, the model's `forget` is answered with the held
//! ask ([`Wording`]), the lamp asks "Shall I forget …?", and the person
//! answers. Asked in English and Vietnamese, with a yes and with a no.
//!
//! It prints counts only: for each wording, language and answer, how many
//! replies called `forget`, and how many called anything else. Never the
//! character, a key, a URL or what the model said.
use super::super::net::{self, Chat, Message, ToolCall};
use serde_json::{Value, json};

/// `remember::one_face_forget_asks`'s held words at `e8e768408a`, before
/// #8041: `{}` is the subject.
pub const OLD_ASKS: &str = "Not done yet: I couldn't see who was asking, so this waits for their yes. Ask them once, in a few words and in the language they spoke, whether you should forget what you kept about {} (for example \"Shall I forget what you kept about your bike lock code?\" or \"Mình có nên quên điều đã giữ về mã khóa xe của bạn không?\"), and then stop. Do not say what was kept, its details, or anything you know about it. Only their yes within 30 seconds runs it; a no or nothing keeps it. Do not call forget again for their yes, and do not say it is forgotten.";

/// Which held words the model is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wording {
    New,
    Old,
}

impl Wording {
    pub const ALL: [Self; 2] = [Self::New, Self::Old];

    pub fn name(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Old => "old",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|wording| wording.name() == name)
    }

    /// The held forget's answer, as the product's for `subject`.
    pub fn asks(self, subject: &str) -> Value {
        match self {
            Self::New => crate::remember::one_face_forget_asks(subject),
            Self::Old => json!({
                "forgotten": false,
                "held": OLD_ASKS.replacen("{}", subject, 1),
            }),
        }
    }
}

/// One language's lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Language {
    pub name: &'static str,
    pub said: &'static str,
    pub subject: &'static str,
    pub asked: &'static str,
    pub yes: &'static str,
    pub no: &'static str,
}

pub const LANGUAGES: [Language; 2] = [
    Language {
        name: "en",
        said: "Lamp, forget my kayak paddle.",
        subject: "kayak paddle",
        asked: "Shall I forget what you kept about your kayak paddle?",
        yes: "Yes.",
        no: "No.",
    },
    Language {
        name: "vi",
        said: "Đèn ơi, quên mái chèo kayak của tôi đi.",
        subject: "mái chèo kayak",
        asked: "Mình có nên quên điều đã giữ về mái chèo kayak của bạn không?",
        yes: "Có.",
        no: "Không.",
    },
];

/// The request for the answer's round, as the second route builds it.
pub fn messages(wording: Wording, language: Language, yes: bool, character: &str) -> Vec<Message> {
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
    let forget = ToolCall {
        id: "call_0".into(),
        name: crate::remember::FORGET.into(),
        args: json!({"about": language.subject}),
        extra: None,
    };
    vec![
        Message::system(system),
        Message::user(language.said),
        Message::assistant_calls(None, std::slice::from_ref(&forget)),
        Message::tool(
            forget.id.clone(),
            wording.asks(language.subject).to_string(),
        ),
        Message::assistant(language.asked),
        Message::user(if yes { language.yes } else { language.no }),
    ]
}

/// One wording, language and answer's counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub asked: u32,
    pub forget: u32,
    pub other: u32,
    pub failed: u32,
}

impl Tally {
    pub fn add(&mut self, calls: &[ToolCall]) {
        self.asked += 1;
        self.forget += u32::from(
            calls
                .iter()
                .any(|call| call.name == crate::remember::FORGET),
        );
        self.other += u32::from(
            calls
                .iter()
                .any(|call| call.name != crate::remember::FORGET),
        );
    }

    pub fn line(&self, wording: Wording, language: Language, yes: bool) -> String {
        format!(
            "held-yes wording={} lang={} answer={} asked={} forget={} other-call={} failed={}",
            wording.name(),
            language.name,
            if yes { "yes" } else { "no" },
            self.asked,
            self.forget,
            self.other,
            self.failed
        )
    }
}

/// `probe-tools PROVIDER_CONFIG SECOND_CONFIG --held-yes [--wording
/// new|old] [--times N]`: the route's DeepSeek, chosen as
/// [`super::tools::run`] chooses it, declaring the spoken turn's functions.
pub async fn run(
    provider_config: &std::path::Path,
    second_config: &std::path::Path,
    runs: u32,
    wordings: &[Wording],
) -> std::io::Result<()> {
    use std::{io, sync::Arc};
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
    let declare: net::Declare = Arc::new(move || tools.clone());
    let chat =
        Chat::deepseek_at(&base, model, key.expose(), declare, false).map_err(io::Error::other)?;
    println!(
        "tools: the yes to a held one-face forget (#8041), DeepSeek's {model} {}, thinking off, {count} functions, {runs} runs of each wording, language and answer",
        super::super::worker::deepseek_where(&config.hosted),
    );
    for &wording in wordings {
        for language in LANGUAGES {
            for yes in [true, false] {
                let messages = messages(wording, language, yes, &character);
                let mut tally = Tally::default();
                for _ in 0..runs {
                    match super::says_kept::asked(&chat, &messages).await {
                        Ok((calls, _)) => tally.add(&calls),
                        Err(_) => tally.failed += 1,
                    }
                }
                println!("{}", tally.line(wording, language, yes));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(message: &Message) -> &str {
        message.words().unwrap_or_default()
    }

    #[test]
    fn each_wording_answers_the_forget_with_its_own_held_words() {
        let new = Wording::New.asks("kayak paddle");
        assert_eq!(new, crate::remember::one_face_forget_asks("kayak paddle"));
        let old = Wording::Old.asks("kayak paddle");
        assert!(
            old["held"]
                .as_str()
                .unwrap()
                .contains("Do not call forget again")
        );
        assert!(old["held"].as_str().unwrap().contains("kayak paddle"));
        assert_ne!(new, old);
        for wording in Wording::ALL {
            assert_eq!(Wording::parse(wording.name()), Some(wording));
            for language in LANGUAGES {
                for yes in [true, false] {
                    let asked = messages(wording, language, yes, "C");
                    assert_eq!(asked.len(), 6);
                    assert_eq!(text(&asked[1]), language.said);
                    assert_eq!(asked[2].tool_calls.len(), 1);
                    let told: Value = serde_json::from_str(text(&asked[3])).unwrap();
                    assert_eq!(told, wording.asks(language.subject));
                    assert_eq!(text(&asked[4]), language.asked);
                    assert_eq!(
                        text(&asked[5]),
                        if yes { language.yes } else { language.no }
                    );
                }
            }
        }
    }

    #[test]
    fn a_reply_counts_its_forget_and_other_calls() {
        let call = |name: &str| ToolCall {
            id: "c".into(),
            name: name.into(),
            args: json!({}),
            extra: None,
        };
        let mut tally = Tally::default();
        tally.add(&[call(crate::remember::FORGET)]);
        tally.add(&[]);
        tally.add(&[call(crate::remember::RECALL)]);
        assert_eq!(
            tally,
            Tally {
                asked: 3,
                forget: 1,
                other: 1,
                failed: 0
            }
        );
    }
}
