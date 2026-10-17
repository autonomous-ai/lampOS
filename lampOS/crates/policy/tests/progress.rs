//! The progress question in the policy's loop, on a real bus of the test's
//! own, with stand-ins for both decision models: the test plays the tasks
//! part, the voice and the eyes, and reads what the policy publishes and
//! writes down. No real model is asked.
use lamp_bus::{Bus, Publisher, Subscriber};
use lamp_msgs::{
    AgentState, Decision, Diagnostic, Face, Faces, Room, RoomState, Rules, Sign, Speech,
    TaskProgress, TaskStatus, Unprompted, Voice, VoiceState,
};
use lamp_policy::{
    attention,
    decide::{Answer, Asked, Decider, Failed, Question},
    policy::{self, Note, Options},
    progress::{self, Choice},
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
const STORE: &str = "0123456789abcdef0123456789abcdef";
const RUN: u64 = 1_791_964_000_000;

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

/// The attention judge: answers anything heard at once.
struct Attention;
impl Decider for Attention {
    fn ask(
        &mut self,
        _: &str,
        questions: &[(String, Question)],
        _: Duration,
    ) -> Result<Asked, Failed> {
        assert_eq!(questions, attention::questions());
        Ok(asked(attention::questions(), &[0.1; 7]))
    }
    fn name(&self) -> &str {
        "attention-stand-in"
    }
}

/// The progress judge, played by the test: it tells the test each state it
/// is shown and answers with what the test gives it next.
struct Judge {
    shown: mpsc::Sender<Value>,
    replies: mpsc::Receiver<Result<Asked, Failed>>,
}
impl Decider for Judge {
    fn ask(
        &mut self,
        state: &str,
        questions: &[(String, Question)],
        deadline: Duration,
    ) -> Result<Asked, Failed> {
        assert_eq!(questions, progress::questions());
        assert_eq!(deadline, ms(900));
        let _ = self
            .shown
            .send(serde_json::from_str(state).expect("the state is JSON"));
        self.replies
            .recv_timeout(WAIT)
            .unwrap_or(Err(Failed::TimedOut))
    }
    fn name(&self) -> &str {
        "progress-stand-in"
    }
}

struct Rig {
    root: PathBuf,
    log_dir: PathBuf,
    bus: Bus,
    stop: Arc<AtomicBool>,
    running: Option<thread::JoinHandle<io::Result<policy::Totals>>>,
    saying: mpsc::Receiver<Note>,
    status: Publisher,
    voice: Publisher,
    heard: Publisher,
    rules: Publisher,
    decisions: Subscriber,
    shown: mpsc::Receiver<Value>,
    replies: mpsc::Sender<Result<Asked, Failed>>,
}

impl Rig {
    fn new(name: &str, utc_offset_minutes: Option<i32>) -> Self {
        Self::with(name, utc_offset_minutes, false)
    }

    /// With `act`, as `lamp-policy --act progress`.
    fn with(name: &str, utc_offset_minutes: Option<i32>, act: bool) -> Self {
        Self::all(name, utc_offset_minutes, act, false)
    }

    /// With `rules`, as `lamp-policy --rules`.
    fn all(name: &str, utc_offset_minutes: Option<i32>, act: bool, rules: bool) -> Self {
        let root = PathBuf::from(format!("/tmp/lpp-{name}-{}", std::process::id()));
        let log_dir = std::env::temp_dir().join(format!(
            "lamp-policy-progress-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&log_dir);
        fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap().rescan_every(ms(20));
        let [heard, _] = Speech::TOPICS;
        let declared = |topic: &str, declaration, source| {
            bus.publisher_declared(topic, &declaration, source).unwrap()
        };
        let (shown_to, shown) = mpsc::channel();
        let (replies, replied) = mpsc::channel();
        let (said_so, saying) = mpsc::channel();
        // Someone is at the lamp, and the room is quiet.
        let mut eyes = declared(Faces::TOPIC, Faces::declaration(), "eyes");
        let mut room = declared(RoomState::TOPIC, RoomState::declaration(), "voice");
        let mut rig = Self {
            decisions: bus
                .subscribe_typed(Decision::TOPIC, "test", Decision::TYPE)
                .unwrap(),
            status: declared(TaskStatus::TOPIC, TaskStatus::declaration(), "tasks"),
            voice: declared(VoiceState::TOPIC, VoiceState::declaration(), "voice"),
            heard: declared(heard, Speech::declaration(), "voice"),
            rules: declared(Rules::TOPIC, Rules::declaration(), "voice"),
            stop: Arc::new(AtomicBool::new(false)),
            running: None,
            saying,
            shown,
            replies,
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
        let progress = progress::Options {
            deadline: ms(900),
            local: utc_offset_minutes.map(|minutes| Arc::new(progress::LocalTime::Offset(minutes))),
            note_every: ms(1_500),
            quiet_for: ms(400),
            act,
            rules,
        };
        let judge = Judge {
            shown: shown_to,
            replies: replied,
        };
        let (bus, stop) = (rig.bus.clone(), Arc::clone(&rig.stop));
        rig.running = Some(thread::spawn(move || {
            policy::run_with_progress(
                &bus,
                Attention,
                &options,
                &stop,
                &mut |note| {
                    let _ = said_so.send(note);
                },
                judge,
                progress,
            )
        }));
        let started = Instant::now();
        while !rig
            .bus
            .subscribers(TaskStatus::TOPIC)
            .unwrap()
            .iter()
            .any(|listener| listener.socket.starts_with("policy.") && listener.listening)
        {
            assert!(started.elapsed() < WAIT, "the policy did not subscribe");
            thread::sleep(ms(5));
        }
        thread::sleep(ms(60));
        rig.voice(Voice::Resting);
        room.publish_json(0, &RoomState { state: Room::Quiet })
            .unwrap();
        // The eyes publish their list over and over, as they do.
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
        rig
    }

    fn voice(&mut self, state: Voice) {
        self.voice.publish_json(0, &VoiceState { state }).unwrap();
    }

    /// The voice says what the person at the lamp asked, typed as the
    /// seventh's #904 keeps it: never to bring news of work unprompted, or
    /// nothing.
    fn keep(&mut self, no_news: bool) {
        let rules = Rules {
            person: "p-be9688998f085098".into(),
            quiet: false,
            quiet_mornings: false,
            rules: u32::from(no_news),
            hello: Unprompted::default(),
            news: Unprompted {
                never: no_news,
                ..Unprompted::default()
            },
        };
        self.rules.publish_json(0, &rules).unwrap();
    }

    /// The tasks part says task 3 is at `changes`, with `signs_seen`.
    fn work(&mut self, state: AgentState, changes: u32, signs_seen: &[Sign]) {
        let status = TaskStatus {
            store_id: STORE.into(),
            run: RUN,
            unread: vec![],
            settled: Vec::new(),
            tasks: vec![TaskProgress {
                task: 3,
                engine: "claude".into(),
                state,
                working_s: Some(600),
                last_active_unix_ms: None,
                read_unix_ms: 1_792_022_712_345,
                changes,
                changed: changes > 0,
                new_share: 0.5,
                signs: signs_seen.to_vec(),
                signs_seen: signs_seen.to_vec(),
            }],
        };
        status.check().unwrap();
        self.status.publish_json(0, &status).unwrap();
    }

    /// The state the progress judge was shown next.
    fn shown(&self) -> Value {
        self.shown.recv_timeout(WAIT).expect("a progress question")
    }
    fn not_asked_for(&self, time: Duration) {
        if let Ok(state) = self.shown.recv_timeout(time) {
            panic!("asked about {state}");
        }
    }
    fn answer(&self, three: [f32; 3]) {
        self.replies
            .send(Ok(asked(progress::questions(), &three)))
            .unwrap();
    }

    /// The next progress decision on the bus.
    fn decision(&mut self) -> Decision {
        let started = Instant::now();
        loop {
            let left = WAIT.saturating_sub(started.elapsed());
            let message = self.decisions.recv(left).unwrap().expect("a decision");
            let decision: Decision = message.parse().unwrap();
            decision.check().unwrap();
            if decision.about == "progress" {
                return decision;
            }
        }
    }

    /// The progress lines of the decisions log once there are `lines`.
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
                .filter(|line| line["about"] == "progress")
                .collect();
            if found.len() >= lines {
                assert_eq!(found.len(), lines);
                return found;
            }
            assert!(started.elapsed() < WAIT, "{} of {lines} lines", found.len());
            thread::sleep(ms(5));
        }
    }

    /// The newest report the policy gave of itself, as keys and values.
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
fn a_change_is_asked_about_once_decided_in_shadow_and_written_down_with_its_state() {
    let mut rig = Rig::new("once", Some(420));
    // What the work said before is where judging begins.
    rig.work(AgentState::Working, 2, &[Sign::Found]);
    rig.not_asked_for(ms(500));
    // A change with a new sign: asked about.
    rig.work(AgentState::Working, 3, &[Sign::TestsPass, Sign::Found]);
    let state = rig.shown();
    assert_eq!(state["work"]["agent"], "Claude Code");
    assert_eq!(state["work"]["minutes_at_it"], 10.0);
    assert_eq!(
        state["work"]["new_signs_of_a_step_forward"],
        serde_json::json!(["the tests pass", "something was found"])
    );
    assert_eq!(state["room"]["someone_at_the_lamp"], true);
    assert_eq!(state["room"]["room"], "quiet: nobody else has been talking");
    assert!(state["room"]["hour_of_day"].as_u64().unwrap() < 24);
    assert_eq!(
        state["room"]["times_the_lamp_spoke_up_unasked_in_the_last_hour"],
        0
    );
    rig.answer([0.9, 0.9, 0.1]);
    let decided = rig.decision();
    assert_eq!(decided.choice, "say_now");
    assert_eq!(decided.by, "stand-in-1.0-20261015");
    assert!(decided.shadow);
    assert_eq!(
        decided.task.as_deref(),
        Some(&*format!("{STORE}:3:3@{RUN}"))
    );
    assert!((decided.probability - 0.729).abs() < 1e-3);
    assert_eq!(decided.answers.len(), 3);
    let line = &rig.log(1)[0];
    assert_eq!(line["context"]["task"], 3);
    assert_eq!(line["context"]["change"], 3);
    assert_eq!(line["state"], state);
    assert_eq!(line["judgement"]["choice"], "say_now");
    assert_eq!(line["asked"], "progress-stand-in");
    assert!(line.get("guard").is_none());
    assert_eq!(line["shadow"], true);
    // The same change again is not asked about.
    rig.work(AgentState::Working, 3, &[Sign::TestsPass, Sign::Found]);
    rig.not_asked_for(ms(400));
    // A newer one within the note's quarter hour is held (here 1.5 s) and
    // asked about once it has passed, knowing what was told.
    rig.work(
        AgentState::Working,
        4,
        &[Sign::TestsPass, Sign::Found, Sign::PullRequest],
    );
    rig.not_asked_for(ms(600));
    let state = rig.shown();
    assert_eq!(state["work"]["progress_notes_told_for_this_work"], 1);
    assert_eq!(
        state["work"]["new_signs_of_a_step_forward"],
        serde_json::json!(["a pull request was opened"])
    );
    assert_eq!(
        state["work"]["signs_the_person_was_already_told_about"],
        serde_json::json!(["the tests pass", "something was found"])
    );
    assert_eq!(
        state["room"]["times_the_lamp_spoke_up_unasked_in_the_last_hour"],
        1
    );
    rig.answer([0.9, 0.9, 0.9]);
    assert_eq!(rig.decision().choice, "wait");
    let report = rig.report();
    for (key, value) in [
        ("progress_decided", "2"),
        ("progress_said", "1"),
        ("progress_held_recent", "1"),
        ("progress_held_talking", "0"),
    ] {
        assert!(report.contains(&(key.into(), value.into())), "{report:?}");
    }
    // Without `--rules` the people's rules are not listened to (#813).
    assert!(!report.iter().any(|(key, _)| key == "progress_held_rule"));
    assert!(
        !rig.bus
            .subscribers(Rules::TOPIC)
            .unwrap_or_default()
            .iter()
            .any(|listener| listener.socket.starts_with("policy."))
    );
    // In shadow the report says nothing of acting.
    assert!(report.iter().all(|(key, _)| key != "progress_acts"));
    let notes = rig.stop();
    assert!(notes.iter().any(|note| matches!(
        note,
        Note::ProgressDecided {
            task: 3,
            change: 3,
            choice: Choice::SayNow,
            guard: None,
            ..
        }
    )));
    // What the policy said of it holds no words.
    let said = notes
        .iter()
        .find(|note| matches!(note, Note::ProgressDecided { .. }))
        .unwrap()
        .to_string();
    assert!(
        said.starts_with("progress decision 1 task=3 change=3 choice=say_now"),
        "{said}"
    );
}

#[test]
fn never_while_anyone_is_talking_to_the_lamp() {
    let mut rig = Rig::new("talking", None);
    rig.work(AgentState::Working, 0, &[]);
    rig.voice(Voice::Listening);
    thread::sleep(ms(100));
    rig.work(AgentState::Working, 1, &[Sign::BuildWorks]);
    rig.not_asked_for(ms(600));
    // Done listening, but something was just heard: still not.
    rig.voice(Voice::Resting);
    rig.heard
        .publish_json(
            0,
            &Speech {
                text: "thanks".into(),
                finished: true,
                locale: None,
                request: None,
            },
        )
        .unwrap();
    rig.not_asked_for(ms(250));
    // Quiet for long enough: asked. Someone speaks while the answer is
    // out: a yes is turned to wait, and asked again once quiet.
    let _ = rig.shown();
    rig.voice(Voice::Listening);
    thread::sleep(ms(100));
    rig.answer([1.0, 1.0, 0.0]);
    let held = rig.decision();
    assert_eq!((held.choice.as_str(), held.probability), ("wait", 1.0));
    assert_eq!(rig.log(1)[0]["guard"], "talking");
    rig.voice(Voice::Resting);
    let again = rig.shown();
    assert_eq!(again["work"]["progress_notes_told_for_this_work"], 0);
    rig.answer([1.0, 1.0, 0.0]);
    assert_eq!(rig.decision().choice, "say_now");
    let report = rig.report();
    assert!(
        report.contains(&("progress_held_talking".into(), "1".into())),
        "{report:?}"
    );
    rig.stop();
}

#[test]
fn acting_a_say_now_goes_out_for_real_while_the_attention_question_stays_shadow() {
    let mut rig = Rig::with("act", None, true);
    rig.work(AgentState::Working, 0, &[]);
    rig.work(AgentState::Working, 1, &[Sign::TestsPass]);
    let _ = rig.shown();
    rig.answer([0.9, 0.9, 0.1]);
    let decided = rig.decision();
    assert_eq!(
        (decided.choice.as_str(), decided.shadow),
        ("say_now", false)
    );
    assert_eq!(
        decided.task.as_deref(),
        Some(&*format!("{STORE}:3:1@{RUN}"))
    );
    // The shadow log is kept, and says the decision acted.
    let line = &rig.log(1)[0];
    assert_eq!(line["shadow"], false);
    assert_eq!(line["judgement"]["choice"], "say_now");
    // What was heard is still only watched: its decision stays shadow.
    rig.heard
        .publish_json(
            0,
            &Speech {
                text: "what time is it".into(),
                finished: true,
                locale: None,
                request: None,
            },
        )
        .unwrap();
    let attention = loop {
        let message = rig.decisions.recv(WAIT).unwrap().expect("a decision");
        let decision: Decision = message.parse().unwrap();
        if decision.about == "attention" {
            break decision;
        }
    };
    assert!(attention.shadow && attention.task.is_none());
    // A note it chose is said by the voice and counted once, as a turn the
    // lamp began itself, not again as a note: the next question shows none
    // until the voice speaks.
    rig.voice(Voice::Resting);
    thread::sleep(ms(1_600));
    rig.work(AgentState::Working, 2, &[Sign::TestsPass, Sign::Done]);
    let state = rig.shown();
    assert_eq!(
        state["room"]["times_the_lamp_spoke_up_unasked_in_the_last_hour"],
        0
    );
    rig.answer([0.1, 0.9, 0.1]);
    assert_eq!(rig.decision().choice, "wait");
    let report = rig.report();
    assert!(
        report.contains(&("progress_acts".into(), "true".into())),
        "{report:?}"
    );
    assert!(
        report.contains(&("shadow".into(), "true".into())),
        "{report:?}"
    );
    rig.stop();
}

#[test]
fn work_not_going_on_is_not_asked_about_and_a_failed_model_leaves_it_to_the_rule() {
    let mut rig = Rig::new("rule", None);
    rig.work(AgentState::Working, 0, &[]);
    // Finished or asking: the voice's own news tells that, not progress.
    rig.work(AgentState::Idle, 1, &[Sign::Done]);
    rig.work(AgentState::NeedsInput, 2, &[Sign::Done]);
    rig.not_asked_for(ms(500));
    rig.work(AgentState::Working, 3, &[Sign::Done]);
    let _ = rig.shown();
    rig.replies.send(Err(Failed::TimedOut)).unwrap();
    let ruled = rig.decision();
    // Someone is there, nothing was said, the room is quiet: the rule says
    // it, with its own unmeasured probability, and nothing from a model.
    assert_eq!(
        (ruled.choice.as_str(), ruled.by.as_str(), ruled.probability),
        ("say_now", "rule", progress::RULE_YES)
    );
    assert!(ruled.answers.is_empty());
    let line = &rig.log(1)[0];
    assert_eq!(line["failed"], "timed_out");
    assert_eq!(line["judgement"]["from_rule"], true);
    rig.stop();
}

/// #813: with `--rules`, what the person at the lamp asked about it
/// speaking first (`memory/rules`, the typed `news.never`) holds progress
/// news: no model is asked while it holds and `progress_held_rule` counts
/// it. Forgotten, the change is asked about. Without `--rules` the topic is
/// not listened to, and the report has no such line (the other tests).
#[test]
fn with_rules_a_kept_rule_holds_progress_news_and_no_model_is_asked() {
    let mut rig = Rig::all("rules", None, false, true);
    assert!(
        rig.bus
            .subscribers(Rules::TOPIC)
            .unwrap()
            .iter()
            .any(|listener| listener.socket.starts_with("policy."))
    );
    rig.keep(true);
    thread::sleep(ms(100));
    rig.work(AgentState::Working, 0, &[]);
    thread::sleep(ms(100));
    rig.work(AgentState::Working, 1, &[Sign::BuildWorks]);
    rig.not_asked_for(ms(600));
    let report = rig.report();
    assert!(
        report.contains(&("progress_held_rule".into(), "1".into())),
        "{report:?}"
    );
    rig.keep(false);
    let state = rig.shown();
    assert_eq!(state["work"]["state"], "working");
    rig.answer([1.0, 1.0, 0.0]);
    assert_eq!(rig.decision().choice, "say_now");
    rig.stop();
}

#[test]
fn without_the_switch_the_tasks_topic_is_not_listened_to_and_the_report_is_as_before() {
    let root = PathBuf::from(format!("/tmp/lpp-off-{}", std::process::id()));
    let log_dir =
        std::env::temp_dir().join(format!("lamp-policy-progress-off-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let bus = Bus::new(&root).unwrap().rescan_every(ms(20));
    let mut reports = bus
        .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
        .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let running = {
        let (bus, stop) = (bus.clone(), Arc::clone(&stop));
        let options = Options {
            log_dir: log_dir.clone(),
            deadline: ms(900),
            pause: ms(300),
            act_attention: false,
        };
        thread::spawn(move || policy::run(&bus, Attention, &options, &stop, &mut |_| {}))
    };
    let report: Diagnostic = loop {
        let message = reports.recv(WAIT).unwrap().expect("a report");
        let report: Diagnostic = message.parse().unwrap();
        if report.name == "policy" {
            break report;
        }
    };
    let keys: Vec<&str> = report.values.iter().map(|pair| pair.key.as_str()).collect();
    assert_eq!(keys, ["decided", "by_rule", "shadow"]);
    assert!(
        bus.subscribers(TaskStatus::TOPIC)
            .unwrap_or_default()
            .is_empty()
    );
    assert!(!root.join("tasks").exists());
    stop.store(true, Ordering::Relaxed);
    running.join().unwrap().unwrap();
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&log_dir);
}
