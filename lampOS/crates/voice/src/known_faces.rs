//! Faces the lamp was asked to remember (`docs/eyes.md`, "Who is there").
//!
//! The eyes keep the prints of the people who asked, and nobody else's. The
//! voice gives the model two functions that ask the eyes to remember or
//! forget the face in front of the lamp, and reads which of those people is
//! at the lamp from the eyes' small file, so that a hello and what a person
//! says can carry their name.
use lamp_gemini::{Tool, ToolCall};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

/// The eyes' request socket, from the voice service's environment.
pub const ENVIRONMENT: &str = "LAMP_EYES_WHO_SOCKET";
/// The provider worker's flag for it.
pub const FLAG: &str = "--who-socket";
pub const REMEMBER: &str = "remember_my_face";
pub const FORGET: &str = "forget_my_face";
/// Where the coordinator reads whether a name is checked before it is
/// kept (#829's names, the first builder at 13:24 on 16 October). `checked`
/// only.
pub const NAMES_ENVIRONMENT: &str = "LAMP_NAMES";
/// The provider worker's flag for it, right after the eyes' socket.
pub const NAMES_FLAG: &str = "--names-checked";
/// The exchanges before the turn in hand that a name may have been said
/// in, to be kept once the lamp has said it back and the person agreed
/// ([`name_heard`]).
pub const SAID_EXCHANGES: usize = 2;

/// What a name is kept from (`LAMP_NAMES=checked`): the person's words of
/// the turn in hand, and of the exchanges before it, oldest first, each
/// with what the lamp said back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Said {
    pub this: String,
    pub before: Vec<(String, String)>,
}

impl Said {
    /// Only the turn in hand.
    pub fn now(this: &str) -> Self {
        Self {
            this: this.to_owned(),
            before: Vec::new(),
        }
    }
}

/// What agrees to what the lamp said back, a word or words in a row:
/// "Yes.", "That's right.", "Of course.", "Go ahead.", "Uh-huh.", and the
/// Vietnamese "Vâng.", "Dạ.", "Đúng rồi." (the reviewer on #1057).
const AGREE: &[&[&str]] = &[
    &["yes"],
    &["yeah"],
    &["yep"],
    &["yup"],
    &["correct"],
    &["right"],
    &["exactly"],
    &["sure"],
    &["ok"],
    &["okay"],
    &["absolutely"],
    &["definitely"],
    &["certainly"],
    &["of", "course"],
    &["please", "do"],
    &["go", "ahead"],
    &["uh", "huh"],
    &["mm", "hmm"],
    &["that's", "it"],
    &["vâng"],
    &["dạ"],
    &["ừ"],
    &["có"],
    &["được"],
    &["đúng", "rồi"],
];
/// Words that take a turn's agreement back, anywhere in it: "No, it's…",
/// "Yeah, no, it's Kim.", "Of course not.", "Dạ không." (Ten and the
/// reviewer on #1208: a correction mostly comes right after the yes word).
const DISAGREE: &[&str] = &[
    "no", "nope", "not", "wrong", "don't", "dont", "isn't", "isnt", "nah", "không", "chưa", "sai",
];
/// Yeses that hold a word that would take one back: "Sure, no problem!",
/// "Yes, I don't mind.", "Sure, why not?". Its words take nothing back.
const AGREE_WITH_A_NO: &[&[&str]] = &[
    &["no", "problem"],
    &["no", "worries"],
    &["why", "not"],
    &["don't", "mind"],
    &["dont", "mind"],
    &["do", "not", "mind"],
];

