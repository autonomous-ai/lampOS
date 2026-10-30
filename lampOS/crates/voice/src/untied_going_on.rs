//! A line let out only as the conversation going on, with no tie to the
//! lamp, keeps no conversation going, on the second route (#3882), off
//! unless `LAMP_UNTIED_GOING_ON=on`.
//!
//! On lamp-06c1 on 28 October, with the product unit's environment, made
//! voices and no faces, desk talk after a conversation with the lamp was
//! answered line after line: "Did you send the slides to Sam yet?", "Not
//! yet, I'll do it after lunch.", "Are you coming to the meeting at four?".
//! Each was let out only as "a conversation with the lamp is going on", and
//! each answer opened a new exchange, so the next line fell inside the
//! window again. A line begun over the lamp's answer was also exempt from
//! the "may be people's talk" note, by its timing alone.
//!
//! With the switch, a line let out only as the conversation going on is
//! *tied* to the lamp when it shows any of these ([`tied`]), else it is
//! untied:
//! - it answers the lamp's question, or names the lamp's own work (the
//!   worker reads both);
//! - its name, a face turned to the lamp or a near voice (such a line is
//!   never let out only as going on);
//! - it shares a content word, by stem or prefix, with the lamp's last
//!   answer or with the line that answer was to ([`shares_a_word`]);
//! - it refers back to what was said, or is a short "and X?"/"còn X?"
//!   ([`refers_back`]).
//! - it asks the lamp a question or for its light, sound or words, with no
//!   sign of desk talk ([`asks_the_lamp`], [`desk_talk`]).
//!
//! An untied line's answer renews no window (the worker's chain, as
//! `LAMP_GOING_ON_NOT_OWN` keeps it), and the line is told it may be
//! people's talk even when it began over the lamp's answer.

/// Where the worker reads the switch. `on` only.
pub const ENVIRONMENT: &str = "LAMP_UNTIED_GOING_ON";

/// Whether the switch is on in this process's environment.
pub fn on() -> bool {
    on_for(std::env::var(ENVIRONMENT).ok().as_deref())
}

