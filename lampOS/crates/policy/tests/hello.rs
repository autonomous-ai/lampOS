//! The hello question in the policy's loop, on a real bus of the test's
//! own, with stand-ins for the decision models: the test plays the voice
//! and the eyes, and reads what the policy publishes and writes down. No
//! real model is asked.
use lamp_bus::{Bus, Publisher, Subscriber};
use lamp_msgs::{
    Decision, Diagnostic, Face, Faces, Room, RoomState, Rules, SkillAsk, Unprompted, Voice,
    VoiceArrival, VoiceState,
};
use lamp_policy::{
    attention,
    decide::{Answer, Asked, Decider, Failed, Question},
    hello::{self, Hello},
    moment::{self, Moment},
    policy::{self, Note, Options},
    progress::{self, Choice, Progress},
};
use serde_json::Value;
use std::{
    fs, io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(5);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn asked(names: Vec<(String, Question)>, yes: &[f32]) -> Asked {
    Asked {
        answers: names
            .into_iter()
            .zip(yes)
            .map(|((name, _), yes)| (name, Answer::Noul(*yes)))
            .collect(),
        model: "stand-in-1.0-20261015".into(),
        took: ms(1),
        usage: None,
        connection: None,
    }
}

/// Never asked: nothing is heard in these tests.
struct Attention;
impl Decider for Attention {
    fn ask(&mut self, _: &str, _: &[(String, Question)], _: Duration) -> Result<Asked, Failed> {
        Ok(asked(attention::questions(), &[0.1; 7]))
    }
    fn name(&self) -> &str {
        "attention-stand-in"
    }
}

/// The hello judge, or the moment judge, played by the test.
struct Judge {
    shown: mpsc::Sender<Value>,
    replies: mpsc::Receiver<Result<Asked, Failed>>,
    moment: bool,
}
impl Decider for Judge {
    fn ask(
        &mut self,
        state: &str,
        questions: &[(String, Question)],
        deadline: Duration,
    ) -> Result<Asked, Failed> {
        // Never longer than the asker can wait, whatever was given.
        if self.moment {
            assert_eq!(questions, moment::questions());
            assert_eq!(deadline, moment::MOST_ASKING);
        } else {
            assert_eq!(questions, hello::questions());
            assert_eq!(deadline, hello::MOST_ASKING);
        }
        let _ = self
            .shown
            .send(serde_json::from_str(state).expect("the state is JSON"));
        self.replies
            .recv_timeout(WAIT)
            .unwrap_or(Err(Failed::TimedOut))
    }
    fn name(&self) -> &str {
        "hello-stand-in"
    }
}

struct Rig {
    root: PathBuf,
    log_dir: PathBuf,
    bus: Bus,
    stop: Arc<AtomicBool>,
    running: Option<thread::JoinHandle<io::Result<policy::Totals>>>,
    saying: mpsc::Receiver<Note>,
    arrivals: Publisher,
    skills: Publisher,
    rules: Publisher,
    voice: Publisher,
    decisions: Subscriber,
    shown: mpsc::Receiver<Value>,
    replies: mpsc::Sender<Result<Asked, Failed>>,
    /// The moment judge's, with `--gate`.
    moment_shown: mpsc::Receiver<Value>,
    moment_replies: mpsc::Sender<Result<Asked, Failed>>,
}

impl Rig {
    fn new(name: &str, act: bool) -> Self {
        Self::with(name, act, false)
    }

    /// With `gate`, as `lamp-policy --gate`: skills' asks are answered.
    fn with(name: &str, act: bool, gate: bool) -> Self {
        Self::all(name, act, gate, false)
    }

    /// With `rules` too, as `lamp-policy --rules`: the people's rules about
    /// speaking first are obeyed.
    fn all(name: &str, act: bool, gate: bool, rules: bool) -> Self {
        let root = PathBuf::from(format!("/tmp/lph-{name}-{}", std::process::id()));
        let log_dir =
            std::env::temp_dir().join(format!("lamp-policy-hello-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&log_dir);
        fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap().rescan_every(ms(20));
        let declared = |topic: &str, declaration, source| {
            bus.publisher_declared(topic, &declaration, source).unwrap()
        };
        let (shown_to, shown) = mpsc::channel();
        let (replies, replied) = mpsc::channel();
        let (moment_shown_to, moment_shown) = mpsc::channel();
        let (moment_replies, moment_replied) = mpsc::channel();
        let (said_so, saying) = mpsc::channel();
        let mut eyes = declared(Faces::TOPIC, Faces::declaration(), "eyes");
        let mut room = declared(RoomState::TOPIC, RoomState::declaration(), "voice");
        let mut rig = Self {
            decisions: bus
                .subscribe_typed(Decision::TOPIC, "test", Decision::TYPE)
                .unwrap(),
            arrivals: declared(VoiceArrival::TOPIC, VoiceArrival::declaration(), "voice"),
            skills: declared(SkillAsk::TOPIC, SkillAsk::declaration(), "skills"),
            rules: declared(Rules::TOPIC, Rules::declaration(), "voice"),
            voice: declared(VoiceState::TOPIC, VoiceState::declaration(), "voice"),
            stop: Arc::new(AtomicBool::new(false)),
            running: None,
            saying,
            shown,
            replies,
            moment_shown,
            moment_replies,
            root,
            log_dir,
            bus,
        };
        let options = Options {
            log_dir: rig.log_dir.clone(),
            deadline: ms(900),
            pause: ms(300),
            act_attention: false,
        };
        let hello = hello::Options {
            quiet_for: ms(400),
            at_most_every: Duration::from_secs(20 * 60),
            act,
            gate,
            rules,
            // No hour: the rule's answer must not hang on the time of day
            // the test is run at.
            ..hello::Options::new(Duration::from_secs(3), None)
        };
        let judge = Judge {
            shown: shown_to,
            replies: replied,
            moment: false,
        };
        let (bus, stop) = (rig.bus.clone(), Arc::clone(&rig.stop));
        rig.running = Some(thread::spawn(move || {
            let hello = Hello::start(judge, hello)?;
            let hello = if gate {
                let judge = Judge {
                    shown: moment_shown_to,
                    replies: moment_replied,
                    moment: true,
                };
                hello.with_moment(Moment::start(
                    judge,
                    moment::Options::new(Duration::from_secs(3), None),
                )?)
            } else {
                hello
            };
            policy::run_with_questions(
                &bus,
                Attention,
                &options,
                &stop,
                &mut |note| {
                    let _ = said_so.send(note);
                },
                None,
                Some(hello),
            )
        }));
        let started = Instant::now();
        while !rig
            .bus
            .subscribers(VoiceArrival::TOPIC)
            .unwrap()
            .iter()
            .any(|listener| listener.socket.starts_with("policy.") && listener.listening)
        {
            assert!(started.elapsed() < WAIT, "the policy did not subscribe");
            thread::sleep(ms(5));
        }
        thread::sleep(ms(60));
        rig.state(Voice::Resting);
        room.publish_json(0, &RoomState { state: Room::Quiet })
            .unwrap();
        let stop = Arc::clone(&rig.stop);
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let near = Face {
                    speaking: None,
                    id: "f1".into(),
                    xmin: 0.3,
                    ymin: 0.2,
                    xmax: 0.6,
                    ymax: 0.6,
                    confidence: 0.9,
                    yaw: None,
                    facing: Some(true),
                };
                let _ = eyes.publish_json(0, &Faces { faces: vec![near] });
                thread::sleep(ms(100));
            }
        });
        thread::sleep(ms(150));
        rig
    }

    fn state(&mut self, state: Voice) {
        self.voice.publish_json(0, &VoiceState { state }).unwrap();
    }

    /// A skill asks whether the lamp may do `act`, as ask `number`.
    fn ask(&mut self, number: u64, act: &str) {
        let ask = SkillAsk {
            ask: number,
            skill: "welcome-back".into(),
            act: act.into(),
            person: None,
            away_min: (act == "hello").then_some(42),
            there_s: (act == "hello").then_some(3),
        };
        self.skills.publish_json(0, &ask).unwrap();
    }

    /// The voice says what the people at the lamp asked about speaking
    /// first: `quiet`, never.
    fn keep(&mut self, quiet: bool) {
        let rules = Rules {
            person: "p-be9688998f085098".into(),
            quiet,
            quiet_mornings: false,
            rules: u32::from(quiet),
            hello: Unprompted::default(),
            news: Unprompted::default(),
        };
        self.rules.publish_json(0, &rules).unwrap();
    }

    /// The voice holds a hello for arrival `number`.
    fn arrive(&mut self, number: u64, since_hello_min: Option<u32>) {
        let arrival = VoiceArrival {
            arrival: number,
            away_min: 42,
            there_s: 3,
            quiet_s: 310,
            since_hello_min,
        };
        self.arrivals.publish_json(0, &arrival).unwrap();
    }

    fn shown(&self) -> Value {
        self.shown.recv_timeout(WAIT).expect("a hello question")
    }
    fn not_asked(&self) {
        if let Ok(state) = self.shown.recv_timeout(ms(300)) {
            panic!("asked about {state}");
        }
    }
    fn answer(&self, two: [f32; 2]) {
        self.replies
            .send(Ok(asked(hello::questions(), &two)))
            .unwrap();
    }

    /// The next hello decision on the bus, and how long it took to come
    /// after `since`.
    fn decision(&mut self) -> Decision {
        let started = Instant::now();
        loop {
            let left = WAIT.saturating_sub(started.elapsed());
            let message = self.decisions.recv(left).unwrap().expect("a decision");
            let decision: Decision = message.parse().unwrap();
            decision.check().unwrap();
            if decision.about == "hello" {
                return decision;
            }
        }
    }

    /// The next decision on the bus that answers ask `number`.
    fn answer_to(&mut self, number: u64) -> Decision {
        let started = Instant::now();
        loop {
            let left = WAIT.saturating_sub(started.elapsed());
            let message = self.decisions.recv(left).unwrap().expect("an answer");
            let decision: Decision = message.parse().unwrap();
            decision.check().unwrap();
            if decision.ask == Some(number) {
                return decision;
            }
        }
    }

    fn log(&self, lines: usize) -> Vec<Value> {
        let started = Instant::now();
        loop {
            let found: Vec<Value> = fs::read_dir(&self.log_dir)
                .into_iter()
                .flatten()
                .map(|file| file.unwrap().path())
                .filter(|path| path.to_string_lossy().contains("decisions-"))
                .flat_map(|path| {
                    fs::read_to_string(path)
                        .unwrap()
                        .lines()
                        .map(|line| serde_json::from_str::<Value>(line).unwrap())
                        .collect::<Vec<_>>()
                })
                .filter(|line| line["about"] == "hello")
                .collect();
            if found.len() >= lines {
                assert_eq!(found.len(), lines);
                return found;
            }
            assert!(started.elapsed() < WAIT, "{} of {lines} lines", found.len());
            thread::sleep(ms(5));
        }
    }

    fn report(&self) -> Vec<(String, String)> {
        let mut reports = self
            .bus
            .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
            .unwrap();
        loop {
            let message = reports.recv(WAIT).unwrap().expect("a report");
            let report: Diagnostic = message.parse().unwrap();
            if report.name == "policy" {
                return report
                    .values
                    .into_iter()
                    .map(|pair| (pair.key, pair.value))
                    .collect();
            }
        }
    }

    fn stop(mut self) -> Vec<Note> {
        self.stop.store(true, Ordering::Relaxed);
        self.running.take().unwrap().join().unwrap().unwrap();
        let notes = self.saying.try_iter().collect();
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_dir_all(&self.log_dir);
        notes
    }
}