fn lower_words(text: &str) -> Vec<String> {
    text.replace('\u{2019}', "'")
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(|word| word.trim_matches('\''))
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Whether the turn agrees to what the lamp said back: something that
/// agrees ([`AGREE`]), and nothing anywhere in it that takes it back
/// ([`DISAGREE`]) but in a yes that holds one ([`AGREE_WITH_A_NO`]).
/// Refusing a yes costs a turn; keeping a wrong name does not fail safe.
fn agrees(said: &str) -> bool {
    let words = lower_words(said);
    let phrase_at = |at: usize, phrase: &[&str]| {
        phrase
            .iter()
            .enumerate()
            .all(|(next, word)| words.get(at + next).is_some_and(|said| said == word))
    };
    let mut pleasantry = vec![false; words.len()];
    for at in 0..words.len() {
        if let Some(phrase) = AGREE_WITH_A_NO.iter().find(|phrase| phrase_at(at, phrase)) {
            pleasantry[at..at + phrase.len()].fill(true);
        }
    }
    let agreeing = (0..words.len())
        .any(|at| pleasantry[at] || AGREE.iter().any(|phrase| phrase_at(at, phrase)));
    agreeing
        && !words
            .iter()
            .zip(&pleasantry)
            .any(|(word, kept)| !kept && DISAGREE.contains(&word.as_str()))
}

/// How many letters the person said a name has: "three letters", "it's 4
/// letters". The last count said, if any.
fn letters_counted(said: &str) -> Option<usize> {
    const NUMBERS: &[&str] = &[
        "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
        "twelve",
    ];
    let words = lower_words(said);
    words
        .windows(2)
        .rev()
        .filter(|pair| pair[1] == "letters" || pair[1] == "letter")
        .find_map(|pair| {
            NUMBERS
                .iter()
                .position(|number| *number == pair[0])
                .map(|at| at + 1)
                .or_else(|| pair[0].parse().ok())
        })
}

/// Whether `name` may be kept from what the person said
/// (`LAMP_NAMES=checked`): said or spelled in the turn in hand
/// ([`said_by_them`]); or said in an exchange before it, said back by the
/// lamp in its last answer, and agreed to in the turn in hand ("Shall I
/// call you Jim?" "Yes."), not a yes to something asked since. Never one
/// whose letters they counted otherwise, in either turn ("three letters",
/// for seven taken).
///
/// Why, from lamp-4ace on 16 October at 16:46 (the first builder on
/// #1057): a name was kept after "Sure." and a lone digit, from the turn
/// before them; and seven letters were put together from four spelled
/// letters, a first name said twice between them, and "three letters".
pub fn name_heard(name: &str, said: &Said) -> bool {
    let letters = name.chars().filter(|c| c.is_alphabetic()).count();
    let counted_otherwise =
        |turn: &str| letters_counted(turn).is_some_and(|counted| counted != letters);
    if counted_otherwise(&said.this) {
        return false;
    }
    if said_by_them(name, &said.this) {
        return true;
    }
    agreed_to(name, said)
        && said
            .before
            .iter()
            .any(|(person, _)| said_by_them(name, person) && !counted_otherwise(person))
}

/// Words that may stand beside a yes to the lamp's read-back without
/// changing it: "Yes, please.", "Sure, that's fine.", "Yeah, call me
/// Kesh.", "Vâng ạ.".
const BESIDE_A_YES: &[&str] = &[
    "please", "call", "me", "that's", "that", "is", "it", "it's", "fine", "good", "great",
    "perfect", "i", "i'd", "my", "name", "thanks", "thank", "you", "lamp", "do", "go", "for",
    "like", "would", "be", "ạ", "gọi", "tôi", "là", "tên",
];

/// Whether `turn` is a plain yes to the lamp's saying `name` back: an
/// agreeing turn ([`agrees`]) holding nothing but yeses, the name itself and
/// words that stand beside a yes ([`BESIDE_A_YES`]). Anything else, a
/// correction with no "no" in it among them ("Yes, but it's Kess.", "Sure,
/// why not Kess?", "Yes, K, E, S, S.") or "Có lẽ." ("maybe"), is asked
/// about again (the reviewer on #1208): that costs a turn, and a misheard
/// name put in place costs the right one. So is a yes said as a question:
/// "Yes?", "Right?", and "Dạ?", which in Vietnamese is "sorry, what?".
fn plain_yes(name: &str, turn: &str) -> bool {
    if turn.trim_end().ends_with('?') {
        return false;
    }
    let words = lower_words(turn);
    let name = folded_words(name);
    let phrase_at = |at: usize, phrase: &[&str]| {
        phrase
            .iter()
            .enumerate()
            .all(|(next, word)| words.get(at + next).is_some_and(|said| said == word))
    };
    let mut covered = vec![false; words.len()];
    for at in 0..words.len() {
        for phrase in AGREE.iter().chain(AGREE_WITH_A_NO) {
            if phrase_at(at, phrase) {
                covered[at..at + phrase.len()].fill(true);
            }
        }
    }
    agrees(turn)
        && words.iter().zip(&covered).all(|(word, covered)| {
            *covered
                || BESIDE_A_YES.contains(&word.as_str())
                || name.contains(&folded_words(word).concat())
        })
}

/// Whether the lamp said `name` back in the exchange just before the turn
/// in hand, and the person said a plain yes to it now ([`plain_yes`]):
/// what `replace` needs, and a name kept from an earlier exchange.
fn agreed_to(name: &str, said: &Said) -> bool {
    plain_yes(name, &said.this)
        && said
            .before
            .last()
            .is_some_and(|(_, lamp)| said_by_them(name, lamp))
}

/// The provider worker's argument for the names switch.
pub fn names_arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("checked") {
        vec![NAMES_FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Whether `name` is in what the person said (`said`): each of its words
/// among theirs, or its letters spelled one by one ("J-I-M", "J I M"). A
/// name the model made up from words heard in another language is not
/// (lamp-4ace, 16 October at 12:11: "No, my name is …" was heard as
/// Spanish words and kept as a name nobody said).
///
/// A word with an apostrophe is kept whole, so that "It's cold" spells no
/// "S", and a spelled run must be the name's word itself, not hold it ("I'm
/// a teacher" spells no "Ma"; the reviewer on #1057). Accents count for
/// nothing ("Jose" is "José", and "J O S E" spells it), written as one
/// letter or as a letter and its mark ("Đức" either way), and "double N" is
/// two of them ("A, double N" spells "Ann"), but "double U" is W.
pub fn said_by_them(name: &str, said: &str) -> bool {
    let (name, theirs) = (folded_words(name), folded_words(said));
    let mut spelled: Vec<String> = Vec::new();
    let mut run = String::new();
    let mut doubled = false;
    for word in &theirs {
        if word == "double" {
            doubled = true;
            continue;
        }
        if doubled && word == "u" {
            run.push('w');
        } else if word.chars().count() == 1 {
            run.push_str(word);
            if doubled {
                run.push_str(word);
            }
        } else if !run.is_empty() {
            spelled.push(std::mem::take(&mut run));
        }
        doubled = false;
    }
    if !run.is_empty() {
        spelled.push(run);
    }
    // One letter said again and again ("It's Q. Q. Q.") is that letter,
    // not a name spelled.
    spelled.retain(|letters| {
        let mut chars = letters.chars();
        let first = chars.next();
        letters.chars().count() < 2 || chars.any(|c| Some(c) != first)
    });
    !name.is_empty()
        && name
            .iter()
            .all(|word| theirs.contains(word) || spelled.iter().any(|letters| letters == word))
}

/// The words of `text` as a name is matched: lowercase, without accents,
/// written as one letter or as a letter and its mark, a word with an
/// apostrophe kept whole and ’ read as '.
fn folded_words(text: &str) -> Vec<String> {
    text.replace('\u{2019}', "'")
        .chars()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .collect::<String>()
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(|word| word.trim_matches('\''))
        .filter(|word| !word.is_empty())
        .map(|word| word.to_lowercase().chars().map(plain).collect())
        .collect()
}

/// Lower-case letters with their accents, by the letter without: the Latin
/// ones and Vietnamese's.
const ACCENTED: &[(&str, char)] = &[
    ("àáâãäåāăąảạằắẳẵặầấẩẫậ", 'a'),
    ("çćč", 'c'),
    ("ďđ", 'd'),
    ("èéêëēėęěẻẽẹềếểễệ", 'e'),
    ("ìíîïīįỉĩị", 'i'),
    ("ľłĺ", 'l'),
    ("ñńň", 'n'),
    ("òóôõöøōőơỏọồốổỗộờớởỡợ", 'o'),
    ("ŕř", 'r'),
    ("śšşș", 's'),
    ("ťţț", 't'),
    ("ùúûüūůűưủũụừứửữự", 'u'),
    ("ýÿỳỷỹỵ", 'y'),
    ("źżž", 'z'),
];

/// `c` without its accent, when it has one ([`ACCENTED`]).
fn plain(c: char) -> char {
    ACCENTED
        .iter()
        .find(|(letters, _)| letters.contains(c))
        .map_or(c, |(_, plain)| *plain)
}

/// Capitalised words the service writes that are no one's name: "I'm OK.",
/// "I'm American.", "Call me Monday." (the reviewer on #1057).
const NOT_A_NAME: &[&str] = &[
    "i",
    "ok",
    "okay",
    "sorry",
    "fine",
    "good",
    "great",
    "here",
    "back",
    "done",
    "ready",
    "home",
    "lamp",
    "lamb",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    // Not April, June or August: those are names too.
    "january",
    "february",
    "march",
    "july",
    "september",
    "october",
    "november",
    "december",
    "tomorrow",
    "today",
    "tonight",
    "american",
    "english",
    "british",
    "canadian",
    "australian",
    "vietnamese",
    "chinese",
    "japanese",
    "korean",
    "thai",
    "indian",
    "french",
    "german",
    "spanish",
    "italian",
    "mexican",
    "russian",
    "dutch",
    "swedish",
    "filipino",
    "indonesian",
    "brazilian",
    "portuguese",
];

/// Words that, just before "my name", make it a question about the name
/// and not the name given ("Do you know what my name is?"). Only these:
/// "Remember, my name is Jim." and "You know my name is Jim." give it (the
/// reviewer on #1057).
const ASKING: &[&str] = &["what", "what's", "whats"];

/// Question words that open a request and not a question about the name:
/// "Can you call me Jim?", "Could you remember that my name is Jim?".
const ASKING_FOR: &[&str] = &["can", "could", "will", "would"];

/// Words a question opens with: a sentence that opens with one and ends
/// with "?" asks, and gives no name.
const QUESTION: &[&str] = &[
    "do", "does", "did", "can", "could", "will", "would", "is", "are", "what", "who", "how", "why",
    "when", "where",
];

/// Whether what the person said gives their name: "my name is Jim", "my
/// name's Jim", "call me Jim", "I am called Jim", "I'm Jim" or "I am Jim",
/// with the name as the service writes one, capitalised and not a word that
/// is no one's name. Not after "what", and not in a question (the reviewer
/// on #1057: "Do you know what my name is, Lamp?"), except one asking the
/// lamp to call them so or to remember it ("Could you call me Jim?", "Do you
/// remember my name is Jim?").
pub fn gives_a_name(said: &str) -> bool {
    let said = said.replace('\u{2019}', "'");
    let words: Vec<String> = said
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    // Their own name, not someone else's: "his name", "your name" are not
    // (Ten on #1224).
    let speaks_of_my_name = words
        .windows(2)
        .any(|pair| pair[0] == "my" && (pair[1] == "name" || pair[1] == "name's"));
    said.split_inclusive(['.', '!', '?'])
        .any(|sentence| gives_a_name_in(sentence, speaks_of_my_name))
}

/// One sentence of [`gives_a_name`]; `speaks_of_my_name`: the request
/// speaks of their own name somewhere, so that "It's Jim." gives one ("Will
/// you remember my name? It's Jim.", the reviewer on #1057), and "What's
/// his name? It's Bob." none.
fn gives_a_name_in(sentence: &str, speaks_of_my_name: bool) -> bool {
    let words: Vec<&str> = sentence
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .collect();
    let lower: Vec<String> = words.iter().map(|word| word.to_lowercase()).collect();
    if sentence.trim_end().ends_with('?')
        && lower.first().is_some_and(|first| {
            QUESTION.contains(&first.as_str()) && !ASKING_FOR.contains(&first.as_str())
        })
        && !lower.iter().any(|word| word == "remember")
    {
        return false;
    }
    let at = |i: usize| lower.get(i).map(String::as_str);
    let named = |i: usize| {
        words.get(i).is_some_and(|word| {
            word.chars().next().is_some_and(char::is_uppercase)
                && !NOT_A_NAME.contains(&word.to_lowercase().as_str())
        })
    };
    let asked = |i: usize| {
        i.checked_sub(1)
            .and_then(at)
            .is_some_and(|before| ASKING.contains(&before))
    };
    // The name first: "Jim's my name.", "Jim is my name.", never a
    // question word or a pointing one ("What's my name?", "That is my
    // name.").
    let name_first = |i: usize| {
        let word = lower[i].trim_end_matches("'s");
        let pointing = ["that", "this", "it", "here", "there", "which"].contains(&word);
        // Someone else's words: "He said Jim is my name." (the reviewer on
        // #1224).
        let reported = i
            .checked_sub(1)
            .and_then(at)
            .is_some_and(|before| ["said", "says", "told", "tells", "tell"].contains(&before));
        !pointing
            && !reported
            && !QUESTION.contains(&word)
            && !NOT_A_NAME.contains(&word)
            && match (at(i + 1), at(i + 2), at(i + 3)) {
                (Some("my"), Some("name"), _) => lower[i].ends_with("'s") && named(i),
                (Some("is"), Some("my"), Some("name")) => named(i),
                _ => false,
            }
    };
    (0..lower.len()).any(|i| match (at(i), at(i + 1), at(i + 2)) {
        (Some("my"), Some("name"), Some("is")) => !asked(i) && named(i + 3),
        (Some("my"), Some("name's"), _) => !asked(i) && named(i + 2),
        (Some("call"), Some("me"), _) => named(i + 2),
        (Some("i"), Some("am"), Some("called")) => named(i + 3),
        (Some("i'm"), _, _) => named(i + 1),
        (Some("i"), Some("am"), _) => named(i + 2),
        // An introduction of themselves: "This is Jim speaking.", "Hi Lamp,
        // this is Jim here.", or in a request that says "my name". A bare
        // "Lamp, this is Jim." at a desk mostly introduces a friend, and the
        // eyes would file the friend's name on the one face they see (the
        // reviewer on #1224).
        (Some("this"), Some("is"), _) => {
            named(i + 2) && (matches!(at(i + 3), Some("speaking" | "here")) || speaks_of_my_name)
        }
        (Some("it's"), _, _) => speaks_of_my_name && named(i + 1),
        (Some("it"), Some("is"), _) => speaks_of_my_name && named(i + 2),
        _ => name_first(i),
    })
}

/// What the model is told beside a request in which the person gave their
/// name, with names checked: a name told is a name kept (the first builder,
/// #705, 13:43: on 15 October the lamp only said it would remember).
/// A condition, so that a match that was no name ("I'm Irish.") costs
/// nothing (the reviewer on #1057).
pub const NAME_TOLD_NOTE: &str = "If they have just told you their own name, call remember_my_face with it now, as they said or spelled it, or if you are not sure of it, say it back and ask them to spell it. Never only say that you will remember it. If what they said was not their name, do nothing about it.";

/// What the model is told when the name is not in what they said.
const NOT_HEARD: &str = "You did not hear that name in what they said. Say the name back to them and ask them to spell it, then call this again with the name as they said or spelled it.";
/// What the model is told when it would replace a name before the person
/// agreed to the new one.
const NOT_AGREED: &str = "They have not said yes to that name yet. Say the new name back and ask whether they want to be known by it instead. Only if they say yes, call this again with the new name and replace true.";
/// Remembering looks for up to four seconds.
pub const DEADLINE: Duration = Duration::from_secs(6);
/// The known file counts while it is this fresh: the eyes renew it every
/// two seconds while the person is there.
const FRESH: Duration = Duration::from_secs(3);
const MOST_NAME_BYTES: usize = 120;

/// The two functions the model is told of when the eyes remember faces.
/// With names checked, remembering takes `replace`, for a face known by
/// another name.
pub fn tools() -> Vec<Tool> {
    tools_checked(false)
}

/// The same, with names checked or not.
pub fn tools_checked(checked: bool) -> Vec<Tool> {
    vec![
        Tool {
            name: REMEMBER.into(),
            description: if checked {
                "Call this right away when someone tells you their name, or asks you to remember their face \
                 or who they are. It looks at them for a few seconds, so they should face you. Then say in a \
                 few words that you will know them now. Never only say that you will remember a name: this is \
                 how you do. If it says it could not see them, tell them why in a few words. Never call it with \
                 a name given for somebody else."
            } else {
                "Call this right away when someone asks you to remember their face or who they are, \
                 and has told you their name. It looks at them for a few seconds, so they should face you. \
                 Then say in a few words that you will know them now. If it says it could not see them, \
                 tell them why in a few words. Never call it for someone who did not ask, or with a name \
                 given for somebody else."
            }
            .into(),
            parameters: Some(if checked {
                json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "Their name, as they said or spelled it."},
                        "replace": {"type": "boolean", "description": "Only when you were told this face is known by another name, and they said yes to being known by this one instead."}
                    },
                    "required": ["name"]
                })
            } else {
                json!({
                    "type": "object",
                    "properties": {"name": {"type": "string", "description": "Their name, as they said it."}},
                    "required": ["name"]
                })
            }),
            silent: false,
        },
        Tool {
            name: FORGET.into(),
            description: "Call this when someone asks you to forget their face or who they are, with the name \
                          you know them by. Then tell them briefly that you have forgotten it."
                .into(),
            parameters: Some(json!({
                "type": "object",
                "properties": {"name": {"type": "string", "description": "The name you know them by."}},
                "required": ["name"]
            })),
            silent: false,
        },
    ]
}

