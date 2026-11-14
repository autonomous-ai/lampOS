//! The voice's nightly pass with a fake Days, lamp and model, whose
//! answers are fixed as the contract has them (the memory side is not
//! merged yet): it runs only at night, quiet, with the slide open, for
//! known people on earlier days; speech or the slide stop a call in
//! flight; the caps, the timeout and an unread reply each end it; and the
//! journal has counts only.
use super::*;
use std::{
    collections::VecDeque,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize},
    },
};

/// 2026-10-14 03:30 UTC.
const NIGHT_MS: u64 = 1_791_892_800_000 + 15 * 3_600_000 + 1_800_000;
/// 2026-10-13 12:00 UTC.
const NOON_MS: u64 = 1_791_892_800_000;
/// 2026-10-13 15:00 UTC: yesterday, as the night of NIGHT_MS sees it.
const YESTERDAY_MS: u64 = NOON_MS + 3 * 3_600_000;
const ANNA: &str = "p-0123456789abcdef";
const BINH: &str = "p-fedcba9876543210";
/// Words of the day that must never reach the journal.
const SECRET: &str = "jasmine-tea-secret";

#[derive(Clone, Default)]
struct FakeLamp {
    stirs: Arc<AtomicU64>,
    loud: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    /// People already noted as heard, given to the night as it is made.
    heard: Arc<Mutex<Vec<(String, u64)>>>,
    /// Who the eyes see at the lamp.
    person: Arc<Mutex<Option<String>>>,
}
impl FakeLamp {
    fn hearing(people: &[(&str, u64)]) -> Self {
        let lamp = Self::default();
        *lamp.heard.lock().unwrap() = people
            .iter()
            .map(|(p, at)| ((*p).to_owned(), *at))
            .collect();
        lamp
    }
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
    fn at_lamp(&self) -> Option<String> {
        self.person.lock().unwrap().clone()
    }
}

/// Days' answers, fixed as the contract has them.
#[derive(Clone, Default)]
struct FakeMemory {
    /// Prepare answers in order; when empty, `"reason":"nothing"`.
    prepares: Arc<Mutex<VecDeque<Value>>>,
    prepared: Arc<Mutex<Vec<Value>>>,
    applied: Arc<Mutex<Vec<Value>>>,
    refuse: Arc<AtomicBool>,
    /// Weekly answers in order; when empty, `"reason":"nothing"`.
    weeklies: Arc<Mutex<VecDeque<Value>>>,
    weekly_lines: Arc<Mutex<Vec<Value>>>,
}
impl FakeMemory {
    fn with(answers: Vec<Value>) -> Self {
        let memory = Self::default();
        *memory.prepares.lock().unwrap() = answers.into();
        memory
    }
    fn prepared(&self) -> Vec<Value> {
        self.prepared.lock().unwrap().clone()
    }
    fn applied(&self) -> Vec<Value> {
        self.applied.lock().unwrap().clone()
    }
    fn weekly_lines(&self) -> Vec<Value> {
        self.weekly_lines.lock().unwrap().clone()
    }
}
impl Memory for FakeMemory {
    fn prepare(&mut self, line: &Value) -> Option<Value> {
        self.prepared.lock().unwrap().push(line.clone());
        if self.refuse.load(Ordering::SeqCst) {
            return Some(json!({"ok":false,"error":"bad_person"}));
        }
        Some(
            self.prepares
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| json!({"ok":true,"reason":"nothing"})),
        )
    }
    fn apply(&mut self, line: &Value) -> Option<Value> {
        self.applied.lock().unwrap().push(line.clone());
        Some(json!({"ok":true,
            "accepted":{"episode":1,"fact":2,"profile":0,"pattern":0,"ahead":1,"feeling":0},
            "dropped":{"episode":0,"fact":1,"profile":0,"pattern":1,"ahead":0,"feeling":0},
            "reasons":{"lamp_words":1,"off":1,"made_up_reason":7},
            "ids":[88,89,90,91]}))
    }
    fn weekly(&mut self, line: &Value) -> Option<Value> {
        self.weekly_lines.lock().unwrap().push(line.clone());
        Some(
            self.weeklies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| json!({"ok":true,"reason":"nothing"})),
        )
    }
}

