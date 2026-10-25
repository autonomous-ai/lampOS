//! A line that plainly asks the lamp to keep something calls `remember`
//! (`LAMP_MEMORY_KEEP_ASKED`). On lamp-4ace on 25 Oct (16:23, the
//! product unit, a played line), "Lamp, remember that I take my coffee
//! black." was answered "Noted: black, no sugar. That's in my head now."
//! with no call at all, so nothing was kept and the next session didn't
//! know it, though the character says never to pretend to remember. With
//! the switch, such a line's first chat round must call [`super::REMEMBER`]
//! (`tool_choice`), so what it says follows what the memory did: kept, or,
//! with nobody known, not kept and said so (`needs_face::NOT_KEPT`). A
//! round after the call ends with its result and is asked as before. Off,
//! nothing is forced.
use crate::known_faces::lower_words;

/// Only the exact value `on`; off unless set.
pub const ENVIRONMENT: &str = "LAMP_MEMORY_KEEP_ASKED";

/// Whether the switch is on.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// What may stand before the ask at a sentence's start, past a greeting.
const BEFORE: &[&str] = &[
    "hey", "hi", "hello", "ok", "okay", "lamp", "so", "and", "oh",
];
/// "Can you …", "Could you …": an ask, even ending in a question mark.
const ASKING: &[&str] = &["can", "could", "would", "will"];
/// The plain asks to keep something, at a sentence's start.
const ASKS: &[&[&str]] = &[
    &["remember", "that"],
    &["remember", "my"],
    &["remember", "i"],
    &["remember", "i'm"],
    &["remember", "we"],
    &["remember", "we're"],
    &["remember", "our"],
    &["remember", "the"],
    &["remember", "this"],
    &["don't", "forget", "that"],
    &["don't", "forget", "my"],
    &["do", "not", "forget", "that"],
    &["keep", "in", "mind", "that"],
    &["make", "a", "note", "that"],
    &["note", "that"],
];
/// A refusal before the ask ("please don't remember that"; "don't forget"
/// is itself an ask, matched first).
const REFUSED: &[&str] = &["no", "not", "never", "don't", "dont", "stop"];
/// A keep taken back anywhere in the line ("…, but do not save that",
/// "Actually, don't keep it", "Never mind"): reviewer-two's and Third's P2
/// on #3463. Matched as words in a row, wherever they are.
const WITHDRAWN: &[&[&str]] = &[
    &["don't", "save"],
    &["dont", "save"],
    &["do", "not", "save"],
    &["don't", "keep"],
    &["dont", "keep"],
    &["do", "not", "keep"],
    &["don't", "store"],
    &["do", "not", "store"],
    &["don't", "remember"],
    &["dont", "remember"],
    &["do", "not", "remember"],
    &["don't", "write"],
    &["do", "not", "write"],
    &["never", "mind"],
    &["nevermind"],
    &["forget", "it"],
    &["forget", "that"],
    &["forget", "about", "it"],
    &["scratch", "that"],
    &["cancel", "that"],
];
/// Before a withdrawal's words, what makes them the line's content, not a
/// withdrawal: someone's own habit ("I don't keep cash", "we don't save
/// receipts"); and before "forget", a negation, as "don't forget that" is
/// an ask.
const ABOUT_SOMEONE: &[&str] = &["i", "we", "they", "he", "she", "you", "it"];
const NOT_FORGET: &[&str] = &["don't", "dont", "not", "never"];

/// Whether the line takes a keep back anywhere ([`WITHDRAWN`]), clause by
/// clause: the word before a withdrawal is read only within its own clause,
/// so a sentence ending "… in it." never makes the next one's "Don't save
/// that." content (reviewer-two's P2 on `aa533296e`). It errs toward taken
/// back: then nothing is forced, and the model answers as without the
/// switch.
pub(crate) fn withdrawn(said: &str) -> bool {
    said.replace('\u{2019}', "'")
        .split(['.', '!', '?', ';', ',', ':', '\u{2014}', '\u{2013}'])
        .any(|clause| clause_withdraws(&lower_words(clause)))
}

