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
    /// With names checked, a rename's yes is judged by its guards
    /// ([`rename_agreed`]), not by [`agreed_to`]'s word list
    /// ([`RENAME_YES_ENVIRONMENT`], #5485).
    pub rename_yes: bool,
    /// With names checked, a name spelled after the lamp asked for it is
    /// taken on a rename as at setup, two letters joined as a word
    /// ([`NAME_LEARN_FIX_ENVIRONMENT`], #7331).
    pub name_learn_fix: bool,
    /// With names checked, a new face is kept with nothing asked, past
    /// setup and at setup's first save, only on speaking-face proof
    /// ([`FACE_NO_ASK_ENVIRONMENT`]).
    pub face_no_ask: bool,
    /// With names checked, a new face's save sent past the face question
    /// only because the eyes' known file named a face is kept only for a
    /// face the eyes know ([`FACE_SAVE_KNOWN_ONLY_ENVIRONMENT`], #7704).
    pub face_save_known_only: bool,
    /// A question whether the lamp knows them ("Do you remember my
    /// face?") is no ask to remember them, and a face already known by the
    /// name asked for is told so with no look and no save
    /// ([`FACE_QUESTION_NOT_ASK_ENVIRONMENT`]).
    pub face_question: bool,
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

/// A rename's yes is judged by its guards ([`rename_agreed`]), not by a
/// word list, and the voice runs a rename held for that yes itself
/// (`second_provider::HeldRename`): lamp-29e7, 6 Nov (#5485), where every
/// yes to "Do you want me to know you as A instead?" was refused for a
/// word outside [`BESIDE_A_YES`]. Only with `LAMP_NAMES=checked`; the
/// second route only; `on` only, off unless set.
pub const RENAME_YES_ENVIRONMENT: &str = "LAMP_NAMES_RENAME_YES";