#[test]
fn an_arrival_is_asked_about_and_decided_for_its_number_in_shadow() {
    let mut rig = Rig::new("shadow", false);
    rig.arrive(1_000_001, Some(95));
    let state = rig.shown();
    assert_eq!(state["arrival"]["minutes_away"], 42);
    assert_eq!(
        state["arrival"]["minutes_since_the_lamp_last_said_hello"],
        95
    );
    assert_eq!(state["room"]["someone_at_the_lamp"], true);
    assert_eq!(state["room"]["facing_the_lamp"], true);
    assert_eq!(state["room"]["room"], "quiet: nobody else has been talking");
    assert!(state["room"]["hour_of_day"].is_null());
    rig.answer([0.9, 0.1]);
    let decided = rig.decision();
    assert_eq!(
        (decided.choice.as_str(), decided.shadow, decided.arrival),
        ("say_now", true, Some(1_000_001))
    );
    assert!(decided.task.is_none());
    assert_eq!(decided.answers.len(), 2);
    let line = &rig.log(1)[0];
    assert_eq!(line["context"]["arrival"], 1_000_001);
    assert_eq!(line["state"], state);
    assert_eq!(line["shadow"], true);
    assert_eq!(line["asked"], "hello-stand-in");
    let report = rig.report();
    for (key, value) in [("hello_decided", "1"), ("hello_said", "1")] {
        assert!(report.contains(&(key.into(), value.into())), "{report:?}");
    }
    assert!(report.iter().all(|(key, _)| key != "hello_acts"));
    let notes = rig.stop();
    let said = notes
        .iter()
        .find(|note| matches!(note, Note::HelloDecided { .. }))
        .unwrap()
        .to_string();
    assert!(
        said.starts_with("hello decision 1 arrival=1000001 choice=say_now"),
        "{said}"
    );
}

