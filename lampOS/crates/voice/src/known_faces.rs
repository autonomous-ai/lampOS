//! Faces the lamp was asked to remember (`docs/eyes.md`, "Who is there").
//!
//! The eyes keep the prints of the people who asked, and nobody else's. The
//! voice gives the model two functions that ask the eyes to remember or
//! forget the face in front of the lamp, and reads which of those people is
//! at the lamp from the eyes' small file, so that a skill's note and what a person
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
/// Who was here's names (#1997): the person at the lamp says yes or no to
/// being named to anyone who asks when they've been at the desk, through
/// the eyes' `visits_yes` and `visits_no`.
pub const TELL: &str = "tell_others_i_was_here";
pub const DONT_TELL: &str = "dont_tell_others_i_was_here";
/// Where they're offered, the switch lamp-skills reads too (`on` only).
pub const VISITS_ENVIRONMENT: &str = "LAMP_WHO_WAS_HERE_NAMES";
/// The provider worker's flag for it, after the names'.
pub const VISITS_FLAG: &str = "--who-visits";

/// What the face functions are switched to: names checked
/// (`LAMP_NAMES=checked`) and the yes and no to being named
/// (`LAMP_WHO_WAS_HERE_NAMES=on`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FaceSwitches {
    pub checked: bool,
    pub visits: bool,
    pub remember_speaker: bool,
    /// Not a switch: the save asked for now would be the first at setup,
    /// while the walk has not yet ended "who you are"
    /// ([`crate::onboarding::who_you_are_open`]). With names checked, the
    /// name is then said back for a yes, or spelled ([`run_switched`]).
    pub at_setup: bool,
    /// A refused `remember_my_face` or `forget_my_face` tells the model the
    /// face record as it stands, and the journal the refusal's reason name
    /// ([`RECORD_TOLD_ENVIRONMENT`], #5141 rows 9 and 10).
    pub record: bool,
}

/// A refused face request tells the model the face record as it stands
/// ([`record_now`]) and the journal its reason's name ([`refusal_reason`]),
/// and a held `forget_my_face` lasts through one question asking them to
/// spell their name (`second_provider::HeldForget`): #5141, rows 7, 9 and
/// 10, from lamp-0c4e on 3 Nov. The second route only; `on` only, off
/// unless set.
pub const RECORD_TOLD_ENVIRONMENT: &str = "LAMP_NAMES_RECORD_TOLD";

/// Whether [`RECORD_TOLD_ENVIRONMENT`] is on.
pub fn record_told_on() -> bool {
    std::env::var(RECORD_TOLD_ENVIRONMENT).ok().as_deref() == Some("on")
}

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

pub(crate) fn lower_words(text: &str) -> Vec<String> {
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
    plain_yes_beside(name, turn, &[])
}

/// The save's own ask, said with the yes to the read-back at setup: "Yes.
/// That's right. Please remember my face." (lamp-52e6 on stable 10226, 5
/// Nov, R1 and the walk's face step: the yes went past [`BESIDE_A_YES`],
/// so the face was never kept and the walk ended with the name only).
/// Only for the first save at setup ([`said_back_and_agreed`]); a rename's
/// yes and a held forget's keep [`plain_yes`].
const BESIDE_THE_SAVE: &[&str] = &[
    "remember", "face", "save", "keep", "and", "now", "go", "ahead", "can", "you",
];

/// [`plain_yes`], with `beside` also standing beside the yes.
fn plain_yes_beside(name: &str, turn: &str, beside: &[&str]) -> bool {
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
                || beside.contains(&word.as_str())
                || name.contains(&folded_words(word).concat())
        })
}

/// Whether `turn` is a plain yes and nothing else ([`plain_yes`] with no
/// name): "Yes.", "OK.", "Vâng ạ.", never "OK, what's my name?" or "Yes?".
/// What runs a forget held for a person's yes on the second route.
pub(crate) fn plain_agreement(turn: &str) -> bool {
    plain_yes("", turn)
}

/// Beside a yes to a held keep only ([`plain_agreement_to_keep`]).
const BESIDE_A_KEEP: &[&str] = &["keep"];

/// [`plain_agreement`], or a plain yes that also says to keep it: "Yes,
/// please keep that.", "Yes, keep it." (lamp-06c1, 5 Nov, S4: the held
/// keep was dropped as not a plain yes). Only for a keep held for the
/// person's yes; a held forget, and every other ask, takes
/// [`plain_agreement`].
pub(crate) fn plain_agreement_to_keep(turn: &str) -> bool {
    plain_yes_beside("", turn, BESIDE_A_KEEP)
}

/// Whether the lamp said `name` back in the exchange just before the turn
/// in hand, and the person said a plain yes to it now ([`plain_yes`]):
/// what `replace` needs, and a name kept from an earlier exchange.
pub(crate) fn agreed_to(name: &str, said: &Said) -> bool {
    plain_yes(name, &said.this)
        && said
            .before
            .last()
            .is_some_and(|(_, lamp)| said_by_them(name, lamp))
}

/// Whether, in the exchanges `said` holds, the person said `name`, the lamp
/// then said it back, and the person's next line was a plain yes to it
/// ([`plain_yes`]): the next exchange's own line, or the turn in hand if
/// the read-back was the last. What the first save at setup needs
/// (#5140), so the usual walk, "Marie, right?" "Yes." and then the face's
/// question and its yes, keeps the name in one call. A correction after
/// the read-back ("No, it's Loren.") is no yes to it. `replace` keeps
/// [`agreed_to`], the read-back in the lamp's last answer only. The yes may
/// carry the save's own ask ([`BESIDE_THE_SAVE`]), as people say it.
fn said_back_and_agreed(name: &str, said: &Said) -> bool {
    said.before.iter().enumerate().any(|(at, (_, lamp))| {
        let next = said
            .before
            .get(at + 1)
            .map_or(said.this.as_str(), |(person, _)| person.as_str());
        said.before[..=at]
            .iter()
            .any(|(person, _)| said_by_them(name, person))
            && said_back(name, lamp)
            && plain_yes_beside(name, next, BESIDE_THE_SAVE)
    })
}

/// What becomes of the first setup save refused only for the read-back's
/// yes ([`SAY_IT_BACK`]), held by the voice for the person's next turn
/// (`second_provider::HeldSave`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HeldSaveVerdict {
    /// The lamp said the name back and this turn is a plain yes to it
    /// ([`said_back_and_agreed`]): the voice runs the save itself.
    Runs,
    /// The lamp's last answer did not say it back: nothing to answer yet.
    Waits,
    /// The person answered the read-back with anything but a yes (a
    /// correction, a spelling, another name, a no): dropped.
    Drops,
}

/// [`HeldSaveVerdict`] for the save held under `name`, on the turn `said`
/// holds (lamp-52e6, 5 Nov, #5391's check: after "Walter. Have I got that
/// right?" and "Yes, that's my name." the model made no call, and no face
/// was kept).
pub(crate) fn held_save_verdict(name: &str, said: &Said) -> HeldSaveVerdict {
    if said_back_and_agreed(name, said) {
        return HeldSaveVerdict::Runs;
    }
    match said.before.last() {
        Some((_, lamp)) if said_back(name, lamp) => HeldSaveVerdict::Drops,
        // The face question with the name folded in is no read-back (El's
        // #5159 rule): its yes answers the face, so the hold goes.
        Some((_, lamp)) if said_by_them(name, lamp) => HeldSaveVerdict::Drops,
        _ if gives_a_name(&said.this) || spelled_name(&said.this).is_some() => {
            HeldSaveVerdict::Drops
        }
        _ => HeldSaveVerdict::Waits,
    }
}

/// Whether a tool answer is the first setup save's refusal for want of the
/// read-back's yes ([`SAY_IT_BACK`]): the voice holds that save.
pub(crate) fn refused_for_the_read_back(told: &Value) -> bool {
    told["error"] == SAY_IT_BACK
}

/// Whether the lamp's line `lamp` says `name` back as a question of its
/// own: a line that also asks about their face ("Nice to meet you, Marie!
/// May I remember your face?") is not one, since its yes may answer only
/// the face (El's P3 on #5159).
fn said_back(name: &str, lamp: &str) -> bool {
    let words = lower_words(lamp);
    said_by_them(name, lamp)
        && !words.iter().any(|word| word == "face" || word == "faces")
        && !words
            .windows(2)
            .any(|two| two == ["khu\u{f4}n", "m\u{1eb7}t"])
}

/// Whether the lamp's `answer` asks them to spell their name: a word of
/// spelling ("spell", "spelling", "spelled", "letter by letter", "đánh
/// vần") anywhere in it. Also what keeps a held `forget_my_face` through
/// that question (`second_provider::HeldForget`, #5141 row 7).
pub fn asks_to_spell(answer: &str) -> bool {
    let words = lower_words(answer);
    words.iter().any(|word| {
        matches!(
            word.as_str(),
            "spell" | "spelling" | "spelled" | "spelt" | "spells"
        )
    }) || words
        .windows(3)
        .any(|three| three == ["letter", "by", "letter"])
        || words
            .windows(2)
            .any(|two| two == ["\u{111}\u{e1}nh", "v\u{1ea7}n"])
}

/// Whether the lamp's last answer, the exchange just before the turn in
/// hand, asked them to spell their name ([`asks_to_spell`]): only then are
/// letters spelled in the turn their name, with no yes. Letters said
/// otherwise ("I work at I B M") are no name (El's P2 on #5159).
fn spelling_asked(said: &Said) -> bool {
    said.before
        .last()
        .is_some_and(|(_, lamp)| asks_to_spell(lamp))
}

/// The provider worker's argument for the names switch.
pub fn names_arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("checked") {
        vec![NAMES_FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// The provider worker's argument for Who was here's names, after the
/// names' ([`VISITS_ENVIRONMENT`]).
pub fn visits_arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![VISITS_FLAG.to_owned()]
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
    let spelled = spelled_runs(&theirs);
    !name.is_empty()
        && (spelled_whole(&name, &spelled)
            || name
                .iter()
                .all(|word| theirs.contains(word) || spelled.iter().any(|letters| letters == word)))
}

/// Whether `name` is said in `said` only as the word that called the lamp:
/// the lamp's name or a word written for it
/// ([`crate::attention::written_for_the_name`]), opening the line past any
/// greeting, set off by a comma or a stop, with more said after it, and the
/// name nowhere else. On lamp-4ace on 24 October the owner's "Lamp, remember
/// my face, and I'm …" was heard as "Man. Remember my face and do", and
/// their face was kept as "Man" and greeted by it. A line that is only the
/// name ("Helen.") still gives it, as does the name said again ("Helen,
/// remember my face. I'm Helen.").
fn only_the_call(name: &str, said: &str) -> bool {
    let name = folded_words(name);
    let [name] = name.as_slice() else {
        return false;
    };
    let mut rest = said;
    let opening = loop {
        let Some(start) = rest.find(char::is_alphanumeric) else {
            return false;
        };
        rest = &rest[start..];
        let end = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '\u{2019}'))
            .unwrap_or(rest.len());
        let (word, after) = rest.split_at(end);
        rest = after;
        if !crate::attention::is_greeting(word) {
            break word;
        }
    };
    let set_off = rest
        .trim_start()
        .starts_with([',', '.', '!', '?', ';', ':']);
    let more_after = rest.chars().any(char::is_alphanumeric);
    folded_words(opening).as_slice() == [name.clone()]
        && crate::attention::written_for_the_name(opening)
        && set_off
        && more_after
        && folded_words(said)
            .iter()
            .filter(|word| *word == name)
            .count()
            == 1
}

/// Whether each of `name`'s words is spelled letter by letter in `said`
/// ("L I N H."), not only said ("Linh."): what `apart` needs (both
/// reviewers' P2 on #2269: a name said again can be misheard again, as
/// exactly the near name).
pub fn spelled_by_them(name: &str, said: &str) -> bool {
    let name = folded_words(name);
    let spelled = spelled_runs(&folded_words(said));
    !name.is_empty()
        && (spelled_whole(&name, &spelled)
            || name
                .iter()
                .all(|word| spelled.iter().any(|letters| letters == word)))
}

/// The name the person spelled letter by letter in `said`, as they spelled
/// it: each run of two letters or more capitalised, the runs joined by a
/// space ("L O R E N." is "Loren"). None when nothing is spelled, or the
/// letters disagree with a count they gave ("four letters", five spelled).
/// What a name not heard is kept as at setup (#5140 row 6).
fn spelled_name(said: &str) -> Option<String> {
    let runs: Vec<String> = spelled_runs(&folded_words(said))
        .into_iter()
        .filter(|letters| letters.chars().count() >= 2)
        .collect();
    let letters: usize = runs.iter().map(|run| run.chars().count()).sum();
    if runs.is_empty() || letters_counted(said).is_some_and(|counted| counted != letters) {
        return None;
    }
    let capitalised = runs.iter().map(|run| {
        let mut chars = run.chars();
        chars
            .next()
            .map(|first| first.to_uppercase().chain(chars).collect::<String>())
            .unwrap_or_default()
    });
    Some(capitalised.collect::<Vec<_>>().join(" "))
}

/// Whether a name of more than one word is spelled as one run of letters
/// ("M A R Y A N N" for "Mary Ann"), with no pause between its words
/// (Reviewer-lead's P3 on #2275: such a person was asked to spell on every
/// try).
fn spelled_whole(name: &[String], spelled: &[String]) -> bool {
    name.len() > 1 && spelled.iter().any(|letters| *letters == name.concat())
}