/// Whether the voice routes this function to the eyes.
pub fn is_tool(name: &str) -> bool {
    matches!(name, REMEMBER | FORGET)
}

/// Ask the eyes to remember or forget, and say what to tell the model; the
/// second value says whether the eyes answered. Blocking: for a thread of
/// its own. With names `checked`, a name the person did not say is not
/// kept, and a face known by another name is asked about first.
pub fn run(socket: &Path, call: &ToolCall, checked: bool) -> (Value, bool) {
    let said = if checked {
        crate::thread::THREAD
            .lock()
            .map(|thread| thread.names_window(SAID_EXCHANGES))
            .unwrap_or_default()
    } else {
        Said::default()
    };
    run_with(socket, call, checked, &said)
}

fn run_with(socket: &Path, call: &ToolCall, checked: bool, said: &Said) -> (Value, bool) {
    let Some(name) = call
        .args
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
    else {
        return (
            json!({"error": "There was no name in that for me to keep."}),
            true,
        );
    };
    if checked && call.name == REMEMBER && !name_heard(name, said) {
        return (json!({"error": NOT_HEARD}), true);
    }
    let replace = call.args.get("replace").and_then(Value::as_bool) == Some(true);
    // A name replaces another only once the lamp said it back and they
    // said yes (the first builder on #1057: a misheard name was kept while
    // the person was still correcting it).
    if checked && call.name == REMEMBER && replace && !agreed_to(name, said) {
        return (json!({"error": NOT_AGREED}), true);
    }
    let line = match call.name.as_str() {
        REMEMBER if checked => json!({"remember": name, "one_face": true, "replace": replace}),
        REMEMBER => json!({"remember": name}),
        FORGET => json!({"forget": name}),
        _ => return (crate::tools::unavailable(), false),
    };
    match crate::tools::exchange(socket, &line, DEADLINE) {
        Some(answer) => (told(&answer), true),
        None => (
            json!({"error": "My eye is not answering right now, so I could not do that."}),
            false,
        ),
    }
}