/// Whether [`RENAME_YES_ENVIRONMENT`] is on.
pub fn rename_yes_on() -> bool {
    std::env::var(RENAME_YES_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The owner's name learned on the paths that lost it on lamp-74ff (#7331):
/// a rename question cut before its name was heard is asked again
/// (`second_provider::held_rename`), a name given while nobody is known
/// goes to the face save (`remember::needs_face`), and a name spelled after
/// the lamp asked for it is taken on a rename too, two letters joined
/// ([`run_switched`]). `on` only; off unless set.
pub const NAME_LEARN_FIX_ENVIRONMENT: &str = "LAMP_NAME_LEARN_FIX";

/// Whether [`NAME_LEARN_FIX_ENVIRONMENT`] is exactly `on`.
pub fn name_learn_fix_on() -> bool {
    std::env::var(NAME_LEARN_FIX_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Faces kept with nothing asked of the person (the first builder 18:50,
/// from the product owner): past setup, only with speaking-face proof
/// ([`run_speaker`], `LAMP_REMEMBER_SPEAKER`), a speaker naming themselves
/// keeps their face at once and the lamp says only "I'll know you next
/// time." ([`KEPT_NO_ASK`]); a face the eyes could not see alone keeps
/// nothing and asks nothing ([`NOT_SEEN_NO_ASK`]). Without that proof
/// (Thirteen's and Heron's HOLDs on #7787) no face is kept and nothing is
/// asked either, only a plain reason said to their own ask for their face
/// ([`no_ask_unkept`], the first builder 21:13). Who was here's question is not asked past setup
/// ([`no_ask_visits`]). At the setup walk's first save (`at_setup`) the
/// same proof keeps the face with no face question and no read-back's yes
/// ([`setup_no_ask_told`], [`SETUP_KEPT_NO_ASK`]); without it nothing is
/// kept and nothing asked, and the step ends with the name only
/// ([`SETUP_NOT_KEPT_NO_ASK`], the first builder's ruling on #7810). Only
/// with names checked; `on` only, off unless set.
pub const FACE_NO_ASK_ENVIRONMENT: &str = "LAMP_FACE_NO_ASK";

/// Whether [`FACE_NO_ASK_ENVIRONMENT`] is exactly `on`.
pub fn face_no_ask_on() -> bool {
    std::env::var(FACE_NO_ASK_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// With `LAMP_NAME_LEARN_FIX`, a new face's save skips the face question
/// while the eyes' known file names a face at the lamp (#7653), and that
/// file can still name someone for up to its 3 s after a stranger steps in
/// (#7704). With this switch that save asks the eyes to keep it only for a
/// face they know (`"known_only":true`); their `not_known` is told as the
/// face question ([`ASK_FACE_FIRST`]), nothing kept. Not with
/// `LAMP_FACE_NO_ASK`, which asks nothing. Voice and eyes ship together:
/// an older eyes build refuses the field. Only with names checked; `on`
/// only, off unless set.
pub const FACE_SAVE_KNOWN_ONLY_ENVIRONMENT: &str = "LAMP_FACE_SAVE_KNOWN_ONLY";

/// Whether [`FACE_SAVE_KNOWN_ONLY_ENVIRONMENT`] is exactly `on`.
pub fn face_save_known_only_on() -> bool {
    std::env::var(FACE_SAVE_KNOWN_ONLY_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on")
}

/// A question whether the lamp knows or remembers them ("Do you remember
/// me?", "Did you remember my face?", "Do you know my face?") is no ask to
/// remember them (lamp-74ff, 19 Nov: two such questions, right after the
/// face was kept, called `remember_my_face`, and the name checks answered
/// that the lamp could not take the look). The model is told so in the
/// tool's description ([`QUESTION_NOT_ASK`]); its call still stands, never
/// judged by words here. A call for the face in view under the name the
/// eyes already know it by, a fresh real match, is told so
/// ([`already_known`]), with no look and nothing saved. `on` only, off
/// unless set.
pub const FACE_QUESTION_NOT_ASK_ENVIRONMENT: &str = "LAMP_FACE_QUESTION_NOT_ASK";

/// Whether [`FACE_QUESTION_NOT_ASK_ENVIRONMENT`] is exactly `on`.
pub fn face_question_not_ask_on() -> bool {
    std::env::var(FACE_QUESTION_NOT_ASK_ENVIRONMENT)
        .ok()
        .as_deref()
        == Some("on")
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
    let pleasantry = pleasantries(&words);
    let agreeing = (0..words.len())
        .any(|at| pleasantry[at] || AGREE.iter().any(|phrase| phrase_at(&words, at, phrase)));
    agreeing && !takes_back(&words, &pleasantry)
}

/// Whether `phrase` is said in `words` from `at`.
fn phrase_at(words: &[String], at: usize, phrase: &[&str]) -> bool {
    phrase
        .iter()
        .enumerate()
        .all(|(next, word)| words.get(at + next).is_some_and(|said| said == word))
}

/// Which of `words` are in a yes that holds a no word ([`AGREE_WITH_A_NO`]:
/// "No problem", "why not").
fn pleasantries(words: &[String]) -> Vec<bool> {
    let mut pleasantry = vec![false; words.len()];
    for at in 0..words.len() {
        if let Some(phrase) = AGREE_WITH_A_NO
            .iter()
            .find(|phrase| phrase_at(words, at, phrase))
        {
            pleasantry[at..at + phrase.len()].fill(true);
        }
    }
    pleasantry
}

/// Whether a [`DISAGREE`] word stands in `words` outside a pleasantry
/// (`pleasantry`, [`pleasantries`]): the half of [`agrees`] that takes a
/// yes back, and of [`refuses`].
fn takes_back(words: &[String], pleasantry: &[bool]) -> bool {
    words
        .iter()
        .zip(pleasantry)
        .any(|(word, kept)| !kept && DISAGREE.contains(&word.as_str()))
}

/// Whether the turn says no to the lamp's question and nothing that
/// forgets (#8165): a no ([`DISAGREE`] outside a pleasantry, a plain no by
/// `services::plain_no`, or a forget word negated, `remember::negates_a_forget`)
/// and no forget word or phrase that is not negated
/// (`remember::forgets_unnegated`). So "No.", "No, keep it.", "Don't.",
/// "Lamp, no.", "No, don't forget it.", "Don't drop it.", "Đừng quên."
/// refuse; "No problem, go ahead.", "No, just forget it.", "No, drop it."
/// and "No, I don't need it." don't. A keep-side backstop to the model's
/// judgment of a one-face forget's answer
/// (`second_provider::take_held_forget`): it only ever keeps, never forgets.
pub(crate) fn refuses(turn: &str) -> bool {
    let words = lower_words(turn);
    let pleasantry = pleasantries(&words);
    (takes_back(&words, &pleasantry)
        || crate::services::plain_no(turn)
        || crate::remember::negates_a_forget(turn))
        && !crate::remember::forgets_unnegated(turn)
}

/// The words a refusal that is only a refusal is made of ([`only_refuses`],
/// #8185): no words, keep words, the forget words a no negates, and words
/// that stand beside them. Never "just", "mind", "wait", "now" or
/// "scratch": with the comma dropped, "Never mind just delete it" or "No,
/// scratch it." asks for the forget (the second read on #8215).
const ONLY_REFUSING: &[&str] = &[
    "no",
    "nope",
    "nah",
    "not",
    "don't",
    "dont",
    "never",
    "kh\u{f4}ng",
    "\u{111}\u{1eeb}ng",
    "ch\u{1edb}",
    "th\u{f4}i",
    "keep",
    "leave",
    "hold",
    "gi\u{1eef}",
    "l\u{1ea1}i",
    "forget",
    "delete",
    "erase",
    "remove",
    "wipe",
    "clear",
    "drop",
    "get",
    "rid",
    "of",
    "qu\u{ea}n",
    "x\u{f3}a",
    "xo\u{e1}",
    "please",
    "lamp",
    "hey",
    "it",
    "that",
    "thanks",
    "thank",
    "you",
    "i",
    "want",
    "need",
    "to",
    "course",
    "\u{111}i",
    "nh\u{e9}",
];

/// Whether the turn is a refusal and nothing else (#8185): [`refuses`],
/// no question, at most six words (as `services::plain_no`), each of them
/// a no, a keep word, a negated forget word or a word beside them
/// ([`ONLY_REFUSING`]), "do" only in "do not". "No.", "Lamp, no.", "No,
/// keep it.", "Don't.", "No, don't forget it.", "Không, giữ lại." are;
/// "No, turn the lights on.", "Yes, but don't forget it." and "No, do it."
/// are not: what else they say is the model's to answer.
pub(crate) fn only_refuses(turn: &str) -> bool {
    if turn.trim_end().ends_with('?') || !refuses(turn) {
        return false;
    }
    let words = lower_words(turn);
    words.len() <= 6
        && words.iter().enumerate().all(|(at, word)| {
            ONLY_REFUSING.contains(&word.as_str())
                || word == "do" && words.get(at + 1).is_some_and(|next| next == "not")
        })
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
    if counted_otherwise(name, &said.this) {
        return false;
    }
    if said_by_them(name, &said.this) {
        return true;
    }
    agreed_to(name, said) && said_before(name, said)
}

/// Whether the person counted `name`'s letters otherwise in `turn`.
fn counted_otherwise(name: &str, turn: &str) -> bool {
    let letters = name.chars().filter(|c| c.is_alphabetic()).count();
    letters_counted(turn).is_some_and(|counted| counted != letters)
}

/// Whether the person said `name` in an exchange before the turn in hand,
/// with no count of its letters otherwise there.
fn said_before(name: &str, said: &Said) -> bool {
    said.before
        .iter()
        .any(|(person, _)| said_by_them(name, person) && !counted_otherwise(name, person))
}

/// [`name_heard`] for a rename with `LAMP_NAMES_RENAME_YES` (#5485): as
/// heard, or said by them in an exchange before and agreed to now by
/// [`rename_agreed`] in place of [`agreed_to`]. So "Can you call me A
/// instead?", "A instead of B, is that right?", "Okay. Can you repeat my
/// name again?" (lamp-29e7, 6 Nov) hears A. Never with their letters
/// counted otherwise.
fn heard_for_rename(name: &str, said: &Said) -> bool {
    name_heard(name, said)
        || (!counted_otherwise(name, &said.this)
            && rename_agreed(name, said)
            && said_before(name, said))
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

/// Which of `words` are a yes's own ([`AGREE`], [`AGREE_WITH_A_NO`]).
fn agreeing_words(words: &[String]) -> Vec<bool> {
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
    covered
}

/// [`plain_yes`], with `beside` also standing beside the yes.
fn plain_yes_beside(name: &str, turn: &str, beside: &[&str]) -> bool {
    if turn.trim_end().ends_with('?') {
        return false;
    }
    let words = lower_words(turn);
    let name = folded_words(name);
    let covered = agreeing_words(&words);
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

/// What a yes to a held keep may say it keeps, right after "keep"
/// ([`plain_agreement_to_keep`]): "keep that", "keep it".
const KEEP_WHAT: &[&str] = &["that", "it", "this", "those", "them"];

/// [`plain_agreement`], or a plain yes that also says to keep it: "Yes,
/// please keep that.", "Yes, keep it." (lamp-06c1, 5 Nov, S4: the held
/// keep was dropped as not a plain yes). Also, with no yes word, the keep
/// said back alone, "Please keep that.", "Keep it.", and, beside a yes,
/// "keep off", which is how speech to text heard "keep that" on lamp-06c1
/// (5 Nov, S14 run 14:21, request 424: "Yes. Please keep off.", and the
/// brother fact was never kept). "Off" only right after "keep", and never
/// without a yes word: "Keep off." alone, or "Yes, keep it off.", is not
/// one ([`unclear_answer_to_keep`]). Any no ([`DISAGREE`]) refuses. Only
/// for a keep held for the person's yes; a held forget, and every other
/// ask, takes [`plain_agreement`].
pub(crate) fn plain_agreement_to_keep(turn: &str) -> bool {
    if turn.trim_end().ends_with('?') {
        return false;
    }
    let words = lower_words(turn);
    let after_keep = |at: usize, these: &[&str]| {
        at > 0 && words[at - 1] == "keep" && these.contains(&words[at].as_str())
    };
    let agreeing = agrees(turn);
    let keeps_it = (1..words.len()).any(|at| after_keep(at, KEEP_WHAT))
        && !words.iter().any(|word| DISAGREE.contains(&word.as_str()));
    if !agreeing {
        // The keep said back with no yes word: only "please", "lamp", "for",
        // "me" and a thanks may stand around it, never a word that makes it a
        // question whose "?" was lost ("Do you keep that", "I'd keep it";
        // #5572's privacy read, Jarvis, 18:41).
        return keeps_it
            && (0..words.len()).all(|at| {
                words[at] == "keep"
                    || after_keep(at, KEEP_WHAT)
                    || BESIDE_A_KEEP_ALONE.contains(&words[at].as_str())
                    || (words[at] == "you" && at > 0 && words[at - 1] == "thank")
            });
    }
    let covered = agreeing_words(&words);
    (0..words.len()).all(|at| {
        covered[at]
            || BESIDE_A_YES.contains(&words[at].as_str())
            || words[at] == "keep"
            || after_keep(at, KEEP_WHAT)
            || after_keep(at, &["off"])
    })
}

/// What may stand around a keep said back with no yes word
/// ([`plain_agreement_to_keep`]): "Please keep that for me, thanks.", "Lamp,
/// keep it.". Not [`BESIDE_A_YES`]'s "do", "would", "i", "i'd" or a "you" of
/// its own, which make the keep a question: "Do you keep that" is asked about
/// once more ([`unclear_answer_to_keep`]).
const BESIDE_A_KEEP_ALONE: &[&str] = &["please", "lamp", "for", "me", "thanks", "thank"];

/// The most words an answer to a held keep is asked about again in
/// ([`unclear_answer_to_keep`]): a line that says more says something else.
const UNCLEAR_MOST: usize = 6;

/// Whether `turn`, the person's answer to the lamp's offer to keep a held
/// keep, is neither a yes to it ([`plain_agreement_to_keep`]) nor a no: a
/// yes or a keep with a word misheard ("Yes. Please key that.", "Please
/// keep of."), "Keep off." with no yes, or a yes asked back ("Yes?"). At
/// most [`UNCLEAR_MOST`] words, a yes word or "keep"/"save" among them,
/// and no word of a no ([`DISAGREE`]). The lamp asks once more, "Should I
/// keep that?", rather than dropping the keep unsaid
/// (`second_provider::take_held_forget`).
pub(crate) fn unclear_answer_to_keep(turn: &str) -> bool {
    let words = lower_words(turn);
    !words.is_empty()
        && words.len() <= UNCLEAR_MOST
        && !plain_agreement_to_keep(turn)
        && !words.iter().any(|word| DISAGREE.contains(&word.as_str()))
        && (agrees(turn)
            || words
                .iter()
                .any(|word| matches!(word.as_str(), "keep" | "save")))
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

/// Words that take a rename's yes back besides [`DISAGREE`]: "Yes, but
/// it's Kess.", "Okay, actually it's Kess.", "Sure, it's Kess though." (the
/// reviewer on #1208), and a maybe: "Có lẽ." ("maybe", whose "có" is a
/// yes word), "Yes, maybe.", "Perhaps."; a hesitant yes, asked once more
/// as #5572 asks an unclear one: "Okay, wait.", "Yes, hold on.", "Okay,
/// let me think."; and the Vietnamese "nhưng" (but) and "khoan" (wait):
/// "Ừ, nhưng thôi.", "Khoan đã." (Thirteen on #5586). Whole words or
/// phrases of them in a row ([`lower_words`], accents kept, so "nhưng" is
/// not "nhung"), never part of a word: "holding" or "think" alone take
/// nothing back.
const TAKE_BACK: &[&[&str]] = &[
    &["but"],
    &["actually"],
    &["though"],
    &["maybe"],
    &["perhaps"],
    &["lẽ"],
    &["wait"],
    &["hold", "on"],
    &["hang", "on"],
    &["let", "me", "think"],
    &["nhưng"],
    &["khoan"],
];

/// Whether the lamp said `name` back in its last answer and the turn in
/// hand agrees to it, judged by its guards, not by a word list
/// (`LAMP_NAMES_RENAME_YES`, #5485): some sentence of it that is no
/// question agrees ([`agrees`]), so "Okay. Can you repeat my name again?"
/// is a yes and a lone "Yes?" is not; nothing in it takes the yes back
/// ([`DISAGREE`], [`TAKE_BACK`]); and it names no one else
/// ([`names_someone_else`]). What `replace` needs with the switch on, in
/// place of [`agreed_to`].
///
/// Why, from lamp-29e7 on 6 Nov: the person said "Yes. Please forget the
/// name B, and this is my face, the face of A." and later "Okay. Can you
/// repeat my name again?", and each was refused for words outside
/// [`BESIDE_A_YES`] or for the question; the face kept the old name.
pub(crate) fn rename_agreed(name: &str, said: &Said) -> bool {
    let said_back = said
        .before
        .last()
        .is_some_and(|(_, lamp)| said_by_them(name, lamp));
    let turn = said.this.as_str();
    let a_statement_agrees = turn
        .split_inclusive(['.', '!', '?'])
        .filter(|sentence| !sentence.trim_end().ends_with('?'))
        .any(agrees);
    let words = lower_words(turn);
    let taken_back = (0..words.len()).any(|at| {
        TAKE_BACK.iter().any(|phrase| {
            phrase
                .iter()
                .enumerate()
                .all(|(next, word)| words.get(at + next).is_some_and(|said| said == word))
        })
    });
    said_back
        && agrees(turn)
        && a_statement_agrees
        && !taken_back
        && !names_someone_else(name, turn)
}

/// Whether `turn` names someone other than `name`: it gives a name
/// ([`gives_a_name`]) once `name`'s own words are taken out ("Yes, my name
/// is Kess."); it spells a run of letters that is not `name` or one of its
/// words ("Yes, K, E, S, S."); or it holds a word one letter from one of
/// `name`'s, as a misheard name corrected is ("Dạ, Kess ạ." to "Kesh",
/// the reviewer on #1208). Only names of four letters or more have such
/// near words: "and" is one letter from "Ann". No word is judged by its
/// capital letter, which is the transcript's, not the person's.
fn names_someone_else(name: &str, turn: &str) -> bool {
    let name_words = folded_words(name);
    let without_name: String = turn
        .split_whitespace()
        .map(|token| match folded_words(token).as_slice() {
            [word] if name_words.contains(word) => token
                .chars()
                .filter(|c| !c.is_alphanumeric())
                .collect::<String>(),
            _ => token.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    let gives_another = gives_a_name(&without_name);
    let whole = name_words.concat();
    let spells_another = spelled_runs(&folded_words(turn))
        .iter()
        .filter(|letters| letters.chars().count() >= 2)
        .any(|letters| *letters != whole && !name_words.contains(letters));
    let near_another = folded_words(turn).iter().any(|word| {
        !name_words.contains(word)
            && name_words
                .iter()
                .filter(|own| own.chars().count() >= 4)
                .any(|own| one_letter_apart(own, word))
    });
    gives_another || spells_another || near_another
}

/// Whether `a` and `b` differ by exactly one letter changed, added or
/// taken out.
fn one_letter_apart(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (short, long) = if a.len() <= b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    match long.len() - short.len() {
        0 => {
            short
                .iter()
                .zip(long.iter())
                .filter(|(x, y)| x != y)
                .count()
                == 1
        }
        1 => (0..long.len()).any(|skip| {
            long.iter()
                .enumerate()
                .filter(|(at, _)| *at != skip)
                .map(|(_, c)| c)
                .eq(short.iter())
        }),
        _ => false,
    }
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
    // The held call keeps the model's raw name; it is judged as the word
    // its letters make, as `run_switched` judges it: "J-A-N-E" read back
    // as "Jane" and said yes to runs (Fifth's P2 on #5580).
    let joined = letters_as_word(name);
    let name = joined.as_deref().unwrap_or(name);
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

/// The name the lamp's last answer said back, when the turn in hand is a
/// plain yes to it and no save was called for it (#5684, the Walter trace:
/// "Name is Walter.", "Walter, did I get that right?", "Yes. That's my
/// name.", and no call at all, so the face wait ended "who you are" with
/// the name only). The name is the one the person's line before gave
/// (`remember::named::name_in`), judged as a held save's is
/// ([`held_save_verdict`]) on that last exchange only: an older read-back
/// already had its answer. `None` for a correction, a question, the face
/// question with the name folded in, or any other answer. Such a yes is to
/// the name only: it asks the face question next, and keeps no face by
/// itself ([`face_agreed`]).
pub(crate) fn read_back_name(said: &Said) -> Option<String> {
    let (person, _) = said.before.last()?;
    let name = crate::remember::named::name_in(person)?;
    let name = name.trim();
    (!name.is_empty() && yes_to_the_read_back(name, said)).then(|| name.to_owned())
}

/// Whether the turn in hand is a plain yes to `name` said back in the
/// lamp's last answer, that exchange alone ([`read_back_name`]).
pub(crate) fn yes_to_the_read_back(name: &str, said: &Said) -> bool {
    let Some(last) = said.before.last() else {
        return false;
    };
    let joined = letters_as_word(name);
    let name = joined.as_deref().unwrap_or(name);
    let last = Said {
        this: said.this.clone(),
        before: vec![last.clone()],
    };
    said_back_and_agreed(name, &last)
}

/// What asks leave to remember, just before "remember": "May I …", "Do
/// you want me to …".
const ASKS_LEAVE: &[&[&str]] = &[
    &["may", "i"],
    &["can", "i"],
    &["could", "i"],
    &["shall", "i"],
    &["should", "i"],
    &["do", "you", "want", "me", "to"],
    &["would", "you", "like", "me", "to"],
    &["is", "it", "okay", "if", "i"],
    &["is", "it", "ok", "if", "i"],
];

/// Words that turn a face question round, or make it a choice, so a yes
/// to it keeps nothing: "Would you rather I didn't remember your face?",
/// "Shall I not …?", "Should I remember your name or your face?", "…
/// without your face?" (Fourteen's HOLDs on #5720).
const TURNS_IT_ROUND: &[&str] = &[
    "not", "never", "forget", "rather", "unless", "instead", "or", "without", "skip", "no",
];

/// The same in Vietnamese anywhere in the line ("Đừng nhớ …", "quên",
/// "hay"/"hoặc" for a choice).
const TURNS_IT_ROUND_VI: &[&str] = &[
    "\u{111}\u{1eeb}ng",
    "qu\u{ea}n",
    "ch\u{1eb3}ng",
    "hay",
    "ho\u{1eb7}c",
];

/// Whether the lamp's line is setup's face question ("May I remember your
/// face, so I know you next time?"), with or without the name in it: it
/// asks leave to remember their face, the face right after "remember"
/// ("your face", "the face"), and nothing in it turns that round
/// (Fourteen's HOLDs on #5720: a yes to "Would you rather I didn't
/// remember your face?" or "May I remember just your name, without your
/// face?" kept the face). In Vietnamese ("Mình nhớ khuôn mặt bạn được
/// không?") it is the "nhớ" right before "khuôn mặt", and the "không"
/// that asks is after it; a "không" or "chưa" before it ("Mình nhớ tên
/// thôi, không nhớ khuôn mặt nhé?") turns it round.
pub(crate) fn asks_for_the_face(lamp: &str) -> bool {
    let words = lower_words(lamp);
    if !lamp.contains('?') {
        return false;
    }
    if let Some(at) = words
        .windows(3)
        .position(|three| three == ["nh\u{1edb}", "khu\u{f4}n", "m\u{1eb7}t"])
    {
        return !words
            .iter()
            .any(|word| TURNS_IT_ROUND_VI.contains(&word.as_str()))
            && !words[..at]
                .iter()
                .any(|word| ["kh\u{f4}ng", "ch\u{1b0}a"].contains(&word.as_str()));
    }
    let turned = words
        .iter()
        .any(|word| TURNS_IT_ROUND.contains(&word.as_str()) || word.ends_with("n't"));
    !turned
        && words.iter().enumerate().any(|(at, word)| {
            word == "remember"
                && matches!(
                    words.get(at + 1..at + 3),
                    Some([whose, face]) if (whose == "your" || whose == "the") && face == "face"
                )
                && ASKS_LEAVE
                    .iter()
                    .any(|leave| at >= leave.len() && words[at - leave.len()..at] == leave[..])
        })
}

/// Whether the turn's own words ask for their face to be kept: "Yes.
/// That's right. Please remember my face."
fn asks_to_keep_the_face(turn: &str) -> bool {
    let words = lower_words(turn);
    words.iter().any(|word| word == "face")
        && words
            .iter()
            .any(|word| ["remember", "keep", "save"].contains(&word.as_str()))
}

/// Whether the turn in hand agrees to their face kept under `name`, which
/// a setup face save the voice runs with no model call needs (the first
/// builder's HOLD on #5700: setup's skill asks "May I remember your face,
/// so I know you next time?" only after a clear yes to the name, and keeps
/// the face only after a clear yes to that). Either a plain yes to the face
/// question in the lamp's last answer, with the name said back and agreed
/// to before it, or a plain yes to the name said back whose own words ask
/// for the face ("Yes. That's right. Please remember my face."). A yes to
/// the name alone is no yes to the face.
pub(crate) fn face_agreed(name: &str, said: &Said) -> bool {
    let joined = letters_as_word(name);
    let name = joined.as_deref().unwrap_or(name);
    if yes_to_the_read_back(name, said) {
        return asks_to_keep_the_face(&said.this);
    }
    let Some(((asked_after, lamp), earlier)) = said.before.split_last() else {
        return false;
    };
    let agreed = Said {
        this: asked_after.clone(),
        before: earlier.to_vec(),
    };
    asks_for_the_face(lamp)
        && said_back_and_agreed(name, &agreed)
        && plain_yes_beside(name, &said.this, BESIDE_THE_SAVE)
}

/// Words that, beside the face words, keep a face out rather than ask for
/// it ([`speaker_gave_face_leave`]): "out", "off", "forget".
const NOT_THE_FACE: &[&str] = &["out", "off", "forget"];

/// Whether the speaker's own line gives leave to keep their face, which a
/// speaker-bound first save at setup needs (`LAMP_REMEMBER_SPEAKER`, B3472
/// after #3472): the line asks to keep their own face ("My name is Kesh,
/// remember my face."), or the lamp's last line was the face question
/// ([`asks_for_the_face`]) and the line agrees to it, asking nothing ("Yes,
/// my name is Kesh."). Never with a refusal in it ([`NOT_A_PLAIN_ASK`]),
/// never someone else's face, and never a name alone: "I'm Kesh." keeps
/// nothing, after "What's your name?" or after the face question.
pub(crate) fn speaker_gave_face_leave(heard: &str, lamp_before: Option<&str>) -> bool {
    let words = lower_words(heard);
    if words
        .iter()
        .any(|word| NOT_A_PLAIN_ASK.contains(&word.as_str()) || word.ends_with("n't"))
    {
        return false;
    }
    // "Keep my face out of this", "Leave my face off it", "Forget my face":
    // the face words beside these keep it out, whatever their order
    // (Heron's P3 on #7622).
    if words
        .iter()
        .any(|word| NOT_THE_FACE.contains(&word.as_str()))
    {
        return false;
    }
    let asks_for_their_face = words.windows(2).any(|two| two == ["my", "face"])
        && words
            .iter()
            .any(|word| ["remember", "keep", "save"].contains(&word.as_str()));
    asks_for_their_face
        || (lamp_before.is_some_and(asks_for_the_face) && agrees(heard) && !heard.contains('?'))
}

/// Whether the speaker's own line asks to keep their face out: face words
/// with "out", "off" or "forget" ([`NOT_THE_FACE`]), and no refusal word
/// beside them ([`NOT_A_PLAIN_ASK`]), so "Keep my face out of this, I'm
/// Kesh." is one and "Don't forget my face" is not. Such a line keeps
/// nothing of the face and asks nothing ([`FACE_KEPT_OUT`], Heron's HOLD
/// on #7622): asking "Shall I remember your face?" right after it would
/// not be listening.
pub(crate) fn keeps_face_out(heard: &str) -> bool {
    let words = lower_words(heard);
    words.iter().any(|word| word == "face")
        && words
            .iter()
            .any(|word| NOT_THE_FACE.contains(&word.as_str()))
        && !words
            .iter()
            .any(|word| NOT_A_PLAIN_ASK.contains(&word.as_str()) || word.ends_with("n't"))
}

/// The name a setup face save runs under with no save called
/// ([`face_agreed`]): the latest name the person gave
/// (`remember::named::name_in`) that the turn in hand agrees to keeping
/// their face under.
pub(crate) fn face_agreed_name(said: &Said) -> Option<String> {
    said.before.iter().rev().find_map(|(person, _)| {
        let name = crate::remember::named::name_in(person)?;
        let name = name.trim();
        (!name.is_empty() && face_agreed(name, said)).then(|| name.to_owned())
    })
}

/// Whether a tool answer is one the name checks gave here, before the eyes
/// were asked (`run_switched`): no name, a name not heard, the lamp's own
/// name, no read-back's yes, or no yes to a new name. The voice's log says
/// "refused here" for those, not that the eyes answered (the
/// Reviewer-lead's P3 on #5415).
pub(crate) fn refused_here(told: &Value) -> bool {
    told["error"].as_str().is_some_and(|error| {
        [
            NO_NAME,
            NOT_HEARD,
            NOT_HEARD_SPELLED,
            NOT_HEARD_SAY_AGAIN,
            ONLY_THE_CALL,
            SAY_IT_BACK,
            NOT_AGREED,
            ASK_FACE_FIRST,
            FACE_KEPT_OUT,
            NOT_KEPT_NO_ASK,
            CANT_KEEP_NO_ASK,
        ]
        .contains(&error)
    })
}

/// Whether a tool answer was given here, the eyes not asked: a refusal
/// ([`refused_here`]), or, with `LAMP_FACE_QUESTION_NOT_ASK`, a face already
/// known by that name (`already_known`). The voice's log then never says
/// the eyes answered.
pub(crate) fn answered_here(told: &Value) -> bool {
    refused_here(told) || (told.get("already_known").is_some() && told.get("remembered").is_none())
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
pub(crate) fn said_back(name: &str, lamp: &str) -> bool {
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

/// What a name not heard is told: [`NOT_HEARD`], which asks for a
/// spelling, or, once the lamp asked them to spell in any exchange the
/// check holds, never a second ask (lamp-3e83, #5564):
/// [`NOT_HEARD_SPELLED`] when they spelled after that ask, so the name
/// their letters make is said back, and [`NOT_HEARD_SAY_AGAIN`] when they
/// didn't, since no name of theirs could pass a read-back's yes then
/// (Jarvis's P2 on #5580: "Lord in." again, "Lauren?", "Yes." looped).
fn not_heard(said: &Said) -> &'static str {
    match said.before.iter().position(|(_, lamp)| asks_to_spell(lamp)) {
        None => NOT_HEARD,
        Some(at) if spelled_after(said, at) => NOT_HEARD_SPELLED,
        Some(_) => NOT_HEARD_SAY_AGAIN,
    }
}

/// Whether any of their lines after the lamp's line `before[at]` (its
/// answer, and the turn in hand) spells letter by letter: a run of two
/// letters or more ([`spelled_runs`]), whether or not it agrees with a
/// count they gave ("four letters", five spelled is still a spelling).
fn spelled_after(said: &Said, at: usize) -> bool {
    said.before[at + 1..]
        .iter()
        .map(|(person, _)| person.as_str())
        .chain([said.this.as_str()])
        .any(|person| {
            spelled_runs(&folded_words(person))
                .iter()
                .any(|letters| letters.chars().count() >= 2)
        })
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

/// A name written as its letters ("J-A-N-E", "J A N E", "j.a.n.e."), as
/// the word they make ([`spelled_name`]): "Jane". None for a name that is
/// not only letters, or of fewer than three, which may be initials ("A J",
/// "T.J."). Nobody calls a person by the spelling of their name: on
/// lamp-3e83 a spelled name came back as "Jane or J-A-N-E?" (#5564).
pub(crate) fn letters_as_word(name: &str) -> Option<String> {
    letters_as_word_of(name, 3)
}

/// [`letters_as_word`] of `least` letters or more: two once the lamp asked
/// for a spelling (`LAMP_NAME_LEARN_FIX`, #7331: "A l" is "Al" then, never
/// initials).
fn letters_as_word_of(name: &str, least: usize) -> Option<String> {
    let words = folded_words(name);
    let only_letters = !words.is_empty()
        && words
            .iter()
            .all(|word| word.chars().count() == 1 || word == "double");
    let letters: usize = spelled_runs(&words)
        .iter()
        .map(|run| run.chars().count())
        .sum();
    (only_letters && letters >= least)
        .then(|| spelled_name(name))
        .flatten()
}

/// Whether a name of more than one word is spelled as one run of letters
/// ("M A R Y A N N" for "Mary Ann"), with no pause between its words
/// (Reviewer-lead's P3 on #2275: such a person was asked to spell on every
/// try).
fn spelled_whole(name: &[String], spelled: &[String]) -> bool {
    name.len() > 1 && spelled.iter().any(|letters| *letters == name.concat())
}

/// How a letter is said as a word ("J" is "Jay", "B" is "Bee", "L" is
/// "El"): what speech to text may write as the letter alone for a name
/// that sounds like it ([`as_said`], #7331).
fn letter_said(letter: char) -> Option<String> {
    match letter {
        'b' | 'c' | 'd' | 'g' | 'p' | 't' | 'v' | 'z' => Some(format!("{letter}ee")),
        'j' | 'k' => Some(format!("{letter}ay")),
        'f' | 'l' | 'm' | 'n' | 's' | 'x' => Some(format!("e{letter}")),
        _ => None,
    }
}

/// `name` and `said` with a name heard as its letter made one
/// (`LAMP_NAME_LEARN_FIX`, #7331, V2 on lamp-74ff): a name that is a lone
/// letter is judged as the word it is said as ([`letter_said`]), and a lone
/// letter in what was said, not in a run of letters spelled, as that word
/// when it is one of the name's. So "J", "Jay" and "J a y" are one name
/// for a rename's checks. Only the checks read it: the eyes get the
/// model's name.
pub(crate) fn as_said(name: &str, said: &Said) -> (String, Said) {
    let lone = |token: &str| match folded_words(token).as_slice() {
        [word] if word.chars().count() == 1 => word.chars().next(),
        _ => None,
    };
    let name = lone(name)
        .and_then(letter_said)
        .unwrap_or_else(|| name.to_owned());
    let name_words = folded_words(&name);
    let swap = |text: &str| -> String {
        let tokens: Vec<&str> = text.split_whitespace().collect();
        tokens
            .iter()
            .enumerate()
            .map(|(at, token)| {
                let in_run = (at > 0 && lone(tokens[at - 1]).is_some())
                    || tokens.get(at + 1).is_some_and(|next| lone(next).is_some());
                match lone(token)
                    .filter(|_| !in_run)
                    .and_then(letter_said)
                    .filter(|word| name_words.contains(word))
                {
                    Some(word) => token
                        .chars()
                        .map(|c| {
                            if c.is_alphabetic() {
                                word.clone()
                            } else {
                                c.to_string()
                            }
                        })
                        .collect(),
                    None => (*token).to_owned(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    let said = Said {
        this: swap(&said.this),
        before: said
            .before
            .iter()
            .map(|(person, lamp)| (swap(person), swap(lamp)))
            .collect(),
    };
    (name, said)
}

/// What the lamp says once it kept a name from letters spelled to it
/// (`LAMP_NAME_LEARN_FIX`, #7331): a statement, the word and its letters,
/// "Ada: A, D, A. Got it.", never another question (the first builder on
/// #7331: a question is one more thing a next line can cut).
fn spelled_back(name: &str) -> String {
    let letters: Vec<String> = name
        .chars()
        .filter(|c| c.is_alphabetic())
        .map(|c| c.to_uppercase().collect())
        .collect();
    format!("{name}: {}. Got it.", letters.join(", "))
}

/// The letters of a name the lamp's line `lamp` said back by its letters
/// ([`spelled_back`]): a word, or up to three in a row, right before a run
/// of their own letters ("Ada: A, D, A.").
fn spelled_back_in(lamp: &str) -> Option<String> {
    let words = folded_words(lamp);
    let single = |word: &String| word.chars().count() == 1;
    (1..words.len()).find_map(|start| {
        if !single(&words[start]) || single(&words[start - 1]) {
            return None;
        }
        let run: String = words[start..]
            .iter()
            .take_while(|word| single(word))
            .map(String::as_str)
            .collect();
        (run.chars().count() >= 2
            && (1..=start.min(3)).any(|count| words[start - count..start].concat() == run))
        .then_some(run)
    })
}

/// The letters a line names one by one by a word they start: "N as in
/// nose", "N for nose".
fn letters_named(turn: &str) -> Vec<char> {
    let words = folded_words(turn);
    let named = |letter: &str, word: &str| {
        letter.chars().count() == 1 && word.chars().count() > 1 && word.starts_with(letter)
    };
    (0..words.len())
        .filter_map(|at| match &words[at..] {
            [letter, as_, in_, word, ..] if as_ == "as" && in_ == "in" && named(letter, word) => {
                letter.chars().next()
            }
            [letter, for_, word, ..] if for_ == "for" && named(letter, word) => {
                letter.chars().next()
            }
            _ => None,
        })
        .collect()
}

/// Whether the turn in hand corrects the name the lamp's last answer said
/// back by its letters ([`spelled_back_in`]) into `name`, in one line
/// (`LAMP_NAME_LEARN_FIX`, #7331): it spells `name` ([`spelled_by_them`]),
/// or `name` is that name with only letters the line names changed, at
/// most as many as it names ("No, N as in nose." makes "Ama" "Ana", never
/// "Nna" nor "Ava"). Such a line is the rename's yes: the lamp's last
/// words were no spell ask, but its letters were what they answered.
fn corrects_spelled_back(name: &str, said: &Said) -> bool {
    let Some(old) = said
        .before
        .last()
        .and_then(|(_, lamp)| spelled_back_in(lamp))
    else {
        return false;
    };
    let new = folded_words(name).concat();
    if new.is_empty() || new == old {
        return false;
    }
    if spelled_by_them(name, &said.this) {
        return true;
    }
    let named = letters_named(&said.this);
    let (old, new): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let changed: Vec<char> = old
        .iter()
        .zip(&new)
        .filter(|(was, now)| was != now)
        .map(|(_, now)| *now)
        .collect();
    old.len() == new.len()
        && !changed.is_empty()
        && changed.len() <= named.len()
        && changed.iter().all(|letter| named.contains(letter))
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

/// With `LAMP_FACE_QUESTION_NOT_ASK`: the second worker's faces, read for
/// the one near face's track ([`reused_name`]); `None` until it starts, or
/// without the bus.
static TRACKS: std::sync::Mutex<Option<std::sync::Arc<crate::speaking_faces::SpeakingFaces>>> =
    std::sync::Mutex::new(None);

/// The name confirmed this session (said back and agreed, or spelled, in a
/// save's checks) and the one near face (`boot/f3`) in view as it was
/// ([`note_confirmed`]). Memory only; a forget clears it.
static CONFIRMED: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);

/// Gives [`reused_name`] the second worker's faces (`None`: no reuse).
pub fn sees_tracks_with(eyes: Option<std::sync::Arc<crate::speaking_faces::SpeakingFaces>>) {
    if let Ok(mut tracks) = TRACKS.lock() {
        *tracks = eyes;
    }
}

/// The one near face in view now, by the second worker's faces.
fn one_near_face_now() -> Option<(std::sync::Arc<crate::speaking_faces::SpeakingFaces>, String)> {
    let eyes = TRACKS.lock().ok()?.clone()?;
    let face = eyes.one_near_face(std::time::Instant::now())?;
    Some((eyes, face))
}

/// Keeps `name` as confirmed this session, with the one near face in view
/// now; with none, or two, nothing is kept and an older confirmation goes.
fn note_confirmed(name: &str) {
    if let Ok(mut confirmed) = CONFIRMED.lock() {
        *confirmed = one_near_face_now().map(|(_, face)| (name.to_owned(), face));
    }
}

/// Forgets the name confirmed this session ([`note_confirmed`]).
fn forget_confirmed() {
    if let Ok(mut confirmed) = CONFIRMED.lock() {
        *confirmed = None;
    }
}

/// With `LAMP_FACE_QUESTION_NOT_ASK`: whether `name` may stand as heard
/// though not said again (lamp-74ff, 19 Nov: saves right after the name
/// was confirmed were refused as not heard). All must hold: it is the name
/// confirmed this session ([`note_confirmed`]: said back and agreed, or
/// spelled), folded alike; exactly one near face is in view now
/// ([`crate::speaking_faces::SpeakingFaces::one_near_face`]); and it is the
/// same track as the one near face when it was confirmed, the same passing
/// id or one followed from it. A held or carried name is never one. The
/// save then goes to the eyes' own checks as any other: a name kept for
/// another face is theirs to refuse ([`NOT_LIKE_THEM`]).
pub(crate) fn reused_name(name: &str) -> bool {
    let Some((then_name, then_face)) = CONFIRMED
        .lock()
        .ok()
        .and_then(|confirmed| confirmed.clone())
    else {
        return false;
    };
    let asked = folded_words(name);
    if asked.is_empty() || folded_words(&then_name) != asked {
        return false;
    }
    one_near_face_now()
        .is_some_and(|(eyes, now)| eyes.same_track(&then_face, &now, std::time::Instant::now()))
}

/// What the model is told of a [`REMEMBER`] call for the face in view under
/// the name the eyes already know it by ([`already_known`]), or answered by
/// the eyes `already` (their prints kept in place, no new person).
pub(crate) const ALREADY_KNOWN_NOTE: &str = "You already know this face by that name. Tell them in a few words that you already know their face, with their name, like \"I already know your face, Ana.\" Never say you'll know them now or that you just remembered them, don't mention noticing their visits, and don't say anything failed.";

/// Where the voice reads whether the eyes mark a name kept by a hold, not
/// a match: the held mark beside the known file ([`held_mark_of`]), written
/// by the eyes with their own `LAMP_FACE_STICKY=on` (#7933). `on` only.
/// Without it a held name (`LAMP_FACE_KNOWN_HOLD`'s 4 s renew) can't be
/// told from a match, so [`already_known`] is never taken from the known
/// file and every save goes to the eyes, which match afresh.
pub const FACE_STICKY_ENVIRONMENT: &str = "LAMP_FACE_STICKY";

#[cfg(test)]
thread_local! {
    /// Tests: the held marks taken as written ([`held_marks_on`]).
    static TEST_HELD_MARKS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether [`FACE_STICKY_ENVIRONMENT`] is exactly `on`.
fn held_marks_on() -> bool {
    #[cfg(test)]
    if TEST_HELD_MARKS.with(std::cell::Cell::get) {
        return true;
    }
    std::env::var(FACE_STICKY_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The eyes' held mark beside the known file `known`: `-held` (#7933's
/// `who::known_held_of`).
fn held_mark_of(known: &Path) -> PathBuf {
    let mut beside = known.as_os_str().to_owned();
    beside.push("-held");
    PathBuf::from(beside)
}

/// Whether the eyes' held mark beside `known` was written within [`FRESH`],
/// as the known file is.
fn held_mark_fresh(known: &Path) -> bool {
    std::fs::metadata(held_mark_of(known))
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|written| SystemTime::now().duration_since(written).ok())
        .is_some_and(|age| age < FRESH)
}

/// With [`FaceSwitches::face_question`]: the known name of the face in view
/// when it is `name`, folded alike (case, accents, spaces;
/// [`folded_words`]), and only as a fresh real match (the owner, 20 Nov
/// 02:18): the known file within [`FRESH`] ([`known_now`]), the eyes
/// marking held names ([`held_marks_on`]) and no fresh held mark beside it.
/// `None` for a face known by another name, an unknown face, none, a held
/// name, or eyes whose held names can't be told: the save then goes to
/// the eyes, which answer `already` or `not_like_them` from a fresh look.
pub(crate) fn already_known(socket: &Path, name: &str) -> Option<String> {
    already_known_marked(socket, name, held_marks_on())
}

/// [`already_known`] with the held marks read or not (`marks`).
fn already_known_marked(socket: &Path, name: &str, marks: bool) -> Option<String> {
    let asked = folded_words(name);
    let known = known_file_of(socket);
    if !marks || asked.is_empty() || held_mark_fresh(&known) {
        return None;
    }
    known_now(&known).filter(|known| folded_words(known) == asked)
}

/// Where the second route reads whether a known face giving another name
/// must call [`REMEMBER`], so the lamp asks before renaming them (`on`
/// only; [`gives_another_name`], #5485).
pub const RENAME_ASKED_ENVIRONMENT: &str = "LAMP_NAMES_RENAME_ASKED";

/// Whether [`RENAME_ASKED_ENVIRONMENT`] is exactly `on`.
pub fn rename_asked_on() -> bool {
    std::env::var(RENAME_ASKED_ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// Whether what the person said gives a name ([`gives_a_name`]) that is not
/// the one the eyes know them by, while they are the one face at the lamp:
/// `known` is the known file's name, `faces` the faces in the eyes' newest
/// fresh look. "Call me Tove." from the one face known as Kess does; their
/// own name ("I'm Kess."), an unknown face, no fresh look or two faces don't.
/// The call this forces asks first (the eyes' `known_as`, [`told_asking`]):
/// nothing is renamed without their yes.
pub fn gives_another_name(said: &str, known: Option<&str>, faces: Option<usize>) -> bool {
    faces == Some(1) && known.is_some_and(|known| gives_a_name(said) && !said_by_them(known, said))
}

/// What the model is told beside a request in which the person gave their
/// name, with names checked: a name told is a name kept (the first builder,
/// #705, 13:43: on 15 October the lamp only said it would remember).
/// A condition, so that a match that was no name ("I'm Irish.") costs
/// nothing (the reviewer on #1057).
pub const NAME_TOLD_NOTE: &str = "If they have just told you their own name, call remember_my_face with it now, as they said it, or if they spelled it, as the name the letters make, written as a word. If you are not sure of it, say it back as a name and ask if you have it right; ask them to spell it only if they say it is wrong, and only once. Never only say that you will remember it. If what they said was not their name, do nothing about it.";

/// What the model is told when the name is not in what they said: a
/// spelling is asked for once ([`not_heard`]).
const NOT_HEARD: &str = "You did not hear that name in what they said. Say the name back to them and ask them to spell it, then call this again with the name as they said it, or as the name their letters make, written as a word, never as letters.";
/// What the model is told when the name is not in what they said after
/// they were asked to spell it ([`not_heard`]): never asked again
/// (lamp-3e83, #5564: the lamp asked for spellings too often).
const NOT_HEARD_SPELLED: &str = "You did not hear that name in what they said, and they were already asked to spell it, so do not ask them to spell it again. Say back the name their letters make, as a name only, never its letters and never as a choice between the name and its spelling, and ask if you have it right. Only if they say yes, call this again with that name.";
/// What the model is told when the name is not in what they said, they
/// were asked to spell it, and gave no letters ([`not_heard`]): a name
/// they never said or spelled can't pass a read-back's yes, so it asks for
/// the name once more, never a spelling again and never a name read back
/// (Jarvis's P2 on #5580).
const NOT_HEARD_SAY_AGAIN: &str = "You did not hear that name in what they said, and they were already asked to spell it, so do not ask them to spell it again, and do not say a name back to them. Ask them to say their name once more, then call this again with the name as they say it.";
/// What the model is told at setup, before the first save, when the name
/// was not said back and agreed to (#5140 row 5).
/// What a speaker-bound first save at setup is told without the speaker's
/// own leave for their face ([`speaker_gave_face_leave`],
/// `LAMP_REMEMBER_SPEAKER`, B3472 after #3472): a name alone is no yes to
/// keeping a face.
pub(crate) const FACE_LEAVE: &str = "I have not kept their face yet: a name alone is no yes to keeping a face. Ask them \"May I remember your face, so I know you next time?\" and only if they say yes with their name in that same answer, call this again.";
/// With `LAMP_FACE_NO_ASK`: beside the first save at setup the eyes kept
/// (`remembered`) on the speaker's own proof ([`run_speaker`]), the one
/// line the lamp says ([`setup_no_ask_told`]).
pub(crate) const SETUP_KEPT_NO_ASK: &str = "Their face is kept, and nothing was asked of them. Say this, in their language: \"I'll know you next time. Say 'forget my face' to undo.\" Ask them nothing about their face or their name.";
/// With `LAMP_FACE_NO_ASK`: the first save at setup with no proof that the
/// face in view said the name (no face seen saying it, another person's
/// name, a save off the speaker-bound route), or whose face the eyes could
/// not see alone: nothing kept, nothing asked, and the step ends with the
/// name only ([`setup_no_ask_told`], [`setup_no_ask_speaker`]).
pub(crate) const SETUP_NOT_KEPT_NO_ASK: &str = "Their face was not kept, and nothing was asked of them. Ask them nothing about their face, don't tell them to try again, and never say you'll know them or that you remembered their face. Call setup_step with step \"who_you_are\" done, since you know their name.";
pub(crate) const SAY_IT_BACK: &str = "I have not kept their face yet. First say the name back to them and ask if you have it right, the name as a word only, never its letters and never as a choice between the name and its spelling. Only if they say yes, call this again with that name; if they correct it, say the new one back the same way.";
/// What the model is told when the name was only the word that called the
/// lamp ([`only_the_call`]).
const ONLY_THE_CALL: &str = "That word was them calling you, not their name, so I have not remembered anyone. Ask them what their name is, then call this again with the name they give.";
const NOT_OWN_NAME: &str = "I could not confirm that this was their own name, so I have not remembered anyone. Ask them to look at me and state their own name directly, or spell their name by itself, then try again.";
const NAME_UNCHANGED: &str = "I cannot safely replace a saved name from this request, so I have not changed anyone's saved face.";
/// What the model is told when it would replace a name before the person
/// agreed to the new one.
/// With `LAMP_NAME_LEARN_FIX` (#7331): a new face not kept for want of
/// the person's yes to keeping it ([`run_switched`]).
/// What the model is told when the speaker's own line asked to keep their
/// face out ([`keeps_face_out`]): nothing kept, nothing to ask.
pub(crate) const FACE_KEPT_OUT: &str = "They asked to keep their face out: it was not kept. Don't ask them about their face, don't tell them to try again, and never say you'll know them; just say that's fine and carry on.";
pub(crate) const ASK_FACE_FIRST: &str = "I have not kept their face: a name is not a yes to keeping a face. Ask them only this, with their name: \"Shall I remember your face as <their name>?\" Call this again only after their yes; if they say no, keep nothing and say that's fine.";
/// With `LAMP_FACE_NO_ASK`: a face the eyes could not see alone (more than
/// one face, none, too far, the eye shut) for a save the person did not ask
/// for in words ([`no_ask_told`]): nothing kept, and nothing asked.
pub(crate) const NOT_SEEN_NO_ASK: &str = "Their face was not kept: I could not see only them clearly. Nothing was asked of them, so ask them nothing about it, don't tell them to try again, and never say you'll know them; just carry on with what they said.";
/// With `LAMP_FACE_NO_ASK`: a new face with no proof it is the speaker's,
/// past setup, the person not having asked for it ([`no_ask_unkept`]):
/// nothing kept, nothing asked, nothing said about it.
pub(crate) const NOT_KEPT_NO_ASK: &str = "Their face was not kept, and nothing was asked of them. Ask them nothing about it and say nothing about their face; just carry on with what they said.";
/// With `LAMP_FACE_NO_ASK`: the same, when they asked in words for their
/// face to be remembered ([`no_ask_unkept`]): the plain reason, nothing asked.
pub(crate) const CANT_KEEP_NO_ASK: &str = "Their face was not kept: I could not see that the face in view was the one saying it. Tell them that plainly in a few words, in their language. Ask them nothing, and don't ask them to try again.";
/// With `LAMP_FACE_NO_ASK`: beside a face kept with nothing asked, the one
/// line the lamp says ([`no_ask_told`]).
pub(crate) const KEPT_NO_ASK: &str = "Their face is kept, and nothing was asked of them. Say this, in their language: \"I'll know you next time. Say 'forget my face' to undo.\" Ask them nothing.";
/// With `LAMP_FACE_NO_ASK` and Who was here's names on (the first builder
/// 19:47): in [`ASK_TO_NAME`]'s place, once their yes to being named was
/// taken with nothing asked ([`no_ask_visits`]), the one undo line.
pub(crate) const VISITS_UNDO: &str = "Also say once, in their language: \"I'll mention you stopped by. Say 'keep my visits private' to undo.\" Ask them nothing about it.";
/// With `LAMP_NAME_LEARN_FIX` (#7331): what the model is told beside a
/// name kept from letters spelled to it ([`spelled_back`]).
pub(crate) const SPELLED_KEPT: &str = "Say say_back exactly as it is, a statement: the name, its letters, and that you've got it. Ask nothing more about the name. If their next line fixes a letter (\"No, N as in nose\") or spells it again, call this again with replace true and the name that makes: that line is their yes, so don't ask again.";
pub(crate) const NOT_AGREED: &str = "They have not said yes to that name yet. Say the new name back and ask whether they want to be known by it instead. Only if they say yes, call this again with the new name and replace true.";
/// What the model is told when the face is near someone known by a name one
/// letter from the one heard (#2176 row 4): the name may be misheard, so
/// they spell it, and no known name is said back. The spelled name goes with
/// `apart`, whatever the first call held, so a sibling heard as exactly a
/// known name is asked once, not twice (Reviewer-lead's P3 on #2252), and
/// the eyes still judge a known name's face. `apart` is taken only when the
/// turn in hand spells the name ([`spelled_by_them`]): a name said again, or
/// a yes, goes without it and is asked about again (both reviewers' P2 on
/// #2269).
const NEAR_TO: &str = "You may have misheard their name. Do not say any name you know. Ask them to spell their name, letter by letter, then call this again with the name their letters make, written as a word, never as letters, and apart true.";
/// What the model is told when the face can't be kept under that name (it
/// is someone's the lamp knows, and this face isn't near theirs). It says
/// nothing of who the lamp knows, and no yes changes it (the reviewer's P1
/// and P2 on #2211).
const NOT_LIKE_THEM: &str = "You cannot keep this face under that name. Do not say why, and do not say whether you know anyone by it. Ask them for another name they would like to be known by. The lamp's owner can remove an old face in the app.";
/// Remembering looks for up to four seconds.
pub const DEADLINE: Duration = Duration::from_secs(6);
/// The known file and its face count while they are this fresh: twice the
/// eyes' check, which renews both while the person is there
/// ([`lamp_msgs::known::FRESH`], #7146).
pub(crate) const FRESH: Duration = lamp_msgs::known::FRESH;
const MOST_NAME_BYTES: usize = 120;

/// With [`FaceSwitches::face_question`] and names checked, before
/// [`QUESTION_NOT_ASK`] (unchecked, the description asks for a told name,
/// so this would contradict it): a
/// clear ask is a call in any turn, also after their face was kept earlier
/// in the conversation, or before they said their name (#8053, row-8053q on
/// lamp-52e6, 21 Nov: such asks called it 17 of 30 after a save, and an ask
/// with no name 0 of 3 to 3 of 3 by line after a greeting). Both are
/// answered here before any look: a face known by that name is told so
/// ([`face_question_first`]), and no name is told [`NO_NAME`]
/// ([`run_switched`]). Never judged by words here.
const ASK_IS_A_CALL: &str = " Call it for every clear ask to remember them, in any turn: also when you kept \
     their face earlier in this conversation (it tells you if you already know them), and before they have \
     said their name (call it with an empty name: it says there was no name, so then ask for it).";

/// With [`FaceSwitches::face_question`], at the end of [`REMEMBER`]'s
/// description, checked or not: a question whether the lamp knows them is
/// no ask. The model's call is never judged by its words here.
const QUESTION_NOT_ASK: &str = " A question of whether you know or remember them (\"Do you remember me?\", \
     \"Did you remember my face?\", \"Do you know my face?\") is not an ask: answer it from who you see now, \
     and never call this for it.";

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
        rename_yes: false,
        name_learn_fix: false,
        face_no_ask: false,
        face_save_known_only: false,
        face_question: false,
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
            .to_owned()
                + if switches.face_question && checked {
                    ASK_IS_A_CALL
                } else {
                    ""
                }
                + if switches.face_question {
                    QUESTION_NOT_ASK
                } else {
                    ""
                },
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
        tools.extend(visits_tools(switches.face_no_ask));
    }
    tools
}

/// With `no_ask` (`LAMP_FACE_NO_ASK`), the no also names the undo the lamp
/// says ([`VISITS_UNDO`]): "keep my visits private".
fn visits_tools(no_ask: bool) -> [Tool; 2] {
    let dont_tell = if no_ask {
        "Call this when someone you know says you should not tell others when they've \
         been here, asks you to keep their visits private (\"keep my visits private\"), or \
         takes back a yes to it (\"don't tell anyone I was here\"). Then tell them briefly \
         that you'll only say someone you know was here."
    } else {
        "Call this when someone you know says you should not tell others when they've \
         been here, or takes back a yes to it (\"don't tell anyone I was here\"). Then \
         tell them briefly that you'll only say someone you know was here."
    };
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
            description: dont_tell.into(),
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
            rename_yes: false,
            name_learn_fix: false,
            face_no_ask: false,
            face_save_known_only: false,
            face_question: false,
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
    // With `LAMP_FACE_QUESTION_NOT_ASK`, before the speaker's own checks:
    // a face known by this name, only when the line's own speaking face is
    // that known face ([`said_by_known_face`]). The confirmed name is never
    // reused here: this route keeps a face only on the speaker's own words
    // ([`reused_name`] is `run_switched`'s).
    if switches.face_question
        && said_by_known_face(line)
        && let Some(name) = call
            .args
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
        && let Some(told) = face_question_first(socket, call, switches, name)
    {
        return record_told(socket, call, told, switches, person, line);
    }
    let mut told = run_speaker(socket, call, switches, line, request, lease, replaced);
    if switches.face_no_ask && !switches.at_setup {
        told = no_ask_speaker(socket, told, call, line, request);
    }
    if switches.face_no_ask && switches.at_setup {
        told = setup_no_ask_speaker(told, call);
    }
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
    // "Keep my face out of this, I'm Kesh.": nothing kept and nothing asked,
    // at setup or past it, with `LAMP_FACE_NO_ASK` or without (Heron's HOLD
    // on #7622), before the name's own checks: a line like that may not
    // read as a plain self-introduction. At setup the step then ends with
    // the name only ([`SETUP_NOT_KEPT_NO_ASK`]). Not a rename, whose face is
    // already kept.
    let renaming = call.args.get("replace").and_then(Value::as_bool) == Some(true);
    if keeps_face_out(&said.this) && !renaming {
        let error = if switches.at_setup {
            SETUP_NOT_KEPT_NO_ASK
        } else {
            FACE_KEPT_OUT
        };
        return (json!({"error": error}), true);
    }
    if !name_heard(name, &said) {
        return (json!({"error": NOT_HEARD}), true);
    }
    if !speaker_named_self(name, &said.this) {
        return (json!({"error": NOT_OWN_NAME}), true);
    }
    // The first save at setup keeps a face only with the speaker's own
    // leave for it in this line: a name alone is no yes to the face (B3472,
    // after #3472). The lamp's last line is its own words, never a name.
    // With `LAMP_FACE_NO_ASK` (the product owner's no-asks rule, 18 Nov)
    // the face question is never asked, so the proof is this line's alone:
    // the eyes saw this face say its own name ([`setup_no_ask_told`]).
    if switches.at_setup && !switches.face_no_ask {
        let lamp = crate::thread::THREAD.lock().ok().and_then(|thread| {
            thread
                .names_window(1)
                .before
                .last()
                .map(|(_, lamp)| lamp.clone())
        });
        if !speaker_gave_face_leave(&said.this, lamp.as_deref()) {
            return (json!({"error": FACE_LEAVE}), true);
        }
    }
    let replace = call.args.get("replace").and_then(Value::as_bool) == Some(true);
    // Past setup, with `LAMP_NAME_LEARN_FIX` (#7331) and without
    // `LAMP_FACE_NO_ASK`, a new face needs the same leave, else
    // `ASK_FACE_FIRST` (Heron's P1 on #7622: this path never read the
    // switch). Not for a rename, a line correcting the letters the lamp just
    // spelled back, or a speaker the eyes already know: the known face
    // beside their known file as the line was heard is the proof's own (the
    // first builder's P2 on #7622), whose "Call me Jay." goes to the eyes
    // and their rename's own yes. With `LAMP_FACE_NO_ASK` the speaker-bound
    // proof replaces the ask: a face seen saying "I'm X" is kept, nothing
    // asked (the first builder's ruling on #7622, 18 Nov 23:10).
    if !switches.at_setup && switches.name_learn_fix && !switches.face_no_ask && !replace {
        let lamp = crate::thread::THREAD.lock().ok().and_then(|thread| {
            thread
                .names_window(1)
                .before
                .last()
                .map(|(_, lamp)| lamp.clone())
        });
        let corrected = lamp.as_deref().is_some_and(|lamp| {
            spelled_back_in(lamp).is_some()
                && corrects_spelled_back(
                    name,
                    &Said {
                        this: said.this.clone(),
                        before: vec![(String::new(), lamp.to_owned())],
                    },
                )
        });
        let speaker_known = line
            .and_then(|line| line.known.as_ref())
            .is_some_and(|(_, face)| *face == proof.face);
        if !corrected && !speaker_known && !speaker_gave_face_leave(&said.this, lamp.as_deref()) {
            return (json!({"error": ASK_FACE_FIRST}), true);
        }
    }
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
            let mut told = told_asking(&answer, switches.visits);
            // With `LAMP_FACE_NO_ASK`, past setup: kept said in one line,
            // nothing asked (the speaker's own line already named them).
            if switches.face_no_ask && !switches.at_setup && !replace {
                told = no_ask_told(told, &answer, &said.this);
            }
            // At setup: kept on that proof, the one line; not seen alone,
            // nothing kept and nothing asked.
            if switches.face_no_ask && switches.at_setup && !replace {
                told = setup_no_ask_told(told, &answer);
            }
            // Past setup only, as in `run_switched`.
            if switches.face_no_ask && switches.visits && !switches.at_setup {
                told = no_ask_visits(socket, told);
            }
            // With `LAMP_FACE_QUESTION_NOT_ASK`, known already (lamp-74ff).
            (told_already(told, &answer, switches, replace), true)
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
        (NOT_HEARD_SPELLED, "not_heard_spelled"),
        (NOT_HEARD_SAY_AGAIN, "not_heard_say_again"),
        (ONLY_THE_CALL, "only_the_call"),
        (NOT_OWN_NAME, "not_own_name"),
        (FACE_LEAVE, "face_leave"),
        (ASK_FACE_FIRST, "ask_face_first"),
        (FACE_KEPT_OUT, "face_kept_out"),
        (NOT_SEEN_NO_ASK, "not_seen_no_ask"),
        (SETUP_NOT_KEPT_NO_ASK, "setup_not_kept_no_ask"),
        (NOT_KEPT_NO_ASK, "not_kept_no_ask"),
        (CANT_KEEP_NO_ASK, "cant_keep_no_ask"),
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
            rename_yes: false,
            name_learn_fix: false,
            face_no_ask: false,
            face_save_known_only: false,
            face_question: false,
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

/// With `LAMP_FACE_QUESTION_NOT_ASK`, before any name check, on either
/// save route ([`run_switched`], and the speaker-bound [`run_speaker`]'s
/// caller, only for the line's own speaking face): the face in view already
/// known by this name, a fresh real match ([`already_known`]), needs no
/// look and no save, whatever the line says (lamp-74ff, 19 Nov: "Did you
/// remember my face then?" right after the face was kept was answered that
/// the lamp couldn't take the look). Whether a line asks or only wonders is
/// the model's call, told in the description ([`QUESTION_NOT_ASK`]), never
/// judged here. `None` goes on as before. Never a rename, nor the setup
/// walk's first save.
fn face_question_first(
    socket: &Path,
    call: &ToolCall,
    switches: FaceSwitches,
    name: &str,
) -> Option<(Value, bool)> {
    let replacing = call.args.get("replace").and_then(Value::as_bool) == Some(true);
    if !switches.face_question || call.name != REMEMBER || replacing || switches.at_setup {
        return None;
    }
    let known = already_known(socket, name)?;
    eprintln!(
        "lamp-voice: remember_my_face for the face in view under the name it is known by: nothing sent (LAMP_FACE_QUESTION_NOT_ASK)"
    );
    Some((
        json!({"already_known": known, "note": ALREADY_KNOWN_NOTE}),
        true,
    ))
}

/// The speaker-bound save's line was said by the face the eyes' known file
/// names: its own speaking face ([`crate::remember_speaker::Proof`]) is the
/// known face read with it, as [`run_speaker`]'s own check. An off-camera
/// voice giving the in-view person's name is not.
fn said_by_known_face(line: Option<&crate::speaking_faces::LineSeen>) -> bool {
    line.is_some_and(|line| {
        line.enrollment
            .as_ref()
            .zip(line.known.as_ref())
            .is_some_and(|(proof, (_, face))| *face == proof.face)
    })
}

/// With `LAMP_FACE_QUESTION_NOT_ASK`: a face the eyes already kept under
/// this name, matched as theirs (their `already`), is said to be known
/// already, never as a first save, and the visits line said at the first
/// save is not said again (lamp-74ff). A rename (`replacing`) is never
/// told known already, but when the eyes replaced a kept name (their
/// `replaced` not empty) the visits line is dropped too: the person heard
/// it when that face was first kept. The rest of the answer (`ok`,
/// `remembered`, the other `also_say` lines) stays. A replace that
/// replaced nothing is a first save and keeps the line.
fn told_already(told: Value, answer: &Value, switches: FaceSwitches, replacing: bool) -> Value {
    if switches.face_question
        && replacing
        && replaced_of(answer).is_some_and(|replaced| !replaced.is_empty())
    {
        return without_visits_line(told);
    }
    if switches.face_question
        && !replacing
        && answer.get("already").and_then(Value::as_bool) == Some(true)
        && let Some(kept) = told["remembered"].as_str().map(str::to_owned)
    {
        return json!({
            "ok": true,
            "remembered": kept,
            "already_known": kept,
            "note": ALREADY_KNOWN_NOTE,
        });
    }
    told
}

/// `told` with [`VISITS_LINE`] taken out of its `also_say`, the leftover
/// lines trimmed; no `also_say` at all when nothing is left.
fn without_visits_line(mut told: Value) -> Value {
    let Some(also_say) = told["also_say"].as_str() else {
        return told;
    };
    let left = also_say.replace(VISITS_LINE, "").trim().to_owned();
    if let Some(fields) = told.as_object_mut() {
        if left.is_empty() {
            fields.remove("also_say");
        } else {
            fields.insert("also_say".into(), json!(left));
        }
    }
    told
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
    // A name written as its letters is kept, and so said back, as the word
    // they make: nobody calls a person by the spelling of their name
    // (lamp-3e83, #5564: "Jane or J-A-N-E?").
    // With `LAMP_NAME_LEARN_FIX` (#7331), two letters too once the lamp
    // asked for a spelling.
    let replacing = call.args.get("replace").and_then(Value::as_bool) == Some(true);
    if let Some(told) = face_question_first(socket, call, switches, name) {
        return told;
    }
    let spelled_rename = checked && switches.name_learn_fix && replacing && spelling_asked(said);
    // Right after the lamp said a spelled name back by its letters
    // ([`spelled_back`]), a line may correct it ([`corrects_spelled_back`]).
    let after_spelled_back = checked
        && switches.name_learn_fix
        && call.name == REMEMBER
        && said
            .before
            .last()
            .is_some_and(|(_, lamp)| spelled_back_in(lamp).is_some());
    let least = if spelled_rename || after_spelled_back {
        2
    } else {
        3
    };
    let joined = (checked && call.name == REMEMBER)
        .then(|| letters_as_word_of(name, least))
        .flatten();
    let name = joined.as_deref().unwrap_or(name);
    let corrected = after_spelled_back && corrects_spelled_back(name, said);
    let setup = checked && switches.at_setup && call.name == REMEMBER;
    // With `LAMP_NAME_LEARN_FIX`, a new face is kept only on the person's
    // leave: their own ask for it ("remember my face") or their yes to the
    // lamp's face question ("Shall I remember your face as Ada?",
    // [`speaker_gave_face_leave`]). A name given is no yes to keeping a
    // face (the first builder on #7331); setup asks its own face question.
    // Before the name's own checks, so a no is never asked to spell.
    // Not while the eyes' known file names a face at the lamp, the file the
    // second voice's rename force reads (`rename_asks`): a known face's
    // "Call me Jay." goes to the eyes, whose "known as" answer holds the
    // rename for their yes, and two faces are the eyes' own refusal
    // (row-7422c on lamp-52e6, #7653).
    // With `LAMP_FACE_NO_ASK`, this route keeps no new face at all: it has
    // no proof the face in view is the speaker's (`one_face` is only one
    // face in view; Thirteen's and Heron's HOLDs on #7787: a TV's "I'm
    // Anna", or a parent's "This is my son Tom" and a yes to "Tom,
    // right?"). A face is kept unasked only on the speaker-bound route
    // ([`run_speaker`]); here nothing is kept and nothing is asked (the
    // first builder 21:13), even with `LAMP_NAME_LEARN_FIX` on, only the
    // plain reason said to their own ask for their face ([`no_ask_unkept`]).
    // Not while the eyes' known file names a face at the lamp: a known
    // face's "Call me Jay." still goes to the eyes' "known as" and the
    // rename's own yes, as above (Heron's and Migrate's HOLDs on #7800).
    // With `LAMP_FACE_QUESTION_NOT_ASK`, a name confirmed here (said back
    // and agreed, or spelled) is kept for this session with the one near
    // face in view, before any refusal, for a later save of that face
    // ([`reused_name`]); a forget lets it go. Only with `LAMP_FACE_NO_ASK`
    // on too: without it this route has no speaking proof, so the reuse is
    // dark and the save goes as today (#8010 deferred, Friday's P3 on
    // #8026).
    let reuses = switches.face_question && switches.face_no_ask && checked && !switches.at_setup;
    if reuses && call.name == FORGET {
        forget_confirmed();
    }
    if reuses
        && call.name == REMEMBER
        && !replacing
        && (said_back_and_agreed(name, said) || spelled_by_them(name, &said.this) || corrected)
    {
        note_confirmed(name);
    }
    let lamp_before = said.before.last().map(|(_, lamp)| lamp.as_str());
    let new_face = checked && call.name == REMEMBER && !replacing && !setup && !corrected;
    if new_face && switches.face_no_ask && known_now(&known_file_of(socket)).is_none() {
        return (no_ask_unkept(Some(name), &said.this, lamp_before), true);
    }
    // A line keeping their face out ("Keep my face out of this, I'm
    // Kesh.") keeps nothing and asks nothing, never the face question it
    // just refused (Heron's HOLD on #7622).
    if new_face
        && switches.name_learn_fix
        && keeps_face_out(&said.this)
        && known_now(&known_file_of(socket)).is_none()
    {
        return (json!({"error": FACE_KEPT_OUT}), true);
    }
    let face_question_skipped =
        new_face && switches.name_learn_fix && !speaker_gave_face_leave(&said.this, lamp_before);
    if face_question_skipped && known_now(&known_file_of(socket)).is_none() {
        return (json!({"error": ASK_FACE_FIRST}), true);
    }
    // Skipped only because the known file named a face, which can still
    // name someone for its 3 s after a stranger steps in: with
    // `LAMP_FACE_SAVE_KNOWN_ONLY` the eyes keep it only for a face they
    // know, and their `not_known` is the face question (#7704).
    let known_only =
        face_question_skipped && switches.face_save_known_only && !switches.face_no_ask;
    // At setup, a name not heard is taken as they spelled it in the turn in
    // hand: the letters are their own words, not the model's guess (#5140
    // row 6, lamp-0c4e: a misheard name never passed this check).
    // With `LAMP_NAME_LEARN_FIX`, so is a rename's (#7331): on lamp-74ff a
    // rename spelled after the lamp asked for it was NOT_HEARD.
    let spelled;
    // With `LAMP_NAME_LEARN_FIX`, a name heard as its letter is the same
    // name for the checks ([`as_said`], #7331); off, as said.
    let (heard_name, heard_said) = if checked && switches.name_learn_fix {
        as_said(name, said)
    } else {
        (name.to_owned(), said.clone())
    };
    let heard = if checked && switches.rename_yes && replacing {
        heard_for_rename(&heard_name, &heard_said)
    } else {
        name_heard(&heard_name, &heard_said)
    } || (setup && said_back_and_agreed(&heard_name, &heard_said))
        || corrected
        || (reuses && call.name == REMEMBER && !replacing && reused_name(name));
    let name = if checked && call.name == REMEMBER && !heard {
        // Only after the lamp asked them to spell, and as many spelled runs
        // as the model's name has words: "L O R E N, and I'm at I B M" is
        // no one name (El's P2 on #5159).
        let words = folded_words(name).len();
        match ((setup && spelling_asked(said)) || spelled_rename)
            .then(|| spelled_name(&said.this))
            .flatten()
            .filter(|letters| letters.split_whitespace().count() == words)
        {
            Some(letters) => {
                spelled = letters;
                spelled.as_str()
            }
            None => return (json!({"error": not_heard(said)}), true),
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
    // With `LAMP_FACE_NO_ASK` (the product owner's no-asks rule, 18 Nov):
    // this route has no proof that the face in view is the one that said
    // the name (one face in view is not the speaker: a TV, or a second
    // person; the Reviewer-lead's HOLD and the first builder's ruling on
    // #7810). A face is kept unasked at setup only on the speaker-bound
    // route ([`run_speaker`]); here nothing is kept and nothing is asked,
    // not the read-back nor the face question, and the step ends with the
    // name only. The name's own checks above stay. No save is held.
    if setup && switches.face_no_ask {
        eprintln!(
            "lamp-voice: the setup face save has no speaking-face proof: nothing kept, nothing asked (LAMP_FACE_NO_ASK)"
        );
        return (json!({"error": SETUP_NOT_KEPT_NO_ASK}), true);
    }
    if setup
        && !(spelling_asked(said) && spelled_by_them(name, &said.this)
            || said_back_and_agreed(name, said)
            || corrected)
    {
        return (json!({"error": SAY_IT_BACK}), true);
    }
    let replace = call.args.get("replace").and_then(Value::as_bool) == Some(true);
    // A name replaces another only once the lamp said it back and they
    // said yes (the first builder on #1057: a misheard name was kept while
    // the person was still correcting it).
    // With `LAMP_NAMES_RENAME_YES` the yes is judged by its guards, not a
    // word list (#5485, lamp-29e7).
    // With `LAMP_NAME_LEARN_FIX`, a rename spelled by them in the turn in
    // hand after the lamp asked for a spelling is their yes, as at setup:
    // spelling is how a person fixes a misheard name (#7331).
    let agreed = (spelled_rename && spelled_by_them(name, &said.this))
        || corrected
        || if switches.rename_yes {
            rename_agreed(&heard_name, &heard_said)
        } else {
            agreed_to(&heard_name, &heard_said)
        };
    if checked && call.name == REMEMBER && replace && !agreed {
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
    let mut line = match call.name.as_str() {
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
    // Only when true, as `apart`: eyes from before #7704 refuse the field.
    if known_only && let Some(line) = line.as_object_mut() {
        line.insert("known_only".into(), json!(true));
    }
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
    // With `LAMP_NAME_LEARN_FIX`, a name kept from letters spelled to it,
    // after a spell ask or as a correction of a read-back, at setup and on
    // a rename alike, is said back as a statement ([`spelled_back`]).
    let kept_spelled = checked
        && switches.name_learn_fix
        && call.name == REMEMBER
        && (corrected || (spelling_asked(said) && spelled_by_them(name, &said.this)));
    match answer {
        Some(answer) => {
            *replaced = replaced_of(&answer);
            let mut told = told_asking(&answer, switches.visits);
            if kept_spelled
                && let Some(kept) = told["remembered"].as_str().map(str::to_owned)
                && let Some(told) = told.as_object_mut()
            {
                told.insert("say_back".into(), json!(spelled_back(&kept)));
                told.insert("note".into(), json!(SPELLED_KEPT));
            }
            // Past setup only: the setup walk's save, its visits ask too, is
            // the setup walk's own part.
            if switches.face_no_ask && switches.visits && !setup {
                told = no_ask_visits(socket, told);
            }
            // With `LAMP_FACE_QUESTION_NOT_ASK`, known already (lamp-74ff).
            (told_already(told, &answer, switches, replacing), true)
        }
        None => (
            json!({"error": "My eye is not answering right now, so I could not do that."}),
            false,
        ),
    }
}

/// What the model is told of a new face sent with nothing asked
/// (`LAMP_FACE_NO_ASK`, the speaker-bound route [`run_speaker`] only, past
/// setup), from the eyes' `answer`: kept, the one line to say
/// ([`KEPT_NO_ASK`]); not seen alone, when the person had not asked for
/// their face in `this`, their own words, nothing to ask or say about it
/// ([`NOT_SEEN_NO_ASK`]). Only the eyes' `remembered` is ever told kept;
/// any other answer is told as it was.
fn no_ask_told(mut told: Value, answer: &Value, this: &str) -> Value {
    if answer.get("remembered").and_then(Value::as_str).is_some() {
        if let Some(told) = told.as_object_mut() {
            let note = match told.get("note").and_then(Value::as_str) {
                Some(note) => format!("{note} {KEPT_NO_ASK}"),
                None => KEPT_NO_ASK.to_owned(),
            };
            told.insert("note".into(), json!(note));
        }
        return told;
    }
    if not_seen_alone(answer) && !speaker_gave_face_leave(this, None) {
        return json!({"error": NOT_SEEN_NO_ASK});
    }
    told
}

/// With `LAMP_FACE_NO_ASK`, past setup, a new face with no proof it is the
/// speaker's own (the first builder 21:13): nothing kept, nothing asked.
/// Their own ask for their face in `this`, their words, under a name they
/// gave as their own or as a yes to the lamp's face question, is told the
/// route's plain reason ([`CANT_KEEP_NO_ASK`]), as the eyes' own reason is
/// told on the speaker route ([`no_ask_told`]); anything else, nothing to
/// say about their face ([`NOT_KEPT_NO_ASK`]): a TV's "I'm Anna", a
/// parent's yes to "Tom, right?", or "Remember my face, this is Tom".
fn no_ask_unkept(name: Option<&str>, this: &str, lamp_before: Option<&str>) -> Value {
    let asked = speaker_gave_face_leave(this, lamp_before)
        && (lamp_before.is_some_and(asks_for_the_face)
            || name.is_none_or(|name| speaker_named_self(name, this)));
    json!({"error": if asked { CANT_KEEP_NO_ASK } else { NOT_KEPT_NO_ASK }})
}

/// [`run_speaker`]'s answer with `LAMP_FACE_NO_ASK`, past setup: its
/// refusals for want of the face seen saying their own name
/// ([`crate::remember_speaker::NOT_SEEN`], [`NOT_OWN_NAME`]), which ask
/// them to look and say it again, are told as [`no_ask_unkept`]: nothing
/// asked. Their words are the request's own when seen, else the turn's.
/// Never a rename (`replace`) or a face the eyes' known file names at the
/// lamp: a known speaker's "Call me Jay." keeps its own answer, never a
/// "not kept" (Heron's and Migrate's HOLDs on #7800).
fn no_ask_speaker(
    socket: &Path,
    told: (Value, bool),
    call: &ToolCall,
    line: Option<&crate::speaking_faces::LineSeen>,
    request: u64,
) -> (Value, bool) {
    let error = told.0.get("error").and_then(Value::as_str);
    if !error
        .is_some_and(|error| error == crate::remember_speaker::NOT_SEEN || error == NOT_OWN_NAME)
    {
        return told;
    }
    let replace = call.args.get("replace").and_then(Value::as_bool) == Some(true);
    if replace || known_now(&known_file_of(socket)).is_some() {
        return told;
    }
    let window = crate::thread::THREAD
        .lock()
        .map(|thread| thread.names_window(1))
        .unwrap_or_default();
    let this = line
        .filter(|line| line.request == request)
        .and_then(|line| line.enrollment.as_ref())
        .map_or(window.this.as_str(), |proof| proof.heard.as_str());
    let lamp_before = window.before.last().map(|(_, lamp)| lamp.as_str());
    let name = call.args.get("name").and_then(Value::as_str).map(str::trim);
    (no_ask_unkept(name, this, lamp_before), told.1)
}

/// Whether the eyes' answer to a remembering is that they could not see one
/// face alone, clearly: two faces, none, too far, or the eye shut.
fn not_seen_alone(answer: &Value) -> bool {
    matches!(
        answer.get("error").and_then(Value::as_str),
        Some("no_face" | "too_far" | "two_faces" | "eyes_shut")
    )
}

/// What the model is told of the first save at setup with nothing asked
/// (`LAMP_FACE_NO_ASK`), sent on the speaker's own proof ([`run_speaker`]),
/// from the eyes' `answer`: kept (the eyes' `remembered` only), the one
/// line to say ([`SETUP_KEPT_NO_ASK`]); not seen alone, nothing kept and
/// nothing to ask or claim ([`SETUP_NOT_KEPT_NO_ASK`]). Any other answer is
/// told as it was.
fn setup_no_ask_told(mut told: Value, answer: &Value) -> Value {
    if answer.get("remembered").and_then(Value::as_str).is_some() {
        if let Some(told) = told.as_object_mut() {
            let note = match told.get("note").and_then(Value::as_str) {
                Some(note) => format!("{note} {SETUP_KEPT_NO_ASK}"),
                None => SETUP_KEPT_NO_ASK.to_owned(),
            };
            told.insert("note".into(), json!(note));
        }
        return told;
    }
    if not_seen_alone(answer) {
        return json!({"error": SETUP_NOT_KEPT_NO_ASK});
    }
    told
}

/// [`run_speaker`]'s answer at setup with `LAMP_FACE_NO_ASK`: its refusals
/// for want of the face seen saying their own name
/// ([`crate::remember_speaker::NOT_SEEN`]: a face seen silent while another
/// voice said it, a TV, nobody seen speaking; [`NOT_OWN_NAME`]: "This is
/// my son Tom"), which ask them to look and say it again, keep nothing and
/// ask nothing ([`SETUP_NOT_KEPT_NO_ASK`]): the step ends with the name
/// only (the first builder's ruling on #7810). Never a rename (`replace`),
/// which keeps its own answer.
fn setup_no_ask_speaker(told: (Value, bool), call: &ToolCall) -> (Value, bool) {
    let error = told.0.get("error").and_then(Value::as_str);
    if !error
        .is_some_and(|error| error == crate::remember_speaker::NOT_SEEN || error == NOT_OWN_NAME)
    {
        return told;
    }
    if call.args.get("replace").and_then(Value::as_bool) == Some(true) {
        return told;
    }
    eprintln!(
        "lamp-voice: the setup face save had no speaking-face proof: nothing kept, nothing asked (LAMP_FACE_NO_ASK)"
    );
    (json!({"error": SETUP_NOT_KEPT_NO_ASK}), told.1)
}

/// With `LAMP_FACE_NO_ASK` and Who was here's names on (the first builder
/// 19:47: [`ASK_TO_NAME`] is in the no-asks rule): a remembering the model
/// would be told to ask about ([`told_asking`]) is not asked. The voice
/// gives the eyes their yes itself, as [`TELL`] would (`visits_yes`, which
/// the eyes take only for that face at the lamp now), and in the question's
/// place the model says the undo line once ([`VISITS_UNDO`]); "keep my
/// visits private" is [`DONT_TELL`], as any no. If the eyes don't take it,
/// nothing is asked and nothing is said about it. Any other answer is told
/// as it was.
fn no_ask_visits(socket: &Path, mut told: Value) -> Value {
    let Some(name) = told["remembered"].as_str().map(str::to_owned) else {
        return told;
    };
    let Some(also_say) = told["also_say"].as_str().map(str::to_owned) else {
        return told;
    };
    if !also_say.contains(ASK_TO_NAME) {
        return told;
    }
    let taken = crate::tools::exchange(socket, &json!({"visits_yes": name}), VISITS_DEADLINE)
        .is_some_and(|answer| visits_told(&answer, true).get("ok").is_some());
    eprintln!(
        "lamp-voice: being named in Who was here {} with nothing asked (LAMP_FACE_NO_ASK)",
        if taken { "taken" } else { "not taken" }
    );
    let counted = also_say.contains(VISITS_LINE);
    let also_say = match (counted, taken) {
        (true, true) => Some(format!("{VISITS_LINE} {VISITS_UNDO}")),
        (false, true) => Some(VISITS_UNDO.to_owned()),
        (true, false) => Some(VISITS_LINE.to_owned()),
        (false, false) => None,
    };
    if let Some(told) = told.as_object_mut() {
        match also_say {
            Some(also_say) => told.insert("also_say".into(), json!(also_say)),
            None => told.remove("also_say"),
        };
    }
    told
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
/// How the model is told the face is known by another name, around that
/// name ([`told_asking`], read back by [`rename_asked`]).
const KNOWN_AS_OPENS: &str = "You already know this face as ";
const KNOWN_AS_ENDS: &str = ". Say the new name back and ask whether they want to be known by it instead. Only if they say yes, call this again with the new name and replace true.";

/// What the model is told when the eyes know the face as `known`.
pub(crate) fn told_known_as(known: &str) -> Value {
    json!({"error": format!("{KNOWN_AS_OPENS}{known}{KNOWN_AS_ENDS}")})
}

/// Whether a `remember_my_face` answer, as the model is told it, has the
/// lamp ask the person whether to be known by the new name instead: the
/// face is known by another name (the eyes' `known_as`), given here, or a
/// `replace` came before their yes ([`NOT_AGREED`]), with no old name
/// known. What a rename is held for (`second_provider::HeldRename`).
pub(crate) fn rename_asked(told: &Value) -> Option<Option<String>> {
    let error = told["error"].as_str()?;
    if error == NOT_AGREED {
        return Some(None);
    }
    error
        .strip_prefix(KNOWN_AS_OPENS)?
        .strip_suffix(KNOWN_AS_ENDS)
        .map(|known| Some(known.to_owned()))
}

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
    // Asked to keep only a face they know (#7704), the eyes knew nobody:
    // the face question, nothing kept.
    if answer.get("error").and_then(Value::as_str) == Some("not_known") {
        return json!({"error": ASK_FACE_FIRST});
    }
    if answer.get("error").and_then(Value::as_str) == Some("known_as")
        && let Some(known) = answer.get("known_as").and_then(Value::as_str)
    {
        return told_known_as(known);
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

/// The names of everyone the eyes know, as last read for
/// `LAMP_NAME_NOT_ANOTHERS` (#8114, the first builder's 10:52): a word
/// written for the lamp's name that is one of them ("Lynn") calls that
/// person, not the lamp. Empty until read, or without the switch.
static FACE_NAMES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// How many owner refreshes ([`OWNER_EVERY`]) pass between reads of
/// [`FACE_NAMES`]: about a minute.
const FACE_NAMES_EVERY: u32 = 6;

/// Sets [`FACE_NAMES`] as a read of the eyes would, for tests.
#[cfg(test)]
pub(crate) fn keep_names_for_test(names: Vec<String>) {
    *FACE_NAMES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = names;
}

/// The names in [`FACE_NAMES`]: those the eyes knew at the last read. Never
/// waits.
pub fn names_kept() -> Vec<String> {
    FACE_NAMES
        .try_lock()
        .map(|names| names.clone())
        .unwrap_or_default()
}

fn keep_owner_fresh_now(file: Option<PathBuf>, socket: Option<PathBuf>) {
    std::thread::spawn(move || {
        let mut count_tries = 0;
        let mut tick: u32 = 0;
        loop {
            // The faces' names for `LAMP_NAME_NOT_ANOTHERS`, about once a
            // minute; eyes that don't answer keep the last read.
            if crate::second_provider::not_anothers_on()
                && tick.is_multiple_of(FACE_NAMES_EVERY)
                && let Some(Some(names)) = socket.as_deref().and_then(names_reached)
            {
                *FACE_NAMES
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = names;
            }
            tick = tick.wrapping_add(1);
            if count_tries < OWNER_COUNT_TRIES {
                count_tries = if owner_counted(file.as_deref(), socket.as_deref()) {
                    OWNER_COUNT_TRIES
                } else {
                    count_tries + 1
                };
            }
            refresh_owner(file.as_deref(), socket.as_deref());
            std::thread::sleep(OWNER_EVERY);
        }
    });
}

/// How many refreshes [`owner_counted`] is tried at, at most, while the
/// eyes don't answer their names: five minutes at [`OWNER_EVERY`].
const OWNER_COUNT_TRIES: u32 = 30;

/// The start-up line that counts a lamp whose owner is the typed name only
/// (`owner_link::startup_line`, #7501), said once the eyes answer their
/// names; whether they did (none to ask is done too).
fn owner_counted(file: Option<&Path>, socket: Option<&Path>) -> bool {
    let Some(socket) = socket else {
        return true;
    };
    let Some(Some(names)) = names_reached(socket) else {
        return false;
    };
    let kept = file.and_then(crate::onboarding::owner_of);
    if let Some(line) =
        crate::owner_link::startup_line(crate::person::now().as_deref(), kept.as_deref(), &names)
    {
        eprintln!("{line}");
    }
    true
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
        (Some(owner), Some(Some(names))) if among(&names, &owner) => {
            // Their face known again (their own name kept, #8471): from
            // now a list without them is the owner gone, as #4290 has it.
            if let Some(file) = file {
                owner_face_seen(file, &owner);
            }
            Some(owner)
        }
        // The eyes knowing nobody (an empty print store) is not the owner
        // gone: the mark stays in the walk's file, through a restart too
        // (#8471's lamp run, the first builder's 14:01 call, a deliberate
        // narrowing of #4290's drop to a list the owner is not in). Until
        // their face is seen again, a first face with another name is not
        // the owner gone either (the first builder's 17:50 call).
        (Some(owner), Some(Some(names))) if names.is_empty() => {
            if let Some(file) = file {
                owner_face_lost(file, &owner);
            }
            None
        }
        (Some(_), Some(Some(_))) if file.is_some_and(owner_face_unknown) => None,
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

/// Walks' files whose owner's face the eyes have not known since they knew
/// nobody, as this process saw it: the walk's file keeps it too
/// ([`crate::onboarding::owner_face_unseen`]), read once the walk takes
/// its mailbox, so a refresh before then drops nothing either.
static OWNER_FACE_UNKNOWN: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Whether the owner of the walk's file at `file` has had no face the eyes
/// know since they knew nobody.
fn owner_face_unknown(file: &Path) -> bool {
    OWNER_FACE_UNKNOWN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .any(|kept| kept == file)
        || crate::onboarding::owner_face_unseen(file)
}

/// The eyes know nobody: `owner`'s face is unknown until seen again, told
/// the walk once.
fn owner_face_lost(file: &Path, owner: &str) {
    let mut unknown = OWNER_FACE_UNKNOWN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if unknown.iter().any(|kept| kept == file) {
        return;
    }
    unknown.push(file.to_owned());
    drop(unknown);
    if !crate::onboarding::owner_face_unseen(file) {
        eprintln!(
            "lamp-voice: the eyes know nobody: the owner is kept, their face unknown until seen again"
        );
        crate::onboarding::owner_face(file, owner, true);
    }
}

/// `owner`'s face known to the eyes again: told the walk once.
fn owner_face_seen(file: &Path, owner: &str) {
    OWNER_FACE_UNKNOWN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .retain(|kept| kept != file);
    if crate::onboarding::owner_face_unseen(file) {
        eprintln!("lamp-voice: the eyes know the owner's face again");
        crate::onboarding::owner_face(file, owner, false);
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

/// A face this tall, as a share of the picture's height, is near the lamp:
/// the eyes' own near rule (`lamp_eyes::who::NEAR_ENOUGH`, 0.10; people at
/// lamp-4ace were 0.12 to 0.27). With `LAMP_FACE_STICKY`, only near faces
/// count toward the by-default name's "more than one face" and "an unknown
/// face in view" ([`most_near_faces_lately`]).
pub const NEAR_ENOUGH: f32 = 0.10;

/// The faces of one look [`count_at_desk`] counts that are also near
/// ([`NEAR_ENOUGH`] tall).
pub fn count_near_at_desk(faces: &lamp_msgs::Faces, desk_only: bool) -> usize {
    at_desk(&faces.faces, desk_only)
        .into_iter()
        .zip(&faces.faces)
        .filter(|(counted, face)| *counted && face.ymax - face.ymin >= NEAR_ENOUGH)
        .count()
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

/// [`FACES_LATELY`] counting near faces only ([`count_near_at_desk`]).
static NEAR_FACES_LATELY: std::sync::Mutex<FacesLately> =
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
                        look.clone(),
                        count_at_desk(&faces, desk_only),
                        faces_told(&faces, desk_only),
                    );
                    drop(lately);
                    NEAR_FACES_LATELY
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .look(at, look, count_near_at_desk(&faces, desk_only));
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

/// [`most_faces_lately`] counting only near faces ([`NEAR_ENOUGH`]): a tiny
/// face far in the background is no one at the lamp. For the by-default
/// name only (`person::someone_else_now`, with `LAMP_FACE_STICKY`); the
/// private read-backs' "owner alone in view" checks count every face
/// ([`most_faces_lately`]). On lamp-74ff (19 Nov 12:07:29) the owner's
/// name was held back for a second face 0.08 tall behind them.
pub fn most_near_faces_lately() -> Option<usize> {
    NEAR_FACES_LATELY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .most(std::time::Instant::now())
}

/// The faces counted for the by-default name: near ones only with
/// `sticky` ([`most_near_faces_lately`]), every one without.
pub fn faces_for_name(sticky: bool) -> Option<usize> {
    if sticky {
        most_near_faces_lately()
    } else {
        most_faces_lately()
    }
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
/// read, then 3 s ([`FRESH`] since #7146), read a file rewritten every 2–3 s as nobody (lamp-52e6, 29 Oct
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

/// Only the exact value `on`: the sticky identity plan's voice part
/// (`docs/face-recognition-sticky-identity-plan.md`). The eyes' held mark
/// beside the known file (`lamp-eyes`'s `who::known_held_of`, written while
/// they keep a name by a hold and not by a match, with their own
/// `LAMP_FACE_STICKY`) is read: a line whose memory scope comes from a held
/// name reads and keeps as theirs with their face in view, never runs a
/// forget, correction, rule or yes, and a held name never counts as the
/// owner present or alone for a private read-back ([`held_now`]). With it,
/// a lone face's late match may move a room conversation's unseen lines
/// (`follows_face::theirs_alone`), and a carried line read as theirs is
/// journalled in their record. Off, nothing reads the mark.
pub const STICKY_ENVIRONMENT: &str = "LAMP_FACE_STICKY";

/// [`STICKY_ENVIRONMENT`], read once for the process.
pub fn sticky_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| sticky_from(|name| std::env::var(name).ok()))
}

/// [`STICKY_ENVIRONMENT`] as `var` gives it: on only for exactly `on`.
pub fn sticky_from(var: impl Fn(&str) -> Option<String>) -> bool {
    var(STICKY_ENVIRONMENT).as_deref() == Some("on")
}

/// The eyes' held mark beside the known file `known`: `-held`.
pub fn known_held_of(known: &Path) -> PathBuf {
    let mut beside = known.as_os_str().to_owned();
    beside.push("-held");
    PathBuf::from(beside)
}

/// Whether the eyes' held mark beside `known` is fresh: written within
/// [`FRESH`], as the known file is. Read whatever the switch; callers gate.
pub fn held_mark(known: &Path) -> bool {
    std::fs::metadata(known_held_of(known))
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|written| SystemTime::now().duration_since(written).ok())
        .is_some_and(|age| age < FRESH)
}

/// With [`STICKY_ENVIRONMENT`], whether the name in the eyes' known file
/// (`LAMP_EYES_WHO_SOCKET`'s) is held, not matched, now ([`held_mark`]).
/// Off, or with no socket, never.
pub fn held_now() -> bool {
    held_in(
        sticky_on(),
        std::env::var_os(ENVIRONMENT).map(PathBuf::from).as_deref(),
    )
}

/// [`held_now`] with the switch `sticky` and the eyes' socket `socket`.
pub fn held_in(sticky: bool, socket: Option<&Path>) -> bool {
    sticky
        && socket
            .filter(|socket| socket.is_absolute())
            .is_some_and(|socket| held_mark(&known_file_of(socket)))
}

/// The owner's checks for a private read-back read the known name through
/// this: `None` while it is held ([`held_now`]), so a held name is never the
/// owner present or alone (the sticky plan's §2).
pub fn known_for_owner_checks() -> Option<String> {
    if held_now() {
        return None;
    }
    known_from_environment()
}

/// [`known_for_owner_checks`] with the switch `sticky` and the eyes'
/// socket `socket`: `None` while the eyes' held mark is fresh (a held name,
/// or a T7 rematch, which the eyes write held).
pub fn known_for_owner_checks_in(sticky: bool, socket: Option<&Path>) -> Option<String> {
    if held_in(sticky, socket) {
        return None;
    }
    socket
        .filter(|socket| socket.is_absolute())
        .and_then(|socket| known_now(&known_file_of(socket)))
}

/// A known person counts as gone only after this long without a fresh known
/// file. The eyes renew it at each check, about every 2 s, while they see the face, so a person
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

    /// lamp-06c1, 5 Nov, S14 run 14:21, request 424: "Yes, please keep
    /// that." to "Want me to keep that for you?" was heard as "Yes. Please
    /// keep off.", and the brother fact was never kept. A keep said back
    /// ("keep that", "keep it"), or "keep off" beside a yes, is a yes to a
    /// held keep; any no still refuses, and to nothing but a keep is it a
    /// yes.
    /// #5572's privacy read (Jarvis, 18:41): with no yes word, the keep said
    /// back alone let every word that stands beside a yes sit around it, so a
    /// question whose "?" speech to text dropped ("Do you keep that", to
    /// "Want me to keep that for you?") kept the fact. Only "please", "lamp",
    /// "for", "me" and a thanks may stand around a keep said with no yes; a
    /// line that asks back is asked about once more, never a yes.
    #[test]
    fn a_question_that_lost_its_mark_is_not_a_yes_to_a_held_keep() {
        for asked_back in [
            "Do you keep that",
            "Would you keep it",
            "You keep that",
            "I would keep it",
            "I'd keep that",
            "Do you keep it, lamp",
        ] {
            assert!(!plain_agreement_to_keep(asked_back), "{asked_back}");
            assert!(unclear_answer_to_keep(asked_back), "{asked_back}");
        }
        for keep in [
            "Please keep that.",
            "Keep it.",
            "Keep that, please.",
            "Keep it for me, thanks.",
            "Thank you, keep that.",
            "Lamp, keep it.",
            "Please keep that for me, thank you.",
        ] {
            assert!(plain_agreement_to_keep(keep), "{keep}");
        }
        // With a yes word, the yes's own company is unchanged.
        for yes in ["Yes, I'd like that, keep it.", "Yes please, do keep it."] {
            assert!(plain_agreement_to_keep(yes), "{yes}");
        }
    }

    #[test]
    fn a_near_yes_to_a_held_keep_is_one() {
        for yes in [
            "Yes. Please keep off.",
            "Yes, keep off.",
            "Please keep that.",
            "Keep it.",
            "Keep that, please.",
            "Yes, keep this.",
        ] {
            assert!(plain_agreement_to_keep(yes), "{yes}");
            assert!(!plain_agreement(yes), "{yes}");
            assert!(!unclear_answer_to_keep(yes), "{yes}");
        }
        for not in [
            "Keep off.",
            "No, keep off.",
            "Don't keep it.",
            "No, don't keep that.",
            "Please don't keep that.",
            "Yes, keep it off.",
            "Keep it?",
            "Keep my face.",
        ] {
            assert!(!plain_agreement_to_keep(not), "{not}");
        }
    }

    /// An answer to a held keep that is neither a yes nor a no (a yes with a
    /// word misheard, a cut "keep", a yes asked back) is asked about once
    /// more; a no, any line that says more, or another ask is not.
    #[test]
    fn an_unclear_answer_to_a_held_keep_is_asked_again() {
        for unclear in [
            "Yes. Please key that.",
            "Yes, please kee.",
            "Yes?",
            "Yeah, keep it off.",
            "Keep off.",
            "Please keep of.",
        ] {
            assert!(!plain_agreement_to_keep(unclear), "{unclear}");
            assert!(unclear_answer_to_keep(unclear), "{unclear}");
        }
        for not in [
            "No.",
            "No, don't keep it.",
            "Not now.",
            "Yes. Please keep that.",
            "",
            "Lamp, what's the tallest mountain in Africa?",
            "Lamp, please remember that my goldfish is called Pip.",
            "Yes, and my sister lives in Boston too.",
        ] {
            assert!(!unclear_answer_to_keep(not), "{not}");
        }
    }

    #[test]
    fn observations_expire_from_source_age_without_renewal_on_reread() {
        let monotonic = Instant::now();
        let wall = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let modified = wall - Duration::from_secs(2);
        let left = FRESH - Duration::from_secs(2);
        let first = FaceObservation::from_source("Ann", modified, wall, monotonic).unwrap();
        assert_eq!(first.valid_until(), monotonic + left);
        let later = Duration::from_millis(500);
        let again =
            FaceObservation::from_source("Ann", modified, wall + later, monotonic + later).unwrap();
        assert_eq!(first.valid_until(), again.valid_until());
        assert!(!again.is_fresh(monotonic + left));
        assert!(FaceObservation::from_source("Ann", modified, wall + left, monotonic).is_none());
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
                rename_yes: false,
                name_learn_fix: false,
                face_no_ask: false,
                face_save_known_only: false,
                face_question: false,
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

    /// `LAMP_NAMES_RENAME_ASKED` (#5485): another name from the one face at
    /// the lamp, known by a different one, asks; their own name, an
    /// unknown face, two faces, no fresh look or no name given don't.
    #[test]
    fn another_name_is_one_given_by_the_one_known_face_that_is_not_theirs() {
        for said in [
            "Call me Tove.",
            "Actually, my name is Tove.",
            "I'm Tove now.",
        ] {
            assert!(gives_another_name(said, Some("Kess"), Some(1)), "{said}");
            assert!(!gives_another_name(said, None, Some(1)), "{said}");
            assert!(!gives_another_name(said, Some("Kess"), Some(2)), "{said}");
            assert!(!gives_another_name(said, Some("Kess"), Some(0)), "{said}");
            assert!(!gives_another_name(said, Some("Kess"), None), "{said}");
        }
        for said in [
            "I'm Kess.",
            "Call me kess.",
            "My name is Kess, remember?",
            "Hello there.",
        ] {
            assert!(!gives_another_name(said, Some("Kess"), Some(1)), "{said}");
        }
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

    /// #8165: a no to the one-face forget question refuses it, a pleasantry
    /// and a yes never do, and a no beside a word that forgets after all
    /// ("No, just forget it.") is no refusal.
    #[test]
    fn refuses_reads_the_noes_and_never_a_yes() {
        for no in [
            "No.",
            "No, keep it.",
            "Don't.",
            "Lamp, no.",
            "No, no, keep it.",
            "Nope.",
            "Nah, leave it.",
            "No thanks.",
            "Of course not.",
            "Kh\u{f4}ng, gi\u{1eef} l\u{1ea1}i.",
            "\u{110}\u{1eeb}ng.",
            "No, don't forget it.",
            "No, don't delete it.",
            "Don't forget it.",
            "\u{110}\u{1eeb}ng qu\u{ea}n.",
            "No, don't get rid of it.",
            "Don't drop it.",
            "No, never drop that.",
            "Don't scratch that.",
        ] {
            assert!(refuses(no), "{no}");
        }
        for not in [
            "Yes.",
            "Lamp, yes.",
            "Loan. Yes.",
            "Yes please.",
            "Sure, go ahead.",
            "No problem, go ahead.",
            "No worries, do it.",
            "Why not?",
            "Yes, I don't mind.",
            "No, just forget it.",
            "No no, delete it.",
            "No, I don't need it.",
            "No, don't keep it.",
            "No, erase it.",
            "No, remove it.",
            "No, drop it.",
            "No, get rid of it.",
            "No, do not keep it.",
            "Why don't you forget it?",
            // Forget said and taken back in one line: asked once more.
            "No, don't keep it, forget it. No wait, don't delete it.",
            "Wait.",
            "Keep it.",
            "",
        ] {
            assert!(!refuses(not), "{not}");
        }
    }

    /// #8185: a refusal that is only a refusal is answered by the lamp at
    /// once; one that says anything else, or no refusal, is not.
    #[test]
    fn only_refuses_reads_a_no_and_nothing_else() {
        for no in [
            "No.",
            "Lamp, no.",
            "No, keep it.",
            "No, no, keep it.",
            "Don't.",
            "Nope.",
            "Nah, leave it.",
            "No thanks.",
            "Of course not.",
            "No, don't forget it.",
            "No, do not delete it.",
            "No, don't get rid of it.",
            "Don't drop it.",
            "No, I want to keep it.",
            "Kh\u{f4}ng, gi\u{1eef} l\u{1ea1}i.",
            "\u{110}\u{1eeb}ng qu\u{ea}n.",
        ] {
            assert!(only_refuses(no), "{no}");
        }
        for not in [
            "No, turn the lights on.",
            "Yes, but don't forget it.",
            "No. What time is it?",
            "No?",
            "No, do it.",
            "No, no, no, please don't forget that, lamp.",
            "I don't want to keep it.",
            "No, I don't want it.",
            "Never mind just delete it",
            "Don't wait just delete it",
            "No not now delete it now",
            "No, scratch it.",
            "No, just forget it.",
            "No problem, go ahead.",
            "Yes.",
            "Keep it.",
            "",
        ] {
            assert!(!only_refuses(not), "{not}");
        }
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

    /// The lamp-29e7 rename of 6 Nov (#5485), with made-up names: B known,
    /// asking to be called A. Each yes the person said, refused by the word
    /// list then ([`agreed_to`]), is a yes by its guards ([`rename_agreed`]),
    /// and the name is heard for the rename ([`heard_for_rename`]).
    #[test]
    fn the_29e7_yeses_to_a_rename_are_agreed_by_their_guards() {
        let cases = [
            // 14:02:31, request 18, as heard.
            (
                "Can you call me Marisol?",
                "I know this face as Tove. Do you want me to know you as Marisol instead?",
                "Yes. please forgot the name Tove and Marisol, and this is my faith the face of Marisol.",
            ),
            // 14:03:28: the name and its spelling again.
            (
                "Yes. please forgot the name Tove and Marisol, and this is my faith the face of Marisol.",
                "This face is known to me as Tove. You want to be known as Marisol instead, is that right?",
                "Okay. One more time. I'm Marisol. M a r i s o l, and this is my face.",
            ),
            // 14:28:00, request 61: no call at all then.
            (
                "Okay. I want to change my name Can you call me Marisol instead?",
                "Marisol instead of Tove, is that right?",
                "Okay. Can you repeat my name again?",
            ),
            (
                "Can you call me Marisol instead?",
                "Marisol instead of Tove, is that right?",
                "Sure, go ahead, forget Tove.",
            ),
        ];
        for (asked, lamp, yes) in cases {
            let said = Said {
                this: yes.into(),
                before: vec![(asked.into(), lamp.into())],
            };
            assert!(!agreed_to("Marisol", &said), "{yes}");
            assert!(rename_agreed("Marisol", &said), "{yes}");
            assert!(heard_for_rename("Marisol", &said), "{yes}");
        }
        // The lamp's last answer must say the new name.
        let not_said_back = Said {
            this: "Okay.".into(),
            before: vec![(
                "Can you call me Marisol instead?".into(),
                "Sure, what should I do?".into(),
            )],
        };
        assert!(!rename_agreed("Marisol", &not_said_back));
        assert!(!heard_for_rename("Marisol", &not_said_back));
    }

    /// Every turn the reviewer on #1208 had asked about again stays refused
    /// by [`rename_agreed`], and every yes there stays a yes.
    #[test]
    fn every_1208_correction_is_still_refused_by_the_rename_guards() {
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
            "Yes, I do.",
            "Yes, do it.",
            "Sure, go for it.",
            "Yes, I'd like that.",
            "Yes, that would be great.",
            "Ừ, gọi tôi là Kesh.",
            "Đúng rồi, tên tôi là Kesh.",
            "Yes, K, E, S, H.",
        ] {
            assert!(rename_agreed("Kesh", &to(yes)), "{yes}");
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
            "Yes?",
            "Okay?",
            "Right?",
            "Dạ?",
            "Sure, why not?",
            "No, Kess. K, E, S, S.",
            "Not quite. Yes, K, E, S, S.",
            "Yeah, no, it's Kim.",
            "Okay, no, it's Kess.",
            "Yes, my name is Kim.",
            "Yes, I'm Kim.",
            "Yes, Kess.",
        ] {
            assert!(!rename_agreed("Kesh", &to(not)), "{not}");
            assert!(!heard_for_rename("Kesh", &to(not)), "{not}");
        }
        // A name of three letters has no near words: "and" is not "Ann".
        let ann = Said {
            this: "Yes, and thanks.".into(),
            before: vec![(
                "Call me Ann.".into(),
                "I know you as A. Shall I call you Ann instead?".into(),
            )],
        };
        assert!(rename_agreed("Ann", &ann));
    }

    /// Thirteen's reads on #5586: a hesitant yes ("wait", "hold on", "hang
    /// on", "let me think") and the Vietnamese "nhưng" (but) and "khoan"
    /// (wait) take a rename's yes back, so it is asked once more (#5572:
    /// unclear means ask again). Whole words and phrases only: "holding",
    /// "on" alone or "think" alone take nothing back.
    #[test]
    fn a_hesitant_yes_to_a_rename_is_asked_again() {
        let to = |name: &str, this: &str| Said {
            this: this.into(),
            before: vec![(
                format!("Can you call me {name} instead?"),
                format!("I know you as A. Shall I call you {name} instead?"),
            )],
        };
        for not in [
            "Okay, wait.",
            "Okay, let me think.",
            "Yes, hold on.",
            "Sure, hang on.",
            "Ừ, nhưng thôi.",
            "Ừ, khoan đã.",
            "Khoan đã.",
        ] {
            assert!(!rename_agreed("Kess", &to("Kess", not)), "{not}");
            assert!(!heard_for_rename("Kess", &to("Kess", not)), "{not}");
        }
        for yes in [
            "Yes, call me Kess.",
            "Yes, go on.",
            "Yes, I think so.",
            "Yes, holding my breath, call me Kess.",
        ] {
            assert!(rename_agreed("Kess", &to("Kess", yes)), "{yes}");
        }
        // Near words still err safe: "like" is one letter from "Mike" and
        // "Luke", so this yes is asked again for them.
        for name in ["Mike", "Luke"] {
            assert!(
                !rename_agreed(name, &to(name, "Yes, I'd like that.")),
                "{name}"
            );
        }
    }

    /// The rename gate with the switch on and off, on the eyes: on, the
    /// 29e7 yes sends `replace`; off, it is refused here as today
    /// ([`plain_yes`]), and the eyes are never asked.
    #[test]
    fn the_rename_gate_takes_the_guards_only_with_the_switch() {
        let call = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({"name": "Marisol", "replace": true}),
        };
        let said = Said {
            this: "Okay. Can you repeat my name again?".into(),
            before: vec![(
                "Okay. I want to change my name Can you call me Marisol instead?".into(),
                "Marisol instead of Tove, is that right?".into(),
            )],
        };
        let switches = |rename_yes: bool| FaceSwitches {
            checked: true,
            rename_yes,
            ..FaceSwitches::default()
        };
        let (socket, asked) = eyes(vec![json!({"remembered": "Marisol", "replaced": ["Tove"]})]);
        let (off, _) = run_switched(&socket, &call, switches(false), &said, None, &mut None);
        assert!(refused_here(&off), "{off}");
        let mut replaced = None;
        let (on, _) = run_switched(&socket, &call, switches(true), &said, None, &mut replaced);
        assert_eq!(on, json!({"ok": true, "remembered": "Marisol"}));
        assert_eq!(replaced, Some(vec!["Tove".to_owned()]));
        // A correction is still refused here, switch on.
        let corrected = Said {
            this: "Yes, but it's Marisa.".into(),
            ..said.clone()
        };
        let (still, _) = run_switched(&socket, &call, switches(true), &corrected, None, &mut None);
        assert!(refused_here(&still), "{still}");
        assert_eq!(
            asked.join().unwrap(),
            [json!({"remember": "Marisol", "one_face": true, "replace": true})]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// What a held rename is held on: the eyes' `known_as` as the model is
    /// told it, with the old name, or a `replace` before their yes, with
    /// none; nothing else.
    #[test]
    fn a_rename_is_asked_by_known_as_or_a_replace_before_the_yes() {
        let known_as = told_asking(&json!({"error": "known_as", "known_as": "Tove"}), false);
        assert_eq!(rename_asked(&known_as), Some(Some("Tove".to_owned())));
        assert_eq!(rename_asked(&json!({"error": NOT_AGREED})), Some(None));
        for not in [
            json!({"ok": true, "remembered": "Marisol"}),
            json!({"error": NOT_HEARD}),
            told_asking(&json!({"error": "no_face"}), false),
        ] {
            assert_eq!(rename_asked(&not), None, "{not}");
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

    /// #8471's lamp run (the first builder's 14:01 call): while the eyes
    /// know nobody (an empty print store) the owner is never dropped, so a
    /// restart keeps `owner` in the walk's file; #4290's drop stands for a
    /// list the owner is not in. A deliberate narrowing of #4290.
    #[test]
    fn an_empty_print_store_never_drops_the_owner_across_a_restart() {
        let (socket, _asked) = eyes(vec![
            serde_json::json!({"people": []}),
            serde_json::json!({"people": []}),
        ]);
        let file = socket.with_file_name("progress.json");
        std::fs::write(&file, r#"{"steps":{},"owner":"Example Name"}"#).unwrap();
        let mailbox = crate::onboarding::mailbox_of(&file);
        for _ in 0..2 {
            refresh_owner(Some(&file), Some(&socket));
        }
        // Only that their face is unknown since, told once; no owner gone.
        assert_eq!(
            std::fs::read_to_string(&mailbox).unwrap_or_default(),
            "{\"owner_face\":{\"owner\":\"Example Name\",\"unseen\":true}}\n"
        );
        // The voice started again: the walk reads its file and mailbox.
        let mut walk = crate::onboarding::Onboarding::at(file.clone());
        for at in 0..8 {
            walk.drain_storage_for_test(at * 250_000);
            let _ = walk.tick(at * 250_000, at * 250_000, true, false, None, true);
            walk.drain_storage_for_test(at * 250_000);
        }
        assert_eq!(
            crate::onboarding::owner_of(&file).as_deref(),
            Some("Example Name")
        );
    }

    /// The walk reading its file and mailbox a couple of seconds on.
    fn walk_settles(walk: &mut crate::onboarding::Onboarding, at: &mut u64) {
        for _ in 0..8 {
            walk.drain_storage_for_test(*at);
            let _ = walk.tick(*at, *at, true, false, None, true);
            walk.drain_storage_for_test(*at);
            *at += 250_000;
        }
    }

    /// The owner named now, tried again past other tests' forgets (they
    /// share the count), one eyes' answer each.
    fn owner_named(file: &Path, socket: &Path, tries: usize) -> Option<String> {
        for _ in 0..tries {
            refresh_owner(Some(file), Some(socket));
            if owner_now().is_some() {
                break;
            }
        }
        owner_now()
    }

    /// 06c1 at `c374700486` (the first builder's 17:50 call on #8471): the
    /// prints lost, learn later's first save was a face under another name
    /// than the walk's owner; the eyes then knew one person, not the owner,
    /// and the owner was dropped. Until the owner's face is seen again a
    /// list without them is not the owner gone; their own name kept then
    /// links the owner again.
    #[test]
    fn prints_lost_another_name_first_then_the_owners_own_name_links_the_owner_again() {
        let (socket, _asked) = eyes(vec![
            serde_json::json!({"people": []}),
            serde_json::json!({"people": ["Robin"]}),
            serde_json::json!({"people": ["Robin"]}),
            serde_json::json!({"people": ["Robin", "Example Name"]}),
            serde_json::json!({"people": ["Robin", "Example Name"]}),
            serde_json::json!({"people": ["Robin", "Example Name"]}),
        ]);
        let file = socket.with_file_name("progress.json");
        std::fs::write(&file, r#"{"steps":{},"owner":"Example Name"}"#).unwrap();
        let mut walk = crate::onboarding::Onboarding::at(file.clone());
        let mut at = 0;
        for step in 0..3 {
            refresh_owner(Some(&file), Some(&socket));
            walk_settles(&mut walk, &mut at);
            assert_eq!(
                crate::onboarding::owner_of(&file).as_deref(),
                Some("Example Name"),
                "step {step}: the owner mark stays"
            );
        }
        assert_eq!(
            owner_named(&file, &socket, 3).as_deref(),
            Some("Example Name"),
            "their own name kept: the owner again"
        );
        walk_settles(&mut walk, &mut at);
        assert_eq!(
            crate::onboarding::owner_of(&file).as_deref(),
            Some("Example Name")
        );
    }

    /// The same, the other name the only one: the owner mark stays and no
    /// owner gone is posted, through a voice started again (the walk's
    /// file keeps that the owner's face is unknown since the prints went).
    #[test]
    fn prints_lost_and_another_name_first_keeps_the_owner_mark_through_a_restart() {
        let (socket, _asked) = eyes(vec![
            serde_json::json!({"people": []}),
            serde_json::json!({"people": ["Robin"]}),
            serde_json::json!({"people": ["Robin"]}),
        ]);
        let file = socket.with_file_name("progress.json");
        std::fs::write(&file, r#"{"steps":{},"owner":"Example Name"}"#).unwrap();
        let mailbox = crate::onboarding::mailbox_of(&file);
        let mut walk = crate::onboarding::Onboarding::at(file.clone());
        let mut at = 0;
        for step in 0..2 {
            refresh_owner(Some(&file), Some(&socket));
            assert!(
                !std::fs::read_to_string(&mailbox)
                    .unwrap_or_default()
                    .contains("face_forgotten"),
                "step {step}: no owner gone"
            );
            walk_settles(&mut walk, &mut at);
            assert_eq!(
                crate::onboarding::owner_of(&file).as_deref(),
                Some("Example Name"),
                "step {step}"
            );
        }
        assert!(
            crate::onboarding::owner_face_unseen(&file),
            "kept in the file"
        );
        drop(walk);
        // A voice started again knows only the walk's file.
        OWNER_FACE_UNKNOWN
            .lock()
            .unwrap()
            .retain(|kept| kept != &file);
        let mut walk = crate::onboarding::Onboarding::at(file.clone());
        walk_settles(&mut walk, &mut at);
        refresh_owner(Some(&file), Some(&socket));
        walk_settles(&mut walk, &mut at);
        assert_eq!(
            crate::onboarding::owner_of(&file).as_deref(),
            Some("Example Name"),
            "started again"
        );
    }

    /// #4290 kept: once the eyes know the owner's face again, a list
    /// without them ends the mark as before.
    #[test]
    fn the_owner_seen_again_then_a_list_without_them_ends_the_mark() {
        let (socket, _asked) = eyes(vec![
            serde_json::json!({"people": []}),
            serde_json::json!({"people": ["Example Name"]}),
            serde_json::json!({"people": ["Robin"]}),
        ]);
        let file = socket.with_file_name("progress.json");
        std::fs::write(&file, r#"{"steps":{},"owner":"Example Name"}"#).unwrap();
        let mut walk = crate::onboarding::Onboarding::at(file.clone());
        let mut at = 0;
        for _ in 0..3 {
            refresh_owner(Some(&file), Some(&socket));
            walk_settles(&mut walk, &mut at);
        }
        assert_eq!(crate::onboarding::owner_of(&file), None);
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
            rename_yes: false,
            name_learn_fix: false,
            face_no_ask: false,
            face_save_known_only: false,
            face_question: false,
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
    /// lamp-74ff, 19 Nov 12:07:29: the owner (w0.07 h0.13 c0.90) and a
    /// tiny face far behind (w0.05 h0.08 c0.81). With `LAMP_FACE_STICKY`
    /// the by-default name counts near faces only: one face, the owner
    /// named; two near faces, not. The read-backs' alone check still counts
    /// every face: the far one blocks it.
    #[test]
    fn a_tiny_far_face_counts_for_no_one_but_still_blocks_an_alone_read_back() {
        let face = |id: &str, width: f32, height: f32, confidence: f32| lamp_msgs::Face {
            ymax: 0.2 + height,
            ..desk_face(id, width, confidence)
        };
        let look = |faces: Vec<lamp_msgs::Face>| lamp_msgs::Faces { faces };
        let owners = ["Ann".to_owned()];
        let far = look(vec![
            face("f1", 0.07, 0.13, 0.90),
            face("f2", 0.05, 0.08, 0.81),
        ]);
        for desk_only in [false, true] {
            assert_eq!(count_near_at_desk(&far, desk_only), 1, "{desk_only}");
        }
        let near = count_near_at_desk(&far, false);
        assert_eq!(
            crate::person::someone_else_now(&owners, Some(near), Some("Ann"), false, false, None),
            None,
            "the owner named"
        );
        // Off the sticky switch, every face: held back, as before.
        let all = count_at_desk(&far, false);
        assert_eq!(all, 2);
        assert_eq!(
            crate::person::someone_else_now(&owners, Some(all), Some("Ann"), false, false, None),
            Some("more than one face in view")
        );
        // Two near faces: not named.
        let two = look(vec![
            face("f1", 0.07, 0.13, 0.90),
            face("f2", 0.08, 0.11, 0.85),
        ]);
        assert_eq!(count_near_at_desk(&two, false), 2);
        assert_eq!(
            crate::person::someone_else_now(&owners, Some(2), Some("Ann"), false, false, None),
            Some("more than one face in view")
        );
        // The alone read-back counts every face (`most_faces_lately`): the
        // far face blocks it.
        let mut every = FacesLately::default();
        let mut nearly = FacesLately::default();
        let at = std::time::Instant::now();
        for seq in 0..2 {
            every.look(at, ("b".into(), seq), count_at_desk(&far, at_desk_only()));
            nearly.look(
                at,
                ("b".into(), seq),
                count_near_at_desk(&far, at_desk_only()),
            );
        }
        assert_eq!(nearly.most(at), Some(1));
        assert_ne!(
            every.most(at),
            Some(1),
            "not alone: the read-back is blocked"
        );
        assert!(!crate::services::alone_in(every.most(at), false));
    }

    fn desk_face(id: &str, width: f32, confidence: f32) -> lamp_msgs::Face {
        lamp_msgs::Face {
            pitch: None,
            looking: None,
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
    /// still names its person at 5 s, as `known_now`'s [`FRESH`] does not; one
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
        assert_eq!(known_now(&path), None, "the FRESH read misses it");
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

    /// #7331 (lamp-74ff, 16 Nov): a rename spelled after the lamp asked for
    /// a spelling was refused, and a name of two letters reached the eyes
    /// as its letters. With `LAMP_NAME_LEARN_FIX`, the letters spelled in
    /// the turn in hand after a spell ask are the name and its yes, as at
    /// setup, two letters joined as a word; off, refused here as before.
    #[test]
    fn a_spelled_short_name_is_heard_on_a_rename() {
        let switches = |name_learn_fix: bool| FaceSwitches {
            checked: true,
            rename_yes: true,
            name_learn_fix,
            ..FaceSwitches::default()
        };
        let rename = |name: &str| ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({"name": name, "replace": true}),
        };
        let spelled = |asked: &str, this: &str| Said {
            this: this.into(),
            before: vec![("Please call me something else.".into(), asked.into())],
        };
        let cases = [
            ("How do you spell it?", "A d a", "A d a", "Ada"),
            ("How do you spell it?", "A, d, a", "Ada", "Ada"),
            ("Could you spell it for me?", "A l", "A l", "Al"),
            ("Could you spell it for me?", "A, l.", "Al", "Al"),
        ];
        let (socket, asked) = eyes(
            cases
                .iter()
                .map(|(_, _, _, word)| json!({"remembered": word, "replaced": ["Tove"]}))
                .collect(),
        );
        for (ask, this, name, _) in cases {
            let said = spelled(ask, this);
            let (off, _) = run_switched(
                &socket,
                &rename(name),
                switches(false),
                &said,
                None,
                &mut None,
            );
            assert!(refused_here(&off), "off, {this}: {off}");
            let (on, _) = run_switched(
                &socket,
                &rename(name),
                switches(true),
                &said,
                None,
                &mut None,
            );
            assert_eq!(on["ok"], true, "on, {this}: {on}");
        }
        // Two letters with no spell ask stay initials, and no yes.
        let (initials, _) = run_switched(
            &socket,
            &rename("A l"),
            switches(true),
            &spelled("Sure, what should I call you?", "A l"),
            None,
            &mut None,
        );
        assert!(refused_here(&initials), "{initials}");
        assert_eq!(
            asked.join().unwrap(),
            cases
                .iter()
                .map(|(_, _, _, word)| json!({"remember": word, "one_face": true, "replace": true}))
                .collect::<Vec<_>>()
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// #7331 (the first builder's question, the Reviewer-lead's P2-3 on
    /// #7343): a save of a new face took no yes, so a name given while
    /// nobody was known led straight to keeping that face. With
    /// `LAMP_NAME_LEARN_FIX`, a new face (no `replace`) is kept only on the
    /// person's own ask for it or their yes to the lamp's face question
    /// ("Shall I remember your face as Ada?"); a name alone or a no sends
    /// nothing, and the model is told to ask. Off, as before.
    #[test]
    fn a_new_face_is_kept_only_on_a_yes_to_keeping_it() {
        let switches = |name_learn_fix: bool| FaceSwitches {
            checked: true,
            rename_yes: true,
            name_learn_fix,
            ..FaceSwitches::default()
        };
        let save = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({"name": "Ada"}),
        };
        let asked = |this: &str| Said {
            this: this.into(),
            before: vec![(
                "My name is Ada.".into(),
                "Nice to meet you, Ada! Shall I remember your face as Ada?".into(),
            )],
        };
        let (socket, sent) = eyes(vec![json!({"remembered": "Ada"}); 3]);
        let run = |on: bool, said: &Said| {
            run_switched(&socket, &save, switches(on), said, None, &mut None).0
        };
        // Off: a name alone keeps the face, as before.
        assert_eq!(run(false, &Said::now("My name is Ada."))["ok"], true);
        // On: a name alone, a no, or another answer send nothing.
        for said in [
            Said::now("My name is Ada."),
            asked("No, thanks."),
            asked("Why do you ask?"),
        ] {
            let told = run(true, &said);
            assert_eq!(told["error"], ASK_FACE_FIRST, "{said:?}: {told}");
            assert!(refused_here(&told));
        }
        // On: a line keeping their face out keeps nothing and asks nothing,
        // never the face question it just refused (Heron's HOLD on #7622);
        // "Don't forget my face" is a refusal word's plain ask, asked first.
        for this in [
            "Keep my face out of this, my name is Ada.",
            "My name is Ada. Leave my face off it.",
            "Forget my face, I'm Ada.",
        ] {
            let told = run(true, &Said::now(this));
            assert_eq!(told, json!({"error": FACE_KEPT_OUT}), "{this}");
            assert!(refused_here(&told));
            assert!(!told.to_string().contains("Shall I"), "{told}");
        }
        let told = run(true, &Said::now("Don't forget my face, my name is Ada."));
        assert_eq!(told["error"], ASK_FACE_FIRST, "{told}");
        // On: their yes to the face question, or their own ask, keeps it.
        assert_eq!(run(true, &asked("Yes."))["ok"], true);
        assert_eq!(
            run(true, &Said::now("Remember my face, my name is Ada."))["ok"],
            true
        );
        assert_eq!(
            sent.join().unwrap(),
            vec![json!({"remember": "Ada", "one_face": true, "replace": false}); 3]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// #7653 (row-7422c on lamp-52e6, 18 Nov): a known face's "Call me
    /// Jay." was refused here as [`ASK_FACE_FIRST`], so the eyes never said
    /// the face is known as Walter and the rename was never held. With
    /// `LAMP_NAME_LEARN_FIX`, while the eyes' known file names a face at the
    /// lamp, the save goes to the eyes and their "known as" answer is a
    /// rename asked ([`rename_asked`]).
    #[test]
    fn a_known_face_giving_another_name_reaches_the_eyes_and_is_asked_to_rename() {
        let save = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({"name": "Jay"}),
        };
        let (socket, sent) = eyes(vec![json!({"error": "known_as", "known_as": "Walter"})]);
        std::fs::write(known_file_of(&socket), "Walter\n").unwrap();
        let switches = FaceSwitches {
            checked: true,
            rename_yes: true,
            name_learn_fix: true,
            ..FaceSwitches::default()
        };
        let (told, _) = run_switched(
            &socket,
            &save,
            switches,
            &Said::now("Call me Jay."),
            None,
            &mut None,
        );
        assert!(!refused_here(&told), "{told}");
        assert_eq!(
            rename_asked(&told),
            Some(Some("Walter".to_owned())),
            "{told}"
        );
        assert_eq!(
            sent.join().unwrap(),
            vec![json!({"remember": "Jay", "one_face": true, "replace": false})]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// #7704: the face question is skipped while the eyes' known file names
    /// a face at the lamp, and that file can still name Walter for up to
    /// 3 s after a stranger steps in. With `LAMP_FACE_SAVE_KNOWN_ONLY` the
    /// save asks the eyes to keep it only for a face they know
    /// (`known_only`); their `not_known` is the face question
    /// ([`ASK_FACE_FIRST`]) and nothing is kept. Off, the save is sent and
    /// kept as before.
    #[test]
    fn with_known_only_a_face_the_eyes_do_not_know_is_asked_the_face_question() {
        let save = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({"name": "Jay"}),
        };
        let switches = |face_save_known_only: bool| FaceSwitches {
            checked: true,
            rename_yes: true,
            name_learn_fix: true,
            face_save_known_only,
            ..FaceSwitches::default()
        };
        let (socket, sent) = eyes(vec![json!({"error": "not_known"})]);
        std::fs::write(known_file_of(&socket), "Walter\n").unwrap();
        let (told, _) = run_switched(
            &socket,
            &save,
            switches(true),
            &Said::now("Call me Jay."),
            None,
            &mut None,
        );
        assert_eq!(told["error"], ASK_FACE_FIRST, "{told}");
        assert!(told.get("remembered").is_none(), "{told}");
        assert_eq!(
            sent.join().unwrap(),
            vec![
                json!({"remember": "Jay", "one_face": true, "replace": false, "known_only": true})
            ]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
        // Off: no `known_only`, and the stranger is kept as the eyes answer.
        let (socket, sent) = eyes(vec![json!({"remembered": "Jay"})]);
        std::fs::write(known_file_of(&socket), "Walter\n").unwrap();
        let (told, _) = run_switched(
            &socket,
            &save,
            switches(false),
            &Said::now("Call me Jay."),
            None,
            &mut None,
        );
        assert_eq!(told["remembered"], "Jay", "{told}");
        assert_eq!(
            sent.join().unwrap(),
            vec![json!({"remember": "Jay", "one_face": true, "replace": false})]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// `LAMP_FACE_QUESTION_NOT_ASK` (lamp-74ff, 19 Nov): the face in view
    /// already known by the name asked for, a fresh real match, is told so
    /// ([`already_known`]) whatever the line says, with nothing sent to the
    /// eyes and no refusal. The line's words are never judged: an unknown
    /// face's call, a face known by another name, a rename, and the switch
    /// off go as before.
    #[test]
    fn with_face_question_a_known_face_sends_nothing_and_any_other_call_goes_as_before() {
        // The eyes mark held names (`LAMP_FACE_STICKY`), none held here.
        TEST_HELD_MARKS.with(|marks| marks.set(true));
        let _serial = FACE_QUESTION_SERIAL
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let switches = |face_question: bool| FaceSwitches {
            checked: true,
            rename_yes: true,
            name_learn_fix: true,
            face_question,
            ..FaceSwitches::default()
        };
        let call = |args: Value| ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args,
        };
        // Runs one save against eyes answering `answers`, with `known` in
        // the known file: what the model is told and what the eyes got.
        let run = |on: bool, known: Option<&str>, args: Value, this: &str, answers: Vec<Value>| {
            let (socket, sent) = eyes(answers);
            if let Some(known) = known {
                std::fs::write(known_file_of(&socket), format!("{known}\n")).unwrap();
            }
            let (told, _) = run_switched(
                &socket,
                &call(args),
                switches(on),
                &Said::now(this),
                None,
                &mut None,
            );
            let sent = sent.join().unwrap();
            let _ = std::fs::remove_dir_all(socket.parent().unwrap());
            (told, sent)
        };
        // Known as Ana, asked again as Ana (any case and spacing), with a
        // question or a plain ask: told so, nothing sent.
        for (name, this) in [
            ("Ana", "Okay. Did you remember my face then?"),
            ("ana", "Do you remember my face?"),
            (" ANA ", "Remember my face."),
            ("Ana", "Do you know who I am? I'd like you to keep my face."),
            ("Ana", "Nhớ mặt mình nhé."),
        ] {
            let (told, sent) = run(true, Some("Ana"), json!({"name": name}), this, vec![]);
            assert_eq!(told["already_known"], "Ana", "{told}");
            assert_eq!(told["note"], ALREADY_KNOWN_NOTE, "{told}");
            assert!(told.get("error").is_none(), "{told}");
            assert!(answered_here(&told) && !refused_here(&told));
            assert!(sent.is_empty(), "{sent:?}");
        }
        assert!(ALREADY_KNOWN_NOTE.contains("already know"));
        // Nobody known, or another face: the call goes on as it does with
        // the switch off, whatever the line says. No words decide it.
        for known in [None, Some("Walter")] {
            for this in [
                "Okay. Did you remember my face then?",
                "Do you remember my face?",
                "Do you know who I am? I'd like you to keep my face.",
            ] {
                let on = run(true, known, json!({"name": "Ana"}), this, vec![]);
                let off = run(false, known, json!({"name": "Ana"}), this, vec![]);
                assert_eq!(on, off, "{this}");
                assert!(on.0.get("already_known").is_none(), "{this}");
            }
        }
        // A face known by another name, and a rename, go as today.
        let known_as = || vec![json!({"error": "known_as", "known_as": "Walter"})];
        for (known, args, this, answers) in [
            (
                Some("Walter"),
                json!({"name": "Jay"}),
                "Call me Jay.",
                known_as(),
            ),
            (
                Some("Ana"),
                json!({"name": "Ana", "replace": true}),
                "Do you remember my face?",
                vec![],
            ),
            (
                Some("Walter"),
                json!({"name": "Walter", "replace": true}),
                "Remember my face.",
                vec![],
            ),
        ] {
            let on = run(true, known, args.clone(), this, answers.clone());
            let off = run(false, known, args, this, answers);
            assert_eq!(on, off, "{this}");
            assert!(on.0.get("already_known").is_none(), "{this}");
        }
        // Off: today's answers, the lamp-74ff refusals.
        let (told, _) = run(
            false,
            Some("Ana"),
            json!({"name": "Ana"}),
            "Okay. Did you remember my face then?",
            vec![],
        );
        assert!(refused_here(&told), "{told}");
        assert!(told.get("already_known").is_none(), "{told}");
        let (told, _) = run(
            false,
            None,
            json!({"name": "Ana"}),
            "Do you remember my face?",
            vec![],
        );
        assert!(refused_here(&told), "{told}");
        // The setup walk's first save is untouched.
        let setup = |face_question: bool| FaceSwitches {
            at_setup: true,
            ..switches(face_question)
        };
        let (socket, _) = eyes(vec![]);
        std::fs::write(known_file_of(&socket), "Ana\n").unwrap();
        let said = Said::now("Do you remember my face?");
        let save = call(json!({"name": "Ana"}));
        assert_eq!(
            run_switched(&socket, &save, setup(true), &said, None, &mut None),
            run_switched(&socket, &save, setup(false), &said, None, &mut None)
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// The tests that read or write the confirmed name and the faces read
    /// for it ([`CONFIRMED`], [`TRACKS`]), one at a time.
    static FACE_QUESTION_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// One look of the eyes now, each face `(id, height, follows)`.
    fn look_now(
        eyes: &crate::speaking_faces::SpeakingFaces,
        seq: u64,
        faces: &[(&str, f32, Option<&str>)],
    ) {
        let faces = lamp_msgs::Faces {
            faces: faces
                .iter()
                .map(|(id, height, follows)| lamp_msgs::Face {
                    pitch: None,
                    looking: None,
                    id: (*id).to_owned(),
                    xmin: 0.2,
                    ymin: 0.2,
                    xmax: 0.4,
                    ymax: 0.2 + height,
                    confidence: 1.0,
                    yaw: None,
                    facing: None,
                    speaking: None,
                    follows: follows.map(str::to_owned),
                })
                .collect(),
        };
        eyes.look(std::time::Instant::now(), Some((("b", seq), &faces)));
    }

    /// `LAMP_FACE_QUESTION_NOT_ASK` (lamp-74ff, 19 Nov: saves right after
    /// the name was confirmed were refused as not heard): a name confirmed
    /// this session (spelled here) stands as heard for a later save only
    /// while one near face is in view and it is the same track, the same
    /// id or one followed from it; then it goes to the eyes' own checks. A
    /// new id, two near faces, a forget, or the switch off ask again. Only
    /// with `LAMP_FACE_NO_ASK` on too: off, this route has no speaking
    /// proof and the name is never reused, the same near face or not. With
    /// it on, a new face with nobody known keeps nothing before any name
    /// check, so here the eyes' known file names Ana (no held marks told,
    /// so no already-known answer): the save goes to the eyes.
    #[test]
    fn with_face_question_a_confirmed_name_is_reused_only_for_the_same_one_near_face() {
        let _serial = FACE_QUESTION_SERIAL
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        TEST_HELD_MARKS.with(|marks| marks.set(false));
        let switches = |face_question: bool, face_no_ask: bool| FaceSwitches {
            checked: true,
            rename_yes: true,
            name_learn_fix: true,
            face_question,
            face_no_ask,
            ..FaceSwitches::default()
        };
        let save = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({"name": "Ana"}),
        };
        let forget = ToolCall {
            id: "1".into(),
            name: FORGET.into(),
            args: json!({"name": "Ana"}),
        };
        let eyes_now = std::sync::Arc::new(crate::speaking_faces::SpeakingFaces::default());
        sees_tracks_with(Some(eyes_now.clone()));
        let mut seq = 0;
        let mut look = |faces: &[(&str, f32, Option<&str>)]| {
            seq += 1;
            look_now(&eyes_now, seq, faces);
        };
        // The eyes' known file names Ana with `LAMP_FACE_NO_ASK` on.
        let at_lamp = |no_ask: bool, answers: Vec<Value>| {
            let (socket, sent) = eyes(answers);
            if no_ask {
                std::fs::write(known_file_of(&socket), "Ana\n").unwrap();
            }
            (socket, sent)
        };
        // Confirmed by spelling, with f1 the one near face (a small far
        // face beside it is no second near one), and kept: off, no face
        // leave yet, so refused; on, the known face's save goes to the
        // eyes.
        let confirm = |on: bool, no_ask: bool| {
            let answers = if no_ask {
                vec![json!({"remembered": "Ana", "replaced": []})]
            } else {
                vec![]
            };
            let (socket, sent) = at_lamp(no_ask, answers);
            let (told, _) = run_switched(
                &socket,
                &save,
                switches(on, no_ask),
                &Said::now("A N A."),
                None,
                &mut None,
            );
            let sent = sent.join().unwrap();
            if no_ask {
                assert_eq!(sent.len(), 1, "{told}");
            } else {
                assert_eq!(told["error"], ASK_FACE_FIRST, "{told}");
                assert!(sent.is_empty());
            }
            let _ = std::fs::remove_dir_all(socket.parent().unwrap());
        };
        // A later "Remember my face." with no name in it, against eyes
        // answering `answers`.
        let again = |on: bool, no_ask: bool, answers: Vec<Value>| {
            let (socket, sent) = at_lamp(no_ask, answers);
            let (told, _) = run_switched(
                &socket,
                &save,
                switches(on, no_ask),
                &Said::now("Remember my face."),
                None,
                &mut None,
            );
            let sent = sent.join().unwrap();
            let _ = std::fs::remove_dir_all(socket.parent().unwrap());
            (told, sent)
        };
        let kept = || vec![json!({"remembered": "Ana", "replaced": []})];
        let asked = vec![json!({"remember": "Ana", "one_face": true, "replace": false})];
        // Not asked again: refused here, nothing sent, nothing kept.
        let unkept = |(told, sent): (Value, Vec<Value>)| {
            assert!(told.get("remembered").is_none(), "{told}");
            assert!(told.get("error").is_some(), "{told}");
            assert!(sent.is_empty(), "{sent:?}");
        };
        // With `LAMP_FACE_NO_ASK` off: never reused, even confirmed with
        // the same one near face in view (the route has no speaking proof).
        look(&[("f1", 0.3, None), ("f2", 0.05, None)]);
        confirm(true, false);
        look(&[("f1", 0.3, None)]);
        let (told, sent) = again(true, false, vec![]);
        assert_eq!(told["error"], NOT_HEARD, "{told}");
        assert!(sent.is_empty());
        forget_confirmed();
        // With it on: the same track saves through the eyes' checks.
        look(&[("f1", 0.3, None), ("f2", 0.05, None)]);
        confirm(true, true);
        look(&[("f1", 0.3, None)]);
        let (told, sent) = again(true, true, kept());
        assert_eq!(told["remembered"], "Ana", "{told}");
        assert_eq!(sent, asked);
        // A face the eyes said follows f1: the same track.
        look(&[("f3", 0.3, Some("f1"))]);
        let (told, sent) = again(true, true, kept());
        assert_eq!(told["remembered"], "Ana", "{told}");
        assert_eq!(sent, asked);
        // The eyes refuse another person's name for this face: no save.
        let (told, sent) = again(true, true, vec![json!({"error": "not_like_them"})]);
        assert_eq!(told, json!({"error": NOT_LIKE_THEM}), "{told}");
        assert_eq!(sent, asked);
        // Two near faces: asked again, nothing sent.
        look(&[("f3", 0.3, None), ("f4", 0.2, None)]);
        unkept(again(true, true, vec![]));
        // A new id since: asked again.
        look(&[("f5", 0.3, None)]);
        unkept(again(true, true, vec![]));
        // Confirmed again with f5, then forgotten: asked again.
        confirm(true, true);
        let (told, _) = again(true, true, kept());
        assert_eq!(told["remembered"], "Ana", "{told}");
        let (socket, _) = eyes(vec![json!({"forgot": true})]);
        let _ = run_switched(
            &socket,
            &forget,
            switches(true, true),
            &Said::now("Forget my face."),
            None,
            &mut None,
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
        unkept(again(true, true, vec![]));
        // The question switch off: as today, never reused.
        confirm(false, false);
        let (told, sent) = again(false, false, vec![]);
        assert_eq!(told["error"], NOT_HEARD, "{told}");
        assert!(sent.is_empty());
        forget_confirmed();
        sees_tracks_with(None);
    }

    /// `LAMP_FACE_QUESTION_NOT_ASK`: the eyes' `already` (a face kept again
    /// under its own name, matched as theirs) is told as known already,
    /// with no visits line; off, as a first save.
    #[test]
    fn with_face_question_a_face_the_eyes_knew_already_is_told_so() {
        let _serial = FACE_QUESTION_SERIAL
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let save = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({"name": "Ana"}),
        };
        let run = |face_question: bool| {
            let (socket, sent) = eyes(vec![json!({
                "remembered": "Ana", "replaced": [], "already": true, "visits": true
            })]);
            let (told, _) = run_switched(
                &socket,
                &save,
                FaceSwitches {
                    checked: true,
                    name_learn_fix: true,
                    face_question,
                    ..FaceSwitches::default()
                },
                &Said::now("Remember my face, my name is Ana."),
                None,
                &mut None,
            );
            assert_eq!(sent.join().unwrap().len(), 1);
            let _ = std::fs::remove_dir_all(socket.parent().unwrap());
            told
        };
        let on = run(true);
        assert_eq!(on["already_known"], "Ana", "{on}");
        assert_eq!(on["remembered"], "Ana", "{on}");
        assert_eq!(on["note"], ALREADY_KNOWN_NOTE, "{on}");
        assert!(on.get("also_say").is_none(), "{on}");
        assert!(!on.to_string().contains(VISITS_LINE), "{on}");
        assert!(!answered_here(&on));
        assert!(ALREADY_KNOWN_NOTE.contains("I already know your face"));
        let off = run(false);
        assert!(off.get("already_known").is_none(), "{off}");
        assert!(
            off["also_say"]
                .as_str()
                .is_some_and(|line| line.contains(VISITS_LINE)),
            "{off}"
        );
    }

    /// With `LAMP_FACE_NO_ASK` on too (#8010 turns it on, with
    /// `LAMP_REMEMBER_SPEAKER`): the already-known answer still comes first,
    /// on the route with no speaking proof, and on the speaker-bound one
    /// only for the line's own speaking face (an off-camera voice giving
    /// the in-view person's name is never told "I already know your
    /// face"); a confirmed name reused for the same one near
    /// face keeps nothing on the route with no speaking proof (nothing
    /// sent, the no-asks answer); and the speaker-bound route never reuses
    /// it (no proof: as before).
    #[test]
    fn with_face_question_and_no_ask_nothing_is_kept_without_speaking_proof() {
        // The eyes mark held names (`LAMP_FACE_STICKY`), none held here.
        TEST_HELD_MARKS.with(|marks| marks.set(true));
        let _serial = FACE_QUESTION_SERIAL
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let no_ask = |remember_speaker: bool| FaceSwitches {
            checked: true,
            rename_yes: true,
            name_learn_fix: true,
            face_no_ask: true,
            remember_speaker,
            face_question: true,
            ..FaceSwitches::default()
        };
        let save = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({"name": "Ana"}),
        };
        let switched = |known: Option<&str>, this: &str| {
            let (socket, sent) = eyes(vec![]);
            if let Some(known) = known {
                std::fs::write(known_file_of(&socket), format!("{known}\n")).unwrap();
            }
            let (told, _) = run_switched(
                &socket,
                &save,
                no_ask(false),
                &Said::now(this),
                None,
                &mut None,
            );
            let sent = sent.join().unwrap();
            let _ = std::fs::remove_dir_all(socket.parent().unwrap());
            (told, sent)
        };
        // Known as Ana: told so, nothing sent.
        let (told, sent) = switched(Some("Ana"), "Do you remember my face?");
        assert_eq!(told["already_known"], "Ana", "{told}");
        assert!(sent.is_empty());
        // Nobody known: the call goes on as with the switch off.
        let (told, sent) = switched(None, "Did you remember my face?");
        assert!(told.get("already_known").is_none(), "{told}");
        assert!(told.get("remembered").is_none(), "{told}");
        assert!(sent.is_empty());
        // Ana confirmed with f1, f1 still the one near face: kept nothing,
        // nothing asked, nothing sent.
        let eyes_now = std::sync::Arc::new(crate::speaking_faces::SpeakingFaces::default());
        sees_tracks_with(Some(eyes_now.clone()));
        look_now(&eyes_now, 1, &[("f1", 0.3, None)]);
        if let Ok(mut confirmed) = CONFIRMED.lock() {
            *confirmed = Some(("Ana".into(), "b/f1".into()));
        }
        assert!(reused_name("Ana"));
        let (told, sent) = switched(None, "Remember my face.");
        assert!(
            [NOT_KEPT_NO_ASK, CANT_KEEP_NO_ASK].contains(&told["error"].as_str().unwrap_or("")),
            "{told}"
        );
        assert!(sent.is_empty(), "{sent:?}");
        // The speaker-bound route: known as Ana (face eyes/f1) is told so
        // first only when the line's own speaking face is eyes/f1. An
        // off-camera voice (no face seen saying it), another face saying
        // it, or no line read, goes on as before: refused with no speaking
        // proof, nothing sent, the confirmed name not reused.
        let speaker = |known: Option<&str>, said_by: Option<Option<&str>>| {
            let (socket, sent) = eyes(vec![]);
            if let Some(known) = known {
                std::fs::write(known_file_of(&socket), format!("{known}\n")).unwrap();
            }
            let line = said_by.map(|face| {
                let now = lamp_ipc::monotonic_us();
                crate::speaking_faces::LineSeen {
                    request: 7_901_001,
                    known: known.map(|known| (known.to_owned(), "eyes/f1".to_owned())),
                    enrollment: face.map(|face| {
                        crate::remember_speaker::Proof::heard(
                            face.to_owned(),
                            now - 1_000_000,
                            now,
                            "Do you remember me? It's Ana.",
                            crate::remember_speaker::generation(),
                        )
                        .unwrap()
                    }),
                    ..Default::default()
                }
            });
            let told = run_scoped(
                &socket,
                &save,
                no_ask(true),
                None,
                line.as_ref(),
                7_901_001,
                None,
            );
            let sent = sent.join().unwrap();
            let _ = std::fs::remove_dir_all(socket.parent().unwrap());
            (told, sent)
        };
        let (told, sent) = speaker(Some("Ana"), Some(Some("eyes/f1")));
        assert_eq!(told.0["already_known"], "Ana", "{}", told.0);
        assert!(sent.is_empty());
        for said_by in [None, Some(None), Some(Some("eyes/f2"))] {
            let (told, sent) = speaker(Some("Ana"), said_by);
            assert!(
                told.0.get("already_known").is_none(),
                "{said_by:?} {}",
                told.0
            );
            assert!(told.0.get("remembered").is_none(), "{said_by:?} {}", told.0);
            assert!(sent.is_empty(), "{said_by:?} {sent:?}");
        }
        let (told, sent) = speaker(None, Some(Some("eyes/f1")));
        assert!(told.0.get("already_known").is_none(), "{}", told.0);
        assert!(told.0.get("remembered").is_none(), "{}", told.0);
        assert!(sent.is_empty());
        forget_confirmed();
        sees_tracks_with(None);
        // The eyes' `already`: known already, never on a rename.
        let answer = json!({"remembered": "Ana", "replaced": [], "already": true});
        let told = json!({"ok": true, "remembered": "Ana"});
        assert_eq!(
            told_already(told.clone(), &answer, no_ask(true), false)["already_known"],
            "Ana"
        );
        assert_eq!(
            told_already(told.clone(), &answer, no_ask(true), true),
            told
        );
        let off = FaceSwitches {
            face_question: false,
            ..no_ask(true)
        };
        assert_eq!(told_already(told.clone(), &answer, off, false), told);
        // A rename the eyes made (`replaced` not empty) drops the visits
        // line said at the first save, keeping every other line; a replace
        // that replaced nothing is a first save and keeps it. Off, as before.
        let renamed = json!({"remembered": "Ben", "replaced": ["Ana"], "visits": true});
        let first = json!({"remembered": "Ben", "replaced": [], "visits": true});
        let with_line =
            |also_say: String| json!({"ok": true, "remembered": "Ben", "also_say": also_say});
        assert_eq!(
            told_already(with_line(VISITS_LINE.into()), &renamed, no_ask(true), true),
            json!({"ok": true, "remembered": "Ben"})
        );
        for other in [ASK_TO_NAME, VISITS_UNDO] {
            assert_eq!(
                told_already(
                    with_line(format!("{VISITS_LINE} {other}")),
                    &renamed,
                    no_ask(true),
                    true
                ),
                with_line(other.into())
            );
        }
        let kept = with_line(VISITS_LINE.into());
        assert_eq!(told_already(kept.clone(), &first, no_ask(true), true), kept);
        assert_eq!(told_already(kept.clone(), &renamed, off, true), kept);
    }

    /// The person, 20 Nov 23:4x: a rename (the face re-saved under a
    /// corrected name, Ana to Ben) is not told the visits line again; they
    /// heard it when the face was first kept. Off, as before.
    #[test]
    fn with_face_question_a_rename_is_not_told_the_visits_line_again() {
        let _serial = FACE_QUESTION_SERIAL
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let call = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({"name": "Ben", "replace": true}),
        };
        let yes = Said {
            this: "Yes, call me Ben.".into(),
            before: vec![("Call me Ben.".into(), "Shall I call you Ben?".into())],
        };
        let run = |face_question: bool, replaced: Value| {
            let (socket, sent) = eyes(vec![json!({
                "remembered": "Ben", "replaced": replaced, "visits": true
            })]);
            std::fs::write(known_file_of(&socket), "Ana\n").unwrap();
            let (told, _) = run_switched(
                &socket,
                &call,
                FaceSwitches {
                    checked: true,
                    rename_yes: true,
                    name_learn_fix: true,
                    face_question,
                    ..FaceSwitches::default()
                },
                &yes,
                None,
                &mut None,
            );
            assert_eq!(
                sent.join().unwrap(),
                vec![json!({"remember": "Ben", "one_face": true, "replace": true})]
            );
            let _ = std::fs::remove_dir_all(socket.parent().unwrap());
            told
        };
        let on = run(true, json!(["Ana"]));
        assert_eq!(on["ok"], true, "{on}");
        assert_eq!(on["remembered"], "Ben", "{on}");
        assert!(on.get("already_known").is_none(), "{on}");
        assert!(!on.to_string().contains(VISITS_LINE), "{on}");
        // Nothing replaced: a first save, the line kept.
        let first = run(true, json!([]));
        assert!(
            first["also_say"]
                .as_str()
                .is_some_and(|line| line.contains(VISITS_LINE)),
            "{first}"
        );
        // The switch off: as before, the line said.
        let off = run(false, json!(["Ana"]));
        assert!(
            off["also_say"]
                .as_str()
                .is_some_and(|line| line.contains(VISITS_LINE)),
            "{off}"
        );
    }

    /// The owner, 20 Nov 02:18: "I already know your face" only on a fresh
    /// real match. A name the eyes hold (their fresh held mark), or eyes
    /// whose held names can't be told (no `LAMP_FACE_STICKY`), never
    /// short-circuit: "Remember me as Ana." from a face held as Ana goes to
    /// the eyes, whose fresh look answers `not_like_them` or `already`.
    #[test]
    fn with_face_question_a_held_name_goes_to_the_eyes() {
        let _serial = FACE_QUESTION_SERIAL
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let save = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({"name": "Ana"}),
        };
        let switches = FaceSwitches {
            checked: true,
            rename_yes: true,
            name_learn_fix: true,
            face_question: true,
            ..FaceSwitches::default()
        };
        let asked = vec![json!({"remember": "Ana", "one_face": true, "replace": false})];
        let run = |marks: bool, held: bool, answers: Vec<Value>| {
            TEST_HELD_MARKS.with(|on| on.set(marks));
            let (socket, sent) = eyes(answers);
            let known = known_file_of(&socket);
            std::fs::write(&known, "Ana\n").unwrap();
            if held {
                std::fs::write(held_mark_of(&known), b"").unwrap();
            }
            let (told, _) = run_switched(
                &socket,
                &save,
                switches,
                &Said::now("Remember me as Ana."),
                None,
                &mut None,
            );
            let sent = sent.join().unwrap();
            let _ = std::fs::remove_dir_all(socket.parent().unwrap());
            TEST_HELD_MARKS.with(|on| on.set(false));
            (told, sent)
        };
        // Held as Ana, another face: the eyes refuse, nothing said known.
        let (told, sent) = run(true, true, vec![json!({"error": "not_like_them"})]);
        assert_eq!(told, json!({"error": NOT_LIKE_THEM}), "{told}");
        assert_eq!(sent, asked);
        // Held marks not read: to the eyes too; their fresh match says so.
        let already = json!({"remembered": "Ana", "replaced": [], "already": true});
        let (told, sent) = run(false, false, vec![already]);
        assert_eq!(told["already_known"], "Ana", "{told}");
        assert_eq!(told["remembered"], "Ana", "{told}");
        assert_eq!(sent, asked);
        // Marks read, none held: the fresh match short-circuits.
        let (told, sent) = run(true, false, vec![]);
        assert_eq!(told["already_known"], "Ana", "{told}");
        assert!(told.get("remembered").is_none(), "{told}");
        assert!(sent.is_empty(), "{sent:?}");
    }

    /// The owner, 20 Nov: the rename path keeps working with
    /// `LAMP_FACE_QUESTION_NOT_ASK`. A known Ana's "Call me Jay." reaches
    /// the eyes and their `known_as` asks the rename; the yes with
    /// `replace` keeps Jay, an `already` from the eyes on it is no "known
    /// already", and a confirmed name is never reused for a rename: each
    /// as with the switch off.
    #[test]
    fn with_face_question_a_rename_runs_as_today() {
        let _serial = FACE_QUESTION_SERIAL
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let switches = |face_question: bool| FaceSwitches {
            checked: true,
            rename_yes: true,
            name_learn_fix: true,
            face_question,
            ..FaceSwitches::default()
        };
        let call = |args: Value| ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args,
        };
        let run = |on: bool, args: Value, said: &Said, answers: Vec<Value>| {
            let (socket, sent) = eyes(answers);
            std::fs::write(known_file_of(&socket), "Ana\n").unwrap();
            let (told, _) = run_switched(&socket, &call(args), switches(on), said, None, &mut None);
            let sent = sent.join().unwrap();
            let _ = std::fs::remove_dir_all(socket.parent().unwrap());
            (told, sent)
        };
        // "Call me Jay." from the face known as Ana: the eyes ask.
        let asked = Said::now("Call me Jay.");
        let known_as = || vec![json!({"error": "known_as", "known_as": "Ana"})];
        let on = run(true, json!({"name": "Jay"}), &asked, known_as());
        assert_eq!(on, run(false, json!({"name": "Jay"}), &asked, known_as()));
        assert_eq!(
            rename_asked(&on.0),
            Some(Some("Ana".to_owned())),
            "{}",
            on.0
        );
        assert_eq!(
            on.1,
            vec![json!({"remember": "Jay", "one_face": true, "replace": false})]
        );
        // Their yes to "Shall I call you Jay?": kept as Jay in place of Ana.
        let yes = Said {
            this: "Yes, call me Jay.".into(),
            before: vec![("Call me Jay.".into(), "Shall I call you Jay?".into())],
        };
        let renamed = || vec![json!({"remembered": "Jay", "replaced": ["Ana"], "already": false})];
        let args = json!({"name": "Jay", "replace": true});
        let on = run(true, args.clone(), &yes, renamed());
        assert_eq!(on, run(false, args.clone(), &yes, renamed()));
        assert_eq!(on.0["remembered"], "Jay", "{}", on.0);
        assert!(on.0.get("already_known").is_none(), "{}", on.0);
        assert_eq!(
            on.1,
            vec![json!({"remember": "Jay", "one_face": true, "replace": true})]
        );
        // An `already` on a rename is no "known already".
        let marked = || vec![json!({"remembered": "Jay", "replaced": ["Ana"], "already": true})];
        let (told, _) = run(true, args.clone(), &yes, marked());
        assert!(told.get("already_known").is_none(), "{told}");
        // A rename with no yes to the new name is refused as today, even
        // with Jay confirmed and the same one near face in view.
        let eyes_now = std::sync::Arc::new(crate::speaking_faces::SpeakingFaces::default());
        sees_tracks_with(Some(eyes_now.clone()));
        look_now(&eyes_now, 1, &[("f1", 0.3, None)]);
        if let Ok(mut confirmed) = CONFIRMED.lock() {
            *confirmed = Some(("Jay".into(), "b/f1".into()));
        }
        let no_yes = Said::now("Remember my face.");
        let on = run(true, args.clone(), &no_yes, vec![]);
        assert_eq!(on, run(false, args, &no_yes, vec![]));
        assert!(refused_here(&on.0), "{}", on.0);
        assert!(on.1.is_empty());
        forget_confirmed();
        sees_tracks_with(None);
    }

    /// The description says a question whether the lamp knows them is no
    /// ask, checked or not, only with the switch.
    #[test]
    fn with_face_question_the_description_says_a_question_is_no_ask() {
        for checked in [false, true] {
            let described = |face_question: bool| {
                tools_for(FaceSwitches {
                    checked,
                    face_question,
                    ..FaceSwitches::default()
                })
                .into_iter()
                .find(|tool| tool.name == REMEMBER)
                .unwrap()
                .description
            };
            let (on, off) = (described(true), described(false));
            assert!(on.ends_with(QUESTION_NOT_ASK), "{on}");
            assert!(on.contains("\"Do you remember me?\""), "{on}");
            assert!(!off.contains("Do you remember me"), "{off}");
            // Unchecked, the description asks for a told name, so the
            // nameless call is not offered (Seventh's read on #8218).
            let call = if checked { ASK_IS_A_CALL } else { "" };
            assert_eq!(on, format!("{off}{call}{QUESTION_NOT_ASK}"));
            assert_eq!(on.contains(ASK_IS_A_CALL), checked, "{on}");
            // #8053: an ask is a call in any turn, after a save or before
            // a name, and a question still never is (it comes last).
            assert!(ASK_IS_A_CALL.contains("every clear ask"), "{on}");
            assert!(ASK_IS_A_CALL.contains("kept their face earlier"), "{on}");
            assert!(ASK_IS_A_CALL.contains("empty name"), "{on}");
            assert!(!off.contains("every clear ask"), "{off}");
            assert!(on.find(ASK_IS_A_CALL) < on.find(QUESTION_NOT_ASK), "{on}");
        }
    }

    /// #8053: the description says a call before their name is told there
    /// was no name, so the model asks for it ([`ASK_IS_A_CALL`]). An empty or blank name, with the
    /// switch on, is told [`NO_NAME`] here and nothing reaches the eyes:
    /// no look, nothing kept, as with it off.
    #[test]
    fn with_face_question_a_call_with_no_name_is_told_to_ask_and_sends_nothing() {
        let switches = |face_question: bool| FaceSwitches {
            checked: true,
            rename_yes: true,
            name_learn_fix: true,
            face_question,
            ..FaceSwitches::default()
        };
        for args in [json!({"name": ""}), json!({"name": "  "}), json!({})] {
            for on in [true, false] {
                let (socket, sent) = eyes(vec![]);
                let call = ToolCall {
                    id: "1".into(),
                    name: REMEMBER.into(),
                    args: args.clone(),
                };
                let (told, _) = run_switched(
                    &socket,
                    &call,
                    switches(on),
                    &Said::now("Please remember my face."),
                    None,
                    &mut None,
                );
                let sent = sent.join().unwrap();
                let _ = std::fs::remove_dir_all(socket.parent().unwrap());
                assert_eq!(told["error"], NO_NAME, "{args} {told}");
                assert!(refused_here(&told), "{told}");
                assert!(sent.is_empty(), "{sent:?}");
            }
        }
    }

    /// `LAMP_FACE_NO_ASK` with a face the eyes' known file names at the
    /// lamp (Heron's and Migrate's HOLDs on #7800): "Call me Jay." is no new
    /// face kept unasked; it reaches the eyes, whose "known as" answer holds
    /// the rename for that person's yes (#7653), never [`NOT_KEPT_NO_ASK`].
    #[test]
    fn with_no_ask_a_known_face_giving_another_name_still_reaches_the_rename() {
        let save = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({"name": "Jay"}),
        };
        let (socket, sent) = eyes(vec![json!({"error": "known_as", "known_as": "Walter"})]);
        std::fs::write(known_file_of(&socket), "Walter\n").unwrap();
        let switches = FaceSwitches {
            checked: true,
            rename_yes: true,
            name_learn_fix: true,
            face_no_ask: true,
            ..FaceSwitches::default()
        };
        let (told, _) = run_switched(
            &socket,
            &save,
            switches,
            &Said::now("Call me Jay."),
            None,
            &mut None,
        );
        assert_ne!(told["error"], NOT_KEPT_NO_ASK, "{told}");
        assert!(!refused_here(&told), "{told}");
        assert_eq!(
            rename_asked(&told),
            Some(Some("Walter".to_owned())),
            "{told}"
        );
        assert_eq!(
            sent.join().unwrap(),
            vec![json!({"remember": "Jay", "one_face": true, "replace": false})]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// `LAMP_FACE_NO_ASK` on the route with no speaking-face proof
    /// (Thirteen's and Heron's HOLDs on #7787; the first builder 21:13):
    /// `one_face` is only one face in view, so no new face is kept here and
    /// nothing is asked. "I'm Ada", a TV or off-camera "I'm Anna", a
    /// parent's "This is my son Tom" and a yes to "Tom, right?", and
    /// "Remember my face, this is Tom" send nothing and are told to say
    /// nothing about the face ([`NOT_KEPT_NO_ASK`]), with
    /// `LAMP_NAME_LEARN_FIX` on or off. Their own ask under their own name,
    /// or a yes to a face question, sends nothing either and is told the
    /// plain reason, still asking nothing ([`CANT_KEEP_NO_ASK`]). Off, the
    /// face question as before. "Forget my face" still forgets, a rename
    /// still runs, and the setup walk's first save is untouched.
    #[test]
    fn with_no_ask_and_no_speaker_proof_nothing_is_kept_or_asked() {
        let switches = |face_no_ask: bool, name_learn_fix: bool| FaceSwitches {
            checked: true,
            rename_yes: true,
            name_learn_fix,
            face_no_ask,
            ..FaceSwitches::default()
        };
        let call = |function: &str, name: &str| ToolCall {
            id: "1".into(),
            name: function.into(),
            args: json!({"name": name}),
        };
        let after = |person: &str, lamp: &str, this: &str| Said {
            this: this.into(),
            before: vec![(person.into(), lamp.into())],
        };
        let (socket, sent) = eyes(vec![
            json!({"remembered": "Tom", "replaced": []}),
            json!({"remembered": "Ada", "replaced": []}),
            json!({"forgot": true}),
            json!({"remembered": "Ada", "replaced": ["Ana"]}),
        ]);
        let run = |on: bool, learn: bool, function: &str, name: &str, said: &Said| {
            run_switched(
                &socket,
                &call(function, name),
                switches(on, learn),
                said,
                None,
                &mut None,
            )
            .0
        };
        let asks = |told: &Value| {
            let text = told.to_string();
            [
                "Shall I",
                "May I",
                "Should I",
                "Do you want",
                "Ask them only",
            ]
            .iter()
            .any(|ask| text.contains(ask))
        };
        let face_question = "Nice to meet you, Ada! Shall I remember your face as Ada?";
        // On: nothing kept, nothing asked, nothing said about the face.
        for (name, said) in [
            ("Ada", Said::now("I'm Ada.")),
            ("Anna", Said::now("I'm Anna.")),
            ("Ada", Said::now("My name is Ada.")),
            ("Tom", after("This is my son Tom.", "Tom, right?", "Yes.")),
            ("Tom", Said::now("Remember my face, this is Tom.")),
        ] {
            for learn in [true, false] {
                let told = run(true, learn, REMEMBER, name, &said);
                assert_eq!(told["error"], NOT_KEPT_NO_ASK, "{learn} {said:?}: {told}");
                assert!(!asks(&told), "{learn} {said:?}: {told}");
                assert!(refused_here(&told));
            }
        }
        // On, their own ask in words: the plain reason, nothing asked.
        for said in [
            Said::now("Remember my face, my name is Ada."),
            after("My name is Ada.", face_question, "Yes."),
        ] {
            for learn in [true, false] {
                let told = run(true, learn, REMEMBER, "Ada", &said);
                assert_eq!(told["error"], CANT_KEEP_NO_ASK, "{learn} {said:?}: {told}");
                assert!(!asks(&told), "{learn} {said:?}: {told}");
                assert!(refused_here(&told));
            }
        }
        // Off, as before: a name alone is asked; the face-leave clause
        // keeps under any name.
        for (name, said) in [
            ("Ada", Said::now("I'm Ada.")),
            ("Tom", after("This is my son Tom.", "Tom, right?", "Yes.")),
        ] {
            let told = run(false, true, REMEMBER, name, &said);
            assert_eq!(told["error"], ASK_FACE_FIRST, "{said:?}: {told}");
        }
        let off = run(
            false,
            true,
            REMEMBER,
            "Tom",
            &Said::now("Remember my face, this is Tom."),
        );
        assert_eq!(off["ok"], true, "{off}");
        let yes = after("My name is Ada.", face_question, "Yes.");
        assert_eq!(run(false, true, REMEMBER, "Ada", &yes)["ok"], true);
        // "Forget my face" still forgets.
        let forgot = run(true, true, FORGET, "Ada", &Said::now("Forget my face."));
        assert_eq!(forgot["ok"], true);
        // A rename agreed to still runs.
        let rename = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({"name": "Ada", "replace": true}),
        };
        let renamed = run_switched(
            &socket,
            &rename,
            switches(true, true),
            &after("Call me Ada.", "Ada, did I get that right?", "Yes."),
            None,
            &mut None,
        )
        .0;
        assert_eq!(renamed["ok"], true, "{renamed}");
        // The setup walk's first save on this route has no speaking-face
        // proof either: nothing sent, nothing kept, nothing asked (#7810).
        let setup = run_switched(
            &socket,
            &call(REMEMBER, "Ada"),
            FaceSwitches {
                at_setup: true,
                ..switches(true, true)
            },
            &Said::now("My name is Ada."),
            None,
            &mut None,
        )
        .0;
        assert_eq!(setup["error"], SETUP_NOT_KEPT_NO_ASK, "{setup}");
        let remember = |name: &str| json!({"remember": name, "one_face": true, "replace": false});
        assert_eq!(
            sent.join().unwrap(),
            [
                remember("Tom"),
                remember("Ada"),
                json!({"forget": "Ada"}),
                json!({"remember": "Ada", "one_face": true, "replace": true}),
            ]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// `LAMP_FACE_NO_ASK` with Who was here's names on (the first builder
    /// 19:47: [`ASK_TO_NAME`] is in the no-asks rule). A face kept is not
    /// asked whether it may be named: the voice gives the eyes its yes, and
    /// the model says the undo line once in the question's place, only if
    /// the eyes took it. "Keep my visits private" is the existing no
    /// ([`DONT_TELL`]). Off, the question as before.
    #[test]
    fn with_no_ask_being_named_is_taken_and_the_undo_said_once() {
        let switches = |face_no_ask: bool| FaceSwitches {
            checked: true,
            visits: true,
            face_no_ask,
            ..FaceSwitches::default()
        };
        let save = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({"name": "Ada"}),
        };
        let kept = json!({"remembered": "Ada", "replaced": []});
        let counted = json!({"remembered": "Ada", "replaced": [], "visits": true});
        let (socket, sent) = eyes(vec![
            kept.clone(),
            json!({"visits": "yes"}),
            counted,
            json!({"visits": "yes"}),
            kept.clone(),
            json!({"error": "not_here"}),
            kept,
            json!({"visits": "no"}),
        ]);
        // Past setup this route keeps no new face with the switch on
        // (the first builder 21:13): a rename agreed to is kept.
        let rename = ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({"name": "Ada", "replace": true}),
        };
        let said = Said {
            this: "Yes.".into(),
            before: vec![("Call me Ada.".into(), "Ada, did I get that right?".into())],
        };
        let run = |on: bool| run_switched(&socket, &rename, switches(on), &said, None, &mut None).0;
        // On: taken, and the undo said once in the question's place.
        let told = run(true);
        assert_eq!(told["ok"], true, "{told}");
        assert_eq!(told["also_say"], VISITS_UNDO, "{told}");
        assert!(!told.to_string().contains(ASK_TO_NAME));
        assert!(VISITS_UNDO.contains("Say 'keep my visits private' to undo."));
        // On, visits counted: that line stays beside the undo.
        let told = run(true);
        assert_eq!(
            told["also_say"],
            format!("{VISITS_LINE} {VISITS_UNDO}"),
            "{told}"
        );
        // On, the eyes didn't take the yes: nothing asked, nothing claimed.
        let told = run(true);
        assert_eq!(told["ok"], true, "{told}");
        assert!(told.get("also_say").is_none(), "{told}");
        // Off: the question as before, and no yes sent.
        assert_eq!(run(false)["also_say"], ASK_TO_NAME);
        // The setup walk's save, on, off the speaker route: no speaking-face
        // proof, so nothing is sent, kept or asked, the visits ask included
        // (#7810), and no yes is sent.
        let read_back = Said {
            this: "Yes.".into(),
            before: vec![(
                "My name is Ada.".into(),
                "Ada, did I get that right?".into(),
            )],
        };
        let setup = run_switched(
            &socket,
            &save,
            FaceSwitches {
                at_setup: true,
                ..switches(true)
            },
            &read_back,
            None,
            &mut None,
        )
        .0;
        assert_eq!(setup, json!({"error": SETUP_NOT_KEPT_NO_ASK}));
        // "Keep my visits private": the existing no, named in its tool.
        let undo = ToolCall {
            id: "2".into(),
            name: DONT_TELL.into(),
            args: json!({"name": "Ada"}),
        };
        assert_eq!(run_visits(&socket, &undo, None).0["ok"], true);
        let description = |on: bool| {
            tools_for(switches(on))
                .into_iter()
                .find(|tool| tool.name == DONT_TELL)
                .unwrap()
                .description
        };
        assert!(description(true).contains("keep my visits private"));
        assert!(!description(false).contains("keep my visits private"));
        let remember = json!({"remember": "Ada", "one_face": true, "replace": true});
        let yes = json!({"visits_yes": "Ada"});
        assert_eq!(
            sent.join().unwrap(),
            [
                remember.clone(),
                yes.clone(),
                remember.clone(),
                yes.clone(),
                remember.clone(),
                yes,
                remember,
                json!({"visits_no": "Ada"}),
            ]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// #7331 (V2 on lamp-74ff, 16 Nov): a name that sounds like a letter
    /// is written by speech to text as the letter, so a rename heard as the
    /// letter, said as the word or spelled was judged three names. With
    /// `LAMP_NAME_LEARN_FIX` they are one name for the rename's checks:
    /// heard "J", "Jay" and spelled "J a y" (made up). Off, refused as
    /// before.
    #[test]
    fn a_name_heard_as_its_letter_is_the_same_name() {
        let switches = |name_learn_fix: bool| FaceSwitches {
            checked: true,
            rename_yes: true,
            name_learn_fix,
            ..FaceSwitches::default()
        };
        let rename = |name: &str| ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({"name": name, "replace": true}),
        };
        let said = |person: &str, lamp: &str, this: &str| Said {
            this: this.into(),
            before: vec![(person.into(), lamp.into())],
        };
        let cases = [
            // Heard as the letter, read back as the word, then a yes.
            (
                "Jay",
                said("Call me J.", "Shall I call you Jay instead?", "Yes."),
            ),
            // The model kept the letter; the yes spells the word.
            (
                "J",
                said("Call me Jay.", "Shall I call you J instead?", "Yes, J a y."),
            ),
        ];
        let (socket, sent) = eyes(
            cases
                .iter()
                .map(|(name, _)| json!({"remembered": name, "replaced": ["Tove"]}))
                .collect(),
        );
        for (name, said) in &cases {
            let (off, _) = run_switched(
                &socket,
                &rename(name),
                switches(false),
                said,
                None,
                &mut None,
            );
            assert!(refused_here(&off), "off, {name}, {said:?}: {off}");
            let (on, _) = run_switched(
                &socket,
                &rename(name),
                switches(true),
                said,
                None,
                &mut None,
            );
            assert_eq!(on["ok"], true, "on, {name}, {said:?}: {on}");
        }
        // Another letter, or a name not said, stays refused.
        for (name, said) in [
            (
                "Jay",
                said("Call me K.", "Shall I call you Jay instead?", "Yes."),
            ),
            (
                "Jay",
                said(
                    "Call me Jay.",
                    "Shall I call you Jay instead?",
                    "Yes, K a y.",
                ),
            ),
        ] {
            let (on, _) = run_switched(
                &socket,
                &rename(name),
                switches(true),
                &said,
                None,
                &mut None,
            );
            assert!(refused_here(&on), "{said:?}: {on}");
        }
        // The eyes keep the name as the model gave it.
        assert_eq!(
            sent.join().unwrap(),
            cases
                .iter()
                .map(|(name, _)| json!({"remember": name, "one_face": true, "replace": true}))
                .collect::<Vec<_>>()
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// #7331 (the Reviewer-lead's P2-1 on #7343, the first builder at
    /// 21:03): a spelled name was kept with no word of what was kept, so
    /// letters misheard (B/D, M/N) went unseen. With `LAMP_NAME_LEARN_FIX`,
    /// a name kept from letters spelled to it, at setup or on a rename, is
    /// said back as a statement, the word and its letters ("Ama: A, M, A.
    /// Got it."), and one line right after that fixes a letter ("No, N as
    /// in nose.") or spells it again renames it with no further yes. Off,
    /// nothing is said back and such a line is refused.
    #[test]
    fn a_spelled_name_kept_is_said_back_and_one_line_corrects_it() {
        let switches = |name_learn_fix: bool, at_setup: bool| FaceSwitches {
            checked: true,
            rename_yes: true,
            name_learn_fix,
            at_setup,
            ..FaceSwitches::default()
        };
        let call = |name: &str, replace: bool| ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({"name": name, "replace": replace}),
        };
        let after = |lamp: &str, this: &str| Said {
            this: this.into(),
            before: vec![("Call me something else.".into(), lamp.into())],
        };
        let spelled = after("How do you spell it?", "A, m, a.");
        let read_back = "Ama: A, M, A. Got it.";
        let (socket, sent) = eyes(vec![
            json!({"remembered": "Ama", "replaced": ["Tove"]}),
            json!({"remembered": "Ama"}),
            json!({"remembered": "Ama"}),
            json!({"remembered": "Ana", "replaced": ["Ama"]}),
            json!({"remembered": "Ana", "replaced": ["Ama"]}),
            json!({"remembered": "Ana", "replaced": ["Ama"]}),
        ]);
        let run = |call: &ToolCall, switches: FaceSwitches, said: &Said| {
            run_switched(&socket, call, switches, said, None, &mut None).0
        };
        // Kept from letters, on a rename and at setup: said back.
        let told = run(&call("Ama", true), switches(true, false), &spelled);
        assert_eq!(told["say_back"], read_back, "{told}");
        assert_eq!(told["note"], SPELLED_KEPT);
        let told = run(&call("Ama", false), switches(true, true), &spelled);
        assert_eq!(told["say_back"], read_back, "{told}");
        // Off: kept at setup as before, nothing said back.
        let told = run(&call("Ama", false), switches(false, true), &spelled);
        assert_eq!(told["ok"], true, "{told}");
        assert_eq!(told.get("say_back"), None);
        // One line right after the read-back corrects it, on a rename and
        // at setup, and the name it makes is said back again.
        for (this, at_setup) in [
            ("No, N as in nose.", false),
            ("No, it's A n a.", false),
            ("No, N as in nose.", true),
        ] {
            let said = after(read_back, this);
            let off = run(&call("Ana", true), switches(false, at_setup), &said);
            assert!(refused_here(&off), "off, {this}: {off}");
            let on = run(&call("Ana", true), switches(true, at_setup), &said);
            assert_eq!(on["say_back"], "Ana: A, N, A. Got it.", "{this}: {on}");
        }
        // A line that fixes another letter, more letters than it names,
        // or another name, renames nothing.
        for (name, this) in [
            ("Ava", "No, N as in nose."),
            ("Nna", "No, N as in nose."),
            ("Linh", "No, N as in nose."),
            ("Ana", "Nice, thanks."),
        ] {
            let on = run(
                &call(name, true),
                switches(true, false),
                &after(read_back, this),
            );
            assert!(refused_here(&on), "{name}, {this}: {on}");
        }
        assert_eq!(sent.join().unwrap().len(), 6);
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// #5564 (lamp-3e83, 6 Nov): a spelled name came back to the person as
    /// letters, "Jane or J-A-N-E?". A name written as its letters is the
    /// word they make, properly cased; a name that is not only letters, and
    /// initials of two, stay as they are.
    #[test]
    fn a_name_written_as_its_letters_is_the_word_they_make() {
        for (name, word) in [
            ("J-A-N-E", "Jane"),
            ("J A N E.", "Jane"),
            ("j.a.n.e.", "Jane"),
            ("L, O, R, E, N", "Loren"),
            ("A double N", "Ann"),
        ] {
            assert_eq!(letters_as_word(name).as_deref(), Some(word), "{name}");
        }
        for name in [
            "Jane",
            "Mary Ann",
            "A J",
            "T.J.",
            "Q Q Q",
            "Jane J-A-N-E",
            "",
        ] {
            assert_eq!(letters_as_word(name), None, "{name}");
        }
    }

    /// `LAMP_FACE_NO_ASK` at setup (the product owner's no-asks rule, 18
    /// Nov; the first builder's ruling on #7810): off the speaker-bound
    /// route there is no proof that the face in view said the name, so with
    /// one face alone in view and the name heard nothing is sent, nothing
    /// kept and nothing asked (no read-back, no face question); the step
    /// ends with the name only. "Forget my face" still forgets: the eyes'
    /// `forgot` is told `ok`, as every forget is ([`told_asking`]).
    /// Switched off, the save waits for the read-back's yes as before
    /// ([`SAY_IT_BACK`]).
    #[test]
    fn with_no_ask_the_setup_face_is_kept_only_on_the_speakers_proof() {
        let switches = |face_no_ask: bool| FaceSwitches {
            checked: true,
            at_setup: true,
            face_no_ask,
            ..FaceSwitches::default()
        };
        let call = |function: &str| ToolCall {
            id: "1".into(),
            name: function.into(),
            args: json!({"name": "Walter"}),
        };
        let named = Said::now("My name is Walter.");
        let (socket, asked) = eyes(vec![json!({"forgot": true})]);
        let run = |on: bool, function: &str| {
            run_switched(
                &socket,
                &call(function),
                switches(on),
                &named,
                None,
                &mut None,
            )
            .0
        };
        let asks = |told: &Value| {
            let text = told.to_string();
            ["May I", "Shall I", "did I get", "ask if you have it right"]
                .iter()
                .any(|ask| text.contains(ask))
        };

        // Switched off: the read-back's yes first, nothing sent.
        assert_eq!(run(false, REMEMBER), json!({"error": SAY_IT_BACK}));

        // On, one face alone in view and the name heard, but no proof it
        // said it: nothing kept, nothing asked, no "I'll know you".
        let told = run(true, REMEMBER);
        assert_eq!(told, json!({"error": SETUP_NOT_KEPT_NO_ASK}));
        assert!(!asks(&told), "{told}");
        assert!(!told.to_string().contains("I'll know you"), "{told}");
        assert_eq!(
            refusal_reason(SETUP_NOT_KEPT_NO_ASK),
            "setup_not_kept_no_ask"
        );

        // "Forget my face" still forgets.
        assert_eq!(run(true, FORGET), json!({"ok": true}));

        let sent = asked.join().unwrap();
        assert_eq!(sent, [json!({"forget": "Walter"})], "{sent:?}");
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// #5564: with names checked, a name the model gives as its letters is
    /// kept as the word they make, at setup and past it, so the lamp says it
    /// back and greets them by it, never by its letters. Before the first
    /// save at setup it is still said back for a yes (#5140 row 5), and the
    /// yes to "Jane, did I get that right?" keeps "Jane". Names unchecked,
    /// as before.
    #[test]
    fn a_name_given_as_its_letters_is_kept_as_the_word() {
        let call = |name: &str| ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({ "name": name }),
        };
        let (socket, asked) = eyes(vec![
            json!({"remembered": "Jane"}),
            json!({"remembered": "Jane"}),
            json!({"remembered": "Jane"}),
            json!({"remembered": "J-A-N-E"}),
        ]);
        // Spelled after the lamp asked: kept as the word, with no yes.
        let asked_to_spell = Said {
            this: "J A N E.".into(),
            before: vec![(
                "I'm Jain.".into(),
                "Sorry, could you spell your name for me?".into(),
            )],
        };
        let (words, _) = run_at_setup(&socket, &call("J-A-N-E"), &asked_to_spell);
        assert_eq!(words, json!({"ok": true, "remembered": "Jane"}));
        // Spelled unasked: said back first, as the word only.
        let (words, _) = run_at_setup(&socket, &call("J-A-N-E"), &Said::now("I'm J A N E."));
        assert_eq!(words, json!({"error": SAY_IT_BACK}));
        let said_back = Said {
            this: "Yes.".into(),
            before: vec![("I'm J A N E.".into(), "Jane, did I get that right?".into())],
        };
        let (words, _) = run_at_setup(&socket, &call("J-A-N-E"), &said_back);
        assert_eq!(words, json!({"ok": true, "remembered": "Jane"}));
        // Past setup, as the word too.
        let (words, _) = run_with(
            &socket,
            &call("J-A-N-E"),
            true,
            &Said::now("My name is J-A-N-E."),
        );
        assert_eq!(words, json!({"ok": true, "remembered": "Jane"}));
        // Names unchecked: the name goes as the model gave it, as before.
        let (words, _) = run_with(&socket, &call("J-A-N-E"), false, &Said::default());
        assert_eq!(words, json!({"ok": true, "remembered": "J-A-N-E"}));
        assert_eq!(
            asked.join().unwrap(),
            [
                json!({"remember": "Jane", "one_face": true, "replace": false}),
                json!({"remember": "Jane", "one_face": true, "replace": false}),
                json!({"remember": "Jane", "one_face": true, "replace": false}),
                json!({"remember": "J-A-N-E"}),
            ]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// #5564 (lamp-3e83): the lamp asked people to spell their name too
    /// often. A name not heard asks for a spelling once ([`NOT_HEARD`]);
    /// once the lamp asked them to spell in an exchange the check holds, a
    /// name still not heard is told [`NOT_HEARD_SPELLED`], which never asks
    /// again and says the name back as a name, at setup and past it.
    /// #5140 row 6 still holds: their letters after the ask are kept.
    #[test]
    fn a_spelling_is_asked_for_once_and_never_again() {
        let call = |name: &str| ToolCall {
            id: "1".into(),
            name: REMEMBER.into(),
            args: json!({ "name": name }),
        };
        let (socket, asked) = eyes(vec![json!({"remembered": "Loren"})]);
        let ask = "Sorry, could you spell your name for me?";
        let (words, _) = run_at_setup(&socket, &call("Lauren"), &Said::now("I'm Lord in."));
        assert_eq!(words, json!({"error": NOT_HEARD}));
        let (words, _) = run_with(&socket, &call("Lauren"), true, &Said::now("I'm Lord in."));
        assert_eq!(words, json!({"error": NOT_HEARD}));
        for said in [
            // The spelling just asked for, and not the model's name.
            Said {
                this: "Four letters. L O R E N.".into(),
                before: vec![("I'm Lord in.".into(), ask.into())],
            },
            // Asked an exchange earlier, then said back and corrected.
            Said {
                this: "No.".into(),
                before: vec![
                    ("I'm Lord in.".into(), ask.into()),
                    ("L O R E N.".into(), "Lauren, did I get that right?".into()),
                ],
            },
        ] {
            let (words, _) = run_at_setup(&socket, &call("Lauren"), &said);
            assert_eq!(words, json!({"error": NOT_HEARD_SPELLED}), "{said:?}");
            let (words, _) = run_with(&socket, &call("Lauren"), true, &said);
            assert_eq!(words, json!({"error": NOT_HEARD_SPELLED}), "{said:?}");
        }
        assert!(refused_here(&json!({"error": NOT_HEARD_SPELLED})));
        assert_eq!(refusal_reason(NOT_HEARD_SPELLED), "not_heard_spelled");
        // Jarvis's P2 on #5580: asked to spell, they gave no letters. No
        // name of theirs can pass a read-back's yes then, so the lamp asks
        // for the name once more: never a second spelling, never a name
        // read back, so no "Lauren?", "Yes." loop.
        for said in [
            // Their name again, no letters.
            Said {
                this: "Lord in.".into(),
                before: vec![("I'm Lord in.".into(), ask.into())],
            },
            // The loop itself: said back without letters, and a yes.
            Said {
                this: "Yes.".into(),
                before: vec![
                    ("I'm Lord in.".into(), ask.into()),
                    ("Lord in.".into(), "Lauren, did I get that right?".into()),
                ],
            },
        ] {
            let (words, _) = run_at_setup(&socket, &call("Lauren"), &said);
            assert_eq!(words, json!({"error": NOT_HEARD_SAY_AGAIN}), "{said:?}");
            let (words, _) = run_with(&socket, &call("Lauren"), true, &said);
            assert_eq!(words, json!({"error": NOT_HEARD_SAY_AGAIN}), "{said:?}");
        }
        assert!(refused_here(&json!({"error": NOT_HEARD_SAY_AGAIN})));
        assert_eq!(refusal_reason(NOT_HEARD_SAY_AGAIN), "not_heard_say_again");
        // Said once more, and heard: the name is theirs, so at setup it
        // goes on to the read-back and its yes, as any heard name does.
        let again = Said {
            this: "Lauren.".into(),
            before: vec![
                ("I'm Lord in.".into(), ask.into()),
                (
                    "Lord in.".into(),
                    "Could you say your name once more?".into(),
                ),
            ],
        };
        let (words, _) = run_at_setup(&socket, &call("Lauren"), &again);
        assert_eq!(words, json!({"error": SAY_IT_BACK}));
        // Their letters after the ask are still their name (#5140 row 6).
        let spelled = Said {
            this: "L, O, R, E, N.".into(),
            before: vec![("I'm Lord in.".into(), ask.into())],
        };
        let (words, _) = run_at_setup(&socket, &call("Lauren"), &spelled);
        assert_eq!(words, json!({"ok": true, "remembered": "Loren"}));
        assert_eq!(
            asked.join().unwrap(),
            [json!({"remember": "Loren", "one_face": true, "replace": false})]
        );
        let _ = std::fs::remove_dir_all(socket.parent().unwrap());
    }

    /// #5564: where the model decides, its words ask for a spelling only
    /// when the name was not understood or was said to be wrong, at most
    /// once, and say a spelled name back as the word it makes, never its
    /// letters nor a choice between the two. Pinned on the exact text.
    #[test]
    fn the_names_words_ask_for_a_spelling_once_and_say_a_name_back_as_a_word() {
        assert_eq!(
            NAME_TOLD_NOTE,
            "If they have just told you their own name, call remember_my_face with it now, as they said it, or if they spelled it, as the name the letters make, written as a word. If you are not sure of it, say it back as a name and ask if you have it right; ask them to spell it only if they say it is wrong, and only once. Never only say that you will remember it. If what they said was not their name, do nothing about it."
        );
        assert_eq!(
            NOT_HEARD,
            "You did not hear that name in what they said. Say the name back to them and ask them to spell it, then call this again with the name as they said it, or as the name their letters make, written as a word, never as letters."
        );
        assert_eq!(
            NOT_HEARD_SPELLED,
            "You did not hear that name in what they said, and they were already asked to spell it, so do not ask them to spell it again. Say back the name their letters make, as a name only, never its letters and never as a choice between the name and its spelling, and ask if you have it right. Only if they say yes, call this again with that name."
        );
        assert_eq!(
            NOT_HEARD_SAY_AGAIN,
            "You did not hear that name in what they said, and they were already asked to spell it, so do not ask them to spell it again, and do not say a name back to them. Ask them to say their name once more, then call this again with the name as they say it."
        );
        assert_eq!(
            SAY_IT_BACK,
            "I have not kept their face yet. First say the name back to them and ask if you have it right, the name as a word only, never its letters and never as a choice between the name and its spelling. Only if they say yes, call this again with that name; if they correct it, say the new one back the same way."
        );
        assert_eq!(
            NEAR_TO,
            "You may have misheard their name. Do not say any name you know. Ask them to spell their name, letter by letter, then call this again with the name their letters make, written as a word, never as letters, and apart true."
        );
        // The walk's own script (`skills/onboarding/skill.toml`): after its
        // read-back (#5532), a spelling only when the name was missed or
        // said to be wrong, once, and the name it spells said back.
        const SCRIPT: &str = include_str!("../../../skills/onboarding/skill.toml");
        let who = SCRIPT
            .split("[onboarding.steps.who_you_are]")
            .nth(1)
            .and_then(|step| step.split("\n[").next())
            .expect("who_you_are");
        const ONCE: &str = " and wait for their answer. Ask to spell it only if it's wrong or missed, once; then say the name it spells, as a word. Only after a clear yes to their name";
        for mode in ["first = ", "reoffer = "] {
            let note = who
                .lines()
                .find(|line| line.starts_with(mode))
                .expect("who_you_are's note");
            assert!(note.contains(ONCE), "{note}");
        }
        assert!(crate::onboarding::WHO_NOTE.contains(ONCE));
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
            .find("say their name back as one clear yes-or-no question on its own")
            .expect("the script says the name back");
        // A clear yes-or-no question, never the bare name: on lamp-52e6
        // (build 10507) the read-back was "Leo? Leo.", no question to answer.
        assert!(
            who.contains("their name and then did I get that right, never their name alone"),
            "{who}"
        );
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
        // Refused here, so never logged as the eyes' answer (the
        // Reviewer-lead's P3); what the eyes answer is theirs.
        assert!(refused_here(&words), "{words}");
        for theirs in [
            json!({"ok": true, "remembered": "Walter"}),
            json!({"error": "no one face in view"}),
        ] {
            assert!(!refused_here(&theirs), "{theirs}");
        }
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
        // A name the model gave as letters is held raw and judged as the
        // word: "Jane, did I get that right?" and a yes runs it (Fifth's
        // P2 on #5580), and a no still drops it.
        for heard in ["My name is Jane.", "J A N E."] {
            for (name, this, verdict) in [
                ("J-A-N-E", "Yes.", Runs),
                ("J A N E", "Yes, that's right.", Runs),
                ("j.a.n.e.", "Yes.", Runs),
                ("J-A-N-E", "No.", Drops),
            ] {
                let said = Said {
                    this: this.into(),
                    before: vec![(heard.into(), "Jane, did I get that right?".into())],
                };
                assert_eq!(
                    held_save_verdict(name, &said),
                    verdict,
                    "{heard} / {name} / {this}"
                );
            }
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
            by_voice: false,
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
            by_voice: false,
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
            by_voice: false,
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
            (NOT_HEARD_SPELLED, "not_heard_spelled"),
            (NOT_HEARD_SAY_AGAIN, "not_heard_say_again"),
            (ONLY_THE_CALL, "only_the_call"),
            (NOT_OWN_NAME, "not_own_name"),
            (FACE_LEAVE, "face_leave"),
            (ASK_FACE_FIRST, "ask_face_first"),
            (FACE_KEPT_OUT, "face_kept_out"),
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

    /// The first builder's HOLD on #5700: a setup face save run with no
    /// model call needs a yes to the face, never the name's yes alone.
    #[test]
    fn a_face_is_agreed_only_by_a_yes_to_the_face() {
        const READ_BACK: &str = "Walter, did I get that right?";
        const FACE: &str = "May I remember your face, so I know you next time?";
        let window = |exchanges: &[(&str, &str)], this: &str| Said {
            this: this.into(),
            before: exchanges
                .iter()
                .map(|(person, lamp)| ((*person).into(), (*lamp).into()))
                .collect(),
        };
        // The name's yes alone is no yes to the face.
        let said = window(&[("Name is Walter.", READ_BACK)], "Yes. That's my name.");
        assert_eq!(read_back_name(&said).as_deref(), Some("Walter"));
        assert!(!face_agreed("Walter", &said));
        assert_eq!(face_agreed_name(&said), None);
        // A yes to the name that asks for the face itself is.
        let said = window(
            &[("Name is Walter.", READ_BACK)],
            "Yes. Please remember my face.",
        );
        assert_eq!(face_agreed_name(&said).as_deref(), Some("Walter"));
        // So is a plain yes to the face question after the name's yes, with
        // the name in it or not, in English or Vietnamese.
        for face in [
            FACE,
            "Walter, may I remember your face?",
            "M\u{ec}nh nh\u{1edb} khu\u{f4}n m\u{1eb7}t b\u{1ea1}n \u{111}\u{1b0}\u{1ee3}c kh\u{f4}ng?",
        ] {
            let said = window(
                &[
                    ("Name is Walter.", READ_BACK),
                    ("Yes. That's my name.", face),
                ],
                "Yes.",
            );
            assert_eq!(face_agreed_name(&said).as_deref(), Some("Walter"), "{face}");
        }
        // A no, a hedge or a question to it; the face question with no yes
        // to the name before it, or after a correction; a line that only
        // says it will look.
        let asked = [
            ("Name is Walter.", READ_BACK),
            ("Yes. That's my name.", FACE),
        ];
        for (exchanges, this) in [
            (&asked[..], "No."),
            (&asked[..], "Not now."),
            (&asked[..], "What for?"),
            (
                &[(
                    "Name is Walter.",
                    "Nice to meet you, Walter! May I remember your face?",
                )][..],
                "Yes.",
            ),
            (
                &[("Name is Walter.", READ_BACK), ("No, it's Walt.", FACE)][..],
                "Yes.",
            ),
            (
                &[
                    ("Name is Walter.", READ_BACK),
                    (
                        "Yes. That's my name.",
                        "Now I'll take a quick look so I know your face.",
                    ),
                ][..],
                "Okay.",
            ),
        ] {
            assert_eq!(face_agreed_name(&window(exchanges, this)), None, "{this}");
        }
    }

    /// Fourteen's HOLD on #5720: a yes to a line that asks NOT to keep the
    /// face is no yes to the face, nor is a yes to a line that asks no leave.
    #[test]
    fn a_yes_to_a_face_question_turned_round_keeps_nothing() {
        for face in [
            "Would you rather I didn't remember your face?",
            "I won't remember your face unless you want me to. Is that okay?",
            "Shall I not remember your face?",
            "Do you want me to forget your face, rather than remember it?",
            "I can't remember your face yet. Should I just call you Walter?",
            "May I remember your face, or would you rather I didn't?",
            "M\u{ec}nh kh\u{f4}ng nh\u{1edb} khu\u{f4}n m\u{1eb7}t b\u{1ea1}n nh\u{e9}?",
            "\u{110}\u{1eeb}ng nh\u{1edb} khu\u{f4}n m\u{1eb7}t m\u{ec}nh nh\u{e9}?",
            // Fourteen's second HOLD: "remember" with something else
            // than the face, or a choice.
            "May I remember just your name, without your face?",
            "May I remember your name, but skip your face?",
            "Can I remember your voice only, no face?",
            "Should I remember your name or your face?",
            "Should I remember your face or your name?",
            "M\u{ec}nh nh\u{1edb} t\u{ea}n th\u{f4}i, kh\u{f4}ng nh\u{1edb} khu\u{f4}n m\u{1eb7}t nh\u{e9}?",
            "M\u{ec}nh nh\u{1edb} khu\u{f4}n m\u{1eb7}t hay t\u{ea}n b\u{1ea1}n?",
        ] {
            let said = Said {
                this: "Yes.".into(),
                before: vec![
                    (
                        "Name is Walter.".into(),
                        "Walter, did I get that right?".into(),
                    ),
                    ("Yes. That's my name.".into(), face.into()),
                ],
            };
            assert_eq!(face_agreed_name(&said), None, "{face}");
        }
        for face in [
            "One more thing: may I remember your face now, so I can remember things for you?",
            "Do you want me to remember your face?",
            "M\u{ec}nh c\u{f3} th\u{1ec3} nh\u{1edb} khu\u{f4}n m\u{1eb7}t b\u{1ea1}n kh\u{f4}ng?",
            "M\u{ec}nh nh\u{1edb} khu\u{f4}n m\u{1eb7}t b\u{1ea1}n \u{111}\u{1b0}\u{1ee3}c kh\u{f4}ng, \u{111}\u{1ec3} l\u{1ea7}n sau nh\u{1ead}n ra b\u{1ea1}n?",
            "May I remember your face, so I know you next time?",
            "Nice to meet you, Henry! May I remember your face?",
        ] {
            assert!(asks_for_the_face(face), "{face}");
        }
    }
}