/// The runs of single letters in `theirs`, each put together as a word
/// ([`said_by_them`]'s rules).
fn spelled_runs(theirs: &[String]) -> Vec<String> {
    let mut spelled: Vec<String> = Vec::new();
    let mut run = String::new();
    let mut doubled = false;
    for word in theirs {
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
    spelled
}

/// The words of `text` as a name is matched: lowercase, without accents,
/// written as one letter or as a letter and its mark, a word with an
/// apostrophe kept whole and ’ read as '.
pub(crate) fn folded_words(text: &str) -> Vec<String> {
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

/// What the person says just before their name, as a line's last words:
/// the name still to come, in English and Vietnamese ("tên tôi là", "tôi
/// tên là", "gọi tôi là", with "mình" or "em" for "tôi"; the reviewer's P3
/// on #2355). Not "I'm" or "I am" alone: "Yes, I am." ends a line.
const OPENS_A_NAME: &[&[&str]] = &[
    &["my", "name", "is"],
    &["my", "name's"],
    &["call", "me"],
    &["i'm", "called"],
    &["i", "am", "called"],
    &["the", "name", "is"],
    &["the", "name's"],
    &["tên", "tôi", "là"],
    &["tôi", "tên", "là"],
    &["gọi", "tôi", "là"],
    &["tên", "mình", "là"],
    &["mình", "tên", "là"],
    &["gọi", "mình", "là"],
    &["tên", "em", "là"],
    &["em", "tên", "là"],
    &["gọi", "em", "là"],
];

/// Said while a name is found: "My name is, um,", "Tên tôi là, ờ,".
const FILLERS: &[&str] = &["um", "uh", "er", "erm", "ah", "eh", "hmm", "ờ", "ừm"];

/// Words that, just before an opening, mean no name follows: "Don't call
/// me.", "Never call me."
const NO_NAME_AFTER: &[&str] = &["don't", "dont", "never", "not"];

/// Words a line opens with that make "call me" a request to call them,
/// whose "?" speech to text often drops: "Can you call me", "Please call
/// me" (the reviewer's P3 on #2355). "You can call me" still opens a name.
const CALL_ME_ASKS: &[&str] = &["can", "could", "will", "would", "please"];

/// Words before what a line says: a greeting and the lamp's name, skipped
/// to find the word it opens with ("Hey Lamp, can you call me").
const BEFORE_A_LINE: &[&str] = &["hey", "hi", "hello", "ok", "okay", "lamp", "so"];

/// Whether what the person said ends opening their name, the name still to
/// come: "Hey Lamp, my name is", "You can call me, um,". A line the
/// turn-end model ends there is cut before the name: on lamp-4ace "my name
/// is" and 400 ms of quiet was `model_complete` (0.933), and the name came
/// as the next line (#2135, the first builder on #1722 at 01:58). Never a
/// question ("Do you know what my name is?", "Can you call me?"), one asking
/// what it is ("Tell me what my name is."), "Don't call me.", or a line that
/// opens with "can", "could" or "please" before "call me" ("Can you call
/// me", a call asked for with its "?" lost).
pub fn opens_a_name(said: &str) -> bool {
    let said = said.replace('\u{2019}', "'");
    if said.trim_end().ends_with('?') {
        return false;
    }
    let mut words: Vec<String> = said
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    while words
        .last()
        .is_some_and(|word| FILLERS.contains(&word.as_str()))
    {
        words.pop();
    }
    // The word the last sentence opens with, past a greeting and the name.
    let last_sentence = said
        .split(['.', '!', '?'])
        .rfind(|sentence| sentence.chars().any(char::is_alphanumeric))
        .unwrap_or_default()
        .to_lowercase();
    let opens_with = last_sentence
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .find(|word| !word.is_empty() && !BEFORE_A_LINE.contains(word))
        .unwrap_or_default()
        .to_owned();
    OPENS_A_NAME.iter().any(|opening| {
        if *opening == ["call", "me"] && CALL_ME_ASKS.contains(&opens_with.as_str()) {
            return false;
        }
        let Some(at) = words.len().checked_sub(opening.len()) else {
            return false;
        };
        let before = at.checked_sub(1).map(|before| words[before].as_str());
        words[at..]
            .iter()
            .zip(opening.iter())
            .all(|(word, open)| word == open)
            && !before
                .is_some_and(|before| ASKING.contains(&before) || NO_NAME_AFTER.contains(&before))
    })
}

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

/// The guarded enrollment path needs the requested name to be the speaker's
/// declaration, not merely a name somewhere in the same line. Consume a
/// complete, simple introduction or bare name/spelling. Mixed narration and
/// corrections require a fresh introduction rather than selecting one clause.
/// `name_heard` still supplies the existing spelling and letter-count checks.
fn speaker_named_self(name: &str, said: &str) -> bool {
    let characters: Vec<_> = said.chars().collect();
    // Unsupported punctuation refuses before folding can erase quotation
    // marks or turn a non-ASCII question into an affirmative declaration.
    if characters
        .iter()
        .enumerate()
        .any(|(at, character)| match character {
            c if c.is_alphanumeric()
                || c.is_whitespace()
                || ('\u{300}'..='\u{36f}').contains(c) =>
            {
                false
            }
            '.' | ',' | '!' | '-' | '?' => false,
            '\'' | '’' => {
                !(at.checked_sub(1)
                    .is_some_and(|before| characters[before].is_alphanumeric())
                    && characters
                        .get(at + 1)
                        .is_some_and(|after| after.is_alphanumeric()))
            }
            _ => true,
        })
    {
        return false;
    }
    let name = folded_words(name);
    let words = folded_words(said);
    if DISAGREE
        .iter()
        .chain(NO_NAME_AFTER)
        .any(|word| words.contains(&folded_words(word).concat()))
    {
        return false;
    }
    if words.iter().any(|word| {
        [
            "maybe",
            "perhaps",
            "possibly",
            "probably",
            "apparently",
            "pretend",
            "suppose",
            "guess",
            "think",
            "said",
            "says",
            "told",
            "tells",
            "quote",
            "unquote",
        ]
        .contains(&word.as_str())
    }) {
        return false;
    }
    let mut body = words.as_slice();
    while body.first().is_some_and(|word| {
        BEFORE_A_LINE.contains(&word.as_str()) || ["yes", "please"].contains(&word.as_str())
    }) {
        body = &body[1..];
    }
    let had_preface = body.len() != words.len();
    let starts = |words: &[String], phrase: &[&str]| {
        words.len() >= phrase.len() && words.iter().zip(phrase).all(|(word, part)| word == part)
    };
    const REQUESTS: &[&[&str]] = &[
        &["can", "you", "remember", "my", "face"],
        &["could", "you", "remember", "my", "face"],
        &["will", "you", "remember", "my", "face"],
        &["would", "you", "remember", "my", "face"],
        &["can", "you", "remember", "my", "name"],
        &["could", "you", "remember", "my", "name"],
        &["please", "remember", "my", "face"],
        &["please", "remember", "my", "name"],
        &["remember", "my", "face"],
        &["remember", "my", "name"],
        &["remember", "me"],
    ];
    // A question mark can end an explicit request, not a tentative name:
    // "Can you remember my face? I'm Kesh." differs from "I'm Kesh?".
    let mut questions_at = Vec::new();
    let mut asked_to_remember = false;
    if !starts(body, &["remember", "my", "name", "is"])
        && let Some(request) = REQUESTS.iter().find(|request| starts(body, request))
    {
        body = &body[request.len()..];
        asked_to_remember = true;
        questions_at.push(words.len() - body.len());
        if body.first().is_some_and(|word| word == "and") {
            body = &body[1..];
        }
    }
    let polite_suffix = body.last().is_some_and(|word| word == "please");
    if polite_suffix {
        body = &body[..body.len() - 1];
    }
    if let Some(request) = REQUESTS.iter().find(|request| {
        body.len() >= request.len() && starts(&body[body.len() - request.len()..], request)
    }) {
        asked_to_remember = true;
        questions_at
            .push(words.len() - usize::from(words.last().is_some_and(|word| word == "please")));
        body = &body[..body.len() - request.len()];
        if body.last().is_some_and(|word| word == "and") {
            body = &body[..body.len() - 1];
        }
    }
    const INTRODUCTIONS: &[&[&str]] = &[
        &["could", "you", "remember", "that", "my", "name", "is"],
        &["can", "you", "remember", "that", "my", "name", "is"],
        &["remember", "my", "name", "is"],
        &["you", "can", "call", "me"],
        &["can", "you", "call", "me"],
        &["could", "you", "call", "me"],
        &["will", "you", "call", "me"],
        &["would", "you", "call", "me"],
        &["i", "am", "called"],
        &["i'm", "called"],
        &["my", "name", "is"],
        &["my", "name's"],
        &["call", "me"],
        &["i", "am"],
        &["i'm"],
        &["ten", "toi", "la"],
        &["toi", "ten", "la"],
        &["goi", "toi", "la"],
        &["toi", "la"],
        &["ten", "minh", "la"],
        &["minh", "ten", "la"],
        &["goi", "minh", "la"],
        &["minh", "la"],
        &["ten", "em", "la"],
        &["em", "ten", "la"],
        &["goi", "em", "la"],
        &["em", "la"],
    ];
    if let Some(opening) = INTRODUCTIONS.iter().find(|opening| starts(body, opening)) {
        if opening.contains(&"call") || opening.contains(&"remember") {
            questions_at.push(words.len());
        }
        body = &body[opening.len()..];
    } else if asked_to_remember || had_preface || polite_suffix {
        // "Hi, Kess", "Kess, please" and "Remember my face, Kess" may
        // address Kess. Only a complete bare name needs no introduction.
        return false;
    }
    let mut questions = said.match_indices('?');
    if let Some((at, _)) = questions.next()
        && (questions.next().is_some() || !questions_at.contains(&folded_words(&said[..at]).len()))
    {
        return false;
    }
    // Only the one final count belongs to this spelling. A second count or
    // other material remains in `body` and refuses the whole line.
    if body.len() >= 2 && matches!(body.last().map(String::as_str), Some("letter" | "letters")) {
        let count = body[body.len() - 2..].join(" ");
        if letters_counted(&count)
            != Some(
                name.iter()
                    .flat_map(|word| word.chars())
                    .filter(|c| c.is_alphabetic())
                    .count(),
            )
        {
            return false;
        }
        body = &body[..body.len() - 2];
    }
    // The caller cannot turn a sentence into a bare name by including its
    // grammar in the argument ("Her Name Is Kess"). Ambiguous word names
    // can still be given as an exact spelling, including the letter I.
    let plain_name = !name.iter().any(|word| {
        NOT_A_NAME.contains(&word.as_str())
            || [
                "my", "your", "his", "her", "their", "our", "name", "name's", "face", "is", "am",
                "are", "and", "but", "she", "he", "they", "you", "me", "this", "that", "remember",
            ]
            .contains(&word.as_str())
    });
    if plain_name && body == name {
        return !name.is_empty();
    }
    if plain_name && body.starts_with(&name) {
        body = &body[name.len()..];
        if body.first().is_some_and(|word| word == "spelled") {
            body = &body[1..];
        }
    }
    // Reuse the existing double-letter and "double U" rules, but no word
    // from another clause may be ignored between the letters of this name.
    let mut letters = body.iter();
    while let Some(word) = letters.next() {
        let letter = if word == "double" {
            letters.next()
        } else {
            Some(word)
        };
        if !letter.is_some_and(|letter| {
            letter.chars().count() == 1 && letter.chars().all(char::is_alphabetic)
        }) {
            return false;
        }
    }
    !name.is_empty() && spelled_runs(body) == [name.concat()]
}

/// Where the second route reads whether a line asking the lamp to remember
/// the person's face, with their name, must call [`REMEMBER`] (`on` only;
/// [`asks_to_remember`]).
pub const ASKED_ENVIRONMENT: &str = "LAMP_SECOND_REMEMBER_ASKED";

/// Whether [`ASKED_ENVIRONMENT`] is on.
pub fn asked_on() -> bool {
    std::env::var(ASKED_ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// What asks the lamp to remember the speaker's own face or who they are.
/// Only their own: "remember her face" and "remember my friend" are not.
const ASKS_TO_REMEMBER: &[&[&str]] = &[
    &["remember", "my", "face"],
    &["remember", "my", "name"],
    &["remember", "me"],
    &["remember", "who", "i", "am"],
    &["remember", "what", "i", "look", "like"],
    &["learn", "my", "face"],
    &["save", "my", "face"],
];

/// Words anywhere in a line that make it no plain ask to remember them: a
/// refusal or a wish not to be kept ("I'd rather you didn't remember me",
/// "Please never remember me", "You cannot remember my face"), any word
/// ending in "n't" ("I don't want you to remember my face"), and the same
/// words written without the apostrophe ("You cant remember me"). With one,
/// the model is asked as before (reviewer-three's and the reviewer-lead's
/// P2s on #3298).
const NOT_A_PLAIN_ASK: &[&str] = &[
    "no", "not", "never", "nope", "rather", "stop", "without", "nobody", "neither", "nor",
    "cannot", "dont", "cant", "wont", "didnt", "doesnt", "shouldnt", "wouldnt", "couldnt", "isnt",
    "arent", "wasnt", "mustnt",
];

/// What may stand before the ask at a sentence's start, past a greeting and
/// the lamp's name ([`BEFORE_A_LINE`]): "please", and "can you", "could
/// you", "would you" or "will you", in that order, each at most once.
const ASKING_OPENERS: &[&str] = &["can", "could", "would", "will"];

/// Whether what the person said is a plain ask that the lamp remember their
/// own face or who they are ([`ASKS_TO_REMEMBER`]), at a sentence's start:
/// "Lamp, remember my face", "Can you remember me?", "Please save my face".
/// Never with a refusal anywhere in the line ([`NOT_A_PLAIN_ASK`]); never
/// the phrase further in, so reported speech ("She said remember my face"),
/// a question of what the lamp knows ("Do you remember me?") and "Help me
/// remember my name tag" are not; and never a bare question ("Remember
/// me?"), which asks whether the lamp knows them (the reviewer-lead's P2 on
/// #3298). English only: [`gives_a_name`], which a forced call needs beside
/// it, has no Vietnamese forms.
pub fn asks_to_remember(said: &str) -> bool {
    let refused = lower_words(said)
        .iter()
        .any(|word| NOT_A_PLAIN_ASK.contains(&word.as_str()) || word.ends_with("n't"));
    let said = said.replace('\u{2019}', "'");
    !refused
        && said.split_inclusive(['.', '!', '?', ';']).any(|sentence| {
            let words = lower_words(sentence);
            let mut at = 0;
            while words
                .get(at)
                .is_some_and(|word| BEFORE_A_LINE.contains(&word.as_str()))
            {
                at += 1;
            }
            if words.get(at).is_some_and(|word| word == "please") {
                at += 1;
            }
            let asking = words
                .get(at)
                .is_some_and(|word| ASKING_OPENERS.contains(&word.as_str()))
                && words.get(at + 1).is_some_and(|word| word == "you");
            if asking {
                at += 2;
                if words.get(at).is_some_and(|word| word == "please") {
                    at += 1;
                }
            }
            let a_question = sentence.trim_end().ends_with('?') && !asking;
            !a_question
                && ASKS_TO_REMEMBER.iter().any(|phrase| {
                    words.len() >= at + phrase.len()
                        && words[at..at + phrase.len()]
                            .iter()
                            .zip(phrase.iter())
                            .all(|(word, asked)| word == asked)
                })
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
/// What the model is told at setup, before the first save, when the name
/// was not said back and agreed to (#5140 row 5).
pub(crate) const SAY_IT_BACK: &str = "I have not kept their face yet. First say the name back to them and ask if you have it right. Only if they say yes, call this again with that name; if they correct it, say the new one back the same way.";
/// What the model is told when the name was only the word that called the
/// lamp ([`only_the_call`]).
const ONLY_THE_CALL: &str = "That word was them calling you, not their name, so I have not remembered anyone. Ask them what their name is, then call this again with the name they give.";
const NOT_OWN_NAME: &str = "I could not confirm that this was their own name, so I have not remembered anyone. Ask them to look at me and state their own name directly, or spell their name by itself, then try again.";
const NAME_UNCHANGED: &str = "I cannot safely replace a saved name from this request, so I have not changed anyone's saved face.";
/// What the model is told when it would replace a name before the person
/// agreed to the new one.
const NOT_AGREED: &str = "They have not said yes to that name yet. Say the new name back and ask whether they want to be known by it instead. Only if they say yes, call this again with the new name and replace true.";
/// What the model is told when the face is near someone known by a name one
/// letter from the one heard (#2176 row 4): the name may be misheard, so
/// they spell it, and no known name is said back. The spelled name goes with
/// `apart`, whatever the first call held, so a sibling heard as exactly a
/// known name is asked once, not twice (Reviewer-lead's P3 on #2252), and
/// the eyes still judge a known name's face. `apart` is taken only when the
/// turn in hand spells the name ([`spelled_by_them`]): a name said again, or
/// a yes, goes without it and is asked about again (both reviewers' P2 on
/// #2269).
const NEAR_TO: &str = "You may have misheard their name. Do not say any name you know. Ask them to spell their name, letter by letter, then call this again with the name as they spelled it and apart true.";
/// What the model is told when the face can't be kept under that name (it
/// is someone's the lamp knows, and this face isn't near theirs). It says
/// nothing of who the lamp knows, and no yes changes it (the reviewer's P1
/// and P2 on #2211).
const NOT_LIKE_THEM: &str = "You cannot keep this face under that name. Do not say why, and do not say whether you know anyone by it. Ask them for another name they would like to be known by. The lamp's owner can remove an old face in the app.";
/// Remembering looks for up to four seconds.
pub const DEADLINE: Duration = Duration::from_secs(6);
/// The known file counts while it is this fresh: the eyes renew it every
/// two seconds while the person is there.
pub(crate) const FRESH: Duration = Duration::from_secs(3);
const MOST_NAME_BYTES: usize = 120;

/// The two functions the model is told of when the eyes remember faces.
/// With names checked, remembering takes `replace`, for a face known by
/// another name.
pub fn tools() -> Vec<Tool> {
    tools_checked(false)
}

/// The same, with names checked or not.
pub fn tools_checked(checked: bool) -> Vec<Tool> {
    tools_for(FaceSwitches {
        checked,
        visits: false,
        remember_speaker: false,
        at_setup: false,
        record: false,
    })
}

/// The face functions as switched: with Who was here's names on, the yes
/// and the no to being named beside them ([`TELL`], [`DONT_TELL`]).
pub fn tools_for(switches: FaceSwitches) -> Vec<Tool> {
    let checked = switches.checked;
    let mut tools = vec![
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
                        "replace": {"type": "boolean", "description": "Only when you were told this face is known by another name, and they said yes to being known by this one instead."},
                        "apart": {"type": "boolean", "description": "Only when you were told you may have misheard their name, and they then spelled it letter by letter."}
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
    ];
    if switches.visits {
        tools.extend(visits_tools());
    }
    tools
}

fn visits_tools() -> [Tool; 2] {
    let name = || {
        Some(json!({
            "type": "object",
            "properties": {"name": {"type": "string", "description": "The name you know them by."}},
            "required": ["name"]
        }))
    };
    [
        Tool {
            name: TELL.into(),
            description: "Call this only on a clear yes, in their own words, from the person you know, to \
                          your telling anyone who asks when they've been at the desk, by name: after you \
                          asked them, or when they say so themselves (\"you can tell people when I've been \
                          here\"). Never on a maybe, and never for someone else."
                .into(),
            parameters: name(),
            silent: false,
        },
        Tool {
            name: DONT_TELL.into(),
            description: "Call this when someone you know says you should not tell others when they've \
                          been here, or takes back a yes to it (\"don't tell anyone I was here\"). Then \
                          tell them briefly that you'll only say someone you know was here."
                .into(),
            parameters: name(),
            silent: false,
        },
    ]
}

/// Whether the voice routes this function to the eyes.
pub fn is_tool(name: &str) -> bool {
    matches!(name, REMEMBER | FORGET | TELL | DONT_TELL)
}

/// Ask the eyes to remember or forget, and say what to tell the model; the
/// second value says whether the eyes answered. Blocking: for a thread of
/// its own. With names `checked`, a name the person did not say is not
/// kept, and a face known by another name is asked about first.
pub fn run(socket: &Path, call: &ToolCall, checked: bool) -> (Value, bool) {
    run_for(
        socket,
        call,
        FaceSwitches {
            checked,
            visits: false,
            remember_speaker: false,
            at_setup: false,
            record: false,
        },
        None,
        None,
    )
}

/// [`run`] as switched, for a request whose admitted scope is `person`'s
/// (`crate::remember::person_id`, `None` for the room's or unknown): a yes
/// to being named is taken only from that person's own scope. `line` is
/// who the eyes saw saying the request's line, if it was read
/// ([`crate::speaking_faces::line_seen`]): a room's "forget my face" needs
/// it ([`forget_name`]).
pub fn run_for(
    socket: &Path,
    call: &ToolCall,
    switches: FaceSwitches,
    person: Option<&str>,
    line: Option<&crate::speaking_faces::LineSeen>,
) -> (Value, bool) {
    run_for_with(socket, call, switches, person, line, None, &mut None)
}

/// [`run_for`], sending to the eyes only while `fence` holds: a forget made
/// on someone's authority sends nothing once that authority is gone
/// (Fourth's P2 on #3564: a line narrowed to the room, or revoked, between
/// the scope's check and the send). `fence` is read before the request is
/// sent, and the request goes on the guarded transport
/// ([`crate::remember::bounded_exchange_fenced`]), which reads it again
/// after the connect and before each write, and never blocks in the
/// connect. Refused before the write, nothing reaches the eyes.
pub fn run_for_fenced(
    socket: &Path,
    call: &ToolCall,
    switches: FaceSwitches,
    person: Option<&str>,
    line: Option<&crate::speaking_faces::LineSeen>,
    fence: &dyn Fn() -> bool,
) -> (Value, bool) {
    run_for_with(socket, call, switches, person, line, Some(fence), &mut None)
}

/// [`run_for`], fenced on `fence` when one is given ([`run_for_fenced`]),
/// with the people a remembering replaced put in `replaced`
/// ([`replaced_of`]).
fn run_for_with(
    socket: &Path,
    call: &ToolCall,
    switches: FaceSwitches,
    person: Option<&str>,
    line: Option<&crate::speaking_faces::LineSeen>,
    fence: Option<&dyn Fn() -> bool>,
    replaced: &mut Option<Vec<String>>,
) -> (Value, bool) {
    let told = run_face(socket, call, switches, person, line, fence, replaced);
    record_told(socket, call, told, switches, person, line)
}

/// [`run_for_with`] before a refusal is told the record ([`record_told`]).
fn run_face(
    socket: &Path,
    call: &ToolCall,
    switches: FaceSwitches,
    person: Option<&str>,
    line: Option<&crate::speaking_faces::LineSeen>,
    fence: Option<&dyn Fn() -> bool>,
    replaced: &mut Option<Vec<String>>,
) -> (Value, bool) {
    if switches.remember_speaker && call.name == REMEMBER {
        return (json!({"error": crate::remember_speaker::NOT_SEEN}), true);
    }
    if matches!(call.name.as_str(), TELL | DONT_TELL) {
        return if switches.visits {
            run_visits(socket, call, person)
        } else {
            (crate::tools::unavailable(), false)
        };
    }
    // "Forget my face" asks for the face of the known person whose line
    // this is, under the name the eyes keep (#2850); on a room's line, only
    // a named person whose own face was seen saying it.
    let mine;
    let call = if call.name == FORGET {
        let asked = call.args.get("name").and_then(Value::as_str);
        let known = FaceObservation::read(&known_file_of(socket));
        let Some(name) = forget_name(asked, known.as_ref(), person, line) else {
            eprintln!(
                "lamp-voice: forget my face named nobody whose line this is or whose face was seen saying it; nothing was sent"
            );
            return (json!({"error": NOT_THEIRS}), true);
        };
        mine = ToolCall {
            args: json!({"name": name}),
            ..call.clone()
        };
        &mine
    } else {
        call
    };
    let checked = switches.checked;
    let said = if checked {
        crate::thread::THREAD
            .lock()
            .map(|thread| thread.names_window(SAID_EXCHANGES))
            .unwrap_or_default()
    } else {
        Said::default()
    };
    run_switched(socket, call, switches, &said, fence, replaced)
}

/// Enrollment under the caller's revocable authority. The switch-off
/// request and all other face tools still use their existing path.
pub fn run_scoped(
    socket: &Path,
    call: &ToolCall,
    switches: FaceSwitches,
    person: Option<&str>,
    line: Option<&crate::speaking_faces::LineSeen>,
    request: u64,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> (Value, bool) {
    run_scoped_replacing(socket, call, switches, person, line, request, lease).0
}

/// [`run_scoped`], with the people the eyes answered a remembering replaced
/// ([`replaced_of`]) beside what the model is told, never in it: only
/// [`OwnerRename::answered`] reads them (Reviewer-three's P1 on #5149).
pub fn run_scoped_replacing(
    socket: &Path,
    call: &ToolCall,
    switches: FaceSwitches,
    person: Option<&str>,
    line: Option<&crate::speaking_faces::LineSeen>,
    request: u64,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> ((Value, bool), Option<Vec<String>>) {
    let mut replaced = None;
    let told = run_scoped_into(
        socket,
        call,
        switches,
        person,
        line,
        request,
        lease,
        &mut replaced,
    );
    (told, replaced)
}

#[allow(clippy::too_many_arguments)]
fn run_scoped_into(
    socket: &Path,
    call: &ToolCall,
    switches: FaceSwitches,
    person: Option<&str>,
    line: Option<&crate::speaking_faces::LineSeen>,
    request: u64,
    lease: Option<&crate::memory_scope::ScopeLease>,
    replaced: &mut Option<Vec<String>>,
) -> (Value, bool) {
    if call.name != REMEMBER || !switches.remember_speaker {
        return run_for_with(socket, call, switches, person, line, None, replaced);
    }
    let told = run_speaker(socket, call, switches, line, request, lease, replaced);
    record_told(socket, call, told, switches, person, line)
}

/// [`run_scoped`]'s own enrollment, before a refusal is told the record
/// ([`record_told`]).
fn run_speaker(
    socket: &Path,
    call: &ToolCall,
    switches: FaceSwitches,
    line: Option<&crate::speaking_faces::LineSeen>,
    request: u64,
    lease: Option<&crate::memory_scope::ScopeLease>,
    replaced: &mut Option<Vec<String>>,
) -> (Value, bool) {
    let proof = line
        .filter(|line| line.request == request)
        .and_then(|line| line.enrollment.as_ref());
    let lease = lease.filter(|lease| lease.key().request == request && lease.is_current());
    let (Some(proof), Some(lease)) = (proof, lease) else {
        return (json!({"error": crate::remember_speaker::NOT_SEEN}), true);
    };
    if !switches.checked || !proof.is_current(lamp_ipc::monotonic_us()) {
        return (json!({"error": crate::remember_speaker::NOT_SEEN}), true);
    }
    let Some(name) = call
        .args
        .get("name")
        .and_then(Value::as_str)
        // Bound caller-controlled work before trimming or checking the name.
        .filter(|name| name.len() <= crate::remember_speaker::HEARD_BYTES)
        .map(str::trim)
        .filter(|name| !name.is_empty())
    else {
        return (
            json!({"error": "There was no name in that for me to keep."}),
            true,
        );
    };
    // Earlier names may belong to someone else. This path uses only the
    // request's own heard words, including its spelling checks.
    let said = Said::now(&proof.heard);
    if !name_heard(name, &said) {
        return (json!({"error": NOT_HEARD}), true);
    }
    if !speaker_named_self(name, &said.this) {
        return (json!({"error": NOT_OWN_NAME}), true);
    }
    let replace = call.args.get("replace").and_then(Value::as_bool) == Some(true);
    // No earlier speaker confirmation is proven. Keep the existing
    // replacement check; it conservatively refuses on this scoped path.
    if replace && !agreed_to(name, &said) {
        return (json!({"error": NAME_UNCHANGED}), true);
    }
    let apart = call.args.get("apart").and_then(Value::as_bool) == Some(true)
        && spelled_by_them(name, &said.this);
    // Kept without a Korean vocative ("민수야" is 민수), checked as said.
    let name = crate::language::without_vocative(name, crate::language::at_start());
    let line = json!({"remember_speaker": {
        "name": name,
        "face": proof.face,
        "began_us": proof.began_us,
        "ended_us": proof.ended_us,
        "replace": replace,
        "apart": apart,
    }});
    let current = || proof.is_current(lamp_ipc::monotonic_us());
    match crate::remember::bounded_exchange_guarded(socket, &line, DEADLINE, lease, &current) {
        // A similar face is not authority to disclose its stored name, and
        // this path cannot prove a later replacement confirmation.
        Some(answer) if answer.get("error").and_then(Value::as_str) == Some("known_as") => {
            (json!({"error": NAME_UNCHANGED}), true)
        }
        Some(answer) => {
            *replaced = replaced_of(&answer);
            (told_asking(&answer, switches.visits), true)
        }
        None => (
            json!({"error": "I could not finish remembering that face. I have not confirmed that it was saved."}),
            false,
        ),
    }
}

/// What the model is told when a face request's authority went before it
/// was sent ([`run_for_fenced`]): nothing was done.
pub const AUTHORITY_GONE: &str =
    "That was not done: I could no longer be sure it was theirs to ask, so nothing was sent.";
/// What the model is told when "forget my face" names no face of the person
/// whose line it is, or, on a room's line, no face seen saying it
/// ([`forget_name`]): nothing is sent.
pub const NOT_THEIRS: &str = "I can only forget the face of the person asking, while I can see them at the lamp. They should look at me and ask again.";

/// What a refused face request is told when no face kept is the asker's
/// ([`record_now`]).
pub const RECORD_UNKNOWN: &str =
    "Nothing was changed, and no face I keep is the one asking right now.";

/// With [`FaceSwitches::record`] (`LAMP_NAMES_RECORD_TOLD`, #5141 rows 9
/// and 10): a refused `remember_my_face` or `forget_my_face` is told the
/// face record as it stands ([`record_now`], as `kept`), and the journal
/// says its reason's name ([`refusal_reason`]), never the person's words,
/// their name or an id. On lamp-0c4e on 3 Nov a face saved under a
/// misheard name could not be forgotten (`NOT_THEIRS`, asked under the
/// name they said) and nothing said what the lamp kept. Anything else, or
/// off, is as it was.
fn record_told(
    socket: &Path,
    call: &ToolCall,
    told: (Value, bool),
    switches: FaceSwitches,
    person: Option<&str>,
    line: Option<&crate::speaking_faces::LineSeen>,
) -> (Value, bool) {
    if !switches.record || !matches!(call.name.as_str(), REMEMBER | FORGET) {
        return told;
    }
    let (mut value, answered) = told;
    let Some(error) = value.get("error").and_then(Value::as_str) else {
        return (value, answered);
    };
    eprintln!("{}", refusal_line(&call.name, refusal_reason(error)));
    value["kept"] = json!(record_now(socket, person, line));
    (value, answered)
}

/// The face record as it stands, for the one asking: the name their face is
/// kept under, when it is theirs, else [`RECORD_UNKNOWN`]. Theirs is the
/// known face at the lamp when the line is that person's
/// (`person`, the admitted scope); on a room's line, the known face the
/// eyes saw saying it, and no other ([`forget_name`]'s rule). So it never
/// names someone else, who may only be in view.
pub fn record_now(
    socket: &Path,
    person: Option<&str>,
    line: Option<&crate::speaking_faces::LineSeen>,
) -> String {
    let theirs = match person {
        Some(person) => FaceObservation::read(&known_file_of(socket))
            .filter(|known| known.person() == person)
            .map(|known| known.name().to_owned()),
        None => line.and_then(|line| {
            let (name, _) = line.known.as_ref()?;
            crate::speaking_faces::seen_saying(
                &line.speakers,
                &memory_person(name),
                line.known.clone(),
            )
            .then(|| name.clone())
        }),
    };
    match theirs {
        Some(name) => format!(
            "Nothing was changed: the face of the one asking is kept as {name}. Use that name to forget or rename it."
        ),
        None => RECORD_UNKNOWN.to_owned(),
    }
}

/// The name of a face refusal's reason, for the journal: each refusal's
/// words to the model have one, the eyes' by their own code. Never words.
pub fn refusal_reason(error: &str) -> &'static str {
    const REASONS: &[(&str, &str)] = &[
        (NOT_HEARD, "not_heard"),
        (ONLY_THE_CALL, "only_the_call"),
        (NOT_OWN_NAME, "not_own_name"),
        (NAME_UNCHANGED, "name_unchanged"),
        (NOT_AGREED, "not_agreed"),
        (NEAR_TO, "near_to"),
        (NOT_LIKE_THEM, "not_like_them"),
        (NOT_THEIRS, "not_theirs"),
        (AUTHORITY_GONE, "authority_gone"),
        (NO_NAME, "no_name"),
        (NOT_KNOWN, "not_known"),
        (NOT_KNOWN_SOME_KEPT_APART, "not_known_some_kept_apart"),
        (EYES_NOT_ANSWERING, "eyes_not_answering"),
        (NOT_CONFIRMED, "not_confirmed"),
        (EYES_COULD_NOT, "eyes_could_not"),
        (crate::remember_speaker::NOT_SEEN, "speaker_not_seen"),
    ];
    if error.starts_with(KNOWN_AS) {
        return "known_as";
    }
    REASONS
        .iter()
        .chain(EYES_REFUSALS)
        .find(|(words, _)| *words == error)
        .map_or("other", |(_, reason)| *reason)
}

/// The journal's line for a refused face request: the function and the
/// reason's name ([`refusal_reason`]), nothing of the person.
pub fn refusal_line(function: &str, reason: &str) -> String {
    let function = if matches!(function, REMEMBER | FORGET) {
        function
    } else {
        "other"
    };
    format!(
        "lamp-voice: {function} refused: {reason}; the model is told the face record as it stands (LAMP_NAMES_RECORD_TOLD)"
    )
}

/// The name "forget my face" asks the eyes to forget, on a line whose
/// admitted scope is the known person `person`'s (#2850). The model's
/// `asked` name is a guess: on the second route it is told the person's
/// name only with `LAMP_SECOND_FACE_NAME=on`, and on lamp-c0a4 its name
/// matched nobody the eyes keep while their known file named the person.
/// So the known person at the lamp now, when they are `person`, is asked
/// for under the name the eyes keep, whatever the model wrote; else the
/// model's name only when it is `person`'s own; else nobody (`None`), so a
/// forget never reaches someone else's face.
///
/// A line of the room's (`person` `None`, no known person's) forgets the
/// named person only when it is theirs: the eyes judged speaking through it
/// (`line`, read when it was heard, [`crate::speaking_faces::LineSeen`]),
/// the known file then named the person the model asked for, and that
/// person's face, and no other, was seen saying it
/// ([`crate::speaking_faces::seen_saying`]); then they are asked for under
/// the eyes' name. Else nobody: no reading, a known file stale when the
/// line was heard, or another face speaking (a visitor or a screen while
/// they sit nearest) forgets nothing, since a forget destroys their face
/// and memory (the first builder on #2602, El's P2 on #3018). The call-time
/// `known` is not used for it: someone who sat down during a slow turn did
/// not say a line heard before they came (El's P3).
pub fn forget_name(
    asked: Option<&str>,
    known: Option<&FaceObservation>,
    person: Option<&str>,
    line: Option<&crate::speaking_faces::LineSeen>,
) -> Option<String> {
    let asked = asked.map(str::trim).filter(|asked| !asked.is_empty());
    let Some(person) = person else {
        let line = line?;
        let (name, _) = line.known.as_ref()?;
        let theirs = memory_person(name);
        return asked
            .filter(|asked| memory_person(asked) == theirs)
            .filter(|_| {
                crate::speaking_faces::seen_saying(&line.speakers, &theirs, line.known.clone())
            })
            .map(|_| name.clone());
    };
    if let Some(known) = known.filter(|known| known.person() == person) {
        return Some(known.name().to_owned());
    }
    asked
        .filter(|asked| memory_person(asked) == person)
        .map(str::to_owned)
}

#[cfg(test)]
fn run_with(socket: &Path, call: &ToolCall, checked: bool, said: &Said) -> (Value, bool) {
    let visits = false;
    run_switched(
        socket,
        call,
        FaceSwitches {
            checked,
            visits,
            remember_speaker: false,
            at_setup: false,
            record: false,
        },
        said,
        None,
        &mut None,
    )
}

/// [`run_with`], names checked, for the first save at setup.
#[cfg(test)]
fn run_at_setup(socket: &Path, call: &ToolCall, said: &Said) -> (Value, bool) {
    run_switched(
        socket,
        call,
        FaceSwitches {
            checked: true,
            at_setup: true,
            ..FaceSwitches::default()
        },
        said,
        None,
        &mut None,
    )
}

fn run_switched(
    socket: &Path,
    call: &ToolCall,
    switches: FaceSwitches,
    said: &Said,
    fence: Option<&dyn Fn() -> bool>,
    replaced: &mut Option<Vec<String>>,
) -> (Value, bool) {
    let checked = switches.checked;
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
    let setup = checked && switches.at_setup && call.name == REMEMBER;
    // At setup, a name not heard is taken as they spelled it in the turn in
    // hand: the letters are their own words, not the model's guess (#5140
    // row 6, lamp-0c4e: a misheard name never passed this check).
    let spelled;
    let heard = name_heard(name, said) || (setup && said_back_and_agreed(name, said));
    let name = if checked && call.name == REMEMBER && !heard {
        // Only after the lamp asked them to spell, and as many spelled runs
        // as the model's name has words: "L O R E N, and I'm at I B M" is
        // no one name (El's P2 on #5159).
        let words = folded_words(name).len();
        match (setup && spelling_asked(said))
            .then(|| spelled_name(&said.this))
            .flatten()
            .filter(|letters| letters.split_whitespace().count() == words)
        {
            Some(letters) => {
                spelled = letters;
                spelled.as_str()
            }
            None => return (json!({"error": NOT_HEARD}), true),
        }
    } else {
        name
    };
    if checked && call.name == REMEMBER && only_the_call(name, &said.this) {
        return (json!({"error": ONLY_THE_CALL}), true);
    }
    // The first save at setup: the name said back and a plain yes to it,
    // or spelled by them now (#5140 row 5, lamp-0c4e: the first name heard
    // was kept, misheard, without being said back).
    if setup
        && !(spelling_asked(said) && spelled_by_them(name, &said.this))
        && !said_back_and_agreed(name, said)
    {
        return (json!({"error": SAY_IT_BACK}), true);
    }
    let replace = call.args.get("replace").and_then(Value::as_bool) == Some(true);
    // A name replaces another only once the lamp said it back and they
    // said yes (the first builder on #1057: a misheard name was kept while
    // the person was still correcting it).
    if checked && call.name == REMEMBER && replace && !agreed_to(name, said) {
        return (json!({"error": NOT_AGREED}), true);
    }
    // `apart` only on a name spelled in the turn in hand: one said again may
    // be misheard again as exactly the near name (both reviewers' P2 on
    // #2269), so it goes without and the eyes ask about it again.
    let apart = call.args.get("apart").and_then(Value::as_bool) == Some(true)
        && spelled_by_them(name, &said.this);
    // A face is kept without a Korean vocative ("민수야" is 민수), checked
    // above as said.
    let name = if call.name == REMEMBER {
        crate::language::without_vocative(name, crate::language::at_start())
    } else {
        name
    };
    let line = match call.name.as_str() {
        // `apart` only when true: eyes from before #2176 row 4 refuse a
        // field they don't know.
        REMEMBER if checked && apart => {
            json!({"remember": name, "one_face": true, "replace": replace, "apart": true})
        }
        REMEMBER if checked => json!({"remember": name, "one_face": true, "replace": replace}),
        REMEMBER => json!({"remember": name}),
        FORGET => json!({"forget": name}),
        _ => return (crate::tools::unavailable(), false),
    };
    if fence.is_some_and(|fence| !fence()) {
        eprintln!(
            "lamp-voice: a face request's authority was gone before it was sent; nothing was sent"
        );
        return (json!({"error": AUTHORITY_GONE}), true);
    }
    let answer = match fence {
        Some(fence) => crate::remember::bounded_exchange_fenced(socket, &line, DEADLINE, fence),
        None => crate::tools::exchange(socket, &line, DEADLINE),
    };
    match answer {
        Some(answer) => {
            *replaced = replaced_of(&answer);
            (told_asking(&answer, switches.visits), true)
        }
        None => (
            json!({"error": "My eye is not answering right now, so I could not do that."}),
            false,
        ),
    }
}

/// The people the eyes say a remembering replaced (`"replaced"`, the names
/// they were kept under, `[]` for none): `None` when the answer has no such
/// list of names, as from eyes older than #5149's P1 fix, which then moves
/// no owner ([`OwnerRename::answered`]). Never told to the model
/// ([`told_asking`] leaves it out) nor journaled.
pub fn replaced_of(answer: &Value) -> Option<Vec<String>> {
    answer
        .get("replaced")?
        .as_array()?
        .iter()
        .map(|name| name.as_str().map(str::to_owned))
        .collect()
}

/// Why a yes to being named is not the named person's own, if it is not:
/// the line's scope `person` must be the memory id of the name, `named`
/// ([`memory_person`]: with the stable id, the id the eyes gave that name,
/// so a person renamed A to B is B's own and a new face saved as A is not;
/// without it, `person_id` of the name, as before).
fn whose_yes(person: Option<&str>, named: &str) -> Option<&'static str> {
    match person {
        None => Some("the line's scope is the room's"),
        Some(id) if id != named => Some("the line's scope is someone else's"),
        Some(_) => None,
    }
}

/// What the model is told when a yes to being named may not be the named
/// person's own: nothing is sent, and it isn't asked again in that turn.
pub const NOT_SURE_WHOSE_YES: &str = "I can't be sure that yes was theirs, so I haven't taken it. Don't ask again now; they can tell you when it's only them at the lamp.";
/// The eyes answer a yes or no to being named within this.
const VISITS_DEADLINE: Duration = Duration::from_millis(1_500);

/// A yes or no to being named, through the eyes. A yes only from the named
/// person's own scope (`person`; Third on #1997); a no from anyone at the
/// lamp, since it only narrows. The journal names nobody.
///
/// The scope is whose face was in view as the line began, not who spoke:
/// someone off camera saying yes for the person in view is taken, unless a
/// face seen speaking narrowed the line to the room
/// (`LAMP_SECOND_SPEAKING_FACE`, the second route only). The person hears
/// the lamp confirm, and can say no.
fn run_visits(socket: &Path, call: &ToolCall, person: Option<&str>) -> (Value, bool) {
    let Some(name) = call
        .args
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
    else {
        return (json!({"error": "There was no name in that."}), true);
    };
    let yes = call.name == TELL;
    if yes && let Some(why) = whose_yes(person, &memory_person(name)) {
        eprintln!(
            "lamp-voice: a yes to being named in Who was here was not taken: {why}; nothing was sent"
        );
        return (json!({"error": NOT_SURE_WHOSE_YES}), true);
    }
    let line = if yes {
        json!({"visits_yes": name})
    } else {
        json!({"visits_no": name})
    };
    match crate::tools::exchange(socket, &line, VISITS_DEADLINE) {
        Some(answer) => {
            let told = visits_told(&answer, yes);
            if told.get("ok").is_some() {
                eprintln!(
                    "lamp-voice: someone the lamp knows said {} to being named in Who was here",
                    if yes { "yes" } else { "no" }
                );
            }
            (told, true)
        }
        None => (
            json!({"error": "My eye is not answering right now, so I could not do that."}),
            false,
        ),
    }
}

/// What the eyes said to a yes or no to being named, in words the model
/// can say.
fn visits_told(answer: &Value, yes: bool) -> Value {
    match (answer.get("visits").and_then(Value::as_str), yes) {
        (Some("yes"), true) => {
            return json!({"ok": true, "note": "Tell them in a few words that you'll say it's them when someone asks who was here."});
        }
        (Some("no"), false) => {
            return json!({"ok": true, "note": "Tell them in a few words that you'll only say someone you know was here."});
        }
        _ => {}
    }
    let words = match answer.get("error").and_then(Value::as_str) {
        Some("not_here") => {
            "I can only take that yes while I can see them at the lamp. They should look at me and say it again."
        }
        Some("not_known") => "I don't know a face by that name.",
        Some("tellable_off") => "That part of me is not running right now, so I could not do it.",
        _ => "My eye could not do that just now.",
    };
    json!({"error": words})
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

/// What a person just remembered is asked once, with Who was here's names
/// on (#1997; Third and the reviewer: the ask says it's when they've been
/// here, told to anyone who asks).
pub const ASK_TO_NAME: &str = "Then ask them once whether you may tell anyone who asks when they've been at the desk, by name; otherwise you'd only say someone you know was here. Call tell_others_i_was_here only on their own clear yes, and dont_tell_others_i_was_here on a no.";

/// What the eyes said, in words the model can say.
#[cfg(test)]
fn told(answer: &Value) -> Value {
    told_asking(answer, false)
}

/// [`told`], and with Who was here's names on (`visits`), a person just
/// remembered is asked once whether they may be named ([`ASK_TO_NAME`]).
fn told_asking(answer: &Value, visits: bool) -> Value {
    if let Some(name) = answer.get("remembered").and_then(Value::as_str) {
        // With visits counted (`lamp-eyes --visits on`), the person is told so
        // in one line, as they are remembered (the first builder on #1670).
        let counted = answer.get("visits").and_then(Value::as_bool) == Some(true);
        let also_say = match (counted, visits) {
            (true, true) => Some(format!("{VISITS_LINE} {ASK_TO_NAME}")),
            (true, false) => Some(VISITS_LINE.to_owned()),
            (false, true) => Some(ASK_TO_NAME.to_owned()),
            (false, false) => None,
        };
        return match also_say {
            Some(also_say) => json!({
                "ok": true,
                "remembered": name,
                "also_say": also_say,
            }),
            None => json!({"ok": true, "remembered": name}),
        };
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
    // No name the lamp knows is said back: the face may be someone else's
    // who only looks alike (the reviewer's P1 and P2 on #2202).
    if answer.get("error").and_then(Value::as_str) == Some("near_to") {
        return json!({"error": NEAR_TO});
    }
    if answer.get("error").and_then(Value::as_str) == Some("not_like_them") {
        return json!({"error": NOT_LIKE_THEM});
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
        Some("speaker_not_seen" | "speaker_expired" | "not_that_face") => {
            crate::remember_speaker::NOT_SEEN
        }
        Some("peer_closed") => "That request ended before I could confirm a face was saved.",
        Some("busy") => "I was already remembering someone. They should ask again in a moment.",
        Some("full") => {
            "I already know as many faces as I can keep. Someone has to be forgotten first."
        }
        Some("not_a_name") => "That is not a name I can keep.",
        _ => "My eye could not do that just now.",
    };
    json!({"error": words})
}

/// The eyes' refusals by their own code, as `told_asking` words them: each
/// one's words, then its code, which is also its reason's name in the
/// journal ([`refusal_reason`]). Beside `told_asking`'s own words, not in
/// place of them, so that code is left as it was; a test pins that they
/// agree. The other refusal words below are the same: each is the text of a
/// refusal written where it is made.
const EYES_REFUSALS: &[(&str, &str)] = &[
    (
        "I could not see a face. They should look at me and ask again.",
        "no_face",
    ),
    (
        "Their face was too far away. They should come closer and ask again.",
        "too_far",
    ),
    (
        "There was more than one face in front of me. Only they should be in front of me when they ask again.",
        "two_faces",
    ),
    (
        "My eye is shut just now, so I could not see them.",
        "eyes_shut",
    ),
    (
        "That request ended before I could confirm a face was saved.",
        "peer_closed",
    ),
    (
        "I was already remembering someone. They should ask again in a moment.",
        "busy",
    ),
    (
        "I already know as many faces as I can keep. Someone has to be forgotten first.",
        "full",
    ),
    ("That is not a name I can keep.", "not_a_name"),
];
/// The eyes' refusal with a code not in [`EYES_REFUSALS`], or none.
const EYES_COULD_NOT: &str = "My eye could not do that just now.";
/// A face request with no name.
const NO_NAME: &str = "There was no name in that for me to keep.";
/// A forget of a name the eyes keep no face under.
const NOT_KNOWN: &str = "I did not know a face by that name.";
/// The eyes did not answer in time.
const EYES_NOT_ANSWERING: &str = "My eye is not answering right now, so I could not do that.";
/// A scoped enrollment the eyes never confirmed.
const NOT_CONFIRMED: &str =
    "I could not finish remembering that face. I have not confirmed that it was saved.";
/// How the eyes' `known_as` refusal begins, the known name after it.
const KNOWN_AS: &str = "You already know this face as ";

/// A fresh source-file observation, with expiry anchored to the source age.
/// This identifies a visible face, not the speaker, and never extends session
/// ownership. Reading belongs off the input/audio path.
#[derive(Clone, Debug)]
pub struct FaceObservation {
    name: String,
    /// The memory id the eyes keep beside the name (#2176), with
    /// `LAMP_MEMORY_STABLE_ID=on` on their side; `None` in a file of the
    /// name alone.
    id: Option<String>,
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
        // The name, and the id on the line after it when the eyes keep one
        // (#2176): anything else on that line is no observation.
        let (name, id) = lamp_msgs::known::read_known(&text)?;
        let mut observation = Self::from_source(name, modified, wall, now)?;
        observation.id = id.map(str::to_owned);
        if let Some(line) = disagreement(stable_ids(), id.is_some(), &SAID_DISAGREE) {
            eprintln!("{line}");
        }
        note_seen(&observation.name, observation.id.as_deref());
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
            id: None,
            valid_until: now.checked_add(remaining)?,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    /// Whose memory this face is: [`person_of`] its name and id.
    pub fn person(&self) -> String {
        person_of(&self.name, self.id.as_deref(), stable_ids())
    }
    pub fn valid_until(&self) -> Instant {
        self.valid_until
    }
    pub fn is_fresh(&self, now: Instant) -> bool {
        now < self.valid_until
    }
}

/// Whether memory is keyed on the eyes' stable id
/// ([`lamp_msgs::known::ENVIRONMENT`]), read once for the process.
fn stable_ids() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(lamp_msgs::known::stable_ids)
}

/// Whether the voice's switch and the eyes' disagree, said in the journal
/// once for the process.
static SAID_DISAGREE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The journal line, the first time only, when the voice's own
/// `LAMP_MEMORY_STABLE_ID` (`on`) and what the eyes say (`eyes_id`: an id
/// line in the known file) differ. The voice follows the eyes only when
/// both agree; on a mismatch it uses no ids ([`person_of`] gives
/// `person_id` of the name). No name and no id in the line.
fn disagreement(
    on: bool,
    eyes_id: bool,
    said: &std::sync::atomic::AtomicBool,
) -> Option<&'static str> {
    if on == eyes_id || said.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    Some(if on {
        "lamp-voice: LAMP_MEMORY_STABLE_ID is on here but the eyes keep no memory id; memory uses no ids"
    } else {
        "lamp-voice: the eyes keep a memory id but LAMP_MEMORY_STABLE_ID is off here; memory uses no ids"
    })
}

/// The one resolver for whose memory a known person's is (#2176): with the
/// stable id `on` here and an id from the eyes (theirs on too), that id;
/// else `person_id` of the name, the memory's key before the stable id, so
/// off, or with the two switches apart, nothing moves.
pub fn person_of(name: &str, id: Option<&str>, on: bool) -> String {
    match id {
        Some(id) if on && lamp_msgs::known::is_person_id(id) => id.to_owned(),
        _ => lamp_msgs::known::person_id(name),
    }
}

/// The ids the eyes last gave beside each name, as the known file was read:
/// so a name the voice holds (the one counted at the lamp, the one at its
/// start) resolves to the id it was read with. A file of the name alone
/// takes the name's id away. At most [`MOST_SEEN_IDS`], the oldest let go.
#[derive(Default)]
pub struct SeenIds(Vec<(String, String)>);

/// More than the eyes keep (20 people).
const MOST_SEEN_IDS: usize = 32;

impl SeenIds {
    fn key(name: &str) -> String {
        name.trim().to_lowercase()
    }
    /// `name` was read with `id` beside it, or none.
    pub fn note(&mut self, name: &str, id: Option<&str>) {
        let key = Self::key(name);
        self.0.retain(|(seen, _)| *seen != key);
        if let Some(id) = id.filter(|id| lamp_msgs::known::is_person_id(id)) {
            self.0.push((key, id.to_owned()));
            if self.0.len() > MOST_SEEN_IDS {
                self.0.remove(0);
            }
        }
    }
    /// [`person_of`] `name` and the id it was last read with.
    pub fn person(&self, name: &str, on: bool) -> String {
        let key = Self::key(name);
        let id = self
            .0
            .iter()
            .find(|(seen, _)| *seen == key)
            .map(|(_, id)| id.as_str());
        person_of(name, id, on)
    }
}

static SEEN_IDS: std::sync::Mutex<SeenIds> = std::sync::Mutex::new(SeenIds(Vec::new()));

fn note_seen(name: &str, id: Option<&str>) {
    if stable_ids()
        && let Ok(mut seen) = SEEN_IDS.lock()
    {
        seen.note(name, id);
    }
}

/// Whose memory the known person by this name is ([`person_of`]): every
/// place the voice makes a memory id from a name the eyes gave comes here.
pub fn memory_person(name: &str) -> String {
    let on = stable_ids();
    if !on {
        return lamp_msgs::known::person_id(name);
    }
    SEEN_IDS.lock().map_or_else(
        |_| lamp_msgs::known::person_id(name),
        |seen| seen.person(name, on),
    )
}

/// Existing face lookup, backed by a bounded observation read. Future or
/// expired source timestamps fail closed rather than appearing freshly seen.
pub fn known_now(path: &Path) -> Option<String> {
    FaceObservation::read(path).map(|observation| observation.name)
}

/// The names of everyone the eyes know (`{"list":true}` on their socket),
/// or `None` when they can't be asked within [`LIST_WITHIN`].
pub fn names_known(socket: &Path) -> Option<Vec<String>> {
    names_reached(socket).flatten()
}

/// [`names_known`], told apart: `None` for eyes that can't be reached (none
/// listening: stopped, crashed, never started), `Some(None)` for eyes
/// reached that gave no answer within [`LIST_WITHIN`] (the Reviewer-lead's
/// P2 on #4325).
pub fn names_reached(socket: &Path) -> Option<Option<Vec<String>>> {
    let answer = crate::tools::reached(socket, &serde_json::json!({"list": true}), LIST_WITHIN)?;
    Some(answer.and_then(|answer| {
        answer["people"].as_array().map(|people| {
            people
                .iter()
                .filter_map(|name| name.as_str().map(str::to_owned))
                .collect()
        })
    }))
}

/// How long [`names_known`] waits for the eyes: only [`keep_owner_fresh`]
/// asks, off any reply's path, and the eyes answer between two of their
/// looks, slower on a busy lamp (lamp-29e7, 29 Oct: a remembering answered
/// in 2.1 s), so a wait of 250 ms could leave the owner unnamed (#4118).
pub const LIST_WITHIN: Duration = Duration::from_secs(3);

/// Whether the eyes still know `owner` ([`names_known`]), by their own
/// rule for one name (any case): so a forgotten face is never named (the
/// Reviewer-lead's P3 on #4290). No socket, or the eyes not answering, is
/// no.
pub fn still_known(socket: Option<&Path>, owner: &str) -> bool {
    socket
        .and_then(names_known)
        .is_some_and(|names| among(&names, owner))
}

/// Whether `owner` is one of the eyes' `names`, as the eyes match a name.
fn among(names: &[String], owner: &str) -> bool {
    names
        .iter()
        .any(|name| name.to_lowercase() == owner.to_lowercase())
}

/// How often [`keep_owner_fresh`] reads the owner mark and asks the eyes:
/// a save is seen within it; a forget clears the owner at once
/// ([`owner_forgotten`]).
pub const OWNER_EVERY: Duration = Duration::from_secs(10);

/// The owner as [`keep_owner_fresh`] last found them, and how many forgets
/// there have been, so a refresh begun before a forget never brings back
/// the name it cleared.
static OWNER_NOW: std::sync::Mutex<(Option<String>, u64)> = std::sync::Mutex::new((None, 0));

#[cfg(test)]
thread_local! {
    /// [`owner_for_test`]'s owner, this thread's only.
    static OWNER_FOR_TEST: std::cell::RefCell<Option<Option<String>>> =
        const { std::cell::RefCell::new(None) };
}

/// Tests: [`owner_now`] answers `owner` on this thread; `None` gives back
/// the kept one.
#[cfg(test)]
pub(crate) fn owner_for_test(owner: Option<Option<String>>) {
    OWNER_FOR_TEST.with(|cell| *cell.borrow_mut() = owner);
}

/// The owner kept at setup while the eyes still know them, as last found:
/// no reply waits on the eyes (Reviewer-two's P3 on #4290).
pub fn owner_now() -> Option<String> {
    #[cfg(test)]
    if let Some(owner) = OWNER_FOR_TEST.with(|cell| cell.borrow().clone()) {
        return owner;
    }
    OWNER_NOW
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .0
        .clone()
}

/// A face forgotten, whoever's: the owner is named no more from now, and
/// the onboarding walk drops the mark (Reviewer-three's P3 on #4290).
pub fn owner_forgotten() {
    let mut now = OWNER_NOW
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *now = (None, now.1.wrapping_add(1));
}

/// Renames of the owner's face under way ([`OwnerRename`]), by onboarding
/// file: how many are in flight, and how many have ended, so a refresh that
/// overlapped one never drops the owner it was moving ([`refresh_owner`]).
static OWNER_RENAMES: std::sync::Mutex<std::collections::BTreeMap<PathBuf, (u32, u64)>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// What [`OWNER_RENAMES`] says for `file` now.
fn renames_of(file: &Path) -> (u32, u64) {
    OWNER_RENAMES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(file)
        .copied()
        .unwrap_or_default()
}

/// A `remember_my_face` with `replace` while setup keeps an owner: the eyes
/// may rename the owner's own record (a face saved at setup under a name
/// misheard, renamed from name A to name B by the person, a tester's lamp
/// on 3 Nov), and [`refresh_owner`], matching the owner by name, would then
/// find A gone and drop the owner for good, so the lamp no longer knew
/// them. Counted in flight from before the eyes are asked until they
/// answered ([`OwnerRename::answered`]) or the call came to nothing
/// (dropped).
pub struct OwnerRename {
    file: PathBuf,
    owner: String,
}

impl OwnerRename {
    /// Begun only for a `remember_my_face` with `replace` and an owner kept
    /// in the onboarding file `onboarding`; else there is nothing to move.
    pub fn begin(call: &ToolCall, onboarding: Option<&Path>) -> Option<Self> {
        if call.name != REMEMBER || call.args.get("replace").and_then(Value::as_bool) != Some(true)
        {
            return None;
        }
        let file = onboarding?;
        let owner = crate::onboarding::owner_of(file)?;
        OWNER_RENAMES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(file.to_owned())
            .or_default()
            .0 += 1;
        Some(Self {
            file: file.to_owned(),
            owner,
        })
    }

    /// The eyes' answer as the model is told it (`told`), and the people
    /// they say it replaced (`replaced`, [`replaced_of`]): the owner moves
    /// to the name kept only when the eyes kept it and replaced exactly one
    /// person, the owner ([`moved_to`]). A list read after the save can't
    /// say whose face was renamed: the owner's removed in the app, then a
    /// visitor's own rename to that name, read the same and gave the
    /// visitor the owner's authority (Reviewer-three's P1 on #5149). Any
    /// other answer, or eyes that don't say (`None`), move nothing. The
    /// rename ends here either way, so a refresh drops an owner the eyes no
    /// longer know as before. The owner the voice names follows at once.
    ///
    /// The journal says whether the owner moved, and how many the eyes
    /// said they replaced, never a name.
    pub fn answered(self, told: &Value, replaced: Option<&[String]>) {
        let Some(to) = moved_to(&self.owner, told, replaced) else {
            if told["ok"] == true {
                match replaced {
                    Some(replaced) => eprintln!(
                        "lamp-voice: a face renamed while setup keeps an owner moved no owner: the eyes replaced {} people, not the owner alone",
                        replaced.len()
                    ),
                    None => eprintln!(
                        "lamp-voice: a face renamed while setup keeps an owner moved no owner: the eyes did not say whom they replaced"
                    ),
                }
            }
            return;
        };
        eprintln!(
            "lamp-voice: a rename of the setup owner's face moved the owner: the eyes replaced the owner alone"
        );
        crate::onboarding::owner_renamed(&self.file, &self.owner, to);
        renamed_in(
            &mut OWNER_NOW
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0,
            &self.owner,
            to,
        );
    }
}

/// The owner named (`now`, [`OWNER_NOW`]'s) follows a rename from `from`
/// to `to`, when it is still `from` (any case).
fn renamed_in(now: &mut Option<String>, from: &str, to: &str) {
    if now
        .as_ref()
        .is_some_and(|owner| owner.to_lowercase() == from.to_lowercase())
    {
        *now = Some(to.to_owned());
    }
}

/// The name `owner` moves to on the eyes' answer `told`: the one they kept,
/// when it was kept and the people it `replaced` are exactly `owner` (any
/// case, as the eyes match a name); else none.
fn moved_to<'a>(owner: &str, told: &'a Value, replaced: Option<&[String]>) -> Option<&'a str> {
    let to = told["remembered"].as_str().filter(|_| told["ok"] == true)?;
    match replaced? {
        [one] if among(std::slice::from_ref(one), owner) => Some(to),
        _ => None,
    }
}

impl Drop for OwnerRename {
    fn drop(&mut self) {
        let mut renames = OWNER_RENAMES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((in_flight, ended)) = renames.get_mut(&self.file) {
            *in_flight = in_flight.saturating_sub(1);
            *ended = ended.wrapping_add(1);
        }
    }
}

/// The owner kept at setup (`onboarding::owner_of`) while the eyes still
/// know them ([`still_known`]), refreshed into [`owner_now`] every
/// [`OWNER_EVERY`] on a thread of its own.
/// Started once per process, however many parts ask for it (the by-default
/// name and the connected services).
pub fn keep_owner_fresh(file: Option<PathBuf>, socket: Option<PathBuf>) {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| keep_owner_fresh_now(file, socket));
}

fn keep_owner_fresh_now(file: Option<PathBuf>, socket: Option<PathBuf>) {
    std::thread::spawn(move || {
        loop {
            refresh_owner(file.as_deref(), socket.as_deref());
            std::thread::sleep(OWNER_EVERY);
        }
    });
}

/// One refresh of [`owner_now`]: dropped if a forget came while the eyes
/// were asked. An owner the eyes answer they no longer know loses the mark
/// for good, however they were forgotten (the app's face removal too), so
/// a face saved later under that name is never taken for the owner (the
/// Reviewer-lead's and Reviewer-two's P2 on #4290). The eyes' ids can't
/// tell them apart: a name forgotten gets the same id back.
///
/// A rename of the owner's face ([`OwnerRename`]) in flight, or ended,
/// while the eyes were asked drops nothing and leaves [`owner_now`] as it
/// was: the eyes may already know them by the new name while the owner
/// mark is still being moved. The drop names the owner it drops
/// ([`crate::onboarding::owner_gone`]), so one read before a move the walk
/// had not yet committed is let go once the owner is the new name.
// ponytail: a forget and a save of the same name within one OWNER_EVERY
// keeps the mark; the eyes would need to say when a face was added.
fn refresh_owner(file: Option<&Path>, socket: Option<&Path>) {
    let forgets = OWNER_NOW
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .1;
    let renames = file.map(renames_of);
    let owner = file.and_then(crate::onboarding::owner_of);
    let names = owner.as_ref().and(socket).and_then(names_reached);
    let found = match (owner, names) {
        (Some(owner), Some(Some(names))) if among(&names, &owner) => Some(owner),
        (Some(owner), Some(Some(_))) => {
            if let Some(file) = file {
                let now = renames_of(file);
                if now.0 > 0 || Some(now) != renames {
                    return;
                }
                crate::onboarding::owner_gone(file, &owner);
            }
            None
        }
        // Eyes reached but slow to answer take nothing back: the mark
        // stands as kept, and a forget by voice still clears it at once
        // ([`owner_forgotten`]) (lamp-29e7, 29 Oct, #4118). Eyes that can't
        // be reached at all name nobody, as before: nothing could then
        // tell a stranger is there (the Reviewer-lead's P2 on #4325).
        // ponytail: a slow-eyes outage keeps the mark as long as it lasts;
        // a bound of a few refreshes if that ever matters (Reviewer-three's P3).
        (Some(owner), Some(None)) => Some(owner),
        _ => None,
    };
    let mut now = OWNER_NOW
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if now.1 == forgets {
        now.0 = found;
    }
}

/// The file the eyes keep the known person's name in while they are at the
/// lamp: `known`, beside their request socket (`lamp-eyes --known-file`).
pub fn known_file_of(socket: &Path) -> PathBuf {
    socket.with_file_name("known")
}

/// What the eyes' known file says, and when the eyes last wrote their names
/// file beside it, as one refresh of the face cache read them: a change
/// from the last refresh's makes the cache cold
/// ([`crate::memory_scope::FaceCache::known_file_read`], #2807), whoever
/// wrote it (the eyes' own socket, the app, the voice's tool). The known
/// file's words change when the known face at the lamp does (removed when
/// nobody known is near); the eyes rewrite `tellable` (`lamp-eyes`'s
/// `who::tellable_of`) after every face remembered, forgotten or renamed,
/// while anyone has said yes to being named. Its mtime only, never its
/// names. Not freshness: the known file's mtime alone, rewritten each look,
/// is no change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownMark {
    known: Option<Vec<u8>>,
    names_written: Option<SystemTime>,
}

impl KnownMark {
    /// Bounded like [`FaceObservation::read`]; a missing or unreadable file
    /// is `None` in its place.
    pub fn read(known: &Path) -> Self {
        use std::io::Read;
        let words = std::fs::File::open(known).ok().and_then(|file| {
            let mut bytes = Vec::new();
            file.take(4097).read_to_end(&mut bytes).ok()?;
            Some(bytes)
        });
        let names_written = std::fs::metadata(known.with_file_name("tellable"))
            .and_then(|metadata| metadata.modified())
            .ok();
        Self {
            known: words,
            names_written,
        }
    }
}

/// Only faces of people at the desk are counted as people at the lamp
/// (`LAMP_FACES_AT_DESK_ONLY=on`, [`people_at_lamp`]); off unless set, and
/// then every face the eyes report is one, as before. On lamp-a0ae (2 Nov,
/// right after setup) the owner's name was held back on 51 of 79 turns as
/// "more than one face in view": beside the owner's face (0.09 to 0.19 of
/// the frame wide, 0.80 to 0.92 sure) the eyes kept reporting faces 0.02 to
/// 0.06 wide at 0.61 to 0.78, a new track id nearly every look, in about
/// half of all looks. The memory owner's rule: only people at the desk
/// count.
pub const AT_DESK_ENVIRONMENT: &str = "LAMP_FACES_AT_DESK_ONLY";

/// A face is someone at the desk only if its box is at least this share of
/// the reference face's height in the same look ([`at_desk`]): relative,
/// so it holds near the lamp and far from it. From lamp-06c1's recording (3
/// Nov): a same-size face beside the owner read 0.93 to 0.95 of their
/// height, one at 0.75 of the size 0.83, at 0.625 0.63 to 0.70, at 0.55
/// 0.55 to 0.56; a0ae's far faces were at most 0.46 of the owner's width.
/// 0.5 would count the 0.55 face.
pub const AT_DESK_HEIGHT: f32 = 0.6;

/// A face is someone at the desk only if the eyes are at least this sure it
/// is a face ([`at_desk`]): a second guard behind the size. On lamp-06c1 the
/// weakest real same-size second face read 0.78, so 0.75 sat too close;
/// a0ae's far faces read 0.61 to 0.78.
pub const AT_DESK_CONFIDENCE: f32 = 0.70;

/// The reference face is the tallest of those at least
/// [`AT_DESK_CONFIDENCE`] sure and at most this share of the frame tall: on
/// lamp-06c1 (3 Nov, the arm moving) a false box 0.85 to 0.97 of the frame
/// tall at 0.61 to 0.81 came and went, and as the reference it would have
/// dropped the owner (0.16 to 0.27 tall).
pub const AT_DESK_TALLEST: f32 = 0.6;

/// Whether [`AT_DESK_ENVIRONMENT`] is exactly `on`, read once.
pub fn at_desk_only() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var(AT_DESK_ENVIRONMENT).ok().as_deref() == Some("on"))
}

/// For each face of one look, whether it is someone at the desk: with
/// `desk_only`, at least [`AT_DESK_CONFIDENCE`] sure and at least
/// [`AT_DESK_HEIGHT`] of the reference face's height, the reference being
/// the tallest face at least that sure and at most [`AT_DESK_TALLEST`] of the
/// frame tall, so a giant false box never drops the owner. A look with a
/// face in it never counts nobody: when none passes, its tallest face counts
/// (the owner's own confidence fell to 0.61 at an edge on lamp-06c1).
/// Without `desk_only`, every face. The eyes' list itself is never changed.
pub fn at_desk(faces: &[lamp_msgs::Face], desk_only: bool) -> Vec<bool> {
    if !desk_only {
        return vec![true; faces.len()];
    }
    let height = |face: &lamp_msgs::Face| face.ymax - face.ymin;
    let sure = |face: &lamp_msgs::Face| face.confidence >= AT_DESK_CONFIDENCE;
    let reference = faces
        .iter()
        .filter(|face| sure(face) && height(face) <= AT_DESK_TALLEST)
        .map(height)
        .fold(0.0_f32, f32::max);
    let mut counted: Vec<bool> = faces
        .iter()
        .map(|face| sure(face) && reference > 0.0 && height(face) >= AT_DESK_HEIGHT * reference)
        .collect();
    if !counted.contains(&true)
        && let Some(tallest) = (0..faces.len())
            .max_by(|one, other| height(&faces[*one]).total_cmp(&height(&faces[*other])))
    {
        counted[tallest] = true;
    }
    counted
}

/// How many people are at the lamp in one look of the eyes: the faces
/// [`at_desk`] keeps, with [`at_desk_only`]; every face without.
pub fn people_at_lamp(faces: &lamp_msgs::Faces) -> usize {
    count_at_desk(faces, at_desk_only())
}

/// [`people_at_lamp`] with the switch given.
pub fn count_at_desk(faces: &lamp_msgs::Faces, desk_only: bool) -> usize {
    at_desk(&faces.faces, desk_only)
        .into_iter()
        .filter(|counted| *counted)
        .count()
}

/// For the journal: each face of one look as its box's width and height
/// and its confidence, counted ones first, then the dropped ones
/// ([`at_desk`]). Sizes only, no id, no place in the picture.
pub fn faces_told(faces: &lamp_msgs::Faces, desk_only: bool) -> String {
    let counted = at_desk(&faces.faces, desk_only);
    let told = |keep: bool| {
        let sizes: Vec<String> = faces
            .faces
            .iter()
            .zip(&counted)
            .filter(|(_, counted)| **counted == keep)
            .map(|(face, _)| {
                format!(
                    "w{:.2} h{:.2} c{:.2}",
                    face.xmax - face.xmin,
                    face.ymax - face.ymin,
                    face.confidence
                )
            })
            .collect();
        if sizes.is_empty() {
            "none".to_owned()
        } else {
            sizes.join(", ")
        }
    };
    format!("counted {}; dropped {}", told(true), told(false))
}

/// Whether a face of one look that the eyes read speaking (`yes`) is one
/// [`at_desk`] does not count: someone far behind the owner talking to the
/// lamp (the Reviewer-lead's P2 on #4987). Never without `desk_only`, as
/// every face counts then.
pub fn speaking_not_at_desk(faces: &lamp_msgs::Faces, desk_only: bool) -> bool {
    at_desk(&faces.faces, desk_only)
        .into_iter()
        .zip(&faces.faces)
        .any(|(counted, face)| {
            !counted
                && face
                    .speaking
                    .as_ref()
                    .is_some_and(|speaking| speaking.state == lamp_msgs::SpeakingState::Yes)
        })
}

/// Whether the name-by-default decision line tells the deciding look's
/// faces ([`faces_told`]): with `desk_only`, or when a face was dropped,
/// so the line reads as before with the switch off.
pub fn faces_worth_telling(told: &str, desk_only: bool) -> bool {
    !told.is_empty() && (desk_only || !told.ends_with("; dropped none"))
}

/// How many people the eyes see at the lamp now ([`people_at_lamp`]), from
/// their newest look on the bus (`humans/faces/tracked`), `None` when there
/// is no unexpired look: for a known face's held gap
/// ([`crate::memory_scope::FaceCache::replace_seen`]), which holds only when
/// the eyes looked and saw none.
pub fn faces_in_view(bus: &lamp_bus::Bus) -> Option<usize> {
    bus.latest(lamp_msgs::Faces::TOPIC)
        .ok()
        .flatten()
        .filter(|message| !message.expired)
        .and_then(|message| message.parse::<lamp_msgs::Faces>().ok())
        .map(|faces| people_at_lamp(&faces))
}

/// How far back [`most_faces_lately`] looks: one look that missed a face
/// (lamp-52e6, 29 Oct 13:03: two faces in 36 of 37 looks, and the reply
/// read the one with none) never makes two people one.
pub const FACES_WITHIN: Duration = Duration::from_secs(3);

/// How long a face not at the desk seen speaking holds the owner's name
/// back ([`spoke_not_at_desk_lately`]): longer than [`FACES_WITHIN`], so a
/// line that ended a few seconds before its reply is still covered.
pub const SPOKE_NOT_AT_DESK_WITHIN: Duration = Duration::from_secs(10);

/// How often [`keep_faces_counted`] reads the eyes' newest look: they look
/// about four times a second.
pub const FACES_EVERY: Duration = Duration::from_millis(100);

/// The eyes' face counts of the last [`FACES_WITHIN`], as [`faces_in_view`]
/// read them, each with its faces' sizes for the journal ([`faces_told`]).
#[derive(Debug, Default)]
pub struct FacesLately(
    std::collections::VecDeque<(std::time::Instant, usize, String)>,
    /// The last look counted, by its publisher's boot and `seq`.
    Option<(String, u64)>,
    /// When a face not at the desk last read speaking
    /// ([`speaking_not_at_desk`]).
    Option<std::time::Instant>,
);

impl FacesLately {
    pub fn seen(&mut self, at: std::time::Instant, faces: usize) {
        self.seen_told(at, faces, String::new());
    }

    /// [`Self::seen`] with the look's faces as the journal tells them.
    pub fn seen_told(&mut self, at: std::time::Instant, faces: usize, told: String) {
        self.0.push_back((at, faces, told));
        self.forget_before(at);
    }

    /// [`Self::seen`] for one of the eyes' looks, counted once however
    /// often it is read: the bus keeps a look as its latest value for 2 s
    /// and [`keep_faces_counted`] reads it every [`FACES_EVERY`], so one
    /// spurious look read three times would pass for two (the Reviewer-lead's
    /// and Fourth's P2 on #4364).
    pub fn look(&mut self, at: std::time::Instant, look: (String, u64), faces: usize) {
        self.look_told(at, look, faces, String::new());
    }

    /// [`Self::look`] with the look's faces as the journal tells them.
    pub fn look_told(
        &mut self,
        at: std::time::Instant,
        look: (String, u64),
        faces: usize,
        told: String,
    ) {
        if self.1.as_ref() != Some(&look) {
            self.1 = Some(look);
            self.seen_told(at, faces, told);
        }
    }

    /// The most faces seen in at least two looks within [`FACES_WITHIN`] of
    /// `now` (in the one look, when there is only one), `None` with no look
    /// in it: one look that missed a face never makes two people one, and
    /// one look that saw a second face that isn't there (lamp-52e6, 29 Oct
    /// 16:52, the one screen face) never makes one person two (#4118).
    pub fn most(&mut self, now: std::time::Instant) -> Option<usize> {
        self.forget_before(now);
        let mut counts: Vec<usize> = self.0.iter().map(|(_, faces, _)| *faces).collect();
        counts.sort_unstable_by(|a, b| b.cmp(a));
        counts.get(1).or(counts.first()).copied()
    }

    /// The faces' sizes of the newest look in [`FACES_WITHIN`] of `now`
    /// with as many people as [`Self::most`] gives, for the journal: the
    /// look that decided it. Empty with none.
    pub fn most_told(&mut self, now: std::time::Instant) -> String {
        let Some(most) = self.most(now) else {
            return String::new();
        };
        self.0
            .iter()
            .rev()
            .find(|(_, faces, _)| *faces == most)
            .map(|(_, _, told)| told.clone())
            .unwrap_or_default()
    }

    /// A face not at the desk read speaking in a look at `at`
    /// ([`speaking_not_at_desk`]).
    pub fn spoke_not_at_desk(&mut self, at: std::time::Instant) {
        self.2 = Some(at);
    }

    /// Whether a face not at the desk read speaking within
    /// [`SPOKE_NOT_AT_DESK_WITHIN`] of `now`.
    pub fn spoke_not_at_desk_since(&self, now: std::time::Instant) -> bool {
        self.2
            .is_some_and(|at| now.saturating_duration_since(at) <= SPOKE_NOT_AT_DESK_WITHIN)
    }

    fn forget_before(&mut self, now: std::time::Instant) {
        while self
            .0
            .front()
            .is_some_and(|(at, _, _)| now.saturating_duration_since(*at) > FACES_WITHIN)
        {
            self.0.pop_front();
        }
    }
}

static FACES_LATELY: std::sync::Mutex<FacesLately> =
    std::sync::Mutex::new(FacesLately(std::collections::VecDeque::new(), None, None));

/// Read the eyes' newest look every [`FACES_EVERY`] into
/// [`most_faces_lately`], on a thread of its own.
// ponytail: polls `latest`, so a look between two reads can be missed; a
// bus subscriber if that ever matters.
pub fn keep_faces_counted(bus: lamp_bus::Bus) {
    // Once per process: the by-default name and the connected services
    // both count faces.
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| keep_faces_counted_now(bus));
}

