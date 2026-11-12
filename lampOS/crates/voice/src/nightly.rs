//! The memory's nightly pass, done by the voice (`LAMP_MEMORY_NIGHTLY`,
//! the memory owner's extraction plan C, step B, 3 Nov). The night runner
//! is adapted from the Tiers night (`tiers_night`, #5000, on Tiers' own
//! bounds from the first builder on #4710), ported to Days.
//!
//! lamp-memory runs with no network, so the pass is split: for each known
//! person heard on an earlier day, the voice asks Days for that day's
//! bounded input (`nightly_prepare`), sends it to the lamp's model through
//! the relay (the same client the voice's replies use), and gives the
//! model's proposal back (`nightly_apply`), where Days checks each item
//! alone and keeps the good ones. The contract is
//! `docs/memory.md` "The nightly pass".
//!
//! Only with `LAMP_MEMORY_NIGHTLY=on` in the voice's environment, the
//! memory's socket, the second route's file and its relay
//! (`deepseek_base_url`): then one thread of its own, which every
//! [`LOOK_EVERY`] does nothing unless all of these hold:
//!
//! - it is the lamp's night ([`NIGHT_FROM_HOUR`] to [`NIGHT_UNTIL_HOUR`],
//!   local);
//! - nobody has spoken or written to the lamp for [`QUIET_FOR`] ([`stir`],
//!   called where the voice hears speech and where it hands a line on);
//! - the privacy slide is open and has been told within [`OPEN_FRESH`]
//!   ([`privacy`], from the privacy gate);
//! - this night's calls, bytes and tokens are under [`NIGHT_CALLS`],
//!   [`NIGHT_BYTES`] and [`NIGHT_TOKENS`].
//!
//! Only a known person's own scope ([`lamp_msgs::known::is_person_id`]):
//! never the room, a rules scope or a guest. The people are those the
//! eyes knew at the lamp ([`Lamp::at_lamp`]) when someone had spoken since
//! the night thread's last look (every [`LOOK_EVERY`], off the turn's
//! path; the memory's writer runs in the route workers, so this is how the
//! voice's own process learns who talked), on a day before today, at most
//! [`MOST_PENDING`] of them, at most [`DAYS_BACK`] days back; held in
//! memory only, so a restart loses that day's list. Days itself keeps only
//! the person's own records with their own words.
//!
//! A call is bounded by [`CALL_TIMEOUT`] and stops at once, the call in
//! flight dropped and nothing applied, when anyone speaks, a line is
//! handed on, or the slide closes; the batch is asked again after the next
//! quiet (Days gives the same job back). A reply the voice cannot read, or
//! a call that timed out, is applied as an empty proposal, which closes the
//! job with nothing kept, so nothing is retried in a loop. Nothing of it is
//! ever said or shown: the module has no speech, ring or bus of its own.
//! Off, none of it runs and [`stir`] and [`privacy`] are one atomic load. The day's words are held only for the call and never
//! journalled: the journal has counts per kind and reason, never text or a
//! person's id.
use futures_util::{FutureExt, future::LocalBoxFuture};
use lamp_local_time::LocalTime;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

