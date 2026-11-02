//! The tiered store beside Days (`LAMP_MEMORY_TIERS`, #4746/#4747): shadow
//! answers are Days' byte for byte, writes are mirrored, every kind of
//! removal reaches the tiers with what the night derived from it, scopes
//! stay apart, the night runs on its clock, and a failing tiered store
//! leaves Days answering.
use lamp_memory::{
    Options,
    days::Days,
    server::Memory,
    store::LocalTime,
    tiers::{self, Mode, Sleeper, Tally, Tiered},
};
use lamp_memory_tiers::SleepInput;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

/// 2026-10-13 12:00 UTC.
const NOON: u64 = 1_791_892_800_000;
const HOUR: u64 = 3_600_000;
/// 03:30 the next morning, in the night's hours.
const NIGHT: u64 = NOON + 15 * HOUR + HOUR / 2;
const P1: &str = "p-0123456789abcdef";
const R1: &str = "r-0123456789abcdef";
const P2: &str = "p-fedcba9876543210";
const ROOM: &str = "everyone";

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("lamp-tiers-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Self(dir)
    }
    fn days(&self) -> Days {
        Days::open(
            Options {
                dir: self.0.clone(),
                keep_days: 30,
                local: LocalTime::Offset(0),
            },
            NOON,
        )
        .unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A fake model: one fact and one owner-model line for each owner record,
/// each with its words, and each episode's gist of its first record's.
struct Model;
impl Sleeper for Model {
    fn propose(&mut self, _scope: &str, input: &SleepInput) -> Result<String, String> {
        let mut facts = vec![];
        let mut lines = vec![];
        for r in input.records() {
            let key = format!("k{}", r.id);
            facts.push(json!({"key":key,"claim":format!("They said {}", r.text),"record_ids":[r.id],"strength":"said","kind":"preference","true_since_ms":r.at_ms,"true_until_ms":null,"supersedes":null}));
            lines.push(json!({"replaces":null,"kind":"owner_model","text":format!("Cares about {}", r.text),"fact_ids":[],"fact_keys":[key]}));
        }
        let episodes: Vec<Value> = input
            .episodes()
            .iter()
            .map(|e| {
                // The first record's last word: derived from it alone.
                let word = input
                    .records()
                    .iter()
                    .find(|r| e.record_ids.contains(&r.id))
                    .and_then(|r| r.text.split(' ').next_back())
                    .unwrap_or("nothing");
                json!({"episode_id":e.id,"record_ids":e.record_ids,"gist":format!("A talk about {word}"),"importance":0.5})
            })
            .collect();
        Ok(json!({"facts":facts,"lines":lines,"episodes":episodes}).to_string())
    }
}

/// A model that holds the worker until told to go on.
struct Held(Arc<Mutex<mpsc::Receiver<()>>>);
impl Sleeper for Held {
    fn propose(&mut self, _scope: &str, _input: &SleepInput) -> Result<String, String> {
        let _ = self.0.lock().unwrap().recv_timeout(Duration::from_secs(20));
        Err("held".into())
    }
}

struct Lamp {
    tiered: Tiered,
    clock: Arc<AtomicU64>,
    logged: Arc<Mutex<Vec<String>>>,
}

fn options(scratch: &Scratch, mode: Mode, clock: &Arc<AtomicU64>) -> tiers::Options {
    let mut options = tiers::Options::new(scratch.0.clone(), mode, LocalTime::Offset(0));
    let read = clock.clone();
    options.clock = Box::new(move || read.load(Ordering::SeqCst));
    options
}

fn lamp(scratch: &Scratch, mode: Mode, sleeper: Option<Box<dyn Sleeper>>) -> Lamp {
    lamp_with(scratch, mode, sleeper, tiers::QUEUE)
}

fn lamp_with(
    scratch: &Scratch,
    mode: Mode,
    sleeper: Option<Box<dyn Sleeper>>,
    queue: usize,
) -> Lamp {
    let clock = Arc::new(AtomicU64::new(NOON));
    let logged = Arc::new(Mutex::new(vec![]));
    let mut options = options(scratch, mode, &clock);
    options.sleeper = sleeper;
    options.queue = queue;
    let into = logged.clone();
    options.log = Arc::new(move |line| into.lock().unwrap().push(line.to_owned()));
    Lamp {
        tiered: Tiered::start(scratch.days(), options),
        clock,
        logged,
    }
}

fn ask(memory: &mut impl Memory, request: Value) -> Value {
    let answer = memory.answer(request.to_string().as_bytes(), NOON);
    serde_json::from_str(&answer.line).unwrap()
}

impl Lamp {
    fn ask(&mut self, request: Value) -> Value {
        ask(&mut self.tiered, request)
    }
    fn exchange(&mut self, person: &str, heard: &str, said: &str) -> u64 {
        let answer = self.ask(
            json!({"op":"append","person":person,"at_unix_ms":NOON,"heard":heard,"said":said}),
        );
        answer["id"].as_u64().unwrap()
    }
    fn keep(&mut self, person: &str, text: &str, sources: &[u64]) -> u64 {
        let answer = self.ask(json!({"op":"remember","person":person,"at_unix_ms":NOON,"text":text,"source_ids":sources}));
        answer["id"].as_u64().unwrap()
    }
    fn view(&self, scope: &str) -> tiers::View {
        assert!(self.tiered.settle());
        self.tiered.view(scope).unwrap_or_default()
    }
    fn tally(&self) -> Tally {
        assert!(self.tiered.settle());
        self.tiered.tally()
    }
    /// The clock at 03:30, and the worker's night run.
    fn sleep(&self) {
        self.clock.store(NIGHT, Ordering::SeqCst);
        assert!(self.tiered.settle());
        assert!(self.tiered.settle());
    }
}

fn script() -> Vec<Value> {
    vec![
        json!({"op":"append","person":P1,"at_unix_ms":NOON,"heard":"my sister Mai lives in Hue","said":"Hue is lovely"}),
        json!({"op":"append","person":P1,"at_unix_ms":NOON+1,"heard":"I train for the marathon on Sundays","said":"Good luck with it"}),
        json!({"op":"remember","person":P1,"at_unix_ms":NOON+2,"text":"the marathon is in March","source_ids":[2]}),
        json!({"op":"append","person":P2,"at_unix_ms":NOON+3,"heard":"I play the cello","said":"Lovely"}),
        json!({"op":"append","person":ROOM,"at_unix_ms":NOON+4,"heard":"somebody hummed a tune","said":"Nice tune"}),
        json!({"op":"recall","person":P1,"query":"marathon"}),
        json!({"op":"search","person":P1,"query":"sister"}),
        json!({"op":"kept","person":P1}),
        json!({"op":"session_open","person":P1,"session":"s1"}),
        json!({"op":"session_close","person":P1,"session":"s1"}),
        json!({"op":"talk_note","person":P1,"text":"they talked of family","source_ids":[1]}),
        json!({"op":"forget","person":P1,"id":1}),
        json!({"op":"forget_last","person":P2}),
        json!({"op":"adopt","person":ROOM,"to":P2,"ids":[1]}),
        json!({"op":"status","person":null}),
        json!({"op":"forget_all","person":P1}),
        json!({"op":"recall","person":P1,"query":"marathon"}),
        json!({"op":"nonsense"}),
    ]
}

#[test]
fn shadow_answers_are_the_days_answers_byte_for_byte() {
    let (alone, beside) = (Scratch::new("alone"), Scratch::new("beside"));
    let mut days = alone.days();
    let mut lamp = lamp(&beside, Mode::Shadow, None);
    for request in script() {
        let line = request.to_string();
        let a = days.answer(line.as_bytes(), NOON);
        let b = lamp.tiered.answer(line.as_bytes(), NOON);
        assert_eq!(a.line, b.line, "{line}");
        assert_eq!(a.log, b.log, "{line}");
        assert_eq!(
            (a.wrote, a.storage_fault, a.forgot),
            (b.wrote, b.storage_fault, b.forgot),
            "{line}"
        );
    }
    let tally = lamp.tally();
    assert_eq!(tally.failures, 0, "{tally:?}");
    assert!(tally.mirrored > 0);
}

#[test]
fn exchanges_and_kept_things_are_mirrored_and_the_rooms_are_not() {
    let scratch = Scratch::new("mirror");
    let mut lamp = lamp(&scratch, Mode::Shadow, None);
    let e = lamp.exchange(P1, "my sister Mai lives in Hue", "Hue is lovely");
    let k = lamp.keep(P1, "the marathon is in March", &[]);
    lamp.exchange(ROOM, "somebody hummed a tune", "Nice tune");
    let view = lamp.view(P1);
    assert!(
        view.records
            .contains(&"my sister Mai lives in Hue".to_owned())
    );
    assert!(view.records.contains(&"Hue is lovely".to_owned()));
    assert!(view.facts.contains(&"the marathon is in March".to_owned()));
    assert_eq!(view.days_ids, vec![e, k]);
    assert!(lamp.tiered.view(ROOM).is_none());
    assert!(!scratch.0.join(tiers::DIR).join("everyone.sqlite3").exists());
    let tally = lamp.tally();
    assert_eq!((tally.mirrored, tally.skipped), (3, 1), "{tally:?}");
}

/// Each kind of removal, and what each must take from the tiers.
#[test]
fn every_kind_of_forget_reaches_the_tiers() {
    let scratch = Scratch::new("forgets");
    let mut lamp = lamp(&scratch, Mode::Shadow, None);
    // A forget by id.
    let a = lamp.exchange(P1, "my locker code is 4471", "Noted");
    lamp.exchange(P1, "the dog is called Biscuit", "Sweet name");
    assert!(lamp.view(P1).holds("4471"));
    lamp.ask(json!({"op":"forget","person":P1,"id":a,"selective":true}));
    let view = lamp.view(P1);
    assert!(!view.holds("4471"));
    assert!(view.holds("Biscuit"));
    // A kept thing forgotten takes the exchange it came up in (#1693).
    let source = lamp.exchange(P1, "we fly to Lisbon in May", "How exciting");
    let kept = lamp.keep(P1, "the Lisbon flight is in May", &[source]);
    assert!(lamp.view(P1).facts.iter().any(|f| f.contains("Lisbon")));
    lamp.ask(json!({"op":"forget","person":P1,"id":kept}));
    assert!(
        lamp.tiered
            .days()
            .record(&person(P1), source)
            .unwrap()
            .is_none()
    );
    let view = lamp.view(P1);
    assert!(!view.holds("Lisbon"));
    // forget_last.
    lamp.exchange(P1, "my blood type is O negative", "Good to know");
    assert!(lamp.view(P1).holds("blood type"));
    lamp.ask(json!({"op":"forget_last","person":P1}));
    assert!(!lamp.view(P1).holds("blood type"));
    // forget_all of one person: the store and its key are gone.
    lamp.exchange(P2, "I play the cello", "Lovely");
    lamp.ask(json!({"op":"forget_all","person":P1}));
    assert!(lamp.view(P1).records.is_empty());
    let root = scratch.0.join(tiers::DIR);
    assert!(!root.join(format!("{P1}.sqlite3")).exists());
    assert!(!root.join(format!("{P1}.sqlite3.forget-key")).exists());
    assert!(lamp.view(P2).holds("cello"));
    // forget_person: theirs and their rules scope.
    lamp.keep(R1, "never call me before nine", &[]);
    lamp.exchange(P1, "I moved to Da Nang", "Welcome");
    assert!(lamp.view(R1).holds("before nine") && lamp.view(P1).holds("Da Nang"));
    lamp.ask(json!({"op":"forget_person","person":P1}));
    assert!(lamp.tiered.view(P1).is_none_or(|v| v.records.is_empty()));
    assert!(lamp.tiered.view(R1).is_none_or(|v| v.records.is_empty()));
    // The rules scope's store and key go with the person's (#4988).
    for scope in [P1, R1] {
        assert!(!root.join(format!("{scope}.sqlite3")).exists(), "{scope}");
        assert!(
            !root.join(format!("{scope}.sqlite3.forget-key")).exists(),
            "{scope}"
        );
    }
    // forget everything.
    lamp.ask(json!({"op":"forget_all","person":null}));
    assert!(lamp.tiered.view(P2).is_none_or(|v| v.records.is_empty()));
    let tally = lamp.tally();
    assert_eq!(tally.failures, 0, "{tally:?}");
    assert!(tally.resets >= 2, "{tally:?}");
}

/// A forget whose map can't be read deletes the scope's whole store
/// (#4988): it is never skipped in silence.
#[test]
fn a_forget_whose_map_cannot_be_read_deletes_the_scopes_store() {
    let scratch = Scratch::new("map-read");
    let mut lamp = lamp(&scratch, Mode::Shadow, None);
    let secret = lamp.exchange(P1, "my locker code is 4471", "Noted");
    lamp.exchange(P1, "the dog is called Biscuit", "Sweet name");
    lamp.exchange(P2, "I play the cello", "Lovely");
    assert!(lamp.view(P1).holds("4471"));
    let root = scratch.0.join(tiers::DIR);
    let store = root.join(format!("{P1}.sqlite3"));
    assert!(store.exists());
    // The map, corrupted under the worker's open connection.
    fs::write(root.join("mirror.sqlite3"), vec![0xA5u8; 8192]).unwrap();
    let answer = lamp.ask(json!({"op":"forget","person":P1,"id":secret,"selective":true}));
    assert_eq!(answer["ok"], true, "{answer}");
    let tally = lamp.tally();
    assert!(tally.failures >= 1 && tally.resets >= 1, "{tally:?}");
    assert!(!store.exists());
    assert!(!root.join(format!("{P1}.sqlite3.forget-key")).exists());
    let view = lamp.view(P1);
    assert!(!view.holds("4471") && !view.holds("Biscuit"), "{view:?}");
    assert!(
        lamp.logged
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.starts_with("tiers map read failed: "))
    );
    // Days forgot only the one.
    assert!(
        lamp.tiered
            .days()
            .record(&person(P1), secret)
            .unwrap()
            .is_none()
    );
}