/// A face forgotten while old face records the eyes couldn't read are kept
/// apart (#1586): what the person is told, plainly.
pub const FORGOTTEN_SOME_KEPT_APART: &str = "Forgotten. Tell them so, and that a few old face records you could not read are kept apart: if those ever come back, their face is forgotten in them too, and a factory reset clears them.";
/// No face of that name where the eyes could look, while old face records
/// they couldn't read are kept apart (#1586).
pub const NOT_KNOWN_SOME_KEPT_APART: &str = "You did not know a face by that name. A few old face records you could not read are kept apart: if one is theirs, it is forgotten if they ever come back, and a factory reset clears them.";
/// What the person who asked to be remembered is told when the lamp also
/// counts how often they come by (#1670): one short line, in the lamp's
/// own words.
pub const VISITS_LINE: &str = "Also say, in one short sentence, that you'll notice how often they come by, so you can greet them more warmly.";

/// What the eyes said, in words the model can say.
fn told(answer: &Value) -> Value {
    if let Some(name) = answer.get("remembered").and_then(Value::as_str) {
        // With visits counted (`lamp-eyes --visits on`), the person is told so
        // in one line, as they are remembered (the first builder on #1670).
        if answer.get("visits").and_then(Value::as_bool) == Some(true) {
            return json!({
                "ok": true,
                "remembered": name,
                "also_say": VISITS_LINE,
            });
        }
        return json!({"ok": true, "remembered": name});
    }
    // Set-aside files the eyes couldn't search: the forget waits for them,
    // and the person is told what was forgotten and what wasn't (#1586),
    // not that the eye failed.
    let unsearched = answer
        .get("unsearched")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    match (answer.get("forgot").and_then(Value::as_bool), unsearched) {
        (Some(true), 0) => return json!({"ok": true}),
        (Some(false), 0) => return json!({"error": "I did not know a face by that name."}),
        (Some(true), _) => return json!({"ok": true, "note": FORGOTTEN_SOME_KEPT_APART}),
        (Some(false), _) => return json!({"error": NOT_KNOWN_SOME_KEPT_APART}),
        (None, _) => {}
    }
    if answer.get("error").and_then(Value::as_str) == Some("known_as")
        && let Some(known) = answer.get("known_as").and_then(Value::as_str)
    {
        return json!({"error": format!(
            "You already know this face as {known}. Say the new name back and ask whether they want to be known by it instead. Only if they say yes, call this again with the new name and replace true."
        )});
    }
    let words = match answer.get("error").and_then(Value::as_str) {
        Some("no_face") => "I could not see a face. They should look at me and ask again.",
        Some("too_far") => "Their face was too far away. They should come closer and ask again.",
        Some("two_faces") => {
            "There was more than one face in front of me. Only they should be in front of me when they ask again."
        }
        Some("eyes_shut") => "My eye is shut just now, so I could not see them.",
        Some("busy") => "I was already remembering someone. They should ask again in a moment.",
        Some("full") => {
            "I already know as many faces as I can keep. Someone has to be forgotten first."
        }
        Some("not_a_name") => "That is not a name I can keep.",
        _ => "My eye could not do that just now.",
    };
    json!({"error": words})
}

/// A fresh source-file observation, with expiry anchored to the source age.
/// This identifies a visible face, not the speaker, and never extends session
/// ownership. Reading belongs off the input/audio path.
#[derive(Clone, Debug)]
pub struct FaceObservation {
    name: String,
    valid_until: Instant,
}

impl FaceObservation {
    pub fn read(path: &Path) -> Option<Self> {
        use std::io::Read;
        // Anchor before file I/O so slow reads cannot renew source freshness.
        let now = Instant::now();
        let wall = SystemTime::now();
        let file = std::fs::File::open(path).ok()?;
        let modified = file.metadata().ok()?.modified().ok()?;
        let mut bytes = Vec::new();
        // The source is a small name file. Refuse oversized/corrupt files.
        file.take(4097).read_to_end(&mut bytes).ok()?;
        if bytes.len() > 4096 {
            return None;
        }
        let text = String::from_utf8(bytes).ok()?;
        let observation = Self::from_source(text.trim(), modified, wall, now)?;
        observation.is_fresh(Instant::now()).then_some(observation)
    }

