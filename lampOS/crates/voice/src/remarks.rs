//! A remark to the room is not a reply to the lamp (#829, scene 3), off
//! unless `LAMP_REMARKS=on`. The Live route only: the second voice's rule
//! (`crate::second_provider`) has no conversation rule to narrow.
//!
//! In a busy room the voice's rule lets out whatever is said while a
//! conversation with the lamp is going on (`crate::attention`, "a
//! conversation with the lamp is going on"), whoever says it. In the
//! battery's scene 3 on lamp-4ace, on the night of 17 October, "I should
//! bring an umbrella then.", said by one person to the other seconds after
//! the lamp's weather answer, was answered 10 times in 10; so was "Okay, I
//! will book the car for eight." A person at the next desk does not answer
//! a remark said across the room: they nod, or say nothing.
//!
//! With the switch, such a line is held as "a remark to the room after the
//! lamp's answer" when all of these hold:
//! - the room is busy, and it was let out only as the conversation going
//!   on: not its name heard, not the text calling it, nobody turned to the
//!   lamp, nobody close to it, not said to it from close;
//! - it is not an answer to the lamp's own question ([`lamp_asked`]);
//! - it looks like a remark ([`remark`]): past "okay", "yes, and" and the
//!   like it begins with a person who does something ("I", "we", "she",
//!   "they", "I'll"…), it has four words or more, and it neither wants
//!   nor needs anything ("need", "want", "like", "too", "wonder"); and it
//!   asks nothing ([`asks`]). A line is held for what it is, never for
//!   what it lacks: "Warmer.", "And tomorrow", "Lights off", "I need more
//!   light" and "It's too dark in here" all play (the reviewer on #1499);
//!   so does a bare answer ("Yes go ahead", "No, not now."), whatever the
//!   lamp asked;
//! - it is English as far as its letters show: every letter plain ASCII
//!   ([`english_letters`]). A line in another language has none of these
//!   cues, Vietnamese among them for its marks, so it is never held here
//!   (Fourth on #705, 13:01).
//!
//! What it costs: a remark meant for the lamp that begins with a person
//! ("I really liked that story.") goes unanswered in a busy room unless
//! the name is said or the person turns to the lamp; a remark in another
//! language is still answered. Of the night's 32 lines the first builder's
//! first filter would have held (#705, 13:0x), the 11 remarks to the room
//! are held and the 13 of ours said to the lamp play: the colour line (6),
//! the answers to "Shall I start it?" (6) and a request heard without its
//! name (1). The 8 lines heard in the office that are none of ours are not
//! judged here.
//!
//! And the lamp's own question is known by the end of its line as it was
//! said, not by what is kept of it ([`lamp_asked`]): a read-back "… Shall I
//! start it?" longer than the thread keeps (`crate::thread`, 400 bytes)
//! lost its `?`, and Gemini's transcript of the lamp's words often has
//! none.
//!
//! Nothing heard is written anywhere here: what was heard is read, and only
//! the verdict's reason is said.

/// Where the coordinator reads the switch. `on` only.
pub const ENVIRONMENT: &str = "LAMP_REMARKS";
/// The provider worker's flag for it, after `--provider-patience`.
pub const FLAG: &str = "--remarks";
/// The verdict's reason.
pub const REMARK: &str =
    "a remark to the room after the lamp's answer, in a busy room, asking nothing of the lamp";

/// Words a question begins with, or a helping verb that makes one: Gemini's
/// transcript often drops the `?` ("and what about tomorrow"). With their
/// contractions, which are one word here ("what's", and "whats" as Gemini
/// may write it).
const QUESTION_WORDS: [&str; 34] = [
    "what", "how", "when", "where", "why", "who", "whom", "whose", "which", "can", "could",
    "would", "will", "shall", "should", "is", "are", "do", "does", "did", "may", "might", "what's",
    "how's", "where's", "who's", "when's", "why's", "whats", "hows", "wheres", "whos", "isn't",
    "aren't",
];