/// The whole pass: `on` only.
pub const ENVIRONMENT: &str = "LAMP_MEMORY_NIGHTLY";
/// Patterns asked for and sent `patterns:true`: `on` only.
pub const PATTERNS_ENVIRONMENT: &str = "LAMP_MEMORY_NIGHTLY_PATTERNS";
/// Feelings asked for and sent `feelings:true`: `on` only.
pub const FEELINGS_ENVIRONMENT: &str = "LAMP_MEMORY_NIGHTLY_FEELINGS";
/// Weekly chapters (B2): `on` only, and only with [`ENVIRONMENT`] on. Each
/// known person heard in a week gets one call for that week's chapter, the
/// first night after it ends, after that night's days; and a recall of
/// theirs naming a day or span is told their chapters of it
/// ([`chapters_on`]).
pub const CHAPTERS_ENVIRONMENT: &str = "LAMP_MEMORY_WEEKLY_CHAPTERS";
/// A week waits for its chapter this many days after its Sunday at most.
pub const WEEKS_BACK: i64 = 7;
/// The drop reasons Days counts for a chapter; any other key is not read.
pub const CHAPTER_REASONS: [&str; 7] = [
    "shape",
    "unknown_source",
    "forgotten",
    "grounded",
    "private_kind",
    "full",
    "done",
];
/// The night's hours, local.
pub const NIGHT_FROM_HOUR: u8 = 3;
pub const NIGHT_UNTIL_HOUR: u8 = 6;
/// No line heard or handed on for this long: the lamp is idle.
pub const QUIET_FOR: Duration = Duration::from_secs(20 * 60);
/// The privacy slide counts as open only when told so this recently.
pub const OPEN_FRESH: Duration = Duration::from_secs(1);
/// How often the thread looks whether it may work, with nothing in hand.
pub const LOOK_EVERY: Duration = Duration::from_secs(60);
/// The most one model call may take.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// A night's calls, bytes (prompt and reply) and tokens at most. The
/// tokens are the relay's own count (`usage`) when it gives one, else one
/// token a byte; before a call, its prompt's bytes over [`BYTES_A_TOKEN`]
/// plus [`REPLY_TOKENS`] must fit what is left.
pub const NIGHT_CALLS: u32 = 16;
pub const NIGHT_BYTES: u64 = 1 << 20;
pub const NIGHT_TOKENS: u64 = 200_000;
pub const BYTES_A_TOKEN: u64 = 3;
pub const REPLY_TOKENS: u64 = 8_192;
/// How long the memory's socket is waited for a prepare or an apply.
pub const MEMORY_WAIT: Duration = Duration::from_secs(3);
/// A prepare's answer at most: Days' 64 KiB input and its framing.
pub const MOST_PREPARE_BYTES: u64 = 80 * 1024;
/// A model reply at most: what still fits Days' 64 KiB request line.
pub const MOST_REPLY_BYTES: usize = 60 * 1024;
/// Days' request line at most (`lamp_memory::protocol::MOST_LINE_BYTES`).
pub const MOST_LINE_BYTES: usize = 64 * 1024;
/// People and days waiting for a pass at most, and how many days back.
pub const MOST_PENDING: usize = 32;
/// Mirrored by `lamp_memory::nightly::DAYS_BACK`, the days a forget marks
/// for the night, and pinned equal by lamp-memory's
/// `tests/nightly_days_back.rs`.
pub const DAYS_BACK: i64 = 3;
/// How often a call in flight looks whether it must stop.
const STOP_CHECK: Duration = Duration::from_millis(20);

/// The item kinds of an apply's answer, in Days' order.
pub const KINDS: [&str; 6] = ["episode", "fact", "profile", "pattern", "ahead", "feeling"];
/// The drop reasons Days counts; any other key is not read.
pub const REASONS: [&str; 16] = [
    "shape",
    "unknown_source",
    "forgotten",
    "lamp_words",
    "question",
    "restates",
    "private_kind",
    "range",
    "days",
    "date",
    "supersede",
    "profile_cap",
    "off",
    "too_many",
    "full",
    "fingerprint",
];

static ON: AtomicBool = AtomicBool::new(false);
static CHAPTERS: AtomicBool = AtomicBool::new(false);

/// Whether a recall is told chapters: [`CHAPTERS_ENVIRONMENT`] and
/// [`ENVIRONMENT`] both `on` when the voice started. Off, one load.
pub fn chapters_on() -> bool {
    CHAPTERS.load(Ordering::Relaxed)
}
/// Speech, a line, or the slide closing: each moves it on.
static STIRS: AtomicU64 = AtomicU64::new(0);
static LAST_STIR_US: AtomicU64 = AtomicU64::new(0);
static OPEN_AT_US: AtomicU64 = AtomicU64::new(0);

/// Someone spoke, or a line was handed on: the night stops at once and
/// waits for [`QUIET_FOR`] again. Off, one load.
pub fn stir() {
    if ON.load(Ordering::Relaxed) {
        LAST_STIR_US.store(lamp_ipc::monotonic_us(), Ordering::Relaxed);
        STIRS.fetch_add(1, Ordering::Relaxed);
    }
}