/// [`withdrawn`] for one clause's words.
fn clause_withdraws(words: &[String]) -> bool {
    (0..words.len()).any(|at| {
        WITHDRAWN.iter().any(|phrase| {
            let matched = words.len() >= at + phrase.len()
                && words[at..at + phrase.len()]
                    .iter()
                    .zip(phrase.iter())
                    .all(|(word, withdrawn)| word == withdrawn);
            let before = at.checked_sub(1).map(|before| words[before].as_str());
            matched
                && !before.is_some_and(|word| ABOUT_SOMEONE.contains(&word))
                && !(phrase[0] == "forget" && before.is_some_and(|word| NOT_FORGET.contains(&word)))
        })
    })
}

/// Whether what the person said plainly asks the lamp to keep something
/// ([`ASKS`]) at a sentence's start: "Lamp, remember that the plant needs
/// water on Fridays", "Please remember my sister's birthday is in May",
/// "Can you remember that I take it black?", "Don't forget that I'm away
/// on Monday". Never a question of what the lamp knows ("Do you remember
/// that?", "Remember the time we went?"), a reminder ("Remember to buy
/// milk": `remind me`'s), a face or a name to be known by
/// ([`crate::known_faces::asks_to_remember`], `remember_my_face`'s), the
/// phrase further in ("She said remember that"), a refusal before it, or a
/// keep taken back anywhere in the line ([`withdrawn`]).
pub fn asks_to_keep(said: &str) -> bool {
    if crate::known_faces::asks_to_remember(said) || withdrawn(said) {
        return false;
    }
    let said = said.replace('\u{2019}', "'");
    said.split_inclusive(['.', '!', '?', ';']).any(|sentence| {
        let words = lower_words(sentence);
        let mut at = 0;
        while words
            .get(at)
            .is_some_and(|word| BEFORE.contains(&word.as_str()))
        {
            at += 1;
        }
        if words.get(at).is_some_and(|word| word == "please") {
            at += 1;
        }
        let asking = words
            .get(at)
            .is_some_and(|word| ASKING.contains(&word.as_str()))
            && words.get(at + 1).is_some_and(|word| word == "you");
        if asking {
            at += 2;
            if words.get(at).is_some_and(|word| word == "please") {
                at += 1;
            }
        }
        let a_question = sentence.trim_end().ends_with('?') && !asking;
        let asked = ASKS.iter().any(|phrase| {
            words.len() > at + phrase.len()
                && words[at..at + phrase.len()]
                    .iter()
                    .zip(phrase.iter())
                    .all(|(word, ask)| word == ask)
        });
        let refused = words[..at.min(words.len())]
            .iter()
            .any(|word| REFUSED.contains(&word.as_str()));
        !a_question && asked && !refused
    })
}

