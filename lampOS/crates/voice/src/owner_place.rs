//! The owner's approximate place, told to the model only for a question
//! that needs a place (#4333, `docs/owner-place.md`), with
//! `LAMP_OWNER_PLACE=on`.
//!
//! The app sets it through `lamp-cloud` and root's helper, rounded to about
//! a kilometre, beside the person's name ([`lamp_msgs::owner_place::FILE`]).
//! A spoken line whose words need a place ("near me", the weather, opening
//! hours: [`asks_for_a_place`]) is told it beside its words, as a staff
//! name's note is told ([`note_for`]); every other line, and every text from
//! the app, is told nothing of it. The file is read again for each such
//! line, so a set or a clear holds from the next question. With no place
//! kept, nothing is told, and the model asks where, as before.
//!
//! While one is kept it stands for the lamp's place: the weather's function
//! asked for no place answers for its rounded position, by its name
//! (`net::weather`), and the unit's `LAMP_PLACE` is not told beside every
//! request ([`kept_now`]), so the model is never given two places. Its
//! area's names join what speech to text listens for ([`terms`]), read
//! again every [`TERMS_EVERY`]. A clear puts `LAMP_PLACE` back.
//!
//! The owner's "forget everything about me" said at the lamp clears it too
//! ([`forget_with_everything`]); another person's never does.

use serde_json::{Value, json};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// The switch, in the voice's environment: `on`, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_OWNER_PLACE";