/// The privacy gate's latest: open (input allowed) or not. Closed stops a
/// call in flight at once. Off, one load.
pub fn privacy(open: bool) {
    if !ON.load(Ordering::Relaxed) {
        return;
    }
    if open {
        OPEN_AT_US.store(lamp_ipc::monotonic_us(), Ordering::Relaxed);
    } else if OPEN_AT_US.swap(0, Ordering::Relaxed) != 0 {
        STIRS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Whether a switch's value is `on`.
pub fn switched_on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// What the lamp is doing, as the night reads it.
pub trait Lamp: Send {
    /// Moves on with every stir and every closing of the slide.
    fn stirs(&self) -> u64;
    fn quiet_for(&self) -> Duration;
    /// The slide open, and told so within [`OPEN_FRESH`].
    fn open(&self) -> bool;
    /// The memory's id of the known person the eyes see at the lamp now.
    fn at_lamp(&self) -> Option<String>;
}

/// The lamp as [`stir`] and [`privacy`] tell it, and the eyes' known file.
pub struct Told;
impl Lamp for Told {
    fn stirs(&self) -> u64 {
        STIRS.load(Ordering::Relaxed)
    }
    fn quiet_for(&self) -> Duration {
        let since = lamp_ipc::monotonic_us().saturating_sub(LAST_STIR_US.load(Ordering::Relaxed));
        Duration::from_micros(since)
    }
    fn open(&self) -> bool {
        let at = OPEN_AT_US.load(Ordering::Relaxed);
        at != 0 && lamp_ipc::monotonic_us().saturating_sub(at) < OPEN_FRESH.as_micros() as u64
    }
    fn at_lamp(&self) -> Option<String> {
        crate::known_faces::known_from_environment()
            .map(|name| crate::known_faces::memory_person(&name))
    }
}

/// Days' two nightly ops: each takes the request line and gives Days'
/// answer, or nothing when none came.
pub trait Memory: Send {
    fn prepare(&mut self, line: &Value) -> Option<Value>;
    fn apply(&mut self, line: &Value) -> Option<Value>;
    /// A weekly chapter's op (`weekly_prepare`, `weekly_apply`); none
    /// unless Days has them.
    fn weekly(&mut self, _line: &Value) -> Option<Value> {
        None
    }
}

/// Days on its socket.
pub struct Socket(pub PathBuf);
impl Memory for Socket {
    fn prepare(&mut self, line: &Value) -> Option<Value> {
        crate::tools::exchange_up_to(&self.0, line, MEMORY_WAIT, MOST_PREPARE_BYTES)
    }
    fn apply(&mut self, line: &Value) -> Option<Value> {
        crate::tools::exchange(&self.0, line, MEMORY_WAIT)
    }
    fn weekly(&mut self, line: &Value) -> Option<Value> {
        crate::tools::exchange_up_to(&self.0, line, MEMORY_WAIT, MOST_PREPARE_BYTES)
    }
}

/// The Monday (days since 1970-01-01) of the week of local day `day`.
pub fn monday_of(day: i64) -> i64 {
    // 1970-01-01 was a Thursday.
    day - (day + 3).rem_euclid(7)
}

/// The `nightly_prepare` line for `person`'s `day` (`YYYY-MM-DD`), after
/// record `after_id`.
pub fn prepare_line(person: &str, day: &str, after_id: u64) -> Value {
    json!({"op":"nightly_prepare","person":person,"day":day,"after_id":after_id})
}

/// The `nightly_apply` line: the switches passed through as the contract
/// says, and the proposal as the model gave it. A line over Days'
/// [`MOST_LINE_BYTES`] carries an empty proposal instead, which closes the
/// job with nothing kept.
pub fn apply_line(person: &str, job: &str, switches: Switches, proposal: Value) -> Value {
    let line = |proposal| {
        json!({"op":"nightly_apply","person":person,"job":job,
            "patterns":switches.patterns,"feelings":switches.feelings,"proposal":proposal})
    };
    let full = line(proposal);
    if full.to_string().len() < MOST_LINE_BYTES {
        full
    } else {
        line(json!({}))
    }
}

/// What a prepare gave: the job, the model's input, and where the next
/// batch of the day starts when there is one.
#[derive(Debug, PartialEq)]
pub struct Prepared {
    pub job: String,
    pub input: Value,
    /// The last record's id, when Days said `more:true`.
    pub more_after: Option<u64>,
}

/// A prepare Days refused, or answered not as the contract has it.
#[derive(Debug, PartialEq, Eq)]
pub struct Refused;

/// A prepare's answer read: a job, `Ok(None)` for `"reason":"nothing"`,
/// or [`Refused`].
pub fn prepared_of(answer: &Value) -> Result<Option<Prepared>, Refused> {
    if answer["ok"] != true {
        return Err(Refused);
    }
    if answer.get("input").is_none() && answer["reason"] == "nothing" {
        return Ok(None);
    }
    let input = answer
        .get("input")
        .filter(|input| input.is_object())
        .ok_or(Refused)?;
    let job = input["job"]
        .as_str()
        .filter(|job| !job.is_empty())
        .ok_or(Refused)?;
    let records = input["records"].as_array().ok_or(Refused)?;
    let last = records
        .iter()
        .filter_map(|record| record["id"].as_u64())
        .max();
    let more_after = if input["more"] == true { last } else { None };
    Ok(Some(Prepared {
        job: job.to_owned(),
        input: input.clone(),
        more_after,
    }))
}

/// An apply's counts: accepted and dropped per kind ([`KINDS`]) and the
/// drop reasons ([`REASONS`]). Only these keys and whole numbers are read,
/// so nothing else of an answer can reach the journal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub accepted: [u64; 6],
    pub dropped: [u64; 6],
    pub reasons: [u64; 16],
}

impl Counts {
    pub fn of(answer: &Value) -> Option<Self> {
        if answer["ok"] != true {
            return None;
        }
        let read = |object: &Value, keys: &[&str], into: &mut [u64]| {
            for (key, count) in keys.iter().zip(into.iter_mut()) {
                *count = object[key].as_u64().unwrap_or(0);
            }
        };
        let mut counts = Self::default();
        read(&answer["accepted"], &KINDS, &mut counts.accepted);
        read(&answer["dropped"], &KINDS, &mut counts.dropped);
        read(&answer["reasons"], &REASONS, &mut counts.reasons);
        Some(counts)
    }

    pub fn accepted_total(&self) -> u64 {
        self.accepted.iter().sum()
    }

    pub fn dropped_total(&self) -> u64 {
        self.dropped.iter().sum()
    }

    /// The journal's words: kinds and reasons with their counts.
    pub fn told(&self) -> String {
        format!(
            "accepted {} ({}), dropped {} ({}), reasons {}",
            self.accepted_total(),
            nonzero(&KINDS, &self.accepted),
            self.dropped_total(),
            nonzero(&KINDS, &self.dropped),
            self.reasons_told()
        )
    }

    /// The drop reasons counted, or `none`.
    pub fn reasons_told(&self) -> String {
        nonzero(&REASONS, &self.reasons)
    }
}

/// `keys` with their counts, those above 0 only, or `none`.
fn nonzero(keys: &[&str], counts: &[u64]) -> String {
    let said: Vec<String> = keys
        .iter()
        .zip(counts)
        .filter(|(_, count)| **count > 0)
        .map(|(key, count)| format!("{key} {count}"))
        .collect();
    if said.is_empty() {
        "none".to_owned()
    } else {
        said.join(", ")
    }
}

/// The two sensitive kinds' switches, passed through to apply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Switches {
    pub patterns: bool,
    pub feelings: bool,
}

