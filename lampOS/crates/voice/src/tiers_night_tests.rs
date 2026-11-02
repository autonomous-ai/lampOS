//! The voice's night with a fake memory, lamp and model: it runs only at
//! night, quiet and with the slide open; speech or the slide stop a call in
//! flight; the caps, the timeout and an unread reply each end it; and
//! nothing is retried or said.
use super::*;
use std::{
    collections::VecDeque,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize},
    },
};

/// 2026-10-14 03:30 UTC.
const NIGHT_MS: u64 = 1_791_892_800_000 + 15 * 3_600_000 + 1_800_000;
/// 2026-10-13 12:00 UTC.
const NOON_MS: u64 = 1_791_892_800_000;

#[derive(Clone, Default)]
struct FakeLamp {
    stirs: Arc<AtomicU64>,
    loud: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
}
impl Lamp for FakeLamp {
    fn stirs(&self) -> u64 {
        self.stirs.load(Ordering::SeqCst)
    }
    fn quiet_for(&self) -> Duration {
        if self.loud.load(Ordering::SeqCst) {
            Duration::ZERO
        } else {
            QUIET_FOR
        }
    }
    fn open(&self) -> bool {
        !self.closed.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Default)]
struct FakeMemory {
    jobs: Arc<Mutex<VecDeque<Job>>>,
    takes: Arc<AtomicUsize>,
    given: Arc<Mutex<Vec<(String, Value)>>>,
    /// Gives answered `busy` before memory answers.
    busy: Arc<AtomicUsize>,
}
impl FakeMemory {
    fn with_jobs(n: usize, input_bytes: usize) -> Self {
        let memory = Self::default();
        for i in 0..n {
            memory.jobs.lock().unwrap().push_back(Job {
                id: format!("job{i}"),
                system: "propose".into(),
                input: format!("{{\"records\":\"{}\"}}", "x".repeat(input_bytes)),
            });
        }
        memory
    }
    fn given(&self) -> Vec<(String, Value)> {
        self.given.lock().unwrap().clone()
    }
}
impl Memory for FakeMemory {
    fn take(&mut self) -> Option<Job> {
        self.takes.fetch_add(1, Ordering::SeqCst);
        self.jobs.lock().unwrap().pop_front()
    }
    fn give(&mut self, id: &str, reply: Value) -> Option<String> {
        self.given
            .lock()
            .unwrap()
            .push((id.to_owned(), reply.clone()));
        if self.busy.load(Ordering::SeqCst) > 0 {
            self.busy.fetch_sub(1, Ordering::SeqCst);
            return Some("busy".into());
        }
        Some(
            if reply.is_object() {
                "applied"
            } else {
                "invalid"
            }
            .into(),
        )
    }
}

/// Answers after `delay`, with `text` and `tokens`; `during` runs as the
/// call starts (to speak, or close the slide); `dropped` is set when a call
/// in flight is cancelled.
struct FakeModel {
    delay: Duration,
    text: String,
    tokens: Option<u64>,
    calls: Arc<AtomicUsize>,
    during: Option<Box<dyn Fn() + Send>>,
    dropped: Arc<AtomicBool>,
}
impl FakeModel {
    fn answering(text: &str) -> Self {
        Self {
            delay: Duration::from_millis(5),
            text: text.into(),
            tokens: None,
            calls: Arc::default(),
            during: None,
            dropped: Arc::default(),
        }
    }
}
struct Guard(Arc<AtomicBool>, bool);
impl Drop for Guard {
    fn drop(&mut self) {
        if !self.1 {
            self.0.store(true, Ordering::SeqCst);
        }
    }
}
impl Model for FakeModel {
    fn ask<'a>(&'a self, _system: &'a str, _input: &'a str) -> Asked<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(during) = &self.during {
            during();
        }
        async move {
            let mut guard = Guard(self.dropped.clone(), false);
            tokio::time::sleep(self.delay).await;
            guard.1 = true;
            Some(Reply {
                text: self.text.clone(),
                tokens: self.tokens,
            })
        }
        .boxed_local()
    }
}

const REPLY: &str = r#"{"facts":[],"lines":[],"episodes":[]}"#;

type FakeNight = Night<FakeMemory, FakeLamp, FakeModel>;
type Logged = Arc<Mutex<Vec<String>>>;

fn night(memory: FakeMemory, lamp: FakeLamp, model: FakeModel, at: u64) -> (FakeNight, Logged) {
    let mut night = Night::new(memory, lamp, model, LocalTime::Offset(0)).unwrap();
    night.clock = Box::new(move || at);
    let logged = Arc::new(Mutex::new(Vec::new()));
    let into = logged.clone();
    night.log = Arc::new(move |line| into.lock().unwrap().push(line.to_owned()));
    (night, logged)
}

