//! `lamp-voice probe-reminders PROVIDER_CONFIG SECOND_CONFIG [RUNS]`: which
//! function DeepSeek calls first for a question about a reminder, with
//! `list_reminders` described as before and as now (the person's call, 30
//! Oct: "what did I ask you to remind me?", "when did I set that
//! reminder?" are answered from the reminder list, not from memory).
//!
//! Each line is asked on its own, as the first line of a spoken turn: the
//! spoken character with the memory's note, the turn's changing part and
//! the memory note a turn is told ([`super::tools::DAYS_NOTE`]), with
//! every function a spoken turn declares and the schedule's, read from the
//! actions part's catalog in `LAMP_ACTIONS_DIR` (`/run/lampos-actions`) as
//! `probe-tools` reads it, so the arms differ only in `list_reminders`'
//! description:
//!
//! - **before**: `list_reminders` as it was described before ([`BEFORE`],
//!   `lamp_actions::schedule::LIST_BEFORE`; a test holds the two the same).
//! - **now**: as the installed catalog describes it, which must be this
//!   build's (it names recall), or the probe refuses to run.
//!
//! The four reminder lines are right with `list_reminders` first; the four
//! others (a memory question, a new reminder, yesterday's talk, a greeting)
//! are right with anything but `list_reminders` (rule 8: never listed
//! unasked). It prints counts only: never the character, a key, a URL or
//! what the model said.
use super::super::net::{self, Chat, Message};
use super::tools::DAYS_NOTE;
use crate::second_config::{CREDENTIALS_ENV, SecondConfig};
use std::{collections::BTreeMap, io, path::Path, sync::Arc};

/// `list_reminders`' description before questions about reminders were
/// named in it (`lamp_actions::schedule::LIST_BEFORE`: the voice has the
/// actions part only for its tests, so a test holds the two the same).
pub const BEFORE: &str = "Read one bounded page of active timers and reminders, oldest first. Each item's kind is timer (set with set_timer) or reminder. To say what is set, or to find one to cancel, leave pending_only out. If next_after_id is present, use it as after_id to read more; never describe an incomplete page as the whole list. If not_yet_due, how many are set and not yet due in all, is more than 0, timers or reminders are set: list again without pending_only before saying none are.";

/// Words only this build's description has: an installed catalog without
/// them is the old one, and both arms would be the same.
pub const NOW_SAYS: &str = "not recall";

/// The function the reminder lines are right with.
pub const LIST: &str = "list_reminders";

/// One line: what is said, and whether it is a question about a reminder.
pub struct Line {
    pub said: &'static str,
    pub about_reminders: bool,
}

pub const LINES: [Line; 8] = [
    Line {
        said: "Hey Lamp, what did I ask you to remind me about?",
        about_reminders: true,
    },
    Line {
        said: "When is my reminder?",
        about_reminders: true,
    },
    Line {
        said: "When did I set that reminder?",
        about_reminders: true,
    },
    Line {
        said: "Do I have any reminders for tomorrow?",
        about_reminders: true,
    },
    Line {
        said: "What's my favourite drink?",
        about_reminders: false,
    },
    Line {
        said: "Remind me to call my sister at six tonight.",
        about_reminders: false,
    },
    Line {
        said: "What did we talk about yesterday?",
        about_reminders: false,
    },
    Line {
        said: "Hey Lamp, good morning! How are you?",
        about_reminders: false,
    },
];

/// How an arm describes `list_reminders`.
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

    /// `functions` with `list_reminders` described as this arm describes it.
    pub fn functions(self, mut functions: Vec<lamp_gemini::Tool>) -> Vec<lamp_gemini::Tool> {
        if self == Self::Before {
            for tool in functions.iter_mut().filter(|tool| tool.name == LIST) {
                tool.description = BEFORE.to_owned();
            }
        }
        functions
    }
}

