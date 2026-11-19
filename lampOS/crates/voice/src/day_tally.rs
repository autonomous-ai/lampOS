//! What the lamp's thinking, listening and speaking cost in a day (#7347).
//!
//! On 16 November 2026 a lamp at home used up its day's allowance before
//! six in the evening and answered nothing for over an hour. The lamp kept
//! no count of its own, so it could not tell how near it was, and nobody
//! could say afterwards what the day had gone on. Here each chat answer's
//! tokens are counted, by model, as the service's own `usage` gives them,
//! with the turns the model declined as not said to the lamp, the sound
//! sent to be heard and the lines sent to be spoken, for the day the
//! allowance is counted in: a UTC day. By the relay's count of that lamp's
//! day, thinking was about 38% of what it cost, listening 30% and speaking
//! 27%.
//!
//! Numbers only: no word, name or request is kept. Off unless [`SWITCH`]
//! is `on`.
//!
//! - An answer is counted where it ends ([`answered`]), a declined turn
//!   where it is declined ([`declined`]) and a line where it is sent to be
//!   spoken ([`spoken`]): a short lock on counts held in memory, never a
//!   file. Sound is counted where it goes to be heard ([`heard`]), a
//!   hundred times a second, so that takes no lock at all: one flag read
//!   and one add. A turn never waits on a file (as `made_counts`).
//! - The counts are added to [`FILE`] at most every [`FLUSH_EVERY`] by a
//!   thread of its own. The voice and its second-route worker are two
//!   processes; each adds its own under [`LOCK`], so neither undoes the
//!   other.
//! - At the first write of a new UTC day, the day before is kept whole as
//!   `yesterday` and today begins from nothing, so a morning's read shows
//!   what a whole day cost. Counts made in the last [`FLUSH_EVERY`] before
//!   midnight may be written into the new day, and those of the last
//!   [`FLUSH_EVERY`] before the process stops are lost. Only a later day
//!   turns the day over: a clock that has gone back across midnight goes
//!   on counting into the day the file is on.
//! - The lamp's clock is not always right. Until it is set
//!   ([`CLOCK_SET_FROM_DAY`]) nothing is written and the counts wait in
//!   memory, so a boot before the clock is set neither loses a day nor
//!   moves it. A file more than a day ahead of a set clock was written
//!   under a clock set ahead: it is brought back to today with its counts.
//! - Once an hour in which this process counted something, the journal
//!   has today's totals in one line ([`told`]).

pub mod replay;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// `LAMP_VOICE_DAY_TALLY=on`: count; anything else, nothing is counted and
/// no file is written.
pub const SWITCH: &str = "LAMP_VOICE_DAY_TALLY";
/// Where the counts are kept: the voice's own folder, root's.
pub const DIRECTORY: &str = "/var/lib/lampOS/voice/day-tally";
/// Today's counts and yesterday's.
pub const FILE: &str = "tally.json";
/// What every writer of [`FILE`] holds across its read and write.
const LOCK: &str = ".tally.lock";
/// The most often a process adds its counts to [`FILE`].
pub const FLUSH_EVERY: Duration = Duration::from_secs(60);
/// The models counted apart, at most: a lamp has two or three. Any past
/// that are counted together under [`OTHER`].
const MOST_MODELS: usize = 8;
/// The longest model name kept; a longer one is cut.
const MOST_NAME: usize = 64;
const OTHER: &str = "other";
const DAY_S: u64 = 86_400;
/// The first day a clock can truly say: 1 Nov 2026, before this was
/// written. A clock that reads an earlier day is not set yet.
const CLOCK_SET_FROM_DAY: u64 = 20_758;

/// One model's chat answers in a day.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Model {
    /// Chat answers that ended.
    pub answers: u64,
    /// Of them, those whose service gave no count of tokens.
    pub uncounted: u64,
    /// Tokens asked with.
    pub prompt_tokens: u64,
    /// Of those, the ones the service's cache held.
    pub cached_tokens: u64,
    /// Tokens answered with.
    pub completion_tokens: u64,
}

impl Model {
    fn add(&mut self, more: &Model) {
        self.answers = self.answers.saturating_add(more.answers);
        self.uncounted = self.uncounted.saturating_add(more.uncounted);
        self.prompt_tokens = self.prompt_tokens.saturating_add(more.prompt_tokens);
        self.cached_tokens = self.cached_tokens.saturating_add(more.cached_tokens);
        self.completion_tokens = self
            .completion_tokens
            .saturating_add(more.completion_tokens);
    }
}