fn on_for(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Small words any line has, beside the memory's own list
/// (`remember::SEARCH_COMMON`), which say nothing of what it is about.
const SMALL_WORDS: &[&str] = &[
    "also", "back", "bit", "just", "yet", "now", "today", "then", "well", "right", "here", "okay",
    "yeah", "sure", "really", "very", "too", "much", "lot", "one", "get", "got", "going", "gonna",
    "want", "need", "let", "make", "know", "think", "like", "good", "fine", "great", "thanks",
    "thank", "yes", "hey", "hello", "lamp", "i'm", "i'll", "it's", "that's", "you're", "don't",
    "can't", "didn't", "isn't", "what's", "there's", "let's", "maybe", "might", "still", "say",
    "said", "see", "out", "off", "into", "over", "after", "before", "than", "only", "even", "all",
];

/// Phrases that refer back to what was just said, as whole words anywhere
/// in a line.
const REFERS_BACK: &[&str] = &[
    "say that",
    "said that",
    "what about",
    "how about",
    "and you",
    "pardon",
    "what do you mean",
    "nói lại",
    "lần nữa",
    "thêm nữa",
    "thì sao",
    "cái đó",
    "gì cơ",
    "đợi đã",
];

/// Words that refer back only in a short line ([`SHORT_WORDS`] or fewer):
/// "Why is that?", "Is it?", "What?", "How come?", "Tại sao?", "Wait,
/// what?", "Tell me more.", "Stop.", "Sorry?". In a longer line they are
/// any sentence's ("Not yet, I'll do it after lunch.", "Sorry, did you send
/// the slides to Sam?", "I'll be back again after lunch.", Tests on #4562).
const REFERS_BACK_SHORT: &[&str] = &[
    "that",
    "it",
    "what",
    "why",
    "how",
    "when",
    "where",
    "who",
    "huh",
    "really",
    "again",
    "more",
    "wait",
    "stop",
    "sorry",
    "đó",
    "nó",
    "hả",
    "sao",
    "khoan",
    "dừng",
    "xin lỗi",
];

/// How many words a line may have for [`REFERS_BACK_SHORT`] to count.
const SHORT_WORDS: usize = 4;

/// Words that, opening a short question ([`FOLLOW_ON_WORDS`] or fewer),
/// carry the last one on with a new thing: "And in Hanoi?", "And
/// tomorrow?", "Còn Hà Nội?".
const FOLLOWS_ON: &[&str] = &["and", "còn"];

/// How many words a question opening with [`FOLLOWS_ON`] may have.
const FOLLOW_ON_WORDS: usize = 6;

/// A line's words, lower-cased, apostrophes kept, padded with a space each
/// side for whole-word matching.
fn words_of(text: &str) -> String {
    let lower = text.to_lowercase().replace('\u{2019}', "'");
    let spaced: String = lower
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect();
    format!(
        " {} ",
        spaced.split_whitespace().collect::<Vec<_>>().join(" ")
    )
}

/// A word with its quotes and a `'s` taken off: "what's" is "what",
/// "that's" "that", "Saigon's" "saigon".
fn plain(word: &str) -> &str {
    let word = word.trim_matches('\'');
    word.strip_suffix("'s").unwrap_or(word)
}

/// A line's words as [`plain`] words, padded as [`words_of`] pads them.
fn plain_words(text: &str) -> String {
    let words = words_of(text);
    let words: Vec<&str> = words.split_whitespace().map(plain).collect();
    format!(" {} ", words.join(" "))
}

/// Words that, opening a question, ask it of the one who knows: the
/// person asks about themselves or a thing ("Should I bring an umbrella?",
/// "Is it safe to go running?", "Is that hot for October?"), not about
/// the one they ask ("Are you coming to the meeting at four?").
const ASKS_OF_A_KNOWER: &[&str] = &[
    "should i",
    "shall i",
    "do i",
    "can i",
    "could i",
    "would i",
    "will i",
    "am i",
    "is it",
    "is that",
    "is this",
    "is there",
    "will it",
    "would it",
    "does it",
    "did it",
    "was it",
    "was that",
    "are there",
    "will there",
];

/// Words that ask for the lamp's own work at a line's start, its light,
/// sound and words: "Make the light a bit warmer.", "Turn the light down
/// please.", "Play some music.", "Tell me a joke.", "Can you check Hanoi
/// instead?". Asks of things a lamp has no hand in ("Can you pass me the
/// charger?") are not.
const LAMP_ASKS: &[&str] = &[
    "make",
    "turn",
    "play",
    "tell",
    "check",
    "set",
    "dim",
    "brighten",
    "show",
    "read",
    "look up",
    "switch",
    "change",
    "mute",
    "unmute",
    "pause",
    "resume",
    "skip",
    "sing",
    "remind",
    "explain",
    "bật",
    "tắt",
    "mở",
    "kể",
    "kiểm tra",
    "đặt",
    "chỉnh",
    "giảm",
    "tăng",
    "hát",
    "nhắc",
];

/// Words before an ask that leave it the same ask: "Can you check …",
/// "Please play …".
const ASK_LEADS: &[&str] = &[
    "please",
    "can you",
    "could you",
    "would you",
    "will you",
    "can you please",
    "could you please",
    "okay",
    "ok",
    "so",
    "now",
    "and",
    "then",
    "lamp",
    "hey lamp",
    "lamp ơi",
];

/// Whether `heard` asks the lamp a question or for its own work, by the
/// opening words of any of its parts (split at commas and stops):
/// [`ASKS_OF_A_KNOWER`] in a question, or [`LAMP_ASKS`] after any of
/// [`ASK_LEADS`]; and it shows no desk talk ([`desk_talk`]).
pub fn asks_the_lamp(heard: &str) -> bool {
    let question = heard.contains('?');
    let asks = |part: &str| {
        let words = plain_words(part);
        let opens = |phrase: &&str| words.starts_with(&format!(" {phrase} "));
        let rest = ASK_LEADS
            .iter()
            .filter(|lead| opens(lead))
            .max_by_key(|lead| lead.len())
            .map_or(words.as_str(), |lead| &words[lead.len() + 1..]);
        (question && ASKS_OF_A_KNOWER.iter().any(opens))
            || LAMP_ASKS
                .iter()
                .any(|phrase| rest.starts_with(&format!(" {phrase} ")))
    };
    heard.split([',', '.', ';', '!', '?']).any(asks) && !desk_talk(heard)
}

/// Words a line opens with that set off no name: "Okay, play some music.",
/// "Sorry, tell me again.".
const NOT_NAMES: &[&str] = &[
    "okay", "ok", "yes", "yeah", "no", "nope", "well", "so", "oh", "hey", "sorry", "wait",
    "please", "thanks", "lamp", "alright", "right", "hmm", "um", "uh", "and", "but", "now",
];

/// Words after which a capitalised word is someone else's name: "Tell Sam
/// the meeting moved", "Ask Linh if she's coming".
const TO_A_NAME: &[&str] = &["tell", "ask", "remind"];

/// Whether `heard` shows it is said between people, whatever it asks:
/// it turns to someone else ([`crate::turned_away::turns_to_another`]),
/// calls "you guys", "hey" someone other than the lamp, or "anh"/"chị"/"em"
/// someone at its start, sets off a name before a comma at its start
/// ("Sam, should I bring an umbrella?"), or tells or asks someone by name.
pub fn desk_talk(heard: &str) -> bool {
    let words = plain_words(heard);
    // "Anh Minh", "Chị Lan": a title, then a name; "Em muốn …" is the
    // speaker's own "I".
    let first: Vec<&str> = heard.split_whitespace().take(2).collect();
    let vietnamese_call = matches!(first.as_slice(), [title, name]
        if ["anh", "chị", "em"].contains(&title.to_lowercase().as_str())
            && name.chars().next().is_some_and(char::is_uppercase));
    // "hey" with no comma after it, then a word that is not the lamp's.
    let lower = heard.to_lowercase();
    let hey_someone = lower
        .split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| pair[0] == "hey" && words_of(pair[1]).trim() != "lamp");
    let set_off_name = heard.split_once(',').is_some_and(|(start, _)| {
        let start = start.trim();
        !start.contains(' ')
            && start.chars().next().is_some_and(char::is_uppercase)
            && !NOT_NAMES.contains(&start.to_lowercase().as_str())
    });
    let tells_a_name = heard
        .split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| {
            TO_A_NAME.contains(&pair[0].to_lowercase().as_str())
                && pair[1].chars().next().is_some_and(char::is_uppercase)
                && pair[1] != "I"
        });
    crate::turned_away::turns_to_another(heard)
        || words.contains(" you guys ")
        || vietnamese_call
        || hey_someone
        || set_off_name
        || tells_a_name
}