/// Words that tell someone to do something, said first: "Remember that my
/// favourite colour is green." asks the lamp to keep it.
const DOING_WORDS: [&str; 48] = [
    "remember", "note", "forget", "keep", "let", "tell", "make", "set", "play", "turn", "remind",
    "check", "show", "start", "stop", "give", "read", "say", "find", "write", "ask", "put", "help",
    "sing", "dim", "brighten", "open", "close", "call", "send", "add", "change", "go", "come",
    "look", "listen", "wait", "hold", "repeat", "explain", "count", "spell", "switch", "cancel",
    "let's", "lets", "don't", "dont",
];

/// Words that come before the first that counts: "And what about…",
/// "Okay, now tell me…", "Yes, and we should…".
const LEADING: [&str; 17] = [
    "and", "so", "but", "then", "okay", "ok", "oh", "well", "now", "please", "hey", "hmm", "um",
    "yes", "yeah", "no", "right",
];

/// Words that speak to the lamp, wherever they are.
const TO_THE_LAMP: [&str; 6] = ["you", "your", "yours", "lamp", "please", "thanks"];

/// A person who does something, as a remark to the room begins ("I should
/// bring an umbrella then.", "we should leave a bit early").
const SUBJECTS: [&str; 24] = [
    "i", "we", "he", "she", "they", "i'll", "i'm", "i'd", "i've", "we'll", "we're", "we'd",
    "we've", "he's", "he'll", "he'd", "she's", "she'll", "she'd", "they're", "they'll", "they'd",
    "they've", "ill",
];

/// Words that make a line about the person a wish the lamp may answer, by
/// their start, so every form counts ("I need more light", "She needs a
/// timer", "I was wondering if it will rain", "I'm hoping for a poem", "I'd
/// love to hear a joke", "I'd prefer something warmer"): the reviewer on
/// #1499.
const WANT_STARTS: [&str; 8] = [
    "need", "want", "lik", "wonder", "wish", "hop", "lov", "prefer",
];
/// The same, as whole words only: "it's too dark", "I'd rather have some
/// jazz", "I could use more light".
const WANTS: [&str; 5] = ["too", "rather", "use", "wanna", "missed"];

/// A line that says the person could not take something in asks the lamp
/// to say it again or to help: "I didn't catch that", "I can't hear it",
/// "I can't read in this light", "I'm not sure I follow". A word of
/// [`CANNOT`] followed, within three words, by one of [`TAKE_IN`].
const CANNOT: [&str; 10] = [
    "can't", "cant", "cannot", "couldn't", "couldnt", "didn't", "didnt", "don't", "dont", "not",
];
const TAKE_IN: [&str; 7] = [
    "hear",
    "see",
    "catch",
    "understand",
    "read",
    "follow",
    "get",
];

/// Whether a word of a line makes it a wish ([`WANT_STARTS`], [`WANTS`]).
fn wants(word: &str) -> bool {
    WANTS.contains(&word) || WANT_STARTS.iter().any(|start| word.starts_with(start))
}

/// Whether a line says the person could not take something in ([`CANNOT`]).
fn could_not_take_in(heard: &str) -> bool {
    let all: Vec<String> = words(heard).collect();
    all.iter().enumerate().any(|(at, word)| {
        CANNOT.contains(&word.as_str())
            && all[at + 1..]
                .iter()
                .take(3)
                .any(|next| TAKE_IN.contains(&next.as_str()))
    })
}

/// The fewest words a remark has: "I should go." is three and passes.
const REMARK_WORDS: usize = 4;

/// Helping verbs a question of the lamp's begins with, followed by a person
/// ("Shall I start it", "Would you like another"): not "Will do." or
/// "Should be sunny tomorrow."
const LAMP_ASKS_WITH: [&str; 13] = [
    "shall", "should", "would", "will", "do", "does", "did", "can", "could", "is", "are", "may",
    "might",
];
const PEOPLE: [&str; 11] = [
    "i", "you", "we", "it", "that", "this", "he", "she", "they", "me", "there",
];
/// Words a question of the lamp's begins with when a helping verb follows
/// within four words ("Which one would you like", "What kind of music do
/// you like"), "is" or "are" within two: not "What a lovely poem.", "How
/// nice." or "How nice it is to see you."
const LAMP_ASKS_WH: [&str; 8] = [
    "what", "which", "when", "where", "how", "who", "whom", "whose",
];