#[test]
fn acting_a_say_now_goes_out_for_real_and_none_follows_within_twenty_minutes() {
    let mut rig = Rig::new("act", true);
    rig.arrive(2_000_001, None);
    let _ = rig.shown();
    rig.answer([0.9, 0.1]);
    let decided = rig.decision();
    assert_eq!(
        (decided.choice.as_str(), decided.shadow),
        ("say_now", false)
    );
    assert_eq!(rig.log(1)[0]["shadow"], false);
    // The next, a minute later by the voice's count: the rule says wait,
    // and no model is asked.
    rig.arrive(2_000_002, Some(1));
    let held = rig.decision();
    assert_eq!(
        (held.choice.as_str(), held.by.as_str(), held.arrival),
        ("wait", "rule", Some(2_000_002))
    );
    rig.not_asked();
    assert_eq!(rig.log(2)[1]["guard"], "greeted_recently");
    // And so does the policy's own memory of the hello it made happen,
    // whatever the voice says.
    rig.arrive(2_000_003, None);
    assert_eq!(rig.decision().choice, "wait");
    rig.not_asked();
    let report = rig.report();
    for (key, value) in [
        ("hello_acts", "true"),
        ("hello_held_recent", "2"),
        ("shadow", "true"),
    ] {
        assert!(report.contains(&(key.into(), value.into())), "{report:?}");
    }
    rig.stop();
}