/// The request for `line`: the spoken character with the memory's note,
/// the turn's changing part and the memory note, then the line.
pub fn messages(character: &str, line: usize) -> Vec<Message> {
    let spoken = super::super::worker::spoken_character(character, false, true);
    let system = format!("{spoken}\n\n{}{DAYS_NOTE}", super::cache::changing(0));
    vec![Message::system(system), Message::user(LINES[line].said)]
}

/// Every function a spoken turn declares, and the schedule's, from the
/// actions part's catalog in `actions`.
pub fn functions(actions: std::path::PathBuf) -> Vec<lamp_gemini::Tool> {
    super::tools::heeding_functions(Some(actions))
}

/// For each arm and line, how many requests called each function first.
pub type Counts = BTreeMap<(Arm, usize), BTreeMap<String, u32>>;

/// Each arm asks each line `runs` times; `chats` is before's and now's.
pub async fn ask(chats: [&Chat; 2], character: &str, runs: u32) -> Counts {
    let mut counts = Counts::new();
    for _ in 0..runs {
        for arm in Arm::ALL {
            let chat = chats[usize::from(arm == Arm::Now)];
            for line in 0..LINES.len() {
                let called = match super::tools::asked(chat, &messages(character, line)).await {
                    Ok(name) => name,
                    Err(error) => {
                        println!(
                            "reminders arm={} line={} failed: {error}",
                            arm.name(),
                            line + 1
                        );
                        continue;
                    }
                };
                *counts
                    .entry((arm, line))
                    .or_default()
                    .entry(called)
                    .or_default() += 1;
            }
        }
    }
    counts
}

/// Whether `called` first is right for `line`.
pub fn right(line: &Line, called: &str) -> bool {
    (called == LIST) == line.about_reminders
}

/// One line per arm and line with its counts and how many were right, then
/// each arm's totals on the reminder lines and the others.
pub fn verdict(counts: &Counts) -> Vec<String> {
    let mut lines = Vec::new();
    for arm in Arm::ALL {
        let (mut asked_on, mut right_on, mut asked_off, mut right_off) = (0, 0, 0, 0);
        for (at, line) in LINES.iter().enumerate() {
            let called = counts.get(&(arm, at)).cloned().unwrap_or_default();
            let asked: u32 = called.values().sum();
            let good: u32 = called
                .iter()
                .filter(|(name, _)| right(line, name))
                .map(|(_, n)| n)
                .sum();
            if line.about_reminders {
                (asked_on, right_on) = (asked_on + asked, right_on + good);
            } else {
                (asked_off, right_off) = (asked_off + asked, right_off + good);
            }
            let shown: Vec<String> = called
                .iter()
                .map(|(name, n)| format!("{name}={n}"))
                .collect();
            lines.push(format!(
                "reminders arm={} line={} right={good}/{asked} ({}{LIST}): {}",
                arm.name(),
                at + 1,
                if line.about_reminders { "" } else { "not " },
                shown.join(" ")
            ));
        }
        lines.push(format!(
            "reminders arm={} reminder lines {right_on}/{asked_on} list_reminders, other lines {right_off}/{asked_off} not list_reminders",
            arm.name()
        ));
    }
    lines
}