fn keep_faces_counted_now(bus: lamp_bus::Bus) {
    std::thread::spawn(move || {
        loop {
            if let Some((look, faces)) = bus
                .latest(lamp_msgs::Faces::TOPIC)
                .ok()
                .flatten()
                .filter(|message| !message.expired)
                .and_then(|message| {
                    let look = (message.header.boot.clone(), message.header.seq);
                    message
                        .parse::<lamp_msgs::Faces>()
                        .ok()
                        .map(|faces| (look, faces))
                })
            {
                let desk_only = at_desk_only();
                let mut lately = FACES_LATELY
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                // Told only for a new look: the sizes are formatted once.
                if lately.1.as_ref() != Some(&look) {
                    let at = std::time::Instant::now();
                    if speaking_not_at_desk(&faces, desk_only) {
                        lately.spoke_not_at_desk(at);
                    }
                    lately.look_told(
                        at,
                        look,
                        count_at_desk(&faces, desk_only),
                        faces_told(&faces, desk_only),
                    );
                }
            }
            std::thread::sleep(FACES_EVERY);
        }
    });
}

/// The most faces in any of the eyes' looks of the last [`FACES_WITHIN`]
/// ([`keep_faces_counted`]), `None` with none in it.
pub fn most_faces_lately() -> Option<usize> {
    FACES_LATELY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .most(std::time::Instant::now())
}