/// One UTC day's counts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Day {
    /// Days since 1970, UTC.
    pub day: u64,
    pub models: BTreeMap<String, Model>,
    /// Turns the model declined as not said to the lamp.
    pub declined: u64,
    /// Sound sent to be heard, in milliseconds.
    pub heard_ms: u64,
    /// Lines that ended with their words asked for ([`line_ended`]): a
    /// request's own and a stop word's. One given up before its end is in
    /// [`Self::heard_ms`] and in none of these.
    pub heard_lines: u64,
    /// Of those, the lines that came back with no words.
    pub heard_no_words_lines: u64,
    /// The sound queued on them, in milliseconds.
    pub heard_no_words_ms: u64,
    /// The sound queued on the lines of turns the model declined as not
    /// said to the lamp, in milliseconds.
    pub heard_declined_ms: u64,
    /// Lines sent to be spoken; one asked again is counted again.
    pub spoken_lines: u64,
    /// Their characters.
    pub spoken_chars: u64,
}

impl Day {
    fn add(&mut self, more: &Day) {
        for (name, model) in &more.models {
            let name = if self.models.contains_key(name) || self.models.len() < MOST_MODELS {
                name.as_str()
            } else {
                OTHER
            };
            self.models.entry(name.to_owned()).or_default().add(model);
        }
        self.declined = self.declined.saturating_add(more.declined);
        self.heard_ms = self.heard_ms.saturating_add(more.heard_ms);
        self.heard_lines = self.heard_lines.saturating_add(more.heard_lines);
        self.heard_no_words_lines = self
            .heard_no_words_lines
            .saturating_add(more.heard_no_words_lines);
        self.heard_no_words_ms = self
            .heard_no_words_ms
            .saturating_add(more.heard_no_words_ms);
        self.heard_declined_ms = self
            .heard_declined_ms
            .saturating_add(more.heard_declined_ms);
        self.spoken_lines = self.spoken_lines.saturating_add(more.spoken_lines);
        self.spoken_chars = self.spoken_chars.saturating_add(more.spoken_chars);
    }

    fn is_empty(&self) -> bool {
        self.models.is_empty()
            && self.declined == 0
            && self.heard_ms == 0
            && self.heard_lines == 0
            && self.heard_declined_ms == 0
            && self.spoken_lines == 0
            && self.spoken_chars == 0
    }

    /// Every model's counts together.
    pub fn all(&self) -> Model {
        let mut all = Model::default();
        for model in self.models.values() {
            all.add(model);
        }
        all
    }
}

/// What [`FILE`] holds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tally {
    pub today: Day,
    /// The day before today, if the lamp counted on it.
    pub yesterday: Option<Day>,
}

/// This process's counts not yet in [`FILE`], and where they go.
#[derive(Default)]
struct Pending {
    /// The folder, once counting has begun.
    directory: Option<PathBuf>,
    counts: Day,
    /// The newest requests whose lines came back with words, and the sound
    /// queued on each in milliseconds: [`MOST_RECENT`] at most, so a turn
    /// declined a moment later finds its own. Never written.
    recent: VecDeque<(u64, u64)>,
}

/// The most requests whose sound is remembered for [`declined`].
const MOST_RECENT: usize = 32;

impl Pending {
    /// A line has ended with its words asked for: `samples` of sound were
    /// queued on it, and it came back with words or with none. `request`
    /// is its request, or `None` for a stop word's line. A line nothing was
    /// queued on was never heard, and is not counted.
    fn line_ended(&mut self, request: Option<u64>, samples: u64, no_words: bool) {
        if samples == 0 {
            return;
        }
        let ms = samples / SAMPLES_A_MS;
        self.counts.heard_lines = self.counts.heard_lines.saturating_add(1);
        if no_words {
            self.counts.heard_no_words_lines = self.counts.heard_no_words_lines.saturating_add(1);
            self.counts.heard_no_words_ms = self.counts.heard_no_words_ms.saturating_add(ms);
        } else if let Some(request) = request {
            if self.recent.len() >= MOST_RECENT {
                self.recent.pop_front();
            }
            self.recent.push_back((request, ms));
        }
    }