/// A prepare's answer for `person`: job `job`, records `first..=last`,
/// `more` as given.
fn input(person: &str, job: &str, first: u64, last: u64, more: bool) -> Value {
    let records: Vec<Value> = (first..=last)
        .map(|id| json!({"id":id,"at_unix_ms":YESTERDAY_MS,"kind":"exchange","heard":SECRET,"said":"ok","text":""}))
        .collect();
    json!({"ok":true,"input":{"job":job,"person":person,"day":"2026-10-13","generation":1,
        "records":records,"existing":[],"profile_lines":0,"profile_changes_left":0,
        "profile_adds_left":15,"more":more}})
}

/// Answers after `delay` with `text` and `tokens`; `during` runs as the
/// call starts; `dropped` is set when a call in flight is cancelled.
struct FakeModel {
    delay: Duration,
    text: String,
    tokens: Option<u64>,
    calls: Arc<AtomicUsize>,
    asked: Arc<Mutex<Vec<(String, String)>>>,
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
            asked: Arc::default(),
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
    fn ask<'a>(&'a self, system: &'a str, input: &'a str) -> Asked<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.asked
            .lock()
            .unwrap()
            .push((system.to_owned(), input.to_owned()));
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

const PROPOSAL: &str = r#"{"episodes":[{"first":1,"last":2,"topic":"tea","gist":"talked tea"}],"facts":[{"kind":"taste","claim":"likes tea","cites":[1]}]}"#;

type FakeNight = Night<FakeMemory, FakeLamp, FakeModel>;
type Logged = Arc<Mutex<Vec<String>>>;

fn night(memory: FakeMemory, lamp: FakeLamp, model: FakeModel, at: u64) -> (FakeNight, Logged) {
    let heard = std::mem::take(&mut *lamp.heard.lock().unwrap());
    let mut night = Night::new(memory, lamp, model, LocalTime::Offset(0)).unwrap();
    for (person, at) in heard {
        night.note(&person, at);
    }
    night.clock = Box::new(move || at);
    let logged = Arc::new(Mutex::new(Vec::new()));
    let into = logged.clone();
    night.log = Arc::new(move |line| into.lock().unwrap().push(line.to_owned()));
    (night, logged)
}

fn anna_yesterday() -> FakeLamp {
    FakeLamp::hearing(&[(ANNA, YESTERDAY_MS)])
}

#[test]
fn an_idle_open_night_prepares_asks_and_applies_with_counts_only_in_the_journal() {
    let memory = FakeMemory::with(vec![input(ANNA, "n1", 1, 2, false)]);
    let model = FakeModel::answering(&format!("```json\n{PROPOSAL}\n```"));
    let asked = model.asked.clone();
    let (mut night, logged) = night(memory.clone(), anna_yesterday(), model, NIGHT_MS);
    let Step::Applied(counts) = night.step() else {
        panic!("not applied");
    };
    assert_eq!(counts.accepted, [1, 2, 0, 0, 1, 0]);
    assert_eq!(counts.dropped, [0, 1, 0, 1, 0, 0]);
    assert_eq!(
        night.memory.prepared(),
        vec![json!({"op":"nightly_prepare","person":ANNA,"day":"2026-10-13","after_id":0})]
    );
    let applied = memory.applied();
    assert_eq!(applied.len(), 1);
    assert_eq!(
        applied[0],
        json!({"op":"nightly_apply","person":ANNA,"job":"n1","patterns":false,"feelings":false,
            "proposal":serde_json::from_str::<Value>(PROPOSAL).unwrap()})
    );
    // The model had the prompt and the day's input as quoted data.
    let asked = asked.lock().unwrap();
    assert!(asked[0].0.contains("Point at record ids"));
    assert!(asked[0].1.contains("<data>") && asked[0].1.contains(SECRET));
    assert_eq!(night.step(), Step::NoWork);
    // The journal has counts, never the day's words, the proposal or who.
    let logged = logged.lock().unwrap();
    assert!(
        logged[0].contains("accepted 4 (episode 1, fact 2, ahead 1), dropped 2 (fact 1, pattern 1), reasons lamp_words 1, off 1"),
        "{logged:?}"
    );
    for line in logged.iter() {
        for word in [
            SECRET,
            "likes tea",
            "talked tea",
            ANNA,
            "made_up_reason",
            "n1",
        ] {
            assert!(!line.contains(word), "{word} in {line}");
        }
    }
}