/// Whether a line's letters are all plain ASCII, so it can be read as
/// English here: any other letter, Latin with marks among them, and it is
/// not.
pub fn english_letters(heard: &str) -> bool {
    heard.chars().any(char::is_alphabetic)
        && heard
            .chars()
            .filter(|c| c.is_alphabetic())
            .all(|c| c.is_ascii_alphabetic())
}

fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .map(|word| word.to_lowercase().replace('\u{2019}', "'"))
}

/// A sentence's words past its leading ones.
fn past_leading(sentence: &str) -> impl Iterator<Item = String> + '_ {
    words(sentence).skip_while(|word| LEADING.contains(&word.as_str()))
}

/// Whether a line asks something of whoever hears it: a `?`, a sentence
/// that begins as a question or with a word that tells them to do
/// something, or a word that speaks to the lamp ("you", "please",
/// "thanks", "thank you").
pub fn asks(heard: &str) -> bool {
    if heard.contains('?') || words(heard).any(|word| TO_THE_LAMP.contains(&word.as_str())) {
        return true;
    }
    heard.split(['.', '!', ';', ',']).any(|sentence| {
        past_leading(sentence).next().is_some_and(|first| {
            QUESTION_WORDS.contains(&first.as_str()) || DOING_WORDS.contains(&first.as_str())
        })
    })
}

/// Whether one sentence or clause asks or tells whoever hears it something:
/// it ends in a `?`, says "you" or "please", or past its leading words
/// begins as a question or with a word that tells them to do something.
/// Unlike [`asks`], "your", "thanks" and the lamp's name alone ask nothing
/// here, so a remark about the lamp ("I love your lamp") is not a request
/// (#4175, Fourth's P2 on #4201: "Turn off the lamp.", "Can you dim the
/// lamp?").
pub fn asks_in(clause: &str) -> bool {
    clause.trim_end().ends_with('?')
        || words(clause).any(|word| word == "you" || word == "please")
        || past_leading(clause).next().is_some_and(|first| {
            QUESTION_WORDS.contains(&first.as_str()) || DOING_WORDS.contains(&first.as_str())
        })
}

/// Whether a line is a question, or names the lamp: a `?`, a sentence that
/// begins with a question word or a helping verb (after one "and", "so",
/// "but" or "then" at most: "And how long would that take by train" with no
/// `?`), or the lamp's name. Narrower than [`asks`]: "you", "please",
/// "thanks" or a word that tells someone to do something make no question
/// here (the reviewer and the first builder on #1636: "Thanks, I'll take the
/// car.", said to another person, asks nothing of the lamp).
pub fn questions(heard: &str) -> bool {
    const ONE_BEFORE: [&str; 4] = ["and", "so", "but", "then"];
    heard.contains('?')
        || words(heard).any(|word| word == "lamp")
        || heard.split(['.', '!', ';']).any(|sentence| {
            let mut sentence = words(sentence);
            match sentence.next() {
                Some(first) if ONE_BEFORE.contains(&first.as_str()) => sentence
                    .next()
                    .is_some_and(|second| QUESTION_WORDS.contains(&second.as_str())),
                Some(first) => QUESTION_WORDS.contains(&first.as_str()),
                None => false,
            }
        })
}

#[cfg(test)]
mod question_tests {
    /// The reviewer on #1636: a question, not a line with "you" or a word
    /// that tells someone to do something.
    #[test]
    fn a_question_is_a_question_mark_or_a_question_word_first() {
        for question in [
            "What about a name for a black kitten then?",
            "And how long would that take by train?",
            "and what about tomorrow",
            "so which one is cheaper",
            "Lamp, the black kitten",
            "Do you like that name?",
        ] {
            assert!(super::questions(question), "{question}");
        }
        for statement in [
            "Yes, and you should bring yours too.",
            "Okay, let's leave a bit early then.",
            "Thanks, I'll take the car.",
            "Yes, and we should leave a bit early.",
            "But the client wants to meet in person.",
            "Remember that my favourite colour is green.",
            "Okay, so what now",
        ] {
            assert!(!super::questions(statement), "{statement}");
        }
    }
}