fn person(id: &str) -> lamp_memory::Person {
    lamp_memory::Person::new(id).unwrap()
}

/// The days kept running out removes the exchange from the tiers too.
#[test]
fn an_exchange_past_the_days_kept_leaves_the_tiers() {
    let scratch = Scratch::new("expiry");
    let mut lamp = lamp(&scratch, Mode::Shadow, None);
    lamp.exchange(P1, "the old project was called Kestrel", "Nice");
    assert!(lamp.view(P1).holds("Kestrel"));
    let later = NOON + 40 * 24 * HOUR;
    let answer = lamp.tiered.answer(
        json!({"op":"status","person":null}).to_string().as_bytes(),
        later,
    );
    assert!(answer.line.contains("\"ok\":true"));
    assert!(!lamp.view(P1).holds("Kestrel"));
}

/// The room's words, adopted by someone known, become theirs in the tiers;
/// the room's never were.
#[test]
fn adopted_room_entries_reach_the_persons_tiers() {
    let scratch = Scratch::new("adopt");
    let mut lamp = lamp(&scratch, Mode::Shadow, None);
    let id = lamp.exchange(ROOM, "I am learning the piano", "Lovely");
    let moved = lamp.ask(json!({"op":"adopt","person":ROOM,"to":P1,"ids":[id]}));
    assert_eq!(moved["moved"], 1, "{moved}");
    let new = moved["ids"][0][1].as_u64().unwrap();
    let view = lamp.view(P1);
    assert!(view.holds("piano"));
    assert_eq!(view.days_ids, vec![new]);
    lamp.ask(json!({"op":"forget","person":P1,"id":new}));
    assert!(!lamp.view(P1).holds("piano"));
}

