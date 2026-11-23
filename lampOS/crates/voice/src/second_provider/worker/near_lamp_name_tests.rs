//! #7698: on lamp-06c1 on 18 Nov (14072) the owner's "Lamp, …", seen
//! speaking with their own face, came back from the speech to text with a
//! word one letter from "Lamp" ("Lap,", "Lamps,", "Clamp,", "Laam,") and
//! was held six times as said to someone it calls by name. Played on the
//! owner's desk ([`super::tests::CarryDesk`]) as the
//! `a_name_that_is_also_a_word_is_a_word_only_when_one_person_is_seen`
//! replay is: the near spelling is not held so; a real name still is.
use super::tests::CarryDesk;

const TO_ANOTHER: &str = "said to someone it calls by name, not to the lamp";

/// The owner's line `words`, their face "f1" facing the lamp and seen
/// speaking it, 3 s after the lamp's answer: why it is held, if it is.
fn held(words: &str) -> Option<&'static str> {
    let mut desk = CarryDesk::new_in(false, false);
    desk.heeding.name_or_follow_up = false;
    desk.answered(3);
    desk.line(1, "f1", words)
}

#[test]
fn a_set_off_word_one_letter_from_lamp_is_not_held_as_someone_elses_name() {
    let _one = crate::provider_worker::tests::one_thread();
    for words in ["Lap, what's my name?", "Lamps, did you keep that?"] {
        assert_ne!(held(words), Some(TO_ANOTHER), "{words}");
    }
    assert_eq!(
        held("Anna, what would you do?"),
        Some(TO_ANOTHER),
        "a name that is no word, with one face"
    );
}