/// Whether `heard` refers back to what was just said: [`REFERS_BACK`]
/// anywhere, [`REFERS_BACK_SHORT`] in a short line, or a short question
/// opening with [`FOLLOWS_ON`].
pub fn refers_back(heard: &str) -> bool {
    let words = plain_words(heard);
    let says = |phrase: &&str| words.contains(&format!(" {phrase} "));
    let count = words.split_whitespace().count();
    let follows_on = heard.contains('?')
        && count <= FOLLOW_ON_WORDS
        && words
            .split_whitespace()
            .next()
            .is_some_and(|first| FOLLOWS_ON.contains(&first));
    REFERS_BACK.iter().any(says)
        || (count <= SHORT_WORDS && REFERS_BACK_SHORT.iter().any(says))
        || follows_on
}

/// Endings taken off an English word for its stem, the longest first.
const ENDINGS: &[&str] = &["iest", "ier", "est", "ing", "ed", "er", "ly", "y"];

/// The stem of a content word: a plural's `s` or `es` off ("glasses" is
/// "glass", "cities" "city"), then for an English word one of
/// [`ENDINGS`], and a consonant doubled before it ("sunny" is "sun",
/// "hotter" "hot", "rainy" "rain"). At least three letters are kept.
fn stem(word: &str) -> String {
    let long = |stem: &str| stem.chars().count() >= 3;
    let mut word = if let Some(stem) = word.strip_suffix("ies").filter(|s| long(s)) {
        format!("{stem}y")
    } else if let Some(stem) = ["sses", "xes", "zes", "ches", "shes"]
        .iter()
        .find_map(|ending| word.strip_suffix(ending).map(|s| (s, ending)))
        .map(|(s, ending)| format!("{s}{}", &ending[..ending.len() - 2]))
        .filter(|s| long(s))
    {
        stem
    } else {
        match word.strip_suffix('s') {
            Some(stem) if long(stem) && !stem.ends_with('s') => stem.to_owned(),
            _ => word.to_owned(),
        }
    };
    if !word.is_ascii() {
        return word;
    }
    if let Some(stem) = ENDINGS
        .iter()
        .find_map(|ending| word.strip_suffix(ending))
        .filter(|s| long(s))
    {
        let bytes = stem.as_bytes();
        let doubled = bytes.len() >= 4
            && bytes[bytes.len() - 1] == bytes[bytes.len() - 2]
            && !b"aeiouls".contains(&bytes[bytes.len() - 1]);
        word = if doubled {
            stem[..stem.len() - 1].to_owned()
        } else {
            stem.to_owned()
        };
    }
    word
}

