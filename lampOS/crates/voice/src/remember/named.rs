//! A name the observation pass keeps must be the person's own, said by
//! them (lamp-06c1, 4 Nov). While the owner's face was at the lamp, silent,
//! a coworker near it said something like "... Connie Long ...", and the
//! pass kept "Their name is Connie Long." in the owner's scope. A name heard
//! once, maybe misheard, maybe from someone else, is no name fact.
//!
//! The pass's proposal that gives a person's own name ([`name_in`]: "Their
//! name is X.", "They go by X.", "Prefers to be called X.") is kept only
//! when one of the exchanges it cites was seen spoken by that person (one
//! face, theirs, as the eyes read the line: the same evidence a room line's
//! speaker needs, `speaker_of`), and in that line they either
//! introduced themselves with that name ("My name is X", "I'm X", "Call me
//! X", "Tên tôi là X") or spelled it, or the lamp read it back in its answer
//! and the person's next line, seen spoken by them too, was a plain yes (the
//! checked-names path, #1057, `known_faces`). A line whose own answer doubts
//! what was heard (#3504 N5) gives none, and neither does the lamp's own
//! name or a word written for it (#3504 N4). Anything else is left out.
//! Names of other people ("Their friend is Connie.") are no name of theirs
//! and are judged as before. Always on: a tightening only.
use serde_json::Value;

use crate::known_faces::{self, Said};

/// The journal's line for proposals left out: counts only, no words.
pub const DROPPED: &str = "lamp-voice provider: observation pass: a name not introduced by its person, seen speaking, left out";

/// What gives the subject's own name, as lower-case words in a row: their
/// name and a verb, or a verb of being called.
const GIVES_NAME: &[&[&str]] = &[
    &["name", "is"],
    &["name's"],
    &["name", "was"],
    &["names", "is"],
    &["are", "called"],
    &["is", "called"],
    &["am", "called"],
    &["are", "named"],
    &["is", "named"],
    &["am", "named"],
    &["goes", "by"],
    &["go", "by"],
    &["to", "be", "called"],
    &["call", "them"],
    &["call", "him"],
    &["call", "her"],
    &["call", "me"],
    &["ten", "la"],
    &["ten", "toi", "la"],
    &["ten", "ho", "la"],
    &["ten", "cua", "ho", "la"],
];

/// Words that make a name sentence about the person themself, read just
/// before "name" (or a verb of being called): "their name", "the user's
/// name", "they are called", and a sentence opening on "Name is".
const THEIRS: &[&str] = &[
    "their", "his", "her", "my", "your", "user's", "person's", "owner's", "they", "he", "she", "i",
    "user", "person", "owner", "full", "first", "last", "real", "given", "nick", "prefers",
    "prefer", "wants", "want", "likes", "like", "asks", "asked", "toi", "ho",
];

/// The name `text` gives the person themself, as written after its
/// phrase ([`GIVES_NAME`]): its capitalised words, in a row. `None` for any
/// other text, and for "Their friend's name is Connie.", which is about
/// someone else. `Some("")` for a name sentence whose name can't be read.
pub fn name_in(text: &str) -> Option<String> {
    for sentence in text.split_inclusive(['.', '!', '?', ';']) {
        let words: Vec<&str> = sentence
            .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '\u{2019}'))
            .filter(|word| !word.is_empty())
            .collect();
        let folded: Vec<String> = words
            .iter()
            .map(|word| {
                known_faces::folded_words(word)
                    .concat()
                    .replace('\u{2019}', "'")
            })
            .collect();
        for at in 0..folded.len() {
            for phrase in GIVES_NAME {
                let end = at + phrase.len();
                if end > folded.len()
                    || !folded[at..end]
                        .iter()
                        .zip(phrase.iter())
                        .all(|(word, phrase)| word == phrase)
                {
                    continue;
                }
                // Whose: the word before the phrase, past "name" for
                // "their name is"; or nothing before ("Name is X.").
                let before = at
                    .checked_sub(1)
                    .map(|before| folded[before].as_str())
                    .map(|word| if word == "name" { "" } else { word });
                let before = match before {
                    Some("") => at.checked_sub(2).map(|before| folded[before].as_str()),
                    before => before,
                };
                let theirs = match before {
                    None => true,
                    Some(word) => THEIRS.contains(&word),
                };
                if !theirs {
                    continue;
                }
                let name: Vec<&str> = words[end..]
                    .iter()
                    .take_while(|word| word.chars().next().is_some_and(char::is_uppercase))
                    .copied()
                    .collect();
                // "Call them back", "Goes by bus": no name after a phrase
                // that is not of a name is no name sentence.
                let of_a_name = phrase.iter().any(|word| {
                    matches!(
                        *word,
                        "name" | "name's" | "names" | "named" | "called" | "ten"
                    )
                });
                if name.is_empty() && !of_a_name {
                    continue;
                }
                return Some(name.join(" "));
            }
        }
    }
    None
}