/// The night's facts, owner-model lines and episode gists, made from words
/// later forgotten, go with them.
#[test]
fn the_night_runs_once_and_what_it_made_from_forgotten_words_goes() {
    let scratch = Scratch::new("night");
    let mut lamp = lamp(&scratch, Mode::Shadow, Some(Box::new(Model)));
    let a = lamp.exchange(P1, "I am allergic to peanuts", "I will remember");
    lamp.exchange(P1, "I love hiking in Sapa", "Sounds fun");
    // Noon: no night.
    assert_eq!(lamp.tally().nights, 0);
    lamp.sleep();
    let tally = lamp.tally();
    assert_eq!((tally.nights, tally.nights_applied), (1, 1), "{tally:?}");
    let view = lamp.view(P1);
    assert!(view.facts.iter().any(|f| f.contains("peanuts")), "{view:?}");
    assert!(view.lines.iter().any(|l| l.contains("peanuts")), "{view:?}");
    assert!(
        view.episodes.iter().any(|e| e.contains("peanuts")),
        "{view:?}"
    );
    // Once a night.
    assert!(lamp.tiered.settle());
    assert_eq!(lamp.tally().nights, 1);
    lamp.ask(json!({"op":"forget","person":P1,"id":a,"selective":true}));
    let view = lamp.view(P1);
    assert!(!view.holds("peanuts"), "{view:?}");
    assert!(view.holds("Sapa"));
    assert!(
        lamp.logged
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.starts_with("tiers night person=p-0123456789abcdef model=applied"))
    );
}

