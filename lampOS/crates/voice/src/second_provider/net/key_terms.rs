//! What the second route's speech to text listens for beside the lamp's own
//! name (`keyterm`, #753, the hearing half of #1712's quality plan): the
//! unit's own words first (`LAMP_SECOND_KEY_TERMS`, the places people ask
//! about and words like "desk lamp"), then, with `LAMP_SECOND_KNOWN_NAMES=on`,
//! the names the lamp knows: the one typed at setup, and the agents it hands
//! work to ("Claude Code", "Codex", "OpenCode", "Devin", when it has a tasks part).
//! Never the known faces' names: the list of everyone the lamp remembers would
//! leave the house with every line, people who only asked to be recognised
//! among them (the reviewer on #1818). Sending them needs the owner's yes.
//! A name misheard is a wrong answer: on lamp-4ace on 19 Oct "Hanoi" came
//! back tangled, and on lamp-52e6 on 15 Oct "Hey Lamp" as "Palin".
//!
//! The names are read now and every minute after, on a thread of their own,
//! so no line waits on a file. The lamp's zone is not one of
//! them: `Asia/Ho_Chi_Minh` is Hanoi's too, so a place belongs in the unit's
//! words.
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Where the worker reads the switch.
pub const ENVIRONMENT: &str = "LAMP_SECOND_KNOWN_NAMES";
/// The lamp's own name, which the query always gives first.
pub const NAME: &str = "Lamp";
/// At most this many terms after the lamp's name. Deepgram takes 100 and
/// 500 tokens, and advises the 20 to 50 that matter most.
pub const MOST: usize = 49;
/// A term is at most this many characters: setup's own limit for a name
/// (`person::MOST_CHARS`).
pub const MOST_CHARS: usize = 64;
/// How often the names are read again.
pub const READ_EVERY: Duration = Duration::from_secs(60);

static NAMES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

/// The names the lamp knows, as last read; none until
/// [`start_from_environment`] has read them, or without the switch. Never
/// waits.
pub fn known() -> Vec<String> {
    NAMES
        .get()
        .and_then(|names| names.try_lock().ok().map(|names| names.clone()))
        .unwrap_or_default()
}

/// What a stream listens for after the lamp's name: the unit's words
/// (`fixed`), then the words of a skill's make with [`SKILL_ENVIRONMENT`]
/// on ([`SKILL_TERMS`]), then the owner's place's names from the app, with
/// `LAMP_OWNER_PLACE=on` (`crate::owner_place::terms`, #4333), then the
/// names the lamp knows ([`known`]), each once.
pub fn listening(fixed: &[String]) -> Vec<String> {
    listening_with(fixed, skill_terms_on())
}

/// [`listening`], with the words of a skill's make (`skill`) or without.
fn listening_with(fixed: &[String], skill: bool) -> Vec<String> {
    gather(
        fixed
            .iter()
            .cloned()
            .chain(
                SKILL_TERMS
                    .iter()
                    .filter(|_| skill)
                    .map(|term| (*term).to_owned()),
            )
            .chain(crate::owner_place::terms())
            .chain(known()),
    )
}

/// `on` only; off unless set. Speech to text listens for the words a
/// skill's make is asked with ([`SKILL_TERMS`]), after the unit's own. On
/// lamp-9adf on 17 Nov the owner's "Lamp, make a skill: every morning at
/// nine, read me my new emails" was written four times without the word,
/// "Make me a skew every real morning" and "Plan. Acne rescue s 9AM" among
/// them, and the model then read the mail now and set a daily reminder of
/// the words; typed in the app, the same ask made the skill.
pub const SKILL_ENVIRONMENT: &str = "LAMP_SKILL_KEYTERM";

/// The words listened for with [`SKILL_ENVIRONMENT`]: few, as Deepgram
/// advises, and plain ([`gather`] keeps each once, within [`MOST`]).
pub const SKILL_TERMS: [&str; 3] = ["skill", "make a skill", "new skill"];

/// Whether [`SKILL_ENVIRONMENT`] is on in this process.
pub fn skill_terms_on() -> bool {
    on(std::env::var(SKILL_ENVIRONMENT).ok().as_deref())
}

