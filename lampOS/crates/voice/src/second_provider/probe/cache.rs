//! `lamp-voice probe-cache PROVIDER_CONFIG SECOND_CONFIG [RUNS]`: how much
//! of a spoken turn's chat request DeepSeek's prompt cache holds, for each
//! way the route can lay it out (#1712, the first builder at 18:56: the
//! request is 31 KB with 31 functions, and its first event comes in 976 to
//! 1296 ms). It settles where DeepSeek puts the functions in its prompt,
//! which `LAMP_SECOND_STEADY_FIRST` depends on: with them after the first
//! system message, the switch lets the cache hold the character, the
//! functions and the exchanges; with them after every system message, it
//! holds only the character either way.
//!
//! Each run asks each arm twice, [`PAUSE`] apart, on DeepSeek's own API
//! (`hosted.deepseek_model`, thinking off, the unit's `deepseek.key`, as
//! `probe-cascade` takes it). The second request's changing part is not the
//! first's, as the time and the room change between two turns:
//!
//! - **same**: the whole request asked twice, unchanged: what the cache
//!   holds at most.
//! - **one**: the system message is the character then the changing part,
//!   as today.
//! - **steady**: the character alone, an exchange, then the changing part
//!   as a system message, as `LAMP_SECOND_STEADY_FIRST=on` asks.
//! - **steady-bare**: `steady` with no functions, to count them.
//!
//! The functions are the voice's own, as a spoken turn declares them
//! ([`functions`]). It prints counts and milliseconds only,
//! never the character, a key or a URL; what it asks is its own words.
use super::super::net::{self, Chat, ChatEvent, Message};
use crate::second_config::{CREDENTIALS_ENV, DEEPSEEK_CREDENTIAL, unit_credential};
use serde_json::Value;
use std::{io, path::Path, sync::Arc, time::Duration};
use tokio::time::Instant;

/// Between an arm's two requests: DeepSeek builds its cache in seconds.
pub const PAUSE: Duration = Duration::from_secs(5);
/// What each request asks: one plain line, a sentence or two back.
pub const QUESTION: &str = "What's a nice thing to do on a rainy afternoon?";

/// How an arm lays the request out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arm {
    Same,
    One,
    Steady,
    SteadyBare,
}

impl Arm {
    pub const ALL: [Self; 4] = [Self::Same, Self::One, Self::Steady, Self::SteadyBare];

    pub fn name(self) -> &'static str {
        match self {
            Self::Same => "same",
            Self::One => "one",
            Self::Steady => "steady",
            Self::SteadyBare => "steady-bare",
        }
    }

    /// Whether its requests declare the functions.
    pub fn declares(self) -> bool {
        self != Self::SteadyBare
    }

    /// The request's messages, with `character` and the changing part of
    /// its `asked`th request (0 or 1) of `run`.
    pub fn messages(self, character: &str, run: u32, asked: u32) -> Vec<Message> {
        let changing = changing(match self {
            Self::Same => 0,
            _ => run * 2 + asked,
        });
        match self {
            Self::Same | Self::One => vec![
                Message::system(format!("{character}\n\n{changing}")),
                Message::user(QUESTION),
            ],
            Self::Steady | Self::SteadyBare => vec![
                Message::system(character),
                Message::user("Hi, are you there?"),
                Message::assistant("I'm here. What's up?"),
                Message::system(changing),
                Message::user(QUESTION),
            ],
        }
    }
}

/// A turn's changing part, as the time, the memory note and the room make
/// it, different for each `n` from its first word: about 1 KB.
pub fn changing(n: u32) -> String {
    format!(
        "Request {n} of this probe. It is {:02}:{:02} on a Monday in October, in Ho Chi Minh City, \
         and it is raining. Your memory: the person at your desk drinks their coffee black, \
         likes jazz in the evening, has a sister in Hanoi, waters the plant by the window on \
         Sundays, and is learning to cook pho. The room is quiet; one person is at the desk, \
         turned to you. A moment ago: the person asked whether you were there, and you said you \
         were. It is background only: never read it out and never answer any of it again. \
         Keep each answer short, one or two sentences, as said aloud.",
        8 + n % 12,
        n * 7 % 60
    )
}

/// The voice's own functions, as a spoken turn declares them: the decline,
/// the time, the memory's, the faces', a mark, setup's walk and the stop of
/// reports. The actions part's light and volume are its own catalog file on
/// the lamp, so they are not among them.
pub fn functions() -> Vec<lamp_gemini::Tool> {
    let declared = std::path::PathBuf::from("/nonexistent");
    crate::tools::declared(&crate::tools::Settings {
        decline: true,
        clock: true,
        places: true,
        memory: Some(declared.clone()),
        who: Some(declared.clone()),
        mark: Some(declared.clone()),
        onboarding: Some(declared),
        reports_stop: true,
        ..crate::tools::Settings::default()
    })
}