impl Switches {
    pub fn from_environment() -> Self {
        let on = |name| switched_on(std::env::var(name).ok().as_deref());
        Self {
            patterns: on(PATTERNS_ENVIRONMENT),
            feelings: on(FEELINGS_ENVIRONMENT),
        }
    }
}

/// What a model call gave: its text, and the tokens the service counted.
pub struct Reply {
    pub text: String,
    pub tokens: Option<u64>,
}

/// A model call in flight.
pub type Asked<'a> = LocalBoxFuture<'a, Option<Reply>>;

/// The night's model. Its future is dropped to stop a call in flight.
pub trait Model: Send {
    fn ask<'a>(&'a self, system: &'a str, input: &'a str) -> Asked<'a>;
}

/// The relay's DeepSeek, thinking off, as the memory's observation pass
/// asks it (`second_provider::worker::observer_at`).
pub struct Relay(pub crate::second_provider::net::Chat);
impl Model for Relay {
    fn ask<'a>(&'a self, system: &'a str, input: &'a str) -> Asked<'a> {
        use crate::second_provider::net::{ChatEvent, Message};
        async move {
            let mut stream = self
                .0
                .send(&[
                    Message::system(system.to_owned()),
                    Message::user(input.to_owned()),
                ])
                .await
                .ok()?;
            let mut text = String::new();
            while let Some(event) = stream.next().await.ok()? {
                match event {
                    ChatEvent::Text(piece) => {
                        text.push_str(&piece);
                        if text.len() > MOST_REPLY_BYTES {
                            return None;
                        }
                    }
                    ChatEvent::Done => break,
                    _ => {}
                }
            }
            let count = |pointer: &str| stream.usage()?.pointer(pointer)?.as_u64();
            let tokens = count("/prompt_tokens")
                .zip(count("/completion_tokens"))
                .map(|(prompt, completion)| prompt + completion);
            Some(Reply { text, tokens })
        }
        .boxed_local()
    }
}

/// A night's bounds.
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub calls: u32,
    pub bytes: u64,
    pub tokens: u64,
    pub timeout: Duration,
    pub quiet_for: Duration,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            calls: NIGHT_CALLS,
            bytes: NIGHT_BYTES,
            tokens: NIGHT_TOKENS,
            timeout: CALL_TIMEOUT,
            quiet_for: QUIET_FOR,
        }
    }
}