#[test]
fn never_while_anyone_is_talking_to_the_lamp() {
    let mut rig = Rig::new("talking", true);
    rig.state(Voice::Listening);
    thread::sleep(ms(100));
    rig.arrive(3_000_001, None);
    let held = rig.decision();
    assert_eq!((held.choice.as_str(), held.by.as_str()), ("wait", "rule"));
    rig.not_asked();
    assert_eq!(rig.log(1)[0]["guard"], "talking");
    // Someone begins to talk while the model is asked: its yes is a wait.
    rig.state(Voice::Resting);
    thread::sleep(ms(500));
    rig.arrive(3_000_002, None);
    let _ = rig.shown();
    rig.state(Voice::Listening);
    thread::sleep(ms(100));
    rig.answer([1.0, 0.0]);
    let turned = rig.decision();
    assert_eq!((turned.choice.as_str(), turned.probability), ("wait", 1.0));
    assert_eq!(rig.log(2)[1]["guard"], "talking");
    let notes = rig.stop();
    assert!(notes.iter().any(|note| matches!(
        note,
        Note::HelloDecided {
            arrival: 3_000_002,
            choice: Choice::Wait,
            guard: Some(hello::Hold::Talking),
            ..
        }
    )));
}

#[test]
fn a_failed_model_leaves_it_to_the_rule_in_time_for_the_voice() {
    let mut rig = Rig::new("rule", true);
    rig.arrive(4_000_001, None);
    let _ = rig.shown();
    rig.replies.send(Err(Failed::TimedOut)).unwrap();
    let ruled = rig.decision();
    assert_eq!(
        (ruled.choice.as_str(), ruled.by.as_str(), ruled.probability),
        ("say_now", "rule", hello::RULE_YES)
    );
    assert_eq!(rig.log(1)[0]["failed"], "timed_out");
    rig.stop();
}

