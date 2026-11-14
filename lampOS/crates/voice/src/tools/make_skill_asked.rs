//! A line that plainly asks the lamp to make a skill calls `make_skill`
//! (`LAMP_MAKE_SKILL_ASKED`, #6835 gap 3). On lamp-52e6 on 13 Nov (build
//! 12635, the second route on deepseek-flash, `LAMP_MADE_SCRIPTS` on in all
//! three units and #6755's script words in the function's description), a
//! played ask to make a skill that checks the processor temperature and sets
//! the light was answered "I can't read my own processor temperature, so I
//! can't make that happen on my own. A skill can, though… Want me to make
//! one?" with no call, as on 12 Nov before #6755 ("I can't run a script or
//! check a temperature"). The description was right: the chat model judged
//! the ask by what it can do itself, not by what a made skill can. With the
//! switch, such a line's first chat round must call
//! [`crate::made_skill::MAKE`] (`tool_choice`), and only when it is declared
//! (`LAMP_MAKE_SKILL`); what the skill does, and whether it can, is the
//! making model's and the checker's. A line with the ask further in, a
//! question about skills, a refusal, a make taken back, and a round after a
//! call are asked as before. Off, nothing is forced.
use crate::known_faces::lower_words;
use crate::second_provider::net::{Force, Forced, Message};

/// Only the exact value `on`; off unless set.
pub const ENVIRONMENT: &str = "LAMP_MAKE_SKILL_ASKED";

/// Whether the switch is on.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// What may stand before the ask at a sentence's start: a greeting, the
/// lamp's name, a "please".
const BEFORE: &[&str] = &[
    "hey", "hi", "hello", "ok", "okay", "lamp", "so", "and", "oh", "please", "now",
];
/// The request constructions before the verb, whole: "can you", "I want
/// you to", "let's".
const ASKING: &[&[&str]] = &[
    &["can", "you"],
    &["could", "you"],
    &["would", "you"],
    &["will", "you"],
    &["i", "want", "you", "to"],
    &["i'd", "like", "you", "to"],
    &["id", "like", "you", "to"],
    &["i", "would", "like", "you", "to"],
    &["i", "need", "you", "to"],
    &["let's"],
    &["lets"],
];
/// The verbs of a make, whole.
const MAKE: &[&[&str]] = &[
    &["make"],
    &["create"],
    &["build"],
    &["write"],
    &["set", "up"],
    &["add"],
];
/// A want with no verb: "I want a skill that …".
const WANT: &[&[&str]] = &[
    &["i", "want"],
    &["i'd", "like"],
    &["id", "like"],
    &["i", "would", "like"],
    &["i", "need"],
];
/// Who it is for, after the verb.
const FOR: &[&str] = &["me", "us"];
/// The article, and a word that may stand before "skill".
const ARTICLE: &[&str] = &["a", "an"];
const KIND: &[&str] = &["new", "little", "small", "quick", "simple"];

/// How many words of `words` from `at` the first of `phrases` takes, if one.
fn phrase_at(words: &[String], at: usize, phrases: &[&[&str]]) -> Option<usize> {
    phrases
        .iter()
        .find(|phrase| {
            phrase
                .iter()
                .enumerate()
                .all(|(next, word)| words.get(at + next).is_some_and(|said| said == word))
        })
        .map(|phrase| phrase.len())
}

/// Whether `word` at `at` is one of `list`.
fn one_of(words: &[String], at: usize, list: &[&str]) -> bool {
    words
        .get(at)
        .is_some_and(|word| list.contains(&word.as_str()))
}

/// Where the ask in `words` ends, when the sentence opens with one: past
/// [`BEFORE`], a request ([`ASKING`]) and a make verb ([`MAKE`]) with "me"
/// or "us", or a want ([`WANT`]); then "a" (or "an"), maybe "new", and
/// "skill".
fn ask_ends(words: &[String]) -> Option<usize> {
    let mut at = 0;
    while one_of(words, at, BEFORE) {
        at += 1;
    }
    if let Some(taken) =
        phrase_at(words, at, WANT).filter(|_| phrase_at(words, at, ASKING).is_none())
    {
        at += taken;
    } else {
        if let Some(taken) = phrase_at(words, at, ASKING) {
            at += taken;
            if one_of(words, at, &["please"]) {
                at += 1;
            }
        }
        at += phrase_at(words, at, MAKE)?;
        if one_of(words, at, FOR) {
            at += 1;
        }
    }
    if !one_of(words, at, ARTICLE) {
        return None;
    }
    at += 1;
    if one_of(words, at, KIND) {
        at += 1;
    }
    (words.get(at)? == "skill").then_some(at + 1)
}