/// The words of `text` that can say what it is about: of three letters or
/// more, not small words, each as its [`stem`].
fn content_words(text: &str) -> Vec<String> {
    words_of(text)
        .split_whitespace()
        .map(plain)
        .filter(|word| word.chars().count() >= 3)
        .filter(|word| {
            !SMALL_WORDS.contains(word) && !crate::remember::SEARCH_COMMON.contains(word)
        })
        .map(stem)
        .collect()
}

/// How long a stem must be for another word it begins to share it: "rain"
/// begins "rainfall", but "car" is no part of "card".
const PREFIX_CHARS: usize = 4;

/// Whether two stems are of one word: the same, or the shorter, of
/// [`PREFIX_CHARS`] or more, begins the longer.
fn same_word(one: &str, other: &str) -> bool {
    let (short, long) = if one.len() <= other.len() {
        (one, other)
    } else {
        (other, one)
    };
    short == long || (short.chars().count() >= PREFIX_CHARS && long.starts_with(short))
}

/// Whether `heard` shares a content word with any of `said`: the lamp's
/// last answer, and the line it answered.
pub fn shares_a_word(heard: &str, said: &[&str]) -> bool {
    let theirs: Vec<String> = said.iter().flat_map(|text| content_words(text)).collect();
    content_words(heard)
        .iter()
        .any(|word| theirs.iter().any(|their| same_word(word, their)))
}