#[test]
fn by_day_while_loud_or_with_the_slide_closed_nothing_is_prepared() {
    let memory = FakeMemory::with(vec![input(ANNA, "n1", 1, 2, false)]);
    let (mut day, _) = night(
        memory.clone(),
        anna_yesterday(),
        FakeModel::answering(PROPOSAL),
        NOON_MS + 86_400_000,
    );
    assert_eq!(day.step(), Step::Waiting);
    let loud = anna_yesterday();
    loud.loud.store(true, Ordering::SeqCst);
    let (mut talking, _) = night(
        memory.clone(),
        loud,
        FakeModel::answering(PROPOSAL),
        NIGHT_MS,
    );
    assert_eq!(talking.step(), Step::Waiting);
    let private = anna_yesterday();
    private.closed.store(true, Ordering::SeqCst);
    let (mut slid, _) = night(
        memory.clone(),
        private,
        FakeModel::answering(PROPOSAL),
        NIGHT_MS,
    );
    assert_eq!(slid.step(), Step::Waiting);
    assert!(memory.prepared().is_empty());
}

#[test]
fn only_known_people_on_earlier_days_each_on_their_own() {
    let memory = FakeMemory::with(vec![
        input(ANNA, "n1", 1, 2, false),
        input(BINH, "n2", 5, 6, false),
    ]);
    let lamp = FakeLamp::hearing(&[
        (ANNA, YESTERDAY_MS),
        ("everyone", YESTERDAY_MS),
        ("r-household", YESTERDAY_MS),
        ("guest", YESTERDAY_MS),
        // Heard today: waits for tomorrow night.
        (BINH, NIGHT_MS - 60_000),
        (BINH, YESTERDAY_MS),
    ]);
    let (mut night, _) = night(
        memory.clone(),
        lamp,
        FakeModel::answering(PROPOSAL),
        NIGHT_MS,
    );
    assert!(matches!(night.step(), Step::Applied(_)));
    assert!(matches!(night.step(), Step::Applied(_)));
    assert_eq!(night.step(), Step::NoWork);
    let people: Vec<Value> = memory
        .prepared()
        .iter()
        .map(|line| line["person"].clone())
        .collect();
    assert_eq!(people.len(), 2);
    assert!(people.contains(&json!(ANNA)) && people.contains(&json!(BINH)));
    // Each apply is its own person's job.
    let applied = memory.applied();
    for line in &applied {
        let job = if line["person"] == ANNA { "n1" } else { "n2" };
        assert_eq!(line["job"], job);
    }
    // Today's day for Binh is still waiting.
    assert_eq!(night.waiting(), 1);
}

#[test]
fn a_day_with_more_records_is_asked_in_batches_after_the_last_id() {
    let memory = FakeMemory::with(vec![
        input(ANNA, "n1", 1, 128, true),
        input(ANNA, "n2", 130, 140, false),
    ]);
    let (mut night, _) = night(
        memory.clone(),
        anna_yesterday(),
        FakeModel::answering(PROPOSAL),
        NIGHT_MS,
    );
    assert!(matches!(night.step(), Step::Applied(_)));
    assert!(matches!(night.step(), Step::Applied(_)));
    assert_eq!(night.step(), Step::NoWork);
    let afters: Vec<Value> = memory
        .prepared()
        .iter()
        .map(|l| l["after_id"].clone())
        .collect();
    assert_eq!(afters, vec![json!(0), json!(128)]);
}

#[test]
fn nothing_or_a_refusal_lets_the_day_go_without_a_call() {
    let memory = FakeMemory::default();
    let model = FakeModel::answering(PROPOSAL);
    let calls = model.calls.clone();
    let (mut night, _) = night(memory.clone(), anna_yesterday(), model, NIGHT_MS);
    assert_eq!(night.step(), Step::Nothing);
    assert_eq!(night.step(), Step::NoWork);
    let refusing = FakeMemory::default();
    refusing.refuse.store(true, Ordering::SeqCst);
    let (mut refused, _) = night_with(refusing.clone());
    assert_eq!(refused.step(), Step::Refused);
    assert_eq!(refused.step(), Step::NoWork);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(memory.applied().is_empty() && refusing.applied().is_empty());
}

