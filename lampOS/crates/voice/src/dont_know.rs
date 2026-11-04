//! A reply that opens with a don't-know about the person's own life, on a
//! line that asks about themself, recalls first
//! (`LAMP_MEMORY_DONT_KNOW_RECALLS`, task C57 after #4522).
//!
//! On lamp-06c1 on 30 Oct the lamp answered 1 to 2 of 15 questions about
//! things the person had asked it to keep, after #4522 told the model to
//! call `recall` before saying it doesn't know (the probe had 16 of 20). The
//! rest were "I don't know" or "I'm not sure" with the answer kept.
//!
//! With the switch, the second route holds a reply's opening on a spoken
//! line that names the person ([`about_themself`]: "my", "me", "I", "tôi",
//! "mình") until it is known whether it opens with a don't-know
//! ([`opens_dont_know`]). One that does is searched for at once: the kept
//! things of the line's own person that share its words, as memory's hits
//! read them (`crate::remember::hits_note_counted`, within
//! `crate::remember::HITS_WITHIN`). Never the room's, and never anyone else's:
//! only a request whose own scope names a person (its lease, or the face and
//! the session agreeing without one) is searched. With kept things found, the
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

/// Openings that say the lamp doesn't know, as their words: lower-case,
/// apostrophes straight, one space between words.
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
    "i don't have any",
    "i don't think you've told me",
    "i don't think you told me",
    "you haven't told me",
    "you didn't tell me",
    "you never told me",
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
];

/// Words a don't-know may open with before it ("Hmm, sorry, I don't know").
const FILLERS: &[&str] = &[
    "hmm", "hm", "um", "uh", "oh", "ah", "well", "sorry", "so", "honestly", "actually", "xin",
    "lỗi", "ừm", "à",
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

/// Whether the reply so far, `held`, opens with a don't-know: `Some(true)`
/// it does, `Some(false)` it does not, `None` not known yet (its words so far
/// may still become one). `whole` when nothing more will come: never `None`.
pub fn opens_dont_know(held: &str, whole: bool) -> Option<bool> {
    let mut rest = words(held);
    // Fillers first, each with what follows it.
    'fillers: loop {
        for filler in FILLERS {
            if let Some(after) = rest
                .strip_prefix(filler)
                .and_then(|after| after.strip_prefix(' '))
            {
                rest = after.to_owned();
                continue 'fillers;
            }
        }
        break;
    }
    if DONT_KNOW
        .iter()
        .any(|phrase| rest == *phrase || rest.starts_with(&format!("{phrase} ")))
    {
        return Some(true);
    }
    // A sentence already ended, or written whole: what it is, it is.
    let ended = held.contains(['.', '!', '?', '\n']);
    // Nothing yet, or the start of one (every phrase starts with "").
    let may_become = DONT_KNOW
        .iter()
        .chain(FILLERS)
        .any(|phrase| phrase.starts_with(rest.as_str()));
    if whole || ended || !may_become {
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
        ] {
            assert_eq!(opens_dont_know(reply, true), Some(false), "{reply}");
        }
    }

    #[test]
    fn an_opening_is_held_only_while_it_may_still_become_one() {
        assert_eq!(opens_dont_know("", false), None);
        assert_eq!(opens_dont_know("I", false), None);
        assert_eq!(opens_dont_know("I do", false), None);
        assert_eq!(opens_dont_know("Hmm, ", false), None);
        assert_eq!(opens_dont_know("I don't kn", false), None);
        assert_eq!(opens_dont_know("I don't know", false), Some(true));
        // Decided at its first word that cannot open one.
        assert_eq!(opens_dont_know("Your", false), Some(false));
        assert_eq!(opens_dont_know("I think", false), Some(false));
        assert_eq!(opens_dont_know("I.", false), Some(false));
        assert_eq!(opens_dont_know("I", true), Some(false));
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