/// A word that, anywhere in the line, may take the make back, put it in
/// doubt or put it off: "No, wait.", "Actually, don't.", "On second
/// thought, no.", "Nah.", "but not now", "later" (the Reviewer-lead's P2 on
/// #6887: a skill with no code goes on at once with no spoken yes, so a
/// wrong force makes what the person said no to). Words, not phrases, as
/// `schedule_asked`'s: with one, nothing is forced, and the model answers
/// as it would without the switch. A skill whose own words carry one ("…
/// when I don't come back") is the model's too: a missed force costs a
/// question, a wrong one a skill nobody wanted.
const DOUBT: &[&str] = &[
    "don't",
    "dont",
    "not",
    "no",
    "nah",
    "nope",
    "wait",
    "hold",
    "cancel",
    "stop",
    "never",
    "nevermind",
    "scratch",
    "forget",
    "bother",
    "actually",
    "thought",
    "instead",
    "rather",
    "unless",
    "maybe",
    "later",
    "ignore",
    "skip",
    "scrap",
    "changed",
    "mind",
    "wondering",
    "sure",
];

/// Whether what the person said plainly asks the lamp to make a skill: the
/// line is one sentence that opens with the ask ("Lamp, make a skill that
/// checks the processor temperature and sets the light", "Can you make me
/// a skill that dims the light at 10 pm?", "I want a skill that nods when
/// I come back", "Create a new skill: …") and says what it does after it,
/// in that sentence (a `:` does not end one). Never "make a skill" with
/// nothing after it (the model asks what), a second sentence after the ask
/// ("Make a skill? Nah.", "Can you make a skill? Just wondering."), a
/// question about skills ("how do I make a skill?", "what skills did you
/// make?", "can you make skills?"), the ask further in ("she said make a
/// skill"), a refusal ("don't make a skill"), a word of [`DOUBT`] anywhere
/// in the line, or a make taken back
/// ([`crate::remember::asked::withdrawn`]: "never mind", "scratch that").
pub fn asks_to_make(said: &str) -> bool {
    if crate::remember::asked::withdrawn(said) {
        return false;
    }
    let said = said.replace('\u{2019}', "'");
    let sentences: Vec<Vec<String>> = said
        .split_inclusive(['.', '!', '?', ';'])
        .map(lower_words)
        .filter(|words| !words.is_empty())
        .collect();
    let [words] = sentences.as_slice() else {
        return false;
    };
    if words.iter().any(|word| DOUBT.contains(&word.as_str())) {
        return false;
    }
    ask_ends(words).is_some_and(|end| words.len() > end)
}

