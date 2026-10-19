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

/// What the voice's environment asks for.
pub fn listen() -> Listen {
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
    /// The yield is over: what it heard is dropped, nothing more of it goes
    /// out once this returns, and its words, if they come later, are not
    /// given.
    fn forget(&mut self);
    /// The reply is over: as `forget`, and no stream is kept.
    fn rest(&mut self);
}

/// What the duplex rule is to be told after a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tell {
    Nothing,
    /// A yield began: it waits for its words.
    Await,
    /// Its words came: whether they were a stop phrase, and how many.
    Words {
        stop: bool,
        count: usize,
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
    /// With `always`: a stop heard is held for the lamp's own words
    /// ([`Self::with_hold`]), and the one held.
    hold: bool,
    held: Option<Held>,
    /// With `hold`: the latest stamp of the lamp's words stamped ahead of
    /// when they were written, and this reply's held stops, each with how
    /// long it waited (ms) and whether it acted.
    ahead_at: u64,
    holds: Vec<(u64, bool)>,
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
            hold: false,
            held: None,
            ahead_at: 0,
            holds: Vec::new(),
        }
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
    pub fn lamp_said(&mut self, text: &str, at_us: u64, ahead: bool) {
        if self.always {
            self.lamp.push_back((at_us, text.to_owned()));
            if ahead {
                self.ahead_at = self.ahead_at.max(at_us);
            }
        }
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
        captured_at_us: u64,
        samples: &[i16],
    ) {
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
        if replying && (sounding || yielded) {
            self.ears.hear(captured_at_us, samples);
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
            if after_speech || self.cadence && self.since_asked >= CADENCE_BLOCKS {
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
        while let Some(heard) = self.ears.words() {
            self.results += 1;
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
        if !replying && !self.rested {
            self.end_reply();
        }
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
        let lamp: Vec<&str> = self.lamp.iter().map(|(_, text)| text.as_str()).collect();
        if !replying || !stop_over(&held.heard, &lamp.join(" ")) {
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
        if covered || now_us >= held.at_us.saturating_add(HOLD_US) {
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
            self.listen_always(replying, sounding, yielded, captured_at_us, samples);
            return Tell::Nothing;
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
            match held {
                Some(held) => held
                    .iter()
                    .for_each(|block| self.ears.hear(block.captured_at_us, &block.samples)),
                None => self.ears.hear(captured_at_us, samples),
            }
            tell = Tell::Await;
        } else if yielded {
            if !self.asked {
                self.ears.hear(captured_at_us, samples);
            }
            if !self.asked && still.is_some_and(|still| still >= FINALIZE_AFTER_STILL_BLOCKS) {
                self.ears.finalize();
                self.asked = true;
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
            tell = Tell::Words {
                stop: stop_phrase(&words),
                count: words.split_whitespace().count(),
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
            self.ears.forget();
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
    let config = crate::second_config::load_from_path(config)?;
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
    heard: mpsc::Receiver<(u64, String)>,
    live: Arc<Live>,
    ended: mpsc::Receiver<()>,
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
    Rest,
}

/// How long a failed connection waits before the next.
const RETRY: Duration = Duration::from_secs(5);
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
        }
    }

    fn number(&self) -> std::sync::MutexGuard<'_, u64> {
        self.number.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn now(&self) -> u64 {
        *self.number()
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
        let number = self.number();
        *number == of
            && lamp_ipc::monotonic_us().saturating_sub(captured_at_us) <= MOST_AGE_US
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
        Self::named("wait words", false, None, None, move || {
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
        Self::named("stop words", always, record, counts, connect)
    }

    /// [`Self::with`], its thread and its journal lines called `name`.
    fn named<C, F>(
        name: &'static str,
        always: bool,
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
                        name, connect, always, record, counts, queued, said, &shared,
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
        let live = self.live.now();
        // Words of a yield that is over are not this one's (the
        // reviewer's P2 on #970).
        std::iter::from_fn(|| self.heard.try_recv().ok())
            .find(|(of, _)| *of == live)
            .map(|(_, words)| words)
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

/// The thread's loop: a stream connected and kept ready while a reply is
/// under way, the yield's audio to it, and its words back once asked for.
/// Its journal lines are called `name`.
#[allow(clippy::too_many_arguments)]
async fn serve<C, F>(
    name: &str,
    connect: C,
    always: bool,
    record: Option<std::path::PathBuf>,
    counts: Option<std::path::PathBuf>,
    mut queued: queue::Receiver<Command>,
    said: mpsc::Sender<(u64, String)>,
    live: &Live,
) where
    C: Fn() -> F,
    F: Future<Output = Result<SttStream, String>> + Send + 'static,
{
    let mut wanted = false;
    let mut ready: Option<SttStream> = None;
    // The stream a yield's audio goes to, and that yield.
    let mut current: Option<(u64, SttStream)> = None;
    // With `always`, the current stream's words as they come, and its yield.
    let mut listening: Option<(u64, tokio::sync::mpsc::UnboundedReceiver<String>)> = None;
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
        }
        waiting.retain(|(of, _, _)| *of == number);
        finalize_waiting = finalize_waiting.filter(|of| *of == number);
        if finishing.as_ref().is_some_and(|(of, _)| *of != number)
            && let Some((_, task)) = finishing.take()
        {
            task.abort();
        }
        if wanted
            && ready.is_none()
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
                            current = ready.take().map(|stream| (of, stream));
                        }
                        match &current {
                            Some((_, stream)) => {
                                for (of, at, block) in waiting.drain(..) {
                                    let taken = live.hand(of, at, &block, stream);
                                    handed.count(taken);
                                    keep(&mut kept, record.is_some() && taken, &block);
                                }
                                let taken = live.hand(of, captured_at_us, &samples, stream);
                                handed.count(taken);
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
                        ready = None;
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
                            for (of, at, block) in waiting.drain(..) {
                                let taken = live.hand(of, at, &block, &stream);
                                handed.count(taken);
                                keep(&mut kept, record.is_some() && taken, &block);
                            }
                            match finalize_waiting.take() {
                                Some(of) => {
                                    finishing = Some((of, tokio::spawn(async move {
                                        stream.finished().await.map_err(|error| error.to_string())
                                    })));
                                }
                                None => current = Some((number, stream)),
                            }
                        } else {
                            ready = Some(stream);
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
                        let _ = said.send((*of, words));
                    }
                    // The stream ended during the reply (#1047, the first
                    // builder at 20:58: nothing came back after the first
                    // pauses): it is let go and another asked for, and what
                    // is heard meanwhile waits for it.
                    _ => {
                        listening = None;
                        if always && let Some((_, stream)) = current.take() {
                            handed.reopened += 1;
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
                    let _ = said.send((of, words));
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
                count: 2
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
            }
        }

        fn interim(words: &[&'static str]) -> Self {
            Self {
                interim: true,
                ..Self::new(words)
            }
        }

        fn connect(&self) -> impl Fn() -> Connected + Send + 'static {
            let (audio, connections, words, interim) = (
                Arc::clone(&self.audio),
                Arc::clone(&self.connections),
                Arc::clone(&self.words),
                self.interim,
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
                    let (near, far) = tokio::io::duplex(1 << 16);
                    tokio::spawn(async move {
                        let mut socket =
                            WebSocketStream::from_raw_socket(far, Role::Server, None).await;
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
            until("a stream is asked for", || service.connections() == 1);
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
        ears.ready();
        until("a stream is asked for", || service.connections() == 1);
        ears.hear(lamp_ipc::monotonic_us() - 4_000_000, &[1; 160]);
        ears.hear(lamp_ipc::monotonic_us(), &[1; 160]);
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
        until("a stream is asked for", || service.connections() == 1);
        let held = live.hold_words.lock().unwrap();
        ears.hear(lamp_ipc::monotonic_us(), &[1; 160]);
        ears.finalize();
        until("the first yield's words have come", || {
            live.holding_words.load(std::sync::atomic::Ordering::SeqCst)
        });
        ears.forget();
        // The thread tells them only now, after the yield is over.
        drop(held);
        until("a stream for the next yield", || service.connections() == 2);
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
}