/// #939: with `--gate`, a skill's ask is answered by its question, and the
/// decision acts and names the ask. A hello is put to the hello question,
/// as the voice's arrivals are, with its minutes away and seconds there, and
/// names the ask, not an arrival; the next is held by the hard rule, the
/// lamp having just been told to greet, with no model asked. News is put to
/// the moment question with how things are around the lamp; anything
/// unprompted while someone talks to the lamp is `wait` by the rule, with
/// no model asked.
#[test]
fn with_the_gate_a_skills_ask_is_decided_by_its_question_and_names_the_ask() {
    let mut rig = Rig::with("gate", true, true);
    // A rule kept, `quiet`: without `--rules` nothing below is held by it
    // (the reviewer on #1003).
    rig.keep(true);
    rig.ask(3_000_001, "hello");
    let shown = rig.shown();
    assert_eq!(
        (
            &shown["arrival"]["minutes_away"],
            &shown["arrival"]["seconds_there"]
        ),
        (&Value::from(42), &Value::from(3))
    );
    rig.answer([0.9, 0.1]);
    let decided = rig.decision();
    assert_eq!(
        (
            decided.about.as_str(),
            decided.choice.as_str(),
            decided.shadow,
            decided.ask,
            decided.arrival
        ),
        ("hello", "say_now", false, Some(3_000_001), None)
    );
    rig.ask(3_000_002, "hello");
    let held = rig.decision();
    assert_eq!(
        (held.choice.as_str(), held.by.as_str(), held.ask),
        ("wait", "rule", Some(3_000_002))
    );
    rig.not_asked();
    assert_eq!(rig.log(2)[1]["guard"], "greeted_recently");
    // News: put to the moment question, as it is now around the lamp.
    rig.ask(3_000_003, "news");
    let shown = rig
        .moment_shown
        .recv_timeout(WAIT)
        .expect("the moment question asked");
    assert_eq!(shown["what"]["skill"], "welcome-back");
    assert_eq!(shown["room"]["someone_at_the_lamp"], true);
    // The hello just said is the lamp's turn only once the voice says it.
    assert_eq!(
        shown["room"]["times_the_lamp_spoke_up_unasked_in_the_last_hour"],
        0
    );
    rig.moment_replies
        .send(Ok(asked(moment::questions(), &[0.8])))
        .unwrap();
    let moment = rig.answer_to(3_000_003);
    assert_eq!(
        (
            moment.about.as_str(),
            moment.choice.as_str(),
            moment.shadow,
            moment.ask,
            moment.answers.get("good_moment").copied()
        ),
        ("moment", "say_now", false, Some(3_000_003), Some(0.8))
    );
    // Someone talking to the lamp: `wait` by the rule, no model asked.
    rig.state(Voice::Listening);
    thread::sleep(ms(50));
    rig.ask(3_000_004, "unprompted");
    let held = rig.answer_to(3_000_004);
    assert_eq!(
        (held.about.as_str(), held.choice.as_str(), held.by.as_str()),
        ("moment", "wait", "rule")
    );
    assert!(rig.moment_shown.recv_timeout(ms(300)).is_err(), "asked");
    let report = rig.report();
    for (key, value) in [
        ("skill_asks", "4"),
        ("moment_decided", "2"),
        ("moment_said", "1"),
        ("moment_held_talking", "1"),
    ] {
        assert!(report.contains(&(key.into(), value.into())), "{report:?}");
    }
    // Without `--rules` the people's rules are not listened to (#813).
    assert!(
        !report
            .iter()
            .any(|(key, _)| key == "hello_held_rule" || key == "moment_held_rule")
    );
    assert!(
        !rig.bus
            .subscribers(Rules::TOPIC)
            .unwrap_or_default()
            .iter()
            .any(|listener| listener.socket.starts_with("policy."))
    );
    rig.stop();
}