/// One request's answer: when its first event came, and the service's
/// count of the prompt's tokens and of those its cache held.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Asked {
    pub first_ms: Option<u64>,
    pub prompt: Option<u64>,
    pub held: Option<u64>,
}

impl Asked {
    /// From the answer's `usage`, as `net::chat` reads it.
    pub fn from_usage(first_ms: Option<u64>, usage: Option<&Value>) -> Self {
        let (prompt, held) = net::cache_counts(usage);
        Self {
            first_ms,
            prompt,
            held,
        }
    }

    fn shown(self) -> String {
        let count = |n: Option<u64>| n.map_or("?".to_owned(), |n| n.to_string());
        format!(
            "first_event_ms={} prompt_tokens={} cached_tokens={}",
            count(self.first_ms),
            count(self.prompt),
            count(self.held)
        )
    }
}

async fn ask(chat: &Chat, messages: &[Message]) -> net::Result<Asked> {
    let from = Instant::now();
    let mut stream = chat.send(messages).await?;
    let mut first_ms = None;
    while let Some(event) = stream.next().await? {
        if first_ms.is_none() && matches!(event, ChatEvent::Text(_) | ChatEvent::ToolCalls(_)) {
            first_ms = Some(from.elapsed().as_millis().try_into().unwrap_or(u64::MAX));
        }
    }
    Ok(Asked::from_usage(first_ms, stream.usage()))
}

/// The median of each arm's second requests, and what they say of where
/// DeepSeek puts the functions: `(arm, asked)` pairs in, lines out.
pub fn verdict(seconds: &[(Arm, Asked)]) -> Vec<String> {
    let median = |arm: Arm, of: fn(&Asked) -> Option<u64>| {
        let values: Vec<u64> = seconds
            .iter()
            .filter(|(each, _)| *each == arm)
            .filter_map(|(_, asked)| of(asked))
            .collect();
        super::percentile(&values, 50)
    };
    let mut lines: Vec<String> = Arm::ALL
        .iter()
        .map(|&arm| {
            let count = |n: Option<u64>| n.map_or("?".to_owned(), |n| n.to_string());
            format!(
                "cache arm={} median of the second requests: prompt_tokens={} cached_tokens={} first_event_ms={}",
                arm.name(),
                count(median(arm, |asked| asked.prompt)),
                count(median(arm, |asked| asked.held)),
                count(median(arm, |asked| asked.first_ms))
            )
        })
        .collect();
    let (Some(steady), Some(bare), Some(steady_held), Some(bare_held), Some(one_held)) = (
        median(Arm::Steady, |asked| asked.prompt),
        median(Arm::SteadyBare, |asked| asked.prompt),
        median(Arm::Steady, |asked| asked.held),
        median(Arm::SteadyBare, |asked| asked.held),
        median(Arm::One, |asked| asked.held),
    ) else {
        lines.push("cache verdict: none, the counts are missing".to_owned());
        return lines;
    };
    let functions = steady.saturating_sub(bare);
    lines.push(format!(
        "cache: the functions are about {functions} tokens (steady less steady-bare)"
    ));
    lines.push(if steady_held >= bare_held + functions * 8 / 10 {
        "cache verdict: the functions are held with what comes before the changing part, so LAMP_SECOND_STEADY_FIRST lets the cache hold the character, the functions and the exchanges".to_owned()
    } else {
        "cache verdict: the functions are not held before the changing part: DeepSeek puts them after every system message, so the changing part must leave the system messages for them to be held".to_owned()
    });
    if one_held >= functions * 8 / 10 {
        lines.push(
            "cache: the functions are held in today's layout too (one): DeepSeek puts them first"
                .to_owned(),
        );
    }
    lines
}