fn night_with(memory: FakeMemory) -> (FakeNight, Logged) {
    night(
        memory,
        anna_yesterday(),
        FakeModel::answering(PROPOSAL),
        NIGHT_MS,
    )
}

fn stopped_by(during: impl Fn(&FakeLamp) + Send + 'static) {
    let memory = FakeMemory::with(vec![input(ANNA, "n1", 1, 2, false)]);
    let lamp = anna_yesterday();
    let mut model = FakeModel::answering(PROPOSAL);
    model.delay = Duration::from_secs(30);
    let seen = lamp.clone();
    model.during = Some(Box::new(move || during(&seen)));
    let dropped = model.dropped.clone();
    let (mut night, _) = night(memory.clone(), lamp, model, NIGHT_MS);
    let started = std::time::Instant::now();
    assert_eq!(night.step(), Step::Stopped);
    // At once: the 30 s call was cancelled in flight, nothing applied, and
    // the day still waits for the next quiet.
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(dropped.load(Ordering::SeqCst));
    assert!(memory.applied().is_empty());
    assert_eq!(night.waiting(), 1);
}

#[test]
fn speech_during_a_call_stops_it_at_once_and_applies_nothing() {
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
    let memory = FakeMemory::with(vec![
        input(ANNA, "n1", 1, 2, true),
        input(ANNA, "n2", 3, 4, true),
        input(ANNA, "n3", 5, 6, false),
    ]);
    let model = FakeModel::answering(PROPOSAL);
    let calls = model.calls.clone();
    let (mut night, _) = night(memory.clone(), anna_yesterday(), model, NIGHT_MS);
    night.bounds.calls = 2;
    assert!(matches!(night.step(), Step::Applied(_)));
    assert!(matches!(night.step(), Step::Applied(_)));
    assert_eq!(night.step(), Step::Spent);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(memory.prepared().len(), 2);
}

#[test]
fn a_night_stops_at_its_token_cap_counted_from_the_relays_usage() {
    let memory = FakeMemory::with(
        (0..5)
            .map(|n| input(ANNA, &format!("n{n}"), n * 2 + 1, n * 2 + 2, true))
            .collect(),
    );
    let mut model = FakeModel::answering(PROPOSAL);
    model.tokens = Some(60_000);
    let (mut night, logged) = night(memory, anna_yesterday(), model, NIGHT_MS);
    night.bounds.tokens = 125_000;
    assert!(matches!(night.step(), Step::Applied(_)));
    assert!(matches!(night.step(), Step::Applied(_)));
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
fn a_batch_too_big_for_what_is_left_of_the_bytes_is_not_asked() {
    let memory = FakeMemory::with(vec![input(ANNA, "n1", 1, 400, false)]);
    let model = FakeModel::answering(PROPOSAL);
    let calls = model.calls.clone();
    let (mut night, _) = night(memory.clone(), anna_yesterday(), model, NIGHT_MS);
    night.bounds.bytes = 20_000;
    assert_eq!(night.step(), Step::Spent);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(memory.applied().is_empty());
}

#[test]
fn a_new_night_has_its_caps_again() {
    let memory = FakeMemory::with(vec![
        input(ANNA, "n1", 1, 2, true),
        input(ANNA, "n2", 3, 4, false),
    ]);
    let at = Arc::new(AtomicU64::new(NIGHT_MS));
    let (mut night, _) = night(
        memory,
        anna_yesterday(),
        FakeModel::answering(PROPOSAL),
        NIGHT_MS,
    );
    let clock = at.clone();
    night.clock = Box::new(move || clock.load(Ordering::SeqCst));
    night.bounds.calls = 1;
    assert!(matches!(night.step(), Step::Applied(_)));
    assert_eq!(night.step(), Step::Spent);
    at.store(NIGHT_MS + 86_400_000, Ordering::SeqCst);
    assert!(matches!(night.step(), Step::Applied(_)));
}

#[test]
fn a_call_past_its_timeout_is_dropped_and_applied_empty() {
    let memory = FakeMemory::with(vec![input(ANNA, "n1", 1, 2, false)]);
    let mut model = FakeModel::answering(PROPOSAL);
    model.delay = Duration::from_secs(30);
    let (mut night, _) = night(memory.clone(), anna_yesterday(), model, NIGHT_MS);
    night.bounds.timeout = Duration::from_millis(100);
    assert_eq!(night.step(), Step::Emptied(Emptied::TimedOut));
    let applied = memory.applied();
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0]["proposal"], json!({}));
    assert_eq!(night.step(), Step::NoWork);
}

