//! The memory's night, done by the voice (`LAMP_MEMORY_TIERS`, the memory
//! owner's choice on #4988, 3 Nov; the first builder's bounds on #4710).
//!
//! lamp-memory runs with no network, so its tiered store's nightly sleep
//! (`docs/memory-design.md`, "The night, through the voice") is a job it
//! queues: the voice takes it on the memory's socket
//! (`tiers_sleep_take`), asks its relay's model, and gives the reply back
//! (`tiers_sleep_give`), where the tiered store's own validator checks it.
//!
//! Only with `LAMP_MEMORY_TIERS` (`shadow` or `on`) in the voice's
//! environment, the memory's socket, the second route's file and its relay
//! (`deepseek_base_url`): then one thread of its own, which every
//! [`LOOK_EVERY`] does nothing unless all of these hold:
//!
//! - it is the lamp's night ([`NIGHT_FROM_HOUR`] to [`NIGHT_UNTIL_HOUR`],
//!   local), the memory's own night hours;
//! - nobody has spoken or written to the lamp for [`QUIET_FOR`] ([`stir`],
//!   called where the voice hears speech and where it hands a line on);
//! - the privacy slide is open and has been told within [`OPEN_FRESH`]
//!   ([`privacy`], from the privacy gate);
//! - this night's calls, bytes and tokens are under [`NIGHT_CALLS`],
//!   [`NIGHT_BYTES`] and [`NIGHT_TOKENS`].
//!
//! The bounds are this pass's own. The memory's `nightly` pass
//! (`LAMP_MEMORY_NIGHTLY`) shares the night's hours and its quiet and slide
//! gates but keeps its own budget: with both switches set, a night may ask
//! the relay up to twice the calls, bytes and tokens, both through
//! memory's one-at-a-time socket. Which pass runs is the memory owner's
//! call before `LAMP_MEMORY_TIERS` goes on.
//!
//! A call is bounded by [`CALL_TIMEOUT`] and stops at once, the call in
//! flight dropped and its job with it, when anyone speaks, a line is handed
//! on, or the slide closes; its prompt has already been sent by then. No
//! call starts within [`LAST_START`] of 06:00. Memory answers each take or
//! give within its own 100 ms (`busy` past it), so the night never holds
//! memory's socket near a turn's first read; a `busy` give is given again
//! ([`GIVE_AGAIN_AFTER`]), and memory applies it once. A reply the voice cannot read, or a call that
//! timed out, is given back as `null`, which memory drops; one memory's checks refuse is dropped there;
//! a job is given out once and never asked again, so nothing is retried in
//! a loop. Nothing of it is ever said or shown: the module has no speech,
//! ring or bus of its own. Off, none of it runs and [`stir`] and
//! [`privacy`] are one atomic load. The job's words are held only for the
//! call and never journalled: the journal has counts.
use futures_util::{FutureExt, future::LocalBoxFuture};
use lamp_local_time::LocalTime;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