#[test]
fn an_idle_open_night_takes_a_job_asks_and_gives_the_reply_back() {
    let memory = FakeMemory::with_jobs(1, 10);
    let (mut night, logged) = night(
        memory.clone(),
        FakeLamp::default(),
        FakeModel::answering(&format!("```json\n{REPLY}\n```")),
        NIGHT_MS,
    );
    assert_eq!(night.step(), Step::Given("applied".into()));
    let given = memory.given();
    assert_eq!(given.len(), 1);
    assert_eq!(given[0].0, "job0");
    assert_eq!(given[0].1, serde_json::from_str::<Value>(REPLY).unwrap());
    assert_eq!(night.step(), Step::NoJob);
    // The journal has counts, never the job's words.
    let logged = logged.lock().unwrap();
    assert!(logged[0].contains("tokens"), "{logged:?}");
    assert!(
        logged
            .iter()
            .all(|l| !l.contains("xxxx") && !l.contains("propose"))
    );
}

#[test]
fn by_day_while_loud_or_with_the_slide_closed_nothing_is_taken() {
    let memory = FakeMemory::with_jobs(1, 10);
    let (mut day, _) = night(
        memory.clone(),
        FakeLamp::default(),
        FakeModel::answering(REPLY),
        NOON_MS,
    );
    assert_eq!(day.step(), Step::Waiting);
    let loud = FakeLamp::default();
    loud.loud.store(true, Ordering::SeqCst);
    let (mut talking, _) = night(memory.clone(), loud, FakeModel::answering(REPLY), NIGHT_MS);
    assert_eq!(talking.step(), Step::Waiting);
    let private = FakeLamp::default();
    private.closed.store(true, Ordering::SeqCst);
    let (mut slid, _) = night(
        memory.clone(),
        private,
        FakeModel::answering(REPLY),
        NIGHT_MS,
    );
    assert_eq!(slid.step(), Step::Waiting);
    assert_eq!(memory.takes.load(Ordering::SeqCst), 0);
}

fn stopped_by(during: impl Fn(&FakeLamp) + Send + 'static) {
    let memory = FakeMemory::with_jobs(1, 10);
    let lamp = FakeLamp::default();
    let mut model = FakeModel::answering(REPLY);
    model.delay = Duration::from_secs(30);
    let seen = lamp.clone();
    model.during = Some(Box::new(move || during(&seen)));
    let dropped = model.dropped.clone();
    let (mut night, _) = night(memory.clone(), lamp, model, NIGHT_MS);
    let started = std::time::Instant::now();
    assert_eq!(night.step(), Step::Stopped);
    // At once: the 30 s call was cancelled in flight, nothing given back.
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(dropped.load(Ordering::SeqCst));
    assert!(memory.given().is_empty());
    // Its tokens unknown (the relay counts at the stream's end): the
    // reserve is counted, as for a timeout.
    let size = ("propose".len() + "{\"records\":\"\"}".len() + 10) as u64;
    assert_eq!(night.spent().2, size / BYTES_A_TOKEN + REPLY_TOKENS);
}

#[test]
fn speech_during_a_call_stops_it_at_once_and_drops_the_job() {
    stopped_by(|lamp| {
        lamp.stirs.fetch_add(1, Ordering::SeqCst);
    });
}

#[test]
fn the_privacy_slide_during_a_call_stops_it_at_once() {
    stopped_by(|lamp| lamp.closed.store(true, Ordering::SeqCst));
}

#[test]
fn a_night_stops_at_its_call_cap() {
    let memory = FakeMemory::with_jobs(5, 10);
    let model = FakeModel::answering(REPLY);
    let calls = model.calls.clone();
    let (mut night, _) = night(memory.clone(), FakeLamp::default(), model, NIGHT_MS);
    night.bounds.calls = 2;
    assert!(matches!(night.step(), Step::Given(_)));
    assert!(matches!(night.step(), Step::Given(_)));
    assert_eq!(night.step(), Step::Spent);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(memory.takes.load(Ordering::SeqCst), 2);
}

#[test]
fn a_night_stops_at_its_token_cap_counted_from_the_relays_usage() {
    let memory = FakeMemory::with_jobs(5, 10);
    let mut model = FakeModel::answering(REPLY);
    model.tokens = Some(60_000);
    let (mut night, logged) = night(memory, FakeLamp::default(), model, NIGHT_MS);
    night.bounds.tokens = 125_000;
    assert!(matches!(night.step(), Step::Given(_)));
    assert!(matches!(night.step(), Step::Given(_)));
    assert_eq!(night.spent().2, 120_000);
    // The next call's reserve no longer fits: not asked.
    assert_eq!(night.step(), Step::Spent);
    assert_eq!(night.step(), Step::Spent);
    assert!(
        logged
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.contains("120000 of 125000 tokens"))
    );
}

#[test]
fn a_job_too_big_for_what_is_left_of_the_bytes_is_not_asked() {
    let memory = FakeMemory::with_jobs(1, 10_000);
    let model = FakeModel::answering(REPLY);
    let calls = model.calls.clone();
    let (mut night, logged) = night(memory, FakeLamp::default(), model, NIGHT_MS);
    night.bounds.bytes = 20_000;
    assert_eq!(night.step(), Step::Spent);
    assert_eq!(night.step(), Step::Spent);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // One line, counts only: no word of the job.
    let logged = logged.lock().unwrap();
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert!(
        logged[0].contains("is more than tonight has left") && logged[0].contains("of 20000 bytes"),
        "{logged:?}"
    );
    assert!(!logged[0].contains("xxx") && !logged[0].contains("propose"));
}