    fn from_source(
        name: &str,
        modified: SystemTime,
        wall: SystemTime,
        now: Instant,
    ) -> Option<Self> {
        let age = wall.duration_since(modified).ok()?;
        let remaining = FRESH.checked_sub(age)?;
        if remaining.is_zero()
            || name.is_empty()
            || name.len() > MOST_NAME_BYTES
            || name.chars().any(char::is_control)
        {
            return None;
        }
        Some(Self {
            name: name.to_owned(),
            valid_until: now.checked_add(remaining)?,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn valid_until(&self) -> Instant {
        self.valid_until
    }
    pub fn is_fresh(&self, now: Instant) -> bool {
        now < self.valid_until
    }
}

/// Existing face lookup, backed by a bounded observation read. Future or
/// expired source timestamps fail closed rather than appearing freshly seen.
pub fn known_now(path: &Path) -> Option<String> {
    FaceObservation::read(path).map(|observation| observation.name)
}

/// The file the eyes keep the known person's name in while they are at the
/// lamp: `known`, beside their request socket (`lamp-eyes --known-file`).
pub fn known_file_of(socket: &Path) -> PathBuf {
    socket.with_file_name("known")
}

/// [`known_now`] for the eyes the environment names.
pub fn known_from_environment() -> Option<String> {
    let socket = std::env::var_os(ENVIRONMENT)?;
    let socket = Path::new(&socket);
    socket
        .is_absolute()
        .then(|| known_now(&known_file_of(socket)))
        .flatten()
}

/// A known person counts as gone only after this long without a fresh known
/// file. The eyes renew it within 3 s while they see the face, so a person
/// who looks away for a few seconds is still there; a new session for the
/// room is opened only once they have been unseen this long (#764).
pub const GONE_AFTER: Duration = Duration::from_secs(30);

/// A different known person counts only once the eyes have known them,
/// and not the one at the lamp, this long. On lamp-4ace on 16 October from
/// 16:06 the eyes flipped the owner between two names kept for one face,
/// and each flip opened a new session: the lamp was deaf to them (the first
/// builder, #705, 16:09). A person who sits down in someone's place keeps
/// on being known as themselves, so they count after this.
pub const SOMEONE_ELSE_AFTER: Duration = Duration::from_secs(10);

/// Who is at the lamp, as the voice counts it for whose session it is:
/// someone known counts at once when nobody was, or when the one counted
/// was last seen [`GONE_AFTER`] ago or more; a different known person only
/// after [`SOMEONE_ELSE_AFTER`] of being known with the one at the lamp
/// unseen; and nobody only after [`GONE_AFTER`]. It decides only whose
/// session it is: an exchange is the person's the eyes name as it is made
/// (`crate::thread::Thread::seen_now`).
#[derive(Default)]
pub struct Presence {
    who: Option<String>,
    seen_at: Option<Instant>,
    /// Someone else known, and since when, while the one at the lamp has
    /// not been seen.
    other: Option<(String, Instant)>,
}

impl Presence {
    /// What the known file said now (`None`: nobody fresh), and who is at
    /// the lamp after it.
    pub fn seen(&mut self, known: Option<String>, now: Instant) -> Option<&str> {
        // The one counted, unseen this long, is gone, whether or not it
        // was read between: the second route reads only as a turn opens,
        // and never sees the 30 s between turns (the reviewer on #1100).
        let gone = self
            .seen_at
            .is_some_and(|at| now.saturating_duration_since(at) >= GONE_AFTER);
        match known {
            Some(who) if !gone && self.who.as_ref().is_some_and(|at| *at != who) => {
                match &self.other {
                    Some((other, since))
                        if *other == who
                            && now.saturating_duration_since(*since) >= SOMEONE_ELSE_AFTER =>
                    {
                        self.who = Some(who);
                        self.seen_at = Some(now);
                        self.other = None;
                    }
                    Some((other, _)) if *other == who => {}
                    _ => self.other = Some((who, now)),
                }
            }
            Some(who) => {
                self.who = Some(who);
                self.seen_at = Some(now);
                self.other = None;
            }
            None if gone => self.who = None,
            None => {}
        }
        self.who.as_deref()
    }
}

/// What a request is told when someone the lamp knows is at it.
pub fn note(name: &str) -> String {
    format!("(The person at the lamp now is {name}: you know their face.)")
}

/// The hello for someone the lamp knows, in place of `speak_up::NOTE`.
pub fn hello(name: &str) -> String {
    format!(
        "(Nobody has spoken. {name}, whose face you know, has just come and sat down in front of you after being \
         away for a while. Say hello to them by name the way a friend at the next desk would: a few warm words and no \
         question they have to answer.)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observations_expire_from_source_age_without_renewal_on_reread() {
        let monotonic = Instant::now();
        let wall = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let modified = wall - Duration::from_secs(2);
        let first = FaceObservation::from_source("Ann", modified, wall, monotonic).unwrap();
        assert_eq!(first.valid_until(), monotonic + Duration::from_secs(1));
        let later = Duration::from_millis(500);
        let again =
            FaceObservation::from_source("Ann", modified, wall + later, monotonic + later).unwrap();
        assert_eq!(first.valid_until(), again.valid_until());
        assert!(!again.is_fresh(monotonic + Duration::from_secs(1)));
        assert!(
            FaceObservation::from_source("Ann", modified, wall + Duration::from_secs(1), monotonic)
                .is_none()
        );
        assert!(
            FaceObservation::from_source("Ann", wall + Duration::from_nanos(1), wall, monotonic)
                .is_none()
        );
        for invalid in ["", "Ann\nBob"] {
            assert!(FaceObservation::from_source(invalid, wall, wall, monotonic).is_none());
        }
    }

    #[test]
    fn what_the_eyes_say_becomes_words_the_model_can_say() {
        assert_eq!(
            told(&json!({"remembered":"Minh"})),
            json!({"ok":true,"remembered":"Minh"})
        );
        // #1670: with visits counted, the person is told so in one line;
        // a `visits` that isn't true says nothing more.
        assert_eq!(
            told(&json!({"remembered":"Minh","visits":true})),
            json!({"ok":true,"remembered":"Minh","also_say":VISITS_LINE})
        );
        assert_eq!(
            told(&json!({"remembered":"Minh","visits":"yes"})),
            json!({"ok":true,"remembered":"Minh"})
        );
        assert_eq!(told(&json!({"forgot":true})), json!({"ok":true}));
        assert!(told(&json!({"forgot":false}))["error"].is_string());
        // #1586: some set-aside records couldn't be searched. What was
        // forgotten is said, and what is kept apart, never "My eye could not
        // do that".
        assert_eq!(
            told(&json!({"forgot":true,"unsearched":1})),
            json!({"ok":true,"note":FORGOTTEN_SOME_KEPT_APART})
        );
        assert_eq!(
            told(&json!({"forgot":false,"unsearched":2})),
            json!({"error":NOT_KNOWN_SOME_KEPT_APART})
        );
        // A real fault is still the eye's.
        assert_eq!(
            told(&json!({"error":"storage"})),
            json!({"error":"My eye could not do that just now."})
        );
        for code in [
            "no_face",
            "too_far",
            "two_faces",
            "eyes_shut",
            "busy",
            "full",
            "not_a_name",
            "else",
        ] {
            let words = told(&json!({"error": code}));
            assert!(
                words["error"]
                    .as_str()
                    .is_some_and(|words| words.ends_with('.')),
                "{code}"
            );
        }
        assert_ne!(
            told(&json!({"error":"too_far"})),
            told(&json!({"error":"no_face"}))
        );
    }

    #[test]
    fn a_call_without_a_name_is_not_sent_and_a_silent_eye_is_said_so() {
        let call = |name: &str, args: Value| ToolCall {
            id: "1".into(),
            name: name.into(),
            args,
        };
        let nowhere = Path::new("/nonexistent/lamp-eyes-who.sock");
        let (words, answered) = run(nowhere, &call(REMEMBER, json!({"name":"  "})), false);
        assert!(words["error"].is_string() && answered);
        let (words, answered) = run(nowhere, &call(REMEMBER, json!({"name":"A"})), false);
        assert!(words["error"].as_str().unwrap().contains("eye") && !answered);
        assert!(is_tool(REMEMBER) && is_tool(FORGET) && !is_tool("remember"));
    }

    #[test]
    fn a_known_person_looking_away_is_still_there_and_another_counts_once_known_10_s() {
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let mut presence = Presence::default();
        assert_eq!(presence.seen(None, at(0)), None);
        assert_eq!(presence.seen(Some("a".into()), at(1)), Some("a"));
        // Looking away: still there until 30 s unseen.
        assert_eq!(presence.seen(None, at(5)), Some("a"));
        assert_eq!(presence.seen(None, at(30)), Some("a"));
        assert_eq!(presence.seen(Some("a".into()), at(31)), Some("a"));
        assert_eq!(presence.seen(None, at(60)), Some("a"));
        assert_eq!(presence.seen(None, at(61)), None);
        // Someone known counts as soon as the eyes know them when nobody was.
        assert_eq!(presence.seen(Some("a".into()), at(62)), Some("a"));
        // Someone else only once known 10 s with "a" unseen: a face flipping
        // between two names kept for it never counts as someone else.
        for second in [63, 66, 70, 72] {
            assert_eq!(presence.seen(Some("b".into()), at(second)), Some("a"));
        }
        assert_eq!(presence.seen(Some("a".into()), at(73)), Some("a"));
        assert_eq!(presence.seen(Some("b".into()), at(74)), Some("a"));
        assert_eq!(presence.seen(Some("b".into()), at(83)), Some("a"));
        assert_eq!(presence.seen(Some("b".into()), at(84)), Some("b"));
        // And "a" again, as someone else, only after 10 s.
        assert_eq!(presence.seen(Some("a".into()), at(85)), Some("b"));
        assert_eq!(presence.seen(Some("a".into()), at(95)), Some("a"));
        // The reviewer on #1100: read only as turns open, the one counted
        // last seen 30 s ago or more is gone, and someone else counts at
        // once. "b" at 3,600 s, an hour after "a".
        assert_eq!(presence.seen(Some("b".into()), at(95 + 3_600)), Some("b"));
        assert_eq!(presence.seen(Some("a".into()), at(95 + 3_629)), Some("b"));
        assert_eq!(presence.seen(Some("a".into()), at(95 + 3_630)), Some("a"));
    }

    #[test]
    fn the_known_name_counts_only_while_the_eyes_keep_it_fresh() {
        let path = std::env::temp_dir().join(format!("lamp-voice-known-{}", std::process::id()));
        assert_eq!(known_now(&path), None);
        std::fs::write(&path, "Minh\n").unwrap();
        assert_eq!(known_now(&path).as_deref(), Some("Minh"));
        let old = SystemTime::now() - Duration::from_secs(10);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(known_now(&path), None);
        std::fs::write(&path, "Minh\u{7}\n").unwrap();
        assert_eq!(known_now(&path), None);
        std::fs::remove_file(&path).unwrap();
        assert!(hello("Minh").contains("Minh") && note("Minh").contains("Minh"));
        assert_eq!(
            known_file_of(Path::new("/run/lampos-eyes/who.sock")),
            PathBuf::from("/run/lampos-eyes/known")
        );
    }

    #[test]
    fn a_name_given_is_told_from_talk_that_only_mentions_a_name() {
        for given in [
            "Hi Lamp, my name is Jim.",
            "My name's Jim.",
            "You can call me Jim.",
            "I'm Jim.",
            "Hello, I am Jim.",
            "I am called Jim.",
            // Curly apostrophes, as the service writes them too.
            "I\u{2019}m Jim.",
            "My name\u{2019}s Jim.",
            "Hi, my name is Jim, what's yours?",
            // The reviewer on #1057: the usual ways of asking to be
            // remembered give the name.
            "Remember, my name is Jim.",
            "Please remember my name is Jim.",
            "Don't forget, my name is Jim.",
            "You know my name is Jim.",
            "Can you call me Jim?",
            "Could you call me Jim from now on?",
            "Could you remember that my name is Jim?",
            "Do you remember my name is Jim?",
            // The reviewer on #1057 at 03:49: names given with no note.
            "Will you remember my name? It's Jim.",
            "This is Jim speaking.",
            "Hi Lamp, this is Jim here.",
            "Jim's my name.",
            "Jim is my name.",
            "My name? It is Jim.",
        ] {
            assert!(gives_a_name(given), "{given}");
        }
        for not in [
            "I'm tired.",
            "I am going home.",
            "Call me later.",
            "What is my name?",
            "Did Jim call?",
            "",
            // The reviewer on #1057: asking for the name, and capitalised
            // words that are no one's.
            "Do you know what my name is, Lamp?",
            "Tell me what my name is again.",
            "Why do you call me Jim?",
            "Did you call me Jim?",
            "I'm OK.",
            "I'm American.",
            "Call me Monday.",
            // "It's …" gives a name only where a name is spoken of.
            "It's Jim's turn.",
            "It's Monday.",
            "My name? It's a secret.",
            "What's my name? It's Monday.",
            "This is great.",
            "Is my name right?",
            // Ten and the reviewer on #1224: someone else introduced, or
            // named, or quoted; a place; the lamp's own name.
            "Lamp, this is Jim.",
            "This is Jim.",
            "Hey Lamp, this is Jim!",
            "Lamp, this is Jim. He sits next to me.",
            "Who is this? This is Jim.",
            "He said Jim is my name.",
            "He said \"Jim is my name.\"",
            "This is Hanoi.",
            "Lamp's my name.",
            "Lamp, this is Jim, my colleague.",
            "This is Jim from accounts.",
            "What's his name? It's Bob.",
            "Her name? It's Anna.",
            "What's your name? It's Lamp.",
            "That's my name.",
            "Who's my name for?",
        ] {
            assert!(!gives_a_name(not), "{not}");
        }
        assert!(
            tools_checked(true)[0]
                .description
                .contains("tells you their name")
        );
        assert!(
            !tools_checked(false)[0]
                .description
                .contains("tells you their name")
        );
    }

    #[test]
    fn a_name_counts_only_when_the_person_said_it_or_spelled_it() {
        assert!(said_by_them("Jim", "Hi, my name is Jim."));
        assert!(said_by_them("Jim", "It's J-I-M."));
        assert!(said_by_them("Jim", "j i m"));
        assert!(said_by_them("Mary Ann", "I'm Mary Ann"));
        assert!(said_by_them("Jo", "Jo. Just Jo."));
        // Made up from words heard in another language, or only the lamp's.
        assert!(!said_by_them("B", "No, mi nombre es y la"));
        assert!(!said_by_them("Mary Ann", "I'm Mary"));
        assert!(!said_by_them("Jim", "Yes."));
        assert!(!said_by_them("  ", "Jim"));
        // The reviewer on #1057: letters split off a word are not spelled,
        // and a spelled run is the name, not a part of one.
        for (name, said) in [
            ("S", "It's cold."),
            ("M", "I'm tired."),
            ("D", "I'd like tea."),
            ("Ma", "I'm a teacher."),
            ("Im", "J I M"),
        ] {
            assert!(!said_by_them(name, said), "{name}: {said}");
        }
        assert!(said_by_them("Jim", "It\u{2019}s J I M."));
        // The reviewer on #1057: accents count for nothing, and "double N"
        // is two.
        for (name, said) in [
            ("Jose", "My name is José."),
            ("José", "It's J O S E."),
            ("Zoe", "I'm Zoë."),
            ("Ann", "A, double N."),
            ("Phương", "Tôi là Phuong."),
            // The reviewer's small gaps at 03:49: a letter and its mark
            // written apart (NFD), the comma below, and "double U".
            ("Đức", "I'm \u{110}u\u{31b}\u{301}c."),
            ("Duc", "I'm \u{110}u\u{31b}\u{301}c."),
            ("Ștefan", "I'm Stefan."),
            ("Ţara", "It's Țara."),
            ("Wu", "Double U, U."),
            ("Lee", "L, double E."),
        ] {
            assert!(said_by_them(name, said), "{name}: {said}");
        }
        assert!(!said_by_them("Anne", "A, double N."));
        assert!(!said_by_them("Uuu", "Double U, U."));
        assert_eq!(names_arguments(Some("checked")), [NAMES_FLAG]);
        assert!(names_arguments(Some("on")).is_empty());
        assert!(names_arguments(None).is_empty());
        assert_eq!(tools_checked(false), tools());
        assert!(
            tools_checked(true)[0].parameters.as_ref().unwrap()["properties"]["replace"]
                .is_object()
        );
    }

    /// The first builder's three records of 16:46 on lamp-4ace (#1057 at
    /// 02:42), with made-up words in the same shapes.
    /// - D: kept after "Sure." and a lone digit, the turn before those a
    ///   one-letter name corrected with that letter three times. Not in the
    ///   turn in hand, nor said back and agreed to: refused, and the letter
    ///   three times spells no name.
    /// - E: "Hey, I'm Kesh Varo" while still correcting: said, so kept for a
    ///   face with no name; replacing one, only once said back and agreed to
    ///   ([`agreed_to`]), never on "No, Kess."
    /// - F: four spelled letters, a first name said twice between them, and
    ///   "three letters": no seven letters are spelled, and none counted;
    ///   the first name itself, three letters as they said, is theirs.
    #[test]
    fn the_three_names_of_16_46_are_refused_or_read_back() {
        let d = Said {
            this: "7".into(),
            before: vec![
                ("No, it's Q. Q. Q.".into(), "Got it, Q.".into()),
                ("Sure.".into(), "Great.".into()),
            ],
        };
        for name in ["Qqq", "Qix", "Q"] {
            assert!(!name_heard(name, &d), "{name}");
        }
        assert!(!said_by_them("Qqq", "Q. Q. Q."));
        assert!(said_by_them("Q", "Q. Q. Q."));

        let e = Said::now("Hey, I'm Kesh Varo.");
        assert!(name_heard("Kesh", &e));
        let still_correcting = Said {
            this: "No, Kess. K, E, S, S.".into(),
            before: vec![(
                "Hey, I'm Kesh Varo.".into(),
                "I know you as Qix. Shall I call you Kesh instead?".into(),
            )],
        };
        assert!(!agreed_to("Kesh", &still_correcting));
        let agreed = Said {
            this: "Yes.".into(),
            ..still_correcting.clone()
        };
        assert!(agreed_to("Kesh", &agreed));

        let f = Said::now("R, Vok, I, Vok, N, A, three letters.");
        for name in ["Rvokina", "Rinavok", "Vokrina"] {
            assert!(!name_heard(name, &f), "{name}");
        }
        // The letters split by the word spell nothing; the word itself, three
        // letters as they said, is theirs; a name they counted otherwise is
        // refused, said or not.
        assert!(!name_heard("Rina", &f));
        assert!(name_heard("Vok", &f));
        assert!(!name_heard(
            "Vok",
            &Said::now("R, Vok, I, Vok, N, A, four letters.")
        ));
        assert!(name_heard("Ina", &Said::now("I, N, A. Three letters.")));
    }

    /// A name said in an exchange before the turn in hand is kept once the
    /// lamp said it back and they agreed; not on a yes to something else,
    /// nor without the lamp saying it.
    #[test]
    fn a_name_said_before_is_kept_once_said_back_and_agreed_to() {
        let said = |this: &str, lamp: &str| Said {
            this: this.into(),
            before: vec![("My name is Jim.".into(), lamp.into())],
        };
        assert!(name_heard(
            "Jim",
            &said("Yes.", "Shall I remember you, Jim?")
        ));
        assert!(name_heard("Jim", &said("Yeah, sure.", "Jim? J, I, M?")));
        assert!(!name_heard(
            "Jim",
            &said("Yes.", "Shall I remember your face?")
        ));
        assert!(!name_heard(
            "Jim",
            &said("No, not yet.", "Shall I remember you, Jim?")
        ));
        assert!(!name_heard("Jim", &said("7", "Shall I remember you, Jim?")));
        assert!(!name_heard("Jim", &Said::now("Yes.")));
        // A yes to something asked since is not one to the name (the
        // reviewer on #1057).
        let later = Said {
            this: "Yes.".into(),
            before: vec![
                (
                    "My name is Jim.".into(),
                    "Shall I remember you, Jim?".into(),
                ),
                (
                    "What time is it?".into(),
                    "It's three. Anything else?".into(),
                ),
            ],
        };
        assert!(!name_heard("Jim", &later));
    }

    /// The reviewer on #1057: ordinary yeses agree. A word that takes a yes
    /// back takes it back anywhere in the turn, but in a yes that holds one
    /// ("no problem"): Ten and the reviewer on #1208.
    #[test]
    fn a_yes_is_any_ordinary_one_and_any_no_takes_it_back() {
        for yes in [
            "Yes.",
            "Sure, no problem!",
            "Yes, no problem.",
            "Sure, why not?",
            "Yes, I don't mind.",
            "Of course.",
            "Absolutely.",
            "Please do.",
            "Go ahead.",
            "Uh-huh.",
            "That's it.",
            "That's right.",
            "Vâng.",
            "Dạ.",
            "Đúng rồi.",
            "Có.",
            "Được.",
            "No problem.",
        ] {
            assert!(agrees(yes), "{yes}");
        }
        for not in [
            "No, not yet.",
            "No, Kess. K, E, S, S.",
            "Not quite. Yes, K, E, S, S.",
            "Nope.",
            "That's not right.",
            "7",
            "Kess.",
            // Ten and the reviewer on #1208: a yes word, then a no.
            "Yeah, no, it's Kim.",
            "Yeah no.",
            "Yes, no wait, it's Kim.",
            "Right, not Jim, Kim.",
            "Of course not.",
            "Absolutely not.",
            "Sure, no.",
            "Okay, no, it's Kess.",
            "Yes and no.",
            "Dạ không.",
            "Dạ, không phải ạ.",
            "Vâng, không phải.",
            "Ừ, sai rồi.",
        ] {
            assert!(!agrees(not), "{not}");
        }
        // So "Yeah, no, Kim." keeps no misheard "Jim", nor puts it in place.
        let corrected = Said {
            this: "Yeah, no, Kim.".into(),
            before: vec![("My name is Jim.".into(), "Shall I call you Jim?".into())],
        };
        assert!(!name_heard("Jim", &corrected));
        assert!(!agreed_to("Jim", &corrected));
    }

    /// The reviewer on #1208 at 04:40: a yes to the lamp's read-back holds
    /// nothing but yeses, the name and words that stand beside a yes. A
    /// correction with no "no" in it is asked about again.
    #[test]
    fn a_yes_to_the_read_back_holds_nothing_but_the_yes_and_the_name() {
        let to = |this: &str| Said {
            this: this.into(),
            before: vec![(
                "Hey, I'm Kesh.".into(),
                "I know you as A. Shall I call you Kesh instead?".into(),
            )],
        };
        for yes in [
            "Yes.",
            "Yes, please.",
            "Sure, that's fine.",
            "Yeah, call me Kesh.",
            "Yes, my name is Kesh.",
            "Dạ.",
            "Vâng ạ.",
            "Yes, I don't mind.",
            "Of course.",
            "Sure, no problem!",
            "Sure, why not.",
            // The reviewer's 05:12 on #1208: ordinary yeses asked again.
            "Yes, I do.",
            "Yes, do it.",
            "Sure, go for it.",
            "Yes, I'd like that.",
            "Yes, that would be great.",
            "Ừ, gọi tôi là Kesh.",
            "Đúng rồi, tên tôi là Kesh.",
        ] {
            assert!(agreed_to("Kesh", &to(yes)), "{yes}");
        }
        for not in [
            "Yes, but it's Kess.",
            "Okay, actually it's Kess.",
            "Sure, it's Kess though.",
            "No worries, it's actually Kess.",
            "No problem, but my name is Kess.",
            "Sure, why not Kess?",
            "Dạ, Kess ạ.",
            "Có lẽ.",
            "Yes, K, E, S, S.",
            // A yes said as a question (the reviewer's 05:12 on #1208).
            "Yes?",
            "Okay?",
            "Right?",
            "Dạ?",
            "Sure, why not?",
        ] {
            assert!(!agreed_to("Kesh", &to(not)), "{not}");
            assert!(!name_heard("Kesh", &to(not)), "{not}");
        }
    }

    /// The eyes stand-in on a real socket: what each request sent, and the
    /// answer it gives to each.
    fn eyes(answers: Vec<Value>) -> (PathBuf, std::thread::JoinHandle<Vec<Value>>) {
        use std::io::{BufRead, BufReader, Write};
        let directory = std::env::temp_dir().join(format!(
            "lamp-voice-names-{}-{}",
            std::process::id(),
            answers.len()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let socket = directory.join("who.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let asked = std::thread::spawn(move || {
            let mut asked = Vec::new();
            for answer in answers {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                asked.push(serde_json::from_str(&line).unwrap());
                let mut out = serde_json::to_vec(&answer).unwrap();
                out.push(b'\n');
                stream.write_all(&out).unwrap();
            }
            asked
        });
        (socket, asked)
    }

    /// The first builder's 13:24, with names checked: a name the person
    /// did not say goes nowhere and the model is told to ask for the
    /// spelling; a spelled one goes to the eyes as one name for one face; a
    /// face known by another name is said so; and `replace` goes once they
    /// said yes. Without the check, it is asked plainly, as before.
    #[test]
    fn with_names_checked_only_a_name_they_said_reaches_the_eyes_and_one_face_keeps_one() {
        let call = |args: Value| ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args,
        };
        let (socket, asked) = eyes(vec![
            json!({"error": "known_as", "known_as": "A"}),
            json!({"remembered": "Jim"}),
            json!({"remembered": "B"}),
        ]);
        let (words, answered) = run_with(
            &socket,
            &call(json!({"name": "B"})),
            true,
            &Said::now("How do you spell my name? No, mi nombre es y la"),
        );
        assert!(answered && words["error"].as_str().unwrap().contains("spell"));
        let (words, _) = run_with(
            &socket,
            &call(json!({"name": "Jim"})),
            true,
            &Said::now("It's J I M."),
        );
        assert!(words["error"].as_str().unwrap().contains("as A"), "{words}");
        // `replace` before they agreed goes nowhere.
        let (words, _) = run_with(
            &socket,
            &call(json!({"name": "Jim", "replace": true})),
            true,
            &Said::now("My name is Jim."),
        );
        assert!(words["error"].as_str().unwrap().contains("yes"), "{words}");
        let agreed = Said {
            this: "Yes, call me Jim.".into(),
            before: vec![(
                "It's J I M.".into(),
                "I know you as A. Shall I call you Jim instead?".into(),
            )],
        };
        let (words, _) = run_with(
            &socket,
            &call(json!({"name": "Jim", "replace": true})),
            true,
            &agreed,
        );
        assert_eq!(words, json!({"ok": true, "remembered": "Jim"}));
        let (words, _) = run_with(
            &socket,
            &call(json!({"name": "B"})),
            false,
            &Said::default(),
        );
        assert_eq!(words, json!({"ok": true, "remembered": "B"}));
        assert_eq!(
            asked.join().unwrap(),
            [
                json!({"remember": "Jim", "one_face": true, "replace": false}),
                json!({"remember": "Jim", "one_face": true, "replace": true}),
                json!({"remember": "B"}),
            ]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }
}
