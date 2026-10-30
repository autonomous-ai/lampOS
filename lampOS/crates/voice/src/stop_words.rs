//! A stop word heard while the lamp has stopped for someone (#828, step 2).
//!
//! The duplex rule stops the lamp's speech when someone is plainly over it,
//! and then knows only sound and timing: speech once the lamp is silent
//! commits the interruption, and stillness resumes the reply. A short
//! "Stop." is over by the time the lamp has stopped, so the rule took it
//! for "mm-hm" and went on. On lamp-4ace on 16 October at 05:39 the short
//! stop words did not stop the story in 2 of 4.
//!
//! With `LAMP_STOP_WORDS=heard` the words said in a yield are listened for,
//! with the second voice's speech-to-text ([`crate::second_config`]):
//! - when a reply begins, a stream is opened and kept ready;
//! - the yield's held audio and every block heard while it is in force go
//!   to it, and the rule waits for the words before stillness may resume
//!   the reply (its deadline still resumes it);
//! - once the speech has been still for [`FINALIZE_AFTER_STILL_BLOCKS`], the
//!   words are asked for. They came about 0.25 s after the end of the
//!   speech through the relay on lamp-52e6 (the sixth builder, 16 October);
//! - a stop phrase ([`stop_phrase`]) commits the yield, as speech would:
//!   the reply ends and what was said goes to the provider. Any other words
//!   let stillness resume it as before.
//!
//! With `heard`, nothing is sent but what was said in a yield; with
//! `heard-always`, everything heard while the lamp speaks is sent
//! ([`Listen::Always`]). Without the second voice's configuration it is
//! off.
use crate::second_config::SecondConfig;
use crate::second_provider::net::SttStream;
use std::{
    collections::VecDeque,
    io,
    sync::{Arc, Mutex, PoisonError, mpsc},
    time::Duration,
};
use tokio::sync::mpsc as queue;

/// Where the coordinator reads whether stop words are listened for:
/// `heard` or `heard-always`.
pub const ENVIRONMENT: &str = "LAMP_STOP_WORDS";

/// What is listened to for a stop word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Listen {
    Off,
    /// What is said in a yield (`heard`).
    InYields,
    /// Everything heard while the lamp speaks (`heard-always`, #828): a
    /// short "Stop." said under the lamp's voice is heard by the canceller's
    /// measures as the lamp's own and makes no yield (the first builder's
    /// 01:51 recordings, #828 at 13:1x).
    Always,
}

/// Where the words are heard, for the journal: the relay's host, never a
/// key or a path.
pub fn where_heard(config: &SecondConfig) -> String {
    match &config.stt {
        crate::second_config::Stt::Deepgram { .. } => "Deepgram".into(),
        crate::second_config::Stt::Autonomous { base_url, .. } => reqwest::Url::parse(base_url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| "a relay".into()),
    }
}

/// What the voice's environment asks for. Nothing with a language other
/// than English (#4473): the stop words and backchannels are English's.
pub fn listen() -> Listen {
    if crate::language::from_environment().is_some() {
        return Listen::Off;
    }
    listen_for(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Where the coordinator reads what the stop words hear ([`Source`]):
/// `raw`, `linear`, or the canceller's output as the provider hears it
/// (unset, or `processed`). On lamp-4ace on 16 October at 22:44 the
/// canceller's output carried 16 to 18 speech starts and no words over a
/// reply: the canceller takes out a short "Stop." said over the lamp with
/// its echo (`lamp_audio::duplex`; the first builder on #1047). Raw blocks
/// carry the lamp's own words too, which [`stop_over`] already leaves out.
pub const SOURCE_ENVIRONMENT: &str = "LAMP_STOP_WORDS_SOURCE";

/// The blocks the stop words hear ([`SOURCE_ENVIRONMENT`]). Anything but
/// [`Source::Processed`] is asked of the capture worker, which sends it
/// beside each cancelled block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    /// The canceller's output, as the provider hears it: unset, `processed`
    /// or any other value.
    #[default]
    Processed,
    /// The block as the microphone recorded it, before the canceller (`raw`,
    /// `--raw-blocks`, #1047). Over the lamp it is mostly the lamp's own
    /// voice: the speech-to-text returned the lamp's story, and a "Lamp,
    /// stop." said over it was never written (the first builder on #828,
    /// 19 October 01:29).
    Raw,
    /// The canceller's linear stage, before its residual-echo suppressor
    /// (`linear`, `--linear-blocks`, #828): the microphone less the echo
    /// its adaptive filter predicts. It keeps the person's word the
    /// suppressor takes away with the lamp's voice, and much of that voice
    /// too: on lamp-4ace's mic2 the linear stage took only 3.1 to 4.1 dB of
    /// it out (#828, 17 October 08:01), far less than on the scripted rig.
    /// A block the canceller did not run for has none, and the stop words
    /// hear the block handed on, as the microphone recorded it.
    Linear,
}

impl Source {
    /// The value the environment names, as the counts file and the
    /// journal write it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Processed => "processed",
            Self::Raw => "raw",
            Self::Linear => "linear",
        }
    }
}

/// What the stop words hear, as the environment asks.
pub fn source() -> Source {
    source_for(std::env::var(SOURCE_ENVIRONMENT).ok().as_deref())
}

fn source_for(value: Option<&str>) -> Source {
    match value {
        Some("raw") => Source::Raw,
        Some("linear") => Source::Linear,
        _ => Source::Processed,
    }
}

/// `on` exactly: the stop stream's speech-to-text is asked with the stop
/// words as key terms (Deepgram's `keyterm`, as the main stream's are sent),
/// first among the unit's own ([`with_key_terms`]). On lamp-4ace's replay
/// on 21 Oct (#2661) the stop path wrote the lamp's own story nearly word
/// for word on `pre_aec` and `aec_linear` and never a played "Stop."
/// (the first builder on #2602, 23:44). Off unless set.
pub const KEYTERM_ENVIRONMENT: &str = "LAMP_STOP_WORDS_KEYTERM";

/// The stop words as the stop stream asks for them ([`KEYTERM_ENVIRONMENT`]):
/// a subset of [`STOP_WORDS`] and [`STOP_PAIRS`] on purpose, the ones people
/// say to a lamp; "quiet", "hush" and "shush" are left out (the reviewer's P3
/// on #2690).
pub const KEY_TERMS: &[&str] = &["Stop", "Wait", "Hold on", "Hang on", "Enough", "Pause"];

