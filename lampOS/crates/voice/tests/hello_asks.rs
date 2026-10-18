//! A due hello asks the policy (#752), end to end on a real bus of the
//! test's own: the voice's `Arrival` says it on `voice/arrival`, the
//! policy's own loop decides with a stand-in judge, and the hello is said,
//! kept, or left to the rule. No model, no camera, no sound.
use lamp_bus::Bus;
use lamp_msgs::{Voice, VoiceArrival, VoiceState};
use lamp_policy::{
    decide::{Answer, Asked, Decider, Failed, Question},
    hello::{self, Hello as PolicyHello},
    policy::{self, Options},
};
use lamp_voice::speak_up::{ASK_FOR_US, Arrival, Hello};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const S: u64 = 1_000_000;
const QUIET: u64 = 60 * S;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// The judge, answering what the test sets: welcome, and intrusive.
#[derive(Clone)]
struct Judge(Arc<Mutex<[f32; 2]>>);
impl Decider for Judge {
    fn ask(
        &mut self,
        _: &str,
        questions: &[(String, Question)],
        _: Duration,
    ) -> Result<Asked, Failed> {
        let two = *self.0.lock().unwrap();
        Ok(Asked {
            answers: questions
                .iter()
                .zip(two)
                .map(|((name, _), yes)| (name.clone(), Answer::Noul(yes)))
                .collect(),
            model: "stand-in".into(),
            took: Duration::ZERO,
            usage: None,
            connection: None,
        })
    }
    fn name(&self) -> &str {
        "stand-in"
    }
}

struct Place {
    root: PathBuf,
    bus: Bus,
    judge: Judge,
    stop: Arc<AtomicBool>,
    policy: Option<thread::JoinHandle<std::io::Result<policy::Totals>>>,
}