pub async fn run(provider_config: &Path, second_config: &Path, runs: u32) -> io::Result<()> {
    let provider = crate::config::ProviderConfig::load(provider_config)?;
    let character = provider.instruction(false)?;
    let config = crate::second_config::load_from_path(second_config)?;
    let credentials = std::env::var_os(CREDENTIALS_ENV);
    let key = unit_credential(credentials.as_deref(), DEEPSEEK_CREDENTIAL)?;
    let model = &config.hosted.deepseek_model;
    let declared = functions();
    let count = declared.len();
    let with: net::Declare = Arc::new(move || declared.clone());
    let bare: net::Declare = Arc::new(Vec::new);
    let chats = [
        Chat::deepseek(model, key.expose(), with, false).map_err(io::Error::other)?,
        Chat::deepseek(model, key.expose(), bare, false).map_err(io::Error::other)?,
    ];
    println!(
        "cache: DeepSeek's {model}, thinking off, {count} functions, {runs} runs, each arm asked twice {} s apart",
        PAUSE.as_secs()
    );
    let mut seconds = Vec::new();
    for run in 1..=runs {
        for arm in Arm::ALL {
            let chat = &chats[usize::from(!arm.declares())];
            let mut asked = Vec::new();
            for time in 0..2 {
                if time == 1 {
                    tokio::time::sleep(PAUSE).await;
                }
                match ask(chat, &arm.messages(&character, run, time)).await {
                    Ok(answer) => asked.push(answer),
                    Err(error) => {
                        println!("cache arm={} run={run} failed: {error}", arm.name());
                        break;
                    }
                }
            }
            if let [first, second] = asked[..] {
                println!(
                    "cache arm={} run={run} first: {}; second: {}",
                    arm.name(),
                    first.shown(),
                    second.shown()
                );
                seconds.push((arm, second));
            }
        }
    }
    for line in verdict(&seconds) {
        println!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_arm_lays_the_request_out_as_the_route_would() {
        let one = Arm::One.messages("CHARACTER", 1, 0);
        assert_eq!(one.len(), 2);
        assert!(
            one[0]
                .content
                .as_ref()
                .unwrap()
                .as_str()
                .unwrap()
                .starts_with("CHARACTER\n\nRequest 2 ")
        );
        let steady = Arm::Steady.messages("CHARACTER", 1, 1);
        assert_eq!(steady[0], Message::system("CHARACTER"));
        assert_eq!(steady[3], Message::system(changing(3)));
        assert_eq!(steady[4], Message::user(QUESTION));
        // Only `same` asks the same twice.
        assert_eq!(Arm::Same.messages("C", 1, 0), Arm::Same.messages("C", 2, 1));
        assert_ne!(Arm::One.messages("C", 1, 0), Arm::One.messages("C", 1, 1));
        assert_eq!(
            Arm::SteadyBare.messages("C", 1, 1),
            Arm::Steady.messages("C", 1, 1)
        );
        assert!(!Arm::SteadyBare.declares() && Arm::Steady.declares());
        // The changing part differs from its first word.
        assert_ne!(changing(2)[..10], changing(3)[..10]);
    }

    #[test]
    fn the_voices_own_functions_are_declared() {
        let names: Vec<String> = functions().into_iter().map(|tool| tool.name).collect();
        for name in [
            "not_for_me",
            "the_time",
            "remember",
            "remember_my_face",
            "setup_step",
        ] {
            assert!(names.iter().any(|each| each == name), "{name}: {names:?}");
        }
    }

    #[test]
    fn the_verdict_says_where_the_functions_sit() {
        let asked = |prompt, held| Asked {
            first_ms: Some(800),
            prompt: Some(prompt),
            held: Some(held),
        };
        // Held before the changing part: steady holds the character, the
        // functions and the exchange.
        let before = [
            (Arm::Same, asked(5_000, 4_992)),
            (Arm::One, asked(5_000, 640)),
            (Arm::Steady, asked(5_100, 4_800)),
            (Arm::SteadyBare, asked(1_100, 768)),
        ];
        let lines = verdict(&before);
        assert!(
            lines.iter().any(|line| line
                == "cache: the functions are about 4000 tokens (steady less steady-bare)"),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.starts_with("cache verdict: the functions are held with"))
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("DeepSeek puts them first"))
        );
        // After every system message: steady holds only the character.
        let after = [
            (Arm::Steady, asked(5_100, 640)),
            (Arm::SteadyBare, asked(1_100, 640)),
            (Arm::One, asked(5_000, 640)),
        ];
        assert!(
            verdict(&after)
                .iter()
                .any(|line| line.starts_with("cache verdict: the functions are not held"))
        );
        // First in the prompt: held in today's layout too.
        let first = [
            (Arm::Steady, asked(5_100, 4_800)),
            (Arm::SteadyBare, asked(1_100, 768)),
            (Arm::One, asked(5_000, 4_300)),
        ];
        assert!(
            verdict(&first)
                .iter()
                .any(|line| line.contains("DeepSeek puts them first"))
        );
        assert!(verdict(&[]).last().unwrap().contains("missing"));
    }
}