/// What one look did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Not the night, not quiet, or the slide closed: nothing asked.
    Waiting,
    /// This night's calls, bytes or tokens are spent.
    Spent,
    /// Nobody heard on an earlier day is waiting.
    NoWork,
    /// Days had nothing of that person's day.
    Nothing,
    /// Days refused the prepare, or gave no answer: that day is let go.
    Refused,
    /// The proposal was applied; Days' counts.
    Applied(Box<Counts>),
    /// A week's chapter was applied: kept or not, and Days' drop reasons
    /// ([`CHAPTER_REASONS`] only).
    Chapter { kept: bool, reasons: String },
    /// The proposal could not be read, or the call timed out: applied
    /// empty, which closes the job with nothing kept.
    Emptied(Emptied),
    /// Applied, but Days' answer was not read.
    Unanswered,
    /// Someone spoke or the slide closed during the call: dropped, nothing
    /// applied, asked again after the next quiet.
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Emptied {
    TimedOut,
    Unread,
}

pub struct Night<M, L, K> {
    pub memory: M,
    pub lamp: L,
    pub model: K,
    pub bounds: Bounds,
    pub switches: Switches,
    /// Unix ms.
    pub clock: Box<dyn Fn() -> u64 + Send>,
    pub local: LocalTime,
    night: i64,
    calls: u32,
    bytes: u64,
    tokens: u64,
    /// (local day, person) waiting for a pass, oldest day first.
    pending: BTreeSet<(i64, String)>,
    /// The next batch of the first pending day: (day, person, after id).
    after: Option<(i64, String, u64)>,
    /// Weekly chapters ([`CHAPTERS_ENVIRONMENT`]): off by default.
    pub chapters: bool,
    /// (the week's Monday, person) waiting for a chapter.
    weeks: BTreeSet<(i64, String)>,
    /// The lamp's stirs at the last look.
    seen_stirs: u64,
    runtime: tokio::runtime::Runtime,
    pub log: Arc<dyn Fn(&str) + Send + Sync>,
}