/// What introduces the speaker's own name in a line, the name right after
/// it, as folded words ([`known_faces::folded_words`]).
const INTRODUCES: &[&[&str]] = &[
    &["my", "name", "is"],
    &["my", "name's"],
    &["call", "me"],
    &["i'm"],
    &["i", "am"],
    &["i", "am", "called"],
    &["this", "is"],
    &["ten", "toi", "la"],
    &["toi", "ten", "la"],
    &["toi", "ten"],
    &["toi", "la"],
];

/// Whether in `heard` the speaker introduced themself as `name`: its words
/// right after one of [`INTRODUCES`], in a line that gives a name
/// (`known_faces::gives_a_name`, which leaves out questions and words that
/// are no one's name) or in Vietnamese.
fn introduced(name: &str, heard: &str) -> bool {
    let name = known_faces::folded_words(name);
    let words = known_faces::folded_words(heard);
    if name.is_empty() {
        return false;
    }
    let english = known_faces::gives_a_name(heard);
    INTRODUCES.iter().any(|phrase| {
        let vietnamese = phrase[0] == "ten" || phrase[0] == "toi";
        (english || vietnamese)
            && (0..words.len()).any(|at| {
                let end = at + phrase.len();
                end + name.len() <= words.len()
                    && words[at..end]
                        .iter()
                        .zip(phrase.iter())
                        .all(|(word, phrase)| word == phrase)
                    && words[end..end + name.len()] == name[..]
            })
    })
}

/// Whether `name` is the lamp's own or a word written for it (#3504 N4).
fn the_lamps(name: &str) -> bool {
    known_faces::folded_words(name)
        .iter()
        .any(|word| crate::attention::written_for_the_name(word))
}