/// The memory's switch, read here too: the night runs only with it set in
/// the voice's unit as well as in lamp-memory's.
pub const ENVIRONMENT: &str = "LAMP_MEMORY_TIERS";
/// The night's hours, local: lamp-memory's own (`tiers::NIGHT_FROM_HOUR`).
pub const NIGHT_FROM_HOUR: u8 = 3;
pub const NIGHT_UNTIL_HOUR: u8 = 6;
/// No line heard or handed on for this long: the lamp is idle. Longer than
/// any conversation's own close, so none is going on.
pub const QUIET_FOR: Duration = Duration::from_secs(20 * 60);
/// The privacy slide counts as open only when told so this recently: a
/// gate that went quiet is taken as closed.
pub const OPEN_FRESH: Duration = Duration::from_secs(1);
/// How often the thread looks whether it may work, when it has nothing in
/// hand.
pub const LOOK_EVERY: Duration = Duration::from_secs(60);
/// The most one model call may take. Below memory's `tiers::JOB_LEASE`.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// A night's calls, bytes (prompt and reply) and tokens at most, for this
/// pass alone (the `nightly` pass has its own; above). The tokens are the relay's own count (`usage`) when it gives one, else one
/// token a byte, counted before a call is made as its prompt's bytes over
/// [`BYTES_A_TOKEN`] plus [`REPLY_TOKENS`].
pub const NIGHT_CALLS: u32 = 16;
pub const NIGHT_BYTES: u64 = 1 << 20;
pub const NIGHT_TOKENS: u64 = 200_000;
pub const BYTES_A_TOKEN: u64 = 3;
pub const REPLY_TOKENS: u64 = 8_192;
/// How long the memory's socket is waited for a take or a give. Memory
/// answers a night op within its own 100 ms bound (`tiers::NIGHT_OP_WAIT`,
/// `busy` past it), so this only covers the socket queued behind a turn's
/// own ops.
pub const MEMORY_WAIT: Duration = Duration::from_secs(3);
/// No call is started this close to the night's end, so none runs past
/// 06:00: a call takes at most [`CALL_TIMEOUT`], and its give follows.
pub const LAST_START: Duration = Duration::from_secs(5 * 60);
/// A give memory answered `busy` (its tiers' thread was slow) is given
/// again this soon, up to [`GIVE_TRIES`] times in all: memory applies a
/// reply once and tells a give of the same job again its outcome. Well
/// inside memory's lease (`tiers::JOB_LEASE`, 120 s).
pub const GIVE_AGAIN_AFTER: Duration = Duration::from_secs(5);
pub const GIVE_TRIES: u32 = 6;
/// A job's answer at most: the tiered store's 64 KiB input and its prompt.
pub const MOST_JOB_BYTES: u64 = 96 * 1024;
/// A model reply at most, the tiered store's own output bound.
pub const MOST_REPLY_BYTES: usize = 32 * 1024;
/// How often a call in flight looks whether it must stop.
const STOP_CHECK: Duration = Duration::from_millis(20);

static ON: AtomicBool = AtomicBool::new(false);
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

/// Whether the switch's value asks for the night: `shadow` or `on`.
pub fn switched_on(value: Option<&str>) -> bool {
    matches!(value, Some("shadow" | "on"))
}

/// What the lamp is doing, as the night reads it.
pub trait Lamp: Send {
    /// Moves on with every stir and every closing of the slide.
    fn stirs(&self) -> u64;
    fn quiet_for(&self) -> Duration;
    /// The slide open, and told so within [`OPEN_FRESH`].
    fn open(&self) -> bool;
}

/// The lamp as [`stir`] and [`privacy`] tell it.
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
}

/// One job of the memory's night: its id, the model's instructions and
/// the input, as memory gives them.
pub struct Job {
    pub id: String,
    pub system: String,
    pub input: String,
}

/// The memory's two night ops.
pub trait Memory: Send {
    fn take(&mut self) -> Option<Job>;
    /// The model's reply (a JSON object, or `null` for one not read);
    /// memory's word for it (`applied`, `invalid`, `stale`, ...).
    fn give(&mut self, id: &str, reply: Value) -> Option<String>;
}

/// The memory on its socket.
pub struct Socket(pub PathBuf);
impl Memory for Socket {
    fn take(&mut self) -> Option<Job> {
        let answer = crate::tools::exchange_up_to(
            &self.0,
            &json!({"op": "tiers_sleep_take"}),
            MEMORY_WAIT,
            MOST_JOB_BYTES,
        )?;
        job_of(&answer)
    }
    fn give(&mut self, id: &str, reply: Value) -> Option<String> {
        let answer = crate::tools::exchange(
            &self.0,
            &json!({"op": "tiers_sleep_give", "job": id, "reply": reply}),
            MEMORY_WAIT,
        )?;
        answer["outcome"].as_str().map(str::to_owned)
    }
}

/// A take's answer read: `{"ok":true,"job":{"id","system","input"}}`.
pub fn job_of(answer: &Value) -> Option<Job> {
    let job = answer.get("job").filter(|job| job.is_object())?;
    Some(Job {
        id: job["id"].as_str()?.to_owned(),
        system: job["system"].as_str()?.to_owned(),
        input: job.get("input").filter(|i| i.is_object())?.to_string(),
    })
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
    /// Memory had no job.
    NoJob,
    /// Given back; memory's word for it.
    Given(String),
    /// Given back, but memory answered `busy` (or not at all): given again
    /// after [`GIVE_AGAIN_AFTER`], before any new job is taken.
    Busy,
    /// Memory stayed busy for every try: the reply dropped.
    GaveUp,
    /// Someone spoke or the slide closed during the call: dropped.
    Stopped,
    TimedOut,
    /// No reply, or none that reads as one JSON object: given back as
    /// `null` and dropped by memory.
    Unread,
}