/// Without a model (the product's lamp-memory has no network) the night
/// still ends episodes and expires, and is counted unconfigured.
#[test]
fn a_night_without_a_model_is_counted_unconfigured() {
    let scratch = Scratch::new("unconfigured");
    let mut lamp = lamp(&scratch, Mode::Shadow, None);
    lamp.exchange(P1, "I like green tea", "Me too");
    lamp.sleep();
    let tally = lamp.tally();
    assert_eq!(
        (
            tally.nights,
            tally.nights_unconfigured,
            tally.nights_applied
        ),
        (1, 1, 0)
    );
    assert!(lamp.view(P1).facts.is_empty());
}

#[test]
fn each_scope_holds_only_its_own_and_on_recalls_only_its_own() {
    let scratch = Scratch::new("scopes");
    let mut lamp = lamp(&scratch, Mode::On, None);
    lamp.keep(P1, "my favourite colour is teal", &[]);
    lamp.keep(P2, "my favourite colour is orange", &[]);
    lamp.keep(ROOM, "the favourite colour of the room is grey", &[]);
    let one = lamp.view(P1);
    assert!(one.holds("teal") && !one.holds("orange") && !one.holds("grey"));
    let two = lamp.view(P2);
    assert!(two.holds("orange") && !two.holds("teal"));
    let found = lamp.ask(json!({"op":"search","person":P1,"query":"favourite colour"}));
    let told = found["tiers"].to_string();
    assert!(told.contains("teal"), "{found}");
    assert!(
        !told.contains("orange") && !told.contains("grey"),
        "{found}"
    );
    // The room's search gets Days' answer alone.
    let room = lamp.ask(json!({"op":"search","person":ROOM,"query":"favourite colour"}));
    assert!(room.get("tiers").is_none(), "{room}");
}