/// Whether the switch is on: `on`, and nothing else.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// With the switch on, the names are read now and every [`READ_EVERY`]
/// after, on a thread of their own: the setup name, and the agents' names
/// when the worker hands work over (`staff`). Once a process; a second call
/// changes nothing.
pub fn start_from_environment(staff: bool) {
    if !on(std::env::var(ENVIRONMENT).ok().as_deref()) || NAMES.get().is_some() {
        return;
    }
    let names = NAMES.get_or_init(|| Mutex::new(Vec::new()));
    let started = std::thread::Builder::new()
        .name("lamp-voice-known-names".into())
        .spawn(move || {
            let mut said = None;
            loop {
                let agents = crate::agent_tasks::agent_names()
                    .into_iter()
                    .filter(|_| staff)
                    .map(str::to_owned);
                let read = gather(crate::person::from_environment().into_iter().chain(agents));
                // Said when the count changes: never the names.
                if said != Some(read.len()) {
                    eprintln!(
                        "lamp-voice second: speech to text listens for {} names the lamp knows",
                        read.len()
                    );
                    said = Some(read.len());
                }
                if let Ok(mut held) = names.lock() {
                    *held = read;
                }
                std::thread::sleep(READ_EVERY);
            }
        });
    if let Err(error) = started {
        eprintln!("lamp-voice second: the names the lamp knows are not listened for: {error}");
    }
}

/// `terms` in order, each once (by its lower case, and never the lamp's own
/// name, which the query gives first), each plain and at most
/// [`MOST_CHARS`], at most [`MOST`] in all.
pub fn gather(terms: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for term in terms {
        if kept.len() >= MOST {
            break;
        }
        let term = term.split_whitespace().collect::<Vec<_>>().join(" ");
        let lower = term.to_lowercase();
        if plain(&term)
            && lower != NAME.to_lowercase()
            && !kept.iter().any(|other| other.to_lowercase() == lower)
        {
            kept.push(term);
        }
    }
    kept
}

/// A term Deepgram takes as meant: words of letters, with spaces, hyphens
/// and apostrophes between them. Never a comma or a semicolon, which it
/// would read as one literal term and boost nothing.
fn plain(term: &str) -> bool {
    let count = term.chars().count();
    (1..=MOST_CHARS).contains(&count)
        && term.chars().next().is_some_and(char::is_alphabetic)
        && term
            .chars()
            .all(|c| c.is_alphabetic() || matches!(c, ' ' | '-' | '\'' | '’'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_units_words_come_first_then_the_names_the_lamp_knows() {
        let fixed = ["Hanoi", "desk lamp"].map(String::from);
        assert_eq!(
            gather(
                fixed
                    .iter()
                    .cloned()
                    .chain(["Minh", "Sam", "Ngọc Anh"].map(String::from))
            ),
            ["Hanoi", "desk lamp", "Minh", "Sam", "Ngọc Anh"]
        );
        // Nothing read in this process: the unit's words alone.
        if NAMES.get().is_none() {
            assert_eq!(listening(&fixed), fixed);
            assert!(known().is_empty());
        }
    }

    #[test]
    fn each_term_is_kept_once_plain_and_within_deepgrams_bounds() {
        // Twice in two casings, and the lamp's own name: once, and never it.
        assert_eq!(
            gather(["sam", "Sam", "lamp", "Lamp"].map(String::from)),
            ["sam"]
        );
        // A zone said as UTC+7, a comma, a number: none is a name.
        for not_a_name in ["UTC+7", "Sam, Minh", "R2D2", "", " ", "-Sam"] {
            assert!(gather([not_a_name.to_owned()]).is_empty(), "{not_a_name:?}");
        }
        assert_eq!(gather(["  New   York ".to_owned()]), ["New York"]);
        assert!(gather(["A".repeat(MOST_CHARS + 1)]).is_empty());
        let many = (0..200).map(|n| {
            format!(
                "Person {}",
                char::from(b'a' + (n % 26) as u8)
                    .to_string()
                    .repeat(1 + n / 26)
            )
        });
        assert_eq!(gather(many).len(), MOST);
    }

    /// With the skill switch, the words of a make come after the unit's
    /// own and before the names; each once, and within the bound with the
    /// unit's own words at theirs.
    #[test]
    fn the_words_of_a_skills_make_follow_the_units_own_with_the_switch() {
        let fixed = ["Hanoi", "Skill"].map(String::from);
        if NAMES.get().is_none() {
            assert_eq!(listening_with(&fixed, false), fixed);
            assert_eq!(
                listening_with(&fixed, true),
                ["Hanoi", "Skill", "make a skill", "new skill"]
            );
        }
        for term in SKILL_TERMS {
            assert_eq!(gather([term.to_owned()]), [term], "{term}");
        }
        let full: Vec<String> = (0..crate::second_config::MOST_KEY_TERMS)
            .map(|n| format!("Place {}", char::from(b'a' + n as u8)))
            .collect();
        let heard = listening_with(&full, true);
        assert!(heard.len() <= MOST, "{}", heard.len());
        assert!(
            SKILL_TERMS
                .iter()
                .all(|term| heard.iter().any(|said| said == term))
        );
    }

    #[test]
    fn the_switch_is_on_only_for_on() {
        assert!(on(Some("on")));
        for off in [None, Some(""), Some("off"), Some("ON")] {
            assert!(!on(off), "{off:?}");
        }
    }
}