/// Whether a face not at the desk read speaking in the eyes' looks of the
/// last [`SPOKE_NOT_AT_DESK_WITHIN`] ([`keep_faces_counted`]), for
/// `person::someone_else_now`.
pub fn spoke_not_at_desk_lately() -> bool {
    FACES_LATELY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .spoke_not_at_desk_since(std::time::Instant::now())
}

/// The faces' sizes of the look [`most_faces_lately`] took its count from
/// ([`FacesLately::most_told`], [`faces_told`]), for the journal; empty
/// with none.
pub fn most_faces_lately_told() -> String {
    FACES_LATELY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .most_told(std::time::Instant::now())
}

/// How long ago the eyes may have written the known file for
/// [`known_lately`]: they rewrite it at each check, about every 2 s and
/// slower when busy, and remove it as soon as the near face matches nobody,
/// so a file still there is the last check's word.
pub const KNOWN_LATELY: Duration = Duration::from_secs(8);

/// The name in the eyes' known file at `path` while it is at most
/// [`KNOWN_LATELY`] old, for the by-default rule (#3884): [`known_now`]'s
/// 3 s read a file rewritten every 2–3 s as nobody (lamp-52e6, 29 Oct
/// 16:49, "an unknown face in view" with the file there; #4118).
pub fn known_lately(path: &Path) -> Option<String> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    if age > KNOWN_LATELY {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    (text.len() <= 4096)
        .then(|| lamp_msgs::known::read_known(&text).map(|(name, _)| name.to_owned()))
        .flatten()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// lamp-06c1, 5 Nov, S4: "Yes, please keep that." is a yes to a held
    /// keep, never a plain yes to anything else; the rest still fails both.
    #[test]
    fn keep_stands_beside_a_yes_only_to_a_held_keep() {
        for yes in [
            "Yes, please keep that.",
            "Yes, keep it.",
            "Yes. Please keep that.",
        ] {
            assert!(plain_agreement_to_keep(yes), "{yes}");
            assert!(!plain_agreement(yes), "{yes}");
        }
        for yes in ["Yes.", "Yes, please."] {
            assert!(
                plain_agreement_to_keep(yes) && plain_agreement(yes),
                "{yes}"
            );
        }
        for not in [
            "Keep that?",
            "No, keep it.",
            "Yes, keep it, and forget my coffee.",
            "Yes, keep my face.",
        ] {
            assert!(!plain_agreement_to_keep(not), "{not}");
        }
    }

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

    /// #1997: with Who was here's names on, someone just remembered is
    /// asked once whether they may be named, beside #1670's visits line;
    /// the eyes' yes, no and refusals are said in words.
    #[test]
    fn someone_remembered_is_asked_once_about_being_named_and_the_eyes_answer_is_said() {
        assert_eq!(
            told_asking(&json!({"remembered":"Minh"}), true),
            json!({"ok":true,"remembered":"Minh","also_say":ASK_TO_NAME})
        );
        assert_eq!(
            told_asking(&json!({"remembered":"Minh","visits":true}), true)["also_say"],
            format!("{VISITS_LINE} {ASK_TO_NAME}")
        );
        assert!(ASK_TO_NAME.contains("when") && ASK_TO_NAME.contains("anyone who asks"));
        assert_eq!(
            told_asking(&json!({"forgot":true}), true),
            json!({"ok":true})
        );
        assert_eq!(visits_told(&json!({"visits":"yes"}), true)["ok"], true);
        assert_eq!(visits_told(&json!({"visits":"no"}), false)["ok"], true);
        // An answer that isn't the one asked for is no success.
        assert!(visits_told(&json!({"visits":"no"}), true)["error"].is_string());
        for code in ["not_here", "not_known", "tellable_off", "storage", "else"] {
            let words = visits_told(&json!({"error": code}), true);
            assert!(
                words["error"]
                    .as_str()
                    .is_some_and(|words| words.ends_with('.')),
                "{code}"
            );
        }
        let names = |visits| {
            tools_for(FaceSwitches {
                checked: true,
                visits,
                remember_speaker: false,
                at_setup: false,
                record: false,
            })
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>()
        };
        assert_eq!(names(true), [REMEMBER, FORGET, TELL, DONT_TELL]);
        assert_eq!(names(false), [REMEMBER, FORGET]);
        assert!(is_tool(TELL) && is_tool(DONT_TELL));
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
        // The eyes' memory id after the name (#2176): the name is read, and
        // a second line that is not an id is nothing.
        std::fs::write(
            &path,
            format!("Minh\n{}\n", crate::remember::person_id("Sam")),
        )
        .unwrap();
        assert_eq!(known_now(&path).as_deref(), Some("Minh"));
        std::fs::write(&path, "Minh\nSam\n").unwrap();
        assert_eq!(known_now(&path), None);
        std::fs::remove_file(&path).unwrap();
        assert!(note("Minh").contains("Minh"));
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
    }

    /// #2135: a line that ends opening a name, the name still to come, is
    /// told from one that gives it, asks about it, or ends.
    #[test]
    fn a_line_ending_before_its_name_is_told_apart() {
        for open in [
            "Hey Lamp, my name is",
            "Hey Lamp, my name is.",
            "Hey, Lamp. My name is",
            "My name's",
            "Hi, my name is, um,",
            "You can call me",
            "Lamp, just call me uh",
            "I'm called",
            "I am called",
            "The name is",
            "My name\u{2019}s",
            "Chào Lamp, tên tôi là",
            "Tôi tên là, ờ,",
            "Lamp ơi, gọi em là",
            "Hey Lamp, you can call me",
            "Can you remember? Call me",
        ] {
            assert!(opens_a_name(open), "{open}");
            assert!(!gives_a_name(open), "{open}");
        }
        for not in [
            "Hey Lamp, my name is Jim.",
            "Call me Jim.",
            "Do you know what my name is?",
            "Tell me what my name is.",
            "Can you call me?",
            "Don't call me.",
            "Yes, I am.",
            "I'm",
            "What's my name",
            "Remember my name.",
            "My name is Jim, and I'm",
            "Tên tôi là Lan.",
            "Tên bạn là gì?",
            "Can you call me",
            "Hey Lamp, could you call me.",
            "Please call me",
            "Lamp, will you call me",
            "",
        ] {
            assert!(!opens_a_name(not), "{not}");
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

    /// `LAMP_SECOND_REMEMBER_ASKED`: a plain ask to remember the speaker's
    /// own face or who they are, never a refusal or a question of what the
    /// lamp knows (reviewer-three's six lines on #3298 among them).
    #[test]
    fn an_ask_to_remember_is_their_own_face_and_never_a_question_or_a_no() {
        for asked in [
            "Lamp, remember my face, I'm Kess.",
            "Can you remember my face?",
            "Please remember me.",
            "Remember who I am.",
            "Hey Lamp, remember what I look like.",
            "Learn my face.",
            "Could you save my face?",
            "Please remember my name is Jim.",
            "I'm Kess. Remember my face.",
            "Lamp, could you please remember my face?",
            "Will you remember me? I'm Kess.",
        ] {
            assert!(asks_to_remember(asked), "{asked}");
        }
        for not in [
            "Do you remember me?",
            "Did you remember my face?",
            "Lamp, do you remember me?",
            "Have you remembered me?",
            "Don't remember my face.",
            "Never remember me.",
            "You don't remember me.",
            "I don't want you to remember my face, I'm Kess.",
            "Please don't ever remember me, I'm Kess.",
            "You shouldn't remember my face, I'm Kess.",
            "I'd rather you didn't remember me, I'm Kess.",
            "Do you still remember me? I'm Kess.",
            "Do you even remember my face? I'm Kess.",
            "Remember my face? No.",
            // The reviewer-lead's two P2s on #3298, and their neighbours.
            "You cannot remember my face, I'm Kess.",
            "You cant remember my face, I'm Kess.",
            "I didnt ask you to remember me, I'm Kess.",
            "She said remember my face, I'm Kess.",
            "He told me to remember my name.",
            "Help me remember my name tag.",
            "Remember me?",
            // An ask said another way is the model's, as before.
            "I want you to remember my face, I'm Kess.",
            "Keep my face, I'm Kess.",
            "Remember her face.",
            "Remember my friend.",
            "Remember to turn the light off.",
            "I remembered my keys.",
            // English only, as `gives_a_name` is.
            "Nhớ mặt tôi nhé.",
        ] {
            assert!(!asks_to_remember(not), "{not}");
        }
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
        // Reviewer-lead's P3 on #2275: a name of two words spelled as one
        // run counts, said or spelled, and a part of it spelled doesn't.
        assert!(said_by_them("Mary Ann", "M A R Y A N N."));
        assert!(spelled_by_them("Mary Ann", "It's M, A, R, Y, A, N, N."));
        assert!(spelled_by_them("Mary Ann", "M A R Y, A N N."));
        assert!(!spelled_by_them("Mary Ann", "Mary Ann."));
        assert!(!said_by_them("Mary Ann", "M A R Y."));
        assert!(!spelled_by_them("Mary Ann", "M A R Y A N."));
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

    /// lamp-29e7 (29 Oct): the owner kept at setup is named by default only
    /// while the eyes still know them, asked each time; no eyes is no.
    #[test]
    fn the_owner_is_named_only_while_the_eyes_still_know_them() {
        let (socket, asked) = eyes(vec![
            serde_json::json!({"people": ["Robin", "Example Name"]}),
            serde_json::json!({"people": ["Robin"]}),
        ]);
        assert!(still_known(Some(&socket), "Example Name"));
        // Forgotten since: not named, at the very next ask.
        assert!(!still_known(Some(&socket), "Example Name"));
        assert_eq!(
            asked.join().unwrap(),
            vec![serde_json::json!({"list": true}); 2]
        );
        assert!(!still_known(None, "Example Name"));
    }

    /// Reviewer-two's P3 on #4290: the owner is refreshed off the reply's
    /// path, from the onboarding file's mark and the eyes' list.
    #[test]
    fn the_owner_is_kept_fresh_off_the_replys_path() {
        let (socket, _asked) = eyes(vec![
            serde_json::json!({"people": ["Example Name"]}),
            serde_json::json!({"people": ["Example Name"]}),
        ]);
        let file = socket.with_file_name("progress.json");
        std::fs::write(&file, r#"{"steps":{},"owner":"Example Name"}"#).unwrap();
        // Other tests' forgets share the count: a refresh they cut is tried again.
        for _ in 0..2 {
            refresh_owner(Some(&file), Some(&socket));
            if owner_now().is_some() {
                break;
            }
        }
        assert_eq!(owner_now().as_deref(), Some("Example Name"));
        // Forgotten: named no more at once, not at the next refresh.
        owner_forgotten();
        assert_eq!(owner_now(), None);
    }

    /// The Reviewer-lead's and Reviewer-two's P2 on #4290: an owner the
    /// eyes no longer know, removed in the app too, loses the mark for good;
    /// the eyes not answering keeps it; a name in another case is theirs.
    #[test]
    fn an_owner_the_eyes_no_longer_know_loses_the_mark_however_forgotten() {
        let (socket, _asked) = eyes(vec![
            serde_json::json!({"people": ["example name"]}),
            serde_json::json!({"people": ["Robin"]}),
        ]);
        let file = socket.with_file_name("progress.json");
        std::fs::write(&file, r#"{"steps":{},"owner":"Example Name"}"#).unwrap();
        let mailbox = crate::onboarding::mailbox_of(&file);
        let told = || std::fs::read_to_string(&mailbox).unwrap_or_default();
        refresh_owner(Some(&file), Some(&socket));
        assert_eq!(told(), "");
        refresh_owner(Some(&file), Some(&socket));
        assert_eq!(
            told(),
            "{\"face_forgotten\":true,\"owner\":\"Example Name\"}\n"
        );
        // No eyes to ask: nothing is told.
        std::fs::remove_file(&mailbox).unwrap();
        refresh_owner(Some(&file), Some(&socket.with_file_name("none.sock")));
        assert_eq!(told(), "");
    }

    /// An onboarding file whose setup owner is `owner`, beside a fresh
    /// folder, and its mailbox read.
    fn owner_file(owner: &str) -> (PathBuf, impl Fn() -> String) {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "lamp-voice-owner-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("progress.json");
        std::fs::write(
            &file,
            serde_json::json!({"steps": {}, "owner": owner}).to_string(),
        )
        .unwrap();
        let mailbox = crate::onboarding::mailbox_of(&file);
        (file, move || {
            std::fs::read_to_string(&mailbox).unwrap_or_default()
        })
    }

    fn rename_to(name: &str, replace: bool) -> ToolCall {
        ToolCall {
            id: "c".into(),
            name: REMEMBER.into(),
            args: serde_json::json!({"name": name, "replace": replace}),
        }
    }

    /// The eyes' raw answer `raw` to a rename of `owner`'s file, through
    /// the voice as `tools` hands it on: told as the model is, `replaced`
    /// beside it. What the mailbox then says.
    fn owner_after(owner: &str, raw: &Value) -> String {
        let (file, told) = owner_file(owner);
        let renaming = OwnerRename::begin(&rename_to("Rosa", true), Some(&file)).unwrap();
        assert_eq!(renames_of(&file).0, 1);
        renaming.answered(&told_asking(raw, false), replaced_of(raw).as_deref());
        assert_eq!(renames_of(&file), (0, 1), "the rename ended");
        told()
    }

    /// A tester's lamp, 3 Nov: the setup owner's face saved under a misheard
    /// name A, renamed to name B, moves the owner to B when the eyes say the
    /// face they replaced was A's, and a refresh while the rename is in
    /// flight drops nothing; a remembering without `replace` begins none.
    #[test]
    fn a_rename_of_the_owners_face_moves_the_owner_and_no_refresh_drops_them() {
        let (socket, asked) = eyes(vec![
            // The refresh in flight: the eyes know B already.
            serde_json::json!({"people": ["Name B"]}),
        ]);
        let file = socket.with_file_name("progress.json");
        std::fs::write(&file, r#"{"steps":{},"owner":"Name A"}"#).unwrap();
        let mailbox = crate::onboarding::mailbox_of(&file);
        let told = || std::fs::read_to_string(&mailbox).unwrap_or_default();
        assert!(OwnerRename::begin(&rename_to("Name B", false), Some(&file)).is_none());
        assert!(OwnerRename::begin(&rename_to("Name B", true), None).is_none());
        let renaming = OwnerRename::begin(&rename_to("Name B", true), Some(&file)).unwrap();
        refresh_owner(Some(&file), Some(&socket));
        assert_eq!(told(), "", "no drop while the rename is in flight");
        let raw = serde_json::json!({"remembered": "Name B", "replaced": ["Name A"]});
        renaming.answered(&told_asking(&raw, false), replaced_of(&raw).as_deref());
        assert_eq!(renames_of(&file).0, 0);
        assert_eq!(
            told(),
            "{\"owner_renamed\":{\"from\":\"Name A\",\"to\":\"Name B\"}}\n"
        );
        // Moved on the answer itself: the eyes' list is asked only by the
        // refresh.
        assert_eq!(
            asked.join().unwrap(),
            vec![serde_json::json!({"list": true})]
        );
        // A failed remembering ends the rename at once, moving nothing.
        std::fs::remove_file(&mailbox).unwrap();
        let failed = OwnerRename::begin(&rename_to("Name B", true), Some(&file)).unwrap();
        failed.answered(&serde_json::json!({"error": "busy"}), None);
        assert_eq!(renames_of(&file).0, 0);
        assert_eq!(told(), "");
    }

    /// The owner the voice names follows the owner's own rename at once,
    /// and only while it is still them. Tried on a value of its own:
    /// [`OWNER_NOW`] is shared with the other owner tests.
    #[test]
    fn the_owner_named_follows_their_own_rename() {
        let mut now = Some("original owner".to_owned());
        renamed_in(&mut now, "Original Owner", "Rosa");
        assert_eq!(now.as_deref(), Some("Rosa"));
        renamed_in(&mut now, "Original Owner", "Ines");
        assert_eq!(now.as_deref(), Some("Rosa"), "someone else's name stays");
        let mut none = None;
        renamed_in(&mut none, "Original Owner", "Rosa");
        assert_eq!(none, None, "a forgotten owner is not brought back");
    }

    /// Reviewer-three's P1 on #5149: the app removed the owner's face, and
    /// before the next refresh a visitor renamed their own face to the
    /// name the eyes now know. Before, a list read after the save that knew
    /// the new name and not the owner's moved the owner, and their
    /// authority, to the visitor. Now the eyes say whom they replaced: the
    /// visitor's old name, so nothing moves and the owner stays.
    #[test]
    fn a_visitors_rename_after_the_owners_removal_moves_no_owner() {
        let (file, told) = owner_file("Original Owner");
        let renaming =
            OwnerRename::begin(&rename_to("Visitor Renamed", true), Some(&file)).unwrap();
        let raw = serde_json::json!({"remembered": "Visitor Renamed", "replaced": ["Visitor"]});
        renaming.answered(&told_asking(&raw, false), replaced_of(&raw).as_deref());
        assert_eq!(told(), "", "no owner_renamed");
        assert_eq!(
            crate::onboarding::owner_of(&file).as_deref(),
            Some("Original Owner")
        );
        assert_eq!(renames_of(&file), (0, 1));
    }

    /// Reviewer-three's condition (a) on #5149: only `replaced` naming
    /// exactly the owner, any case, moves them; missing (older eyes), not a
    /// list, a list with anything but names, empty, two people, or someone
    /// else moves nothing.
    #[test]
    fn only_the_owner_replaced_alone_moves_the_owner() {
        let moved = "{\"owner_renamed\":{\"from\":\"Original Owner\",\"to\":\"Rosa\"}}\n";
        for replaced in [
            serde_json::json!(["Original Owner"]),
            serde_json::json!(["original owner"]),
        ] {
            let raw = serde_json::json!({"remembered": "Rosa", "replaced": replaced});
            assert_eq!(owner_after("Original Owner", &raw), moved, "{raw}");
        }
        for raw in [
            serde_json::json!({"remembered": "Rosa"}),
            serde_json::json!({"remembered": "Rosa", "replaced": null}),
            serde_json::json!({"remembered": "Rosa", "replaced": "Original Owner"}),
            serde_json::json!({"remembered": "Rosa", "replaced": {"name": "Original Owner"}}),
            serde_json::json!({"remembered": "Rosa", "replaced": [1]}),
            serde_json::json!({"remembered": "Rosa", "replaced": ["Original Owner", 2]}),
            serde_json::json!({"remembered": "Rosa", "replaced": [["Original Owner"]]}),
            serde_json::json!({"remembered": "Rosa", "replaced": []}),
            serde_json::json!({"remembered": "Rosa", "replaced": ["Original Owner", "Ines"]}),
            serde_json::json!({"remembered": "Rosa", "replaced": ["Original Owner", "original owner"]}),
            serde_json::json!({"remembered": "Rosa", "replaced": ["Ines"]}),
            serde_json::json!({"remembered": "Rosa", "replaced": ["Original"]}),
            serde_json::json!({"error": "known_as", "known_as": "Ines", "replaced": ["Original Owner"]}),
        ] {
            assert_eq!(owner_after("Original Owner", &raw), "", "{raw}");
        }
    }

    /// Reviewer-three's condition (b) on #5149: the owner's face removed in
    /// the app, then a visitor's rename the eyes refuse: the rename ends on
    /// the refusal, so the next refresh drops the owner the eyes no longer
    /// know, as it would with no rename at all.
    #[test]
    fn a_refused_rename_after_the_owners_removal_leaves_the_refresh_to_drop_them() {
        let (socket, asked) = eyes(vec![serde_json::json!({"people": ["Visitor"]})]);
        let file = socket.with_file_name("progress.json");
        std::fs::write(&file, r#"{"steps":{},"owner":"Original Owner"}"#).unwrap();
        let mailbox = crate::onboarding::mailbox_of(&file);
        let renaming = OwnerRename::begin(&rename_to("Rosa", true), Some(&file)).unwrap();
        let raw = serde_json::json!({"error": "not_like_them"});
        renaming.answered(&told_asking(&raw, false), replaced_of(&raw).as_deref());
        assert_eq!(renames_of(&file).0, 0, "nothing left in flight");
        refresh_owner(Some(&file), Some(&socket));
        assert_eq!(
            std::fs::read_to_string(&mailbox).unwrap_or_default(),
            "{\"face_forgotten\":true,\"owner\":\"Original Owner\"}\n"
        );
        assert_eq!(
            asked.join().unwrap(),
            vec![serde_json::json!({"list": true})]
        );
    }

    /// Reviewer-three's condition (c) on #5149: whom the eyes replaced
    /// reaches the rename beside the answer, never what the model is told.
    #[test]
    fn whom_the_eyes_replaced_is_never_told_to_the_model() {
        let raw = serde_json::json!({"remembered": "Rosa", "replaced": ["Original Owner"]});
        let (socket, asked) = eyes(vec![raw.clone(), raw.clone()]);
        let mut replaced = None;
        let said = Said::default();
        let switches = FaceSwitches {
            checked: false,
            visits: true,
            remember_speaker: false,
            at_setup: false,
            record: false,
        };
        let (told, _) = run_switched(
            &socket,
            &rename_to("Rosa", true),
            switches,
            &said,
            None,
            &mut replaced,
        );
        assert_eq!(replaced, Some(vec!["Original Owner".to_owned()]));
        assert!(told.get("replaced").is_none(), "{told}");
        assert!(!told.to_string().contains("Original Owner"), "{told}");
        // The public path the tools take gives the same answer, without it.
        let (told, _) = run_scoped(
            &socket,
            &rename_to("Rosa", true),
            switches,
            None,
            None,
            1,
            None,
        );
        assert!(!told.to_string().contains("Original Owner"), "{told}");
        assert_eq!(asked.join().unwrap().len(), 2);
        for visits in [false, true] {
            assert!(told_asking(&raw, visits).get("replaced").is_none());
        }
    }

    /// A face `width` of the frame wide at `confidence`, square in a 4:3
    /// frame, as the eyes report it.
    fn desk_face(id: &str, width: f32, confidence: f32) -> lamp_msgs::Face {
        lamp_msgs::Face {
            id: id.to_owned(),
            xmin: 0.3,
            ymin: 0.2,
            xmax: 0.3 + width,
            ymax: 0.2 + width * 4.0 / 3.0,
            confidence,
            yaw: None,
            facing: None,
            speaking: None,
            follows: None,
        }
    }

    fn desk_look(faces: &[(f32, f32)]) -> lamp_msgs::Faces {
        lamp_msgs::Faces {
            faces: faces
                .iter()
                .enumerate()
                .map(|(k, (width, confidence))| desk_face(&format!("f{k}"), *width, *confidence))
                .collect(),
        }
    }

    /// The a0ae name-forget report (2 Nov), the first builder's shape: the owner 0.09 to 0.19 of the
    /// frame wide at 0.80 to 0.92, beside faces 0.02 to 0.06 wide at 0.61
    /// to 0.78 that come and go, is one person in every look; a second
    /// person at the desk beside them is still two.
    #[test]
    fn only_faces_at_the_desk_are_people_at_the_lamp() {
        let count = |faces: &[(f32, f32)]| count_at_desk(&desk_look(faces), true);
        // The live sample's looks, and the ranges' other ends.
        let a0ae: [&[(f32, f32)]; 7] = [
            &[(0.15, 0.88)],
            &[(0.15, 0.88), (0.06, 0.67), (0.02, 0.61)],
            &[(0.13, 0.80), (0.06, 0.67)],
            &[(0.12, 0.87), (0.04, 0.78), (0.02, 0.61)],
            &[(0.19, 0.92), (0.06, 0.78), (0.03, 0.70)],
            &[(0.09, 0.80), (0.04, 0.78)],
            &[(0.09, 0.80), (0.06, 0.61), (0.02, 0.61)],
        ];
        for look in a0ae {
            assert_eq!(count(look), 1, "{look:?}");
        }
        // Churning through 3 s of looks: one person, so the owner is named.
        let at = std::time::Instant::now();
        let mut lately = FacesLately::default();
        for (k, look) in a0ae.iter().cycle().take(12).enumerate() {
            let faces = desk_look(look);
            lately.look_told(
                at + Duration::from_millis(250 * k as u64),
                ("b".into(), k as u64),
                count_at_desk(&faces, true),
                faces_told(&faces, true),
            );
        }
        let now = at + Duration::from_millis(2_900);
        assert_eq!(lately.most(now), Some(1));
        let owner = vec!["Lan".to_owned()];
        assert_eq!(
            crate::person::someone_else_now(
                &owner,
                lately.most(now),
                Some("Lan"),
                false,
                false,
                None
            ),
            None,
            "named by default beside the far faces"
        );
        // Off, as before: the same looks are two people, and the name held.
        let mut before = FacesLately::default();
        for (k, look) in a0ae.iter().cycle().take(12).enumerate() {
            before.seen(
                at + Duration::from_millis(250 * k as u64),
                count_at_desk(&desk_look(look), false),
            );
        }
        assert_eq!(before.most(now), Some(3));
        assert_eq!(
            crate::person::someone_else_now(
                &owner,
                before.most(now),
                Some("Lan"),
                false,
                false,
                None
            ),
            Some("more than one face in view")
        );
        // A guest far behind the owner who is the only voice: their face,
        // not counted, read speaking holds the name back (the
        // Reviewer-lead's P2 on #4987); the owner speaking, or a far face
        // read still, does not. Off, nothing is dropped, so never.
        let speaking = |look: &[(f32, f32)], who: usize, state: lamp_msgs::SpeakingState| {
            let mut faces = desk_look(look);
            faces.faces[who].speaking = Some(lamp_msgs::Speaking {
                state,
                score: 0.05,
                from: None,
            });
            faces
        };
        let guest = speaking(
            &[(0.15, 0.88), (0.07, 0.80)],
            1,
            lamp_msgs::SpeakingState::Yes,
        );
        assert_eq!(count_at_desk(&guest, true), 1);
        assert!(speaking_not_at_desk(&guest, true));
        assert!(!speaking_not_at_desk(&guest, false));
        let owner_speaking = speaking(
            &[(0.15, 0.88), (0.07, 0.80)],
            0,
            lamp_msgs::SpeakingState::Yes,
        );
        assert!(!speaking_not_at_desk(&owner_speaking, true));
        let guest_still = speaking(
            &[(0.15, 0.88), (0.07, 0.80)],
            1,
            lamp_msgs::SpeakingState::No,
        );
        assert!(!speaking_not_at_desk(&guest_still, true));
        let mut heard = FacesLately::default();
        assert!(!heard.spoke_not_at_desk_since(at));
        heard.spoke_not_at_desk(at);
        heard.look(at, ("b".into(), 1), count_at_desk(&guest, true));
        let reply = at + Duration::from_secs(4);
        assert_eq!(heard.most(reply), None, "past the count's 3 s");
        assert!(heard.spoke_not_at_desk_since(reply));
        assert_eq!(
            crate::person::someone_else_now(
                &owner,
                Some(1),
                Some("Lan"),
                false,
                heard.spoke_not_at_desk_since(reply),
                None
            ),
            Some("a face not at the desk seen speaking")
        );
        assert!(
            !heard
                .spoke_not_at_desk_since(at + SPOKE_NOT_AT_DESK_WITHIN + Duration::from_millis(1))
        );
        // Two people side by side at the desk: two, and the name held.
        assert_eq!(count(&[(0.14, 0.85), (0.12, 0.82)]), 2);
        assert_eq!(count(&[(0.14, 0.85), (0.12, 0.82), (0.03, 0.66)]), 2);
        // Someone well behind the owner, sure it is a face: not at the desk.
        assert_eq!(count(&[(0.15, 0.90), (0.06, 0.80)]), 1);
        // A near face the eyes are unsure of: not counted beside the owner;
        // alone, a look with a face never counts nobody.
        assert_eq!(count(&[(0.15, 0.88), (0.13, 0.65)]), 1);
        assert_eq!(count(&[(0.13, 0.65)]), 1);
        // The owner at an edge, 0.61 sure, alone: still one.
        assert_eq!(count(&[(0.093, 0.61)]), 1);
        // A large unsure misdetection never pushes the owner out.
        assert_eq!(count(&[(0.40, 0.60), (0.12, 0.85)]), 1);
        // No face, no person.
        assert_eq!(count(&[]), 0);
        // lamp-06c1's recording (3 Nov), by height: a same-size face beside
        // the owner (0.93 of their height, the weaker 0.78 sure) is two.
        let by_height = |faces: &[(f32, f32)]| {
            let look = lamp_msgs::Faces {
                faces: faces
                    .iter()
                    .enumerate()
                    .map(|(k, (height, confidence))| lamp_msgs::Face {
                        ymax: 0.1 + height,
                        ymin: 0.1,
                        ..desk_face(&format!("f{k}"), 0.1, *confidence)
                    })
                    .collect(),
            };
            count_at_desk(&look, true)
        };
        assert_eq!(by_height(&[(0.177, 0.90), (0.165, 0.78)]), 2);
        // A face 0.55 of the owner's height at 0.84: not at the desk.
        assert_eq!(by_height(&[(0.177, 0.91), (0.098, 0.84)]), 1);
        // A giant false box (0.85 to 0.97 tall at 0.61 to 0.81, the arm
        // moving) never knocks the owner out: unsure, it is dropped; sure,
        // it is never the reference, so the owner still counts.
        let giant = |confidence: f32| {
            let faces = lamp_msgs::Faces {
                faces: vec![
                    lamp_msgs::Face {
                        xmin: 0.03,
                        xmax: 0.53,
                        ymin: 0.02,
                        ymax: 0.93,
                        ..desk_face("f10", 0.5, confidence)
                    },
                    lamp_msgs::Face {
                        ymin: 0.5,
                        ymax: 0.67,
                        ..desk_face("f14", 0.10, 0.88)
                    },
                ],
            };
            at_desk(&faces.faces, true)
        };
        assert_eq!(giant(0.61), [false, true]);
        assert_eq!(giant(0.81), [true, true]);
        // Off: every face the eyes report, as `faces.len()`.
        for look in a0ae {
            assert_eq!(count_at_desk(&desk_look(look), false), look.len());
        }
        assert_eq!(count_at_desk(&desk_look(&[(0.13, 0.65)]), false), 1);
        // The eyes' list is never changed.
        let faces = desk_look(&[(0.15, 0.88), (0.02, 0.61)]);
        let kept = faces.clone();
        let _ = count_at_desk(&faces, true);
        assert_eq!(faces, kept);
    }

    /// The decision line's sizes: counted faces first, dropped ones after,
    /// sizes and confidence only (no id, no place in the picture).
    #[test]
    fn the_faces_told_are_sizes_only() {
        let faces = desk_look(&[(0.15, 0.88), (0.06, 0.67)]);
        let told = faces_told(&faces, true);
        assert_eq!(told, "counted w0.15 h0.20 c0.88; dropped w0.06 h0.08 c0.67");
        assert!(!told.contains("f0") && !told.contains("f1"), "{told}");
        assert_eq!(
            faces_told(&faces, false),
            "counted w0.15 h0.20 c0.88, w0.06 h0.08 c0.67; dropped none"
        );
        // Told with the switch on, or when a face was dropped; off, the
        // decision line reads as before (the Reviewer-lead's P3 on #4987).
        assert!(faces_worth_telling(&told, true));
        assert!(faces_worth_telling(&told, false));
        let off = faces_told(&faces, false);
        assert!(!faces_worth_telling(&off, false));
        assert!(faces_worth_telling(&off, true));
        assert!(!faces_worth_telling("", true));
        let mut lately = FacesLately::default();
        let at = std::time::Instant::now();
        assert_eq!(lately.most_told(at), "");
        lately.look_told(at, ("b".into(), 1), 1, told.clone());
        assert_eq!(lately.most_told(at), told);
    }

    /// lamp-52e6, 29 Oct 13:03: one look that missed a face among two in
    /// view never makes them one; the owner looking away is still none.
    #[test]
    fn the_most_faces_of_the_last_seconds_count_not_one_look() {
        let at = std::time::Instant::now();
        let mut lately = FacesLately::default();
        assert_eq!(lately.most(at), None);
        lately.seen(at, 2);
        assert_eq!(lately.most(at), Some(2), "one look alone counts");
        lately.seen(at + Duration::from_millis(250), 0);
        lately.seen(at + Duration::from_millis(500), 2);
        assert_eq!(lately.most(at + Duration::from_millis(600)), Some(2));
        // One look read three times is one look: a spurious second face
        // in it alone is still one person.
        let mut read = FacesLately::default();
        read.look(at, ("b".into(), 1), 1);
        for k in 1..4 {
            read.look(at + Duration::from_millis(100 * k), ("b".into(), 2), 2);
        }
        read.look(at + Duration::from_millis(500), ("b".into(), 3), 1);
        assert_eq!(read.most(at + Duration::from_secs(1)), Some(1));
        // One spurious second face among looks of one: one person.
        let mut one = FacesLately::default();
        for (k, faces) in [1, 1, 2, 1].into_iter().enumerate() {
            one.seen(at + Duration::from_millis(250 * k as u64), faces);
        }
        assert_eq!(one.most(at + Duration::from_secs(1)), Some(1));
        // Past the window: the newest look alone.
        lately.seen(at + Duration::from_secs(4), 0);
        assert_eq!(lately.most(at + Duration::from_secs(4)), Some(0));
        assert_eq!(lately.most(at + Duration::from_secs(8)), None);
    }

    /// lamp-29e7, 29 Oct (#4118): eyes reached but too busy to answer in
    /// time take nothing back; eyes nobody can reach (stopped, crashed), or
    /// no eyes at all, name no owner (the Reviewer-lead's P2 on #4325).
    #[test]
    fn the_owner_stands_while_the_eyes_are_slow_and_goes_while_they_are_down() {
        let dir = std::env::temp_dir().join(format!("owner-unanswered-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("progress.json");
        std::fs::write(&file, r#"{"steps":{},"owner":"Example Name"}"#).unwrap();
        // Slow: a listener that takes the ask and never answers.
        let slow = dir.join("slow.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&slow).unwrap();
        // Other tests' forgets share the count: a refresh they cut is tried again.
        for _ in 0..3 {
            refresh_owner(Some(&file), Some(&slow));
            if owner_now().is_some() {
                break;
            }
        }
        assert_eq!(owner_now().as_deref(), Some("Example Name"));
        assert!(
            !crate::onboarding::mailbox_of(&file).exists(),
            "nothing is forgotten"
        );
        // Down: a socket nobody listens on, then no eyes at all.
        for socket in [Some(dir.join("down.sock")), None] {
            for _ in 0..3 {
                refresh_owner(Some(&file), socket.as_deref());
                if owner_now().is_none() {
                    break;
                }
            }
            assert_eq!(owner_now(), None, "{socket:?}");
            assert!(
                !crate::onboarding::mailbox_of(&file).exists(),
                "the mark is kept"
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// lamp-52e6, 29 Oct 16:49 (#4118): a known file rewritten every 2–3 s
    /// still names its person at 5 s, as `known_now`'s 3 s did not; one
    /// past [`KNOWN_LATELY`], or none, names nobody.
    #[test]
    fn the_known_file_counts_for_the_by_default_rule_a_few_seconds_old() {
        let dir = std::env::temp_dir().join(format!("known-lately-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("known");
        assert_eq!(known_lately(&path), None);
        std::fs::write(&path, "Example Name\n").unwrap();
        let aged = |seconds| {
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(SystemTime::now() - Duration::from_secs(seconds))
                .unwrap();
        };
        aged(5);
        assert_eq!(known_now(&path), None, "the 3 s read misses it");
        assert_eq!(known_lately(&path).as_deref(), Some("Example Name"));
        aged(KNOWN_LATELY.as_secs() + 2);
        assert_eq!(known_lately(&path), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The eyes stand-in on a real socket: what each request sent, and the
    /// answer it gives to each.
    fn eyes(answers: Vec<Value>) -> (PathBuf, std::thread::JoinHandle<Vec<Value>>) {
        use std::io::{BufRead, BufReader, Write};
        // One folder per call (#3792's P3: two tests with as many answers
        // shared a socket).
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "lamp-voice-names-{}-{}-{}",
            std::process::id(),
            answers.len(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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

    /// #5140 row 5 (lamp-0c4e, 3 Nov): at setup the first name heard was
    /// kept with the face without being said back, and it was misheard.
    /// Before the first save at setup the name is said back and a plain yes
    /// heard; a no, or a name corrected, is said back again; a name they
    /// spelled is their own and goes as it is. The usual walk, the name said
    /// back and agreed, then the face asked and agreed, goes in one call; a
    /// yes to the face alone, or after a correction, is no yes to the name.
    /// Past setup, as before.
    #[test]
    fn at_setup_the_first_name_is_said_back_for_a_yes_before_it_is_kept() {
        const FACE: &str = "May I remember your face, so I know you next time?";
        let call = |name: &str| ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({ "name": name }),
        };
        let (socket, asked) = eyes(vec![
            json!({"remembered": "Lauren"}),
            json!({"remembered": "Loren"}),
            json!({"remembered": "Marie"}),
            json!({"remembered": "Lauren"}),
        ]);
        let (words, answered) = run_at_setup(&socket, &call("Lauren"), &Said::now("I'm Lauren."));
        assert!(answered, "{words}");
        assert!(
            words["error"]
                .as_str()
                .is_some_and(|told| told.contains("say the name back")),
            "{words}"
        );
        let said_back = |this: &str| Said {
            this: this.into(),
            before: vec![("I'm Lauren.".into(), "Lauren, did I get that right?".into())],
        };
        let (words, _) = run_at_setup(&socket, &call("Loren"), &said_back("No, it's Loren."));
        assert!(words["error"].is_string(), "{words}");
        let (words, _) = run_at_setup(&socket, &call("Lauren"), &said_back("Yes."));
        assert_eq!(words, json!({"ok": true, "remembered": "Lauren"}));
        // Spelled with no ask to spell: said back first, as any name (El's
        // P2 on #5159); spelled after the lamp asked: kept with no yes.
        let (words, _) = run_at_setup(&socket, &call("Loren"), &Said::now("It's L O R E N."));
        assert!(
            words["error"]
                .as_str()
                .is_some_and(|told| told.contains("say the name back")),
            "{words}"
        );
        let asked_to_spell = Said {
            this: "It's L O R E N.".into(),
            before: vec![(
                "I'm Lord in.".into(),
                "Could you spell your name for me?".into(),
            )],
        };
        let (words, _) = run_at_setup(&socket, &call("Loren"), &asked_to_spell);
        assert_eq!(words, json!({"ok": true, "remembered": "Loren"}));
        // The usual walk: "Marie, right?" "Yes." then the face's question
        // and its yes: one call, no turn more.
        let walk = Said {
            this: "Yes.".into(),
            before: vec![
                ("I'm Marie.".into(), "Marie, right?".into()),
                ("Yes.".into(), FACE.into()),
            ],
        };
        let (words, _) = run_at_setup(&socket, &call("Marie"), &walk);
        assert_eq!(words, json!({"ok": true, "remembered": "Marie"}));
        // Said back, then corrected: the face's yes is no yes to the name.
        let corrected = Said {
            this: "Yes.".into(),
            before: vec![
                ("I'm Lauren.".into(), "Lauren, right?".into()),
                ("No, it's Loren.".into(), FACE.into()),
            ],
        };
        let (words, _) = run_at_setup(&socket, &call("Lauren"), &corrected);
        assert!(words["error"].is_string(), "{words}");
        // Never said back: the face's yes alone is no yes to the name.
        let unsaid = Said {
            this: "Yes.".into(),
            before: vec![("I'm Marie.".into(), FACE.into())],
        };
        let (words, _) = run_at_setup(&socket, &call("Marie"), &unsaid);
        assert!(words["error"].is_string(), "{words}");
        // Past setup, a name they said goes at once, as before.
        let (words, _) = run_with(&socket, &call("Lauren"), true, &Said::now("I'm Lauren."));
        assert_eq!(words, json!({"ok": true, "remembered": "Lauren"}));
        assert_eq!(
            asked.join().unwrap(),
            [
                json!({"remember": "Lauren", "one_face": true, "replace": false}),
                json!({"remember": "Loren", "one_face": true, "replace": false}),
                json!({"remember": "Marie", "one_face": true, "replace": false}),
                json!({"remember": "Lauren", "one_face": true, "replace": false}),
            ]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// #5140 row 6 (lamp-0c4e, 3 Nov): a misheard name never passed the
    /// names check. At setup a name not heard is asked for spelled out, and
    /// the letters they spell are their own words: the face is kept under
    /// the name as spelled, not the model's guess. Letters that disagree
    /// with the count they gave are not; past setup, as before.
    #[test]
    fn at_setup_a_misheard_name_is_asked_spelled_and_kept_as_spelled() {
        let call = |name: &str| ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({ "name": name }),
        };
        let (socket, asked) = eyes(vec![json!({"remembered": "Loren"})]);
        let (words, _) = run_at_setup(&socket, &call("Lauren"), &Said::now("I'm Lord in."));
        assert!(
            words["error"]
                .as_str()
                .is_some_and(|told| told.contains("spell")),
            "{words}"
        );
        let spelled = |this: &str| Said {
            this: this.into(),
            before: vec![(
                "I'm Lord in.".into(),
                "Sorry, could you spell your name for me?".into(),
            )],
        };
        let (words, _) = run_at_setup(
            &socket,
            &call("Lauren"),
            &spelled("Four letters. L O R E N."),
        );
        assert!(words["error"].is_string(), "{words}");
        let (words, _) = run_at_setup(&socket, &call("Lauren"), &spelled("L, O, R, E, N."));
        assert_eq!(words, json!({"ok": true, "remembered": "Loren"}));
        // Past setup, a name not heard is refused, as before.
        let (words, _) = run_with(&socket, &call("Lauren"), true, &spelled("L, O, R, E, N."));
        assert!(words["error"].is_string(), "{words}");
        assert_eq!(
            asked.join().unwrap(),
            [json!({"remember": "Loren", "one_face": true, "replace": false})]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// Stable 10226, lamp-52e6, 5 Nov 11:28 (the core-verifier's R1, and the
    /// walk's face step the same way): the name said back alone and
    /// "Yes. That's right. Please remember my face." kept no face, the yes
    /// going past a plain yes's words, so the walk ended with the name only.
    /// A yes that carries the save's own ask is a yes to the read-back; a
    /// correction, a spelling, or a read-back folded into the face question
    /// (El's #5159 rule) still keeps nothing.
    #[test]
    fn at_setup_a_yes_that_asks_for_the_save_keeps_the_face() {
        let call = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({ "name": "Henry" }),
        };
        const HEARD: &str = "Hey, Lamp. My name is Henry. Please remember my face.";
        const READ_BACK: &str =
            "I'll take a look at you, Henry, so I know you next time. Henry, did I get that right?";
        let after = |lamp: &str, this: &str| Said {
            this: this.into(),
            before: vec![(HEARD.into(), lamp.into())],
        };
        let (socket, asked) = eyes(vec![json!({"remembered": "Henry"})]);
        // The first call, the name and the ask in one line: said back first.
        let (words, _) = run_at_setup(&socket, &call, &Said::now(HEARD));
        assert!(
            words["error"]
                .as_str()
                .is_some_and(|told| told.contains("say the name back")),
            "{words}"
        );
        for (lamp, this) in [
            (READ_BACK, "No, it's Harry. Please remember my face."),
            (READ_BACK, "Yes, but it's spelled H E N R Y E."),
            (READ_BACK, "Remember my face."),
            (
                "Nice to meet you, Henry! May I remember your face?",
                "Yes. That's right. Please remember my face.",
            ),
        ] {
            let (words, _) = run_at_setup(&socket, &call, &after(lamp, this));
            assert!(words["error"].is_string(), "{this}: {words}");
        }
        let (words, _) = run_at_setup(
            &socket,
            &call,
            &after(READ_BACK, "Yes. That's right. Please remember my face."),
        );
        assert_eq!(words, json!({"ok": true, "remembered": "Henry"}));
        assert_eq!(
            asked.join().unwrap(),
            [json!({"remember": "Henry", "one_face": true, "replace": false})]
        );
        // A rename's yes keeps the strict words: the save's ask isn't one.
        assert!(!agreed_to(
            "Henry",
            &after(READ_BACK, "Yes. That's right. Please remember my face.")
        ));
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// Fifth's cause on #5415 (5 Nov): the walk's own "who you are" script
    /// asked the face question with the name folded in and no read-back,
    /// so every setup save was refused for the read-back. Built from the
    /// script's own lines (`skills/onboarding/skill.toml`): it now says the
    /// name back on its own first, and its yes, then the face question and
    /// its yes, reach the eyes in one call. The folded line of Fifth's run
    /// A is still refused.
    #[test]
    fn the_setup_scripts_own_lines_reach_the_eyes_in_one_call() {
        const SCRIPT: &str = include_str!("../../../skills/onboarding/skill.toml");
        let who = SCRIPT
            .split("[onboarding.steps.who_you_are]")
            .nth(1)
            .and_then(|step| step.lines().find(|line| line.starts_with("first = ")))
            .expect("who_you_are's first note");
        let quoted: Vec<&str> = who.split("\\\"").skip(1).step_by(2).collect();
        assert_eq!(quoted[0], "What should I call you?", "{who}");
        let face = quoted[1];
        assert!(face.contains("face"), "{face}");
        let back = who
            .find("say their name back as a question on its own")
            .expect("the script says the name back");
        assert!(back < who.find(face).unwrap(), "read-back before the face");
        let call = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({ "name": "Walter" }),
        };
        let (socket, asked) = eyes(vec![json!({"remembered": "Walter"})]);
        // As the script now goes: the name, its read-back and yes, then the
        // face question and its yes, the call on that yes.
        let walk = Said {
            this: "Yes, please.".into(),
            before: vec![
                (
                    "My name is Walter.".into(),
                    "Walter, did I get that right?".into(),
                ),
                ("Yes.".into(), face.into()),
            ],
        };
        let (words, _) = run_at_setup(&socket, &call, &walk);
        assert_eq!(words, json!({"ok": true, "remembered": "Walter"}));
        // Fifth's run A, as the old script went: refused for the read-back.
        let folded = Said {
            this: "Yes, please.".into(),
            before: vec![(
                "My name is Walter.".into(),
                format!("Nice to meet you, Walter. {face}"),
            )],
        };
        let (words, _) = run_at_setup(&socket, &call, &folded);
        assert!(refused_for_the_read_back(&words), "{words}");
        assert_eq!(asked.join().unwrap().len(), 1, "only the walk's call");
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// lamp-52e6, 5 Nov, #5391's check: "My name is Walter." with a call
    /// refused for the read-back, "Walter. Have I got that right?", and
    /// "Yes, that's my name." with no call kept no face. The held save's
    /// verdict on the yes runs it, through the same check as a call; a
    /// correction or a read-back folded into the face question drops it.
    #[test]
    fn at_setup_a_held_save_runs_on_the_yes_to_the_read_back() {
        use HeldSaveVerdict::{Drops, Runs, Waits};
        let call = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({ "name": "Walter" }),
        };
        const HEARD: &str = "My name is Walter.";
        const READ_BACK: &str = "Walter. Have I got that right?";
        let after = |lamp: &str, this: &str| Said {
            this: this.into(),
            before: vec![(HEARD.into(), lamp.into())],
        };
        let (socket, asked) = eyes(vec![
            json!({"remembered": "Walter"}),
            json!({"remembered": "Walter"}),
        ]);
        // (a) The call on the name line is refused for the read-back.
        let (words, _) = run_at_setup(&socket, &call, &Said::now(HEARD));
        assert!(refused_for_the_read_back(&words), "{words}");
        // The hold is judged only on the turn after the refused call, so
        // the name line itself is never judged.
        for (lamp, this, verdict) in [
            // (a) and (b): a yes, plain or with the save's ask.
            (READ_BACK, "Yes, that's my name.", Runs),
            (
                READ_BACK,
                "Yes. That's right. Please remember my face.",
                Runs,
            ),
            // (c) A correction, a spelling, a no.
            (READ_BACK, "No, it's Walt.", Drops),
            (READ_BACK, "W A L T.", Drops),
            (READ_BACK, "No.", Drops),
            // (e) The read-back folded into the face question (El's #5159).
            (
                "Nice to meet you, Walter! May I remember your face?",
                "Yes.",
                Drops,
            ),
            // Something else asked: it waits.
            ("What would you like me to call the room?", "Hmm.", Waits),
        ] {
            assert_eq!(
                held_save_verdict("Walter", &after(lamp, this)),
                verdict,
                "{lamp} / {this}"
            );
        }
        // The held save run on the yes is the call's own eyes request.
        for this in [
            "Yes, that's my name.",
            "Yes. That's right. Please remember my face.",
        ] {
            let (words, _) = run_at_setup(&socket, &call, &after(READ_BACK, this));
            assert_eq!(words, json!({"ok": true, "remembered": "Walter"}));
        }
        assert_eq!(
            asked.join().unwrap(),
            [
                json!({"remember": "Walter", "one_face": true, "replace": false}),
                json!({"remember": "Walter", "one_face": true, "replace": false}),
            ]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// El's HOLD on #5159: at setup, letters are the name only after the
    /// lamp asked them to spell, and only as many runs as the model's name
    /// has words. "I work at I B M" or "I'm from the U S" said otherwise is
    /// no name, and a read-back folded into the face question is no
    /// read-back: none of these is sent to the eyes.
    #[test]
    fn at_setup_letters_not_asked_for_and_a_read_back_in_the_face_question_keep_nothing() {
        let call = |name: &str| ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({ "name": name }),
        };
        let (socket, asked) = eyes(vec![json!({"remembered": "Loren"})]);
        let after = |lamp: &str, this: &str| Said {
            this: this.into(),
            before: vec![("Hi.".into(), lamp.into())],
        };
        let asked_name = "What should I call you?";
        let asked_spelled = "Sorry, could you spell your name for me?";
        let face_question = "Nice to meet you, Marie! May I remember your face?";
        for (name, said) in [
            // Letters not asked for: no name, with no yes.
            ("Lauren", after(asked_name, "I'm Lord in, I work at I B M.")),
            ("Lauren", after(asked_name, "Yes, I'm from the U S.")),
            ("Ibm", after(asked_name, "I work at I B M.")),
            // Asked for, but more runs than the name has words.
            (
                "Lauren",
                after(asked_spelled, "L O R E N, and I'm at I B M."),
            ),
            // A read-back inside the face question, then a yes.
            (
                "Marie",
                Said {
                    this: "Yes.".into(),
                    before: vec![("I'm Marie.".into(), face_question.into())],
                },
            ),
        ] {
            let (words, _) = run_at_setup(&socket, &call(name), &said);
            assert!(words["error"].is_string(), "{name}: {words}");
        }
        // Asked for, one run for a one-word name: kept as spelled.
        let (words, _) = run_at_setup(
            &socket,
            &call("Lauren"),
            &after(asked_spelled, "L O R E N."),
        );
        assert_eq!(words, json!({"ok": true, "remembered": "Loren"}));
        assert_eq!(
            asked.join().unwrap(),
            [json!({"remember": "Loren", "one_face": true, "replace": false})]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// On lamp-4ace on 24 October the owner's "Lamp, remember my face, and
    /// I'm …" was heard as "Man. Remember my face and do", and their face was
    /// kept as "Man": the word that called the lamp, opening the line, is no
    /// name, and the model is told to ask for theirs. Said alone, or again
    /// as theirs, it is still a name.
    #[test]
    fn the_word_that_called_the_lamp_is_not_their_name() {
        for (name, said) in [
            ("Man", "Man. Remember my face and do"),
            ("Helen", "Helen, remember my face."),
            ("Lynn", "Hey, Lynn. Remember me."),
            ("Louvre", "Louvre, remember my face, please."),
            ("Lamp", "Lamp, remember my face."),
        ] {
            assert!(only_the_call(name, said), "{name}: {said}");
        }
        for (name, said) in [
            ("Helen", "Helen."),
            ("Helen", "Helen, remember my face. I'm Helen."),
            ("Kess", "Kess, remember my face."),
            ("Man", "Lamp, remember my face. My name is Man."),
            ("Mary Ann", "Mary Ann, remember me."),
            ("Lynn", "Remember my face, Lynn."),
            ("Man", "Man"),
        ] {
            assert!(!only_the_call(name, said), "{name}: {said}");
        }
        let call = |name: &str| ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({ "name": name }),
        };
        let (socket, asked) = eyes(vec![
            json!({"remembered": "Kess"}),
            json!({"remembered": "Man"}),
        ]);
        let (words, answered) = run_with(
            &socket,
            &call("Man"),
            true,
            &Said::now("Man. Remember my face and do"),
        );
        assert!(answered && words["error"] == ONLY_THE_CALL, "{words}");
        let (words, _) = run_with(
            &socket,
            &call("Kess"),
            true,
            &Said::now("Lamp, remember my face. I'm Kess."),
        );
        assert_eq!(words, json!({"ok": true, "remembered": "Kess"}));
        // Without names checked, as before.
        let (words, _) = run_with(
            &socket,
            &call("Man"),
            false,
            &Said::now("Man. Remember my face and do"),
        );
        assert_eq!(words, json!({"ok": true, "remembered": "Man"}));
        assert_eq!(
            asked.join().unwrap(),
            [
                json!({"remember": "Kess", "one_face": true, "replace": false}),
                json!({"remember": "Man"}),
            ]
        );
    }

    /// #2176 row 4, lamp-4ace 20 Oct: a person known by their face was
    /// saved again under their name misheard. A near face with a near name
    /// gets a spelling asked, with no known name said back; a yes to a known
    /// name the lamp said never stands in for hearing it (the reviewer's P1
    /// on #2202); the spelled name goes to the eyes; the same name spelled
    /// again goes with `apart`, said again or agreed to without it; a known
    /// name with a face not theirs is asked about before it replaces theirs.
    #[test]
    fn a_near_name_is_spelled_and_no_yes_stands_in_for_it() {
        let call = |args: Value| ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args,
        };
        let (socket, asked) = eyes(vec![
            json!({"error": "near_to", "near_to": "Minh"}),
            json!({"remembered": "Minh"}),
            json!({"remembered": "Linh"}),
            json!({"error": "near_to", "near_to": "Minh"}),
            json!({"error": "near_to", "near_to": "Minh"}),
            json!({"error": "not_like_them"}),
        ]);
        let (words, _) = run_with(
            &socket,
            &call(json!({"name": "Linh"})),
            true,
            &Said::now("Remember my face, I'm Linh."),
        );
        let told = words["error"].as_str().unwrap();
        assert!(
            told.contains("spell") && told.contains("apart true") && !told.contains("Minh"),
            "{told}"
        );
        // "Are you Minh?" "Yes." is not Minh's name heard.
        let (words, _) = run_with(
            &socket,
            &call(json!({"name": "Minh"})),
            true,
            &Said {
                this: "Yes.".into(),
                before: vec![("Remember my face, I'm Linh.".into(), "Are you Minh?".into())],
            },
        );
        assert!(
            words["error"].as_str().unwrap().contains("spell"),
            "{words}"
        );
        let (words, _) = run_with(
            &socket,
            &call(json!({"name": "Minh"})),
            true,
            &Said::now("M I N H."),
        );
        assert_eq!(words, json!({"ok": true, "remembered": "Minh"}));
        let (words, _) = run_with(
            &socket,
            &call(json!({"name": "Linh", "apart": true})),
            true,
            &Said::now("L I N H."),
        );
        assert_eq!(words, json!({"ok": true, "remembered": "Linh"}));
        // Both reviewers' P2 on #2269: `apart` with the name said again, or a
        // yes to it said back, goes without it, and the spelling is asked
        // for again.
        for said in [
            Said::now("Minh."),
            Said {
                this: "Yes.".into(),
                before: vec![("M I N H.".into(), "Minh, is that right?".into())],
            },
        ] {
            let (words, _) = run_with(
                &socket,
                &call(json!({"name": "Minh", "apart": true})),
                true,
                &said,
            );
            assert!(
                words["error"].as_str().unwrap().contains("spell"),
                "{words}"
            );
        }
        let (words, _) = run_with(
            &socket,
            &call(json!({"name": "Minh"})),
            true,
            &Said::now("I'm Minh."),
        );
        let refused = words["error"].as_str().unwrap();
        assert!(
            refused.contains("another name") && !refused.contains("replace"),
            "{refused}"
        );
        assert_eq!(
            asked.join().unwrap(),
            [
                json!({"remember": "Linh", "one_face": true, "replace": false}),
                json!({"remember": "Minh", "one_face": true, "replace": false}),
                json!({"remember": "Linh", "one_face": true, "replace": false, "apart": true}),
                json!({"remember": "Minh", "one_face": true, "replace": false}),
                json!({"remember": "Minh", "one_face": true, "replace": false}),
                json!({"remember": "Minh", "one_face": true, "replace": false}),
            ]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// #3739, a tester's lamp on 27 Oct: one face was kept twice, under its
    /// name and a name misheard far from it. The eyes' refusal for a face
    /// near someone kept under another name asks for a spelling and says no
    /// name (the HOLDs on #3792); a no, or "someone else" alone, sends
    /// nothing; only the name spelled goes `apart` (Third's 18:02 HOLD).
    #[test]
    fn a_new_name_for_a_near_face_says_no_name_and_a_no_sends_nothing() {
        let call = |args: Value| ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args,
        };
        let (socket, asked) = eyes(vec![
            json!({"error": "near_to", "near_to": "Ossa"}),
            json!({"remembered": "Kemal"}),
        ]);
        let (words, _) = run_with(
            &socket,
            &call(json!({"name": "Kemal"})),
            true,
            &Said::now("Remember my face, I'm Kemal."),
        );
        let told = words["error"].as_str().unwrap();
        assert!(told.contains("spell") && !told.contains("Ossa"), "{told}");
        let asked_back = || {
            vec![(
                "Remember my face, I'm Kemal.".to_owned(),
                "Sorry, could you spell Kemal for me?".to_owned(),
            )]
        };
        // A no, or "someone else" alone: nothing reaches the eyes, whatever
        // the model calls.
        for this in ["No.", "No, I'm someone else."] {
            for args in [
                json!({"name": "Kemal", "replace": true}),
                json!({"name": "Kemal", "apart": true}),
            ] {
                let (words, answered) = run_with(
                    &socket,
                    &call(args),
                    true,
                    &Said {
                        this: this.into(),
                        before: asked_back(),
                    },
                );
                assert!(answered && words.get("ok").is_none(), "{this}: {words}");
            }
        }
        // Spelled: kept apart.
        let (words, _) = run_with(
            &socket,
            &call(json!({"name": "Kemal", "apart": true})),
            true,
            &Said::now("K E M A L."),
        );
        assert_eq!(words, json!({"ok": true, "remembered": "Kemal"}));
        assert_eq!(
            asked.join().unwrap(),
            [
                json!({"remember": "Kemal", "one_face": true, "replace": false}),
                json!({"remember": "Kemal", "one_face": true, "replace": false, "apart": true}),
            ]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// #2176 row 3: with the stable id on, a renamed person's memory is the
    /// id the eyes keep beside the new name, their old name's; a file of the
    /// name alone (a person from before, or the stable id off) keys it on
    /// `person_id` of the name, as before; off, an id is never used.
    #[test]
    fn whose_memory_is_the_eyes_id_with_the_stable_id_else_the_names() {
        use crate::remember::person_id;
        let minh = person_id("Minh");
        // Renamed Minh to Sam: Sam's memory is Minh's.
        assert_eq!(person_of("Sam", Some(&minh), true), minh);
        // An old record, no id: unchanged.
        assert_eq!(person_of("Lan", None, true), person_id("Lan"));
        // Off: the name's, whatever the file says.
        assert_eq!(person_of("Sam", Some(&minh), false), person_id("Sam"));
        // Not an id: the name's.
        assert_eq!(person_of("Sam", Some("../x"), true), person_id("Sam"));
        // A name the voice holds resolves to the id it was read with.
        let mut seen = SeenIds::default();
        seen.note("Sam", Some(&minh));
        assert_eq!(seen.person("sam", true), minh);
        assert_eq!(seen.person("Sam", false), person_id("Sam"));
        seen.note("Sam", None);
        assert_eq!(seen.person("Sam", true), person_id("Sam"));
        // The observation itself, read from the eyes' file.
        let path = std::env::temp_dir().join(format!("lamp-voice-known-id-{}", std::process::id()));
        std::fs::write(&path, format!("Sam\n{minh}\n")).unwrap();
        let observation = FaceObservation::read(&path).unwrap();
        assert_eq!(observation.name(), "Sam");
        assert_eq!(
            person_of(observation.name(), observation.id.as_deref(), true),
            minh
        );
        // Off (no LAMP_MEMORY_STABLE_ID in tests): the name's, as before.
        assert_eq!(observation.person(), person_id("Sam"));
        std::fs::remove_file(&path).unwrap();
    }

    /// #5150's P3: with the stable id, a yes to being named is the named
    /// person's own by the id the eyes gave that name, not by the name's
    /// hash. Renamed A to B, B's yes is theirs; a new face later saved as A
    /// (A's hash with `-2`) is not B's to agree for. Off, as before.
    #[test]
    fn a_yes_to_being_named_follows_the_eyes_id_of_the_name() {
        use lamp_msgs::known::person_id;
        let first_a = person_id("A");
        let new_a = format!("{first_a}-2");
        let mut seen = SeenIds::default();
        seen.note("B", Some(&first_a));
        seen.note("A", Some(&new_a));
        // B, renamed from A, keeps A's first id as their scope.
        assert_eq!(whose_yes(Some(&first_a), &seen.person("B", true)), None);
        // B can't agree for the new A.
        assert_eq!(
            whose_yes(Some(&first_a), &seen.person("A", true)),
            Some("the line's scope is someone else's")
        );
        // The new A's own yes is theirs.
        assert_eq!(whose_yes(Some(&new_a), &seen.person("A", true)), None);
        // Off: the name's hash, as before.
        assert_eq!(whose_yes(Some(&first_a), &seen.person("A", false)), None);
        assert_eq!(
            whose_yes(Some(&first_a), &seen.person("B", false)),
            Some("the line's scope is someone else's")
        );
        assert_eq!(
            whose_yes(None, &seen.person("A", true)),
            Some("the line's scope is the room's")
        );
    }

    /// The voice follows the eyes (#2176): when its own switch and the eyes'
    /// id line differ, it says so once, with no name and no id, and uses no
    /// ids; when they agree it says nothing.
    #[test]
    fn a_switch_apart_from_the_eyes_is_said_once_and_uses_no_ids() {
        use crate::remember::person_id;
        use std::sync::atomic::AtomicBool;
        let said = AtomicBool::new(false);
        assert_eq!(disagreement(true, true, &said), None);
        assert_eq!(disagreement(false, false, &said), None);
        let line = disagreement(false, true, &said).unwrap();
        assert!(
            line.starts_with("lamp-voice: ") && line.contains("off here"),
            "{line}"
        );
        assert!(!line["lamp-voice: ".len()..].contains("p-"), "{line}");
        assert_eq!(disagreement(false, true, &said), None, "once");
        assert_eq!(disagreement(true, false, &said), None, "once in all");
        let said = AtomicBool::new(false);
        let line = disagreement(true, false, &said).unwrap();
        assert!(
            line.contains("on here") && line.contains("no ids"),
            "{line}"
        );
        // Apart, no ids: on with no id, or off with one, is the name's.
        let minh = person_id("Minh");
        assert_eq!(person_of("Sam", None, true), person_id("Sam"));
        assert_eq!(person_of("Sam", Some(&minh), false), person_id("Sam"));
    }

    /// #2850 (lamp-c0a4, 22 October): "forget my face" on the second route
    /// sent the model's guess at a name, which the eyes didn't keep, while
    /// their known file named the person whose line it was. The known
    /// person at the lamp is asked for under the eyes' own name, whatever
    /// the model wrote; the model's name only when it is the line's own
    /// person's; never anyone else's; and a room's line as before.
    #[test]
    fn forget_my_face_asks_for_the_known_person_whose_line_it_is() {
        let now = Instant::now();
        let wall = SystemTime::now();
        let alex = FaceObservation::from_source("Alex", wall, wall, now).unwrap();
        let bo = FaceObservation::from_source("Bo", wall, wall, now).unwrap();
        let theirs = crate::remember::person_id("alex");
        let theirs = Some(theirs.as_str());
        // The known person whose line it is: the eyes' name, not the guess.
        for asked in [Some("Alexander"), Some("me"), Some(""), None, Some("Bo")] {
            assert_eq!(
                forget_name(asked, Some(&alex), theirs, None).as_deref(),
                Some("Alex"),
                "{asked:?}"
            );
        }
        // Nobody known now, or someone else: the model's name only when it
        // is theirs.
        for known in [None, Some(&bo)] {
            assert_eq!(
                forget_name(Some(" alex "), known, theirs, None).as_deref(),
                Some("alex")
            );
            assert_eq!(forget_name(Some("Bo"), known, theirs, None), None);
            assert_eq!(forget_name(None, known, theirs, None), None);
        }
        // A room's line: only the named person whose own face, and no
        // other, was seen saying it, under the eyes' name.
        use crate::speaking_faces::LineSeen;
        let seen = |speakers: &[&str], known: Option<(&str, &str)>| LineSeen {
            request: 1,
            speakers: speakers.iter().map(|id| (*id).to_owned()).collect(),
            known: known.map(|(name, face)| (name.to_owned(), face.to_owned())),
            enrollment: None,
        };
        let theirs_said = seen(&["f1"], Some(("Alex", "f1")));
        for known in [Some(&alex), Some(&bo), None] {
            assert_eq!(
                forget_name(Some(" alex "), known, None, Some(&theirs_said)).as_deref(),
                Some("Alex")
            );
        }
        // El's P2 on #3018: Alex sits nearest while someone else says it, or
        // both speak, or nobody was seen speaking: nobody.
        for line in [
            seen(&["f2"], Some(("Alex", "f1"))),
            seen(&["f1", "f2"], Some(("Alex", "f1"))),
            seen(&[], Some(("Alex", "f1"))),
        ] {
            assert_eq!(
                forget_name(Some("Alex"), Some(&alex), None, Some(&line)),
                None,
                "{line:?}"
            );
        }
        // No reading of the line, a known file stale when it was heard
        // (`known_with_face` gives none), someone else known, or no name.
        assert_eq!(forget_name(Some("Alex"), Some(&alex), None, None), None);
        let stale = seen(&["f1"], None);
        assert_eq!(
            forget_name(Some("Alex"), Some(&alex), None, Some(&stale)),
            None
        );
        let bo_said = seen(&["f1"], Some(("Bo", "f1")));
        assert_eq!(
            forget_name(Some("Alex"), Some(&alex), None, Some(&bo_said)),
            None
        );
        assert_eq!(
            forget_name(Some(" "), Some(&alex), None, Some(&theirs_said)),
            None
        );
        assert_eq!(
            forget_name(None, Some(&alex), None, Some(&theirs_said)),
            None
        );
    }

    /// The first builder on #2602 and El's P2 on #3018: a room's line (no
    /// known person's) asking to forget a named person sends nothing unless
    /// the eyes saw that person's own face, and no other, saying it, with
    /// their known file fresh when it was heard (read as the voice reads it,
    /// `speaking_faces::known_with_face`); else the model hears
    /// [`NOT_THEIRS`] and the eyes are never asked.
    #[test]
    fn a_rooms_forget_needs_the_named_face_seen_saying_it() {
        use crate::speaking_faces::{LineSeen, known_face_of, known_with_face};
        use std::io::{BufRead, BufReader, Write};
        let directory =
            std::env::temp_dir().join(format!("lamp-voice-room-forget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let socket = directory.join("who.sock");
        let known = known_file_of(&socket);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let call = |name: &str| ToolCall {
            id: "1".into(),
            name: FORGET.into(),
            args: json!({ "name": name }),
        };
        let switches = FaceSwitches::default();
        // The eyes' files as the line is heard, and the faces seen saying it.
        let at_the_lamp = |text: &str, face: &str| {
            std::fs::write(&known, text).unwrap();
            std::fs::write(known_face_of(&known), format!("{face}\n")).unwrap();
        };
        let heard = |speakers: &[&str]| LineSeen {
            request: 1,
            speakers: speakers.iter().map(|id| (*id).to_owned()).collect(),
            known: known_with_face(&known),
            enrollment: None,
        };
        let refused = |line: Option<&LineSeen>| {
            let told = run_for(&socket, &call("Alex"), switches, None, line);
            assert_eq!(told, (json!({"error": NOT_THEIRS}), true), "{line:?}");
        };
        // Nobody known at the lamp, then someone else.
        refused(Some(&heard(&["f1"])));
        at_the_lamp("Bo\n", "f1");
        refused(Some(&heard(&["f1"])));
        // Alex sits nearest while a visitor or a screen says it, or both
        // speak, or nobody's face was seen speaking it, or speaking was not
        // judged for the line at all.
        at_the_lamp("Alex\n", "f1");
        refused(Some(&heard(&["f2"])));
        refused(Some(&heard(&["f1", "f2"])));
        refused(Some(&heard(&[])));
        refused(None);
        // Alex's known file stale when the line was heard: a line said
        // before they sat down is not theirs, whatever the file says now.
        let old = SystemTime::now() - Duration::from_secs(10);
        std::fs::File::options()
            .write(true)
            .open(&known)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let before = heard(&["f1"]);
        at_the_lamp("Alex\n", "f1");
        refused(Some(&before));
        assert_eq!(
            listener.accept().map(|_| ()).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "the eyes were asked"
        );
        // Alex's own face seen saying it: forgotten under the eyes' name; and
        // so with the eyes' memory id after the name
        // (`LAMP_MEMORY_STABLE_ID=on`, #2176).
        listener.set_nonblocking(false).unwrap();
        let asked = std::thread::spawn(move || {
            (0..2)
                .map(|_| {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut line = String::new();
                    BufReader::new(stream.try_clone().unwrap())
                        .read_line(&mut line)
                        .unwrap();
                    stream.write_all(b"{\"forgot\":true}\n").unwrap();
                    serde_json::from_str::<Value>(&line).unwrap()
                })
                .collect::<Vec<_>>()
        });
        let theirs = heard(&["f1"]);
        let told = run_for(&socket, &call("alex"), switches, None, Some(&theirs));
        assert_eq!(told, (json!({"ok": true}), true));
        let id = crate::remember::person_id("an id the eyes gave");
        at_the_lamp(&format!("Alex\n{id}\n"), "f7");
        let theirs = heard(&["f7"]);
        assert_eq!(theirs.known, Some(("Alex".to_owned(), "f7".to_owned())));
        let told = run_for(&socket, &call("Alex"), switches, None, Some(&theirs));
        assert_eq!(told, (json!({"ok": true}), true));
        assert_eq!(
            asked.join().unwrap(),
            [json!({"forget": "Alex"}), json!({"forget": "Alex"})]
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// #2850 through a real socket: the line is Alex's and the eyes' known
    /// file names them, so `{"forget":"Alex"}` is sent for the model's
    /// "Alexander"; with the known file naming someone else and the model
    /// naming them too, nothing is sent and the model is told why.
    #[test]
    fn forget_my_face_sends_the_name_the_eyes_keep() {
        use std::io::{BufRead, BufReader, Write};
        let directory =
            std::env::temp_dir().join(format!("lamp-voice-forget-face-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let socket = directory.join("who.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let asked = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            stream.write_all(b"{\"forgot\":true}\n").unwrap();
            serde_json::from_str::<Value>(&line).unwrap()
        });
        let call = |name: &str| ToolCall {
            id: "1".into(),
            name: FORGET.into(),
            args: json!({ "name": name }),
        };
        let alex = crate::remember::person_id("Alex");
        std::fs::write(known_file_of(&socket), "Alex\n").unwrap();
        let told = run_for(
            &socket,
            &call("Alexander"),
            FaceSwitches::default(),
            Some(&alex),
            None,
        );
        assert_eq!(told, (json!({"ok": true}), true));
        assert_eq!(asked.join().unwrap(), json!({"forget": "Alex"}));
        std::fs::write(known_file_of(&socket), "Bo\n").unwrap();
        let told = run_for(
            &socket,
            &call("Bo"),
            FaceSwitches::default(),
            Some(&alex),
            None,
        );
        assert_eq!(told, (json!({"error": NOT_THEIRS}), true));
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// #5141 rows 7, 9 and 10 (lamp-0c4e, 3 Nov): with the record switch a
    /// refused face request is told the face record as it stands, so a face
    /// saved under a misheard name can be forgotten under the name it is
    /// kept by; never someone else's, who is only in view. Off, the refusal
    /// is byte for byte as before.
    #[test]
    fn with_the_record_switch_a_face_refusal_says_the_record_as_it_stands() {
        use crate::speaking_faces::{LineSeen, known_face_of, known_with_face};
        let directory =
            std::env::temp_dir().join(format!("lamp-voice-record-told-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        // No eyes listen: none of these refusals asks them anything.
        let socket = directory.join("who.sock");
        let known = known_file_of(&socket);
        let forget = |name: &str| ToolCall {
            id: "1".into(),
            name: FORGET.into(),
            args: json!({ "name": name }),
        };
        let on = FaceSwitches {
            record: true,
            ..FaceSwitches::default()
        };
        // The face was saved as "Loren"; they ask under the name they say.
        std::fs::write(&known, "Loren\n").unwrap();
        std::fs::write(known_face_of(&known), "f1\n").unwrap();
        let heard = |speakers: &[&str]| LineSeen {
            request: 1,
            speakers: speakers.iter().map(|id| (*id).to_owned()).collect(),
            known: known_with_face(&known),
            enrollment: None,
        };
        let theirs = heard(&["f1"]);
        let kept = "Nothing was changed: the face of the one asking is kept as Loren. Use that name to forget or rename it.";
        assert_eq!(
            run_for(&socket, &forget("Lauren"), on, None, Some(&theirs)),
            (json!({"error": NOT_THEIRS, "kept": kept}), true)
        );
        // Loren in view while another face says it: theirs is not told.
        let visitor = heard(&["f2"]);
        assert_eq!(
            run_for(&socket, &forget("Lauren"), on, None, Some(&visitor)),
            (json!({"error": NOT_THEIRS, "kept": RECORD_UNKNOWN}), true)
        );
        // The line is Loren's own: a face request refused before the eyes
        // are asked says so too; someone else's line hears nothing of it.
        let loren = crate::remember::person_id("Loren");
        let speaker = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({ "name": "Lauren" }),
        };
        let speaker_on = FaceSwitches {
            remember_speaker: true,
            ..on
        };
        assert_eq!(
            run_for(&socket, &speaker, speaker_on, Some(&loren), None),
            (
                json!({"error": crate::remember_speaker::NOT_SEEN, "kept": kept}),
                true
            )
        );
        let bo = crate::remember::person_id("Bo");
        assert_eq!(
            run_for(&socket, &speaker, speaker_on, Some(&bo), None),
            (
                json!({"error": crate::remember_speaker::NOT_SEEN, "kept": RECORD_UNKNOWN}),
                true
            )
        );
        // Off: as before.
        let off = FaceSwitches::default();
        assert_eq!(
            run_for(&socket, &forget("Lauren"), off, None, Some(&theirs)),
            (json!({"error": NOT_THEIRS}), true)
        );
        assert_eq!(
            run_for(
                &socket,
                &speaker,
                FaceSwitches {
                    remember_speaker: true,
                    ..off
                },
                Some(&loren),
                None
            ),
            (json!({"error": crate::remember_speaker::NOT_SEEN}), true)
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }

    /// #5141 row 10: each refusal's words have a reason name for the
    /// journal, the eyes' by their own code, and the journal line says the
    /// function and that name only: no words, no name, no id.
    #[test]
    fn each_face_refusal_has_a_reason_name_and_the_journal_says_only_it() {
        for (error, reason) in [
            (NOT_HEARD, "not_heard"),
            (ONLY_THE_CALL, "only_the_call"),
            (NOT_OWN_NAME, "not_own_name"),
            (NAME_UNCHANGED, "name_unchanged"),
            (NOT_AGREED, "not_agreed"),
            (NEAR_TO, "near_to"),
            (NOT_LIKE_THEM, "not_like_them"),
            (NOT_THEIRS, "not_theirs"),
            (AUTHORITY_GONE, "authority_gone"),
            (NO_NAME, "no_name"),
            (NOT_KNOWN, "not_known"),
            (crate::remember_speaker::NOT_SEEN, "speaker_not_seen"),
        ] {
            assert_eq!(refusal_reason(error), reason, "{error}");
        }
        // The eyes' refusals, by the words the model is told of each code.
        for code in [
            "no_face",
            "too_far",
            "two_faces",
            "eyes_shut",
            "peer_closed",
            "busy",
            "full",
            "not_a_name",
        ] {
            let told = told(&json!({ "error": code }));
            assert_eq!(refusal_reason(told["error"].as_str().unwrap()), code);
        }
        let known_as = told(&json!({"error": "known_as", "known_as": "Loren"}));
        assert_eq!(
            refusal_reason(known_as["error"].as_str().unwrap()),
            "known_as"
        );
        // The words where they are made, as the journal names them.
        assert_eq!(
            refusal_reason(told(&json!({}))["error"].as_str().unwrap()),
            "eyes_could_not"
        );
        let not_known = told(&json!({"forgot": false}));
        assert_eq!(
            refusal_reason(not_known["error"].as_str().unwrap()),
            "not_known"
        );
        let socket =
            std::env::temp_dir().join(format!("lamp-voice-no-name-{}", std::process::id()));
        let nameless = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({ "name": " " }),
        };
        let (no_name, _) = run_with(&socket, &nameless, false, &Said::default());
        assert_eq!(
            refusal_reason(no_name["error"].as_str().unwrap()),
            "no_name"
        );
        assert_eq!(refusal_reason("something new"), "other");
        assert_eq!(
            refusal_line(FORGET, "not_theirs"),
            "lamp-voice: forget_my_face refused: not_theirs; the model is told the face record as it stands (LAMP_NAMES_RECORD_TOLD)"
        );
        assert!(refusal_line("Loren's own", "near_to").starts_with("lamp-voice: other refused"));
    }

    /// #5141 row 7: what keeps a held face forget through the lamp's
    /// question asking them to spell their name.
    #[test]
    fn an_answer_asking_them_to_spell_their_name_is_told_apart() {
        for answer in [
            "Sorry. How do you spell your name?",
            "Could you spell it for me, letter by letter?",
            "Bạn đánh vần tên giúp mình nhé?",
        ] {
            assert!(asks_to_spell(answer), "{answer}");
        }
        for answer in [
            "Shall I forget your face?",
            "What's your name?",
            "That was a spellbinding story.",
        ] {
            assert!(!asks_to_spell(answer), "{answer}");
        }
    }
}