/// `observations` without a name of the person's own ([`name_in`]) that
/// the cited lines don't show they gave: `records` as the job has them
/// (`"id"`, `"kind"`, `"heard"`, `"said"`), `seen` the ids of the
/// exchanges seen spoken by the conversation's person. And how many went.
pub fn kept(observations: Vec<Value>, records: &[Value], seen: &[u64]) -> (Vec<Value>, usize) {
    let before = observations.len();
    let kept: Vec<Value> = observations
        .into_iter()
        .filter(|observation| {
            let Some(name) = name_in(observation["text"].as_str().unwrap_or_default()) else {
                return true;
            };
            let cited: Vec<u64> = observation["source_ids"]
                .as_array()
                .map(|ids| ids.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_default();
            given(&name, records, &cited, seen)
        })
        .collect();
    let dropped = before - kept.len();
    (kept, dropped)
}

/// Whether one of the `cited` exchanges shows the person gave `name`.
fn given(name: &str, records: &[Value], cited: &[u64], seen: &[u64]) -> bool {
    if name.is_empty() || the_lamps(name) {
        return false;
    }
    fn exchange<'a>(record: &'a Value, seen: &[u64]) -> Option<(&'a str, &'a str)> {
        let id = record["id"].as_u64()?;
        (record["kind"] == "exchange" && seen.contains(&id)).then(|| {
            (
                record["heard"].as_str().unwrap_or_default(),
                record["said"].as_str().unwrap_or_default(),
            )
        })
    }
    records.iter().enumerate().any(|(at, record)| {
        if !record["id"].as_u64().is_some_and(|id| cited.contains(&id)) {
            return false;
        }
        let Some((heard, said)) = exchange(record, seen) else {
            return false;
        };
        if super::doubted::doubts_what_was_heard(said) {
            return false;
        }
        if introduced(name, heard) || known_faces::spelled_by_them(name, heard) {
            return true;
        }
        // Read back by the lamp and agreed to in their next line.
        records
            .get(at + 1)
            .and_then(|next| exchange(next, seen))
            .is_some_and(|(next, _)| {
                known_faces::said_by_them(name, heard)
                    && known_faces::agreed_to(
                        name,
                        &Said {
                            this: next.to_owned(),
                            before: vec![(heard.to_owned(), said.to_owned())],
                        },
                    )
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn exchange(id: u64, heard: &str, said: &str) -> Value {
        json!({"id": id, "kind": "exchange", "heard": heard, "said": said})
    }

    fn proposal(text: &str, ids: &[u64]) -> Value {
        json!({"text": text, "category": "preference", "source_ids": ids, "supersedes": null})
    }

    /// The pass's ways of writing someone's own name, and texts that are
    /// not one: someone else's name, a name in a thread.
    #[test]
    fn a_name_of_their_own_is_read_and_others_are_not() {
        for (text, name) in [
            ("Their name is Connie Long.", "Connie Long"),
            ("The user's name is Robin.", "Robin"),
            ("Name is Robin.", "Robin"),
            ("They go by Bo.", "Bo"),
            ("Prefers to be called Robbie.", "Robbie"),
            ("Their full name is Robin Hart.", "Robin Hart"),
            ("They are called Mai.", "Mai"),
            ("Tên của họ là Linh.", "Linh"),
            ("Their name is connie long.", ""),
        ] {
            assert_eq!(name_in(text).as_deref(), Some(name), "{text}");
        }
        for text in [
            "Talked to Connie Long today.",
            "Their friend's name is Connie.",
            "Their sister is named Lan.",
            "Likes green tea.",
            "Is meeting Connie on Friday 6 November.",
            "Call them back tomorrow.",
            "Goes by bus to work.",
        ] {
            assert_eq!(name_in(text), None, "{text}");
        }
    }

    /// lamp-06c1, 4 Nov: "... Connie Long ..." from a voice no face was
    /// seen speaking, the owner's face silent: no name kept.
    #[test]
    fn a_name_heard_from_an_unseen_speaker_is_not_kept() {
        let records = [exchange(
            55,
            "yeah I talked to Connie Long today about the launch",
            "Sounds like a busy day!",
        )];
        let proposals = vec![
            proposal("Their name is Connie Long.", &[55]),
            proposal("Talked to Connie Long today about the launch.", &[55]),
        ];
        let (kept, dropped) = kept(proposals, &records, &[]);
        assert_eq!(dropped, 1);
        assert_eq!(kept.len(), 1);
        assert_eq!(
            kept[0]["text"],
            "Talked to Connie Long today about the launch."
        );
        // Seen spoken by them, but no introduction: not a name either.
        let (kept, dropped) = super::kept(
            vec![proposal("Their name is Connie Long.", &[55])],
            &records,
            &[55],
        );
        assert_eq!((kept.len(), dropped), (0, 1));
    }

    /// "My name is Robin", seen spoken by the person: kept. So is a name
    /// spelled, or read back and agreed to, every line seen spoken by them.
    #[test]
    fn a_name_they_introduced_seen_speaking_is_kept() {
        let records = [exchange(
            7,
            "Hi Lamp, my name is Robin.",
            "Nice to meet you, Robin!",
        )];
        let (kept, dropped) = kept(vec![proposal("Their name is Robin.", &[7])], &records, &[7]);
        assert_eq!((kept.len(), dropped), (1, 0));
        for heard in ["I'm Robin.", "Call me Robin.", "Tên tôi là Robin."] {
            let records = [exchange(7, heard, "Hello Robin!")];
            let (kept, _) =
                super::kept(vec![proposal("Their name is Robin.", &[7])], &records, &[7]);
            assert_eq!(kept.len(), 1, "{heard}");
        }
        let spelled = [exchange(8, "It's R O B I N.", "Got it.")];
        let (kept, _) = super::kept(vec![proposal("Their name is Robin.", &[8])], &spelled, &[8]);
        assert_eq!(kept.len(), 1, "spelled");
        let read_back = [
            exchange(9, "Robin.", "Robin, is that right?"),
            exchange(10, "Yes.", "Great."),
        ];
        let (kept, _) = super::kept(
            vec![proposal("Their name is Robin.", &[9])],
            &read_back,
            &[9, 10],
        );
        assert_eq!(kept.len(), 1, "read back");
        // The yes not seen spoken by them: not confirmed.
        let (kept, _) = super::kept(
            vec![proposal("Their name is Robin.", &[9])],
            &read_back,
            &[9],
        );
        assert!(kept.is_empty(), "yes unseen");
        // Introduced, but not seen spoken by them: not kept.
        let (kept, _) = super::kept(vec![proposal("Their name is Robin.", &[7])], &records, &[]);
        assert!(kept.is_empty(), "unseen");
    }

    /// A misheard fragment, seen spoken by them, is no name: no
    /// introduction, a question, the lamp's own name (N4), a line whose own
    /// answer doubts what was heard (N5), or a name not cited.
    #[test]
    fn a_misheard_fragment_is_not_kept() {
        for (heard, said, text) in [
            ("Connie long.", "Sorry?", "Their name is Connie Long."),
            ("conny lo", "Hmm?", "Their name is Conny."),
            (
                "What's my name, Robin?",
                "I don't know yet.",
                "Their name is Robin.",
            ),
            ("I'm Lamp.", "That's my name!", "Their name is Lamp."),
            (
                "My name is Konnie.",
                "Did you say Connie or Konnie?",
                "Their name is Konnie.",
            ),
            ("I'm fine.", "Good to hear.", "Name is Fine."),
        ] {
            let records = [exchange(3, heard, said)];
            let (kept, dropped) = kept(vec![proposal(text, &[3])], &records, &[3]);
            assert_eq!((kept.len(), dropped), (0, 1), "{heard} / {text}");
        }
        // Introduced in a line not cited: not this proposal's evidence.
        let records = [
            exchange(3, "My name is Robin.", "Hi Robin."),
            exchange(4, "I like tea.", "Nice."),
        ];
        let (kept, _) = super::kept(
            vec![proposal("Their name is Robin.", &[4])],
            &records,
            &[3, 4],
        );
        assert!(kept.is_empty());
    }
}
