//! A reply that opens with a don't-know about the person's own life, on a
//! line that asks about themself, recalls first
//! (`LAMP_MEMORY_DONT_KNOW_RECALLS`, task C57 after #4522).
//!
//! On lamp-06c1 on 30 Oct the lamp answered 1 to 2 of 15 questions about
//! things the person had asked it to keep, after #4522 told the model to
//! call `recall` before saying it doesn't know (the probe had 16 of 20). The
//! rest were "I don't know" or "I'm not sure" with the answer kept.
//!
//! With the switch, the second route holds a reply's first sentence on a
//! spoken line that names the person ([`about_themself`]: "my", "me", "I",
//! "tôi", "mình") until it is known whether it says a don't-know anywhere in
//! it ([`opens_dont_know`]: "Your goldfish — I don't remember that." is one).
//! One that does is searched for at once: the kept
//! things of the line's own person that share its words, as memory's hits
//! read them (`crate::remember::hits_note_counted`, within
//! `crate::remember::HITS_WITHIN`). Never the room's, and never anyone else's:
//! only a request whose own scope names a person (its lease, or the face and
//! the session agreeing without one) is searched. Never a line carried into
//! their conversation going on (`LAMP_CONVERSATION_CARRIES`): no face was
//! seen speaking it, and an off-camera guest beside them looks the same
//! (Heron's P2 on #5508). With kept things found, the
//! don't-know is not said, and the model is asked once more with them and
//! [`NOTE`]. With none, the held words go on as they came. Once a turn, and
//! never after the turn called `recall` itself.
//!
//! Off unless the environment says exactly `on`; off, every reply goes on
//! byte for byte as before.

/// Where the second worker reads the switch. `on` only.
pub const ENVIRONMENT: &str = "LAMP_MEMORY_DONT_KNOW_RECALLS";