/// Whether a line is a remark to the room: English as its letters show; past
/// its leading words it begins with a person who does something, has
/// [`REMARK_WORDS`] words or more and wants nothing ([`WANTS`]); and it
/// asks nothing ([`asks`]).
pub fn remark(heard: &str) -> bool {
    let all: Vec<String> = words(heard).collect();
    english_letters(heard)
        && all.len() >= REMARK_WORDS
        && past_leading(heard)
            .next()
            .is_some_and(|first| SUBJECTS.contains(&first.as_str()))
        && !all.iter().any(|word| wants(word))
        && !could_not_take_in(heard)
        && !asks(heard)
}

/// Whether the lamp's words, as the end of them was last heard, asked a
/// question: a `?` anywhere in that end ("Shall I start it? It takes about
/// a minute."), or its last sentence asks without one: a helping verb and a
/// person ("Shall I start it", "Is there anything else"), a question word
/// with a helping verb within four words after it ("Which one would you
/// like"), "how about", "or would", "Want me to…", or "let me know".
pub fn lamp_asked(said_last: &str) -> bool {
    if said_last.contains('?') {
        return true;
    }
    let said = said_last.trim_end().trim_end_matches(['.', '!', ' ']);
    let Some(last) = said.rsplit(['.', '!']).next() else {
        return false;
    };
    let opening: Vec<String> = past_leading(last).take(5).collect();
    let asks_with = match opening.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["want", "me" | "to", ..] | ["how", "about", ..] => true,
        [helper, person, ..] if LAMP_ASKS_WITH.contains(&helper) && PEOPLE.contains(&person) => {
            true
        }
        // "is" and "are" only right after it ("What time is it"), not "How
        // nice it is to see you".
        [wh, ref after @ ..] if LAMP_ASKS_WH.contains(&wh) => {
            after.iter().enumerate().any(|(at, word)| {
                LAMP_ASKS_WITH.contains(word) && (at < 2 || !matches!(*word, "is" | "are"))
            })
        }
        _ => false,
    };
    let lower = last.to_lowercase();
    asks_with || lower.contains("let me know") || lower.contains("or would")
}