/// #813: with `--rules`, a rule the people at the lamp kept about speaking
/// first holds the hello and a skill's asks as the other hard rules do:
/// `wait` by the rule, with no model asked, and counted. Forgotten, the next
/// hello is asked about again.
#[test]
fn with_rules_a_kept_rule_holds_the_hello_and_the_skills_asks_and_no_model_is_asked() {
    let mut rig = Rig::all("rules", true, true, true);
    rig.keep(true);
    thread::sleep(ms(100));
    rig.arrive(5_000_001, None);
    let held = rig.decision();
    assert_eq!(
        (held.choice.as_str(), held.by.as_str(), held.arrival),
        ("wait", "rule", Some(5_000_001))
    );
    rig.not_asked();
    assert_eq!(rig.log(1)[0]["guard"], "kept_rule");
    rig.ask(5_000_002, "news");
    let held = rig.answer_to(5_000_002);
    assert_eq!(
        (held.about.as_str(), held.choice.as_str(), held.by.as_str()),
        ("moment", "wait", "rule")
    );
    assert!(rig.moment_shown.recv_timeout(ms(300)).is_err(), "asked");
    // The rule forgotten, the next hello is asked about and said.
    rig.keep(false);
    thread::sleep(ms(100));
    rig.arrive(5_000_003, None);
    let _ = rig.shown();
    rig.answer([0.9, 0.1]);
    let said = rig.decision();
    assert_eq!(
        (said.choice.as_str(), said.arrival),
        ("say_now", Some(5_000_003))
    );
    let report = rig.report();
    for (key, value) in [
        ("hello_held_rule", "1"),
        ("hello_held_talking", "0"),
        ("moment_held_rule", "1"),
        ("moment_held_talking", "0"),
    ] {
        assert!(report.contains(&(key.into(), value.into())), "{report:?}");
    }
    let notes = rig.stop();
    assert!(notes.iter().any(|note| matches!(
        note,
        Note::HelloDecided {
            arrival: 5_000_001,
            choice: Choice::Wait,
            guard: Some(hello::Hold::KeptRule),
            ..
        }
    )));
}

#[test]
fn without_the_switch_arrivals_are_not_listened_to_and_the_report_is_as_before() {
    struct Never;
    impl Decider for Never {
        fn ask(&mut self, _: &str, _: &[(String, Question)], _: Duration) -> Result<Asked, Failed> {
            Err(Failed::TimedOut)
        }
        fn name(&self) -> &str {
            "never"
        }
    }
    let root = PathBuf::from(format!("/tmp/lph-off-{}", std::process::id()));
    let log_dir =
        std::env::temp_dir().join(format!("lamp-policy-hello-off-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let bus = Bus::new(&root).unwrap().rescan_every(ms(20));
    let mut reports = bus
        .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
        .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    // The progress question on, the hello question off.
    let running = {
        let (bus, stop) = (bus.clone(), Arc::clone(&stop));
        let options = Options {
            log_dir: log_dir.clone(),
            deadline: ms(900),
            pause: ms(300),
            act_attention: false,
        };
        thread::spawn(move || {
            let progress = Progress::start(Never, progress::Options::new(ms(900), None)).unwrap();
            policy::run_with_questions(
                &bus,
                Attention,
                &options,
                &stop,
                &mut |_| {},
                Some(progress),
                None,
            )
        })
    };
    let report: Diagnostic = loop {
        let message = reports.recv(WAIT).unwrap().expect("a report");
        let report: Diagnostic = message.parse().unwrap();
        if report.name == "policy" {
            break report;
        }
    };
    let keys: Vec<&str> = report.values.iter().map(|pair| pair.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "decided",
            "by_rule",
            "shadow",
            "progress_decided",
            "progress_by_rule",
            "progress_said",
            "progress_held_talking",
            "progress_held_recent"
        ]
    );
    assert!(
        bus.subscribers(VoiceArrival::TOPIC)
            .unwrap_or_default()
            .is_empty()
    );
    assert!(!root.join("voice/arrival").exists());
    // Nor, without `--gate`, the skills' asks (#939).
    assert!(
        bus.subscribers(SkillAsk::TOPIC)
            .unwrap_or_default()
            .is_empty()
    );
    stop.store(true, Ordering::Relaxed);
    running.join().unwrap().unwrap();
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&log_dir);
}