#[test]
fn on_adds_the_tiers_beside_days_answer_and_shadow_never_does() {
    let (s, o) = (Scratch::new("shadow-recall"), Scratch::new("on-recall"));
    let mut shadow = lamp(&s, Mode::Shadow, None);
    let mut on = lamp(&o, Mode::On, None);
    for lamp in [&mut shadow, &mut on] {
        lamp.keep(P1, "the wifi password is on the fridge", &[]);
        assert!(lamp.tiered.settle());
    }
    let request = json!({"op":"recall","person":P1,"query":"wifi password"});
    let quiet = shadow.ask(request.clone());
    let told = on.ask(request);
    assert!(quiet.get("tiers").is_none());
    let mut without = told.clone();
    without.as_object_mut().unwrap().remove("tiers");
    assert_eq!(without, quiet);
    let item = &told["tiers"]["items"][0];
    assert!(item["text"].as_str().unwrap().contains("fridge"), "{told}");
    assert_eq!(item["ids"], json!([1]));
}

#[test]
fn a_tiered_store_that_cannot_open_leaves_days_answering() {
    let (alone, beside) = (Scratch::new("broken-alone"), Scratch::new("broken"));
    fs::create_dir_all(&beside.0).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&beside.0, fs::Permissions::from_mode(0o700)).unwrap();
    // A file where the tiers' directory would be.
    fs::write(beside.0.join(tiers::DIR), b"not a directory").unwrap();
    let mut days = alone.days();
    let mut lamp = lamp(&beside, Mode::On, None);
    for request in script() {
        let line = request.to_string();
        let a = days.answer(line.as_bytes(), NOON);
        let b = lamp.tiered.answer(line.as_bytes(), NOON);
        assert_eq!(a.line, b.line, "{line}");
    }
    let tally = lamp.tally();
    assert!(tally.failures > 0, "{tally:?}");
    assert_eq!(tally.mirrored, 0);
}