pub struct Night<M, L, K> {
    pub memory: M,
    pub lamp: L,
    pub model: K,
    pub bounds: Bounds,
    /// Unix ms.
    pub clock: Box<dyn Fn() -> u64 + Send>,
    pub local: LocalTime,
    day: i64,
    calls: u32,
    bytes: u64,
    tokens: u64,
    /// A reply memory answered `busy` to, to give again: the job's id, the
    /// reply, and the tries so far. Held only between those tries.
    held: Option<(String, Value, u32)>,
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
            clock: Box::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis() as u64)
            }),
            local,
            day: i64::MIN,
            calls: 0,
            bytes: 0,
            tokens: 0,
            held: None,
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

    fn may_work(&self) -> bool {
        self.lamp.open() && self.lamp.quiet_for() >= self.bounds.quiet_for
    }

    /// One look: at most one job taken, asked and given back.
    pub fn step(&mut self) -> Step {
        let now = (self.clock)();
        if !(NIGHT_FROM_HOUR..NIGHT_UNTIL_HOUR).contains(&self.local.hour_of_day(now)) {
            return Step::Waiting;
        }
        let day = self.local.day(now);
        if day != self.day {
            self.day = day;
            (self.calls, self.bytes, self.tokens) = (0, 0, 0);
        }
        if !self.may_work() {
            return Step::Waiting;
        }
        if let Some((id, reply, tries)) = self.held.take() {
            let step = self.give(id, reply, tries);
            self.said(&step);
            return step;
        }
        let last_start = now.saturating_add(LAST_START.as_millis() as u64);
        if !(NIGHT_FROM_HOUR..NIGHT_UNTIL_HOUR).contains(&self.local.hour_of_day(last_start)) {
            // Too near 06:00 for a call to end in the night.
            return Step::Waiting;
        }
        if self.calls >= self.bounds.calls
            || self.bytes >= self.bounds.bytes
            || self.tokens >= self.bounds.tokens
        {
            return Step::Spent;
        }
        let stirs = self.lamp.stirs();
        let Some(job) = self.memory.take() else {
            return Step::NoJob;
        };
        let size = (job.system.len() + job.input.len()) as u64;
        let reserve = size / BYTES_A_TOKEN + REPLY_TOKENS;
        if self.bytes + size + MOST_REPLY_BYTES as u64 > self.bounds.bytes
            || self.tokens + reserve > self.bounds.tokens
        {
            // Not asked: memory drops it at its lease's end, and its
            // records wait for the next night. Said once: the night is
            // spent from here.
            self.calls = self.bounds.calls;
            (self.log)(&format!(
                "tiers night: a job of {size} bytes ({reserve} tokens reserved) is more than tonight has left; spent at {} of {} bytes, {} of {} tokens",
                self.bytes, self.bounds.bytes, self.tokens, self.bounds.tokens
            ));
            return Step::Spent;
        }
        if self.lamp.stirs() != stirs || !self.may_work() {
            return Step::Stopped;
        }
        self.calls += 1;
        self.bytes += size;
        let (lamp, model, timeout) = (&self.lamp, &self.model, self.bounds.timeout);
        let asked = self.runtime.block_on(async {
            let call = tokio::time::timeout(timeout, model.ask(&job.system, &job.input));
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
        let step = match asked {
            None => {
                // Cut in flight: the relay's own count (`usage`) comes only
                // at the stream's end, so this call's tokens are unknown.
                // Its prompt may still be billed: the reserve is counted,
                // as for a call that timed out.
                self.tokens += reserve;
                Step::Stopped
            }
            Some(Err(_)) => {
                self.tokens += reserve;
                let _ = self.memory.give(&job.id, Value::Null);
                Step::TimedOut
            }
            Some(Ok(reply)) => {
                let (text, tokens) = reply.map_or((String::new(), None), |r| (r.text, r.tokens));
                self.bytes += text.len() as u64;
                self.tokens += tokens.unwrap_or(size + text.len() as u64);
                if self.lamp.stirs() != stirs || !self.lamp.open() {
                    Step::Stopped
                } else {
                    match reply_of(&text) {
                        Some(reply) => self.give(job.id, reply, 0),
                        None => {
                            let _ = self.memory.give(&job.id, Value::Null);
                            Step::Unread
                        }
                    }
                }
            }
        };
        self.said(&step);
        step
    }

    /// A model's reply given to memory; `busy` (or no answer) holds it to
    /// give again, at most [`GIVE_TRIES`] times. Memory applies it once
    /// whatever the tries.
    fn give(&mut self, id: String, reply: Value, tries: u32) -> Step {
        match self.memory.give(&id, reply.clone()) {
            Some(outcome) if outcome != "busy" => Step::Given(outcome),
            _ if tries + 1 < GIVE_TRIES => {
                self.held = Some((id, reply, tries + 1));
                Step::Busy
            }
            // Memory drops the job at its lease's end; its records wait
            // for the next night.
            _ => Step::GaveUp,
        }
    }

    fn said(&self, step: &Step) {
        (self.log)(&format!(
            "tiers night: {}; tonight {} of {} calls, {} of {} bytes, {} of {} tokens",
            told(step),
            self.calls,
            self.bounds.calls,
            self.bytes,
            self.bounds.bytes,
            self.tokens,
            self.bounds.tokens
        ));
    }

    /// Looks for ever: at once again after a job given back, after
    /// [`GIVE_AGAIN_AFTER`] when memory was busy, else every [`LOOK_EVERY`].
    pub fn run(mut self) {
        loop {
            match self.step() {
                Step::Given(_) => {}
                Step::Busy => std::thread::sleep(GIVE_AGAIN_AFTER),
                _ => std::thread::sleep(LOOK_EVERY),
            }
        }
    }
}

/// A step in the journal's words: no word of the job.
fn told(step: &Step) -> String {
    match step {
        Step::Waiting => "waiting".into(),
        Step::Spent => "spent".into(),
        Step::NoJob => "no job".into(),
        Step::Given(outcome) => format!("given back, memory says {outcome}"),
        Step::Busy => "given back, memory busy; given again soon".into(),
        Step::GaveUp => "memory stayed busy; the reply dropped".into(),
        Step::Stopped => "stopped: someone spoke or the slide closed; the job dropped".into(),
        Step::TimedOut => "the model's call timed out; the job dropped".into(),
        Step::Unread => "the model's reply did not read; dropped".into(),
    }
}

/// The model's text as one JSON object, a code fence around it allowed.
pub fn reply_of(text: &str) -> Option<Value> {
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
    let said = |why: &str| eprintln!("lamp-voice: tiers night off: {why}");
    let Some(socket) = std::env::var_os(crate::remember::ENVIRONMENT)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    else {
        return said("no memory socket (LAMP_MEMORY_SOCKET)");
    };
    // The relay's own credential, never a person's own key: the `nightly`
    // pass's own reading of the second route's file.
    let chat = match crate::nightly::relay_from_environment() {
        Ok(relay) => relay.0,
        Err(error) => return said(&error),
    };
    let local = match LocalTime::zone("system") {
        Ok(local) => local,
        Err(error) => return said(&error.to_string()),
    };
    start(&socket, Relay(chat), local);
}

fn start(socket: &Path, model: Relay, local: LocalTime) {
    LAST_STIR_US.store(lamp_ipc::monotonic_us(), Ordering::Relaxed);
    ON.store(true, Ordering::Relaxed);
    let night = match Night::new(Socket(socket.to_owned()), Told, model, local) {
        Ok(night) => night,
        Err(error) => {
            ON.store(false, Ordering::Relaxed);
            return eprintln!("lamp-voice: tiers night off: {error}");
        }
    };
    let spawned = std::thread::Builder::new()
        .name("lamp-voice-tiers-night".into())
        .spawn(move || night.run());
    match spawned {
        Ok(_) => eprintln!(
            "lamp-voice: tiers night on: {NIGHT_FROM_HOUR:02}:00 to {NIGHT_UNTIL_HOUR:02}:00, after {} min quiet with the slide open, at most {NIGHT_CALLS} calls, {NIGHT_BYTES} bytes and {NIGHT_TOKENS} tokens a night (LAMP_MEMORY_TIERS)",
            QUIET_FOR.as_secs() / 60
        ),
        Err(error) => {
            ON.store(false, Ordering::Relaxed);
            eprintln!("lamp-voice: tiers night off: {error}");
        }
    }
}

#[cfg(test)]
#[path = "tiers_night_tests.rs"]
mod tests;