/// Whether the switch `value` is on.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// [`on`] as the environment says.
pub fn on_from_environment() -> bool {
    on(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// The owner's place kept now, with the switch on: read from the file each
/// time, so a clear holds at once. `None` without the switch.
pub fn kept_now() -> Option<lamp_msgs::owner_place::Place> {
    on_from_environment()
        .then(|| lamp_msgs::owner_place::read(Path::new(lamp_msgs::owner_place::FILE)))
        .flatten()
}

/// How often [`terms`] are read again: a set or a clear from the app
/// reaches the next speech stream opened after it.
pub const TERMS_EVERY: Duration = Duration::from_secs(10);

static TERMS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

/// What speech to text listens for of `place`: each part of its name
/// between commas ("Thao Dien", "Thu Duc"), or the whole name when it has
/// none.
pub fn terms_of(place: &lamp_msgs::owner_place::Place) -> Vec<String> {
    place
        .name
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The owner's place's terms ([`terms_of`]) as last read; none until
/// [`start_terms`] has read them, without the switch, or with no place
/// kept. Never waits.
pub fn terms() -> Vec<String> {
    TERMS
        .get()
        .and_then(|terms| terms.try_lock().ok().map(|terms| terms.clone()))
        .unwrap_or_default()
}

/// With the switch on, the place's terms are read now and every
/// [`TERMS_EVERY`] after, on a thread of their own, so no stream waits on
/// the file. Once a process; a second call changes nothing.
pub fn start_terms() {
    if !on_from_environment() || TERMS.get().is_some() {
        return;
    }
    let terms = TERMS.get_or_init(|| Mutex::new(Vec::new()));
    let started = std::thread::Builder::new()
        .name("lamp-voice-owner-place".into())
        .spawn(move || {
            loop {
                let read = kept_now().map(|place| terms_of(&place)).unwrap_or_default();
                if let Ok(mut terms) = terms.lock() {
                    *terms = read;
                }
                std::thread::sleep(TERMS_EVERY);
            }
        });
    if let Err(error) = started {
        eprintln!("lamp-voice second: the owner's place is not listened for: {error}");
    }
}

/// Nothing of the place is listened for from now: it was forgotten.
fn forget_terms() {
    if let Some(terms) = TERMS.get()
        && let Ok(mut terms) = terms.lock()
    {
        terms.clear();
    }
}

/// Single words that make a line one about where the person is.
const PLACE_WORDS: &[&str] = &[
    "nearby",
    "nearest",
    "closest",
    "local",
    "neighbourhood",
    "neighborhood",
    "directions",
    "weather",
    "forecast",
    "temperature",
    "rain",
    "raining",
    "rainy",
    "snow",
    "snowing",
    "sunny",
    "umbrella",
    "sunrise",
    "sunset",
];

/// Phrases that do, found anywhere in the line.
const PLACE_PHRASES: &[&str] = &[
    "near me",
    "near here",
    "near us",
    "near my place",
    "near home",
    "around here",
    "around me",
    "close to me",
    "close to here",
    "close by",
    "in my area",
    "in the area",
    "how far",
    "where can i",
    "where can we",
    "places to",
    "opening hours",
    "open now",
    "open today",
    "open tonight",
    "open late",
    "open until",
    "still open",
    "is it open",
    "are they open",
    // Vietnamese.
    "gần đây",
    "gần tôi",
    "gần nhà",
    "gần chỗ",
    "quanh đây",
    "thời tiết",
    "nhiệt độ",
    "trời mưa",
    "có mưa",
    // Opening and closing hours, never "mở cửa sổ" (open the window).
    "giờ mở cửa",
    "giờ đóng cửa",
    "còn mở cửa",
];

/// Whether `heard` asks something that depends on where the person is:
/// what is near, the weather, opening hours, how far. Read from the words
/// alone, as the early search reads them (`worker::early_query`): a line
/// that needs a place in other words is told nothing, and the model asks,
/// as it did before.
pub fn asks_for_a_place(heard: &str) -> bool {
    let lower = heard.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .collect();
    let has = |word: &str| words.contains(&word);
    // "What time does the pharmacy open?", "When does it close?"
    let hours = (lower.contains("what time") || has("when"))
        && ["open", "opens", "close", "closes", "closing"]
            .iter()
            .any(|word| has(word));
    hours
        || PLACE_WORDS.iter().any(|word| has(word))
        || PLACE_PHRASES.iter().any(|phrase| lower.contains(phrase))
}

/// What the model is told beside a line that needs a place: the area's name
/// and its rounded position, for this answer only.
pub fn note(place: &lamp_msgs::owner_place::Place) -> String {
    format!(
        "(For you to know, not to answer: this may need where they are. The owner set the lamp's place in the app: around {}, at about {:.2}, {:.2}, to within a kilometre or so. If they named no place, answer for there: put the area in a search, call the weather with no place, and name the area, never the numbers. If they named a place, answer for theirs.)",
        place.name, place.lat, place.lon
    )
}

/// The note for `heard` with the place kept at `file`: `None` for words
/// that need no place ([`asks_for_a_place`]), and when no place is kept,
/// so the model asks, as before. The file is read only for such words.
pub fn note_for(file: &Path, heard: &str) -> Option<String> {
    if !asks_for_a_place(heard) {
        return None;
    }
    lamp_msgs::owner_place::read(file).map(|place| note(&place))
}

/// What the model is told when the owner's place could not be removed
/// with everything else ([`forget_with_everything`]).
pub const PLACE_KEPT: &str = "The place the owner set in the app could not be forgotten just now. Tell them so; they can clear it in the app.";

/// The owner's "forget everything about me" (`told`, as the model hears
/// it): once their memory was emptied, for a request the second route's
/// guard marked as asking for everything, the place the app set goes too,
/// and the answer says so. The switch is not asked: a place kept while it
/// was on goes even after it is turned off, so "everything" never leaves it
/// behind (#4356). Another person's forget, a room's, an unmarked one, or
/// one whose memory was not emptied, leaves it and `told` as they were.
pub fn forget_with_everything(
    told: (Value, bool),
    lease: &crate::memory_scope::ScopeLease,
) -> (Value, bool) {
    if !lease.everything_marked() {
        return told;
    }
    // The owner by either name they go by, the typed one and the one their
    // face was kept as (`person::owner_ids`, Friday's P2 on #4476).
    forget_if(
        Path::new(lamp_msgs::owner_place::FILE),
        told,
        lease.admitted_person(),
        &crate::person::owner_ids(),
    )
}

/// [`forget_with_everything`] once its mark is judged: the place at `file`
/// removed when `admitted`, the line's person, is one of `owners`, the
/// owner's memory ids, and `told` says their memory was emptied, the switch
/// on or not.
pub(crate) fn forget_if(
    file: &Path,
    mut told: (Value, bool),
    admitted: Option<&str>,
    owners: &[String],
) -> (Value, bool) {
    let emptied =
        told.0["forgotten"] == true || told.0["note"] == json!("I was keeping nothing of them.");
    let theirs = admitted.is_some_and(|admitted| owners.iter().any(|owner| owner == admitted));
    if !emptied || !theirs || std::fs::symlink_metadata(file).is_err() {
        return told;
    }
    match std::fs::remove_file(file) {
        Ok(()) => {
            eprintln!("lamp-voice: the owner's place was cleared with everything about them");
            forget_terms();
            told.0["place"] = json!("forgotten too");
        }
        Err(error) => {
            eprintln!(
                "lamp-voice: the owner's place could not be cleared with everything about them: {}",
                error.kind()
            );
            told.0["everything"] = json!(false);
            told.0["place"] = json!(PLACE_KEPT);
        }
    }
    told
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::owner_place::Place;

    fn hanoi() -> Place {
        Place {
            name: "Ba Dinh, Hanoi".into(),
            lat: 21.04,
            lon: 105.83,
            set_at: "2026-10-29T08:30:00Z".into(),
        }
    }

    fn folder(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lamp-voice-place-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_switch_is_on_with_on_alone() {
        assert!(on(Some("on")));
        for value in [None, Some("1"), Some("ON"), Some("off"), Some("")] {
            assert!(!on(value), "{value:?}");
        }
        // Off, no place is kept now and none is listened for.
        assert_eq!(kept_now(), None);
        assert!(terms().is_empty());
    }

    /// The area's names join what speech to text listens for, each part.
    #[test]
    fn the_places_names_are_listened_for_by_their_parts() {
        let mut place = hanoi();
        assert_eq!(terms_of(&place), ["Ba Dinh", "Hanoi"]);
        place.name = "Thao Dien, Thu Duc".into();
        assert_eq!(terms_of(&place), ["Thao Dien", "Thu Duc"]);
        place.name = "Shibuya".into();
        assert_eq!(terms_of(&place), ["Shibuya"]);
        assert_eq!(
            crate::second_provider::net::key_terms::gather(
                ["Hanoi".to_owned()].into_iter().chain(terms_of(&hanoi()))
            ),
            ["Hanoi", "Ba Dinh"]
        );
    }

    /// Only a question that needs a place is one.
    #[test]
    fn a_place_question_is_found_by_its_words() {
        for said in [
            "What's a good coffee place near me?",
            "Lamp, is there a pharmacy nearby?",
            "Where's the nearest ATM?",
            "What's the weather like?",
            "Will it rain tomorrow?",
            "What time does the supermarket open?",
            "When does the post office close today?",
            "Is it open now?",
            "Where can I get a haircut around here?",
            "How far is the airport?",
            "Quán cà phê nào gần đây?",
            "Thời tiết hôm nay thế nào?",
        ] {
            assert!(asks_for_a_place(said), "{said}");
        }
        for said in [
            "What's two plus two?",
            "Tell me a joke.",
            "Remember that my favourite colour is green.",
            "Open the window, please.",
            "Close the door.",
            "What time is it?",
            "When is my next meeting?",
            "I'm training for a marathon.",
            "Set a timer for ten minutes.",
            "Mở cửa sổ giúp tôi.",
            "",
        ] {
            assert!(!asks_for_a_place(said), "{said}");
        }
    }

    /// The model sees the place only for a place question, and only when
    /// one is kept; a clear holds from the next question, nothing cached.
    #[test]
    fn the_model_is_told_the_place_only_for_a_place_question() {
        let dir = folder("note");
        let file = dir.join("place");
        let question = "What's a good coffee place near me?";
        // None kept: nothing told, so the model asks where, as before.
        assert_eq!(note_for(&file, question), None);
        std::fs::write(&file, lamp_msgs::owner_place::text(&hanoi())).unwrap();
        let told = note_for(&file, question).unwrap();
        assert!(told.contains("around Ba Dinh, Hanoi"), "{told}");
        assert!(told.contains("21.04, 105.83"), "{told}");
        assert!(told.starts_with("(For you to know, not to answer:"));
        for said in ["Tell me a joke.", "What time is it?", "Who are you?"] {
            assert_eq!(note_for(&file, said), None, "{said}");
        }
        // Cleared from the app: the next question is told nothing.
        std::fs::remove_file(&file).unwrap();
        assert_eq!(note_for(&file, question), None);
        // A file finer than the grid is told rounded.
        std::fs::write(
            &file,
            r#"{"name":"Ba Dinh","lat":21.035123,"lon":105.834987,"set_at":"2026-10-29T08:30:00Z"}"#,
        )
        .unwrap();
        let told = note_for(&file, question).unwrap();
        assert!(
            told.contains("21.04, 105.83") && !told.contains("21.035"),
            "{told}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The owner's own "forget everything" clears the place; another
    /// person's, a room's, or one that emptied nothing leaves it.
    #[test]
    fn only_the_owners_forget_everything_clears_the_place() {
        let dir = folder("forget");
        let file = dir.join("place");
        let emptied = || (json!({"forgotten": true, "everything": true}), true);
        let keep = || std::fs::write(&file, lamp_msgs::owner_place::text(&hanoi())).unwrap();
        keep();
        for (admitted, owner) in [
            (Some("p-guest"), Some("p-owner")),
            (None, Some("p-owner")),
            (Some("p-owner"), None),
            (None, None),
        ] {
            let owners: Vec<String> = owner.into_iter().map(str::to_owned).collect();
            let told = forget_if(&file, emptied(), admitted, &owners);
            assert_eq!(told, emptied(), "{admitted:?} {owner:?}");
            assert!(file.exists(), "{admitted:?} {owner:?}");
        }
        // A forget whose memory was not emptied leaves it.
        let failed = (json!({"error": "the memory did not answer"}), false);
        assert_eq!(
            forget_if(
                &file,
                failed.clone(),
                Some("p-owner"),
                &["p-owner".to_owned()]
            ),
            failed
        );
        assert!(file.exists());
        // The owner's: gone, and said.
        let (told, answered) =
            forget_if(&file, emptied(), Some("p-owner"), &["p-owner".to_owned()]);
        assert!(answered);
        assert_eq!(told["place"], "forgotten too");
        assert_eq!(told["everything"], true);
        assert!(!file.exists());
        // Nothing kept: nothing said of it.
        let nothing = (json!({"note": "I was keeping nothing of them."}), true);
        assert_eq!(
            forget_if(
                &file,
                nothing.clone(),
                Some("p-owner"),
                &["p-owner".to_owned()]
            ),
            nothing
        );
        // Not removable: kept, and said so.
        let file_in = dir.join("place-dir");
        std::fs::create_dir_all(file_in.join("inner")).unwrap();
        let (told, _) = forget_if(
            &file_in,
            emptied(),
            Some("p-owner"),
            &["p-owner".to_owned()],
        );
        assert_eq!(told["place"], PLACE_KEPT);
        assert_eq!(told["everything"], false);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Friday's P2 on #4476: the owner kept as "Robin" with a typed "Robin
    /// Example" (#4530) forgets as "Robin", and their place goes too: the
    /// line's person matched against either of the owner's ids.
    #[test]
    fn the_owner_forgets_the_place_by_either_name_they_go_by() {
        let dir = folder("forget-split");
        let file = dir.join("place");
        std::fs::write(&file, lamp_msgs::owner_place::text(&hanoi())).unwrap();
        let emptied = || (json!({"forgotten": true, "everything": true}), true);
        let owners = ["p-robin-example".to_owned(), "p-robin".to_owned()];
        assert_eq!(
            forget_if(&file, emptied(), Some("p-guest"), &owners),
            emptied()
        );
        assert!(file.exists());
        let (told, _) = forget_if(&file, emptied(), Some("p-robin"), &owners);
        assert_eq!(told["place"], "forgotten too");
        assert!(!file.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// #4356: with the switch off, a place kept while it was on still goes
    /// with the owner's "forget everything"; it is only never told.
    #[test]
    fn the_switch_off_the_owners_forget_still_clears_a_kept_place() {
        let dir = folder("forget-off");
        let file = dir.join("place");
        std::fs::write(&file, lamp_msgs::owner_place::text(&hanoi())).unwrap();
        assert!(!on_from_environment());
        // Off, nothing is told of it.
        assert!(kept_now().is_none());
        let emptied = (json!({"forgotten": true, "everything": true}), true);
        let (told, answered) = forget_if(&file, emptied, Some("p-owner"), &["p-owner".to_owned()]);
        assert!(answered);
        assert_eq!(told["place"], "forgotten too");
        assert!(!file.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