/// Whether the switch is on in this process's environment.
pub fn on() -> bool {
    switch(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Whether a value of [`ENVIRONMENT`] turns it on: `on` only.
pub fn switch(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Told before the round asked again, with the kept things found after it.
pub const NOTE: &str = "(For you to know: you were about to say you don't know or aren't sure, but your memory holds what this person asked you to keep that may answer them, below. Answer their question from it in one or two short sentences, as theirs. If none of it answers what they asked, say only that you don't remember; never make anything up.)";

/// Words that say the lamp doesn't know, as their words: lower-case,
/// apostrophes straight, one space between words. Found anywhere in the
/// reply's first sentence ([`opens_dont_know`]): on lamp-06c1 on 5 Nov (S14)
/// "Your goldfish — I don't remember that.", "I don't have a time for your
/// yoga class — that's not something you've told me." and "I can't tell you
/// a lucky number — I've no way to know yours." were don't-knows an opening
/// match never caught.
const DONT_KNOW: &[&str] = &[
    "i don't know",
    "i do not know",
    "i dont know",
    "i'm not sure",
    "i am not sure",
    "im not sure",
    "i'm not certain",
    "i don't remember",
    "i do not remember",
    "i dont remember",
    "i don't recall",
    "i do not recall",
    "i can't remember",
    "i cannot remember",
    "i can't recall",
    "i have no idea",
    "i've no idea",
    "no idea",
    "i don't have that",
    "no way to know",
    "i don't think you've told me",
    "i don't think you told me",
    "you haven't told me",
    "you didn't tell me",
    "you never told me",
    "you've never told me",
    "not something you've told me",
    "not something you told me",
    "nothing's been kept",
    "nothing has been kept",
    "i'm afraid i don't",
    "i am afraid i don't",
    "tôi không biết",
    "tôi không nhớ",
    "tôi không chắc",
    "mình không biết",
    "mình không nhớ",
    "mình không chắc",
    "em không biết",
    "em không nhớ",
    "em không chắc",
    "không biết",
    "không nhớ",
    "bạn chưa nói",
    "bạn chưa kể",
    "bạn chưa từng nói",
    "bạn chưa bao giờ nói",
    "chưa được lưu",
];

/// What a [`QUALIFIED`] phrase needs after it, in its sentence, to say the
/// lamp doesn't know.
enum Needs {
    /// Its next word is one of these: "I can't tell you a lucky number", never
    /// "I can't tell you how glad I am!".
    Next(&'static [&'static str]),
    /// One of these words later in its sentence: "nothing about those has been
    /// kept", never "there's nothing about it to worry about"; "I don't have a
    /// lucky number for you", never "I don't have a favorite myself" (Thirteen's
    /// P2 on #5508).
    Later(&'static [&'static str]),
}

/// Words that say what the person told or the lamp kept.
const KEPT_WORDS: &[&str] = &[
    "kept", "keep", "told", "tell", "saved", "noted", "remember", "know", "known",
];

/// Words that tie a "have" to the person: theirs, or what they told.
const THEIRS_WORDS: &[&str] = &[
    "you", "your", "yours", "kept", "told", "saved", "noted", "remember", "know",
];

/// Vietnamese: the person's ("bạn").
const BAN: &[&str] = &["bạn"];

/// Phrases that say the lamp doesn't know only with what follows them
/// ([`Needs`]): each, alone, is common in replies that are no don't-know.
const QUALIFIED: &[(&str, Needs)] = &[
    ("i don't have any", Needs::Later(THEIRS_WORDS)),
    ("i don't have a", Needs::Later(THEIRS_WORDS)),
    ("i don't have an", Needs::Later(THEIRS_WORDS)),
    ("i don't have your", Needs::Later(&[])),
    ("i do not have a", Needs::Later(THEIRS_WORDS)),
    ("i do not have any", Needs::Later(THEIRS_WORDS)),
    ("i dont have a", Needs::Later(THEIRS_WORDS)),
    ("i dont have any", Needs::Later(THEIRS_WORDS)),
    ("i haven't got a", Needs::Later(THEIRS_WORDS)),
    ("i haven't got any", Needs::Later(THEIRS_WORDS)),
    (
        "i can't tell you",
        Needs::Next(&[
            "a", "an", "your", "yours", "that", "what", "where", "when", "which", "who", "whether",
            "if",
        ]),
    ),
    (
        "i cannot tell you",
        Needs::Next(&[
            "a", "an", "your", "yours", "that", "what", "where", "when", "which", "who", "whether",
            "if",
        ]),
    ),
    ("nothing about", Needs::Later(KEPT_WORDS)),
    ("tôi không có", Needs::Later(BAN)),
    ("mình không có", Needs::Later(BAN)),
    ("mình chưa có", Needs::Later(BAN)),
    ("em không có", Needs::Later(BAN)),
    ("em chưa có", Needs::Later(BAN)),
    ("chưa có gì về", Needs::Later(BAN)),
    ("không có gì về", Needs::Later(BAN)),
];

/// Whether `said` (its words, a space before and after) has `phrase` with
/// what it `needs` after it.
fn qualified(said: &str, phrase: &str, needs: &Needs) -> bool {
    let padded = format!(" {phrase} ");
    said.match_indices(&padded).any(|(at, _)| {
        let rest = &said[at + padded.len()..];
        match needs {
            Needs::Next(next) => rest
                .split(' ')
                .next()
                .is_some_and(|word| next.contains(&word)),
            Needs::Later(later) => {
                later.is_empty() || rest.split(' ').any(|word| later.contains(&word))
            }
        }
    })
}

/// How much of a reply's first sentence is held at most while its end is
/// awaited: past it, the words so far decide.
const FIRST_SENTENCE_CHARS: usize = 200;

/// Where a sentence ends.
const SENTENCE_ENDS: [char; 8] = [
    '.', '!', '?', '\n', '\u{2026}', '\u{3002}', '\u{ff1f}', '\u{ff01}',
];

/// Words of a line that name the person asking.
const ABOUT_THEMSELF: &[&str] = &[
    "i", "i'm", "im", "i've", "ive", "i'd", "i'll", "me", "my", "mine", "myself", "tôi", "mình",
];

/// `text` as its words: lower-case, letters, digits and apostrophes
/// ("’" as "'"), one space between.
fn words(text: &str) -> String {
    text.to_lowercase()
        .replace('’', "'")
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(|word| word.trim_matches('\''))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether the line `heard` names the person asking: a question about their
/// own life ("what's my favourite snack?", "where am I going on Friday?").
pub fn about_themself(heard: &str) -> bool {
    words(heard)
        .split(' ')
        .any(|word| ABOUT_THEMSELF.contains(&word))
}

/// Whether the reply so far, `held`, opens with a don't-know: one of
/// [`DONT_KNOW`]'s words, or a [`QUALIFIED`] phrase with what it needs,
/// anywhere in its first sentence ("Dr." or "3.5" ends it early, deciding
/// false). `Some(true)` it
/// does, `Some(false)` it does not, `None` not known yet (its first sentence
/// has not ended). `whole` when nothing more will come: never `None`. A
/// first sentence longer than [`FIRST_SENTENCE_CHARS`] is decided by what
/// was held.
pub fn opens_dont_know(held: &str, whole: bool) -> Option<bool> {
    let end = held.find(SENTENCE_ENDS);
    let first = match end {
        Some(at) => &held[..at],
        // Still coming: a last word may not be whole yet ("no idea" of
        // "no ideal").
        None if !whole => {
            held.trim_end_matches(|c: char| c.is_alphanumeric() || c == '\'' || c == '’')
        }
        None => held,
    };
    let said = format!(" {} ", words(first));
    if DONT_KNOW
        .iter()
        .any(|phrase| said.contains(&format!(" {phrase} ")))
        || QUALIFIED
            .iter()
            .any(|(phrase, needs)| qualified(&said, phrase, needs))
    {
        return Some(true);
    }
    if whole || end.is_some() || held.chars().count() > FIRST_SENTENCE_CHARS {
        return Some(false);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dont_know_is_known_from_its_opening() {
        for reply in [
            "I don't know.",
            "I don’t know your favourite snack yet.",
            "Hmm, I'm not sure.",
            "Sorry, I don't remember that.",
            "I'm afraid I don't know.",
            "You haven't told me that.",
            "Tôi không nhớ.",
        ] {
            assert_eq!(opens_dont_know(reply, true), Some(true), "{reply}");
        }
        for reply in [
            "Your favourite snack is pretzels.",
            "I think it's pretzels.",
            "In the top drawer.",
            "I know! Pretzels.",
            "I don't.",
            // lamp-06c1, 5 Nov (S14): answers, not don't-knows.
            "Denver — you told me your brother lives there.",
            "Purple — you told me that.",
            "Thursdays — you told me your book club meets on Thursdays.",
            // A don't-know after the first sentence is not its opening.
            "Pretzels. I don't know the brand.",
            // Thirteen's P2 on #5508: no don't-knows.
            "I can't tell you how glad I am!",
            "There's nothing about it to worry about.",
            "I don't have a favorite myself, but…",
            "I don't have a favorite myself, but rain sounds lovely.",
            "Mình không có ý đó.",
        ] {
            assert_eq!(opens_dont_know(reply, true), Some(false), "{reply}");
        }
    }

    /// lamp-06c1, 5 Nov (S14, build 10343, the switch on): each reply was
    /// said over a kept thing, and none was caught at its opening. Each is
    /// one now, in English and in Vietnamese.
    #[test]
    fn a_dont_know_anywhere_in_the_first_sentence_is_one() {
        for reply in [
            // Requests 60 and 73 on run 15:08.
            "I don't have a lucky number for you — that's not something you've told me. What is it?",
            "I don't have a favorite board game for you — that's not something you've told me. What is it?",
            // Request 74.
            "Your goldfish — I don't remember that. What's it called?",
            // Request 75.
            "I don't have a time for your yoga class — that's not something you've told me. When is it?",
            // Run 14:21, requests 460, 470 and 477.
            "I can't tell you a lucky number — I've no way to know yours. Pick one you like the look of.",
            "I don't remember any running shoes — nothing about those has been kept.",
            "Running shoes — nothing about those has been kept.",
            "Soup — nothing's been kept about that.",
            "That's not something you've told me.",
            "Cá vàng của bạn — mình không nhớ.",
            "Mình không có giờ học yoga của bạn — bạn chưa nói với mình.",
            "Về cờ — chưa có gì về trò chơi của bạn.",
            "Số may mắn của bạn — chưa được lưu.",
        ] {
            assert_eq!(opens_dont_know(reply, true), Some(true), "{reply}");
            // Known by its first sentence, before the rest comes.
            let first = reply.split_inclusive(SENTENCE_ENDS).next().unwrap();
            assert_eq!(opens_dont_know(first, false), Some(true), "{first}");
        }
    }

    #[test]
    fn a_first_sentence_is_held_until_it_ends() {
        assert_eq!(opens_dont_know("", false), None);
        assert_eq!(opens_dont_know("I", false), None);
        assert_eq!(opens_dont_know("I do", false), None);
        assert_eq!(opens_dont_know("Hmm, ", false), None);
        assert_eq!(opens_dont_know("I don't kn", false), None);
        // Its last word may still grow.
        assert_eq!(opens_dont_know("I don't know", false), None);
        assert_eq!(opens_dont_know("I don't know ", false), Some(true));
        assert_eq!(opens_dont_know("Your goldfish — ", false), None);
        assert_eq!(
            opens_dont_know("Your goldfish — I don't remember ", false),
            Some(true)
        );
        // Decided at its first sentence's end.
        assert_eq!(opens_dont_know("Your", false), None);
        assert_eq!(
            opens_dont_know("Denver — you told me so.", false),
            Some(false)
        );
        assert_eq!(opens_dont_know("I think", false), None);
        assert_eq!(opens_dont_know("I.", false), Some(false));
        assert_eq!(opens_dont_know("I", true), Some(false));
        // A first sentence past its bound is decided by what was held.
        let long = "Your favourite ".repeat(20);
        assert_eq!(opens_dont_know(&long, false), Some(false));
    }

    #[test]
    fn a_line_about_themself_names_them() {
        assert!(about_themself("What's my favourite snack?"));
        assert!(about_themself("Where am I going on Friday?"));
        assert!(about_themself("Trà của tôi là gì?"));
        assert!(!about_themself("What's the capital of France?"));
        assert!(!about_themself("Tell a joke."));
    }

    #[test]
    fn only_on_turns_it_on() {
        assert!(switch(Some("on")));
        for value in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
            assert!(!switch(value), "{value:?}");
        }
    }
}