#[test]
fn an_unread_reply_is_applied_empty_once_and_never_asked_again() {
    let memory = FakeMemory::with(vec![input(ANNA, "n1", 1, 2, false)]);
    let model = FakeModel::answering("I think they like tea.");
    let calls = model.calls.clone();
    let (mut night, logged) = night(memory.clone(), anna_yesterday(), model, NIGHT_MS);
    assert_eq!(night.step(), Step::Emptied(Emptied::Unread));
    assert_eq!(night.step(), Step::NoWork);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(memory.applied()[0]["proposal"], json!({}));
    assert!(logged.lock().unwrap().iter().all(|l| !l.contains("tea")));
}

#[test]
fn the_switches_pass_through_and_shape_the_prompt() {
    let memory = FakeMemory::with(vec![input(ANNA, "n1", 1, 2, false)]);
    let model = FakeModel::answering(PROPOSAL);
    let asked = model.asked.clone();
    let (mut night, _) = night(memory.clone(), anna_yesterday(), model, NIGHT_MS);
    night.switches = Switches {
        patterns: true,
        feelings: false,
    };
    assert!(matches!(night.step(), Step::Applied(_)));
    let applied = memory.applied();
    assert_eq!(applied[0]["patterns"], true);
    assert_eq!(applied[0]["feelings"], false);
    let system = &asked.lock().unwrap()[0].0;
    assert!(system.contains("\"patterns\"") && !system.contains("\"feelings\""));
}

#[test]
fn an_apply_line_over_the_memorys_bound_carries_an_empty_proposal() {
    let big = json!({"facts":[{"claim":"x".repeat(MOST_LINE_BYTES)}]});
    let line = apply_line(ANNA, "n1", Switches::default(), big);
    assert_eq!(line["proposal"], json!({}));
    let small = apply_line(ANNA, "n1", Switches::default(), json!({"facts":[]}));
    assert_eq!(small["proposal"], json!({"facts":[]}));
}

#[test]
fn off_the_switch_nothing_starts_and_the_signals_do_nothing() {
    for value in [None, Some(""), Some("off"), Some("yes"), Some("shadow")] {
        assert!(!switched_on(value));
    }
    assert!(switched_on(Some("on")));
    // No test starts the night: the signals leave it as it was.
    let before = STIRS.load(Ordering::Relaxed);
    stir();
    privacy(false);
    privacy(true);
    assert_eq!(STIRS.load(Ordering::Relaxed), before);
    assert_eq!(OPEN_AT_US.load(Ordering::Relaxed), 0);
}

#[test]
fn whoever_the_eyes_know_at_the_lamp_when_someone_spoke_is_noted_for_that_day() {
    let memory = FakeMemory::with(vec![input(ANNA, "n1", 1, 2, false)]);
    let lamp = FakeLamp::default();
    let at = Arc::new(AtomicU64::new(YESTERDAY_MS));
    let (mut night, _) = night(
        memory.clone(),
        lamp.clone(),
        FakeModel::answering(PROPOSAL),
        0,
    );
    let clock = at.clone();
    night.clock = Box::new(move || clock.load(Ordering::SeqCst));
    // Anna at the lamp, nobody spoke: not noted.
    *lamp.person.lock().unwrap() = Some(ANNA.into());
    assert_eq!(night.step(), Step::Waiting);
    assert_eq!(night.waiting(), 0);
    // Someone spoke with the room's face only, or a guest: not noted.
    *lamp.person.lock().unwrap() = None;
    lamp.stirs.fetch_add(1, Ordering::SeqCst);
    assert_eq!(night.step(), Step::Waiting);
    *lamp.person.lock().unwrap() = Some("guest".into());
    lamp.stirs.fetch_add(1, Ordering::SeqCst);
    assert_eq!(night.step(), Step::Waiting);
    assert_eq!(night.waiting(), 0);
    // Anna spoke: her day is noted, and passed the next night.
    *lamp.person.lock().unwrap() = Some(ANNA.into());
    lamp.stirs.fetch_add(1, Ordering::SeqCst);
    assert_eq!(night.step(), Step::Waiting);
    assert_eq!(night.waiting(), 1);
    *lamp.person.lock().unwrap() = None;
    at.store(NIGHT_MS, Ordering::SeqCst);
    assert!(matches!(night.step(), Step::Applied(_)));
    assert_eq!(memory.prepared()[0]["person"], ANNA);
    assert_eq!(memory.prepared()[0]["day"], "2026-10-13");
}