#[test]
fn a_call_past_its_timeout_is_dropped_and_given_back_empty() {
    let memory = FakeMemory::with_jobs(1, 10);
    let mut model = FakeModel::answering(REPLY);
    model.delay = Duration::from_secs(30);
    let (mut night, _) = night(memory.clone(), FakeLamp::default(), model, NIGHT_MS);
    night.bounds.timeout = Duration::from_millis(100);
    assert_eq!(night.step(), Step::TimedOut);
    assert_eq!(memory.given(), vec![("job0".to_owned(), Value::Null)]);
}

#[test]
fn an_unread_reply_is_given_back_empty_once_and_never_asked_again() {
    let memory = FakeMemory::with_jobs(1, 10);
    let model = FakeModel::answering("I think they like tea.");
    let calls = model.calls.clone();
    let (mut night, _) = night(memory.clone(), FakeLamp::default(), model, NIGHT_MS);
    assert_eq!(night.step(), Step::Unread);
    assert_eq!(night.step(), Step::NoJob);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(memory.given(), vec![("job0".to_owned(), Value::Null)]);
}

#[test]
fn off_the_switch_nothing_starts_and_the_signals_do_nothing() {
    for value in [None, Some(""), Some("off"), Some("yes")] {
        assert!(!switched_on(value));
    }
    assert!(switched_on(Some("shadow")) && switched_on(Some("on")));
    // No test starts the night: stir and privacy leave it as it was.
    let before = STIRS.load(Ordering::Relaxed);
    stir();
    privacy(false);
    privacy(true);
    assert_eq!(STIRS.load(Ordering::Relaxed), before);
    assert_eq!(OPEN_AT_US.load(Ordering::Relaxed), 0);
}

#[test]
fn the_reply_is_read_only_as_one_json_object() {
    assert!(reply_of(REPLY).is_some());
    assert!(reply_of(&format!("```\n{REPLY}\n```")).is_some());
    assert!(reply_of("[1,2]").is_none());
    assert!(reply_of("sure! {}").is_none());
}

/// Nothing of the night is ever said or shown: its code names no speech,
/// ring, expression or bus.
#[test]
fn nothing_of_the_night_is_said_or_shown() {
    let source = include_str!("tiers_night.rs");
    let code = source.split("#[cfg(test)]").next().unwrap();
    for part in [
        "crate::say",
        "crate::ring",
        "bus_out",
        "expression_out",
        "lamp_bus",
        "playback",
    ] {
        assert!(!code.contains(part), "{part}");
    }
}

#[test]
fn a_give_memory_was_busy_for_is_given_again_before_any_new_job() {
    let memory = FakeMemory::with_jobs(2, 10);
    memory.busy.store(2, Ordering::SeqCst);
    let model = FakeModel::answering(REPLY);
    let calls = model.calls.clone();
    let (mut night, _) = night(memory.clone(), FakeLamp::default(), model, NIGHT_MS);
    assert_eq!(night.step(), Step::Busy);
    assert_eq!(night.step(), Step::Busy);
    assert_eq!(night.step(), Step::Given("applied".into()));
    // The same job given three times, one call, one take, before the next.
    let ids: Vec<String> = memory.given().into_iter().map(|(id, _)| id).collect();
    assert_eq!(ids, ["job0", "job0", "job0"]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(memory.takes.load(Ordering::SeqCst), 1);
    assert!(matches!(night.step(), Step::Given(_)));
    assert_eq!(memory.takes.load(Ordering::SeqCst), 2);
}

#[test]
fn a_give_memory_stays_busy_for_is_dropped_after_its_tries() {
    let memory = FakeMemory::with_jobs(1, 10);
    memory.busy.store(100, Ordering::SeqCst);
    let (mut night, _) = night(
        memory.clone(),
        FakeLamp::default(),
        FakeModel::answering(REPLY),
        NIGHT_MS,
    );
    for _ in 1..GIVE_TRIES {
        assert_eq!(night.step(), Step::Busy);
    }
    assert_eq!(night.step(), Step::GaveUp);
    assert_eq!(memory.given().len(), GIVE_TRIES as usize);
    assert_eq!(night.step(), Step::NoJob);
}

#[test]
fn no_call_starts_within_its_last_start_of_six() {
    let memory = FakeMemory::with_jobs(1, 10);
    // 05:56, the night's last minutes.
    let at = NOON_MS + 17 * 3_600_000 + 56 * 60_000;
    let (mut night, _) = night(
        memory.clone(),
        FakeLamp::default(),
        FakeModel::answering(REPLY),
        at,
    );
    assert_eq!(night.step(), Step::Waiting);
    assert_eq!(memory.takes.load(Ordering::SeqCst), 0);
}