/// Whether the switch is on, as the environment says.
pub fn key_terms_from_environment() -> bool {
    std::env::var(KEYTERM_ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// `config` as the stop stream connects with it: with `on`, the stop words
/// first among its key terms, then the unit's own not already among them,
/// at most [`crate::second_config::MOST_KEY_TERMS`]. Without, as it is.
pub fn with_key_terms(mut config: SecondConfig, on: bool) -> SecondConfig {
    config.stt_key_terms = stop_key_terms(std::mem::take(&mut config.stt_key_terms), on);
    config
}

/// The key terms the stop stream asks with, from the unit's `own`
/// ([`with_key_terms`]).
fn stop_key_terms(own: Vec<String>, on: bool) -> Vec<String> {
    if !on {
        return own;
    }
    KEY_TERMS
        .iter()
        .map(|term| (*term).to_owned())
        .chain(
            own.into_iter()
                .filter(|term| !KEY_TERMS.iter().any(|stop| stop.eq_ignore_ascii_case(term))),
        )
        .take(crate::second_config::MOST_KEY_TERMS)
        .collect()
}

/// A makeup gain in dB for the canceller's output ([`Source::Processed`],
/// `aec_only`) before the stop stream hears it, to the level `aec_linear`
/// has: on the replay (#2661) `aec_only` gave 2 results in 81 pauses. A
/// number over 0 and at most [`MOST_GAIN_DB`]; anything else, none. Only
/// that source is gained ([`StopWords::with_gain`]).
pub const GAIN_ENVIRONMENT: &str = "LAMP_STOP_WORDS_GAIN_DB";
/// The most gain taken: past it the suppressor's floor is noise.
pub const MOST_GAIN_DB: f32 = 24.0;

/// The gain the environment asks for, bounded ([`GAIN_ENVIRONMENT`]).
pub fn gain_from_environment() -> Option<f32> {
    gain_for(std::env::var(GAIN_ENVIRONMENT).ok().as_deref())
}

/// `value` as a gain: a number over 0 and at most [`MOST_GAIN_DB`].
pub fn gain_for(value: Option<&str>) -> Option<f32> {
    value?
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|gain| gain.is_finite() && *gain > 0.0 && *gain <= MOST_GAIN_DB)
}

/// The gain for stop words heard in a yield, whose [`StopWords`] keeps the
/// default source while capture hands on `source`'s blocks: none unless they
/// are the canceller's output (El's P2 on #2690).
pub fn yield_gain(gain_db: Option<f32>, source: Source) -> Option<f32> {
    gain_db.filter(|_| source == Source::Processed)
}

/// `samples` raised by `gain_db`, clipped at full scale.
pub fn raised(samples: &[i16], gain_db: f32) -> Vec<i16> {
    let factor = 10f32.powf(gain_db / 20.0);
    samples
        .iter()
        .map(|&sample| {
            (f32::from(sample) * factor)
                .round()
                .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
        })
        .collect()
}

/// Where the coordinator reads whether a stop heard over the lamp is held
/// for the lamp's own words (`own-words`, #828 section 2): at a reply's
/// start, with nothing queued, the lamp's text for a block can come after
/// the speech-to-text's words for it, and its own "stop" stopped it. Off
/// unless set.
pub const HOLD_ENVIRONMENT: &str = "LAMP_STOP_WORDS_HOLD";

/// Whether a stop heard over the lamp waits for its own words.
pub fn holds_for_own_words() -> bool {
    hold_for(std::env::var(HOLD_ENVIRONMENT).ok().as_deref())
}

fn hold_for(value: Option<&str>) -> bool {
    value == Some("own-words")
}

/// With [`holds_for_own_words`], the longest a stop heard over the lamp
/// waits for the lamp's own words to cover the block it came in, on the
/// blocks' capture clock: what a person's real stop is delayed by, at most.
pub const HOLD_US: u64 = 150_000;

fn listen_for(value: Option<&str>) -> Listen {
    match value {
        Some("heard") => Listen::InYields,
        Some("heard-always") => Listen::Always,
        _ => Listen::Off,
    }
}

/// The lamp's own words this long before what is heard are its own, not a
/// person's: it may say "wait" in a story, and the canceller lets its voice
/// through. Its words are written a moment before they are played.
pub const LAMP_WORDS_US: u64 = 6_000_000;

/// When words whose text came at `now_us` will be heard: after the
/// `queued_samples` of the reply's audio ahead of them, at the render rate.
/// The voice holds up to 30 s of a reply ahead of playback, so its words
/// are stamped with this for [`StopWords::lamp_said`] (the reviewer on
/// #1047).
pub fn plays_at_us(now_us: u64, queued_samples: usize) -> u64 {
    now_us.saturating_add(queued_samples as u64 * 1_000_000 / u64::from(lamp_audio::RENDER_RATE))
}

/// Blocks of stillness in a yield after which its words are asked for:
/// 100 ms, so that they come back inside the 400 ms after which stillness
/// would resume the reply.
pub const FINALIZE_AFTER_STILL_BLOCKS: u8 = 10;

/// Still blocks after speech, while the lamp speaks, after which the
/// stream is asked for the words so far ([`Ears::flush`]): 200 ms. With
/// the relay's 269 ms to answer, a "Stop." stops the lamp about 0.5 s
/// after it is said.
pub const FLUSH_AFTER_STILL_BLOCKS: u8 = 20;

/// With the raw or linear blocks ([`Source`]), blocks sent while the lamp
/// sounds after which the words are asked for, whatever the detector heard: 50,
/// half a second. Over the lamp the canceller's output, which the detector
/// scores, holds no speech, so a pause after speech never came and no
/// words were asked for (the first builder at 02:29 and 02:57 on #1047).
/// Words said across an ask come back in two results; a stop word is one
/// word, but "Hold on." split there is missed.
pub const CADENCE_BLOCKS: u16 = 50;

/// Words that tell the lamp to stop, alone or among others: "Stop.",
/// "Wait, wait, wait.", "Okay, okay, that is enough.", "Lamp, stop."
const STOP_WORDS: &[&str] = &["stop", "wait", "enough", "pause", "quiet", "hush", "shush"];
/// Two words that do the same together: "Hold on.", "Hang on."
const STOP_PAIRS: &[(&str, &str)] = &[("hold", "on"), ("hang", "on")];

/// Words that, just before a stop word, turn it round: "Don't stop.", "No,
/// never stop." (the reviewer on #970).
const NOT: &[&str] = &[
    "don't", "dont", "not", "never", "doesn't", "didn't", "won't", "can't", "cannot",
];

/// The words of `text`, lowercase. A word with an apostrophe is kept
/// whole, and ’ reads as '.
fn words(text: &str) -> Vec<String> {
    text.replace('\u{2019}', "'")
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(|word| word.trim_matches('\''))
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Whether what was heard tells the lamp to stop.
pub fn stop_phrase(heard: &str) -> bool {
    stop_over(heard, "")
}

/// Where the voice reads whether a yield whose words are a backchannel is
/// held as a yield, not committed as a turn (#2939): `on`. Off unless set,
/// and inert without `LAMP_STOP_WORDS=heard` or `heard-always`.
pub const BACKCHANNEL_ENVIRONMENT: &str = "LAMP_BACKCHANNEL_HOLDS";

/// Whether the environment asks for backchannels to be held.
pub fn holds_backchannels() -> bool {
    std::env::var(BACKCHANNEL_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Where the voice reads whether an aside said while the lamp speaks is
/// held as a backchannel is (#2939, 25 Oct): `on`. Off unless set, and inert
/// without [`BACKCHANNEL_ENVIRONMENT`]: with it, [`ASIDES`] count as
/// backchannel words and "hm" or "hmm" alone is one ("Hmm, interesting."
/// stopped the lamp's story 4 of 4 times on lamp-52e6 on 25 Oct).
pub const ASIDES_ENVIRONMENT: &str = "LAMP_ASIDES_HOLD";

/// Whether the environment asks for asides to be held as backchannels.
pub fn holds_asides() -> bool {
    std::env::var(ASIDES_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Where the voice reads whether a yield's decision is journaled with its
/// words (#2939): `on`. Off unless set, for debugging only, and only with
/// backchannel holds; at most two words of a person's speech are written
/// ([`StopWords::with_backchannel_debug`]).
pub const BACKCHANNEL_DEBUG_ENVIRONMENT: &str = "LAMP_BACKCHANNEL_DEBUG";

/// Whether the environment asks for a yield's words to be journaled: only
/// with backchannel holds on too.
pub fn debugs_backchannels() -> bool {
    holds_backchannels()
        && std::env::var(BACKCHANNEL_DEBUG_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// How many words `heard` has: none when it has no word at all, as a
/// final of only "..." (#2939), else as many as spaces part.
fn word_count(heard: &str) -> usize {
    if words(heard).is_empty() {
        0
    } else {
        heard.split_whitespace().count()
    }
}

/// `heard` with every hyphen or dash taken out: "uh\u{2011}huh" is "uhhuh".
fn dashes_joined(heard: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    heard
        .nfc()
        .filter(|c| !matches!(c, '-' | '\u{2010}'..='\u{2015}' | '\u{2212}'))
        .collect()
}

/// The journal line for a yield's words with
/// [`BACKCHANNEL_DEBUG_ENVIRONMENT`]: at most two words, lower case and
/// without punctuation, and why they were decided so; none for more.
fn debug_line(heard: &str, lamp_asked: bool, vietnamese: bool, asides: bool) -> Option<String> {
    let said = words(&dashes_joined(heard));
    if said.len() > 2 {
        return None;
    }
    let why = if said.is_empty() {
        "no words"
    } else if stop_phrase(heard) {
        "a stop phrase"
    } else if backchannel(heard, vietnamese, asides) {
        if lamp_asked {
            "on the list, but the lamp asked a question"
        } else {
            "on the list"
        }
    } else if heard.contains('?') {
        "asked as a question"
    } else if !vietnamese && said.iter().all(|word| LEADS_ONLY.contains(&word.as_str())) {
        "leads-only"
    } else {
        "not on the list"
    };
    Some(format!(
        "lamp-voice: backchannel debug: a yield's words \"{}\": {why}",
        said.join(" ")
    ))
}

/// Words that only acknowledge, said while the lamp speaks: "Mm-hm.",
/// "Yeah.", "Oh, okay." Hyphens and other dashes are joined first, so
/// "uh-huh", with a hyphen or a dash, is "uhhuh"; the speech-to-text's other
/// spellings are on it too: "Mhmm.", "Mmm.", "Uh-hum.", "Yea", "Yeh"
/// (#2939, lamp-52e6 on 23 Oct: a played "Uh-huh." came as one word not on
/// the list and committed). "Uh", "so", "no", "what" and "huh" are left
/// out: they may begin a turn; so is "uh-uh", a no. "Oh", "ah", "hm" and
/// "hmm" only lead another ([`LEADS_ONLY`]).
const BACKCHANNELS: &[&str] = &[
    "mm", "mmm", "mhm", "mhmm", "mmhm", "mmhmm", "hm", "hmm", "uhhuh", "uhhum", "yeah", "yea",
    "yeh", "yah", "yes", "yep", "yup", "ok", "okay", "okey", "alright", "right", "oh", "ah",
    "sure",
];
/// Backchannel words that are one only before another ("Oh, okay.", "Hmm,
/// I see."): alone, "Oh." or "Hmm." may open a turn, and on lamp-52e6 on
/// 23 Oct a "So." and an "Ahem." said over the lamp each came as one word
/// taken for a backchannel and held.
const LEADS_ONLY: &[&str] = &["oh", "ah", "hm", "hmm"];
/// With `LAMP_ASIDES_HOLD=on` (#2939, 25 Oct): remarks a person makes to
/// themselves or the room while the lamp tells something, not meant to stop
/// it ("Hmm, interesting.", "Wow.", "Oh, nice."), counted as backchannel
/// words; "hm" and "hmm" then stand alone too.
const ASIDES: &[&str] = &["interesting", "wow", "nice"];
/// The same, when the speech-to-text is told Vietnamese (`stt_language`).
/// "Rồi" and "đúng" are left out: as often a reply ("done", "that's right")
/// as an acknowledgment (the reviewer-lead on #2959).
const BACKCHANNELS_VI: &[&str] = &["ừ", "ừm", "ờ", "à", "ồ", "dạ", "vâng", "ok"];
/// Pairs that acknowledge as one word does, as the speech-to-text may
/// write them: "I see.", "All right.", "Uh huh." (#2939, lamp-52e6 on 23
/// Oct: "Right" came as two words and committed).
const BACKCHANNEL_PAIRS: &[(&str, &str)] = &[
    ("i", "see"),
    ("all", "right"),
    ("uh", "huh"),
    ("uh", "hum"),
    ("mm", "hm"),
    ("mm", "hmm"),
    ("got", "it"),
];

/// Whether the lamp's last finished sentence in `said` is a question: a
/// "Yes." or "Sure." said then answers it, and is no backchannel (the
/// reviewer-lead on #2959).
pub fn lamp_asked(said: &str) -> bool {
    said.trim_end()
        .rfind(['.', '!', '?', '\u{2026}', '\u{ff1f}', '\u{3002}'])
        .is_some_and(|at| said[at..].starts_with(['?', '\u{ff1f}']))
}

/// Whether what was heard is a backchannel (#2939): one or two words, all
/// on the list for the language, a pair of [`BACKCHANNEL_PAIRS`] counting
/// as one ("Oh, I see.", "Okay, sure."), no question mark and no stop
/// phrase. With `asides` ([`ASIDES_ENVIRONMENT`]), [`ASIDES`] are on the
/// English list and only "oh" and "ah" lead only.
pub fn backchannel(heard: &str, vietnamese: bool, asides: bool) -> bool {
    // Any hyphen or dash: "uh\u{2011}huh", "mm\u{2013}hm".
    let heard = dashes_joined(heard);
    let list = if vietnamese {
        BACKCHANNELS_VI
    } else {
        BACKCHANNELS
    };
    let said = words(&heard);
    let (mut at, mut units) = (0, 0);
    while at < said.len() {
        let pair = !vietnamese
            && said.get(at + 1).is_some_and(|next| {
                BACKCHANNEL_PAIRS.contains(&(said[at].as_str(), next.as_str()))
            });
        let word = said[at].as_str();
        let aside = asides && !vietnamese && ASIDES.contains(&word);
        if !(pair || aside || list.contains(&word)) {
            return false;
        }
        at += if pair { 2 } else { 1 };
        units += 1;
    }
    let leads_only = !vietnamese
        && said.iter().all(|word| {
            LEADS_ONLY.contains(&word.as_str())
                && !(asides && matches!(word.as_str(), "hm" | "hmm"))
        });
    !heard.contains('?') && (1..=2).contains(&units) && !leads_only && !stop_phrase(&heard)
}

/// Whether `word`, lower case, is one of the stop words, or a word of a
/// pair that stops ("hold", "hang", "on"): the one list, for a reader that
/// needs to know a stop asks nothing more (`crate::wait_words`).
pub fn stop_word(word: &str) -> bool {
    STOP_WORDS.contains(&word)
        || STOP_PAIRS
            .iter()
            .any(|(first, second)| word == *first || word == *second)
}

/// Whether what was heard tells the lamp to stop, when the lamp itself has
/// just said `lamp`: a stop word, or pair, that is not among the lamp's own
/// words, nor right after one that turns it round. "The fox had to wait"
/// in its story stops nothing, and nor does "Don't stop."
pub fn stop_over(heard: &str, lamp: &str) -> bool {
    let (heard, lamp) = (words(heard), words(lamp));
    let own = |word: &String| lamp.contains(word);
    let turned = |at: usize| at > 0 && NOT.contains(&heard[at - 1].as_str());
    heard
        .iter()
        .enumerate()
        .any(|(at, word)| STOP_WORDS.contains(&word.as_str()) && !own(word) && !turned(at))
        || heard.windows(2).enumerate().any(|(at, pair)| {
            STOP_PAIRS.contains(&(pair[0].as_str(), pair[1].as_str()))
                && !(own(&pair[0]) && own(&pair[1]))
                && !turned(at)
        })
}

/// Where a yield's words come from.
pub trait Ears: Send {
    /// A reply has begun: have a stream ready for a yield in it.
    fn ready(&mut self);
    /// Audio said in the yield in force, in order, with when it was
    /// captured.
    fn hear(&mut self, captured_at_us: u64, samples: &[i16]);
    /// The yield's speech has gone still: ask for its words.
    fn finalize(&mut self);
    /// Listening to everything ([`Listen::Always`]): a pause, so ask for
    /// the words of what has been heard so far, and go on listening.
    fn flush(&mut self) {}
    /// The words, once they came: at most once a yield.
    fn words(&mut self) -> Option<String>;
    /// Listening to everything: the next result, and whether it may decide
    /// a yield's words, being final and ending after the yield in force
    /// began ([`Self::mark`], #2939). By default every result may.
    fn result(&mut self) -> Option<Said> {
        self.words().map(|words| Said {
            words,
            decides: true,
        })
    }
    /// Listening to everything: a yield begins after the audio heard so
    /// far, and a result that ends before it is not its words.
    fn mark(&mut self) {}
    /// The yield is over: what it heard is dropped, nothing more of it goes
    /// out once this returns, and its words, if they come later, are not
    /// given.
    fn forget(&mut self);
    /// The reply is over: as `forget`, and no stream is kept.
    fn rest(&mut self);
}

/// A result listening to everything ([`Ears::result`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Said {
    pub words: String,
    /// Final, and ending after the yield in force began: it may decide the
    /// yield's words. An interim result, or one of a flush or an utterance
    /// before the yield, may not (reviewer-two's P2 on #3200).
    pub decides: bool,
}

/// What the duplex rule is to be told after a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tell {
    Nothing,
    /// A yield began: it waits for its words.
    Await,
    /// Its words came: whether they were a stop phrase, how many, and
    /// whether they were a backchannel ([`backchannel`]).
    Words {
        stop: bool,
        count: usize,
        backchannel: bool,
    },
    /// It ended before any words came.
    Unheard,
}

/// The yield's words, followed block by block.
pub struct StopWords {
    ears: Box<dyn Ears>,
    replying: bool,
    rested: bool,
    yielded: bool,
    asked: bool,
    /// The yield's words were first asked for since last looked at
    /// ([`Self::take_asked`]).
    just_asked: bool,
    told: bool,
    /// The microphone was closed ([`Self::close`]): the duplex rule's
    /// yield, which a hand Stop or a head touch leaves in force, is not a
    /// new one, and is let be until it ends (the reviewer on #970).
    closed: bool,
    /// Everything heard while the lamp speaks is listened to
    /// ([`Listen::Always`]), with the lamp's own words of the last
    /// [`LAMP_WORDS_US`], a stop heard and not yet taken, and whether this
    /// reply's one stop has been heard.
    always: bool,
    lamp: VecDeque<(u64, String)>,
    stopped: bool,
    stop_heard: bool,
    /// This reply's blocks sent and results back, for the journal at its
    /// end: whether the stream heard anything at all.
    sent: usize,
    results: usize,
    /// With `always`: whether the block in hand is speech-like, as the
    /// coordinator's detector scored it ([`Self::heard_speech`]); whether
    /// speech has been heard since the last pause; the still blocks since
    /// it; and the pauses this reply asked for words at.
    speech_like: bool,
    voiced: bool,
    still: u8,
    flushes: usize,
    /// [`COUNTS_ENVIRONMENT`]: where this reply's counts go at its end, and
    /// what it heard so far, each with whether it stopped the lamp.
    counts: Option<std::path::PathBuf>,
    heard: Vec<Kept>,
    /// With `always`: the words are also asked for every
    /// [`CADENCE_BLOCKS`] sent ([`Self::with_cadence`]), and the blocks
    /// sent since they last were.
    cadence: bool,
    since_asked: u16,
    /// What the coordinator hands on ([`Self::with_source`]), for the
    /// counts file.
    source: Source,
    /// A makeup gain for the canceller's output ([`Self::with_gain`]).
    gain: Option<f32>,
    /// With `always`: a stop heard is held for the lamp's own words
    /// ([`Self::with_hold`]), and the one held.
    hold: bool,
    held: Option<Held>,
    /// With `hold`: the latest stamp of the lamp's words stamped ahead of
    /// when they were written, and this reply's held stops, each with how
    /// long it waited (ms) and whether it acted.
    ahead_at: u64,
    holds: Vec<(u64, bool)>,
    /// The speech-to-text is told Vietnamese: its backchannels are
    /// Vietnamese ([`Self::with_language`]).
    vietnamese: bool,
    /// Asides count as backchannels ([`Self::with_asides`]).
    asides: bool,
    /// The lamp's last finished sentence before the yield in force was a
    /// question: its words answer it, and are no backchannel.
    lamp_asked: bool,
    /// With `always`: a yield is told its words as with `heard`
    /// ([`Self::with_backchannel_holds`], #2939).
    tells_yields: bool,
    /// With `tells_yields`: the final words of the yield in force so far,
    /// and whether they were last told as a backchannel.
    yield_words: String,
    yield_backchannel: bool,
    /// With `tells_yields` and `hold`: the yield's words held as a stop
    /// is ([`Self::held_stop`]), from the capture time of the block their
    /// result arrived in, with their count and whether a backchannel.
    yield_held: Option<(u64, usize, bool)>,
    /// A yield's words are journaled with its decision
    /// ([`Self::with_backchannel_debug`]).
    debug: bool,
}

/// A stop heard over the lamp and held ([`HOLD_US`]): what was heard,
/// the capture time of the block its result arrived in (after the words
/// were said, by the relay's answer), and its results' numbers.
struct Held {
    heard: String,
    at_us: u64,
    results: Vec<usize>,
}

impl StopWords {
    /// What is said in a yield ([`Listen::InYields`]).
    pub fn new(ears: Box<dyn Ears>) -> Self {
        Self {
            ears,
            replying: false,
            rested: true,
            yielded: false,
            asked: false,
            just_asked: false,
            told: false,
            always: false,
            lamp: VecDeque::new(),
            stopped: false,
            stop_heard: false,
            closed: false,
            sent: 0,
            results: 0,
            speech_like: false,
            voiced: false,
            still: 0,
            flushes: 0,
            counts: None,
            heard: Vec::new(),
            cadence: false,
            since_asked: 0,
            source: Source::Processed,
            gain: None,
            hold: false,
            held: None,
            ahead_at: 0,
            holds: Vec::new(),
            vietnamese: false,
            asides: false,
            lamp_asked: false,
            tells_yields: false,
            yield_words: String::new(),
            yield_backchannel: false,
            yield_held: None,
            debug: false,
        }
    }

    /// With `on`, listening to everything, a yield is told its words as with
    /// `heard` ([`Tell::Await`], then [`Tell::Words`] with its own final
    /// words only: a backchannel as it comes, told again as words if a
    /// later final makes it more), and they are asked for at its
    /// [`FINALIZE_AFTER_STILL_BLOCKS`]th still block if no pause has asked
    /// yet: `LAMP_BACKCHANNEL_HOLDS=on` with `heard-always` (#2939). A stop
    /// phrase is told as a stop, and commits the yield as with `heard`,
    /// whether or not it also stops the lamp as a hand Stop does.
    pub fn with_backchannel_holds(mut self, on: bool) -> Self {
        self.tells_yields = on && self.always;
        self
    }

    /// With `on`, [`ASIDES`] are backchannels too: `LAMP_ASIDES_HOLD=on`,
    /// which acts only through backchannel holds (#2939, 25 Oct).
    pub fn with_asides(mut self, on: bool) -> Self {
        self.asides = on;
        self
    }

    /// With `on`, a yield's words, at most two, are journaled with why
    /// they were decided so: `LAMP_BACKCHANNEL_DEBUG=on`, taken only with
    /// backchannel holds (#2939). Off, nothing more is journaled.
    pub fn with_backchannel_debug(mut self, on: bool) -> Self {
        self.debug = on;
        self
    }

    /// The journal line for a yield's words, when debugging asks for one
    /// ([`Self::with_backchannel_debug`]).
    fn debug_words(&self, heard: &str) -> Option<String> {
        self.debug
            .then(|| debug_line(heard, self.lamp_asked, self.vietnamese, self.asides))
            .flatten()
    }

    /// With `on`, listening to everything, a stop heard is acted on only
    /// once the lamp's own words cover the block it came in, or after
    /// [`HOLD_US`]: if they then say it themselves, it is the lamp's own
    /// and stops nothing (#828 section 2).
    pub fn with_hold(mut self, on: bool) -> Self {
        self.hold = on && self.always;
        self
    }

    /// With `on`, listening to everything, the words are asked for every
    /// [`CADENCE_BLOCKS`] sent as well as at each pause after speech: for
    /// the raw or linear blocks, whose speech the detector does not score.
    pub fn with_cadence(mut self, on: bool) -> Self {
        self.cadence = on && self.always;
        self
    }

    /// Hearing `source`'s blocks: anything but the canceller's output is
    /// asked for on the cadence too ([`Self::with_cadence`]), and the
    /// counts file says which it was.
    pub fn with_source(mut self, source: Source) -> Self {
        self.source = source;
        self.with_cadence(source != Source::Processed)
    }

    /// The canceller's output raised by `gain_db` before it is heard
    /// ([`GAIN_ENVIRONMENT`]); the raw and linear blocks never are.
    pub fn with_gain(mut self, gain_db: Option<f32>) -> Self {
        self.gain = gain_db;
        self
    }

    /// The speech-to-text's language (`stt_language`): Vietnamese (`vi`,
    /// `vi-VN`) takes the Vietnamese backchannels (#2939).
    pub fn with_language(mut self, language: Option<&str>) -> Self {
        self.vietnamese = language.is_some_and(|language| {
            let language = language.to_ascii_lowercase();
            language == "vi" || language.starts_with("vi-")
        });
        self
    }

    /// `samples` as the ears hear them: raised with a gain, on the
    /// canceller's output only.
    fn heard<'a>(&self, samples: &'a [i16]) -> std::borrow::Cow<'a, [i16]> {
        match self.gain.filter(|_| self.source == Source::Processed) {
            Some(gain) => std::borrow::Cow::Owned(raised(samples, gain)),
            None => std::borrow::Cow::Borrowed(samples),
        }
    }

    /// Everything heard while the lamp speaks ([`Listen::Always`]). A yield
    /// takes its usual course; a stop phrase heard stops the reply as a
    /// hand Stop does ([`Self::spoken_stop`]).
    pub fn always(ears: Box<dyn Ears>) -> Self {
        Self::always_counted(ears, counts_path())
    }

    /// The same, with each reply's counts written to `counts`.
    pub fn always_counted(ears: Box<dyn Ears>, counts: Option<std::path::PathBuf>) -> Self {
        Self {
            always: true,
            counts,
            ..Self::new(ears)
        }
    }

    /// Whether the next block is speech-like, as the coordinator's detector
    /// scored it, for the pauses at which the words are asked for.
    pub fn heard_speech(&mut self, speech_like: bool) {
        self.speech_like = speech_like;
    }

    /// Whether everything heard while the lamp speaks is listened to
    /// ([`Listen::Always`]).
    pub fn hears_always(&self) -> bool {
        self.always
    }

    /// The lamp said `text`, as its words were written, at `at_us`: when
    /// they play, `ahead` of when they were written when the reply's audio
    /// is queued before them. Only such words can end a held stop early:
    /// stamped as written, a piece can be text for audio played before it.
    /// In a yield the words heard before it say whether the lamp had just
    /// asked something ([`lamp_asked`], #2959).
    pub fn lamp_said(&mut self, text: &str, at_us: u64, ahead: bool) {
        self.lamp.push_back((at_us, text.to_owned()));
        if self.always && ahead {
            self.ahead_at = self.ahead_at.max(at_us);
        }
    }

    /// Whether the yield in force had its words first asked for since
    /// last looked at: they end there, and what is said after is not in
    /// them (reviewer-two's P2s on #3295).
    pub fn take_asked(&mut self) -> bool {
        std::mem::take(&mut self.just_asked)
    }

    /// Whether a stop phrase was heard over the lamp since last asked:
    /// once a reply.
    pub fn spoken_stop(&mut self) -> bool {
        std::mem::take(&mut self.stopped)
    }

    /// [`Listen::Always`]: every block while the lamp sounds, or while it
    /// has stopped for someone, goes to the stream, and each result is
    /// read as it comes.
    fn listen_always(
        &mut self,
        replying: bool,
        sounding: bool,
        yielded: bool,
        still: Option<u8>,
        captured_at_us: u64,
        samples: &[i16],
    ) -> Tell {
        if replying && !self.replying {
            self.ears.ready();
            self.rested = false;
            self.stopped = false;
            self.stop_heard = false;
            self.sent = 0;
            self.results = 0;
            self.voiced = false;
            self.still = 0;
            self.flushes = 0;
            self.since_asked = 0;
            self.heard.clear();
            self.held = None;
            self.ahead_at = 0;
            self.holds.clear();
        }
        self.replying = replying;
        let mut tell = Tell::Nothing;
        if self.tells_yields {
            if replying && yielded && !self.yielded {
                self.yielded = true;
                self.asked = false;
                self.told = false;
                self.yield_words.clear();
                self.yield_backchannel = false;
                self.yield_held = None;
                self.lamp_asked = self.lamp_asked_by(captured_at_us);
                self.ears.mark();
                tell = Tell::Await;
            } else if !yielded && self.yielded {
                self.yielded = false;
                if !self.told {
                    tell = Tell::Unheard;
                }
            }
        }
        if replying && (sounding || yielded) {
            let heard = self.heard(samples);
            self.ears.hear(captured_at_us, &heard);
            self.sent += 1;
            // Through the relay a short "Stop." gets no interim result, and
            // its words come only once `Finalize` asks for them (the first
            // builder's `stt-check` on lamp-4ace, 16:59): they are asked
            // for at each pause after speech, and the stream goes on.
            self.since_asked = self.since_asked.saturating_add(1);
            if self.speech_like {
                self.voiced = true;
                self.still = 0;
            } else if self.voiced {
                self.still += 1;
            }
            let after_speech = self.voiced && self.still >= FLUSH_AFTER_STILL_BLOCKS;
            // A yield told its words is asked for them as `heard` asks.
            let yield_still = self.yielded
                && !self.asked
                && still.is_some_and(|still| still >= FINALIZE_AFTER_STILL_BLOCKS);
            if after_speech || yield_still || self.cadence && self.since_asked >= CADENCE_BLOCKS {
                self.just_asked |= self.yielded && !self.asked;
                self.asked |= self.yielded;
                self.ears.flush();
                self.flushes += 1;
                self.voiced = false;
                self.still = 0;
                self.since_asked = 0;
            }
        }
        let since = captured_at_us.saturating_sub(LAMP_WORDS_US);
        while self.lamp.front().is_some_and(|(at, _)| *at < since) {
            self.lamp.pop_front();
        }
        while let Some(Said {
            words: heard,
            decides,
        }) = self.ears.result()
        {
            self.results += 1;
            // Only the yield's own final words decide it: an interim
            // "Okay" before "Okay, what was that?", or a flush's words from
            // before the yield, leave it undecided and its commit held
            // (reviewer-two's P2 on #3200). A backchannel so far is told,
            // and told again as words if a later final adds more.
            // A final with no words is none: the yield stays undecided,
            // and resumes at the bound as with no words (#2939).
            if self.yielded
                && decides
                && !words(&heard).is_empty()
                && (!self.told || self.yield_backchannel)
            {
                if !self.yield_words.is_empty() {
                    self.yield_words.push(' ');
                }
                self.yield_words.push_str(&heard);
                let held = !self.lamp_asked
                    && backchannel(&self.yield_words, self.vietnamese, self.asides);
                if let Some(line) = self.debug_words(&self.yield_words) {
                    eprintln!("{line}");
                }
                if !(self.told && held) {
                    // The hold reads stop words itself, as a stop over the
                    // lamp is read: a stop the hand-Stop path leaves unsaid
                    // still commits the yield (#2939), and the lamp's own
                    // words, as they are now or within the hold, do not
                    // (reviewer-two's P2 on #3295).
                    let count = word_count(&self.yield_words);
                    let lamp: Vec<&str> = self.lamp.iter().map(|(_, text)| text.as_str()).collect();
                    let stop = stop_over(&self.yield_words, &lamp.join(" "));
                    self.yield_held = None;
                    if stop && self.hold {
                        self.yield_held = Some((captured_at_us, count, held));
                    } else {
                        tell = Tell::Words {
                            stop,
                            count,
                            backchannel: held,
                        };
                    }
                }
                self.told = true;
                self.yield_backchannel = held;
            }
            let lamp: Vec<&str> = self.lamp.iter().map(|(_, text)| text.as_str()).collect();
            let mut stops = replying && !self.stop_heard && stop_over(&heard, &lamp.join(" "));
            if stops && self.hold {
                // Decided below, against the lamp's words as they then are.
                let held = self.held.get_or_insert_with(|| Held {
                    heard: String::new(),
                    at_us: captured_at_us,
                    results: Vec::new(),
                });
                held.heard.push(' ');
                held.heard.push_str(&heard);
                held.results.push(self.results);
                stops = false;
            } else if stops {
                self.stop();
            }
            if self.counts.is_some() {
                let says_stop = stop_phrase(&heard);
                let mut text = heard;
                if text.len() > HEARD_BYTES {
                    let mut end = HEARD_BYTES;
                    while !text.is_char_boundary(end) {
                        end -= 1;
                    }
                    text.truncate(end);
                }
                keep_heard(
                    &mut self.heard,
                    Kept {
                        n: self.results,
                        text,
                        stopped: stops,
                        says_stop,
                    },
                );
            }
        }
        self.decide_held(replying, captured_at_us);
        if let Some((at_us, count, backchannel)) = self.yield_held
            && let Some(stop) = self.held_stop(replying, &self.yield_words, at_us, captured_at_us)
        {
            self.yield_held = None;
            if self.yielded {
                tell = Tell::Words {
                    stop,
                    count,
                    backchannel,
                };
            }
        }
        if !replying && !self.rested {
            self.end_reply();
        }
        tell
    }

    /// Whether the lamp's last finished sentence said by `at_us` was a
    /// question ([`lamp_asked`]).
    fn lamp_asked_by(&self, at_us: u64) -> bool {
        let said: Vec<&str> = self
            .lamp
            .iter()
            .filter(|(at, _)| *at <= at_us)
            .map(|(_, text)| text.as_str())
            .collect();
        lamp_asked(&said.concat())
    }

    /// The stop held, at a block captured at `now_us`: let go if the lamp's
    /// words now hold it or the reply is over; acted on once they cover
    /// the block its result arrived in (a word of theirs stamped ahead of
    /// when it was written, at or after that block), or [`HOLD_US`] after
    /// it. Own words later still than that stop the lamp: a person's stop
    /// is never held longer.
    fn decide_held(&mut self, replying: bool, now_us: u64) {
        let Some(held) = self.held.as_ref() else {
            return;
        };
        let waited_ms = now_us.saturating_sub(held.at_us) / 1000;
        let Some(acts) = self.held_stop(replying, &held.heard, held.at_us, now_us) else {
            return;
        };
        if !acts {
            if replying {
                eprintln!(
                    "lamp-voice: stop words: a stop phrase heard was the lamp's own, let go after {waited_ms} ms held"
                );
            }
            self.holds.push((waited_ms, false));
            self.held = None;
            return;
        }
        let covered = self.ahead_at >= held.at_us;
        if let Some(held) = self.held.take() {
            for kept in &mut self.heard {
                if held.results.contains(&kept.n) {
                    kept.stopped = true;
                }
            }
        }
        eprintln!(
            "lamp-voice: stop words: a held stop phrase acts after {waited_ms} ms held{}",
            if covered {
                ", the lamp's words covering it"
            } else {
                ""
            }
        );
        self.holds.push((waited_ms, true));
        self.stop();
    }

    /// A stop phrase `heard`, held since a block captured at `at_us`, at a
    /// block captured at `now_us`: `Some(false)` if the lamp's words now
    /// hold it or the reply is over, `Some(true)` once they cover the
    /// block it was held at or [`HOLD_US`] after it, `None` while held.
    fn held_stop(&self, replying: bool, heard: &str, at_us: u64, now_us: u64) -> Option<bool> {
        let lamp: Vec<&str> = self.lamp.iter().map(|(_, text)| text.as_str()).collect();
        if !replying || !stop_over(heard, &lamp.join(" ")) {
            Some(false)
        } else if self.ahead_at >= at_us || now_us >= at_us.saturating_add(HOLD_US) {
            Some(true)
        } else {
            None
        }
    }

    /// A stop phrase heard over the lamp stops this reply.
    fn stop(&mut self) {
        self.stop_heard = true;
        self.stopped = true;
        eprintln!("lamp-voice: stop words: a stop phrase was heard over the lamp's voice");
    }

    /// The reply is over, by its end or by a close ([`Self::close`]):
    /// listening to everything, its counts go to the file and the journal
    /// before the stream rests, so that a stop heard over the lamp, which
    /// ends it through the hand's path, is in them too (the reviewer on
    /// #1047).
    fn end_reply(&mut self) {
        // Written before the stream rests, so that its thread adds its
        // own counts to them.
        if let Some(path) = self.counts.as_deref() {
            let value = serde_json::json!({
                "at_unix_ms": unix_ms(),
                "reply": {
                    "blocks_sent": self.sent,
                    "pauses_asked": self.flushes,
                    "asked_every_ms": self.cadence.then_some(u64::from(CADENCE_BLOCKS) * 10),
                    "source": self.source.as_str(),
                    "results": self.results,
                    "stop_heard": self.stop_heard,
                    "held": self.hold.then(|| self.holds.iter().map(|(waited_ms, acted)| {
                        serde_json::json!({"waited_ms": waited_ms, "acted": acted})
                    }).collect::<Vec<_>>()),
                    "heard": self.heard.iter().map(|kept| {
                        serde_json::json!({"n": kept.n, "text": kept.text, "stopped_the_lamp": kept.stopped})
                    }).collect::<Vec<_>>(),
                },
            });
            if let Err(error) = write_json(path, &value) {
                eprintln!(
                    "lamp-voice: stop words: the reply's counts could not be written to {}: {error}",
                    path.display()
                );
            }
        }
        self.ears.rest();
        self.rested = true;
        self.lamp.clear();
        // Counts only: whether anything was sent and heard back.
        eprintln!(
            "lamp-voice: stop words: the reply's {} blocks went to the speech-to-text, its words asked for at {} pauses, {} results came back{}{}",
            self.sent,
            self.flushes,
            self.results,
            if self.stop_heard {
                ", a stop phrase among them"
            } else {
                ""
            },
            if self.hold {
                format!(", {} stops held", self.holds.len())
            } else {
                String::new()
            }
        );
    }

    /// One capture block, after the rule has heard it. `replying`: a reply
    /// is under way, and `sounding`: the lamp's sound still reaches the
    /// microphone. `yielded`: a yield is in force after this block, and
    /// `still` its blocks of stillness. `held`: the audio held for a yield
    /// that began at this block, this block's own among it; `samples`: this
    /// block's audio, captured at `captured_at_us`.
    #[allow(clippy::too_many_arguments)]
    pub fn block(
        &mut self,
        replying: bool,
        sounding: bool,
        yielded: bool,
        still: Option<u8>,
        held: Option<&[crate::activity::ObservedAudio]>,
        captured_at_us: u64,
        samples: &[i16],
    ) -> Tell {
        if self.always {
            return self.listen_always(replying, sounding, yielded, still, captured_at_us, samples);
        }
        let since = captured_at_us.saturating_sub(LAMP_WORDS_US);
        while self.lamp.front().is_some_and(|(at, _)| *at < since) {
            self.lamp.pop_front();
        }
        if self.closed {
            if yielded {
                return Tell::Nothing;
            }
            self.closed = false;
        }
        if replying && !self.replying {
            self.ears.ready();
            self.rested = false;
        }
        self.replying = replying;
        let mut tell = Tell::Nothing;
        if yielded && !self.yielded {
            self.yielded = true;
            self.asked = false;
            self.told = false;
            // What the lamp had said by the yield, not what it was to say.
            self.lamp_asked = self.lamp_asked_by(captured_at_us);
            match held {
                Some(held) => {
                    for block in held {
                        let heard = self.heard(&block.samples);
                        self.ears.hear(block.captured_at_us, &heard);
                    }
                }
                None => {
                    let heard = self.heard(samples);
                    self.ears.hear(captured_at_us, &heard);
                }
            }
            tell = Tell::Await;
        } else if yielded {
            if !self.asked {
                let heard = self.heard(samples);
                self.ears.hear(captured_at_us, &heard);
            }
            if !self.asked && still.is_some_and(|still| still >= FINALIZE_AFTER_STILL_BLOCKS) {
                self.ears.finalize();
                self.asked = true;
                self.just_asked = true;
            }
        } else if self.yielded {
            self.yielded = false;
            self.ears.forget();
            if !self.told {
                tell = Tell::Unheard;
            }
        }
        if !replying && !yielded && !self.rested {
            self.ears.rest();
            self.rested = true;
        }
        if self.yielded
            && !self.told
            && let Some(words) = self.ears.words()
        {
            self.told = true;
            if let Some(line) = self.debug_words(&words) {
                eprintln!("{line}");
            }
            tell = Tell::Words {
                stop: stop_phrase(&words),
                count: word_count(&words),
                backchannel: !self.lamp_asked && backchannel(&words, self.vietnamese, self.asides),
            };
        }
        tell
    }

    /// The microphone was closed, by mute, privacy or a hand Stop: what a
    /// yield heard is dropped, and no stream is kept until the next reply
    /// (#828). Idempotent.
    pub fn close(&mut self) {
        if self.yielded {
            self.yielded = false;
            // Listening to everything, the stream is the reply's, not the
            // yield's: it rests below.
            if !self.always {
                self.ears.forget();
            }
        }
        if !self.rested {
            if self.always {
                self.end_reply();
            } else {
                self.ears.rest();
                self.rested = true;
            }
        }
        self.replying = false;
        self.closed = true;
        self.stopped = false;
        self.held = None;
        self.ahead_at = 0;
        self.lamp.clear();
    }
}

/// `lamp-voice stt-check SECOND_CONFIG WAV` (#1047, the first builder at
/// 15:29): a recording, 16 kHz mono 16-bit, through the speech-to-text the
/// second voice's configuration names, as the stop words send it, with
/// everything the service sends back printed by kind and when. For a
/// recording made for the purpose, never a person's: the words heard are
/// printed.
pub fn stt_check(config: &std::path::Path, wav: &std::path::Path) -> io::Result<()> {
    let config = crate::second_config::load_from_path(config)?
        .with_environment(|name| std::env::var_os(name))?;
    let mut reader = hound::WavReader::open(wav).map_err(io::Error::other)?;
    let spec = reader.spec();
    if (
        spec.sample_rate,
        spec.channels,
        spec.bits_per_sample,
        spec.sample_format,
    ) != (16_000, 1, 16, hound::SampleFormat::Int)
    {
        return Err(io::Error::other("stt-check takes 16 kHz mono 16-bit audio"));
    }
    let samples = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(io::Error::other)?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(crate::second_provider::net::stt_check(&config, &samples))
        .map_err(|error| io::Error::other(error.to_string()))
}

/// The second voice's speech-to-text, on its own thread so that nothing on
/// the coordinator's path waits for the network.
///
/// What may go out is decided apart from the queue to that thread
/// ([`Live`], the reviewer on #970): each yield has a number, every block
/// and every result carries it, and forgetting a yield, resting or
/// dropping the ears changes the live number at once. The thread checks
/// it under its lock as it hands each block to the stream, so once
/// [`Ears::forget`] or [`Ears::rest`] has returned, or the ears are
/// dropped on any way out of the conversation, nothing of that yield is
/// handed on, however much of it is still queued. The thread is woken past
/// the queue and drops the yield's stream, which ends its socket's writer.
/// Words are taken only for the yield in force.
pub struct RelayEars {
    commands: queue::Sender<Command>,
    /// Each result, its reply's number, and the number of the last yield
    /// marked when it was classified ([`Command::Mark`]).
    heard: mpsc::Receiver<(u64, Said, u64)>,
    live: Arc<Live>,
    ended: mpsc::Receiver<()>,
    /// The yields marked so far ([`Ears::mark`]).
    marks: u64,
}

#[derive(Debug)]
enum Command {
    Ready,
    Hear {
        of: u64,
        captured_at_us: u64,
        samples: Vec<i16>,
    },
    Finalize {
        of: u64,
    },
    Flush {
        of: u64,
    },
    /// Listening to everything: the `of`th yield begins after the audio
    /// sent so far.
    Mark {
        of: u64,
    },
    Rest,
}

/// How long a failed connection waits before the next.
const RETRY: Duration = Duration::from_secs(5);
/// A stream the relay closed sooner than this after it connected is a
/// failed connection: the next waits [`RETRY`], so a relay that closes
/// each stream as it opens is not asked again and again.
const FAST_CLOSE: Duration = Duration::from_secs(3);
/// Heard in yields, the streams kept connected while a reply is under
/// way: the ready one and a spare, so a yield that takes one leaves one
/// for the next (lamp-52e6, 24 Oct 13:40: a yield began 0.6 s after the
/// one before took the ready stream, the next connected 666 ms later,
/// and its words came after the hold ran out). Listening to everything,
/// and for a waited sound's words, one.
const MOST_READY_IN_YIELDS: usize = 2;
/// The most audio kept while no stream is ready: 3 s of 10 ms blocks.
const MOST_WAITING: usize = 300;
/// A block captured longer ago than this is never handed on: the most
/// kept while no stream is ready.
const MOST_AGE_US: u64 = 3_000_000;
/// The most commands queued for the thread: a yield's held audio (up to
/// 1.6 s with `LAMP_FIRST_WORDS=kept`) and its 1.1 s, with room to spare.
/// One that does not fit is dropped: the yield is then unheard.
const MOST_QUEUED: usize = 512;
/// How long dropping the ears waits for the thread to end.
const DROP_WAIT: Duration = Duration::from_millis(250);
/// The live number once the ears are dropped: nothing more goes.
const CLOSED: u64 = u64::MAX;

/// Which yield's audio may still go out and whose words may still be
/// taken, shared by the coordinator and the thread, apart from the queue.
struct Live {
    number: Mutex<u64>,
    revoked: tokio::sync::Notify,
    /// The tests' holds on the thread: before each command, before it
    /// tells words, and whether it has ended.
    #[cfg(test)]
    pause: Mutex<()>,
    #[cfg(test)]
    hold_words: Mutex<()>,
    #[cfg(test)]
    holding_words: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    over: std::sync::atomic::AtomicBool,
    /// Shared with the real worker, only for an explicitly dated fixture.
    #[cfg(test)]
    fixture_clock: std::sync::OnceLock<(u64, std::time::Instant)>,
}

impl Live {
    fn new() -> Self {
        Self {
            number: Mutex::new(0),
            revoked: tokio::sync::Notify::new(),
            #[cfg(test)]
            pause: Mutex::new(()),
            #[cfg(test)]
            hold_words: Mutex::new(()),
            #[cfg(test)]
            holding_words: std::sync::atomic::AtomicBool::new(false),
            #[cfg(test)]
            over: std::sync::atomic::AtomicBool::new(false),
            #[cfg(test)]
            fixture_clock: std::sync::OnceLock::new(),
        }
    }

    fn number(&self) -> std::sync::MutexGuard<'_, u64> {
        self.number.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn now(&self) -> u64 {
        *self.number()
    }

    #[cfg(test)]
    fn fixture_now_us(&self) -> u64 {
        self.fixture_clock
            .get()
            .map_or_else(lamp_ipc::monotonic_us, |(base, began)| {
                base + u64::try_from(began.elapsed().as_micros()).expect("bounded fixture time")
            })
    }

    /// Nothing of the yield in force goes on, from now: the next has a new
    /// number. The thread is woken to drop what it holds of it.
    fn revoke(&self) {
        {
            let mut number = self.number();
            if *number != CLOSED {
                *number += 1;
            }
        }
        self.revoked.notify_one();
    }

    /// Nothing goes on, ever.
    fn close(&self) {
        *self.number() = CLOSED;
        self.revoked.notify_one();
    }

    /// `samples` of the yield `of`, captured at `captured_at_us`, handed to
    /// `stream` if that yield is in force and the block is not too old:
    /// checked and handed under the lock a revocation takes.
    fn hand(&self, of: u64, captured_at_us: u64, samples: &[i16], stream: &SttStream) -> bool {
        #[cfg(not(test))]
        let clock = lamp_ipc::monotonic_us;
        #[cfg(test)]
        let clock = || self.fixture_now_us();
        let number = self.number();
        *number == of
            && clock().saturating_sub(captured_at_us) <= MOST_AGE_US
            && stream.send(samples).is_ok()
    }
}

impl RelayEars {
    /// The speech-to-text `config` names, from a thread of its own. With
    /// `always` ([`Listen::Always`]), each result's words are given as they
    /// come, interim ones among them, and no stream is asked to finish.
    pub fn start(config: SecondConfig, always: bool) -> io::Result<Self> {
        let record = std::env::var_os(AUDIO_ENVIRONMENT)
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_absolute());
        Self::with(always, record, counts_path(), move || {
            let config = config.clone();
            async move {
                SttStream::connect(&config)
                    .await
                    .map_err(|error| error.to_string())
            }
        })
    }

    /// The speech-to-text `config` names, from a thread of its own, for a
    /// waited sound's words (`crate::wait_words`, #1123): its journal lines
    /// say `wait words`, and nothing it hears is kept or counted.
    pub fn for_waits(config: SecondConfig) -> io::Result<Self> {
        Self::named("wait words", false, 1, None, None, move || {
            let config = config.clone();
            async move {
                SttStream::connect(&config)
                    .await
                    .map_err(|error| error.to_string())
            }
        })
    }

    /// The same, with streams from `connect`.
    fn with<C, F>(
        always: bool,
        record: Option<std::path::PathBuf>,
        counts: Option<std::path::PathBuf>,
        connect: C,
    ) -> io::Result<Self>
    where
        C: Fn() -> F + Send + 'static,
        F: Future<Output = Result<SttStream, String>> + Send + 'static,
    {
        let most_ready = if always { 1 } else { MOST_READY_IN_YIELDS };
        Self::named("stop words", always, most_ready, record, counts, connect)
    }

    /// [`Self::with`], its thread and its journal lines called `name`,
    /// keeping up to `most_ready` streams connected while a reply is under
    /// way.
    fn named<C, F>(
        name: &'static str,
        always: bool,
        most_ready: usize,
        record: Option<std::path::PathBuf>,
        counts: Option<std::path::PathBuf>,
        connect: C,
    ) -> io::Result<Self>
    where
        C: Fn() -> F + Send + 'static,
        F: Future<Output = Result<SttStream, String>> + Send + 'static,
    {
        let (commands, queued) = queue::channel(MOST_QUEUED);
        let (said, heard) = mpsc::channel();
        let (end, ended) = mpsc::channel();
        let live = Arc::new(Live::new());
        let shared = Arc::clone(&live);
        std::thread::Builder::new()
            .name(format!("lamp-voice-{}", name.replace(' ', "-")))
            .spawn(move || {
                match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    // Its streams and their tasks go with the runtime.
                    Ok(runtime) => runtime.block_on(serve(
                        name, connect, always, most_ready, record, counts, queued, said, &shared,
                    )),
                    Err(error) => {
                        eprintln!("lamp-voice: {name} cannot be heard: {error}");
                    }
                }
                #[cfg(test)]
                shared.over.store(true, std::sync::atomic::Ordering::SeqCst);
                let _ = end.send(());
            })?;
        Ok(Self {
            commands,
            heard,
            live,
            ended,
            marks: 0,
        })
    }

    fn send(&self, command: Command) {
        let _ = self.commands.try_send(command);
    }

    fn drain(&self) {
        while self.heard.try_recv().is_ok() {}
    }
}

impl Ears for RelayEars {
    fn ready(&mut self) {
        self.send(Command::Ready);
    }
    fn hear(&mut self, captured_at_us: u64, samples: &[i16]) {
        self.send(Command::Hear {
            of: self.live.now(),
            captured_at_us,
            samples: samples.to_vec(),
        });
    }
    fn finalize(&mut self) {
        self.send(Command::Finalize {
            of: self.live.now(),
        });
    }
    fn flush(&mut self) {
        self.send(Command::Flush {
            of: self.live.now(),
        });
    }
    fn words(&mut self) -> Option<String> {
        self.result().map(|said| said.words)
    }
    fn result(&mut self) -> Option<Said> {
        let (live, marks) = (self.live.now(), self.marks);
        // Words of a yield that is over are not this one's (the
        // reviewer's P2 on #970). A result classified before this yield's
        // mark reached the thread decides nothing, however it was
        // classified then (reviewer-two at 11:11 on #3200).
        std::iter::from_fn(|| self.heard.try_recv().ok())
            .find(|(of, _, _)| *of == live)
            .map(|(_, said, marked)| Said {
                decides: said.decides && marked == marks,
                ..said
            })
    }
    fn mark(&mut self) {
        self.marks += 1;
        self.send(Command::Mark { of: self.marks });
    }
    fn forget(&mut self) {
        self.live.revoke();
        self.drain();
    }
    fn rest(&mut self) {
        self.live.revoke();
        self.send(Command::Rest);
        self.drain();
    }
}

impl Drop for RelayEars {
    /// On every way out of the conversation, the mute's and the privacy
    /// loss's returns among them: nothing more goes, and the thread is
    /// given a moment to end.
    fn drop(&mut self) {
        self.live.close();
        let _ = self.ended.recv_timeout(DROP_WAIT);
    }
}

/// Blocks handed to a stream, blocks that could not be, and how many times
/// a stream ended during the reply and another was asked for.
#[derive(Default)]
struct Handed {
    taken: usize,
    dropped: usize,
    reopened: usize,
}

/// Where the coordinator reads where to write each reply's last 10 s of
/// stream audio, as sent, for `lamp-voice stt-check` (#1047, the first
/// builder at 20:58). An absolute path, written over at each reply's end.
/// Unset: nothing is kept.
pub const AUDIO_ENVIRONMENT: &str = "LAMP_STOP_WORDS_AUDIO";

/// With [`Listen::Always`], where each reply's counts are written at its
/// end, as JSON, written over each reply: what the stop words sent and got
/// back, what the stream heard and whether it stopped the lamp, and the
/// stream's own counts. An absolute path, off unless set. On lamp-4ace on
/// 17 October the journal could not be read after midnight's log sync, and
/// whether a stop came from the stop words or the duplex yield could not be
/// told (the first builder on #1047). It holds what was said over the
/// lamp's replies: for played tests.
pub const COUNTS_ENVIRONMENT: &str = "LAMP_STOP_WORDS_COUNTS";

fn counts_path() -> Option<std::path::PathBuf> {
    std::env::var_os(COUNTS_ENVIRONMENT)
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// What a reply heard: its last this many results, and as many again of
/// the earliest whose words hold a stop phrase, however early (Fourth and
/// the first builder on #828, 17 Oct 13:18 and 13:42): a cut-in 9 s into a
/// story fell after the first 16, so whether its stop came back could not
/// be read.
const MOST_HEARD: usize = 16;

/// One result kept for the counts: its number in the reply (from 1), its
/// words, whether it stopped the lamp, and whether its words hold a stop
/// phrase.
#[derive(Clone, Debug, PartialEq)]
struct Kept {
    n: usize,
    text: String,
    stopped: bool,
    says_stop: bool,
}

/// Adds `result` and lets go of what is no longer kept: a result is kept
/// while it is among the last [`MOST_HEARD`], or while its words hold a
/// stop phrase and it is one of the first [`MOST_HEARD`] that do. In the
/// order heard.
fn keep_heard(kept: &mut Vec<Kept>, result: Kept) {
    let newest = result.n;
    kept.push(result);
    let mut stops = 0;
    kept.retain(|each| {
        let recent = newest - each.n < MOST_HEARD;
        let pinned = each.says_stop && {
            stops += 1;
            stops <= MOST_HEARD
        };
        recent || pinned
    });
}
/// Each result's words, at most this many bytes of them.
const HEARD_BYTES: usize = 200;

/// `value` written to `path` whole: through a file beside it, renamed.
fn write_json(path: &std::path::Path, value: &serde_json::Value) -> io::Result<()> {
    let beside = path.with_extension("json.part");
    std::fs::write(&beside, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&beside, path)
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis().try_into().unwrap_or(u64::MAX))
}

/// The most kept: 10 s at 16 kHz.
const MOST_KEPT: usize = 160_000;

/// `samples`, kept at the end of `kept` when `on`, the oldest let go past
/// [`MOST_KEPT`].
fn keep(kept: &mut VecDeque<i16>, on: bool, samples: &[i16]) {
    if !on {
        return;
    }
    kept.extend(samples.iter().copied());
    while kept.len() > MOST_KEPT {
        kept.pop_front();
    }
}

/// `kept` as a 16 kHz mono 16-bit WAV at `path`.
fn write_kept(path: &std::path::Path, kept: &VecDeque<i16>) -> Result<(), hound::Error> {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for sample in kept {
        writer.write_sample(*sample)?;
    }
    writer.finalize()
}

impl Handed {
    fn count(&mut self, taken: bool) {
        if taken {
            self.taken += 1;
        } else {
            self.dropped += 1;
        }
    }
}

/// The ready streams the relay has closed are dropped, and another is
/// asked for; one closed within [`FAST_CLOSE`] of connecting holds the
/// next connection off until [`RETRY`] from now.
fn drop_closed(
    name: &str,
    ready: &mut VecDeque<(tokio::time::Instant, SttStream)>,
    retry_at: &mut Option<tokio::time::Instant>,
) {
    ready.retain(|(connected, stream)| {
        if stream.still_open() {
            return true;
        }
        let fast = connected.elapsed() < FAST_CLOSE;
        eprintln!(
            "lamp-voice: {name}: a ready stream was closed {} ms after it connected; another is asked for{}",
            connected.elapsed().as_millis(),
            if fast {
                format!(" in {} s", RETRY.as_secs())
            } else {
                String::new()
            }
        );
        if fast {
            *retry_at = Some(tokio::time::Instant::now() + RETRY);
        }
        false
    });
}

/// The thread's loop: up to `most_ready` streams connected and kept ready
/// while a reply is under way, the yield's audio to one, and its words
/// back once asked for. Its journal lines are called `name`.
#[allow(clippy::too_many_arguments)]
async fn serve<C, F>(
    name: &str,
    connect: C,
    always: bool,
    most_ready: usize,
    record: Option<std::path::PathBuf>,
    counts: Option<std::path::PathBuf>,
    mut queued: queue::Receiver<Command>,
    said: mpsc::Sender<(u64, Said, u64)>,
    live: &Live,
) where
    C: Fn() -> F,
    F: Future<Output = Result<SttStream, String>> + Send + 'static,
{
    let mut wanted = false;
    // Oldest first, each with when it connected.
    let mut ready: VecDeque<(tokio::time::Instant, SttStream)> = VecDeque::new();
    // The stream a yield's audio goes to, and that yield; and when it
    // connected.
    let mut current: Option<(u64, SttStream)> = None;
    let mut current_connected = tokio::time::Instant::now();
    // With `always`, the current stream's words as they come, and its yield.
    let mut listening: Option<(u64, tokio::sync::mpsc::UnboundedReceiver<String>)> = None;
    // Beside it, whether each result is final and where it ends (#2939).
    let mut finals: Option<tokio::sync::mpsc::UnboundedReceiver<(bool, f64)>> = None;
    // On one clock, in samples, across the streams a reply opens and its
    // pending connections: the samples handed to them so far, where the
    // current stream's own audio began, where the yield in force began,
    // and that yield's number ([`Command::Mark`]). A position that is off
    // errs one way: the audio a stream holds back makes its results end
    // early, and blocks waiting at a mark that are dropped make it late,
    // so a result is at worst not the yield's and decides nothing
    // (reviewer-two at 11:11 and 11:15 on #3200).
    let mut total: u64 = 0;
    let mut base: u64 = 0;
    let mut mark: u64 = 0;
    let mut marked: u64 = 0;
    let mut waiting: VecDeque<(u64, u64, Vec<i16>)> = VecDeque::new();
    let mut finalize_waiting: Option<u64> = None;
    let mut connecting: Option<tokio::task::JoinHandle<Result<SttStream, String>>> = None;
    let mut finishing: Option<(u64, tokio::task::JoinHandle<Result<String, String>>)> = None;
    let mut retry_at: Option<tokio::time::Instant> = None;
    let mut told_failure = false;
    // When the connection in hand was asked for, for the journal.
    let mut asked_at = tokio::time::Instant::now();
    // With `always`, the blocks handed to the current stream and those
    // dropped (too old, or a backlog), for the journal at the reply's end.
    let mut handed = Handed::default();
    // With `LAMP_STOP_WORDS_AUDIO`: the last 10 s of what this reply's
    // streams were given, exactly as sent, for `stt-check`.
    let mut kept: VecDeque<i16> = VecDeque::new();
    // What the stream a revocation took away sent back, for the end-of-reply
    // line: a reply's end revokes before its `Rest` comes (#1047, the
    // first builder at 21:02: that line was never written).
    let mut last_replies: Option<std::sync::Arc<crate::second_provider::net::Replies>> = None;
    loop {
        // What a revocation took away is dropped before anything else.
        let number = live.now();
        if number == CLOSED {
            eprintln!("lamp-voice: {name}: the speech-to-text thread ends: the voice let it go");
            return;
        }
        if current.as_ref().is_some_and(|(of, _)| *of != number)
            && let Some((_, stream)) = current.take()
        {
            last_replies = Some(stream.replies());
        }
        if listening.as_ref().is_some_and(|(of, _)| *of != number) {
            listening = None;
        }
        if always
            && listening.is_none()
            && let Some((of, stream)) = current.as_mut()
        {
            listening = stream.take_words().map(|words| (*of, words));
            finals = stream.take_finals();
        }
        waiting.retain(|(of, _, _)| *of == number);
        finalize_waiting = finalize_waiting.filter(|of| *of == number);
        if finishing.as_ref().is_some_and(|(of, _)| *of != number)
            && let Some((_, task)) = finishing.take()
        {
            task.abort();
        }
        drop_closed(name, &mut ready, &mut retry_at);
        if wanted
            && ready.len() < most_ready
            && connecting.is_none()
            && retry_at.is_none_or(|at| tokio::time::Instant::now() >= at)
        {
            asked_at = tokio::time::Instant::now();
            connecting = Some(tokio::spawn(connect()));
        }
        tokio::select! {
            biased;
            () = live.revoked.notified() => {}
            command = queued.recv() => {
                #[cfg(test)]
                drop(live.pause.lock().unwrap_or_else(PoisonError::into_inner));
                match command {
                    None => {
                        eprintln!("lamp-voice: {name}: the speech-to-text thread ends: nothing more can be asked of it");
                        return;
                    }
                    Some(Command::Ready) => {
                        wanted = true;
                        kept.clear();
                    }
                    Some(Command::Hear { of, captured_at_us, samples }) => {
                        if of != live.now() {
                            continue;
                        }
                        if current.is_none() {
                            // One the relay has closed since is passed over.
                            drop_closed(name, &mut ready, &mut retry_at);
                            current = ready.pop_front().map(|(connected, stream)| {
                                current_connected = connected;
                                (of, stream)
                            });
                            base = total;
                        }
                        match &current {
                            Some((_, stream)) => {
                                for (of, at, block) in waiting.drain(..) {
                                    let taken = live.hand(of, at, &block, stream);
                                    handed.count(taken);
                                    total += if taken { block.len() as u64 } else { 0 };
                                    keep(&mut kept, record.is_some() && taken, &block);
                                }
                                let taken = live.hand(of, captured_at_us, &samples, stream);
                                handed.count(taken);
                                total += if taken { samples.len() as u64 } else { 0 };
                                keep(&mut kept, record.is_some() && taken, &samples);
                            }
                            None => {
                                if waiting.len() == MOST_WAITING {
                                    waiting.pop_front();
                                }
                                waiting.push_back((of, captured_at_us, samples));
                            }
                        }
                    }
                    Some(Command::Finalize { of }) => {
                        if of != live.now() {
                            continue;
                        }
                        match current.take() {
                            Some((of, stream)) => {
                                finishing = Some((of, tokio::spawn(async move {
                                    stream.finished().await.map_err(|error| error.to_string())
                                })));
                            }
                            None => finalize_waiting = Some(of),
                        }
                    }
                    // The stream asked for the words heard so far and kept
                    // open; none yet, nothing to ask.
                    Some(Command::Flush { of }) => {
                        if of == live.now()
                            && let Some((_, stream)) = current.as_ref()
                        {
                            let _ = stream.flush();
                        }
                    }
                    // Blocks still waiting for a stream go to it first.
                    Some(Command::Mark { of }) => {
                        let waited: usize = waiting.iter().map(|(_, _, block)| block.len()).sum();
                        mark = total + waited as u64;
                        marked = of;
                    }
                    Some(Command::Rest) => {
                        if always {
                            let replies = current
                                .as_ref()
                                .map(|(_, stream)| stream.replies())
                                .or_else(|| last_replies.take());
                            eprintln!(
                                "lamp-voice: stop words: the stream took {} blocks and dropped {} (too old, or a backlog), and was opened again {} times; {}",
                                handed.taken,
                                handed.dropped,
                                handed.reopened,
                                replies.as_ref().map_or_else(
                                    || "no stream was open".to_owned(),
                                    |replies| format!("the last sent back {}", replies.told())
                                )
                            );
                            // Beside the reply's own counts, written just
                            // before it rested the stream.
                            if let Some(path) = counts.as_deref() {
                                let mut value = std::fs::read(path)
                                    .ok()
                                    .and_then(|bytes| {
                                        serde_json::from_slice::<serde_json::Value>(&bytes).ok()
                                    })
                                    .filter(serde_json::Value::is_object)
                                    .unwrap_or_else(|| serde_json::json!({}));
                                value["stream"] = serde_json::json!({
                                    "blocks_taken": handed.taken,
                                    "blocks_dropped": handed.dropped,
                                    "opened_again": handed.reopened,
                                    "last": replies.as_ref().map(|replies| replies.counts()),
                                });
                                if let Err(error) = write_json(path, &value) {
                                    eprintln!(
                                        "lamp-voice: stop words: the stream's counts could not be written to {}: {error}",
                                        path.display()
                                    );
                                }
                            }
                        }
                        if let Some(path) = record.as_deref()
                            && !kept.is_empty()
                        {
                            match write_kept(path, &kept) {
                                Ok(()) => eprintln!(
                                    "lamp-voice: stop words: the reply's last {:.1} s of stream audio are in {}",
                                    kept.len() as f64 / 16_000.0,
                                    path.display()
                                ),
                                Err(error) => eprintln!(
                                    "lamp-voice: stop words: the reply's stream audio could not be written to {}: {error}",
                                    path.display()
                                ),
                            }
                        }
                        kept.clear();
                        last_replies = None;
                        handed = Handed::default();
                        wanted = false;
                        ready.clear();
                        current = None;
                        listening = None;
                        waiting.clear();
                        finalize_waiting = None;
                        if let Some(task) = connecting.take() {
                            task.abort();
                        }
                        if let Some((_, task)) = finishing.take() {
                            task.abort();
                        }
                    }
                }
            },
            connected = async { connecting.as_mut().expect("guarded").await }, if connecting.is_some() => {
                connecting = None;
                match connected {
                    Ok(Ok(stream)) => {
                        eprintln!(
                            "lamp-voice: {name}: a stream is ready, connected in {} ms{}",
                            asked_at.elapsed().as_millis(),
                            if told_failure { ", the speech-to-text reached again" } else { "" }
                        );
                        told_failure = false;
                        retry_at = None;
                        let number = live.now();
                        if current.is_none() && (!waiting.is_empty() || finalize_waiting.is_some()) {
                            base = total;
                            for (of, at, block) in waiting.drain(..) {
                                let taken = live.hand(of, at, &block, &stream);
                                handed.count(taken);
                                total += if taken { block.len() as u64 } else { 0 };
                                keep(&mut kept, record.is_some() && taken, &block);
                            }
                            match finalize_waiting.take() {
                                Some(of) => {
                                    finishing = Some((of, tokio::spawn(async move {
                                        stream.finished().await.map_err(|error| error.to_string())
                                    })));
                                }
                                None => {
                                    current_connected = tokio::time::Instant::now();
                                    current = Some((number, stream));
                                }
                            }
                        } else {
                            ready.push_back((tokio::time::Instant::now(), stream));
                        }
                    }
                    Ok(Err(error)) => {
                        if !told_failure {
                            eprintln!("lamp-voice: {name}: the speech-to-text cannot be reached, retried every {} s: {error}", RETRY.as_secs());
                            told_failure = true;
                        }
                        retry_at = Some(tokio::time::Instant::now() + RETRY);
                    }
                    Err(_) => {}
                }
            },
            heard = async { listening.as_mut().expect("guarded").1.recv().await }, if listening.is_some() => {
                match (heard, listening.as_ref()) {
                    (Some(words), Some((of, _))) => {
                        // Without a final's account, it decides nothing.
                        let decides = finals
                            .as_mut()
                            .and_then(|finals| finals.try_recv().ok())
                            .is_some_and(|(is_final, end)| {
                                is_final && base + (end * 16_000.0) as u64 > mark
                            });
                        let _ = said.send((*of, Said { words, decides }, marked));
                    }
                    // The stream ended during the reply (#1047, the first
                    // builder at 20:58: nothing came back after the first
                    // pauses): it is let go and another asked for, and what
                    // is heard meanwhile waits for it.
                    _ => {
                        listening = None;
                        if always && let Some((_, stream)) = current.take() {
                            handed.reopened += 1;
                            if current_connected.elapsed() < FAST_CLOSE {
                                retry_at = Some(tokio::time::Instant::now() + RETRY);
                            }
                            eprintln!(
                                "lamp-voice: {name}: the stream ended during the reply after it sent back {}; another is asked for",
                                stream.replies().told()
                            );
                        }
                    }
                }
            },
            finished = async { (&mut finishing.as_mut().expect("guarded").1).await }, if finishing.is_some() => {
                let Some((of, _)) = finishing.take() else {
                    continue;
                };
                if let Ok(Ok(words)) = finished {
                    #[cfg(test)]
                    let _held = {
                        live.holding_words.store(true, std::sync::atomic::Ordering::SeqCst);
                        live.hold_words.lock().unwrap_or_else(PoisonError::into_inner)
                    };
                    let _ = said.send((of, Said { words, decides: true }, marked));
                }
            },
            _ = tokio::time::sleep_until(retry_at.unwrap_or_else(tokio::time::Instant::now)), if retry_at.is_some() && connecting.is_none() => {
                retry_at = None;
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #2939, `LAMP_BACKCHANNEL_DEBUG`: off by default nothing is
    /// journaled; on, at most two words, lower case without punctuation,
    /// with why they were decided so; never more words.
    #[test]
    fn backchannel_debug_journals_at_most_two_words_and_only_when_asked() {
        let quiet = StopWords::new(Box::new(Scripted(Arc::default())));
        assert_eq!(quiet.debug_words("Right."), None);
        let mut asked =
            StopWords::new(Box::new(Scripted(Arc::default()))).with_backchannel_debug(true);
        let line = |words: &StopWords, heard: &str| words.debug_words(heard);
        for (heard, expected) in [
            ("Right.", "\"right\": on the list"),
            ("Uh\u{2011}huh!", "\"uhhuh\": on the list"),
            ("Rite.", "\"rite\": not on the list"),
            ("Oh.", "\"oh\": leads-only"),
            ("Yeah?", "\"yeah\": asked as a question"),
            ("Wait, wait.", "\"wait wait\": a stop phrase"),
            ("...", "\"\": no words"),
        ] {
            assert_eq!(
                line(&asked, heard),
                Some(format!(
                    "lamp-voice: backchannel debug: a yield's words {expected}"
                )),
                "{heard}"
            );
        }
        assert_eq!(line(&asked, "Tell me a story."), None);
        asked.lamp_asked = true;
        assert_eq!(
            line(&asked, "Yes."),
            Some(
                "lamp-voice: backchannel debug: a yield's words \"yes\": on the list, but the lamp asked a question"
                    .into()
            )
        );
    }

    /// #2939: a final of only punctuation is no words, as an empty one.
    #[test]
    fn punctuation_alone_counts_no_words() {
        assert_eq!(word_count("..."), 0);
        assert_eq!(word_count(" - "), 0);
        assert_eq!(word_count("Uh-huh, okay."), 2);
    }

    /// #1047 and #828: `raw` and `linear` as written; anything else, and
    /// unset, hears the canceller's output as before.
    #[test]
    fn the_source_is_raw_or_linear_only_when_asked() {
        assert_eq!(source_for(Some("raw")), Source::Raw);
        assert_eq!(source_for(Some("linear")), Source::Linear);
        for value in [
            None,
            Some(""),
            Some("processed"),
            Some("RAW"),
            Some("Linear"),
            Some("aec_linear"),
            Some("mic1"),
        ] {
            assert_eq!(source_for(value), Source::Processed, "{value:?}");
        }
        for source in [Source::Processed, Source::Raw, Source::Linear] {
            assert_eq!(source_for(Some(source.as_str())), source);
        }
    }
    use std::sync::{Arc, Mutex};

    /// #2939: one or two words, all on the language's list, no "?", no
    /// stop phrase.
    #[test]
    fn backchannels_are_short_acknowledgments_and_nothing_else() {
        for (heard, vietnamese, is) in [
            ("Mm-hm.", false, true),
            ("Uh-huh.", false, true),
            ("Yeah.", false, true),
            ("Oh, okay.", false, true),
            ("MHM", false, true),
            ("Sure", false, true),
            // A pair, not two words of the list (#2939, 23 Oct on lamp-52e6).
            ("I see.", false, true),
            ("I see?", false, false),
            ("All right.", false, true),
            ("Oh, I see.", false, true),
            ("Okay, sure.", false, true),
            ("Uh huh.", false, true),
            ("Mm hm.", false, true),
            ("Got it.", false, true),
            ("I see, I see, I see.", false, false),
            ("Got", false, false),
            ("All", false, false),
            ("I", false, false),
            ("See.", false, false),
            ("Ừ.", true, true),
            ("Dạ, vâng.", true, true),
            // "Ừ" decomposed: U+0055 U+031B U+0300, read as NFC.
            ("U\u{31b}\u{300}.", true, true),
            ("Stop.", false, false),
            ("Wait, I", false, false),
            ("Okay, stop.", false, false),
            ("What was that?", false, false),
            ("Why?", false, false),
            ("Yeah?", false, false),
            ("Uh", false, false),
            ("So", false, false),
            // Alone they may open a turn; before another they lead it.
            ("Oh.", false, false),
            ("Hmm.", false, false),
            ("Oh, oh.", false, false),
            ("Hmm, okay.", false, true),
            // The speech-to-text's spellings, any dash (#2939).
            ("Mhmm.", false, true),
            ("Uh\u{2011}huh.", false, true),
            ("Mm\u{2013}hm.", false, true),
            ("Uh-uh.", false, false),
            ("No.", false, false),
            ("Yeah, yeah, yeah.", false, false),
            ("", false, false),
            // Each list for its own language.
            ("Ừ.", false, false),
            ("Yeah.", true, false),
            ("Hả?", true, false),
            // As often a reply as an acknowledgment (#2959's P3).
            ("Rồi.", true, false),
            ("Đúng.", true, false),
        ] {
            assert_eq!(backchannel(heard, vietnamese, false), is, "{heard:?}");
        }
    }

    /// #2939, 25 Oct: with `LAMP_ASIDES_HOLD=on`, a short aside is a backchannel,
    /// alone or two together; a question, a stop, "wait", "hold on" or a
    /// longer line is not. Off, the asides are not.
    #[test]
    fn asides_are_backchannels_only_with_the_switch() {
        for heard in [
            "Hmm, interesting.",
            "Interesting.",
            "Hmm.",
            "Wow.",
            "Oh, nice.",
            "Nice.",
            "Wow, okay.",
            "Uh-huh.",
            "Yeah, right.",
        ] {
            assert!(backchannel(heard, false, true), "{heard:?} on");
        }
        for heard in [
            "Hmm, interesting.",
            "Interesting.",
            "Hmm.",
            "Wow.",
            "Oh, nice.",
        ] {
            assert!(!backchannel(heard, false, false), "{heard:?} off");
        }
        for heard in [
            "Interesting?",
            "Wow, stop.",
            "Wait.",
            "Hold on.",
            "Hmm, wait.",
            "That's interesting, tell me more.",
            "Wow, wow, wow.",
            "Oh.",
        ] {
            assert!(!backchannel(heard, false, true), "{heard:?} on");
        }
        // Vietnamese keeps its own list.
        assert!(!backchannel("Wow.", true, true));
    }

    /// #2959: whether the lamp's last finished sentence was a question.
    #[test]
    fn the_lamp_asked_when_its_last_finished_sentence_is_a_question() {
        for (said, asked) in [
            ("Shall I go on?", true),
            ("It rained. Shall I go on?  ", true),
            ("Shall I go on? Well", true),
            ("Bạn có muốn nghe tiếp không？", true),
            ("It rained all day.", false),
            ("Shall I go on? Okay. Then", false),
            ("Once upon a time", false),
            ("", false),
        ] {
            assert_eq!(lamp_asked(said), asked, "{said:?}");
        }
    }

    #[test]
    fn stop_phrases_are_heard_alone_and_among_other_words() {
        for stop in [
            "Stop.",
            "Wait.",
            "Wait, wait, wait.",
            "Okay, okay, that is enough.",
            "Lamp, stop.",
            "Okay, stop.",
            "Hold on.",
            "Hang on, I have a question.",
            "Pause.",
            "Quiet, please.",
        ] {
            assert!(stop_phrase(stop), "{stop}");
        }
        for not in [
            "Mm-hm.",
            "Yeah.",
            "Wow.",
            "Tell me a joke instead.",
            "Hang",
            "On the other hand.",
            "",
            // Turned round (the reviewer on #970).
            "Don't stop.",
            "No, don't stop.",
            "Don\u{2019}t stop, I love this part.",
            "Never stop.",
        ] {
            assert!(!stop_phrase(not), "{not}");
        }
        // A stop word after one that is turned round still counts.
        assert!(stop_phrase("Don't worry, stop."));
    }

    /// The first builder on #2602 (21 Oct 23:44): the stop stream's key
    /// terms are the stop words, first, only with `LAMP_STOP_WORDS_KEYTERM`;
    /// the unit's own follow, each once, within the bound.
    #[test]
    fn the_stop_stream_asks_for_the_stop_words_only_with_the_switch() {
        let own = vec!["Hanoi".to_owned(), "stop".to_owned()];
        assert_eq!(stop_key_terms(own.clone(), false), own);
        let asked = stop_key_terms(own, true);
        assert_eq!(&asked[..KEY_TERMS.len()], KEY_TERMS);
        assert_eq!(&asked[KEY_TERMS.len()..], ["Hanoi"], "\"stop\" once");
        let many = (0..40).map(|n| format!("w{n}")).collect();
        assert_eq!(
            stop_key_terms(many, true).len(),
            crate::second_config::MOST_KEY_TERMS
        );
    }

    /// A makeup gain is a number of dB over 0 and at most 24, clips at full
    /// scale, and raises only the canceller's output, never the raw or the
    /// linear blocks.
    #[test]
    fn the_gain_is_bounded_and_raises_only_the_cancellers_output() {
        assert_eq!(gain_for(Some("12")), Some(12.0));
        assert_eq!(gain_for(Some(" 6.5 ")), Some(6.5));
        for bad in [
            None,
            Some("0"),
            Some("-6"),
            Some("24.5"),
            Some("inf"),
            Some("NaN"),
            Some("x"),
        ] {
            assert_eq!(gain_for(bad), None, "{bad:?}");
        }
        assert_eq!(
            raised(&[100, -100, 20_000, -20_000], 20.0),
            [1_000, -1_000, i16::MAX, i16::MIN]
        );
        // Heard in a yield, the blocks are the capture's chosen source: raw or
        // linear blocks are never raised (El's P2 on #2690).
        assert_eq!(yield_gain(Some(12.0), Source::Processed), Some(12.0));
        assert_eq!(yield_gain(Some(12.0), Source::Raw), None);
        assert_eq!(yield_gain(Some(12.0), Source::Linear), None);
        assert_eq!(yield_gain(None, Source::Processed), None);
        for (source, raised_to) in [
            (Source::Processed, 1_000),
            (Source::Linear, 100),
            (Source::Raw, 100),
        ] {
            let script = Arc::new(Mutex::new(Script::default()));
            let mut words = StopWords::new(Box::new(Scripted(script.clone())))
                .with_source(source)
                .with_gain(Some(20.0));
            words.block(true, true, true, None, None, 0, &block(100));
            assert!(
                script
                    .lock()
                    .unwrap()
                    .told
                    .contains(&format!("hear {raised_to}")),
                "{source:?}: {:?}",
                script.lock().unwrap().told
            );
        }
    }

    /// What the scripted ears were told, and the words they give.
    #[derive(Default)]
    struct Script {
        told: Vec<String>,
        words: Option<String>,
    }

    struct Scripted(Arc<Mutex<Script>>);

    impl Ears for Scripted {
        fn ready(&mut self) {
            self.0.lock().unwrap().told.push("ready".into());
        }
        fn hear(&mut self, _: u64, samples: &[i16]) {
            self.0
                .lock()
                .unwrap()
                .told
                .push(format!("hear {}", samples[0]));
        }
        fn finalize(&mut self) {
            self.0.lock().unwrap().told.push("finalize".into());
        }
        fn flush(&mut self) {
            self.0.lock().unwrap().told.push("flush".into());
        }
        fn words(&mut self) -> Option<String> {
            self.0.lock().unwrap().words.take()
        }
        fn forget(&mut self) {
            self.0.lock().unwrap().told.push("forget".into());
        }
        fn rest(&mut self) {
            self.0.lock().unwrap().told.push("rest".into());
        }
    }

    fn block(n: i16) -> [i16; 160] {
        [n; 160]
    }

    fn observed(n: i16) -> crate::activity::ObservedAudio {
        crate::activity::ObservedAudio {
            sequence: n as u64,
            captured_at_us: n as u64 * 10_000,
            samples: block(n),
        }
    }

    /// A reply; a yield with three blocks held; two blocks while paused and
    /// then stillness, its words asked for at the tenth still block and
    /// taken once; then the yield ends and the reply with it.
    #[test]
    fn a_yield_is_heard_from_its_held_audio_asked_once_and_forgotten() {
        let script = Arc::new(Mutex::new(Script::default()));
        let mut words = StopWords::new(Box::new(Scripted(Arc::clone(&script))));
        assert_eq!(
            words.block(true, true, false, None, None, 0, &block(1)),
            Tell::Nothing
        );
        let held = [observed(2), observed(3), observed(4)];
        assert_eq!(
            words.block(true, true, true, Some(0), Some(&held), 0, &block(4)),
            Tell::Await
        );
        assert_eq!(
            words.block(true, true, true, Some(0), None, 0, &block(5)),
            Tell::Nothing
        );
        for still in 1..=10 {
            assert_eq!(
                words.block(true, true, true, Some(still), None, 0, &block(6)),
                Tell::Nothing
            );
        }
        script.lock().unwrap().words = Some("Wait, wait.".into());
        assert_eq!(
            words.block(true, true, true, Some(11), None, 0, &block(7)),
            Tell::Words {
                stop: true,
                count: 2,
                backchannel: false
            }
        );
        script.lock().unwrap().words = Some("Stop.".into());
        assert_eq!(
            words.block(true, true, true, Some(12), None, 0, &block(7)),
            Tell::Nothing
        );
        assert_eq!(
            words.block(true, true, false, None, None, 0, &block(8)),
            Tell::Nothing
        );
        assert_eq!(
            words.block(false, false, false, None, None, 0, &block(9)),
            Tell::Nothing
        );
        let told = script.lock().unwrap().told.clone();
        let mut expected = vec!["ready", "hear 2", "hear 3", "hear 4", "hear 5"];
        expected.extend(["hear 6"; 10]);
        expected.extend(["finalize", "forget", "rest"]);
        assert_eq!(told, expected);
    }

    /// #1047, the first builder's `stt-check` at 16:59: through the relay a
    /// short "Stop." gets no interim result, and its words come only once
    /// `Finalize` asks for them. Listening to everything, the words are
    /// asked for after each 200 ms pause that follows speech, and the
    /// stream goes on; never after a pause with no speech before it.
    #[test]
    fn listening_always_asks_for_the_words_at_each_pause_after_speech() {
        let script = Arc::new(Mutex::new(Script::default()));
        let mut words = StopWords::always(Box::new(Scripted(Arc::clone(&script))));
        let mut at = 0u64;
        let mut heard = |words: &mut StopWords, speech: bool, blocks: u64| {
            for _ in 0..blocks {
                at += 10_000;
                words.heard_speech(speech);
                words.block(true, true, false, None, None, at, &block(1));
            }
        };
        let flushes = |script: &Arc<Mutex<Script>>| {
            script
                .lock()
                .unwrap()
                .told
                .iter()
                .filter(|told| *told == "flush")
                .count()
        };
        // Quiet from the start: nothing asked.
        heard(&mut words, false, 50);
        assert_eq!(flushes(&script), 0);
        // "Stop.", then 190 ms still: not yet; at 200 ms, once.
        heard(&mut words, true, 30);
        heard(&mut words, false, u64::from(FLUSH_AFTER_STILL_BLOCKS) - 1);
        assert_eq!(flushes(&script), 0);
        heard(&mut words, false, 1);
        assert_eq!(flushes(&script), 1);
        // The stillness going on asks nothing more; the next speech and
        // pause do.
        heard(&mut words, false, 100);
        assert_eq!(flushes(&script), 1);
        heard(&mut words, true, 10);
        heard(&mut words, false, u64::from(FLUSH_AFTER_STILL_BLOCKS));
        assert_eq!(flushes(&script), 2);
        assert!(!script.lock().unwrap().told.contains(&"rest".to_owned()));
    }

    /// #1047, the first builder at 02:57: with the raw blocks the words are
    /// asked for every half second while the lamp sounds, though the
    /// detector hears no speech over it; a pause after speech asks too, and
    /// starts the half second again. Nothing is counted while nothing is
    /// sent, and without the cadence quiet asks nothing.
    #[test]
    fn with_the_raw_blocks_the_words_are_asked_for_every_half_second() {
        let flushes = |script: &Arc<Mutex<Script>>| {
            script
                .lock()
                .unwrap()
                .told
                .iter()
                .filter(|told| *told == "flush")
                .count()
        };
        let script = Arc::new(Mutex::new(Script::default()));
        let mut words =
            StopWords::always(Box::new(Scripted(Arc::clone(&script)))).with_cadence(true);
        let mut at = 0u64;
        let mut heard = |words: &mut StopWords, sounding: bool, speech: bool, blocks: u16| {
            for _ in 0..blocks {
                at += 10_000;
                words.heard_speech(speech);
                words.block(true, sounding, false, None, None, at, &block(1));
            }
        };
        heard(&mut words, true, false, CADENCE_BLOCKS - 1);
        assert_eq!(flushes(&script), 0);
        heard(&mut words, true, false, 1);
        assert_eq!(flushes(&script), 1);
        heard(&mut words, true, false, CADENCE_BLOCKS);
        assert_eq!(flushes(&script), 2);
        // The lamp silent and no yield: nothing sent, nothing counted.
        heard(&mut words, false, false, 3 * CADENCE_BLOCKS);
        assert_eq!(flushes(&script), 2);
        // Speech and the 0.2 s pause after it ask, and the half second
        // starts again from there.
        heard(&mut words, true, true, 10);
        heard(&mut words, true, false, u16::from(FLUSH_AFTER_STILL_BLOCKS));
        assert_eq!(flushes(&script), 3);
        heard(&mut words, true, false, CADENCE_BLOCKS - 1);
        assert_eq!(flushes(&script), 3);
        heard(&mut words, true, false, 1);
        assert_eq!(flushes(&script), 4);

        let plain = Arc::new(Mutex::new(Script::default()));
        let mut words = StopWords::always(Box::new(Scripted(Arc::clone(&plain))));
        for step in 0..u64::from(4 * CADENCE_BLOCKS) {
            words.heard_speech(false);
            words.block(true, true, false, None, None, step * 10_000, &block(1));
        }
        assert_eq!(flushes(&plain), 0);
        // Only listening to everything has it.
        let yields = StopWords::new(Box::new(Scripted(Arc::new(Mutex::new(Script::default())))))
            .with_cadence(true);
        assert!(!yields.cadence);
    }

    /// #828: the raw and the linear blocks are asked for on the cadence,
    /// the canceller's output is not, and each reply's counts say which
    /// was heard.
    #[test]
    fn the_source_sets_the_cadence_and_is_in_each_replys_counts() {
        for (source, cadence) in [
            (Source::Processed, false),
            (Source::Raw, true),
            (Source::Linear, true),
        ] {
            let words =
                StopWords::always(Box::new(Scripted(Arc::new(Mutex::new(Script::default())))))
                    .with_source(source);
            assert_eq!((words.source, words.cadence), (source, cadence));
        }
        let dir =
            std::env::temp_dir().join(format!("lamp-voice-stop-source-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stop-words.json");
        let mut words = StopWords::always_counted(
            Box::new(Scripted(Arc::new(Mutex::new(Script::default())))),
            Some(path.clone()),
        )
        .with_source(Source::Linear);
        let mut at = 0u64;
        for _ in 0..3 {
            at += 10_000;
            words.block(true, true, false, None, None, at, &block(1));
        }
        words.block(false, false, false, None, None, at + 10_000, &block(1));
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["reply"]["source"], "linear", "{written}");
        assert_eq!(written["reply"]["asked_every_ms"], 500, "{written}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #1047, the first builder at 02:05: with the journal unreadable on
    /// the lamp, each reply's counts go to a file at its end: what was
    /// sent and asked for, what the stream heard and whether it stopped
    /// the lamp. The stream's own counts are added beside them.
    #[test]
    fn listening_always_writes_each_replys_counts_to_a_file() {
        let dir =
            std::env::temp_dir().join(format!("lamp-voice-stop-counts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stop-words.json");
        let script = Arc::new(Mutex::new(Script::default()));
        let mut words =
            StopWords::always_counted(Box::new(Scripted(Arc::clone(&script))), Some(path.clone()));
        words.lamp_said("Once upon a time there was a fox.", 0, false);
        let mut at = 0u64;
        for _ in 0..30 {
            at += 10_000;
            words.heard_speech(true);
            words.block(true, true, false, None, None, at, &block(1));
        }
        script.lock().unwrap().words = Some("Stop.".into());
        at += 10_000;
        words.block(true, true, false, None, None, at, &block(1));
        assert!(words.spoken_stop());
        at += 10_000;
        words.block(false, false, false, None, None, at, &block(1));
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["reply"]["blocks_sent"], 31, "{written}");
        assert_eq!(written["reply"]["results"], 1, "{written}");
        assert_eq!(written["reply"]["stop_heard"], true, "{written}");
        assert_eq!(written["reply"]["source"], "processed", "{written}");
        assert_eq!(
            written["reply"]["heard"],
            serde_json::json!([{"n": 1, "text": "Stop.", "stopped_the_lamp": true}])
        );
        assert!(script.lock().unwrap().told.contains(&"rest".to_owned()));
        // Without a path, nothing is kept or written.
        let mut quiet =
            StopWords::always(Box::new(Scripted(Arc::new(Mutex::new(Script::default())))));
        assert!(quiet.counts.is_none() || std::env::var_os(COUNTS_ENVIRONMENT).is_some());
        quiet.block(true, true, false, None, None, 10_000, &block(1));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The reviewer on #1047 at 07:42: a stop heard over the lamp ends the
    /// reply through the hand's path, which closes the stream. Its counts
    /// were then never written, so `stopped_the_lamp: true` could never
    /// appear. Closing writes them as the reply's end does.
    #[test]
    fn a_stop_that_closes_the_reply_is_in_its_counts() {
        let dir =
            std::env::temp_dir().join(format!("lamp-voice-stop-closed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stop-words.json");
        let script = Arc::new(Mutex::new(Script::default()));
        let mut words =
            StopWords::always_counted(Box::new(Scripted(Arc::clone(&script))), Some(path.clone()));
        let mut at = 0u64;
        for _ in 0..30 {
            at += 10_000;
            words.heard_speech(true);
            words.block(true, true, false, None, None, at, &block(1));
        }
        script.lock().unwrap().words = Some("Stop.".into());
        at += 10_000;
        words.block(true, true, false, None, None, at, &block(1));
        assert!(words.spoken_stop());
        words.close();
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(written["reply"]["stop_heard"], true, "{written}");
        assert_eq!(
            written["reply"]["heard"],
            serde_json::json!([{"n": 1, "text": "Stop.", "stopped_the_lamp": true}])
        );
        assert!(script.lock().unwrap().told.contains(&"rest".to_owned()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Fourth and the first builder on #828 (17 Oct 13:18, 13:42): a
    /// stop said 9 s into a story came after the first 16 results, so the
    /// counts could not show it. They keep the last 16, and every earlier
    /// result whose words hold a stop phrase, each with its number.
    #[test]
    fn the_counts_keep_the_last_results_and_every_stop_said_however_early() {
        let mut kept = Vec::new();
        for n in 1..=40usize {
            let says_stop = n == 5 || n == 30;
            keep_heard(
                &mut kept,
                Kept {
                    n,
                    text: if says_stop {
                        "Stop.".into()
                    } else {
                        format!("once upon a time {n}")
                    },
                    stopped: n == 30,
                    says_stop,
                },
            );
        }
        let numbers: Vec<usize> = kept.iter().map(|k| k.n).collect();
        let mut wanted = vec![5];
        wanted.extend(25..=40);
        assert_eq!(numbers, wanted);
        assert!(kept.iter().find(|k| k.n == 30).unwrap().stopped);
        // A stream that says stop over and over keeps at most 16 of the
        // earliest beside the last 16.
        let mut kept = Vec::new();
        for n in 1..=100usize {
            keep_heard(
                &mut kept,
                Kept {
                    n,
                    text: "stop".into(),
                    stopped: false,
                    says_stop: true,
                },
            );
        }
        assert_eq!(kept.len(), 2 * MOST_HEARD);
        assert_eq!(kept.first().unwrap().n, 1);
        assert_eq!(kept.last().unwrap().n, 100);
    }

    /// Through the stream: twenty results of the lamp's story, a stop said
    /// as the sixth, and the file keeps it with its number.
    #[test]
    fn a_stop_said_before_the_last_sixteen_is_still_in_the_file() {
        let dir =
            std::env::temp_dir().join(format!("lamp-voice-stop-early-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stop-words.json");
        let script = Arc::new(Mutex::new(Script::default()));
        let mut words =
            StopWords::always_counted(Box::new(Scripted(Arc::clone(&script))), Some(path.clone()));
        let mut at = 0u64;
        for _ in 0..30 {
            at += 10_000;
            words.heard_speech(true);
            words.block(true, true, false, None, None, at, &block(1));
        }
        for n in 1..=24 {
            script.lock().unwrap().words = Some(if n == 6 {
                "Wait, stop.".into()
            } else {
                format!("the fox ran {n}")
            });
            at += 10_000;
            words.block(true, true, false, None, None, at, &block(1));
        }
        at += 10_000;
        words.block(false, false, false, None, None, at, &block(1));
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let heard = written["reply"]["heard"].as_array().unwrap();
        let numbers: Vec<u64> = heard.iter().map(|h| h["n"].as_u64().unwrap()).collect();
        assert_eq!(numbers[0], 6, "{written}");
        assert_eq!(heard[0]["text"], "Wait, stop.");
        assert_eq!(numbers.len(), 17, "{written}");
        assert_eq!(
            *numbers.last().unwrap(),
            written["reply"]["results"].as_u64().unwrap()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The stamp the coordinator gives the lamp's words: their text's
    /// arrival, plus the reply's audio queued ahead of them at 24 kHz (the
    /// reviewer on #1047 at 09:10: the sum had no test).
    #[test]
    fn the_lamps_words_play_after_the_audio_queued_ahead_of_them() {
        assert_eq!(plays_at_us(1_000_000, 0), 1_000_000);
        assert_eq!(plays_at_us(1_000_000, 240_000), 11_000_000);
        assert_eq!(plays_at_us(1_000_000, 24), 1_001_000);
        assert_eq!(plays_at_us(u64::MAX, 240_000), u64::MAX);
    }

    /// The reviewer on #1047 at 07:42: the lamp's words are its own while
    /// they are being heard. Their text can come 10 s or more before their
    /// sound, the voice holding up to 30 s of a reply ahead of playback, so
    /// the coordinator stamps them with when they will play. Stamped then,
    /// the story's own "wait" read back from the raw microphone at 12.5 s
    /// stops nothing; stamped as its text came, at 1 s, it stopped the lamp.
    #[test]
    fn the_lamps_own_words_count_from_when_they_play() {
        let stops = |stamped_at_us: u64| {
            let script = Arc::new(Mutex::new(Script::default()));
            let mut words = StopWords::always(Box::new(Scripted(Arc::clone(&script))));
            words.lamp_said("And so the fox had to wait.", stamped_at_us, true);
            let mut at = 12_000_000u64;
            for _ in 0..50 {
                at += 10_000;
                words.heard_speech(true);
                words.block(true, true, false, None, None, at, &block(1));
            }
            script.lock().unwrap().words = Some("the fox had to wait".into());
            words.block(true, true, false, None, None, at + 10_000, &block(1));
            words.spoken_stop()
        };
        assert!(!stops(11_000_000));
        assert!(stops(1_000_000));
    }

    /// A yield that ends before its words is said to be unheard. The
    /// microphone closing drops a yield and the stream at once, and only
    /// once, and the next reply has a stream ready again.
    #[test]
    fn a_yield_without_words_is_unheard_and_closing_drops_everything_once() {
        let script = Arc::new(Mutex::new(Script::default()));
        let mut words = StopWords::new(Box::new(Scripted(Arc::clone(&script))));
        words.block(true, true, false, None, None, 0, &block(1));
        assert_eq!(
            words.block(true, true, true, Some(0), None, 0, &block(2)),
            Tell::Await
        );
        assert_eq!(
            words.block(true, true, false, None, None, 0, &block(3)),
            Tell::Unheard
        );
        assert_eq!(
            words.block(true, true, true, Some(0), None, 0, &block(4)),
            Tell::Await
        );
        words.close();
        words.close();
        // The reviewer on #970: a hand Stop leaves the duplex rule's yield in
        // force; it is not begun again, and not called unheard when it ends.
        assert_eq!(
            words.block(true, true, true, Some(3), None, 0, &block(40)),
            Tell::Nothing
        );
        assert_eq!(
            words.block(true, true, false, None, None, 0, &block(5)),
            Tell::Nothing
        );
        let told = script.lock().unwrap().told.clone();
        assert_eq!(
            told,
            [
                "ready", "hear 2", "forget", "hear 4", "forget", "rest", "ready"
            ]
        );
        // The next yield is heard again.
        assert_eq!(
            words.block(true, true, true, Some(0), None, 0, &block(6)),
            Tell::Await
        );
    }

    /// A stand-in for the speech-to-text on an in-memory socket: it counts
    /// the audio bytes it is sent, and answers each stream's `Finalize`
    /// with the next of its words.
    struct Service {
        audio: Arc<std::sync::atomic::AtomicUsize>,
        connections: Arc<std::sync::atomic::AtomicUsize>,
        words: Arc<Mutex<VecDeque<&'static str>>>,
        /// Each stream's words are also given as an interim result once its
        /// first audio comes, as the service does while someone speaks.
        interim: bool,
        /// How long each connection takes, as the relay's 0.6 to 0.7 s.
        delay: Duration,
        /// Each stream is closed by the service as soon as it opens.
        closes_on_open: bool,
    }

    type Connected =
        std::pin::Pin<Box<dyn Future<Output = Result<SttStream, String>> + Send + 'static>>;

    impl Service {
        fn new(words: &[&'static str]) -> Self {
            Self {
                audio: Arc::default(),
                connections: Arc::default(),
                words: Arc::new(Mutex::new(words.iter().copied().collect())),
                interim: false,
                delay: Duration::ZERO,
                closes_on_open: false,
            }
        }

        fn interim(words: &[&'static str]) -> Self {
            Self {
                interim: true,
                ..Self::new(words)
            }
        }

        fn connect(&self) -> impl Fn() -> Connected + Send + 'static {
            let (audio, connections, words, interim, delay, closes_on_open) = (
                Arc::clone(&self.audio),
                Arc::clone(&self.connections),
                Arc::clone(&self.words),
                self.interim,
                self.delay,
                self.closes_on_open,
            );
            move || {
                use futures_util::{SinkExt, StreamExt};
                use tokio_tungstenite::{
                    WebSocketStream,
                    tungstenite::{Message, protocol::Role},
                };
                connections.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let audio = Arc::clone(&audio);
                let said = words.lock().unwrap().pop_front().unwrap_or("");
                Box::pin(async move {
                    tokio::time::sleep(delay).await;
                    let (near, far) = tokio::io::duplex(1 << 16);
                    tokio::spawn(async move {
                        let mut socket =
                            WebSocketStream::from_raw_socket(far, Role::Server, None).await;
                        if closes_on_open {
                            let _ = socket.close(None).await;
                            return;
                        }
                        let mut told = !interim;
                        while let Some(Ok(message)) = socket.next().await {
                            match message {
                                Message::Binary(bytes) => {
                                    audio.fetch_add(
                                        bytes.len(),
                                        std::sync::atomic::Ordering::SeqCst,
                                    );
                                    if !std::mem::replace(&mut told, true) {
                                        let results = format!(
                                            r#"{{"type":"Results","is_final":false,"speech_final":false,"start":0.0,"duration":0.5,"channel":{{"alternatives":[{{"transcript":"{said}"}}]}}}}"#
                                        );
                                        let _ = socket.send(Message::text(results)).await;
                                    }
                                }
                                Message::Text(text) if text.contains("Finalize") => {
                                    let results = format!(
                                        r#"{{"type":"Results","is_final":true,"speech_final":true,"from_finalize":true,"start":0.0,"duration":30.0,"channel":{{"alternatives":[{{"transcript":"{said}"}}]}}}}"#
                                    );
                                    let _ = socket.send(Message::text(results)).await;
                                    let _ = socket.close(None).await;
                                    break;
                                }
                                _ => {}
                            }
                        }
                    });
                    Ok(SttStream::on(
                        WebSocketStream::from_raw_socket(near, Role::Client, None).await,
                    ))
                }) as Connected
            }
        }

        fn audio(&self) -> usize {
            self.audio.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn connections(&self) -> usize {
            self.connections.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    fn until(what: &str, mut done: impl FnMut() -> bool) {
        let began = std::time::Instant::now();
        while !done() {
            assert!(began.elapsed() < Duration::from_secs(5), "{what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// The reviewer's P1 on #970, through the real thread: a yield's audio
    /// queued behind it while it is held never reaches the service once the
    /// yield is forgotten, the reply rested, or the ears dropped (as every
    /// way out of the conversation drops them, the mute's and the privacy
    /// loss's among them). After a forget, the next yield's audio goes. A
    /// block captured too long ago never goes.
    #[test]
    fn audio_queued_for_a_yield_never_goes_once_it_is_forgotten_rested_or_dropped() {
        for way in ["forget", "rest", "drop"] {
            let service = Service::new(&[]);
            let mut ears = Some(RelayEars::with(false, None, None, service.connect()).unwrap());
            let live = Arc::clone(&ears.as_ref().unwrap().live);
            ears.as_mut().unwrap().ready();
            until("the ready stream and a spare are asked for", || {
                service.connections() >= 2
            });
            let paused = live.pause.lock().unwrap();
            for _ in 0..50 {
                ears.as_mut()
                    .unwrap()
                    .hear(lamp_ipc::monotonic_us(), &[1; 160]);
            }
            match way {
                "forget" => ears.as_mut().unwrap().forget(),
                "rest" => ears.as_mut().unwrap().rest(),
                _ => drop(ears.take()),
            }
            drop(paused);
            std::thread::sleep(Duration::from_millis(200));
            assert_eq!(service.audio(), 0, "{way}");
            match ears.as_mut() {
                Some(ears) if way == "forget" => {
                    ears.hear(lamp_ipc::monotonic_us(), &[1; 160]);
                    until("the next yield's audio goes", || service.audio() == 320);
                }
                Some(_) => {}
                None => until("the thread has ended", || {
                    live.over.load(std::sync::atomic::Ordering::SeqCst)
                }),
            }
        }
        // Too old: captured 4 s ago, it never goes; the block after it does.
        let service = Service::new(&[]);
        let mut ears = RelayEars::with(false, None, None, service.connect()).unwrap();
        ears.live
            .fixture_clock
            .set((600_000_000, std::time::Instant::now()))
            .unwrap();
        ears.ready();
        until("the ready stream and a spare are asked for", || {
            service.connections() >= 2
        });
        ears.hear(ears.live.fixture_now_us() - 4_000_000, &[1; 160]);
        ears.hear(ears.live.fixture_now_us(), &[1; 160]);
        until("the fresh block goes", || service.audio() >= 320);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(service.audio(), 320);
    }

    /// The reviewer's P2 on #970, through the real thread: the words of a
    /// yield that come after it was forgotten, and before the next yield
    /// asks, are never taken for the next yield's; its own are.
    #[test]
    fn words_that_come_for_a_yield_after_it_is_forgotten_are_not_the_next_ones() {
        let service = Service::new(&["Stop.", "Mm-hm."]);
        let mut ears = RelayEars::with(false, None, None, service.connect()).unwrap();
        let live = Arc::clone(&ears.live);
        ears.ready();
        until("the ready stream and a spare are asked for", || {
            service.connections() >= 2
        });
        let held = live.hold_words.lock().unwrap();
        ears.hear(lamp_ipc::monotonic_us(), &[1; 160]);
        ears.finalize();
        until("the first yield's words have come", || {
            live.holding_words.load(std::sync::atomic::Ordering::SeqCst)
        });
        ears.forget();
        // The thread tells them only now, after the yield is over.
        drop(held);
        until("a spare for the next yield", || service.connections() >= 3);
        ears.hear(lamp_ipc::monotonic_us(), &[1; 160]);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(ears.words(), None);
        ears.finalize();
        let mut words = None;
        until("the next yield's own words", || {
            words = words.take().or_else(|| ears.words());
            words.is_some()
        });
        assert_eq!(words.as_deref(), Some("Mm-hm."));
    }

    /// lamp-52e6, 24 Oct 13:40: a yield began after the one before took
    /// the ready stream, and its words waited 666 ms for the next to
    /// connect. With a spare kept, two yields 300 ms apart each find one
    /// connected, though connecting takes 700 ms.
    #[test]
    fn two_yields_300_ms_apart_both_find_a_connected_stream() {
        let service = Service {
            delay: Duration::from_millis(700),
            ..Service::new(&["Stop.", "Wait."])
        };
        let mut ears = RelayEars::with(false, None, None, service.connect()).unwrap();
        ears.ready();
        until("the spare is asked for", || service.connections() >= 2);
        std::thread::sleep(Duration::from_millis(900));
        let mut heard = Vec::new();
        for _ in 0..2 {
            let began = std::time::Instant::now();
            ears.hear(lamp_ipc::monotonic_us(), &[1; 160]);
            ears.finalize();
            let mut words = None;
            until("the yield's words", || {
                words = words.take().or_else(|| ears.words());
                words.is_some()
            });
            // Under the 700 ms a connection takes, with room for a busy
            // test machine.
            assert!(
                began.elapsed() < Duration::from_millis(450),
                "the words waited {:?} for a stream",
                began.elapsed()
            );
            heard.push(words.unwrap());
            ears.forget();
            std::thread::sleep(Duration::from_millis(300).saturating_sub(began.elapsed()));
        }
        assert_eq!(heard, ["Stop.", "Wait."]);
    }

    /// A relay that closes each stream as it opens is not asked again and
    /// again: a stream closed within `FAST_CLOSE` of connecting holds the
    /// next connection off for `RETRY`, through a second of yields 10 ms
    /// apart, each looking for a ready stream.
    #[test]
    fn a_stream_the_relay_closes_on_open_is_not_asked_for_again_and_again() {
        let service = Service {
            closes_on_open: true,
            ..Service::new(&[])
        };
        let mut ears = RelayEars::with(false, None, None, service.connect()).unwrap();
        ears.ready();
        let began = std::time::Instant::now();
        while began.elapsed() < Duration::from_secs(1) {
            ears.hear(lamp_ipc::monotonic_us(), &[1; 160]);
            std::thread::sleep(Duration::from_millis(10));
            ears.forget();
        }
        assert!(
            service.connections() <= 3,
            "{} connections in a second",
            service.connections()
        );
        assert_eq!(service.audio(), 0);
    }

    /// Off, the coordinator builds no ears; built, they open nothing
    /// until a reply begins, and nothing more once it is over.
    #[test]
    fn no_stream_opens_with_the_switch_off_or_without_a_reply() {
        assert_eq!(listen_for(None), Listen::Off);
        let service = Service::new(&[]);
        let mut ears = RelayEars::with(false, None, None, service.connect()).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(service.connections(), 0);
        ears.ready();
        until("the ready stream and a spare", || {
            service.connections() >= 2
        });
        ears.rest();
        ears.hear(lamp_ipc::monotonic_us(), &[1; 160]);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(service.connections(), 2);
    }

    #[test]
    fn a_stop_word_the_lamp_did_not_say_itself_is_heard_over_it() {
        assert!(stop_over("Stop.", ""));
        assert!(stop_over(
            "the fox had to wait stop",
            "and the fox had to wait"
        ));
        assert!(!stop_over(
            "the fox had to wait",
            "and the fox had to wait for the rain"
        ));
        assert!(!stop_over("Hold on.", "hold on, said the owl"));
        assert!(stop_over("Hang on.", "hold on, said the owl"));
        assert!(!stop_over("Mm-hm.", ""));
        assert_eq!(listen_for(Some("heard")), Listen::InYields);
        assert_eq!(listen_for(Some("heard-always")), Listen::Always);
        assert_eq!(listen_for(Some("on")), Listen::Off);
        assert_eq!(listen_for(None), Listen::Off);
    }

    /// With `heard-always`: every block goes to the stream while the lamp
    /// sounds, and none once it has gone quiet; a stop word the lamp said
    /// itself stops nothing; one said over it stops the reply once; the next
    /// reply begins afresh; a yield takes its usual course, told nothing.
    #[test]
    fn listening_always_hears_the_lamps_whole_reply_and_stops_on_a_word_it_did_not_say() {
        let script = Arc::new(Mutex::new(Script::default()));
        let mut words = StopWords::always(Box::new(Scripted(Arc::clone(&script))));
        let at = |n: u64| 10_000_000 + n * 10_000;
        assert_eq!(
            words.block(true, true, false, None, None, at(1), &block(1)),
            Tell::Nothing
        );
        words.lamp_said("And so the fox had to wait.", at(1), false);
        script.lock().unwrap().words = Some("the fox had to wait".into());
        words.block(true, true, false, None, None, at(2), &block(2));
        assert!(!words.spoken_stop());
        // A yield: the rule is told nothing, and its audio goes on.
        assert_eq!(
            words.block(true, true, true, Some(0), None, at(3), &block(3)),
            Tell::Nothing
        );
        script.lock().unwrap().words = Some("Stop.".into());
        words.block(true, true, false, None, None, at(4), &block(4));
        assert!(words.spoken_stop());
        assert!(!words.spoken_stop());
        script.lock().unwrap().words = Some("Stop.".into());
        words.block(true, true, false, None, None, at(5), &block(5));
        assert!(!words.spoken_stop(), "a second stop in the same reply");
        // The lamp quiet: nothing more goes.
        words.block(true, false, false, None, None, at(6), &block(6));
        words.block(false, false, false, None, None, at(7), &block(7));
        // A new reply hears afresh, and the old reply's words are forgotten.
        words.block(true, true, false, None, None, at(800), &block(8));
        script.lock().unwrap().words = Some("wait".into());
        words.block(true, true, false, None, None, at(801), &block(9));
        assert!(words.spoken_stop());
        let told = script.lock().unwrap().told.clone();
        assert_eq!(
            told,
            [
                "ready", "hear 1", "hear 2", "hear 3", "hear 4", "hear 5", "rest", "ready",
                "hear 8", "hear 9"
            ]
        );
    }

    /// #828 section 2, on the blocks' 10 ms capture clock, no sleeps: a
    /// reply begins at block 0 with "Once upon a time," written as it came
    /// (nothing queued); the stream's `heard` comes at the block 100 ms in;
    /// the lamp's `own` words, if any, are written `after_ms` after that,
    /// stamped as they come. The delay from the words to the stop, in ms,
    /// or `None` if nothing was stopped in the 400 ms after them.
    fn stop_delay_ms(hold: bool, heard: &str, own: Option<(&str, u64)>) -> Option<u64> {
        stop_delay_pieces_ms(hold, heard, own.as_slice())
    }

    /// The same, with the lamp's own words written in pieces, each
    /// `(text, after_ms)`, as transcripts come.
    fn stop_delay_pieces_ms(hold: bool, heard: &str, own: &[(&str, u64)]) -> Option<u64> {
        let script = Arc::new(Mutex::new(Script::default()));
        let mut words = StopWords::always_counted(Box::new(Scripted(Arc::clone(&script))), None)
            .with_hold(hold);
        let at = |ms: u64| 20_000_000 + ms * 1000;
        words.lamp_said("Once upon a time,", at(0), false);
        for ms in (0..=500).step_by(10) {
            if ms == 100 {
                script.lock().unwrap().words = Some(heard.into());
            }
            for (text, after_ms) in own {
                if ms == 100 + after_ms {
                    words.lamp_said(text, at(ms), false);
                }
            }
            words.block(true, true, false, None, None, at(ms), &block(1));
            if words.spoken_stop() {
                return Some(ms - 100);
            }
        }
        None
    }

    /// (a) The lamp's own "stop", its text written 80 ms after the stream's
    /// words for it, as at a reply's start with nothing queued: held, it is
    /// the lamp's own and stops nothing. Without the hold it stopped the
    /// lamp at once.
    #[test]
    fn the_lamps_own_stop_whose_text_comes_80_ms_late_stops_nothing() {
        let own = Some(("And the owl said: stop, little fox.", 80));
        assert_eq!(stop_delay_ms(true, "the owl said stop", own), None);
        assert_eq!(
            stop_delay_ms(true, "Wait.", Some(("Wait for me, said the owl.", 80))),
            None
        );
        // The reviewer's probe on #1353 at 17:34: its text in two pieces, the
        // first, for audio played before the stop, ends no hold.
        assert_eq!(
            stop_delay_pieces_ms(
                true,
                "the owl said stop",
                &[("And the owl said:", 20), ("stop, little fox.", 60)]
            ),
            None
        );
    }

    /// Reviewer-two's P2 on #3295: with backchannel holds a yield's words
    /// are a stop only as a stop heard over the lamp is. The lamp's own
    /// "stop", its text 80 ms late, is held and told as no stop once that
    /// text comes; words the lamp has just said are no stop at once; a
    /// person's stop is told as one when its hold ends, at 150 ms. Each
    /// tell of words, with its delay from the result in ms.
    #[test]
    fn with_backchannel_holds_a_yields_words_are_no_stop_if_the_lamp_said_them() {
        let told = |heard: &str, said: &str, own: Option<(&str, u64)>| {
            let script = Arc::new(Mutex::new(Script::default()));
            let mut words =
                StopWords::always_counted(Box::new(Scripted(Arc::clone(&script))), None)
                    .with_hold(true)
                    .with_backchannel_holds(true);
            let at = |ms: u64| 20_000_000 + ms * 1000;
            words.lamp_said(said, at(0), false);
            let mut tells = Vec::new();
            for ms in (0..=500).step_by(10) {
                if ms == 100 {
                    script.lock().unwrap().words = Some(heard.into());
                }
                if let Some((text, after_ms)) = own
                    && ms == 100 + after_ms
                {
                    words.lamp_said(text, at(ms), false);
                }
                let yielded = ms >= 50;
                let still = yielded.then_some(0);
                if let Tell::Words { stop, .. } =
                    words.block(true, true, yielded, still, None, at(ms), &block(1))
                {
                    tells.push((ms - 100, stop));
                }
            }
            tells
        };
        let own = Some(("And the owl said: stop, little fox.", 80));
        assert_eq!(
            told("the owl said stop", "Once upon a time,", own),
            [(80, false)]
        );
        assert_eq!(
            told("the fox had to wait", "And so the fox had to wait.", None),
            [(0, false)]
        );
        assert_eq!(
            told("Lamp, stop.", "Once upon a time,", None),
            [(150, true)]
        );
    }

    /// (b) A person's stop with no such words of the lamp's acts at 150 ms,
    /// the most it is delayed; sooner once the lamp's words cover its
    /// block, and at once if they already did (its audio queued ahead).
    #[test]
    fn a_persons_stop_is_held_at_most_150_ms() {
        assert_eq!(stop_delay_ms(true, "Lamp, stop.", None), Some(150));
        assert_eq!(HOLD_US, 150_000);
        assert_eq!(
            stop_delay_ms(true, "Lamp, stop.", Some(("the owl flew home", 40))),
            Some(150)
        );
        let script = Arc::new(Mutex::new(Script::default()));
        let mut words = StopWords::always_counted(Box::new(Scripted(Arc::clone(&script))), None)
            .with_hold(true);
        words.block(true, true, false, None, None, 1_000_000, &block(1));
        words.lamp_said("and the owl flew home", 9_000_000, true);
        script.lock().unwrap().words = Some("Stop.".into());
        words.block(true, true, false, None, None, 1_010_000, &block(1));
        assert!(words.spoken_stop(), "covered already: no delay");
        // A reply that ends while its stop is held stops nothing, and the
        // next begins with none held.
        let script = Arc::new(Mutex::new(Script::default()));
        let mut words = StopWords::always_counted(Box::new(Scripted(Arc::clone(&script))), None)
            .with_hold(true);
        words.block(true, true, false, None, None, 1_000_000, &block(1));
        script.lock().unwrap().words = Some("Stop.".into());
        words.block(true, true, false, None, None, 1_010_000, &block(1));
        words.block(false, false, false, None, None, 1_020_000, &block(1));
        words.block(true, true, false, None, None, 2_000_000, &block(1));
        words.block(true, true, false, None, None, 2_200_000, &block(1));
        assert!(!words.spoken_stop());
    }

    /// (c) The lamp's own words later than the hold: its own "stop" stops it
    /// at 150 ms. That is the safe side: a stop the person meant is never
    /// held longer than 150 ms nor dropped, and a false one ends the reply
    /// as a hand Stop does, with the lamp listening again.
    #[test]
    fn the_lamps_own_words_later_than_the_hold_stop_it_at_150_ms() {
        let own = Some(("And the owl said: stop, little fox.", 200));
        assert_eq!(stop_delay_ms(true, "the owl said stop", own), Some(150));
    }

    /// The first builder's and the reviewer's P3 on #1353: each held stop,
    /// how long it waited and whether it acted, is in the reply's counts.
    #[test]
    fn each_held_stop_and_its_wait_is_in_the_replys_counts() {
        let dir = std::env::temp_dir().join(format!("lamp-voice-stop-held-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("stop-words.json");
        let script = Arc::new(Mutex::new(Script::default()));
        let mut words =
            StopWords::always_counted(Box::new(Scripted(Arc::clone(&script))), Some(path.clone()))
                .with_hold(true);
        let at = |ms: u64| 20_000_000 + ms * 1000;
        for ms in (0..=600).step_by(10) {
            match ms {
                100 => script.lock().unwrap().words = Some("the owl said stop".into()),
                180 => words.lamp_said("And the owl said: stop.", at(ms), false),
                300 => script.lock().unwrap().words = Some("Lamp, wait.".into()),
                _ => {}
            }
            words.block(true, true, false, None, None, at(ms), &block(1));
            if words.spoken_stop() {
                assert_eq!(ms, 450);
            }
        }
        words.block(false, false, false, None, None, at(610), &block(1));
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            written["reply"]["held"],
            serde_json::json!([{"waited_ms": 80, "acted": false}, {"waited_ms": 150, "acted": true}]),
            "{written}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// (d) With the switch off, as before: every stop the lamp did not say
    /// by then acts at once. The hold is only for listening to everything,
    /// and only for `own-words`.
    #[test]
    fn without_the_hold_a_stop_acts_at_once() {
        let own = Some(("And the owl said: stop, little fox.", 80));
        assert_eq!(stop_delay_ms(false, "the owl said stop", own), Some(0));
        assert_eq!(stop_delay_ms(false, "Lamp, stop.", None), Some(0));
        assert!(hold_for(Some("own-words")));
        for value in [None, Some(""), Some("on"), Some("OWN-WORDS")] {
            assert!(!hold_for(value), "{value:?}");
        }
        let yields = StopWords::new(Box::new(Scripted(Arc::new(Mutex::new(Script::default())))))
            .with_hold(true);
        assert!(!yields.hold);
    }

    /// With `heard-always`, through the real thread: a reply's audio goes
    /// to the stream as it is heard, and the words come back as the
    /// service gives them, interim ones, with no stream asked to finish;
    /// none are given once the reply is over.
    #[test]
    fn listening_always_gives_the_words_as_they_come_through_the_thread() {
        let service = Service::interim(&["Stop."]);
        let mut ears = RelayEars::with(true, None, None, service.connect()).unwrap();
        ears.ready();
        until("a stream is asked for", || service.connections() == 1);
        let mut words = None;
        until("the interim words", || {
            ears.hear(lamp_ipc::monotonic_us(), &[1; 160]);
            words = words.take().or_else(|| ears.words());
            words.is_some()
        });
        assert_eq!(words.as_deref(), Some("Stop."));
        ears.rest();
        ears.hear(lamp_ipc::monotonic_us(), &[1; 160]);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(ears.words(), None);
    }

    /// #1047, the first builder at 20:58: a service that closes the stream
    /// once it has answered a `Finalize`, as the relay may. The words it
    /// gave are read, the stream is replaced at once, and the reply's audio
    /// goes on to the new one. With `LAMP_STOP_WORDS_AUDIO`, the reply's
    /// stream audio, as sent, is written at its end; with
    /// `LAMP_STOP_WORDS_COUNTS`, the stream's counts beside the reply's
    /// (the first builder at 02:05).
    #[test]
    fn a_stream_closed_after_a_flush_is_replaced_and_its_audio_written() {
        let service = Service::new(&["Stop.", ""]);
        let path = std::env::temp_dir().join(format!(
            "lamp-stop-words-{}-{:x}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let counts = path.with_extension("json");
        let mut ears = RelayEars::with(
            true,
            Some(path.clone()),
            Some(counts.clone()),
            service.connect(),
        )
        .unwrap();
        ears.ready();
        until("a stream is asked for", || service.connections() == 1);
        until("audio reaches the service", || {
            ears.hear(lamp_ipc::monotonic_us(), &[1; 160]);
            service.audio() > 0
        });
        ears.flush();
        let mut words = None;
        until("the flush's words", || {
            words = words.take().or_else(|| ears.words());
            words.is_some()
        });
        assert_eq!(words.as_deref(), Some("Stop."));
        // At least two, not exactly: the ears keep a stream ready, so the
        // one this audio takes up is soon followed by the next asked for.
        // Checked for exactly two, a thread that took it up and asked again
        // before this one looked never let the wait end (ARM64, #1699's run
        // 39271945892).
        until("another stream", || {
            ears.hear(lamp_ipc::monotonic_us(), &[2; 160]);
            service.connections() >= 2
        });
        let before = service.audio();
        until("audio reaches the new stream", || {
            ears.hear(lamp_ipc::monotonic_us(), &[2; 160]);
            service.audio() > before
        });
        // The reply's own counts, as `StopWords` writes them before it rests
        // the stream.
        std::fs::write(&counts, br#"{"reply":{"results":1}}"#).unwrap();
        ears.rest();
        until("the reply's stream audio is written", || {
            hound::WavReader::open(&path).is_ok_and(|wav| wav.len() >= 320)
        });
        let wav = hound::WavReader::open(&path).unwrap();
        assert_eq!((wav.spec().sample_rate, wav.spec().channels), (16_000, 1));
        let mut written = serde_json::Value::Null;
        until("the stream's counts are written", || {
            written = std::fs::read(&counts)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default();
            written.get("stream").is_some()
        });
        assert_eq!(written["reply"]["results"], 1, "{written}");
        assert!(
            written["stream"]["blocks_taken"].as_u64() > Some(0),
            "{written}"
        );
        assert_eq!(written["stream"]["opened_again"], 1, "{written}");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&counts);
    }

    /// A stand-in for the speech-to-text that sends each result the test
    /// pushes as it is pushed, and counts the audio bytes it is sent; its
    /// connection completes only once `open` (#2939).
    struct Pushed {
        audio: Arc<std::sync::atomic::AtomicUsize>,
        open: Arc<std::sync::atomic::AtomicBool>,
        /// Each connection's way to its client, in the order they came: the
        /// first is the stream the reply's audio goes to.
        push: Arc<Mutex<Vec<tokio::sync::mpsc::UnboundedSender<String>>>>,
        flux: bool,
    }

    impl Pushed {
        fn new(flux: bool, open: bool) -> Self {
            Self {
                audio: Arc::default(),
                open: Arc::new(open.into()),
                push: Arc::default(),
                flux,
            }
        }

        fn connect(&self) -> impl Fn() -> Connected + Send + 'static {
            let (audio, open, push, flux) = (
                Arc::clone(&self.audio),
                Arc::clone(&self.open),
                Arc::clone(&self.push),
                self.flux,
            );
            move || {
                use futures_util::{SinkExt, StreamExt};
                use tokio_tungstenite::{
                    WebSocketStream,
                    tungstenite::{Message, protocol::Role},
                };
                let (audio, open, push) =
                    (Arc::clone(&audio), Arc::clone(&open), Arc::clone(&push));
                Box::pin(async move {
                    while !open.load(std::sync::atomic::Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                    let (near, far) = tokio::io::duplex(1 << 20);
                    let (pushing, mut pushed) = tokio::sync::mpsc::unbounded_channel::<String>();
                    push.lock().unwrap().push(pushing);
                    tokio::spawn(async move {
                        let mut socket =
                            WebSocketStream::from_raw_socket(far, Role::Server, None).await;
                        loop {
                            tokio::select! {
                                message = socket.next() => match message {
                                    Some(Ok(Message::Binary(bytes))) => {
                                        audio.fetch_add(bytes.len(), std::sync::atomic::Ordering::SeqCst);
                                    }
                                    Some(Ok(_)) => {}
                                    _ => break,
                                },
                                Some(text) = pushed.recv() => {
                                    if text.is_empty() {
                                        let _ = socket.close(None).await;
                                        break;
                                    }
                                    let _ = socket.send(Message::text(text)).await;
                                }
                            }
                        }
                    });
                    let socket = WebSocketStream::from_raw_socket(near, Role::Client, None).await;
                    Ok(if flux {
                        SttStream::on_flux(socket)
                    } else {
                        SttStream::on(socket)
                    })
                }) as Connected
            }
        }

        /// A result ending at `end` seconds into the stream's audio.
        fn result(&self, words: &str, is_final: bool, end: f64) {
            self.result_on(0, words, is_final, end);
        }

        /// The same, on the `stream`th connection.
        fn result_on(&self, stream: usize, words: &str, is_final: bool, end: f64) {
            let text = format!(
                r#"{{"type":"Results","is_final":{is_final},"speech_final":{is_final},"start":{start},"duration":0.3,"channel":{{"alternatives":[{{"transcript":"{words}"}}]}}}}"#,
                start = end - 0.3
            );
            self.push.lock().unwrap()[stream].send(text).unwrap();
        }

        /// The first connection closes, as the relay may close a stream.
        fn close_first(&self) {
            self.push.lock().unwrap()[0].send(String::new()).unwrap();
        }

        /// A Flux `TurnInfo` of `event`, its window ending at `end`.
        fn turn(&self, words: &str, event: &str, end: f64) {
            let text = format!(
                r#"{{"type":"TurnInfo","event":"{event}","turn_index":0,"audio_window_start":0.0,"audio_window_end":{end},"transcript":"{words}"}}"#
            );
            self.push.lock().unwrap()[0].send(text).unwrap();
        }

        /// Seconds of audio the service has been sent.
        fn seconds(&self) -> f64 {
            self.audio.load(std::sync::atomic::Ordering::SeqCst) as f64 / 32_000.0
        }
    }

    /// `seconds` of audio heard, in 10 ms blocks.
    fn hear_for(ears: &mut RelayEars, seconds: f64) {
        for _ in 0..(seconds * 100.0).round() as usize {
            ears.hear(lamp_ipc::monotonic_us(), &[1; 160]);
        }
    }

    /// The next `count` results, as (words, whether they decide).
    fn results(ears: &mut RelayEars, count: usize) -> Vec<(String, bool)> {
        let mut got = Vec::new();
        until("the results", || {
            got.extend(ears.result().map(|said| (said.words, said.decides)));
            got.len() >= count
        });
        got
    }

    /// Reviewer-two at 11:11 on #3200, through the real thread: a final
    /// from before a yield, queued before the yield's mark reached the
    /// thread, or published after it, decides nothing; the yield's own
    /// final, ending after the mark, does. Positions are the stream's
    /// audio, in the same seconds as its results' ends.
    #[test]
    fn only_a_final_ending_after_the_yields_mark_decides_it_through_the_thread() {
        let service = Pushed::new(false, true);
        let mut ears = RelayEars::with(true, None, None, service.connect()).unwrap();
        ears.ready();
        hear_for(&mut ears, 1.0);
        until("the audio before the yield", || service.seconds() >= 1.0);
        // Classified and queued while no yield was marked yet.
        service.result("Yeah.", true, 0.5);
        std::thread::sleep(Duration::from_millis(200));
        ears.mark();
        hear_for(&mut ears, 0.5);
        until("the yield's audio", || service.seconds() >= 1.5);
        // Late, of the audio before the mark; then the yield's own.
        service.result("Yeah.", true, 0.8);
        service.result("What", false, 1.3);
        service.result("What was that?", true, 1.4);
        assert_eq!(
            results(&mut ears, 4),
            [
                ("Yeah.".to_owned(), false),
                ("Yeah.".to_owned(), false),
                ("What".to_owned(), false),
                ("What was that?".to_owned(), true),
            ]
        );
    }

    /// Reviewer-two at 11:15 on #3200, through the real thread: audio
    /// heard while the stream is still connecting, a yield marked, and
    /// more heard before the connection completes. The mark stays where
    /// the yield began in what the stream is sent: an old segment ending
    /// before it decides nothing, the yield's own does.
    #[test]
    fn a_yield_marked_while_the_stream_connects_keeps_its_place_through_the_thread() {
        let service = Pushed::new(false, false);
        let mut ears = RelayEars::with(true, None, None, service.connect()).unwrap();
        ears.ready();
        hear_for(&mut ears, 1.0);
        ears.mark();
        hear_for(&mut ears, 0.5);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(service.seconds(), 0.0);
        service
            .open
            .store(true, std::sync::atomic::Ordering::SeqCst);
        until("the waiting audio reaches the stream", || {
            service.seconds() >= 1.5
        });
        service.result("Yeah.", true, 0.9);
        service.result("Wait, what?", true, 1.4);
        assert_eq!(
            results(&mut ears, 2),
            [
                ("Yeah.".to_owned(), false),
                ("Wait, what?".to_owned(), true),
            ]
        );
    }

    /// The stream closed mid-reply and replaced (#1047): a yield marked on
    /// the new one is placed on its own audio, not the reply's, so the new
    /// stream's segment from before the mark decides nothing and its own
    /// after the mark does (#2939).
    #[test]
    fn a_yield_marked_after_the_stream_is_replaced_is_placed_on_the_new_one() {
        let service = Pushed::new(false, true);
        let mut ears = RelayEars::with(true, None, None, service.connect()).unwrap();
        ears.ready();
        hear_for(&mut ears, 1.0);
        until("the first stream's audio", || service.seconds() >= 1.0);
        until("another stream kept ready", || {
            service.push.lock().unwrap().len() >= 2
        });
        service.close_first();
        std::thread::sleep(Duration::from_millis(100));
        hear_for(&mut ears, 0.5);
        until("the new stream's audio", || service.seconds() >= 1.5);
        ears.mark();
        hear_for(&mut ears, 0.5);
        until("the yield's audio", || service.seconds() >= 2.0);
        service.result_on(1, "Okay.", true, 0.4);
        service.result_on(1, "Okay, wait.", true, 0.9);
        assert_eq!(
            results(&mut ears, 2),
            [
                ("Okay.".to_owned(), false),
                ("Okay, wait.".to_owned(), true),
            ]
        );
    }

    /// Reviewer-two at 11:15 on #3200, through the real thread on Flux: an
    /// `Update`'s words, which may still change, decide nothing; the
    /// turn's `EndOfTurn` after the mark does, read in a later drain.
    #[test]
    fn on_flux_only_an_end_of_turn_decides_a_yield_through_the_thread() {
        let service = Pushed::new(true, true);
        let mut ears = RelayEars::with(true, None, None, service.connect()).unwrap();
        ears.ready();
        ears.mark();
        hear_for(&mut ears, 1.0);
        until("the audio", || service.seconds() >= 0.9);
        service.turn("Okay.", "Update", 0.4);
        assert_eq!(results(&mut ears, 1), [("Okay.".to_owned(), false)]);
        service.turn("Okay, what was that?", "EndOfTurn", 0.9);
        assert_eq!(
            results(&mut ears, 1),
            [("Okay, what was that?".to_owned(), true)]
        );
    }

    /// Reviewer-two on #3200, through the real thread on Flux: an idle
    /// stream's keep-alive quiet (80 ms after 5 s) is audio the caller never
    /// fed, so after one, an `EndOfTurn` ending before the yield's mark in
    /// the caller's audio decides nothing, though its end on the wire is
    /// past it; the yield's own does.
    #[test]
    fn on_flux_a_keep_alive_before_the_audio_does_not_move_the_yields_mark() {
        let service = Pushed::new(true, true);
        let mut ears = RelayEars::with(true, None, None, service.connect()).unwrap();
        ears.ready();
        std::thread::sleep(Duration::from_millis(5_200));
        until("one keep-alive", || service.seconds() >= 0.08);
        hear_for(&mut ears, 1.0);
        until("the audio before the yield", || service.seconds() >= 1.04);
        ears.mark();
        hear_for(&mut ears, 0.5);
        until("the yield's audio", || service.seconds() >= 1.52);
        // 0.96 s of the caller's audio: before the mark.
        service.turn("Yeah.", "EndOfTurn", 1.04);
        service.turn("What was that?", "EndOfTurn", 1.5);
        assert_eq!(
            results(&mut ears, 2),
            [
                ("Yeah.".to_owned(), false),
                ("What was that?".to_owned(), true),
            ]
        );
    }
}
