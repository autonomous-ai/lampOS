//! What the second route's speech to text listens for beside the lamp's own
//! name (`keyterm`, #753, the hearing half of #1712's quality plan): the
//! unit's own words first (`LAMP_SECOND_KEY_TERMS`, the places people ask
//! about and words like "desk lamp"), then, with `LAMP_SECOND_KNOWN_NAMES=on`,
//! the names the lamp knows: the one typed at setup, the agents it hands work
//! to ("Claude Code", "Codex", "OpenCode", when it has a tasks part) and the
//! known faces'.
//! A name misheard is a wrong answer: on lamp-4ace on 19 Oct "Hanoi" came
//! back tangled, and on lamp-52e6 on 15 Oct "Hey Lamp" as "Palin".
//!
//! The names are read now and every minute after, on a thread of their own,
//! so no line waits on the eyes or a file. The lamp's zone is not one of
//! them: `Asia/Ho_Chi_Minh` is Hanoi's too, so a place belongs in the unit's
//! words.
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
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
/// How long the eyes are given to list the known faces.
const EYES_DEADLINE: Duration = Duration::from_millis(500);

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
/// (`fixed`), then the names the lamp knows ([`known`]), each once.
pub fn listening(fixed: &[String]) -> Vec<String> {
    gather(fixed.iter().cloned().chain(known()))
}

/// Whether the switch is on: `on`, and nothing else.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// With the switch on, the names are read now and every [`READ_EVERY`]
/// after, on a thread of their own; the known faces from the eyes' socket
/// `eyes` (the worker's `--who-socket`), when it has one, and the agents'
/// names when the worker hands work over (`staff`). Once a process; a
/// second call changes nothing.
pub fn start_from_environment(eyes: Option<PathBuf>, staff: bool) {
    if !on(std::env::var(ENVIRONMENT).ok().as_deref()) || NAMES.get().is_some() {
        return;
    }
    let names = NAMES.get_or_init(|| Mutex::new(Vec::new()));
    let eyes = eyes.filter(|socket| socket.is_absolute());
    let started = std::thread::Builder::new()
        .name("lamp-voice-known-names".into())
        .spawn(move || {
            let mut said = None;
            loop {
                let agents = crate::agent_tasks::agent_names()
                    .into_iter()
                    .filter(|_| staff)
                    .map(str::to_owned);
                let read = gather(
                    crate::person::from_environment()
                        .into_iter()
                        .chain(agents)
                        .chain(eyes.as_deref().map(known_faces).unwrap_or_default()),
                );
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

/// The known faces' names, as the eyes list them on their socket; none if
/// they don't answer in time.
fn known_faces(socket: &Path) -> Vec<String> {
    crate::tools::exchange(socket, &json!({"list": true}), EYES_DEADLINE)
        .and_then(|answer| answer.get("people").and_then(Value::as_array).cloned())
        .map(|people| {
            people
                .iter()
                .filter_map(|name| name.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
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

    #[test]
    fn the_switch_is_on_only_for_on() {
        assert!(on(Some("on")));
        for off in [None, Some(""), Some("off"), Some("ON")] {
            assert!(!on(off), "{off:?}");
        }
    }
}