/// A burst past the queue never loses a forget: the tiers are emptied
/// before anything queued after it.
#[test]
fn a_full_queue_empties_the_tiers_rather_than_lose_a_forget() {
    let scratch = Scratch::new("overflow");
    let (go, wait) = mpsc::channel();
    let held = Held(Arc::new(Mutex::new(wait)));
    let mut lamp = lamp_with(&scratch, Mode::Shadow, Some(Box::new(held)), 2);
    let secret = lamp.exchange(P1, "the safe opens with 1984", "Noted");
    assert!(lamp.tiered.settle());
    // The night holds the worker in the model.
    lamp.clock.store(NIGHT, Ordering::SeqCst);
    lamp.exchange(P1, "wake the worker", "Awake");
    std::thread::sleep(Duration::from_millis(200));
    for n in 0..4 {
        lamp.exchange(P1, &format!("filler line {n}"), "Fine");
    }
    lamp.ask(json!({"op":"forget","person":P1,"id":secret,"selective":true}));
    assert!(lamp.tiered.tally().dropped > 0);
    go.send(()).unwrap();
    let view = lamp.view(P1);
    assert!(!view.holds("1984"), "{view:?}");
    assert!(lamp.tally().wipes >= 1);
}

/// A model that says it was called, then panics: the worker dies with it.
struct Panics(mpsc::Sender<()>);
impl Sleeper for Panics {
    fn propose(&mut self, _scope: &str, _input: &SleepInput) -> Result<String, String> {
        let _ = self.0.send(());
        panic!("the model panicked");
    }
}

/// A worker that stopped (a panic) can't hear a later forget: the request
/// side empties the tiers' directory instead.
#[test]
fn a_stopped_worker_empties_the_tiers_from_the_request_side() {
    let scratch = Scratch::new("stopped");
    let (called, panicked) = mpsc::channel();
    let mut lamp = lamp(&scratch, Mode::Shadow, Some(Box::new(Panics(called))));
    let secret = lamp.exchange(P1, "the alarm code is 7310", "Noted");
    assert!(lamp.view(P1).holds("7310"));
    let root = scratch.0.join(tiers::DIR);
    assert!(root.join(format!("{P1}.sqlite3.forget-key")).exists());
    // The night calls the model, which takes the worker down.
    lamp.clock.store(NIGHT, Ordering::SeqCst);
    lamp.exchange(P1, "wake the worker", "Awake");
    panicked.recv_timeout(Duration::from_secs(10)).unwrap();
    // Its queue closes as the thread unwinds; then the forget can't be sent.
    std::thread::sleep(Duration::from_millis(300));
    assert!(root.join(format!("{P1}.sqlite3")).exists());
    lamp.ask(json!({"op":"forget","person":P1,"id":secret,"selective":true}));
    let left: Vec<_> = fs::read_dir(&root).unwrap().flatten().collect();
    assert!(left.is_empty(), "{} files left", left.len());
    assert!(
        lamp.logged
            .lock()
            .unwrap()
            .iter()
            .any(|l| l == "tiers emptied: the worker stopped")
    );
    // Days answers on, and nothing more is written there.
    lamp.exchange(P1, "after the stop", "Fine");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    let tally = lamp.tiered.tally();
    assert!(tally.wipes >= 1 && tally.failures >= 1, "{tally:?}");
}

/// What a process didn't live to apply is applied at the next start.
#[test]
fn a_start_removes_what_days_no_longer_holds() {
    let scratch = Scratch::new("restart");
    {
        let mut lamp = lamp(&scratch, Mode::Shadow, None);
        lamp.exchange(P1, "my pin is 2580", "Noted");
        lamp.exchange(P1, "I like jazz", "Nice");
        assert!(lamp.view(P1).holds("2580"));
    }
    {
        // Days alone, as with the switch off: a forget the tiers never see.
        let mut days = scratch.days();
        ask(
            &mut days,
            json!({"op":"forget","person":P1,"id":1,"selective":true}),
        );
    }
    let lamp = lamp(&scratch, Mode::Shadow, None);
    let view = lamp.view(P1);
    assert!(!view.holds("2580"), "{view:?}");
    assert!(view.holds("jazz"));
}