impl<M: Memory, L: Lamp, K: Model> Night<M, L, K> {
    pub fn new(memory: M, lamp: L, model: K, local: LocalTime) -> std::io::Result<Self> {
        Ok(Self {
            memory,
            lamp,
            model,
            bounds: Bounds::default(),
            switches: Switches::default(),
            clock: Box::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64)
            }),
            local,
            night: i64::MIN,
            calls: 0,
            bytes: 0,
            tokens: 0,
            pending: BTreeSet::new(),
            after: None,
            chapters: false,
            weeks: BTreeSet::new(),
            seen_stirs: 0,
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .enable_io()
                .build()?,
            log: Arc::new(|line| eprintln!("lamp-voice: {line}")),
        })
    }

    /// This night's calls, bytes and tokens so far.
    pub fn spent(&self) -> (u32, u64, u64) {
        (self.calls, self.bytes, self.tokens)
    }

    /// How many person-days wait for a pass.
    pub fn waiting(&self) -> usize {
        self.pending.len()
    }

    fn may_work(&self) -> bool {
        self.lamp.open() && self.lamp.quiet_for() >= self.bounds.quiet_for
    }

    /// `person` talked at `at_unix_ms`: their day waits for a pass. Only a
    /// known person's id ([`lamp_msgs::known::is_person_id`]).
    pub fn note(&mut self, person: &str, at_unix_ms: u64) {
        if lamp_msgs::known::is_person_id(person) {
            let day = self.local.day(at_unix_ms);
            self.pending.insert((day, person.to_owned()));
            if self.chapters {
                self.weeks.insert((monday_of(day), person.to_owned()));
            }
        }
    }

    /// Who talked since the last look, as days to pass: the known person
    /// at the lamp when the stirs moved on; only days within [`DAYS_BACK`],
    /// at most [`MOST_PENDING`], the oldest let go first.
    fn take_heard(&mut self, now: u64, today: i64) {
        let stirs = self.lamp.stirs();
        if stirs != self.seen_stirs {
            self.seen_stirs = stirs;
            if let Some(person) = self.lamp.at_lamp() {
                self.note(&person, now);
            }
        }
        self.pending
            .retain(|(day, _)| *day >= today - DAYS_BACK && *day <= today);
        while self.pending.len() > MOST_PENDING {
            self.pending.pop_first();
        }
        self.weeks
            .retain(|(monday, _)| *monday + 6 >= today - WEEKS_BACK && *monday <= today);
        while self.weeks.len() > MOST_PENDING {
            self.weeks.pop_first();
        }
    }

    /// How many person-weeks wait for a chapter.
    pub fn weeks_waiting(&self) -> usize {
        self.weeks.len()
    }

    /// The day `day` is done for `person`: its batches all applied, or let
    /// go.
    fn done(&mut self, day: i64, person: &str) {
        self.pending.remove(&(day, person.to_owned()));
        self.after = None;
    }

    /// One look: at most one batch prepared, asked and applied.
    pub fn step(&mut self) -> Step {
        let now = (self.clock)();
        let today = self.local.day(now);
        self.take_heard(now, today);
        if !(NIGHT_FROM_HOUR..NIGHT_UNTIL_HOUR).contains(&self.local.hour_of_day(now)) {
            return Step::Waiting;
        }
        if today != self.night {
            self.night = today;
            (self.calls, self.bytes, self.tokens) = (0, 0, 0);
        }
        if !self.may_work() {
            return Step::Waiting;
        }
        if self.calls >= self.bounds.calls
            || self.bytes >= self.bounds.bytes
            || self.tokens >= self.bounds.tokens
        {
            return Step::Spent;
        }
        let Some((day, person)) = self.pending.iter().find(|(day, _)| *day < today).cloned() else {
            // The days done: a week over waits for its chapter.
            let week = self
                .weeks
                .iter()
                .find(|(monday, _)| *monday + 6 < today)
                .cloned();
            return match week {
                Some((monday, person)) if self.chapters => self.chapter(monday, &person),
                _ => Step::NoWork,
            };
        };
        let after_id = match &self.after {
            Some((d, p, after)) if *d == day && *p == person => *after,
            _ => 0,
        };
        let stirs = self.lamp.stirs();
        let prepared = self
            .memory
            .prepare(&prepare_line(&person, &day_name(day), after_id))
            .ok_or(Refused)
            .and_then(|answer| prepared_of(&answer));
        let prepared = match prepared {
            Ok(Some(prepared)) => prepared,
            Ok(None) => {
                self.done(day, &person);
                return self.told(Step::Nothing);
            }
            Err(Refused) => {
                self.done(day, &person);
                return self.told(Step::Refused);
            }
        };
        let system = crate::nightly_prompt::system(self.switches);
        let input = crate::nightly_prompt::user(&prepared.input);
        let (proposal, emptied) = match self.ask(&system, &input, stirs) {
            Ok(asked) => asked,
            Err(step) => return self.told(step),
        };
        let answer =
            self.memory
                .apply(&apply_line(&person, &prepared.job, self.switches, proposal));
        match prepared.more_after.filter(|after| *after > after_id) {
            Some(after) => self.after = Some((day, person, after)),
            None => self.done(day, &person),
        }
        let step = match (emptied, answer.as_ref().and_then(Counts::of)) {
            (Some(emptied), _) => Step::Emptied(emptied),
            (None, Some(counts)) => Step::Applied(Box::new(counts)),
            (None, None) => Step::Unanswered,
        };
        self.told(step)
    }

    /// One model call within the night's bounds: the proposal read (empty
    /// when unread or timed out, and why), or the step that ends it
    /// ([`Step::Spent`], [`Step::Stopped`]).
    fn ask(
        &mut self,
        system: &str,
        input: &str,
        stirs: u64,
    ) -> Result<(Value, Option<Emptied>), Step> {
        let size = (system.len() + input.len()) as u64;
        let reserve = size / BYTES_A_TOKEN + REPLY_TOKENS;
        if self.bytes + size + MOST_REPLY_BYTES as u64 > self.bounds.bytes
            || self.tokens + reserve > self.bounds.tokens
        {
            // Not asked: the job waits in Days, and the day for the next
            // night (within DAYS_BACK).
            self.calls = self.bounds.calls;
            return Err(Step::Spent);
        }
        if self.lamp.stirs() != stirs || !self.may_work() {
            return Err(Step::Stopped);
        }
        self.calls += 1;
        self.bytes += size;
        let (lamp, model, timeout) = (&self.lamp, &self.model, self.bounds.timeout);
        let asked = self.runtime.block_on(async {
            let call = tokio::time::timeout(timeout, model.ask(system, input));
            tokio::pin!(call);
            loop {
                tokio::select! {
                    biased;
                    done = &mut call => break Some(done),
                    () = tokio::time::sleep(STOP_CHECK) => {
                        if lamp.stirs() != stirs || !lamp.open() {
                            // The call in flight is dropped with this
                            // future: cancelled.
                            break None;
                        }
                    }
                }
            }
        });
        let (proposal, emptied) = match asked {
            None => return Err(Step::Stopped),
            Some(Err(_)) => {
                self.tokens += reserve;
                (json!({}), Some(Emptied::TimedOut))
            }
            Some(Ok(reply)) => {
                let (text, tokens) = reply.map_or((String::new(), None), |r| (r.text, r.tokens));
                self.bytes += text.len() as u64;
                self.tokens += tokens.unwrap_or(size + text.len() as u64);
                if self.lamp.stirs() != stirs || !self.lamp.open() {
                    return Err(Step::Stopped);
                }
                match proposal_of(&text) {
                    Some(proposal) => (proposal, None),
                    None => (json!({}), Some(Emptied::Unread)),
                }
            }
        };
        Ok((proposal, emptied))
    }

    /// A week's chapter for `person`: prepared, asked and applied once;
    /// the week is let go whatever came of it.
    fn chapter(&mut self, monday: i64, person: &str) -> Step {
        let stirs = self.lamp.stirs();
        let line = json!({"op":"weekly_prepare","person":person,"week":day_name(monday)});
        let input = match self.memory.weekly(&line) {
            Some(answer) if answer["ok"] == true && answer["reason"] == "nothing" => {
                self.weeks.remove(&(monday, person.to_owned()));
                return self.told(Step::Nothing);
            }
            Some(answer)
                if answer["ok"] == true
                    && answer["input"]["job"]
                        .as_str()
                        .is_some_and(|job| !job.is_empty()) =>
            {
                answer["input"].clone()
            }
            _ => {
                self.weeks.remove(&(monday, person.to_owned()));
                return self.told(Step::Refused);
            }
        };
        let system = crate::nightly_prompt::chapter_system();
        let user = crate::nightly_prompt::chapter_user(&input);
        let (proposal, emptied) = match self.ask(system, &user, stirs) {
            Ok(asked) => asked,
            Err(step) => return self.told(step),
        };
        let applied = self
            .memory
            .weekly(&json!({"op":"weekly_apply","person":person,
            "job":input["job"],"proposal":proposal}));
        self.weeks.remove(&(monday, person.to_owned()));
        let step = match (emptied, applied) {
            (Some(emptied), _) => Step::Emptied(emptied),
            (None, Some(answer)) if answer["ok"] == true => Step::Chapter {
                kept: answer["accepted"] == 1,
                reasons: nonzero(
                    &CHAPTER_REASONS,
                    &CHAPTER_REASONS.map(|key| answer["reasons"][key].as_u64().unwrap_or(0)),
                ),
            },
            (None, _) => Step::Unanswered,
        };
        self.told(step)
    }

    fn told(&self, step: Step) -> Step {
        (self.log)(&format!(
            "nightly: {}; tonight {} of {} calls, {} of {} bytes, {} of {} tokens, {} waiting",
            told(&step),
            self.calls,
            self.bounds.calls,
            self.bytes,
            self.bounds.bytes,
            self.tokens,
            self.bounds.tokens,
            self.pending.len()
        ));
        step
    }

    /// Looks for ever: at once again after a batch applied, else every
    /// [`LOOK_EVERY`].
    pub fn run(mut self) {
        loop {
            let step = self.step();
            if !matches!(
                step,
                Step::Applied(_)
                    | Step::Chapter { .. }
                    | Step::Emptied(_)
                    | Step::Nothing
                    | Step::Unanswered
            ) {
                std::thread::sleep(LOOK_EVERY);
            }
        }
    }
}