#[test]
fn days_older_than_three_are_let_go() {
    let lamp = FakeLamp::hearing(&[(ANNA, YESTERDAY_MS - 5 * 86_400_000)]);
    let (mut night, _) = night(
        FakeMemory::default(),
        lamp,
        FakeModel::answering(PROPOSAL),
        NIGHT_MS,
    );
    assert_eq!(night.step(), Step::NoWork);
    assert_eq!(night.waiting(), 0);
}

#[test]
fn prepare_answers_are_read_as_the_contract_has_them() {
    assert_eq!(
        prepared_of(&json!({"ok":true,"reason":"nothing"})),
        Ok(None)
    );
    assert!(prepared_of(&json!({"ok":false,"error":"bad_person"})).is_err());
    assert!(prepared_of(&json!({"ok":true,"input":{"records":[]}})).is_err());
    let more = prepared_of(&input(ANNA, "n9", 3, 9, true))
        .unwrap()
        .unwrap();
    assert_eq!((more.job.as_str(), more.more_after), ("n9", Some(9)));
    let last = prepared_of(&input(ANNA, "n9", 3, 9, false))
        .unwrap()
        .unwrap();
    assert_eq!(last.more_after, None);
}

#[test]
fn the_proposal_is_read_only_as_one_json_object() {
    assert!(proposal_of(PROPOSAL).is_some());
    assert!(proposal_of(&format!("```\n{PROPOSAL}\n```")).is_some());
    assert!(proposal_of("{}").is_some());
    assert!(proposal_of("[1,2]").is_none());
    assert!(proposal_of("sure! {}").is_none());
}

#[test]
fn days_are_named_as_the_memory_names_them() {
    assert_eq!(day_name(0), "1970-01-01");
    assert_eq!(
        day_name(LocalTime::Offset(0).day(YESTERDAY_MS)),
        "2026-10-13"
    );
}