/// Whether a line let out only as the conversation going on is tied to the
/// lamp by its words: it refers back, shares a content word with what the
/// lamp last said or the line that was to (`said`), or asks the lamp a
/// question or for its own work with no sign of desk talk
/// ([`asks_the_lamp`]). The worker adds the lamp's question and its own
/// work.
pub fn tied(heard: &str, said: &[&str]) -> bool {
    refers_back(heard) || shares_a_word(heard, said) || asks_the_lamp(heard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_is_on_only() {
        assert!(on_for(Some("on")));
        for value in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
            assert!(!on_for(value), "{value:?}");
        }
    }

    /// #3882's desk1 lines, after the lamp's Saigon answer, show no tie.
    #[test]
    fn desk_talk_after_an_answer_is_untied() {
        let said = [
            "Lamp, what's the weather in Saigon?",
            "It's sunny in Saigon, about thirty-three degrees.",
        ];
        for line in [
            "Did you send the slides to Sam yet?",
            "Not yet, I'll do it after lunch.",
            "Are you coming to the meeting at four?",
            "I think so, but I might be a bit late.",
            "Can you pass me the charger? The white one.",
        ] {
            assert!(!tied(line, &said), "{line}");
        }
    }

    /// Real follow-ups refer back or share a word, in English and
    /// Vietnamese.
    #[test]
    fn follow_ups_are_tied() {
        let said = [
            "Lamp, what's the weather in Saigon?",
            "It's sunny in Saigon, about thirty-three degrees.",
        ];
        for line in [
            "What about tomorrow?",
            "And what about tomorrow?",
            "Say that again.",
            "Sorry, can you say that last part again?",
            "Wait, what?",
            "What?",
            "Why is that?",
            "Tell me more.",
            "Stop.",
            "Is Saigon always this hot?",
            "Will it be sunny tomorrow too?",
            "How many degrees in Fahrenheit?",
            "Nói lại đi.",
            "Còn ngày mai thì sao?",
        ] {
            assert!(tied(line, &said), "{line}");
        }
    }

    /// Sixth's P2s on #4562: a wh-word or "tại sao"/"sao" alone in a short
    /// line, a short question opening "and"/"còn" with a new thing, and
    /// "what's"/"that's" read as "what"/"that". Nothing of the last answer
    /// is needed.
    #[test]
    fn short_questions_and_follow_ons_refer_back() {
        for line in [
            "Why?",
            "How come?",
            "When?",
            "Where is that?",
            "Who said?",
            "Tại sao?",
            "Sao vậy?",
            "And in Hanoi?",
            "And tomorrow?",
            "and in ho chi minh city?",
            "Còn Hà Nội?",
            "Còn thành phố Hồ Chí Minh?",
            "What's that?",
            "That's weird, why?",
            "What\u{2019}s it?",
        ] {
            assert!(refers_back(line), "{line}");
            assert!(tied(line, &[]), "{line}");
        }
        for line in [
            // Not a question, or too long to be a short follow-on.
            "And then I went home.",
            "And did you send the slides to Sam before the meeting?",
            "Còn sớm mà.",
            // A wh-word in a longer line is any sentence's.
            "When is the meeting with the design team?",
            "Not yet, I'll do it after lunch.",
            "That's the one I bought at the market last week.",
        ] {
            assert!(!refers_back(line), "{line}");
        }
    }

    /// Tests' P2 on #4562: a question asked of the one who knows, or an ask
    /// for the lamp's light, sound or words, is tied with no word of the
    /// answer; with a sign of desk talk it is not.
    #[test]
    fn questions_and_asks_of_the_lamp_are_tied() {
        let said = [
            "Lamp, what's the weather in Saigon?",
            "It's sunny in Saigon, about thirty-three degrees.",
        ];
        for line in [
            "Should I bring an umbrella?",
            "Do I need a jacket then?",
            "Should I take a jacket?",
            "Is it safe to go running?",
            "Is that hot for October?",
            "Can you check Hanoi instead?",
            "Make the light a bit warmer.",
            "Turn the light down please.",
            "Play some music.",
            "Tell me a joke.",
            "Please play something calm.",
            "Okay, tell me a joke.",
            "Bật nhạc đi.",
            "Em muốn nghe nhạc, bật nhạc đi.",
        ] {
            assert!(asks_the_lamp(line), "{line}");
            assert!(tied(line, &said), "{line}");
        }
        for line in [
            "Sam, should I bring an umbrella?",
            "Hey Sam, play some music.",
            "Should I bring an umbrella, you guys?",
            "Tell Sam the meeting moved to four.",
            "Anh Minh, bật nhạc đi.",
            "Mom, can you turn the light down?",
            // desk1's lines ask nothing of the lamp.
            "Did you send the slides to Sam yet?",
            "Are you coming to the meeting at four?",
            "Can you pass me the charger? The white one.",
            "Not yet, I'll do it after lunch.",
        ] {
            assert!(!asks_the_lamp(line), "{line}");
            assert!(!tied(line, &said), "{line}");
        }
        assert!(desk_talk("Hey Sam, play some music."));
        assert!(!desk_talk("Hey, play some music."));
        assert!(!desk_talk("Hey lamp, play some music."));
    }

    /// Tests' P3 on #4562: "wait", "stop", "more", "again" and "sorry" refer
    /// back only in a short line; in a longer one they are any sentence's.
    #[test]
    fn refer_back_words_tie_only_short_lines() {
        let said = [
            "Lamp, what's the weather in Saigon?",
            "It's sunny in Saigon, about thirty-three degrees.",
        ];
        for line in [
            "Sorry, did you send the slides to Sam?",
            "Wait for me at the door, I'm almost done.",
            "Can you do more of the report tonight?",
            "I'll be back again after lunch.",
        ] {
            assert!(!refers_back(line), "{line}");
            assert!(!tied(line, &said), "{line}");
        }
        for line in [
            "Wait, what?",
            "Tell me more.",
            "Stop.",
            "Sorry?",
            "Again?",
            "Khoan đã.",
        ] {
            assert!(refers_back(line), "{line}");
        }
    }

    /// Sixth's P3 on #4562: a word ties to another of its stem or that it
    /// begins, not only by a plural's `s`, and "glasses" is "glass".
    #[test]
    fn words_tie_by_a_shared_stem_or_a_prefix() {
        for (one, other) in [
            ("sun", "sunny"),
            ("rain", "rainy"),
            ("hot", "hotter"),
            ("hot", "hottest"),
            ("glasses", "glass"),
            ("boxes", "box"),
            ("cities", "city"),
            ("degrees", "degree"),
            ("forecast", "forecasting"),
            ("rain", "rainfall"),
        ] {
            assert!(shares_a_word(one, &[other]), "{one} {other}");
            assert!(shares_a_word(other, &[one]), "{other} {one}");
        }
        assert_eq!(stem("glasses"), "glass");
        assert_eq!(stem("sunny"), "sun");
        assert_eq!(stem("hotter"), "hot");
        assert_eq!(stem("rainy"), "rain");
        // A short word is no part of a longer one; Vietnamese is not
        // stemmed as English.
        for (one, other) in [("car", "card"), ("son", "song"), ("sam", "saigon")] {
            assert!(!shares_a_word(one, &[other]), "{one} {other}");
        }
        assert_eq!(stem("ngày"), "ngày");
        assert!(tied(
            "Will it be this hot again?",
            &["It's hotter than yesterday."]
        ));
        assert!(!tied(
            "Did you send the slides to Sam yet?",
            &["It's sunny in Saigon, about thirty-three degrees."]
        ));
    }
}