/// A step in the journal's words: counts only, no word of the day and no
/// person's id.
fn told(step: &Step) -> String {
    match step {
        Step::Waiting => "waiting".into(),
        Step::Spent => "spent".into(),
        Step::NoWork => "nobody waiting".into(),
        Step::Nothing => "nothing of that day".into(),
        Step::Refused => "the memory refused the prepare; that day let go".into(),
        Step::Applied(counts) => format!("applied, {}", counts.told()),
        Step::Chapter { kept, reasons } => format!(
            "a week's chapter {}, reasons {reasons}",
            if *kept { "kept" } else { "not kept" }
        ),
        Step::Emptied(Emptied::TimedOut) => "the model's call timed out; applied empty".into(),
        Step::Emptied(Emptied::Unread) => "the model's reply did not read; applied empty".into(),
        Step::Unanswered => "applied; the memory's answer did not read".into(),
        Step::Stopped => {
            "stopped: someone spoke or the slide closed; the call dropped, nothing applied".into()
        }
    }
}

/// The local day `day` (days since 1970-01-01) as `YYYY-MM-DD`.
pub fn day_name(day: i64) -> String {
    let (year, month, date) = crate::local_time::civil(day);
    format!("{year:04}-{month:02}-{date:02}")
}

/// The model's text as one JSON object, a code fence around it allowed.
pub fn proposal_of(text: &str) -> Option<Value> {
    let text = text.trim();
    let text = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .and_then(|t| t.strip_suffix("```"))
        .unwrap_or(text)
        .trim();
    serde_json::from_str::<Value>(text)
        .ok()
        .filter(Value::is_object)
}