/// The force the second route's chat is given with the switch on
/// (`second_provider::net::Chat::forcing`): a request whose last message is
/// a person's line that asks to keep something must call `remember`.
pub(crate) fn force() -> crate::second_provider::net::Force {
    std::sync::Arc::new(|messages: &[crate::second_provider::net::Message]| {
        let last = messages.last().filter(|message| message.role == "user")?;
        asks_to_keep(last.words()?).then_some(crate::second_provider::net::Forced {
            name: super::REMEMBER,
            only: None,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plain asks, as speech to text writes them, against the questions,
    /// reminders, face asks, refusals and passing mentions that are not.
    #[test]
    fn a_plain_ask_to_keep_something_is_told_apart() {
        for said in [
            "Lamp, remember that I take my coffee black.",
            "Lamp. Remember that the plant by the window needs water on Fridays.",
            "Hey Lamp, please remember my sister's birthday is in May.",
            "Can you remember that I'm allergic to peanuts?",
            "Lamp, don't forget that I'm away on Monday.",
            "Okay, keep in mind that the cat is called Miso.",
            "Remember we're out of milk.",
            // Content negations are the thing to keep, not a keep taken back.
            "Remember that I don't drink coffee.",
            "Remember I don't take sugar.",
            "Remember that I don't keep cash at home.",
            "Remember that I don't remember names well.",
        ] {
            assert!(asks_to_keep(said), "{said}");
        }
        for said in [
            "Lamp, do you remember what I told you?",
            "Remember the time we went to the beach?",
            "Remember to buy milk.",
            "Lamp, remember my face, I'm Sam.",
            "Remember me?",
            "Please don't remember that.",
            "She said remember that it's late.",
            "I can't remember that song.",
            "Lamp, what do you remember?",
            "Remember that",
            // Taken back in the same line (reviewer-two's and Third's P2 on
            // #3463), in its own sentence or the same one.
            "Lamp, remember that I take my coffee black. Actually, do not save that.",
            "Remember that the plant needs water on Fridays, but do not save that.",
            "Remember my sister's birthday is in May. Never mind.",
            "Remember that I'm away on Monday. Scratch that.",
            "Remember that the code is 42, actually forget it.",
            "Remember that I like jazz. Don't keep that, though.",
            // A sentence ending in a pronoun doesn't make the next one's
            // withdrawal content (reviewer-two's P2 on `aa533296e`).
            "Remember that I take my coffee without sugar in it. Don't save that.",
            "Remember that I left the keys with you. Don't save that.",
            "Remember that the spare key is under it, do not keep that.",
        ] {
            assert!(!asks_to_keep(said), "{said}");
        }
    }

    /// The force names `remember` for such a line alone, and never for a
    /// round that ends with a call's result.
    #[test]
    fn the_force_names_remember_for_an_ask_and_nothing_else() {
        use crate::second_provider::net::Message;
        let force = force();
        let ask = [Message::user("Lamp, remember that I take my coffee black.")];
        assert_eq!(
            force(&ask).map(|forced| forced.name),
            Some(super::super::REMEMBER)
        );
        assert_eq!(force(&[Message::user("What time is it?")]), None);
        let answered = [
            Message::user("Lamp, remember that I take my coffee black."),
            Message::tool("c", "{\"remembered\":true}".to_owned()),
        ];
        assert_eq!(force(&answered), None);
    }

    /// The same P2 on the wire: the chat's first round carries a
    /// `tool_choice` for `remember` for a plain ask, and none for the same
    /// ask taken back in its line, nor without the force.
    #[test]
    fn a_keep_taken_back_in_its_line_sends_no_tool_choice() {
        use crate::second_provider::net::{Chat, Declare, Message};
        let declare: Declare = std::sync::Arc::new(|| super::super::tools_with(false));
        let forcing = Chat::deepseek("deepseek-chat", "k", declare.clone(), false).unwrap();
        forcing.forcing(force());
        let plain = Chat::deepseek("deepseek-chat", "k", declare, false).unwrap();
        let choice = |chat: &Chat, said: &str| {
            chat.wire(&[Message::system("You are a lamp."), Message::user(said)])
                .2
                .get("tool_choice")
                .cloned()
        };
        let remember =
            Some(serde_json::json!({"type": "function", "function": {"name": "remember"}}));
        let ask = "Lamp, remember that I take my coffee black.";
        assert_eq!(choice(&forcing, ask), remember);
        assert_eq!(choice(&plain, ask), None);
        for said in [
            "Lamp, remember that I take my coffee black. Actually, do not save that.",
            "Remember that the plant needs water on Fridays, but do not save that.",
            "Remember that I take my coffee without sugar in it. Don't save that.",
            "Remember that I left the keys with you. Don't save that.",
        ] {
            assert_eq!(choice(&forcing, said), None, "{said}");
        }
        // Content negations and "don't forget" are still asks on the wire.
        for said in [
            "Remember that I don't keep cash at home.",
            "Lamp, don't forget that I'm away on Monday.",
        ] {
            assert_eq!(choice(&forcing, said), remember, "{said}");
        }
    }
}