pub async fn run(provider_config: &Path, second_config: &Path, runs: u32) -> io::Result<()> {
    let provider = crate::config::ProviderConfig::load(provider_config)?;
    let character = provider.instruction(false)?;
    let config: SecondConfig = crate::second_config::load_from_path(second_config)?
        .with_environment(|name| std::env::var_os(name))?;
    let credentials = std::env::var_os(CREDENTIALS_ENV);
    let (base, key) = super::tools::deepseek_route(&config, credentials.as_deref())?;
    let model = &config.hosted.deepseek_model;
    let actions = std::env::var_os(crate::tools::ACTIONS_ENVIRONMENT)
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| "/run/lampos-actions".into(), std::path::PathBuf::from);
    let built = functions(actions);
    if !built
        .iter()
        .any(|tool| tool.name == LIST && tool.description.contains(NOW_SAYS))
    {
        return Err(io::Error::other(
            "this build's list_reminders isn't in the actions part's catalog (LAMP_ACTIONS_DIR): run it where this build's lamp-actions runs, with LAMP_SCHEDULE_TOOLS=on",
        ));
    }
    let chat = |arm: Arm| {
        let declared = arm.functions(built.clone());
        let declare: net::Declare = Arc::new(move || declared.clone());
        Chat::deepseek_at(&base, model, key.expose(), declare, false).map_err(io::Error::other)
    };
    let (before, now) = (chat(Arm::Before)?, chat(Arm::Now)?);
    println!(
        "reminders: DeepSeek's {model} {}, thinking off, {} functions, {} lines, {runs} runs of each arm",
        super::super::worker::deepseek_where(&config.hosted),
        built.len(),
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

    /// This build's actions catalog with the schedule on, written to a
    /// folder of its own as lamp-actions writes it.
    fn functions_built() -> Vec<lamp_gemini::Tool> {
        let dir = std::env::temp_dir().join(format!(
            "probe-reminders-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut offered = lamp_actions::catalog(true, true);
        offered.extend(lamp_actions::schedule::catalog());
        std::fs::write(
            dir.join(crate::tools::CATALOG_FILE),
            lamp_actions::catalog_file(&offered),
        )
        .unwrap();
        let tools = functions(dir.clone());
        std::fs::remove_dir_all(dir).unwrap();
        tools
    }

    #[test]
    fn before_is_the_actions_parts_old_description_and_now_is_told_apart() {
        assert_eq!(BEFORE, lamp_actions::schedule::LIST_BEFORE);
        assert!(!BEFORE.contains(NOW_SAYS));
        assert!(lamp_actions::schedule::LIST_DESCRIBED.contains(NOW_SAYS));
    }

    #[test]
    fn the_arms_differ_only_in_list_reminders_description() {
        let built = functions_built();
        let (before, now) = (
            Arm::Before.functions(built.clone()),
            Arm::Now.functions(built),
        );
        assert_eq!(before.len(), now.len());
        let mut differ = Vec::new();
        for (old, new) in before.iter().zip(&now) {
            assert_eq!(old.name, new.name);
            if old.description != new.description {
                differ.push(old.name.clone());
            }
        }
        assert_eq!(differ, [LIST]);
        let listing = now.iter().find(|tool| tool.name == LIST).unwrap();
        assert_eq!(listing.description, lamp_actions::schedule::LIST_DESCRIBED);
    }

    #[test]
    fn each_line_is_asked_alone_after_the_memory_note() {
        for (at, line) in LINES.iter().enumerate() {
            let asked = messages("CHARACTER", at);
            assert_eq!(asked.len(), 2);
            let text = serde_json::to_string(&asked).unwrap();
            assert!(text.contains("Your memory"), "{text}");
            assert!(text.contains(line.said), "{text}");
        }
        assert_eq!(LINES.iter().filter(|l| l.about_reminders).count(), 4);
    }

    #[test]
    fn the_verdict_counts_list_reminders_as_right_only_on_reminder_lines() {
        let mut counts = Counts::new();
        for at in 0..LINES.len() {
            let entry = counts.entry((Arm::Now, at)).or_default();
            entry.insert(LIST.to_owned(), 2);
            entry.insert("recall".to_owned(), 1);
        }
        let said = verdict(&counts);
        assert!(
            said.contains(
                &"reminders arm=now line=1 right=2/3 (list_reminders): list_reminders=2 recall=1"
                    .to_owned()
            ),
            "{said:?}"
        );
        assert!(said.contains(&"reminders arm=now line=5 right=1/3 (not list_reminders): list_reminders=2 recall=1".to_owned()), "{said:?}");
        assert!(said.contains(&"reminders arm=now reminder lines 8/12 list_reminders, other lines 4/12 not list_reminders".to_owned()), "{said:?}");
    }
}