/// From the voice's environment: with the switch, the memory's socket and
/// the second route's relay, the night's thread is started; otherwise
/// nothing, and why is said once when the switch is set.
pub fn start_from_environment() {
    if !switched_on(std::env::var(ENVIRONMENT).ok().as_deref()) {
        return;
    }
    let said = |why: &str| eprintln!("lamp-voice: nightly off: {why}");
    let Some(socket) = std::env::var_os(crate::remember::ENVIRONMENT)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    else {
        return said("no memory socket (LAMP_MEMORY_SOCKET)");
    };
    let model = match relay_from_environment() {
        Ok(model) => model,
        Err(why) => return said(&why),
    };
    let local = match LocalTime::zone("system") {
        Ok(local) => local,
        Err(error) => return said(&error.to_string()),
    };
    let chapters = switched_on(std::env::var(CHAPTERS_ENVIRONMENT).ok().as_deref());
    // A recall finds the chapters already made even when no night runs.
    CHAPTERS.store(chapters, Ordering::Relaxed);
    start(
        &socket,
        model,
        local,
        Switches::from_environment(),
        chapters,
    );
}

/// The relay's model as the second route's file names it: the relay's own
/// credential, never a person's own key.
pub fn relay_from_environment() -> Result<Relay, String> {
    let config = match crate::second_config::load_from_env().and_then(|config| {
        config
            .map(|c| c.with_environment(|n| std::env::var_os(n)))
            .transpose()
    }) {
        Ok(Some(config)) => config,
        Ok(None) => return Err("no second route (LAMP_VOICE_SECOND_CONFIG)".into()),
        Err(error) => return Err(format!("the second route's file: {error}")),
    };
    relay_of(&config)
}

/// [`relay_from_environment`] for a second route's file already read.
pub fn relay_of(config: &crate::second_config::SecondConfig) -> Result<Relay, String> {
    let Some(base) = config.hosted.deepseek_base_url.clone() else {
        return Err("the second route has no relay (deepseek_base_url)".into());
    };
    crate::second_provider::net::Chat::deepseek_at(
        &base,
        &config.hosted.deepseek_model,
        config.llm_key.expose(),
        Arc::new(Vec::new),
        false,
    )
    .map(Relay)
    .map_err(|error| error.to_string())
}

fn start(socket: &Path, model: Relay, local: LocalTime, switches: Switches, chapters: bool) {
    LAST_STIR_US.store(lamp_ipc::monotonic_us(), Ordering::Relaxed);
    ON.store(true, Ordering::Relaxed);
    let mut night = match Night::new(Socket(socket.to_owned()), Told, model, local) {
        Ok(night) => night,
        Err(error) => {
            ON.store(false, Ordering::Relaxed);
            return eprintln!("lamp-voice: nightly off: {error}");
        }
    };
    night.switches = switches;
    night.chapters = chapters;
    let spawned = std::thread::Builder::new()
        .name("lamp-voice-nightly".into())
        .spawn(move || night.run());
    match spawned {
        Ok(_) => eprintln!(
            "lamp-voice: nightly on: {NIGHT_FROM_HOUR:02}:00 to {NIGHT_UNTIL_HOUR:02}:00, after {} min quiet with the slide open, at most {NIGHT_CALLS} calls, {NIGHT_BYTES} bytes and {NIGHT_TOKENS} tokens a night; patterns {}, feelings {}, weekly chapters {} (LAMP_MEMORY_NIGHTLY)",
            QUIET_FOR.as_secs() / 60,
            if switches.patterns { "on" } else { "off" },
            if switches.feelings { "on" } else { "off" },
            if chapters { "on" } else { "off" },
        ),
        Err(error) => {
            ON.store(false, Ordering::Relaxed);
            eprintln!("lamp-voice: nightly off: {error}");
        }
    }
}

#[cfg(test)]
#[path = "nightly_tests.rs"]
mod tests;