    /// `request`'s turn was declined: counted, with the sound of its own
    /// lines if they are still remembered.
    fn declined(&mut self, request: u64) {
        self.counts.declined = self.counts.declined.saturating_add(1);
        let mut ms = 0u64;
        self.recent.retain(|(of, line_ms)| {
            if *of == request {
                ms = ms.saturating_add(*line_ms);
            }
            *of != request
        });
        self.counts.heard_declined_ms = self.counts.heard_declined_ms.saturating_add(ms);
    }
}

static PENDING: OnceLock<Mutex<Pending>> = OnceLock::new();
/// Whether counting has begun, for [`heard`]: read where sound is sent to
/// be heard, so that path takes no lock.
static COUNTING: AtomicBool = AtomicBool::new(false);
/// Samples of sound sent to be heard and not yet in [`Pending`].
static HEARD_SAMPLES: AtomicU64 = AtomicU64::new(0);
/// The sound sent to be heard is 16 kHz: this many samples a millisecond.
const SAMPLES_A_MS: u64 = 16;

fn pending() -> std::sync::MutexGuard<'static, Pending> {
    PENDING
        .get_or_init(|| Mutex::new(Pending::default()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Counting begins if [`SWITCH`] is `on`, into [`DIRECTORY`].
pub fn start_from_environment() {
    if std::env::var(SWITCH).is_ok_and(|value| value == "on") {
        start(Path::new(DIRECTORY));
    }
}

/// Counting begins into `directory`, with the flush on a thread of its
/// own. A second call changes nothing.
pub fn start(directory: &Path) {
    {
        let mut pending = pending();
        if pending.directory.is_some() {
            return;
        }
        pending.directory = Some(directory.to_path_buf());
    }
    COUNTING.store(true, Ordering::Relaxed);
    let directory = directory.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("lamp-voice-day-tally".into())
        .spawn(move || {
            let mut told_hour = None;
            let mut told_waiting = false;
            loop {
                std::thread::sleep(FLUSH_EVERY);
                let now_s = unix_s();
                if !clock_is_set(now_s) && !std::mem::replace(&mut told_waiting, true) {
                    eprintln!("lamp-voice: the day's tally waits for the clock to be set");
                }
                match flush(&directory, now_s) {
                    Ok(Some(tally)) if told_hour != Some(now_s / 3_600) => {
                        told_hour = Some(now_s / 3_600);
                        eprintln!("lamp-voice: thinking today (UTC): {}", told(&tally.today));
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("lamp-voice: the day's tally not kept: {error}"),
                }
            }
        });
    match spawned {
        Ok(_) => eprintln!(
            "lamp-voice: the day's thinking, listening and speaking are counted, numbers only ({SWITCH}), in {DIRECTORY}/{FILE}"
        ),
        Err(error) => {
            eprintln!("lamp-voice: the day's tally not begun: {error}");
            COUNTING.store(false, Ordering::Relaxed);
            pending().directory = None;
        }
    }
}

/// One chat answer on `model` has ended, with the service's `usage` if it
/// gave one. Nothing unless counting has begun.
pub fn answered(model: &str, usage: Option<&Value>) {
    let mut pending = pending();
    if pending.directory.is_none() {
        return;
    }
    pending.counts.add(&one_answer(model, usage));
}

/// One answer on `model` as a day's counts: the service's `usage` in
/// DeepSeek's form (`prompt_cache_hit_tokens`) or OpenAI's
/// (`prompt_tokens_details.cached_tokens`), as the journal's `chat answer`
/// line reads it. With no count of either kind it is one answer that gave
/// none.
fn one_answer(model: &str, usage: Option<&Value>) -> Day {
    let count = |pointer: &str| usage.and_then(|usage| usage.pointer(pointer)?.as_u64());
    let prompt = count("/prompt_tokens");
    let cached =
        count("/prompt_cache_hit_tokens").or_else(|| count("/prompt_tokens_details/cached_tokens"));
    let completion = count("/completion_tokens");
    let one = Model {
        answers: 1,
        uncounted: u64::from(prompt.is_none() && completion.is_none()),
        prompt_tokens: prompt.unwrap_or(0),
        // Never more than the prompt it is a part of.
        cached_tokens: cached.unwrap_or(0).min(prompt.unwrap_or(0)),
        completion_tokens: completion.unwrap_or(0),
    };
    let mut day = Day::default();
    day.models.insert(name_of(model), one);
    day
}

/// `request`'s turn was declined by the model as not said to the lamp:
/// counted, with the sound of its lines. Nothing unless counting has begun.
pub fn declined(request: u64) {
    let mut pending = pending();
    if pending.directory.is_some() {
        pending.declined(request);
    }
}

/// A line has ended with its words asked for: `samples` of 16 kHz sound
/// were queued on it, for `request` or for a stop word (`None`), and it
/// came back with no words or with some. Nothing unless counting has begun.
pub fn line_ended(request: Option<u64>, samples: u64, no_words: bool) {
    let mut pending = pending();
    if pending.directory.is_some() {
        pending.line_ended(request, samples, no_words);
    }
}

/// `samples` of 16 kHz sound have gone to be heard. One flag read and one
/// add: it is called for every block sent. Nothing unless counting has begun.
pub fn heard(samples: usize) {
    if COUNTING.load(Ordering::Relaxed) {
        HEARD_SAMPLES.fetch_add(samples as u64, Ordering::Relaxed);
    }
}

/// One line has gone to be spoken: counted with its characters, never
/// kept. Nothing unless counting has begun.
pub fn spoken(text: &str) {
    let mut pending = pending();
    if pending.directory.is_some() {
        pending.counts.spoken_lines = pending.counts.spoken_lines.saturating_add(1);
        pending.counts.spoken_chars = pending
            .counts
            .spoken_chars
            .saturating_add(text.chars().count() as u64);
    }
}

/// `samples` as whole milliseconds of sound, and the samples left over.
fn ms_of(samples: u64) -> (u64, u64) {
    (samples / SAMPLES_A_MS, samples % SAMPLES_A_MS)
}

/// A model's name as it is kept: its printable ASCII, [`MOST_NAME`] at
/// most, or [`OTHER`] if none is left. A model's name is the service's
/// own, never a person's word.
fn name_of(model: &str) -> String {
    let name: String = model
        .chars()
        .filter(|c| c.is_ascii_graphic())
        .take(MOST_NAME)
        .collect();
    if name.is_empty() {
        OTHER.to_owned()
    } else {
        name
    }
}

fn unix_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The whole milliseconds in `samples`, taken out of it; what is left of
/// a millisecond stays there for the next time.
fn whole_ms_taken(samples: &AtomicU64) -> u64 {
    let (ms, left) = ms_of(samples.swap(0, Ordering::Relaxed));
    samples.fetch_add(left, Ordering::Relaxed);
    ms
}

/// Whether the clock reads a day it can truly be.
fn clock_is_set(now_s: u64) -> bool {
    now_s / DAY_S >= CLOCK_SET_FROM_DAY
}

/// This process's counts added to [`FILE`] under [`LOCK`], for the UTC day
/// `now_s` is in: read, the day turned over if it has changed, the counts
/// added, written whole beside it and moved over it. `None` when this
/// process had nothing to add and the day had not changed: nothing is
/// written. Counts that could not be written are kept for the next time.
fn flush(directory: &Path, now_s: u64) -> io::Result<Option<Tally>> {
    // Until the clock is set nothing is written and nothing is taken: the
    // counts wait in memory for a day that can be named.
    if !clock_is_set(now_s) {
        return Ok(None);
    }
    // The sound counted since the last time, as whole milliseconds; what is
    // left of a millisecond waits for the next.
    let heard_ms = whole_ms_taken(&HEARD_SAMPLES);
    let counts = {
        let mut pending = pending();
        pending.counts.heard_ms = pending.counts.heard_ms.saturating_add(heard_ms);
        std::mem::take(&mut pending.counts)
    };
    let written = add(directory, now_s, &counts);
    if written.is_err() {
        pending().counts.add(&counts);
    }
    written
}

fn add(directory: &Path, now_s: u64, counts: &Day) -> io::Result<Option<Tally>> {
    fs::create_dir_all(directory)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join(LOCK))?;
    rustix::fs::flock(&lock, rustix::fs::FlockOperation::LockExclusive)?;
    let path = directory.join(FILE);
    let mut tally: Tally = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            // Said, so a lost day is not a silent one (Heron on #7355). The
            // next write makes the file whole again, so it is said once.
            eprintln!(
                "lamp-voice: the day's tally in {} is not one this build reads: begun again",
                path.display()
            );
            Tally::default()
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Tally::default(),
        Err(error) => return Err(error),
    };
    let before = tally.clone();
    let day = now_s / DAY_S;
    if day > tally.today.day {
        // Only a later day turns the day over.
        let ended = std::mem::take(&mut tally.today);
        // Only the day just before is yesterday; an older one is let go.
        tally.yesterday = (ended.day + 1 == day && !ended.is_empty()).then_some(ended);
        tally.today.day = day;
    } else if tally.today.day > day + 1 {
        // The file is more than a day ahead of a clock that is set, so it
        // was written under a clock set ahead and since put right (Heron on
        // #7355). Its counts are a day's at most and the newest there are,
        // so they are kept as today's; a day before that is not the day
        // before today is let go.
        tally.today.day = day;
        tally.yesterday = tally.yesterday.take().filter(|kept| kept.day + 1 == day);
    }
    // Otherwise it is the file's day still, or the clock has gone back
    // across midnight, and that goes on counting into the day the file is
    // on (Heron on #7355).
    tally.today.add(counts);
    if tally == before {
        return Ok(None);
    }
    let new = directory.join(format!(".{FILE}.new"));
    let mut file = fs::File::create(&new)?;
    file.write_all(&serde_json::to_vec(&tally).map_err(io::Error::other)?)?;
    file.sync_all()?;
    fs::rename(&new, path)?;
    Ok(Some(tally))
}

/// What [`FILE`] in `directory` holds now, if it can be read. Counts of the
/// last [`FLUSH_EVERY`] may not be in it yet.
pub fn read(directory: &Path) -> Option<Tally> {
    serde_json::from_slice(&fs::read(directory.join(FILE)).ok()?).ok()
}

/// A day's counts in the journal's words: numbers only.
pub fn told(day: &Day) -> String {
    let all = day.all();
    let mut told = format!(
        "{} chat answers, {} prompt tokens ({} from the cache), {} completion tokens; {} turns declined; {} s of sound sent to be heard, of {} lines ended {} had no words ({} s) and {} s was in declined turns; {} lines sent to be spoken ({} characters)",
        all.answers,
        all.prompt_tokens,
        all.cached_tokens,
        all.completion_tokens,
        day.declined,
        day.heard_ms / 1_000,
        day.heard_lines,
        day.heard_no_words_lines,
        day.heard_no_words_ms / 1_000,
        day.heard_declined_ms / 1_000,
        day.spoken_lines,
        day.spoken_chars
    );
    if all.uncounted > 0 {
        told.push_str(&format!("; {} answers gave no count", all.uncounted));
    }
    if day.models.len() > 1 {
        let by_model: Vec<String> = day
            .models
            .iter()
            .map(|(name, model)| {
                format!(
                    "{name} {} answers, {} tokens",
                    model.answers,
                    model.prompt_tokens.saturating_add(model.completion_tokens)
                )
            })
            .collect();
        told.push_str(&format!("; by model: {}", by_model.join(", ")));
    }
    told
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "lamp-day-tally-{name}-{}-{}",
            std::process::id(),
            unix_s()
        ));
        let _ = fs::remove_dir_all(&folder);
        folder
    }

    fn one(model: &str, usage: Value) -> Day {
        one_answer(model, Some(&usage))
    }

    /// An answer's `usage` in both services' forms, and with none. The
    /// cache's share is never more than the prompt it is a part of.
    #[test]
    fn an_answers_usage_is_read_in_both_forms() {
        let model = |usage: Option<&Value>| one_answer("m", usage).models["m"].clone();
        let deepseek = json!({
            "prompt_tokens": 11_000, "prompt_cache_hit_tokens": 9_000, "completion_tokens": 40
        });
        assert_eq!(
            model(Some(&deepseek)),
            Model {
                answers: 1,
                uncounted: 0,
                prompt_tokens: 11_000,
                cached_tokens: 9_000,
                completion_tokens: 40,
            }
        );
        let openai = json!({
            "prompt_tokens": 900,
            "prompt_tokens_details": {"cached_tokens": 512},
            "completion_tokens": 7
        });
        assert_eq!(
            model(Some(&openai)),
            Model {
                answers: 1,
                uncounted: 0,
                prompt_tokens: 900,
                cached_tokens: 512,
                completion_tokens: 7,
            }
        );
        let odd = json!({"prompt_tokens": 10, "prompt_cache_hit_tokens": 99});
        assert_eq!(model(Some(&odd)).cached_tokens, 10);
        let none = Model {
            answers: 1,
            uncounted: 1,
            ..Model::default()
        };
        assert_eq!(model(None), none);
        assert_eq!(model(Some(&json!({}))), none);
    }

    /// Until counting begins nothing is counted. No test begins it, so in
    /// a test process an answer and a declined turn leave nothing behind.
    #[test]
    fn until_counting_begins_nothing_is_counted() {
        answered(
            "m",
            Some(&json!({"prompt_tokens": 5, "completion_tokens": 1})),
        );
        declined(1);
        heard(1_600);
        line_ended(Some(1), 1_600, true);
        spoken("Hello there.");
        assert_eq!(HEARD_SAMPLES.load(Ordering::Relaxed), 0);
        let pending = pending();
        assert!(pending.directory.is_none());
        assert!(pending.counts.is_empty());
    }

    /// A line's sound is told by what it came to: no words, or words in a
    /// turn the model then declined. A stop word's line is no request's,
    /// and only the newest requests are remembered.
    #[test]
    fn a_lines_sound_is_told_by_what_it_came_to() {
        let mut pending = Pending::default();
        // 3 s with no words: a request's line and a stop word's.
        pending.line_ended(Some(1), 32_000, true);
        pending.line_ended(None, 16_000, true);
        // 3 s with words, in two lines of one request, then declined.
        pending.line_ended(Some(2), 16_000, false);
        pending.line_ended(Some(2), 32_000, false);
        // 1 s with words, never declined: it is in neither count.
        pending.line_ended(Some(3), 16_000, false);
        // A stop word's words are no request's.
        pending.line_ended(None, 16_000, false);
        // A line nothing was queued on was never heard.
        pending.line_ended(Some(4), 0, true);
        pending.line_ended(None, 0, false);
        pending.declined(2);
        // Declined again, or a request never heard: a turn, with no sound.
        pending.declined(2);
        pending.declined(9);
        assert_eq!(pending.counts.heard_lines, 6);
        assert_eq!(pending.counts.heard_no_words_lines, 2);
        assert_eq!(pending.counts.heard_no_words_ms, 3_000);
        assert_eq!(pending.counts.heard_declined_ms, 3_000);
        assert_eq!(pending.counts.declined, 3);
        assert_eq!(pending.recent.len(), 1);
        // Only the newest requests are remembered.
        let many = 2 * MOST_RECENT as u64;
        for request in 10..10 + many {
            pending.line_ended(Some(request), 16_000, false);
        }
        assert_eq!(pending.recent.len(), MOST_RECENT);
        pending.declined(10);
        assert_eq!(pending.counts.heard_declined_ms, 3_000);
        pending.declined(10 + many - 1);
        assert_eq!(pending.counts.heard_declined_ms, 4_000);
    }

    /// Sound is kept as whole milliseconds, with what is left of one waiting
    /// for the next write; it and the spoken lines add up in the file and
    /// are said in the journal's line.
    #[test]
    fn sound_and_spoken_lines_add_up_and_are_told() {
        assert_eq!(ms_of(16_000), (1_000, 0));
        assert_eq!(ms_of(16_015), (1_000, 15));
        assert_eq!(ms_of(15), (0, 15));
        let dir = folder("speech");
        let noon = 20_000 * DAY_S + 12 * 3_600;
        let first = Day {
            heard_ms: 61_500,
            heard_lines: 4,
            heard_no_words_lines: 1,
            heard_no_words_ms: 20_000,
            heard_declined_ms: 30_500,
            spoken_lines: 2,
            spoken_chars: 80,
            ..Day::default()
        };
        let second = Day {
            heard_ms: 13_500,
            spoken_lines: 1,
            spoken_chars: 40,
            ..Day::default()
        };
        assert!(!first.is_empty());
        add(&dir, noon, &first).unwrap().unwrap();
        let tally = add(&dir, noon + 60, &second).unwrap().unwrap();
        assert_eq!(tally.today.heard_ms, 75_000);
        assert_eq!(tally.today.spoken_lines, 3);
        assert_eq!(tally.today.spoken_chars, 120);
        assert_eq!(
            told(&tally.today),
            "0 chat answers, 0 prompt tokens (0 from the cache), 0 completion tokens; 0 turns \
             declined; 75 s of sound sent to be heard, of 4 lines ended 1 had no words (20 s) \
             and 30 s was in declined turns; 3 lines sent to be spoken (120 characters)"
        );
        // What is left of a millisecond is carried, never lost.
        let samples = AtomicU64::new(16_015);
        assert_eq!(whole_ms_taken(&samples), 1_000);
        assert_eq!(samples.load(Ordering::Relaxed), 15);
        samples.fetch_add(1, Ordering::Relaxed);
        assert_eq!(whole_ms_taken(&samples), 1);
        assert_eq!(samples.load(Ordering::Relaxed), 0);
        // A file written before these counts reads with them at nothing.
        let old: Tally =
            serde_json::from_str(r#"{"today":{"day":20000,"models":{},"declined":4}}"#).unwrap();
        assert_eq!(old.today.heard_ms, 0);
        assert_eq!(old.today.declined, 4);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Two processes' counts add up in one file, by model, and a write with
    /// nothing to add leaves the file as it is.
    #[test]
    fn counts_from_two_writers_add_up_by_model() {
        let dir = folder("add");
        let noon = 20_000 * DAY_S + 12 * 3_600;
        let main = one(
            "deepseek-chat",
            json!({"prompt_tokens": 11_000, "prompt_cache_hit_tokens": 9_000, "completion_tokens": 40}),
        );
        let second = one(
            "small-model",
            json!({"prompt_tokens": 600, "completion_tokens": 5}),
        );
        add(&dir, noon, &main).unwrap().unwrap();
        add(&dir, noon + 30, &second).unwrap().unwrap();
        let declined = Day {
            declined: 2,
            ..Day::default()
        };
        let tally = add(&dir, noon + 60, &declined).unwrap().unwrap();
        assert_eq!(tally.today.day, 20_000);
        assert_eq!(tally.today.models["deepseek-chat"].prompt_tokens, 11_000);
        assert_eq!(tally.today.models["deepseek-chat"].cached_tokens, 9_000);
        assert_eq!(tally.today.models["small-model"].answers, 1);
        assert_eq!(tally.today.declined, 2);
        assert_eq!(tally.yesterday, None);
        // Nothing to add, the same day: nothing is written.
        assert_eq!(add(&dir, noon + 120, &Day::default()).unwrap(), None);
        assert_eq!(read(&dir), Some(tally.clone()));
        assert_eq!(
            told(&tally.today),
            "2 chat answers, 11600 prompt tokens (9000 from the cache), 45 completion tokens; \
             2 turns declined; 0 s of sound sent to be heard, of 0 lines ended 0 had no \
             words (0 s) and 0 s was in declined turns; 0 lines sent to be spoken (0 \
             characters); by model: deepseek-chat 1 answers, 11040 tokens, small-model 1 \
             answers, 605 tokens"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The day turns over at 00:00 UTC: the day before is kept whole as
    /// yesterday, today begins from nothing, and a day older than that is
    /// let go.
    #[test]
    fn a_new_utc_day_keeps_the_day_before_and_begins_again() {
        let dir = folder("day");
        let late = 20_000 * DAY_S + DAY_S - 10;
        let usage = json!({"prompt_tokens": 100, "completion_tokens": 10});
        add(&dir, late, &one("m", usage.clone())).unwrap().unwrap();
        // Ten seconds later it is the next day, with nothing new to add.
        let turned = add(&dir, late + 10, &Day::default()).unwrap().unwrap();
        assert_eq!(turned.today.day, 20_001);
        assert!(turned.today.is_empty());
        assert_eq!(turned.yesterday.as_ref().unwrap().day, 20_000);
        assert_eq!(turned.yesterday.as_ref().unwrap().all().prompt_tokens, 100);
        // Three days on: day 20,001 had nothing, and 20,000 is not yesterday.
        let later = add(&dir, late + 10 + 3 * DAY_S, &one("m", usage))
            .unwrap()
            .unwrap();
        assert_eq!(later.today.day, 20_004);
        assert_eq!(later.today.all().answers, 1);
        assert_eq!(later.yesterday, None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A clock that goes back across midnight turns nothing over: today's
    /// counts stay, and the new ones join them.
    #[test]
    fn a_clock_gone_back_keeps_todays_counts() {
        let dir = folder("back");
        let early = 20_000 * DAY_S + 30;
        let usage = json!({"prompt_tokens": 100, "completion_tokens": 10});
        add(&dir, early - DAY_S, &one("m", usage.clone())).unwrap();
        let today = add(&dir, early, &one("m", usage.clone())).unwrap().unwrap();
        assert_eq!(today.today.day, 20_000);
        assert_eq!(today.yesterday.as_ref().unwrap().day, 19_999);
        let kept = add(&dir, early - 60, &one("m", usage.clone()))
            .unwrap()
            .unwrap();
        assert_eq!(kept.today.day, 20_000);
        assert_eq!(kept.yesterday, today.yesterday);
        assert_eq!(read(&dir).unwrap().today.all().answers, 2);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A file written under a clock set far ahead is brought back to today
    /// with its counts once the clock is right, and a day before it that
    /// is not the day before today is let go. The next day then turns over
    /// as any other.
    #[test]
    fn a_file_from_a_clock_set_ahead_is_brought_back_with_its_counts() {
        let dir = folder("ahead");
        let usage = json!({"prompt_tokens": 100, "completion_tokens": 10});
        let ahead = 25_000 * DAY_S + 30;
        add(&dir, ahead - DAY_S, &one("m", usage.clone())).unwrap();
        let wrong = add(&dir, ahead, &one("m", usage.clone())).unwrap().unwrap();
        assert_eq!(wrong.today.day, 25_000);
        assert_eq!(wrong.yesterday.as_ref().unwrap().day, 24_999);
        let right = 20_800 * DAY_S + 30;
        let back = add(&dir, right, &one("m", usage.clone())).unwrap().unwrap();
        assert_eq!(back.today.day, 20_800);
        assert_eq!(back.today.all().answers, 2);
        assert_eq!(back.yesterday, None);
        // Nothing to add: the file is still brought back, once.
        let dir_idle = folder("ahead-idle");
        add(&dir_idle, ahead, &one("m", usage.clone())).unwrap();
        let idle = add(&dir_idle, right, &Day::default()).unwrap().unwrap();
        assert_eq!(idle.today.day, 20_800);
        assert_eq!(idle.today.all().answers, 1);
        assert!(add(&dir_idle, right, &Day::default()).unwrap().is_none());
        let next = add(&dir, right + DAY_S, &one("m", usage)).unwrap().unwrap();
        assert_eq!(next.today.day, 20_801);
        assert_eq!(next.yesterday.unwrap().all().answers, 2);
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&dir_idle);
    }

    /// Until the clock reads a day it can truly be, nothing is written and
    /// nothing is taken from the counts waiting in memory.
    #[test]
    fn until_the_clock_is_set_nothing_is_written() {
        assert!(!clock_is_set(0));
        assert!(!clock_is_set(CLOCK_SET_FROM_DAY * DAY_S - 1));
        assert!(clock_is_set(CLOCK_SET_FROM_DAY * DAY_S));
        let dir = folder("unset");
        assert!(flush(&dir, 1_000).unwrap().is_none());
        assert!(!dir.exists());
    }

    /// A file that is not a tally is begun again rather than failing the
    /// write, and a model past the most kept apart is counted as other.
    #[test]
    fn a_bad_file_begins_again_and_many_models_are_bounded() {
        let dir = folder("bad");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(FILE), b"not json").unwrap();
        let now = 20_000 * DAY_S;
        let mut tally = Tally::default();
        for index in 0..MOST_MODELS + 3 {
            let usage = json!({"prompt_tokens": 1, "completion_tokens": 1});
            tally = add(&dir, now, &one(&format!("model-{index:02}"), usage))
                .unwrap()
                .unwrap();
        }
        assert_eq!(tally.today.models.len(), MOST_MODELS + 1);
        assert_eq!(tally.today.models[OTHER].answers, 3);
        assert_eq!(tally.today.all().answers, (MOST_MODELS + 3) as u64);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A model's name is kept as printable ASCII, cut, and never empty; and
    /// an answer with no usage is counted as one that gave no count.
    #[test]
    fn names_are_bounded_and_an_answer_without_usage_is_still_counted() {
        assert_eq!(name_of("deepseek-chat"), "deepseek-chat");
        assert_eq!(name_of("a b\nc\u{e9}"), "abc");
        assert_eq!(name_of(""), OTHER);
        assert_eq!(name_of(&"x".repeat(200)).len(), MOST_NAME);
        let mut day = Day::default();
        day.models.insert(
            "m".into(),
            Model {
                answers: 1,
                uncounted: 1,
                ..Model::default()
            },
        );
        assert_eq!(
            told(&day),
            "1 chat answers, 0 prompt tokens (0 from the cache), 0 completion tokens; 0 turns \
             declined; 0 s of sound sent to be heard, of 0 lines ended 0 had no words (0 s) \
             and 0 s was in declined turns; 0 lines sent to be spoken (0 characters); \
             1 answers gave no count"
        );
    }
}