/// Nothing of the night is ever said or shown: its code names no speech,
/// ring, expression or bus.
#[test]
fn nothing_of_the_night_is_said_or_shown() {
    for source in [
        include_str!("nightly.rs"),
        include_str!("nightly_prompt.rs"),
    ] {
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
}

/// 2026-10-08 15:00 UTC, a Thursday of last week as NIGHT_MS sees it.
const LAST_WEEK_MS: u64 = NOON_MS - 5 * 86_400_000 + 3 * 3_600_000;
const CHAPTER: &str = r#"{"chapter":{"text":"A week of tea","cites":[1,2]}}"#;

/// The night with weekly chapters on, `heard` noted after.
fn weekly(
    memory: FakeMemory,
    heard: &[(&str, u64)],
    model: FakeModel,
    chapters: bool,
) -> (FakeNight, Logged) {
    let mut night = Night::new(memory, FakeLamp::default(), model, LocalTime::Offset(0)).unwrap();
    night.chapters = chapters;
    for (person, at) in heard {
        night.note(person, *at);
    }
    night.clock = Box::new(|| NIGHT_MS);
    let logged = Arc::new(Mutex::new(Vec::new()));
    let into = logged.clone();
    night.log = Arc::new(move |line| into.lock().unwrap().push(line.to_owned()));
    (night, logged)
}

fn week_input(job: &str) -> Value {
    json!({"ok":true,"input":{"job":job,"person":ANNA,"from":"2026-10-05","to":"2026-10-11",
        "items":[{"id":1,"kind":"episode","day":"2026-10-08","text":SECRET},
                 {"id":2,"kind":"fact","day":"2026-10-09","text":"likes tea"}]}})
}

#[test]
fn a_week_over_gets_one_chapter_call_after_the_days_with_counts_only_logged() {
    let memory = FakeMemory::default();
    *memory.weeklies.lock().unwrap() = vec![
        week_input("w3"),
        json!({"ok":true,"accepted":1,"reasons":{},"id":9}),
    ]
    .into();
    let model = FakeModel::answering(CHAPTER);
    let asked = model.asked.clone();
    let (mut night, logged) = weekly(memory.clone(), &[(ANNA, LAST_WEEK_MS)], model, true);
    // Last Thursday is past DAYS_BACK for a day's pass; its week is over.
    assert_eq!(
        night.step(),
        Step::Chapter {
            kept: true,
            reasons: "none".into()
        }
    );
    assert!(memory.prepared().is_empty(), "no day's pass");
    assert_eq!(
        memory.weekly_lines(),
        vec![
            json!({"op":"weekly_prepare","person":ANNA,"week":"2026-10-05"}),
            json!({"op":"weekly_apply","person":ANNA,"job":"w3",
                "proposal":serde_json::from_str::<Value>(CHAPTER).unwrap()}),
        ]
    );
    let asked = asked.lock().unwrap();
    assert!(asked[0].0.contains("week's chapter"));
    assert!(asked[0].1.contains("<data>") && asked[0].1.contains(SECRET));
    // Once a week: let go, nothing more.
    assert_eq!(night.step(), Step::NoWork);
    assert_eq!(night.weeks_waiting(), 0);
    for line in logged.lock().unwrap().iter() {
        for word in [SECRET, "A week of tea", ANNA, "w3"] {
            assert!(!line.contains(word), "{word} in {line}");
        }
    }
}

#[test]
fn with_the_switch_off_no_week_is_asked_for() {
    let memory = FakeMemory::default();
    *memory.weeklies.lock().unwrap() = vec![week_input("w3")].into();
    let model = FakeModel::answering(CHAPTER);
    let calls = model.calls.clone();
    let (mut night, _) = weekly(memory.clone(), &[(ANNA, LAST_WEEK_MS)], model, false);
    assert_eq!(night.step(), Step::NoWork);
    assert_eq!(night.weeks_waiting(), 0);
    assert!(memory.weekly_lines().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn the_days_come_first_and_a_week_not_over_waits() {
    let memory = FakeMemory::with(vec![input(ANNA, "n1", 1, 2, false)]);
    let (mut night, _) = weekly(
        memory.clone(),
        &[(ANNA, YESTERDAY_MS)],
        FakeModel::answering(PROPOSAL),
        true,
    );
    assert!(matches!(night.step(), Step::Applied(_)));
    // Yesterday's week (from Monday 12 Oct) is not over yet.
    assert_eq!(night.step(), Step::NoWork);
    assert!(memory.weekly_lines().is_empty());
    assert_eq!(night.weeks_waiting(), 1);
    // Not the room's, nor a guest's.
    let (mut room, _) = weekly(
        FakeMemory::default(),
        &[("everyone", LAST_WEEK_MS), ("guest", LAST_WEEK_MS)],
        FakeModel::answering(CHAPTER),
        true,
    );
    assert_eq!(room.weeks_waiting(), 0);
    assert_eq!(room.step(), Step::NoWork);
}

#[test]
fn a_dropped_chapter_logs_only_known_reasons_and_nothing_or_refused_lets_go() {
    let memory = FakeMemory::default();
    *memory.weeklies.lock().unwrap() = vec![
        week_input("w3"),
        json!({"ok":true,"accepted":0,"reasons":{"grounded":1,"made_up_reason":4}}),
    ]
    .into();
    let (mut night, _) = weekly(
        memory.clone(),
        &[(ANNA, LAST_WEEK_MS)],
        FakeModel::answering(CHAPTER),
        true,
    );
    assert_eq!(
        night.step(),
        Step::Chapter {
            kept: false,
            reasons: "grounded 1".into()
        }
    );
    for (answer, step) in [
        (json!({"ok":true,"reason":"nothing"}), Step::Nothing),
        (json!({"ok":false,"error":"done"}), Step::Refused),
    ] {
        let memory = FakeMemory::default();
        *memory.weeklies.lock().unwrap() = vec![answer].into();
        let model = FakeModel::answering(CHAPTER);
        let calls = model.calls.clone();
        let (mut night, _) = weekly(memory.clone(), &[(ANNA, LAST_WEEK_MS)], model, true);
        assert_eq!(night.step(), step);
        assert_eq!(night.weeks_waiting(), 0);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