impl Place {
    /// A bus, and the policy on it with `--hello`, acting or not.
    fn new(name: &str, act: bool) -> Self {
        let root = PathBuf::from(format!("/tmp/lvha-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(root.join("b")).unwrap().rescan_every(ms(20));
        let judge = Judge(Arc::new(Mutex::new([1.0, 0.0])));
        let stop = Arc::new(AtomicBool::new(false));
        let policy = {
            let (bus, stop, judge) = (bus.clone(), stop.clone(), judge.clone());
            let options = Options {
                log_dir: root.join("log"),
                deadline: ms(900),
                pause: ms(300),
                act_attention: false,
                report_every: policy::REPORT_EVERY,
            };
            let hello = hello::Options {
                quiet_for: ms(400),
                act,
                ..hello::Options::new(ms(1_500), None)
            };
            thread::spawn(move || {
                let hello = PolicyHello::start(judge.clone(), hello)?;
                policy::run_with_questions(
                    &bus,
                    judge,
                    &options,
                    &stop,
                    &mut |_| {},
                    None,
                    Some(hello),
                )
            })
        };
        let started = Instant::now();
        while !bus
            .subscribers(VoiceArrival::TOPIC)
            .unwrap()
            .iter()
            .any(|listener| listener.socket.starts_with("policy.") && listener.listening)
        {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the policy did not subscribe"
            );
            thread::sleep(ms(5));
        }
        Self {
            root,
            bus,
            judge,
            stop,
            policy: Some(policy),
        }
    }

    /// Someone back after six minutes, there three seconds: a hello due,
    /// asking the policy on this bus.
    fn arrival(&self) -> (Arrival, u64) {
        self.arrival_from(1_000 * S)
    }

    fn arrival_from(&self, start: u64) -> (Arrival, u64) {
        let mut now = start;
        let mut arrival = Arrival::watching(None, true, now)
            .asking(&self.bus)
            .unwrap();
        now += 360 * S;
        arrival.seen(true, now);
        now += 3 * S;
        arrival.seen(true, now);
        assert!(arrival.due(QUIET, now));
        (arrival, now)
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(policy) = self.policy.take() {
            let _ = policy.join();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Call `hello` every 2 ms, as the voice's loop does, the voice's clock
/// moving with the real one from `start_us`, until it is no longer
/// `NotYet`; what it came to, and after how long.
fn answered(arrival: &mut Arrival, start_us: u64) -> (Hello, Duration) {
    let began = Instant::now();
    loop {
        let now_us = start_us + u64::try_from(began.elapsed().as_micros()).unwrap();
        let hello = arrival.hello(true, QUIET, now_us);
        if hello != Hello::NotYet {
            return (hello, began.elapsed());
        }
        assert!(began.elapsed() < Duration::from_secs(5));
        thread::sleep(ms(2));
    }
}

#[test]
fn an_arrival_a_say_now_and_one_hello_then_none_within_twenty_minutes() {
    let place = Place::new("chain", true);
    let (mut arrival, now) = place.arrival();
    let (hello, waited) = answered(&mut arrival, now);
    assert_eq!(hello, Hello::SayNow);
    // Far inside the voice's 2 s with a judge that answers at once.
    assert!(waited < Duration::from_secs(1), "{waited:?}");
    let told = arrival.take_told().unwrap();
    assert_eq!(
        (told["answer"].as_str(), told["arrival"].as_u64()),
        (Some("say_now"), Some(now))
    );
    // Said: not due again for this arrival.
    arrival.said(now);
    assert!(!arrival.due(QUIET, now + S));
    // Someone else within twenty minutes: the policy's own rule says wait.
    let (mut next, now) = place.arrival_from(2_000 * S);
    assert_eq!(answered(&mut next, now).0, Hello::Wait);
    assert_eq!(next.take_told().unwrap()["answer"], "wait");
}

#[test]
fn a_wait_keeps_the_hello_and_says_so() {
    let place = Place::new("wait", true);
    *place.judge.0.lock().unwrap() = [0.2, 0.6];
    let (mut arrival, now) = place.arrival();
    assert_eq!(answered(&mut arrival, now).0, Hello::Wait);
    assert_eq!(arrival.take_told().unwrap()["answer"], "wait");
}

#[test]
fn never_while_anyone_is_talking_to_the_lamp() {
    let place = Place::new("talking", true);
    let mut voice = place
        .bus
        .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
        .unwrap();
    voice
        .publish_json(
            0,
            &VoiceState {
                state: Voice::Listening,
            },
        )
        .unwrap();
    thread::sleep(ms(100));
    let (mut arrival, now) = place.arrival();
    assert_eq!(answered(&mut arrival, now).0, Hello::Wait);
}

#[test]
fn a_policy_that_only_watches_or_does_not_answer_leaves_it_to_the_rule() {
    // In shadow: the rule, as soon as the decision comes.
    let place = Place::new("shadow", false);
    let (mut arrival, now) = place.arrival();
    let (hello, waited) = answered(&mut arrival, now);
    assert_eq!(hello, Hello::ByRule);
    assert!(waited < Duration::from_secs(1), "{waited:?}");
    assert_eq!(arrival.take_told().unwrap()["answer"], "shadow");
    // Nobody answering: the rule once two seconds have passed on the voice's
    // clock, and not a moment before. The clock is stepped, as speak_up's
    // own test steps it, not waited out on the host's (#1128).
    let root = PathBuf::from(format!("/tmp/lvha-none-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let bus = Bus::new(&root).unwrap();
    let mut lonely = Arrival::watching(None, true, 1_000 * S)
        .asking(&bus)
        .unwrap();
    let mut now = 1_360 * S;
    lonely.seen(true, now);
    now += 3 * S;
    lonely.seen(true, now);
    assert_eq!(lonely.hello(true, QUIET, now), Hello::NotYet);
    assert_eq!(
        lonely.hello(true, QUIET, now + ASK_FOR_US - 1),
        Hello::NotYet
    );
    assert_eq!(lonely.hello(true, QUIET, now + ASK_FOR_US), Hello::ByRule);
    assert_eq!(lonely.take_told().unwrap()["answer"], "none");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn without_the_switch_the_hello_is_as_it_was_and_nothing_is_said_on_the_bus() {
    let root = PathBuf::from(format!("/tmp/lvha-off-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let bus = Bus::new(&root).unwrap();
    let mut watching = bus
        .subscribe_typed(VoiceArrival::TOPIC, "test", VoiceArrival::TYPE)
        .unwrap();
    let mut arrival = Arrival::watching(None, true, 1_000 * S);
    let mut now = 1_360 * S;
    arrival.seen(true, now);
    now += 3 * S;
    arrival.seen(true, now);
    // The tick it is due and ready, it is the rule's: no wait at all.
    assert_eq!(arrival.hello(false, QUIET, now), Hello::NotYet);
    assert_eq!(arrival.hello(true, QUIET, now), Hello::ByRule);
    assert!(arrival.take_told().is_none());
    // Nothing went on the bus, and no publisher was made.
    assert!(watching.recv(ms(200)).unwrap().is_none());
    assert!(bus.publishers(VoiceArrival::TOPIC).unwrap().is_empty());
    let _ = fs::remove_dir_all(&root);
}