/// The force the second route's chat is given with the switch on
/// (`second_provider::net::Chat::forcing`): a request whose last message is
/// a person's line that plainly asks to make a skill must call
/// `make_skill`. The chat forces it only when `make_skill` is declared.
pub fn force() -> Force {
    std::sync::Arc::new(|messages: &[Message]| {
        let last = messages.last().filter(|message| message.role == "user")?;
        asks_to_make(last.words()?).then_some(Forced {
            name: crate::made_skill::MAKE,
            only: None,
            fields: None,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The played make of #6835 gap 3, and the asks a person says, as
    /// speech to text writes them, against questions, refusals and passing
    /// mentions that are not.
    #[test]
    fn a_plain_ask_to_make_a_skill_is_told_apart() {
        for said in [
            // row-6413d4b's clip on lamp-52e6, 13 Nov.
            "Lamp, make a skill that checks the processor temperature and sets the light.",
            "Hey Lamp, make a skill that sets the light from the processor temperature.",
            "Make me a skill that dims the light at 10 pm.",
            "Can you make me a skill that nods when I come back to my desk?",
            "Could you please create a skill that says hello when I sit down?",
            "Lamp, I want a skill that turns the light red when the CPU is hot.",
            "I'd like you to make a skill that checks the weather every morning.",
            "Okay, build a new skill: when I come back to my desk, nod at me.",
            "Let's make a skill that hums at noon.",
            "Write a skill that runs a script to read the temperature.",
            "Make a skill: when I come back to my desk, nod at me",
        ] {
            assert!(asks_to_make(said), "{said}");
        }
        for said in [
            "Lamp, make a skill.",
            "Can you make a skill?",
            "Can you make skills?",
            "How do I make a skill?",
            "What skills did you make?",
            "Did you make a skill for me?",
            "She said make a skill that dims the light.",
            "Don't make a skill that dims the light.",
            "Please don't make a skill for that.",
            "Make a skill that dims the light at 10 pm. Never mind.",
            "Make a skill that hums at noon, actually scratch that.",
            "Make it brighter.",
            "Make the light warmer.",
            "I like the skill that dims the light.",
            "Lamp, use the skill that dims the light.",
            "Is there a skill that dims the light?",
            // The Reviewer-lead's probes on #6887: a take-back, a doubt or
            // a put-off anywhere, or a second sentence after the ask.
            "Make a skill that dims the light. No, wait.",
            "Make a skill that dims the light. Actually, don't.",
            "Make a skill that dims the light. On second thought, no.",
            "Make a skill? Nah.",
            "Can you make a skill? Just wondering.",
            "Could you make a skill? I'm not sure you can.",
            "I want a skill that dims the light, but not now.",
            "Make a skill later that dims the light.",
            "Lamp, make a skill. It checks the processor temperature and sets the light.",
            // A skill whose own words carry a doubt word is the model's.
            "Lamp, make a skill that turns the light off when I don't come back.",
        ] {
            assert!(!asks_to_make(said), "{said}");
        }
    }

    /// The force names `make_skill` for such a line alone, and never for a
    /// round that ends with a call's result.
    #[test]
    fn the_force_names_make_skill_for_an_ask_and_nothing_else() {
        let force = force();
        let ask = [Message::user(
            "Lamp, make a skill that checks the processor temperature and sets the light.",
        )];
        assert_eq!(
            force(&ask).map(|forced| forced.name),
            Some(crate::made_skill::MAKE)
        );
        assert_eq!(force(&[Message::user("What time is it?")]), None);
        let answered = [
            ask[0].clone(),
            Message::tool("c", "{\"made\":true}".to_owned()),
        ];
        assert_eq!(force(&answered), None);
    }

    /// On the wire: the first round of the played make carries a
    /// `tool_choice` for `make_skill` when it is declared, none without the
    /// force, and none when `make_skill` isn't declared (`LAMP_MAKE_SKILL`
    /// off).
    #[test]
    fn the_played_make_sends_a_tool_choice_for_make_skill_when_declared() {
        use crate::second_provider::net::{Chat, Declare};
        let made: Declare = std::sync::Arc::new(|| {
            let mut tools = crate::remember::tools_with(false);
            tools.push(crate::make_skill_local::tool());
            tools
        });
        let without: Declare = std::sync::Arc::new(|| crate::remember::tools_with(false));
        let forcing = Chat::deepseek("deepseek-chat", "k", made.clone(), false).unwrap();
        forcing.forcing(force());
        let plain = Chat::deepseek("deepseek-chat", "k", made, false).unwrap();
        let undeclared = Chat::deepseek("deepseek-chat", "k", without, false).unwrap();
        undeclared.forcing(force());
        let choice = |chat: &Chat, said: &str| {
            chat.wire(&[Message::system("You are a lamp."), Message::user(said)])
                .2
                .get("tool_choice")
                .cloned()
        };
        let make = Some(
            serde_json::json!({"type": "function", "function": {"name": crate::made_skill::MAKE}}),
        );
        let ask = "Lamp, make a skill that checks the processor temperature and sets the light.";
        assert_eq!(choice(&forcing, ask), make);
        assert_eq!(choice(&plain, ask), None);
        assert_eq!(choice(&undeclared, ask), None);
        assert_eq!(choice(&forcing, "How do I make a skill?"), None);
    }
}