/// The worker's argument for the switch in the environment.
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading [`FLAG`] off a worker's flags.
pub fn take(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == FLAG => (true, rest),
        _ => (false, flags),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The night's scripted lines by kind (the first builder on #705,
    /// 13:0x), scene 3's lines as Gemini may write them, without their
    /// punctuation (Fourth, 13:01), and the reviewer's asks on #1499 that
    /// have no cue the first rule knew: a remark is held for what it is.
    #[test]
    fn only_the_remarks_to_the_room_are_remarks() {
        for remark_said in [
            "I should bring an umbrella then.",
            "i should bring an umbrella then",
            "Okay, I will book the car for eight.",
            "Yes, and we should leave a bit early.",
            "yes and we should leave a bit early",
            "I'll pick up the keys on the way.",
        ] {
            assert!(remark(remark_said), "{remark_said}");
        }
        for asked in [
            "And what about tomorrow?",
            "and what about tomorrow",
            "Remember that my favourite colour is green.",
            "write me a poem about the moon",
            "No thanks, not now.",
            "Thanks, that's perfect.",
            "Thank you.",
            "Could you make it a bit brighter",
            "Please turn the light off",
            "Okay, now tell me a joke",
            "Is it going to rain",
            // The reviewer's probe on #1499.
            "What's the weather tomorrow",
            "whats the time",
            "How's the weather",
            "Where's the nearest pharmacy",
            "And tomorrow",
            "Tomorrow too",
            "Anything else",
            "Brighter",
            "A bit brighter",
            "Warmer.",
            "Louder",
            "Lights off",
            "Pause",
            "Next song",
            "Raise the volume",
            "Wake me at seven",
            "Take a picture",
            "Try again",
            "Another one",
            "Timer for ten minutes",
            "Let's hear some jazz",
            "Don't forget my meeting",
            "I need more light",
            "It's too dark in here",
            "I'd like a poem about the sea",
            "I want some music",
            "I wonder if it will rain tomorrow",
            // The reviewer's second probe on #1499: other forms and cousins
            // of the want words, and lines that could not take something in.
            "I was wondering if it will rain tomorrow",
            "I wanted to know the weather tomorrow",
            "She needs a timer for ten minutes",
            "I'm hoping for a poem about the sea",
            "I'd love to hear a joke",
            "I would love some music",
            "I'd prefer something warmer",
            "I'd rather have some jazz",
            "I could use more light",
            "I didn't catch that",
            "I can't hear it",
            "I don't understand that",
            "I can't read in this light",
            // The reviewer's leftovers on #1499 (15:07).
            "I wanna hear a joke",
            "I missed what she said",
            "I'm not sure I follow",
        ] {
            assert!(!remark(asked), "{asked}");
        }
    }

    /// A bare answer is never a remark, whatever the lamp asked: "Yes go
    /// ahead", as Gemini often writes it, with no comma (the reviewer on
    /// #1499).
    #[test]
    fn a_bare_answer_is_never_a_remark() {
        for said in [
            "No, not now.",
            "Not now",
            "Yes",
            "Sure",
            "Okay",
            "Yeah do it",
            "Yes go ahead",
            "Nope.",
        ] {
            assert!(!remark(said), "{said}");
        }
    }

    /// Another language has none of the cues, so it is never a remark here:
    /// Vietnamese for its marks, and any other script.
    #[test]
    fn a_line_in_another_language_is_never_a_remark() {
        for said in [
            "Trời sắp mưa rồi, mình nên đi sớm.",
            "Mai mình đi sớm nhé",
            "明天会下雨",
            "हम कल जाएंगे",
        ] {
            assert!(!english_letters(said), "{said}");
            assert!(!remark(said), "{said}");
        }
        assert!(!remark("..."), "no letters at all");
        assert!(!remark(""), "nothing heard");
    }

    /// The lamp's question known by the end of its line, with or without
    /// its `?`, and in the shapes the reviewer on #1499 found missed.
    #[test]
    fn the_lamps_question_is_known_by_how_it_ends() {
        for asked in [
            "I can ask Claude Code to write a poem about the rain. Shall I start it?",
            "I can ask Claude Code to write a poem about the rain. Shall I start it",
            "Shall I start it.",
            "Do you want another one",
            "Here it is. Would you like a second verse?  ",
            "Shall I start it? It takes about a minute.",
            "Would you like a second verse? Just say the word.",
            "Let me know if you want me to start it.",
            "Want me to start it",
            // The lamp's own questions without their `?` (the reviewer's
            // second probe on #1499).
            "Which one would you like",
            "What would you like to hear",
            "When should I remind you",
            "What time should I set it for",
            "How about a song",
            "Is there anything else",
            "Tea, or would you like coffee",
            "What kind of music do you like",
        ] {
            assert!(lamp_asked(asked), "{asked}");
        }
        for told in [
            "There you go, turned it down for you.",
            "It will be sunny tomorrow",
            "Will do.",
            "What a lovely poem.",
            "How nice.",
            "Should be sunny tomorrow.",
            "How nice it is to see you.",
            // Known misses (Thirteen on #1533): "is" or "are" said third or
            // later as the only helping verb, the price of keeping "How nice
            // it is to see you" out. They matter only without the `?`.
            "Which of these is better",
            "What kind of tea is it",
            "Where in the house is it",
            "",
        ] {
            assert!(!lamp_asked(told), "{told}");
        }
    }

    #[test]
    fn the_switch_is_on_only_and_its_flag_is_taken_once() {
        assert_eq!(arguments(Some("on")), [FLAG]);
        for not_on in [None, Some("yes"), Some("ON"), Some("1")] {
            assert!(arguments(not_on).is_empty());
        }
        let flags: Vec<String> = [FLAG, "--next"].map(String::from).into();
        assert_eq!(take(&flags), (true, &flags[1..]));
        assert_eq!(take(&flags[1..]), (false, &flags[1..]));
    }
}
