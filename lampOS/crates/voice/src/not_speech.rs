//! Sounds that are not something said (`docs/not-speech.md`).
//!
//! The detector takes any short loud sound for speech, and the model hears
//! words in it. The coordinator asks here, when an utterance ends, whether
//! it was long enough to have been said at all.

/// Fewer blocks of speech than this in a whole utterance and it was a sound,
/// not something said: a cough, a click, a bump. On lamp-4ace on 2026-10-13
/// the fourteen such sounds the lamp answered ("Sorry, I missed that", "I'm
/// Lamp. What's your name?") had 11 to 33 blocks, twelve of them under 25;
/// the four shortest things really said to it that morning had 34 to 41.
/// It was 25 until two sounds of 25 and 26 blocks, one after the other, got
/// "I'm here if you need anything."
pub const SCRAP_BLOCKS: usize = 30;
/// An utterance taken as said with fewer blocks of speech than this has how
/// its blocks were judged written into its record, as one thrown out has:
/// 0.8 s of speech, which is every one-word and two-word thing said.
pub const SHORT_BLOCKS: usize = 80;
/// A short word soon after the lamp's own is an answer to it ("Yes.", "No.")
/// and is never taken for a sound. It was ten seconds at first: on lamp-4ace
/// each sound the lamp answered then excused the next one, eight seconds on.
pub const AFTER_OWN_WORDS_US: u64 = 4_000_000;

/// Where the coordinator reads whether an utterance judged a sound still goes
/// to the model in a room that is not busy (`quiet-room`), for the model to
/// judge by its sound. Off unless set. Measured on lamp-4ace on 2026-10-14:
/// the gate threw out 13 of 36 short words said into quiet ("Hi.", "Yes.",
/// "Hello?") and no count of their blocks tells them from tonal sounds
/// (`docs/not-speech.md`).
pub const SHORT_ENVIRONMENT: &str = "LAMP_SHORT_SOUNDS";
/// The value that lets a short utterance through, as `quiet-room` does, and
/// has the provider worker hold its answer unless something shows it was
/// said to the lamp (#800, [`CHECKED_FLAG`]).
pub const CHECKED: &str = "quiet-room-checked";
/// The provider worker's flag for [`CHECKED`].
pub const CHECKED_FLAG: &str = "--sounds-checked";

/// Whether the environment lets a short utterance through in a quiet room.
pub fn short_sounds_from_environment() -> bool {
    short_sounds(std::env::var(SHORT_ENVIRONMENT).ok().as_deref())
}

fn short_sounds(value: Option<&str>) -> bool {
    matches!(value, Some("quiet-room" | CHECKED))
}

/// The provider worker's argument for [`CHECKED`].
pub fn arguments_from_environment() -> Vec<String> {
    arguments(std::env::var(SHORT_ENVIRONMENT).ok().as_deref())
}

fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some(CHECKED) {
        vec![CHECKED_FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading [`CHECKED_FLAG`] off a worker's flags.
pub fn take_checked(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == CHECKED_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// Short things a person says to the lamp, in English as it is spoken to:
/// a heard text that is one of these, whole, is a word and not a room's
/// sound (#800). The lamp is spoken to in English (`deploy/character`).
pub const SHORT_WORDS: &[&str] = &[
    "yes",
    "yeah",
    "yep",
    "yup",
    "no",
    "nope",
    "stop",
    "what",
    "okay",
    "ok",
    "sure",
    "go ahead",
    "yes go ahead",
    "go on",
    "thanks",
    "thank you",
    "hello",
    "hi",
    "hey",
    "lamp",
    "hey lamp",
    "please",
    "wait",
    "right",
    "cool",
    "great",
    "nice",
    "again",
    "sorry",
    "pardon",
    "huh",
    "really",
    "why",
    "how",
    "when",
    "who",
    "where",
    "done",
    "louder",
    "quieter",
    "good",
    "fine",
    "never mind",
    "nevermind",
    "bye",
    "goodbye",
    "good night",
    "morning",
    "good morning",
    "do it",
    "start",
    "cancel",
    "shush",
    "quiet",
    "be quiet",
    "enough",
    "that's it",
    "that's all",
    "all right",
    "alright",
];

/// The words of a heard text, lower-case, without marks.
pub fn plain_words(text: &str) -> Vec<String> {
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// Whether a heard text is, whole, one of [`SHORT_WORDS`].
pub fn short_word(text: &str) -> bool {
    let words = plain_words(text);
    !words.is_empty() && SHORT_WORDS.contains(&words.join(" ").as_str())
}

/// Whether an utterance judged a sound is still sent on: with the switch,
/// in a room that is not busy. A room where the last three things heard went
/// unanswered is busy, so a run of sounds the model leaves alone closes the
/// door again by itself.
pub fn sent_anyway(switch: bool, busy: bool) -> bool {
    switch && !busy
}

/// Whether speech that begins over a turn in progress is excused from being
/// judged a sound. Over the lamp's own answer it is someone cutting in, and
/// over something that was itself said it is someone going on with it. Over
/// a sound it is neither.
pub fn takes_over(lamp_answering: bool, replaced_was_said: bool) -> bool {
    lamp_answering || replaced_was_said
}

/// After a hand or a head stops the lamp's speech (`hand_stop`,
/// `head_stop`), a short utterance is a sound for this long, not sent as a
/// request, whatever [`AFTER_OWN_WORDS_US`], the close count and
/// [`sent_anyway`] say ([`dropped`]). On lamp-4ace on
/// 16 October from 16:32:16 to 16:32:34 the owner tapped the lamp's head to
/// stop a story six times. Each time, within 1 to 3 s, the tap's own sound
/// was heard ("¿Quién?", "un", "¿Qué?"), excused as a short word after the
/// lamp's own, sent to the model, and the story began again (the first
/// builder, #829, 16:33).
pub const AFTER_STOP_US: u64 = 4_000_000;

/// The first request this soon after a hand or the head stopped the lamp
/// tells the model it was stopped, so that it does not pick up what it was
/// saying again (#829, the first builder at 16:33): two minutes, a request
/// in the same conversation.
pub const TELL_STOP_WITHIN_US: u64 = 120_000_000;

/// Where the coordinator reads whether the model is told of a Stop
/// ([`StopToTell`]). `on` only: off until it is played on lamp-4ace (the
/// first builder on #1148).
pub const STOP_TOLD_ENVIRONMENT: &str = "LAMP_STOP_TOLD";

/// Whether the environment tells the model of a Stop.
pub fn stop_told_from_environment() -> bool {
    stop_told(std::env::var(STOP_TOLD_ENVIRONMENT).ok().as_deref())
}

fn stop_told(value: Option<&str>) -> bool {
    value == Some("on")
}

/// A Stop the model has not been told of yet (`LAMP_STOP_TOLD=on`): when a
/// hand or the head stopped the lamp's speech, and the session of the
/// answer it stopped. The first request after it on that session, within
/// [`TELL_STOP_WITHIN_US`], tells the model. A request that took it and was
/// then thrown out as a sound, before its End went, gives it back: on
/// lamp-4ace at 16:32 a tap's own thump was heard just after the tap, and
/// the person's real question came after it (the first builder on #1148).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StopToTell(Option<(u64, u64)>);

impl StopToTell {
    /// A Stop at `at_us` cut the answer on `session`.
    pub fn stopped(&mut self, at_us: u64, session: u64) {
        self.0 = Some((at_us, session));
    }

    /// For a request beginning on `session` at `now_us`: the Stop, once,
    /// if it cut an answer on that session within [`TELL_STOP_WITHIN_US`].
    /// One on another session, or older, is let go: a new session has not
    /// the answer that was stopped.
    pub fn take_for(&mut self, session: u64, now_us: u64) -> Option<(u64, u64)> {
        self.0
            .take()
            .filter(|(at, of)| *of == session && now_us.saturating_sub(*at) <= TELL_STOP_WITHIN_US)
    }

    /// The request that took `stop` was thrown out before its End went: the
    /// next one tells it, unless a newer Stop came meanwhile.
    pub fn given_back(&mut self, stop: (u64, u64)) {
        if self.0.is_none_or(|(at, _)| at < stop.0) {
            self.0 = Some(stop);
        }
    }
}

/// Whether the lamp was stopped by a hand or its head (`stopped_at_us`)
/// less than [`AFTER_STOP_US`] before `now_us`, and has not spoken since
/// (`last_spoke_us`): once it answers again ("Want another?"), a short
/// "Yes." is an answer to that, not the tap's own sound (the reviewer on
/// #1107).
pub fn just_stopped(stopped_at_us: Option<u64>, last_spoke_us: Option<u64>, now_us: u64) -> bool {
    stopped_at_us.is_some_and(|at| {
        now_us.saturating_sub(at) < AFTER_STOP_US && last_spoke_us.is_none_or(|spoke| spoke <= at)
    })
}

/// How long ago the lamp's own words were (`last_spoke_us`), as an excuse
/// for a short word after them: none just after it was stopped, for its
/// words then were cut, not answered.
pub fn own_words_ago(
    last_spoke_us: Option<u64>,
    stopped_at_us: Option<u64>,
    now_us: u64,
) -> Option<u64> {
    if just_stopped(stopped_at_us, last_spoke_us, now_us) {
        return None;
    }
    last_spoke_us.map(|at| now_us.saturating_sub(at))
}

/// Whether an utterance that has just ended is dropped as a sound and not
/// sent as a request: [`scrap`] by its blocks, unless short sounds go to
/// the model ([`sent_anyway`]). Just after a Stop (`just_stopped`) neither
/// the close count nor short sounds excuse it: a tap on the head is close
/// to the lamp by nature, and its thump is no word (the reviewer on #1107:
/// with #1137's close count, a thump of 12 blocks and 4 loud ones was kept).
#[allow(clippy::too_many_arguments)]
pub fn dropped(
    speech_blocks: usize,
    took_over: bool,
    since_own_words_us: Option<u64>,
    loud_when_close: Option<usize>,
    short_sounds: bool,
    busy: bool,
    just_stopped: bool,
) -> bool {
    scrap(
        speech_blocks,
        took_over,
        since_own_words_us,
        loud_when_close.filter(|_| !just_stopped),
    ) && !sent_anyway(short_sounds && !just_stopped, busy)
}

/// Whether an utterance that has just ended, with this many blocks scored
/// as speech, was a sound. `since_own_words_us` is how long ago the lamp was
/// last heard, if it ever was.
pub fn was_a_sound(speech_blocks: usize, took_over: bool, since_own_words_us: Option<u64>) -> bool {
    speech_blocks < SCRAP_BLOCKS
        && !took_over
        && since_own_words_us.is_none_or(|since| since >= AFTER_OWN_WORDS_US)
}

/// A block this loud or louder (about -31 dBFS, on the face's scale) counts
/// as speech-like in an utterance said close to the lamp, whatever the
/// detector scored it.
pub const LOUD_LEVEL: u8 = 128;
/// An utterance said close to the lamp with fewer speech-like blocks than
/// this is a sound: its speech blocks and its loud ones together.
pub const CLOSE_SCRAP_BLOCKS: usize = 16;

/// [`was_a_sound`], or for an utterance said close to the lamp (the room
/// microphone rose [`crate::attention::NEAR_RISE_DB`] or more over its quiet
/// during it, `busy-near`'s sign; `loud_when_close` is then its loud blocks
/// the detector did not call speech) the same with its loud blocks counted
/// as speech and [`CLOSE_SCRAP_BLOCKS`] needed. On lamp-4ace on 2026-10-16
/// from 17:13 the owner at the desk was dropped as a sound six times: 14 to
/// 26 blocks counted, and in two of them 15 and 20 loud blocks scored under
/// 0.5. A knock or a mug set down by the lamp rises as much, and is a few
/// blocks: it is still a sound (Ten on #1137). Everything after the gate
/// still judges the utterance.
pub fn scrap(
    speech_blocks: usize,
    took_over: bool,
    since_own_words_us: Option<u64>,
    loud_when_close: Option<usize>,
) -> bool {
    let a_sound = was_a_sound(speech_blocks, took_over, since_own_words_us);
    match loud_when_close {
        None => a_sound,
        Some(loud) => a_sound && speech_blocks + loud < CLOSE_SCRAP_BLOCKS,
    }
}

/// Whether an answer opens with the name of a function said aloud, not
/// called: `not_for_me`, or the service's own way of writing a call
/// (`print(default_api.not_for_me())`). On lamp-4ace on 2026-10-13, with
/// seven functions declared, three answers in one test were the words
/// "not_for_me", played through the speaker to two people talking to each
/// other. A word with an underscore in it at the start of an answer is
/// never something a lamp says.
pub fn names_a_function(opening: &str) -> bool {
    let opening = opening.trim_start();
    if opening.starts_with("print(")
        || opening.starts_with("tool_code")
        || opening.starts_with("default_api")
    {
        return true;
    }
    let name: &str = opening
        .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
        .next()
        .unwrap_or_default();
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.contains('_')
        && name.split('_').all(|part| !part.is_empty())
}

/// Whether a piece of an answer is, whole, a function call written as words:
/// a name as [`names_a_function`] reads one, alone or followed by its
/// arguments in `{…}` or `(…)` (`not_for_me{}`, `print(default_api.x())`).
/// On lamp-4ace on 2026-10-16 at 17:12 the lamp's words were
/// `not_for_me{}`. Ordinary words with an underscore inside a sentence are
/// not one.
pub fn bare_call(piece: &str) -> bool {
    call_arguments(piece.trim().trim_end_matches('.')).is_some_and(|rest| {
        rest.is_empty()
            || (rest.starts_with('{') && rest.ends_with('}'))
            || (rest.starts_with('(') && rest.ends_with(')'))
    })
}

/// Whether `text` is a call so far: whole, or a name with its arguments
/// begun and not yet closed (`set_timer{"min`).
fn a_call_so_far(text: &str) -> bool {
    bare_call(text)
        || call_arguments(text.trim()).is_some_and(|rest| {
            let open = |a: char, b: char| {
                rest.starts_with(a) && rest.matches(a).count() > rest.matches(b).count()
            };
            open('{', '}') || open('(', ')')
        })
}

/// Whether a reply so far is the refusal "not for me" written as words
/// (`LAMP_NOT_FOR_ME_WORDS`). On lamp-4ace, r06, request 131, a line let
/// out only as the conversation going on ("That lamp is cute, where did you
/// get it?") got "Not for me — that's about me, not to me.", spoken. It is
/// the refusal when "not for me", case aside, is the whole reply, or is
/// followed straight away by the refusal's own clause (`REFUSAL_CLAUSES`:
/// "that's about me", "not to me"; "to answer" and "to reply" only at a
/// sentence end, '.', '!', '?' or the reply's end); punctuation, quotes,
/// dashes and whitespace between or around the words are skipped, and so
/// are speech tags before them (`[softly]`, `LAMP_SECOND_SPEECH_TAGS`).
/// Anything else after it is an answer: `CONVERSATION_ALONE` tells the
/// model a follow-up to the lamp is for it, so "Not for me, thanks, I'm
/// more of a lo-fi lamp.", "Not for me to say, but…" and "Not for me to
/// answer, but…" are said.
/// `Some(true)`: it is; `Some(false)`: it can't be ("Nothing for me…",
/// "Note for me", "Not for me, thanks"); `None`: not yet known, the text
/// ending inside its words, after "not for me" with nothing but
/// punctuation since (only its end makes that the whole reply), partway
/// through a clause, after "to answer" or "to reply" before what follows
/// shows a sentence end, or inside a speech tag. `ended`: no more text comes,
/// so a last word is whole. An apostrophe inside a word, `'` or `’`, is
/// the word's own, so "me’s" is no "me"; single quotes enclosing the words
/// ('Not for me', ‘Not for me’) are punctuation.
pub fn opens_not_for_me(text: &str, ended: bool) -> Option<bool> {
    const WORDS: [&str; 3] = ["not", "for", "me"];
    fn in_word(c: char) -> bool {
        c.is_alphanumeric() || c == '\'' || c == '\u{2019}'
    }
    // The next word of `rest`, lower-cased with `’` as `'`, what follows
    // it, and whether more of it may still come. A single quote enclosing
    // it ('Not, me’) is no part of it; one inside it ("me’s") is.
    fn next_word(rest: &str, ended: bool) -> (String, &str, bool) {
        let rest = rest.trim_start_matches(|c| !in_word(c) || c == '\'');
        let end = rest.find(|c| !in_word(c)).unwrap_or(rest.len());
        let open = !ended && end == rest.len();
        let word = rest[..end].trim_end_matches(['\'', '\u{2019}']);
        (
            word.to_lowercase().replace('\u{2019}', "'"),
            &rest[end..],
            open,
        )
    }
    let mut rest = text;
    loop {
        rest = rest.trim_start_matches(|c| !in_word(c) && c != '[');
        let Some(tag) = rest.strip_prefix('[') else {
            break;
        };
        rest = match tag.find(']') {
            Some(close) => &tag[close + 1..],
            // A tag still open waits, as the voice's own cut holds it; one
            // too long to be a tag is none, and the reply is said.
            None if !ended && tag.len() < crate::second_provider::speech_tags::HELD_MOST => {
                return None;
            }
            None => return Some(false),
        };
    }
    for want in WORDS {
        let (word, after, open) = next_word(rest, ended);
        if open {
            return (!want.starts_with(word.as_str())).then_some(false);
        }
        if word != want {
            return Some(false);
        }
        rest = after;
    }
    // After "not for me": nothing more, or one of the clauses.
    let mut clauses = REFUSAL_CLAUSES.to_vec();
    for at in 0.. {
        let (word, after, open) = next_word(rest, ended);
        if word.is_empty() {
            // Nothing but punctuation since the last word.
            return match (open, at) {
                (true, _) => None,
                (false, 0) => Some(true),
                (false, _) => Some(false),
            };
        }
        clauses.retain(|clause| {
            clause.get(at).is_some_and(|want| match open {
                true => want.starts_with(word.as_str()),
                false => *want == word,
            })
        });
        if clauses.is_empty() {
            return Some(false);
        }
        if open {
            return None;
        }
        if let Some(clause) = clauses.iter().find(|clause| clause.len() == at + 1) {
            if !SENTENCE_END_CLAUSES.contains(clause) {
                return Some(true);
            }
            // "Not for me to answer, but I think it's lovely." is an answer;
            // only a sentence ending here is the refusal.
            return sentence_ends(after, ended);
        }
        rest = after;
    }
    unreachable!("the loop above returns")
}

/// The refusal's own words straight after "not for me" (r06's "Not for me —
/// that's about me, not to me."), lower-case, `'` for any apostrophe.
const REFUSAL_CLAUSES: [&[&str]; 6] = [
    &["that's", "about", "me"],
    &["that", "is", "about", "me"],
    &["it's", "about", "me"],
    &["not", "to", "me"],
    &["to", "answer"],
    &["to", "reply"],
];

/// The clauses that are the refusal only at a sentence end: "Not for me to
/// answer, but…" is the idiom, as "Not for me to say, but…" is.
const SENTENCE_END_CLAUSES: [&[&str]; 2] = [&["to", "answer"], &["to", "reply"]];

/// Whether `after`, what follows a clause's last word, ends the sentence:
/// '.', '!' or '?', or the reply's end, past closing quotes and whole
/// speech tags; `None` while that is not yet known.
fn sentence_ends(after: &str, ended: bool) -> Option<bool> {
    let mut rest = after;
    loop {
        rest = rest.trim_start_matches(|c: char| {
            c.is_whitespace() || matches!(c, '"' | '\'' | '\u{201d}' | '\u{2019}' | ')')
        });
        let Some(tag) = rest.strip_prefix('[') else {
            break;
        };
        match tag.find(']') {
            Some(close) => rest = &tag[close + 1..],
            None if !ended && tag.len() < crate::second_provider::speech_tags::HELD_MOST => {
                return None;
            }
            None => return Some(false),
        }
    }
    match rest.chars().next() {
        None if ended => Some(true),
        None => None,
        Some(c) => Some(matches!(c, '.' | '!' | '?')),
    }
}

/// What follows a function's name at the start of `text`.
fn call_arguments(text: &str) -> Option<&str> {
    if !names_a_function(text) {
        return None;
    }
    let name_end = text
        .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.'))
        .unwrap_or(text.len());
    Some(text[name_end..].trim())
}

/// The pieces of one answer, with a function call written as words left
/// out whatever pieces it came in (" not_for" then "_me{}"): a piece that
/// is, or with the ones held before it makes, a call so far is held back,
/// and what is held is never said, as it is code.
// ponytail: a call split inside its first word (" not" then "_for_me{}")
// still leaves its tail in; holding every lower-case word would need an
// end-of-answer flush in each user.
#[derive(Debug, Default)]
pub struct CallsLeftOut {
    request: Option<u64>,
    held: String,
}

impl CallsLeftOut {
    pub const fn new() -> Self {
        Self {
            request: None,
            held: String::new(),
        }
    }

    /// Whether `piece` of answer `request` is words to keep.
    pub fn keeps(&mut self, request: u64, piece: &str) -> bool {
        if self.request != Some(request) {
            self.request = Some(request);
            self.held.clear();
        }
        let joined = format!("{}{piece}", self.held);
        if a_call_so_far(&joined) {
            self.held = joined;
            return false;
        }
        self.held.clear();
        if a_call_so_far(piece) {
            self.held = piece.to_owned();
            return false;
        }
        true
    }
}

/// Where the coordinator reads whether, in a conversation, a turn heard in
/// another script or as a number alone is asked again rather than answered
/// (#926). Off unless set to [`UNCLEAR`].
pub const UNCLEAR_ENVIRONMENT: &str = "LAMP_OTHER_LANGUAGE";
pub const UNCLEAR: &str = "sorry";
/// The provider worker's flag for it.
pub const UNCLEAR_FLAG: &str = "--ask-unclear";

/// The provider worker's argument for asking again, from the environment.
/// Never with a language other than English (#4473): another script is
/// then the conversation's own.
pub fn unclear_arguments_from_environment() -> Vec<String> {
    if crate::language::from_environment().is_some() {
        return Vec::new();
    }
    unclear_arguments(std::env::var(UNCLEAR_ENVIRONMENT).ok().as_deref())
}

fn unclear_arguments(value: Option<&str>) -> Vec<String> {
    if value == Some(UNCLEAR) {
        vec![UNCLEAR_FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// Take a leading [`UNCLEAR_FLAG`] off a worker's flags.
pub fn take_unclear(flags: &[String]) -> (bool, &[String]) {
    match flags {
        [flag, rest @ ..] if flag == UNCLEAR_FLAG => (true, rest),
        _ => (false, flags),
    }
}

/// Where the coordinator reads a language hint for the service's transcript
/// of what it hears (#926): on lamp-4ace on 2026-10-17 English said with a
/// Vietnamese accent came back in Devanagari and was held as another
/// script. Unset, the transcript has no hint, as before.
pub const TRANSCRIPTION_ENVIRONMENT: &str = "LAMP_TRANSCRIPTION_LANGUAGE";
/// The provider worker's flag for it, followed by the language tag.
pub const TRANSCRIPTION_FLAG: &str = "--transcription-language";

/// The provider worker's arguments for the transcript's language hint, from
/// the environment.
pub fn transcription_arguments_from_environment() -> Vec<String> {
    transcription_arguments(std::env::var(TRANSCRIPTION_ENVIRONMENT).ok().as_deref())
}

fn transcription_arguments(value: Option<&str>) -> Vec<String> {
    match value {
        Some(tag) if language_tag(tag) => vec![TRANSCRIPTION_FLAG.to_owned(), tag.to_owned()],
        Some(other) if !other.is_empty() => {
            // A wrong value must not stop the voice: no hint, said once a
            // worker.
            eprintln!(
                "lamp-voice: {TRANSCRIPTION_ENVIRONMENT}={other:?} is not a language tag such as en-US; the transcript has no hint"
            );
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// A language tag exactly as `lamp_gemini` takes one, by its own rule, so
/// a value passed on is never one the worker's session refuses (the
/// reviewer on #1367: `en-U` passed here and stopped the worker).
fn language_tag(value: &str) -> bool {
    lamp_gemini::valid_language_code(value)
}

/// Take a leading [`TRANSCRIPTION_FLAG`] and its tag off a worker's flags.
pub fn take_transcription(flags: &[String]) -> (Option<String>, &[String]) {
    match flags {
        [flag, tag, rest @ ..] if flag == TRANSCRIPTION_FLAG => (Some(tag.clone()), rest),
        _ => (None, flags),
    }
}

/// The session with the transcript's language hint, when there is one.
pub fn with_transcription(
    session: lamp_gemini::SessionConfig,
    language: Option<&str>,
) -> lamp_gemini::SessionConfig {
    let Some(language) = language else {
        return session;
    };
    // A hint the session refuses is said and dropped: the voice starts
    // without it rather than not at all.
    match session.clone().transcription_language(language) {
        Ok(hinted) => hinted,
        Err(error) => {
            eprintln!(
                "lamp-voice provider: transcript language {language:?} refused ({error:?}); no hint"
            );
            session
        }
    }
}

/// Why something heard in a conversation is not answered but asked again
/// (#926): the conversation is in English, so heard mostly in another
/// script it was misheard, not said in that language.
pub const OTHER_SCRIPT: &str = "heard mostly in another script than the conversation's";
/// Heard as a number alone when the lamp had asked nothing: a fragment.
pub const ONLY_DIGITS: &str = "heard as a number alone, and the lamp had asked nothing";
/// A cut-in over the lamp's answer heard with no English word in it: the
/// rest of what was said was lost or misheard (#926, #828).
pub const CUT_IN_FRAGMENT: &str = "a cut-in heard with no English word in it";

/// Common English words: a cut-in of one or two words with none of these
/// in it is a fragment, not something said (#926). Not a language detector:
/// it only has to know the words people cut the lamp off with, and the
/// small words any English phrase has. On lamp-4ace on 2026-10-16 English
/// cut-ins over a story came back as "vestido", "analizar", "miel miel",
/// "Tá.", "Los Ángeles".
pub const ENGLISH_WORDS: &[&str] = &[
    // What people cut in with.
    "stop",
    "wait",
    "okay",
    "ok",
    "no",
    "yes",
    "yeah",
    "yep",
    "nope",
    "hey",
    "hi",
    "hello",
    "sorry",
    "what",
    "why",
    "how",
    "who",
    "when",
    "where",
    "please",
    "thanks",
    "thank",
    "enough",
    "quiet",
    "shush",
    "pause",
    "go",
    "continue",
    "again",
    "more",
    "less",
    "louder",
    "softer",
    "slower",
    "faster",
    "never",
    "mind",
    "fine",
    "sure",
    "right",
    "good",
    "great",
    "nice",
    "cool",
    "wow",
    "oh",
    "ah",
    "um",
    "uh",
    "hmm",
    "huh",
    "really",
    "actually",
    "joke",
    "story",
    "song",
    "music",
    "lamp",
    "light",
    "help",
    "listen",
    "look",
    "tell",
    "say",
    "said",
    "talk",
    "speak",
    "repeat",
    "change",
    "switch",
    "different",
    "something",
    "else",
    "instead",
    "other",
    "next",
    "back",
    "done",
    "finish",
    "finished",
    "start",
    "begin",
    "end",
    "one",
    "two",
    "three",
    "time",
    "timer",
    "alarm",
    "reminder",
    "remind",
    "weather",
    "now",
    "later",
    "today",
    "tomorrow",
    "tonight",
    // The small words of any English phrase.
    "a",
    "an",
    "the",
    "i",
    "me",
    "my",
    "you",
    "your",
    "we",
    "us",
    "our",
    "he",
    "she",
    "it",
    "its",
    "they",
    "them",
    "this",
    "that",
    "these",
    "those",
    "is",
    "are",
    "was",
    "were",
    "be",
    "been",
    "am",
    "do",
    "does",
    "did",
    "don't",
    "doesn't",
    "didn't",
    "not",
    "can",
    "can't",
    "could",
    "will",
    "won't",
    "would",
    "should",
    "have",
    "has",
    "had",
    "and",
    "or",
    "but",
    "so",
    "if",
    "then",
    "than",
    "to",
    "of",
    "in",
    "on",
    "at",
    "for",
    "with",
    "about",
    "from",
    "up",
    "down",
    "out",
    "off",
    "over",
    "just",
    "all",
    "some",
    "any",
    "too",
    "very",
    "here",
    "there",
    "let",
    "let's",
    "it's",
    "i'm",
    "you're",
    "that's",
    "what's",
    "how's",
    "there's",
];

fn english(word: &str) -> bool {
    ENGLISH_WORDS.contains(&word)
}

/// A letter of the Latin script, as English and the languages written like
/// it use: plain ASCII and the Latin supplements and extensions.
fn latin(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c as u32, 0x00C0..=0x024F | 0x1E00..=0x1EFF)
}

/// Whether what was heard in a conversation held in English makes no sense
/// as a turn of it, and why: mostly another script's letters, or a number
/// alone when the lamp's last line was not a question (`lamp_asked`), or, for
/// a cut-in over the lamp's answer (`cut_in`), any words none of
/// which is English ([`ENGLISH_WORDS`]). Otherwise languages written in the
/// Latin script are not told apart here.
pub fn unclear_in_conversation(
    heard: &str,
    lamp_asked: bool,
    cut_in: bool,
) -> Option<&'static str> {
    let letters = heard.chars().filter(|c| c.is_alphabetic()).count();
    let other = heard
        .chars()
        .filter(|c| c.is_alphabetic() && !latin(*c))
        .count();
    if letters > 0 && other * 2 > letters {
        return Some(OTHER_SCRIPT);
    }
    // One number alone; several ("1 2 3") are someone counting, and only a
    // cut-in's fragment below.
    let numbers = heard
        .split(|c: char| !c.is_numeric())
        .filter(|part| !part.is_empty())
        .count();
    if letters == 0 && numbers == 1 && !lamp_asked {
        return Some(ONLY_DIGITS);
    }
    if cut_in && (letters > 0 || numbers > 1) {
        let words = plain_words(heard);
        // No English word at all, however many: over the lamp's English
        // answer, a cut-in heard all in another language was misheard
        // (lamp-4ace 08:00: "Okay, okay, that is enough." heard as "Venga,
        // nos vemos luego.", four words, answered "See you later!").
        if !words.is_empty() && !words.iter().any(|word| english(word)) {
            return Some(CUT_IN_FRAGMENT);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `LAMP_NOT_FOR_ME_WORDS`: "not for me", however written, as the whole
    /// reply or followed by the refusal's own clause, is a refusal; followed
    /// by anything else it is an answer, and one cut short is not known
    /// until more comes or it ends.
    #[test]
    fn a_reply_opening_with_not_for_me_is_told_from_one_that_does_not() {
        for refusal in [
            // r06's own words.
            "Not for me \u{2014} that's about me, not to me.",
            "Not for me \u{2014} that\u{2019}s about me,",
            "not for me, that is about me.",
            "Not for me: it's about me!",
            "Not for me, not to me.",
            "Not for me to answer.",
            "\u{201c}Not for me to reply\u{201d}.",
            "Not for me to answer!",
            "Not for me to reply? ",
            "[softly] Not for me \u{2014} that's about me.",
        ] {
            assert_eq!(opens_not_for_me(refusal, false), Some(true), "{refusal}");
        }
        // The whole reply: known only at its end.
        for whole in [
            "Not for me",
            "not for me.",
            "Not for me ",
            "Not for me!",
            "  \"NOT FOR ME\"",
            "\u{2014} Not, for me.",
            "\u{201c}Not for me\u{201d}",
            "not_for_me{}",
            // A speech tag first (`LAMP_SECOND_SPEECH_TAGS`) is not a word.
            "[softly] Not for me.",
            " [sighs][whispers] \"Not for me\"",
            // Single quotes enclosing the words.
            "'Not for me'",
            "'Not for me.'",
            "\u{2018}Not for me\u{2019}",
            "\u{2018}Not for me\u{2019}.",
        ] {
            assert_eq!(opens_not_for_me(whole, false), None, "{whole:?}");
            assert_eq!(opens_not_for_me(whole, true), Some(true), "{whole:?}");
        }
        for reply in [
            "Nothing for me to add, but",
            "Note for me",
            "Not formal",
            "Not sure",
            "No, ",
            "Sure",
            "It's not for me",
            "Not for me\u{2019}s",
            "Not for me's",
            // "Not." is decided by the next word that is not the refusal's.
            "Not. For example, ",
            "Not for meat ",
            // A follow-up to the lamp answered: an answer.
            "Not for me, thanks, I'm more of a lo-fi lamp.",
            "Not for me today",
            "Not for me either.",
            "Not for me to say, but it's lovely.",
            "Not for me to answer, but I think it's lovely.",
            "Not for me to reply \u{2014} it's lovely.",
            "not for me to judge",
            "Not for me, that's about you",
            "[softly] Sure.",
        ] {
            assert_eq!(opens_not_for_me(reply, false), Some(false), "{reply}");
        }
        for so_far in [
            "",
            " ",
            "\u{2014} ",
            "N",
            "No",
            "Not",
            "Not f",
            "Not for",
            "Not for me",
            "Not for me, ",
            "Not for me t",
            "Not for me th",
            "Not for me to",
            "Not for me to ",
            "Not for me to answer",
            "Not for me to answer ",
            "\u{201c}Not for me to reply\u{201d}",
            "Not for me to answer [sof",
            "Not for me \u{2014} that's about ",
            "Not for me\u{2019}",
            "Not. ",
            "Not. For ",
            "[soft",
            "[softly] ",
            "[softly] Not for me ",
        ] {
            assert_eq!(opens_not_for_me(so_far, false), None, "{so_far:?}");
        }
        for ended in [
            "",
            "No",
            "Not for",
            "Not for me to",
            "Not for me, that's about",
            "[soft",
        ] {
            assert_eq!(opens_not_for_me(ended, true), Some(false), "{ended:?}");
        }
        // "To answer" and "to reply" ending the reply: the refusal.
        for ended in [
            "Not for me to answer",
            "Not for me to answer ",
            "\u{201c}Not for me to reply\u{201d}",
            "Not for me to answer [softly]",
        ] {
            assert_eq!(opens_not_for_me(ended, true), Some(true), "{ended:?}");
        }
    }

    /// #926: `LAMP_TRANSCRIPTION_LANGUAGE` reaches the provider worker as a
    /// flag and its tag, and from there the session's transcript, and only
    /// there; unset or not a tag, nothing changes.
    #[test]
    fn a_transcription_language_reaches_only_the_sessions_transcript() {
        assert_eq!(
            transcription_arguments(Some("en-US")),
            [TRANSCRIPTION_FLAG, "en-US"]
        );
        for value in [
            None,
            Some(""),
            Some("english please"),
            Some("e"),
            Some("en_US"),
            Some("en-U"),
            Some("en-u-ca-gregory"),
        ] {
            assert!(transcription_arguments(value).is_empty(), "{value:?}");
        }
        let flags: Vec<String> = [TRANSCRIPTION_FLAG, "vi-VN", "--next"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(
            take_transcription(&flags),
            (Some("vi-VN".to_owned()), &flags[2..])
        );
        assert_eq!(take_transcription(&flags[2..]), (None, &flags[2..]));
        let session = || {
            lamp_gemini::SessionConfig::new(
                "wss://proxy.example/live",
                lamp_gemini::Credential::api_key("TEST-ONLY-NOT-A-SECRET").unwrap(),
            )
            .unwrap()
        };
        let setup = |config: &lamp_gemini::SessionConfig| -> serde_json::Value {
            serde_json::from_str(&lamp_gemini::wire::encode_setup(config).unwrap()).unwrap()
        };
        let hinted = setup(&with_transcription(session(), Some("en-US")));
        assert_eq!(
            hinted["setup"]["inputAudioTranscription"],
            serde_json::json!({"languageCodes": ["en-US"]})
        );
        assert!(
            hinted["setup"]["generationConfig"]["speechConfig"]
                .get("languageCode")
                .is_none()
        );
        let plain = setup(&with_transcription(session(), None));
        assert_eq!(plain, setup(&session()));
        // A tag the session refuses is dropped, never the worker.
        assert_eq!(setup(&with_transcription(session(), Some("en-U"))), plain);
    }

    /// The first builder on #1148: a Stop, then the tap's thump taken as a
    /// request and thrown out as a sound, then the person's real question,
    /// whose Start must say the lamp was stopped. Once only, on the
    /// session whose answer was cut, within two minutes. And the switch is
    /// `on` only.
    #[test]
    fn a_stop_is_told_to_the_first_real_request_on_its_session() {
        const S: u64 = 1_000_000;
        let mut stop = StopToTell::default();
        stop.stopped(10 * S, 3);
        // The thump takes it, and is thrown out before its End went.
        let thump = stop.take_for(3, 11 * S);
        assert_eq!(thump, Some((10 * S, 3)));
        assert_eq!(stop.take_for(3, 11 * S), None);
        stop.given_back(thump.unwrap());
        // The real question has it, once.
        assert_eq!(stop.take_for(3, 13 * S), Some((10 * S, 3)));
        assert_eq!(stop.take_for(3, 14 * S), None);
        // A newer Stop meanwhile is not replaced by the older one given back.
        stop.stopped(20 * S, 3);
        stop.given_back((10 * S, 3));
        assert_eq!(stop.take_for(3, 21 * S), Some((20 * S, 3)));
        // Another session, or past two minutes: let go.
        stop.stopped(30 * S, 3);
        assert_eq!(stop.take_for(4, 31 * S), None);
        assert_eq!(stop.take_for(3, 31 * S), None);
        stop.stopped(40 * S, 3);
        assert_eq!(stop.take_for(3, 40 * S + TELL_STOP_WITHIN_US + 1), None);
        assert!(stop_told(Some("on")));
        for value in [None, Some(""), Some("off"), Some("ON")] {
            assert!(!stop_told(value), "{value:?}");
        }
    }

    /// #829, the first builder at 16:33: a tap's own sound just after it
    /// stopped the lamp is a sound, not a short word answering the lamp, and
    /// is not sent as a request even where short sounds go to the model, or
    /// it was said close to the lamp. Real words are taken, and so is a
    /// short word once the moment has passed or the lamp has spoken again.
    #[test]
    fn a_short_sound_just_after_the_lamp_is_stopped_is_a_sound() {
        const S: u64 = 1_000_000;
        let (spoke, stopped) = (Some(10 * S), Some(10 * S + 100_000));
        // 2 s after the stop: no excuse, a sound.
        let ago = own_words_ago(spoke, stopped, 12 * S);
        assert_eq!(ago, None);
        assert!(was_a_sound(12, false, ago));
        assert!(just_stopped(stopped, spoke, 12 * S));
        // Said close (12 blocks and 4 loud, the reviewer's probe on #1107):
        // kept by the close count, but not just after a stop.
        assert!(!dropped(12, false, None, Some(4), false, false, false));
        assert!(dropped(12, false, None, Some(4), false, false, true));
        // With short sounds sent to the model in a quiet room: sent on, but
        // not just after a stop.
        assert!(!dropped(12, false, None, None, true, false, false));
        assert!(dropped(12, false, None, None, true, false, true));
        // The lamp spoke again after the stop: the 4 s are over.
        assert!(!just_stopped(stopped, Some(12 * S), 13 * S));
        // Real words are not a sound.
        assert!(!was_a_sound(SCRAP_BLOCKS, false, ago));
        // Not stopped: a short word 2 s after the lamp's own is its answer.
        let ago = own_words_ago(spoke, None, 12 * S);
        assert_eq!(ago, Some(2 * S));
        assert!(!was_a_sound(12, false, ago));
        // Once the moment has passed, as before.
        assert!(!just_stopped(
            stopped,
            spoke,
            10 * S + 100_000 + AFTER_STOP_US
        ));
        assert_eq!(
            own_words_ago(spoke, stopped, 10 * S + 100_000 + AFTER_STOP_US),
            Some(AFTER_STOP_US + 100_000)
        );
    }

    /// #800: `quiet-room-checked` lets short sounds through as `quiet-room`
    /// does and tells the provider worker to check them; nothing else does.
    #[test]
    fn the_checked_value_lets_sounds_through_and_has_the_worker_check_them() {
        assert!(short_sounds(Some("quiet-room")) && short_sounds(Some(CHECKED)));
        for other in [None, Some("on"), Some("checked"), Some("QUIET-ROOM")] {
            assert!(!short_sounds(other));
            assert!(arguments(other).is_empty());
        }
        assert!(arguments(Some("quiet-room")).is_empty());
        assert_eq!(arguments(Some(CHECKED)), [CHECKED_FLAG]);
        let flags: Vec<String> = [CHECKED_FLAG, "--next"].map(String::from).into();
        assert_eq!(take_checked(&flags), (true, &flags[1..]));
        assert_eq!(take_checked(&flags[1..]), (false, &flags[1..]));
    }

    /// What was heard is a short word only whole: the words people say to
    /// the lamp, not a sound written as a word in another language.
    #[test]
    fn a_short_word_is_one_said_to_the_lamp_whole() {
        for word in [
            "Yes.",
            "yeah",
            "Stop!",
            "What?",
            "Yes, go ahead.",
            "Okay.",
            "Thank you.",
        ] {
            assert!(short_word(word), "{word}");
        }
        for not in [
            "¿Qué?",
            "Claro.",
            "Hola.",
            "albero",
            "",
            "...",
            "Stop the music",
            "Yes I think so",
        ] {
            assert!(!short_word(not), "{not}");
        }
        assert_eq!(plain_words(" ¿Qué, PASA? "), ["qué", "pasa"]);
    }

    #[test]
    fn a_sound_goes_on_only_with_the_switch_and_only_in_a_room_that_is_not_busy() {
        assert!(sent_anyway(true, false));
        assert!(!sent_anyway(true, true));
        assert!(!sent_anyway(false, false));
        assert!(!sent_anyway(false, true));
    }

    #[test]
    fn a_function_named_aloud_is_told_from_something_said() {
        for named in [
            "not_for_me",
            " not_for_me.",
            "not_for_me()",
            "volume_down",
            "heard_my_name, yes",
            "print(default_api.not_for_me())",
            "default_api.recall(about='coffee')",
            "tool_code",
        ] {
            assert!(names_a_function(named), "{named}");
        }
        for said in [
            "Not for me.",
            "not for me",
            "Remember, I told you.",
            "recall",
            "Four. Easy one.",
            "...",
            "",
            "_",
            "a_",
            "_a",
            "Good_morning",
            "I can't do that_one",
        ] {
            assert!(!names_a_function(said), "{said}");
        }
    }

    const LONG_AGO: Option<u64> = Some(60_000_000);

    #[test]
    fn the_sounds_the_lamp_answered_are_sounds() {
        // 16:30 on 2026-10-13: two a second apart. The first is dropped, so
        // the second has no turn to take over.
        assert!(was_a_sound(25, takes_over(false, false), LONG_AGO));
        assert!(was_a_sound(26, takes_over(false, false), LONG_AGO));
        // Before the lamp has said anything at all.
        assert!(was_a_sound(11, false, None));
    }

    #[test]
    fn the_shortest_things_really_said_are_not() {
        for blocks in [34, 37, 41] {
            assert!(!was_a_sound(blocks, false, LONG_AGO));
        }
    }

    #[test]
    fn a_quick_word_over_the_lamp_or_after_it_is_not_a_sound() {
        // "Stop." over the lamp's answer.
        assert!(!was_a_sound(12, takes_over(true, false), Some(0)));
        // "Yes." two seconds after the lamp's question; the same word after
        // five seconds of nothing is on its own.
        assert!(!was_a_sound(15, false, Some(2_000_000)));
        assert!(was_a_sound(15, false, Some(5_000_000)));
    }

    #[test]
    fn the_end_of_a_sentence_is_excused_and_a_sound_after_a_sound_is_not() {
        // "What is the capital" ... "of France?": the second half is short
        // and replaces a turn whose speech was long enough to be said.
        assert!(!was_a_sound(18, takes_over(false, true), LONG_AGO));
        // A sound let through for being close to the lamp's words, then
        // another over it once that excuse has run out.
        assert!(was_a_sound(18, takes_over(false, false), Some(4_500_000)));
    }

    /// #926: on lamp-4ace on 2026-10-16 at 05:39 a cut-in was heard as
    /// Hindi and answered in Hindi; an earlier one heard as "23" got "Four."
    #[test]
    fn heard_in_another_script_or_as_a_bare_number_in_a_conversation_is_unclear() {
        assert_eq!(
            unclear_in_conversation("हिंदू के स्थान", false, false),
            Some(OTHER_SCRIPT)
        );
        assert_eq!(
            unclear_in_conversation("हिंदू के स्थान", true, false),
            Some(OTHER_SCRIPT)
        );
        assert_eq!(
            unclear_in_conversation("23", false, false),
            Some(ONLY_DIGITS)
        );
        assert_eq!(
            unclear_in_conversation(" 23. ", false, false),
            Some(ONLY_DIGITS)
        );
        // A number answering the lamp's question is an answer.
        assert_eq!(unclear_in_conversation("23", true, false), None);
        for said in [
            "No no no, tell me a joke instead.",
            "Yes.",
            "Café crème, s'il vous plaît.",
            "¿Qué?",
            "Set a timer for 23 minutes",
            "",
            "...",
        ] {
            assert_eq!(unclear_in_conversation(said, false, false), None, "{said}");
        }
        // A word of another script inside English is still English.
        assert_eq!(
            unclear_in_conversation("I said namaste नमस्ते to her", false, false),
            None
        );
        assert_eq!(
            unclear_in_conversation("你好", false, false),
            Some(OTHER_SCRIPT)
        );
        assert_eq!(
            unclear_in_conversation("Привет, как дела", false, false),
            Some(OTHER_SCRIPT)
        );
    }

    #[test]
    fn asking_again_is_off_unless_set_to_sorry() {
        assert_eq!(unclear_arguments(Some("sorry")), [UNCLEAR_FLAG]);
        for other in [None, Some("on"), Some("Sorry"), Some("")] {
            assert!(unclear_arguments(other).is_empty());
        }
        let flags: Vec<String> = [UNCLEAR_FLAG, "--next"].map(String::from).into();
        assert_eq!(take_unclear(&flags), (true, &flags[1..]));
        assert_eq!(take_unclear(&flags[1..]), (false, &flags[1..]));
    }

    /// #926, the first builder's addition: tonight's English cut-ins over a
    /// story came back as Latin-script fragments. A cut-in of one or two
    /// words, none of them English, is unclear; what people cut in with
    /// passes; longer or non-cut-in turns are left to the model.
    #[test]
    fn a_cut_in_with_no_english_word_is_unclear() {
        for fragment in [
            "vestido",
            "analizar",
            "miel miel",
            "Tá.",
            "Los Ángeles",
            "mueve mueve mueve",
            "mueve mueve mueve mueve mueve mueve",
            "pensar",
            "Venga, nos vemos luego.",
            "vestido rojo y azul fuerte",
        ] {
            assert_eq!(
                unclear_in_conversation(fragment, false, true),
                Some(CUT_IN_FRAGMENT),
                "{fragment}"
            );
            assert_eq!(
                unclear_in_conversation(fragment, false, false),
                None,
                "{fragment}"
            );
        }
        assert_eq!(
            unclear_in_conversation("23", false, true),
            Some(ONLY_DIGITS)
        );
        // Counting is not a number alone: a cut-in's fragment, else the model's.
        assert_eq!(
            unclear_in_conversation("1 2 3", false, true),
            Some(CUT_IN_FRAGMENT)
        );
        assert_eq!(unclear_in_conversation("1 2 3", false, false), None);
        for said in [
            "Stop.",
            "Wait.",
            "Okay.",
            "No.",
            "No no.",
            "Tell me",
            "a joke",
            "Stop, please.",
            "That's enough",
            "Wait, wait, wait.",
        ] {
            assert_eq!(unclear_in_conversation(said, false, true), None, "{said}");
        }
    }

    /// A call written as words is caught whole; words that merely contain
    /// an underscore, or a sentence after a call, are not.
    #[test]
    fn a_bare_call_is_a_function_call_written_as_words() {
        for call in [
            "not_for_me{}",
            " not_for_me ",
            "not_for_me()",
            "set_timer{\"minutes\": 5}",
            "print(default_api.not_for_me())",
            "not_for_me.",
        ] {
            assert!(bare_call(call), "{call}");
        }
        for words in [
            "Sure.",
            "It is noon.",
            "not_for_me is a tool, sorry.",
            "Hello there",
        ] {
            assert!(!bare_call(words), "{words}");
        }
    }

    /// A call is left out whatever pieces it came in; the words around it
    /// are kept, and a new answer starts with nothing held.
    #[test]
    fn a_call_split_across_pieces_is_left_out_whole() {
        let mut calls = CallsLeftOut::default();
        let kept: Vec<_> = [
            "Yes?",
            " not_for",
            "_me{}",
            " Sure.",
            " set_timer{",
            "\"minutes\": 5}",
        ]
        .into_iter()
        .filter(|piece| calls.keeps(1, piece))
        .collect();
        assert_eq!(kept, ["Yes?", " Sure."]);
        assert!(!calls.keeps(1, " not_for"));
        assert!(calls.keeps(2, "_me{} is mine"));
        assert!(calls.keeps(2, " It is noon."));
    }

    /// The owner's six turns at lamp-4ace's desk on 2026-10-16, 17:13 to
    /// 17:16 (turns 5, 7 and 9 to 12), by the blocks the gate counted and
    /// their loud blocks the detector did not call speech: each passes the
    /// gate said close, and is a sound when it was not. A knock or a mug set
    /// down close to the lamp, a few speech blocks and a few loud ones, is
    /// still a sound (Ten on #1137).
    #[test]
    fn a_short_utterance_said_close_passes_the_scrap_gate_and_a_knock_does_not() {
        for (counted, loud) in [(16, 0), (19, 0), (19, 4), (17, 5), (26, 27), (14, 26)] {
            assert!(scrap(counted, false, None, None), "{counted}");
            assert!(!scrap(counted, false, None, Some(loud)), "{counted}");
        }
        for (counted, loud) in [(5, 3), (10, 3), (12, 2), (3, 6)] {
            assert!(scrap(counted, false, None, Some(loud)), "{counted} {loud}");
        }
        assert!(!scrap(SCRAP_BLOCKS, false, None, None));
    }
}