#[test]
fn the_switch_is_off_unless_shadow_or_on() {
    assert_eq!(Mode::parse(None), Ok(None));
    assert_eq!(Mode::parse(Some("")), Ok(None));
    assert_eq!(Mode::parse(Some("off")), Ok(None));
    assert_eq!(Mode::parse(Some("shadow")), Ok(Some(Mode::Shadow)));
    assert_eq!(Mode::parse(Some("on")), Ok(Some(Mode::On)));
    assert!(Mode::parse(Some("ON")).is_err());
    assert!(Mode::parse(Some("yes")).is_err());
    assert!(!tiers::mirrored(ROOM) && !tiers::mirrored("r-everyone"));
    assert!(tiers::mirrored(P1) && tiers::mirrored(R1));
}

/// The product leaves the tiers off: no product unit or drop-in names the
/// switch (off until the played test and two reads).
#[test]
fn the_product_does_not_set_the_switch() {
    let product = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/product");
    let mut read = 0;
    let mut stack = vec![product];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(text) = fs::read_to_string(&path) {
                read += 1;
                assert!(!text.contains(tiers::SWITCH), "{}", path.display());
            }
        }
    }
    assert!(read > 10);
}

/// One request to a running `lamp-memory`, once it answers on its socket.
fn ask_socket(socket: &std::path::Path, request: Value) -> Value {
    use std::io::{BufRead, BufReader, Write};
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let stream = loop {
        match std::os::unix::net::UnixStream::connect(socket) {
            Ok(stream) => break stream,
            Err(error) if std::time::Instant::now() > deadline => {
                panic!("nobody answers on the socket: {error}")
            }
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut reader = BufReader::new(stream);
    reader
        .get_mut()
        .write_all(format!("{request}\n").as_bytes())
        .unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

/// A `--memory-days` start with the switch off removes the tiered copy an
/// earlier `shadow` left, since no forget made while off reaches it
/// (#4988); Days answers as before.
#[test]
fn an_off_start_removes_a_left_tiered_copy_and_answers_the_same() {
    use std::process::{Command, Stdio};
    let scratch = Scratch::new("off-start");
    fs::create_dir_all(&scratch.0).unwrap();
    let dir = scratch.0.join("store");
    let socket = scratch.0.join("m.sock");
    let root = dir.join(tiers::DIR);
    let start = |switch: Option<&str>, log: &str| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_lamp-memory"));
        command
            .arg("--dir")
            .arg(&dir)
            .arg("--socket")
            .arg(&socket)
            .args(["--utc-offset-minutes", "0", "--memory-days"])
            .env_remove(tiers::SWITCH)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(fs::File::create(scratch.0.join(log)).unwrap());
        if let Some(switch) = switch {
            command.env(tiers::SWITCH, switch);
        }
        command.spawn().unwrap()
    };
    let recall = json!({"op":"recall","person":P1,"query":"locker"});
    let mut shadow = start(Some("shadow"), "shadow.log");
    let appended = ask_socket(
        &socket,
        json!({"op":"append","person":P1,"at_unix_ms":NOON,"heard":"my locker code is 4471","said":"Noted"}),
    );
    assert_eq!(appended["ok"], true, "{appended}");
    let before = ask_socket(&socket, recall.clone());
    assert_eq!(before["ok"], true, "{before}");
    let store = root.join(format!("{P1}.sqlite3"));
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !store.exists() {
        assert!(std::time::Instant::now() < deadline, "no tiered store");
        std::thread::sleep(Duration::from_millis(10));
    }
    shadow.kill().unwrap();
    shadow.wait().unwrap();
    for (switch, log) in [(None, "off.log"), (Some("yes"), "unknown.log")] {
        if switch.is_some() {
            // A copy left again, for the unknown value's start.
            fs::create_dir_all(&root).unwrap();
            fs::write(&store, b"left").unwrap();
        }
        let _ = fs::remove_file(&socket);
        let mut off = start(switch, log);
        let after = ask_socket(&socket, recall.clone());
        assert_eq!(after, before);
        assert!(!root.exists(), "{log}");
        off.kill().unwrap();
        off.wait().unwrap();
        let logged = fs::read_to_string(scratch.0.join(log)).unwrap();
        // The removal's own line, counts only: the request lines may name
        // the person, it never does.
        let line = logged
            .lines()
            .find(|l| l.contains("tiers off: a left tiered copy removed stores=1 files="))
            .unwrap_or_else(|| panic!("{logged}"));
        assert!(!line.contains(P1), "{line}");
    }
}
