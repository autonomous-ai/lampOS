//! The policy's loop on a real bus, in a directory of the test's own, with
//! a stand-in for the decision model: the test plays the voice, and reads
//! what the policy publishes, writes down and says about itself.
use lamp_bus::{Bus, Publisher, Subscriber};
use lamp_msgs::{Addressed, Decision, Diagnostic, Level, NameHeard, Speech, Voice, VoiceState};
use lamp_policy::{
    attention::{self, Context, Room},
    decide::{Answer, Asked, Connection, Decider, Failed, Question, Usage},
    policy::{self, Beside, Note, Options, Timing, Totals},
};
use serde_json::{Value, json};
use std::{
    fs, io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

/// The longest any wait for the policy lasts. Every wait returns as soon as
/// what it waits for has come, so this is only a bound against a host that
/// stalls the policy's thread for seconds (#1060).
const WAIT: Duration = Duration::from_secs(20);
const PAUSE: Duration = Duration::from_millis(300);
/// The loop's own waits run short (#1806): the lamp's words waited for
/// 400 ms after its first sound, not a second, and the eyes' list good for
/// a second, not two. Both long enough beside the tests' own steps for a
/// busy host's stall.
const QUICK: Timing = Timing {
    words_for: Duration::from_millis(400),
    seen_for: Duration::from_secs(1),
};
/// The seven answers of the first measured example, in the order asked.
const SEVEN: [f32; 7] = [0.07, 0.06, 0.28, 0.73, 0.06, 0.05, 0.09];

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn asked(seven: [f32; 7]) -> Asked {
    Asked {
        answers: attention::questions()
            .into_iter()
            .zip(seven)
            .map(|((name, _), yes)| (name, Answer::Noul(yes)))
            .collect(),
        model: "stand-in-1.0-20261014".into(),
        took: ms(1),
        usage: Some(Usage {
            input_tokens: Some(1066),
            output_tokens: Some(134),
            cost: Some(4.4772e-05),
        }),
        connection: None,
    }
}

/// A decision model played by the test: it tells the test each state it
/// is shown, and answers as the test's script says.
struct StandIn<F> {
    script: F,
    shown: mpsc::Sender<Value>,
    /// How often it was told to get ready.
    readied: Arc<AtomicUsize>,
}

impl<F: FnMut(&Value) -> Result<Asked, Failed>> Decider for StandIn<F> {
    fn ask(
        &mut self,
        state: &str,
        questions: &[(String, Question)],
        deadline: Duration,
    ) -> Result<Asked, Failed> {
        // What it is asked is the seven questions, with the deadline the
        // policy was given.
        assert_eq!(questions, attention::questions());
        assert_eq!(deadline, ms(900));
        let state: Value = serde_json::from_str(state).expect("the state is JSON");
        let _ = self.shown.send(state.clone());
        (self.script)(&state)
    }

    fn ready(&mut self, within: Duration) {
        // No longer than an asking may take.
        assert!(within <= ms(900), "{within:?}");
        self.readied.fetch_add(1, Ordering::SeqCst);
    }

    fn name(&self) -> &str {
        "stand-in"
    }
}

/// The policy running on a bus of the test's own, and the test as the
/// voice on the same bus.
struct Rig {
    root: PathBuf,
    log_dir: PathBuf,
    bus: Bus,
    stop: Arc<AtomicBool>,
    running: Option<thread::JoinHandle<io::Result<Totals>>>,
    /// What the policy says about itself, as it says it, and what of that
    /// the test has taken in so far.
    saying: mpsc::Receiver<Note>,
    notes: Vec<Note>,
    heard: Publisher,
    said: Publisher,
    state: Publisher,
    decisions: Subscriber,
    /// Each state the stand-in was shown.
    states: mpsc::Receiver<Value>,
    /// How often the stand-in was told to get ready.
    readied: Arc<AtomicUsize>,
    /// Every piece of text the test published as heard or said.
    spoken: Vec<String>,
}

impl Rig {
    fn new<F>(name: &str, script: F) -> Self
    where
        F: FnMut(&Value) -> Result<Asked, Failed> + Send + 'static,
    {
        Self::with(name, script, None, |_| {})
    }

    /// With a `lease`, the bus uses it, and the test's publishers declare
    /// nothing and are not listed until they first publish: a voice that
    /// is not there yet. `before` is given the rig before the policy
    /// starts.
    fn with<F>(name: &str, script: F, lease: Option<Duration>, before: impl FnOnce(&Self)) -> Self
    where
        F: FnMut(&Value) -> Result<Asked, Failed> + Send + 'static,
    {
        Self::with_pause(name, script, lease, PAUSE, before)
    }

    /// With its own pause before an utterance is judged: longer than the
    /// test's own sleeps between the pieces of one utterance by more than a
    /// busy host's stall, so that the pieces stay one utterance (#1060).
    fn paused<F>(name: &str, pause: Duration, script: F) -> Self
    where
        F: FnMut(&Value) -> Result<Asked, Failed> + Send + 'static,
    {
        Self::with_pause(name, script, None, pause, |_| {})
    }

    /// With the second attention judgement on (`--attention-next`, #1040).
    fn next<F>(name: &str, script: F) -> Self
    where
        F: FnMut(&Value) -> Result<Asked, Failed> + Send + 'static,
    {
        let beside = || Beside {
            attention_next: true,
            ..Beside::default()
        };
        Self::started(name, script, None, PAUSE, |_| {}, beside)
    }

    /// With the loop's own waits [`QUICK`].
    fn quick<F>(name: &str, script: F) -> Self
    where
        F: FnMut(&Value) -> Result<Asked, Failed> + Send + 'static,
    {
        let beside = || Beside {
            timing: QUICK,
            ..Beside::default()
        };
        Self::started(name, script, None, PAUSE, |_| {}, beside)
    }

    /// With these kinds decided by the rule alone (`--rule-decides`, #705).
    fn rule_decides<F>(name: &str, script: F, kinds: &'static [&'static str]) -> Self
    where
        F: FnMut(&Value) -> Result<Asked, Failed> + Send + 'static,
    {
        let beside = move || Beside {
            rule_decides: kinds
                .iter()
                .map(|kind| lamp_policy::replay::Kind::parse(kind).unwrap())
                .collect(),
            ..Beside::default()
        };
        Self::started(name, script, None, PAUSE, |_| {}, beside)
    }

    fn with_pause<F>(
        name: &str,
        script: F,
        lease: Option<Duration>,
        pause: Duration,
        before: impl FnOnce(&Self),
    ) -> Self
    where
        F: FnMut(&Value) -> Result<Asked, Failed> + Send + 'static,
    {
        Self::started(name, script, lease, pause, before, Beside::default)
    }

    fn started<F>(
        name: &str,
        script: F,
        lease: Option<Duration>,
        pause: Duration,
        before: impl FnOnce(&Self),
        beside: impl FnOnce() -> Beside + Send + 'static,
    ) -> Self
    where
        F: FnMut(&Value) -> Result<Asked, Failed> + Send + 'static,
    {
        // A root short enough for a socket's path, which the system's own
        // temporary directory on a Mac is not.
        let root = PathBuf::from(format!("/tmp/lp-{name}-{}", std::process::id()));
        let log_dir =
            std::env::temp_dir().join(format!("lamp-policy-log-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&log_dir);
        let _ = fs::remove_file(&log_dir);
        fs::create_dir_all(&root).unwrap();
        // Publishers look for new subscribers every 20 ms, not 200.
        let bus = Bus::new(&root).unwrap().rescan_every(ms(20));
        let bus = bus.lease(lease.unwrap_or(lamp_bus::DEFAULT_LEASE));
        let [heard, said] = Speech::TOPICS;
        // As the voice publishes: each topic declared with its type.
        let publisher = |topic: &str, declaration: lamp_bus::Declaration| match lease {
            None => bus
                .publisher_declared(topic, &declaration, "voice")
                .unwrap(),
            Some(_) => bus.publisher(topic, declaration.kind, "voice").unwrap(),
        };
        let (shown, states) = mpsc::channel();
        let (said_so, saying) = mpsc::channel();
        let mut rig = Self {
            // The test listens before the policy has anything to say.
            decisions: bus
                .subscribe_typed(Decision::TOPIC, "test", Decision::TYPE)
                .unwrap(),
            heard: publisher(heard, Speech::declaration()),
            said: publisher(said, Speech::declaration()),
            state: publisher(VoiceState::TOPIC, VoiceState::declaration()),
            stop: Arc::new(AtomicBool::new(false)),
            running: None,
            saying,
            notes: Vec::new(),
            states,
            readied: Arc::new(AtomicUsize::new(0)),
            spoken: Vec::new(),
            root,
            log_dir,
            bus,
        };
        before(&rig);
        let (bus, stop) = (rig.bus.clone(), Arc::clone(&rig.stop));
        let options = Options {
            log_dir: rig.log_dir.clone(),
            deadline: ms(900),
            pause,
            act_attention: false,
            // A report every 20 ms, not the lamp's second, so that a wait for
            // the next one takes no real time (#1128).
            report_every: ms(20),
        };
        let stand_in = StandIn {
            script,
            shown,
            readied: Arc::clone(&rig.readied),
        };
        rig.running = Some(thread::spawn(move || {
            let beside = beside();
            policy::run_beside(
                &bus,
                stand_in,
                &options,
                &stop,
                &mut |note| {
                    let _ = said_so.send(note);
                },
                beside,
            )
        }));
        // The policy has subscribed once its three sockets are there.
        let started = Instant::now();
        let listening = |topic: &str| {
            rig.bus
                .subscribers(topic)
                .unwrap()
                .iter()
                .any(|listener| listener.socket.starts_with("policy.") && listener.listening)
        };
        while !(listening(heard) && listening(said) && listening(VoiceState::TOPIC)) {
            assert!(started.elapsed() < WAIT, "the policy did not subscribe");
            thread::sleep(ms(5));
        }
        rig
    }

    /// The voice heard `text`: one piece, not marked as the last.
    fn hear(&mut self, text: &str) {
        self.hear_as(text, false, None);
    }

    fn hear_as(&mut self, text: &str, finished: bool, request: Option<u64>) {
        self.hear_at(text, finished, request, lamp_ipc::monotonic_us());
    }

    /// Heard, with the stamp `at_us` the voice sent it with (#1652).
    fn hear_sent(&mut self, text: &str, at_us: u64) {
        self.hear_at(text, false, None, at_us);
    }

    fn hear_at(&mut self, text: &str, finished: bool, request: Option<u64>, at_us: u64) {
        self.spoken.push(text.to_owned());
        let piece = Speech {
            text: text.to_owned(),
            finished,
            locale: None,
            request,
        };
        let sent = self.heard.publish_json(at_us, &piece).unwrap();
        assert_eq!((sent.delivered, sent.dropped), (1, 0), "{text:?}");
    }

    /// The lamp said `text` in answer to `request`.
    fn say(&mut self, text: &str, request: u64) {
        self.say_sent(text, request, lamp_ipc::monotonic_us());
    }

    /// The same, with the stamp `at_us` it was sent with (#1652).
    fn say_sent(&mut self, text: &str, request: u64, at_us: u64) {
        self.spoken.push(text.to_owned());
        let piece = Speech {
            text: text.to_owned(),
            finished: false,
            locale: None,
            request: Some(request),
        };
        assert_eq!(self.said.publish_json(at_us, &piece).unwrap().delivered, 1);
    }

    /// The voice's state, stamped as the voice stamps it: now.
    fn voice(&mut self, state: Voice) {
        self.voice_sent(state, lamp_ipc::monotonic_us());
    }

    /// The voice's state, with the stamp `at_us` it was sent with (#1652).
    fn voice_sent(&mut self, state: Voice, at_us: u64) {
        let sent = self
            .state
            .publish_json(at_us, &VoiceState { state })
            .unwrap();
        assert_eq!(sent.delivered, 1);
    }

    /// The next decision the policy publishes.
    fn decision(&mut self) -> Decision {
        let message = self
            .decisions
            .recv(WAIT)
            .unwrap()
            .expect("a decision on the bus");
        assert_eq!(message.header.source, "policy");
        assert_eq!(message.header.type_name.as_deref(), Some(Decision::TYPE));
        let decision: Decision = message.parse().unwrap();
        assert_eq!(decision.check(), Ok(()));
        decision
    }

    /// Until a report of the policy's says `key` (#1060): what it found in
    /// its once-a-second look, waited for, not slept for.
    fn reported(&self, key: &str) {
        let mut reports = self
            .bus
            .subscribe_typed(Diagnostic::TOPIC, "test_reported", Diagnostic::TYPE)
            .unwrap();
        let started = Instant::now();
        loop {
            let left = WAIT.saturating_sub(started.elapsed());
            let message = reports.recv(left).unwrap().expect("a report");
            let report: Diagnostic = message.parse().unwrap();
            if report.values.iter().any(|fact| fact.key == key) {
                return;
            }
            assert!(started.elapsed() < WAIT, "no report with {key}");
        }
    }

    fn no_decision_for(&mut self, time: Duration) {
        assert!(self.decisions.recv(time).unwrap().is_none());
    }

    /// The state the stand-in was shown next.
    fn shown(&self) -> Value {
        self.states.recv_timeout(WAIT).expect("a question asked")
    }

    /// The decisions log once it has `lines` lines. A decision is
    /// published before it is written down, so a test that has just read
    /// one from the bus may be ahead of the log by a moment.
    fn log(&self, lines: usize) -> Vec<Value> {
        let started = Instant::now();
        loop {
            let log = self.log_now();
            if log.len() >= lines {
                assert_eq!(log.len(), lines, "more lines than decisions");
                return log;
            }
            assert!(started.elapsed() < WAIT, "{} of {lines} lines", log.len());
            thread::sleep(ms(5));
        }
    }

    /// The lines of the decisions log, every day's file together.
    fn log_now(&self) -> Vec<Value> {
        self.lines_of("decisions-")
    }

    /// The outcomes log once it has `lines` lines.
    fn outcomes(&self, lines: usize) -> Vec<Value> {
        let started = Instant::now();
        loop {
            let log = self.lines_of("outcomes-");
            if log.len() >= lines {
                assert_eq!(log.len(), lines, "more outcomes than expected");
                return log;
            }
            assert!(started.elapsed() < WAIT, "{} of {lines} lines", log.len());
            thread::sleep(ms(5));
        }
    }

    /// The lines of the files of one kind. The directory holds the two
    /// logs and nothing else.
    fn lines_of(&self, kind: &str) -> Vec<Value> {
        let Ok(files) = fs::read_dir(&self.log_dir) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = files.map(|file| file.unwrap().path()).collect();
        files.sort();
        files
            .iter()
            .filter(|file| {
                let name = file.file_name().unwrap().to_str().unwrap().to_owned();
                assert!(
                    (name.starts_with("decisions-") || name.starts_with("outcomes-"))
                        && name.ends_with(".jsonl"),
                    "{name}"
                );
                name.starts_with(kind)
            })
            .flat_map(|file| whole_records(&fs::read_to_string(file).unwrap()))
            .collect()
    }

    /// Wait until the policy has done with its decision of this number:
    /// published it, tried to write it down, and said so. A decision read
    /// from the bus may not have reached the log yet.
    fn done_with(&mut self, number: u64) {
        loop {
            let note = self.saying.recv_timeout(WAIT).expect("a decision made");
            self.notes.push(note);
            if matches!(note, Note::Decided { number: of, .. } if of == number) {
                return;
            }
        }
    }

    /// Stop the policy. What it counted and what it said about itself,
    /// none of which may hold anything that was heard or said.
    fn stop(mut self) -> (Totals, Vec<Note>) {
        self.stop.store(true, Ordering::SeqCst);
        let totals = self.running.take().unwrap().join().unwrap();
        let mut notes = std::mem::take(&mut self.notes);
        notes.extend(self.saying.try_iter());
        for note in &notes {
            let line = note.to_string();
            for text in &self.spoken {
                let text = text.trim();
                assert!(
                    text.len() < 4 || !line.contains(text),
                    "{line:?} holds {text:?}"
                );
            }
        }
        (totals.unwrap(), notes)
    }
}

/// The records of a log's text, its whole lines only: the log may be
/// mid-write while a test waits on it, and its last line is a record only
/// once its newline is there (an ARM64 run on #3829 read half of one). A
/// whole line that isn't JSON still fails the test.
fn whole_records(text: &str) -> Vec<Value> {
    let whole = text.rfind('\n').map_or("", |end| &text[..end]);
    whole
        .lines()
        .map(|line| serde_json::from_str(line).expect("a line of JSON"))
        .collect()
}

#[test]
fn a_log_line_still_being_written_is_not_read_yet() {
    // Reviewer-two's P3 on #3836: the race, made certain.
    assert_eq!(whole_records(""), Vec::<Value>::new());
    assert_eq!(whole_records("{\"a\":1"), Vec::<Value>::new());
    assert_eq!(whole_records("{\"a\":1}\n{\"b\":"), vec![json!({"a": 1})]);
    assert_eq!(
        whole_records("{\"a\":1}\n{\"b\":2}\n"),
        vec![json!({"a": 1}), json!({"b": 2})]
    );
}

#[test]
#[should_panic(expected = "a line of JSON")]
fn a_whole_log_line_that_isnt_json_still_fails() {
    whole_records("{\"a\":1}\nnot json\n");
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(running) = self.running.take() {
            let _ = running.join();
        }
        let _ = fs::remove_dir_all(&self.root);
        let _ = fs::remove_dir_all(&self.log_dir);
        let _ = fs::remove_file(&self.log_dir);
    }
}

fn decided(notes: &[Note]) -> usize {
    notes
        .iter()
        .filter(|note| matches!(note, Note::Decided { .. }))
        .count()
}

#[test]
fn pieces_become_one_utterance_and_one_decision_is_published_and_logged() {
    let mut rig = Rig::new("one", |_| Ok(asked(SEVEN)));
    rig.hear(" Hey");
    thread::sleep(ms(60));
    rig.hear(" Lamp,");
    thread::sleep(ms(60));
    rig.hear(" what time is it?");
    let last = Instant::now();
    let decision = rig.decision();
    let after = last.elapsed();
    // Not before the pause that ends the utterance, and soon after it:
    // the stand-in answers at once.
    assert!(after >= PAUSE, "{after:?}");
    assert!(after < PAUSE + ms(500), "{after:?}");
    println!("decision {after:?} after the last piece, of which {PAUSE:?} is the pause");
    // The room is not known and the lamp has said nothing, so the sum is
    // the bias and the seven answers: 0.51558, which is 0.6261.
    assert!(
        (decision.probability - 0.6261).abs() < 0.0005,
        "{}",
        decision.probability
    );
    let probability = decision.probability;
    assert_eq!(
        decision,
        Decision {
            about: "attention".into(),
            choice: "look_up".into(),
            probability,
            answers: attention::questions()
                .into_iter()
                .zip(SEVEN)
                .map(|((name, _), yes)| (name, yes))
                .collect(),
            by: "stand-in-1.0-20261014".into(),
            took_ms: decision.took_ms,
            shadow: true,
            request: None,
            task: None,
            arrival: None,
            ask: None,
            proposal: None,
        }
    );
    assert!(decision.took_ms < 500);
    // What the model was shown: the pieces as one, in the measured shape.
    let context = Context {
        heard: "Hey Lamp, what time is it?".into(),
        room: Room::Unknown,
        lamp_is_speaking: false,
        lamp_last_said: None,
        seconds_since_lamp_spoke: None,
        name_heard: None,
        like_asker: None,
    };
    assert_eq!(rig.shown(), attention::state(&context));
    // One utterance, one decision, one line.
    rig.no_decision_for(PAUSE + ms(200));
    let log = rig.log(1);
    let line = &log[0];
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let at = line["at_unix_ms"].as_u64().unwrap();
    assert!(at <= now && now - at < 5_000, "{at} and {now}");
    assert_eq!(
        *line,
        json!({
            "at_unix_ms": at,
            "about": "attention",
            "shadow": true,
            "context": {
                "heard": "Hey Lamp, what time is it?",
                "room": "unknown",
                "lamp_is_speaking": false,
                "lamp_last_said": null,
                "seconds_since_lamp_spoke": null,
            },
            "in_conversation": false,
            "state": attention::state(&context),
            "asked": "stand-in",
            "answers": {
                "calls_lamp": 0.07, "names_someone_else": 0.06, "others_business": 0.28,
                "lamp_could_answer": 0.73, "follows_lamp": 0.06, "listening_noise": 0.05,
                "asks_to_stop": 0.09,
            },
            "judgement": {
                "probability": line["judgement"]["probability"],
                "choice": "look_up",
                "from_rule": false,
            },
            "by": "stand-in-1.0-20261014",
            "took_ms": decision.took_ms,
            "waited_ms": line["waited_ms"],
            "usage": {"input_tokens": 1066, "output_tokens": 134, "cost": 4.4772e-05},
            "pieces": 3,
            "cut": false,
        })
    );
    let logged = line["judgement"]["probability"].as_f64().unwrap();
    assert!((logged - f64::from(probability)).abs() < 1e-6);
    // It waited for the pause, and for little else.
    let waited = line["waited_ms"].as_u64().unwrap();
    assert!((300..800).contains(&waited), "{waited}");
    let (totals, notes) = rig.stop();
    assert_eq!(
        totals,
        Totals {
            utterances: 1,
            decided: 1,
            ..Totals::default()
        }
    );
    assert_eq!(decided(&notes), 1);
    assert!(matches!(
        notes.last(),
        Some(Note::Decided {
            number: 1,
            choice: attention::Choice::LookUp,
            by_rule: false,
            pieces: 3,
            bytes: 26,
            ..
        })
    ));
}

/// The reviewer on #908 (06:40): turn 1's words, and 50 ms later turn 2's,
/// neither marked finished, through the real Policy loop and bus. Two
/// questions and two decisions, each carrying its own turn's number: never
/// one decision about both carrying the later number, which would hold or
/// release turn 2's action on turn 1's words.
#[test]
fn pieces_of_two_turns_are_judged_apart_each_with_its_own_number() {
    let mut rig = Rig::new("two-turns", |_| {
        Ok(asked([0.9, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0]))
    });
    rig.hear_as("Hey Lamp, turn it up. ", false, Some(1));
    thread::sleep(ms(50));
    rig.hear_as("Thailand.", false, Some(2));
    let (first, second) = (rig.decision(), rig.decision());
    assert_eq!((first.request, second.request), (Some(1), Some(2)));
    assert_eq!(rig.shown()["heard"], "Hey Lamp, turn it up.");
    assert_eq!(rig.shown()["heard"], "Thailand.");
    let log = rig.log(2);
    assert_eq!(
        (log[0]["request"].clone(), log[1]["request"].clone()),
        (json!(1), json!(2))
    );
    rig.stop();
}

#[test]
fn a_piece_marked_finished_is_judged_without_the_pause_and_carries_its_request() {
    let mut rig = Rig::new("finished", |_| {
        Ok(asked([0.9, 0.0, 0.0, 0.9, 0.0, 0.0, 0.0]))
    });
    rig.hear("Lamp,");
    thread::sleep(ms(40));
    let last = Instant::now();
    rig.hear_as(" good night.", true, Some(12));
    let decision = rig.decision();
    let after = last.elapsed();
    assert!(after < PAUSE, "{after:?}");
    println!("decision {after:?} after a piece marked finished");
    assert_eq!(decision.request, Some(12));
    assert_eq!(rig.shown()["heard"], "Lamp, good night.");
    assert_eq!(rig.log(1)[0]["request"], 12);
    assert_eq!(rig.log(1)[0]["pieces"], 2);
    rig.stop();
}

#[test]
fn once_the_voice_is_no_longer_listening_a_decision_does_not_wait_the_pause() {
    let mut rig = Rig::new("stopped", |_| Ok(asked(SEVEN)));
    // The person is speaking: what is heard waits the whole pause.
    rig.voice(Voice::Listening);
    thread::sleep(ms(60));
    let sent = Instant::now();
    rig.hear("What is");
    rig.decision();
    assert!(sent.elapsed() >= PAUSE, "{:?}", sent.elapsed());
    // They have stopped, and the text of what they said comes.
    rig.voice(Voice::Thinking);
    thread::sleep(ms(60));
    let sent = Instant::now();
    rig.hear("What is two plus two?");
    rig.decision();
    let after = sent.elapsed();
    println!("decision {after:?} after the text of something already said");
    assert!(after < PAUSE - ms(100), "{after:?}");
    rig.stop();
}

#[test]
fn the_judge_is_told_to_get_ready_each_time_someone_begins_to_speak() {
    let mut rig = Rig::new("ready", |_| Ok(asked(SEVEN)));
    let readied = Arc::clone(&rig.readied);
    let reaches = |count: usize| {
        let since = Instant::now();
        while readied.load(Ordering::SeqCst) < count {
            assert!(since.elapsed() < WAIT, "not told {count} times");
            thread::sleep(ms(2));
        }
        // And not once more than that.
        thread::sleep(ms(80));
        assert_eq!(readied.load(Ordering::SeqCst), count);
    };
    reaches(0);
    let began = Instant::now();
    rig.voice(Voice::Listening);
    reaches(1);
    println!("told to get ready within {:?}", began.elapsed() - ms(80));
    // Still listening is not beginning again.
    rig.voice(Voice::Listening);
    reaches(1);
    // What they said is asked about as ever, and the lamp answers.
    rig.voice(Voice::Thinking);
    rig.hear("What is two plus two?");
    rig.decision();
    rig.voice(Voice::Speaking);
    rig.voice(Voice::Resting);
    reaches(1);
    // The next thing said.
    rig.voice(Voice::Listening);
    reaches(2);
    rig.stop();
}

/// The rigs here report every 20 ms, so nothing else pins the lamp's own
/// period (#1490's reviewer): `lamp-policy` passes `REPORT_EVERY`.
#[test]
fn the_lamps_policy_reports_once_a_second() {
    assert_eq!(policy::REPORT_EVERY, Duration::from_secs(1));
}

#[test]
fn the_policy_says_how_it_is_once_a_second_and_that_it_decides_without_a_model() {
    let mut rig = Rig::new("report", |state| {
        if state["heard"] == "Is anyone there?" {
            Err(Failed::TimedOut)
        } else {
            Ok(asked(SEVEN))
        }
    });
    let mut reports = rig
        .bus
        .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
        .unwrap();
    let mut next = || -> Diagnostic {
        reports
            .recv(WAIT)
            .unwrap()
            .expect("a report within a few seconds")
            .parse()
            .unwrap()
    };
    let well = next();
    assert_eq!((well.level, well.name.as_str()), (Level::Ok, "policy"));
    assert_eq!(well.message, "");
    let facts = |report: &Diagnostic| -> Vec<(String, String)> {
        report
            .values
            .iter()
            .map(|fact| (fact.key.clone(), fact.value.clone()))
            .collect()
    };
    assert_eq!(
        facts(&well),
        [
            ("decided".to_string(), "0".to_string()),
            ("by_rule".to_string(), "0".to_string()),
            ("shadow".to_string(), "true".to_string()),
        ]
    );
    // The model fails: the rule decides, and the report says so.
    rig.hear("Is anyone there?");
    assert_eq!(rig.decision().by, "rule");
    let since = Instant::now();
    let warned = loop {
        let report = next();
        if report.level == Level::Warn {
            break report;
        }
        assert!(since.elapsed() < WAIT, "{report:?}");
    };
    assert_eq!(
        warned.message,
        "the decision model is not answering: the rule decides until it does"
    );
    assert_eq!(
        facts(&warned)[..2],
        facts(&well)[..2]
            .iter()
            .map(|(key, _)| (key.clone(), "1".to_string()))
            .collect::<Vec<_>>()[..]
    );
    // It answers again, and the policy is well again.
    rig.hear("What is two plus two?");
    assert_ne!(rig.decision().by, "rule");
    let since = Instant::now();
    loop {
        let report = next();
        if report.level == Level::Ok {
            assert_eq!(facts(&report)[0].1, "2");
            assert_eq!(facts(&report)[1].1, "1");
            break;
        }
        assert!(since.elapsed() < WAIT, "{report:?}");
    }
    rig.stop();
}

#[test]
fn the_lamps_name_heard_while_someone_spoke_is_joined_to_what_they_said() {
    let mut rig = Rig::new("name", |_| Ok(asked(SEVEN)));
    // Nothing on the lamp listens for its name: not known, and not written.
    rig.voice(Voice::Listening);
    rig.voice(Voice::Thinking);
    rig.hear("What time is it?");
    let unknown = rig.decision();
    rig.done_with(1);
    assert!(rig.log(1)[0]["context"].get("name_heard").is_none());
    rig.voice(Voice::Resting);

    // Something starts to listen for it. The policy looks once a second.
    let mut name = rig
        .bus
        .publisher_declared(NameHeard::TOPIC, &NameHeard::declaration(), "name")
        .unwrap();
    name.alive().unwrap();
    rig.reported("name_listening");
    // Someone speaks, and the name is not heard: known, and nothing.
    rig.voice(Voice::Listening);
    thread::sleep(ms(100));
    rig.voice(Voice::Thinking);
    rig.hear("Did you send the report?");
    let not_called = rig.decision();
    rig.done_with(2);
    assert_eq!(rig.log(2)[1]["context"]["name_heard"], 0.0);
    rig.voice(Voice::Resting);
    thread::sleep(ms(100));

    // Someone speaks and the name is heard as they begin: the hearing is
    // said 400 ms after the name began, which was just before the voice
    // began to listen.
    name.alive().unwrap();
    rig.voice(Voice::Listening);
    thread::sleep(ms(300));
    name.publish_json(
        0,
        &NameHeard {
            score: 0.97,
            ago_ms: 400,
        },
    )
    .unwrap();
    thread::sleep(ms(200));
    rig.voice(Voice::Thinking);
    rig.hear("What time is it?");
    let called = rig.decision();
    rig.done_with(3);
    let log = rig.log(3);
    let heard = log[2]["context"]["name_heard"].as_f64().unwrap();
    assert!((heard - 0.97).abs() < 1e-6, "{heard}");
    // The same answers from the model: with the name it is more likely
    // for the lamp than without, and than when nothing listened.
    assert!(
        called.probability > not_called.probability + 0.3,
        "{called:?} {not_called:?}"
    );
    assert!(called.probability > unknown.probability);
    // The model was shown the same state each time.
    let shown: Vec<Value> = rig.states.try_iter().collect();
    assert_eq!(shown.len(), 3);
    assert!(shown.iter().all(|state| state.get("name_heard").is_none()));
    rig.voice(Voice::Resting);
    // Half a second clear of the hearing's `NAME_BEFORE` window. With
    // 100 ms the next listening began about 0.9 s after the last, and the
    // hearing, dated 100 ms before that, was outside the window by a few
    // ms only: a turn of the policy that read it a moment late dated it
    // inside, under load (#1488).
    thread::sleep(ms(600));

    // The next thing said, with no name: the hearing before is not its.
    name.alive().unwrap();
    rig.voice(Voice::Listening);
    thread::sleep(ms(100));
    rig.voice(Voice::Thinking);
    rig.hear("And tomorrow?");
    rig.decision();
    rig.done_with(4);
    assert_eq!(rig.log(4)[3]["context"]["name_heard"], 0.0);
    rig.stop();
}

/// #4306: how alike a request's voice was to the conversation's asker's, as
/// the voice says it on `attention/addressed`, is joined to what was heard
/// of that request by its number and written down with the decision. The
/// model is not shown it, and at its weight of 0 the decision is the one
/// without it.
#[test]
fn the_likeness_to_the_asker_said_for_a_request_is_joined_to_what_was_heard_of_it() {
    let mut rig = Rig::new("asker", |_| Ok(asked(SEVEN)));
    // The policy subscribes to it beside the voice's own three topics.
    let started = Instant::now();
    while !rig
        .bus
        .subscribers(Addressed::TOPIC)
        .unwrap()
        .iter()
        .any(|listener| listener.socket.starts_with("policy.") && listener.listening)
    {
        assert!(started.elapsed() < WAIT, "the policy did not subscribe");
        thread::sleep(ms(5));
    }
    let mut addressed = rig
        .bus
        .publisher_declared(Addressed::TOPIC, &Addressed::declaration(), "voice")
        .unwrap();
    let said = |request: u64, like_asker: Option<f32>| Addressed {
        session: 1,
        request,
        began_us: 1_000_000,
        ended_us: Some(2_000_000),
        rise_db: None,
        near: None,
        looks: 0,
        facing_height: None,
        facing_share: None,
        facing_close: None,
        name_score: 0.0,
        addressed: false,
        like_asker,
    };
    // Said for request 7 with no likeness: nothing is written.
    rig.voice(Voice::Listening);
    rig.voice(Voice::Thinking);
    assert_eq!(
        addressed.publish_json(0, &said(7, None)).unwrap().delivered,
        1
    );
    rig.hear_as("And tomorrow?", true, Some(7));
    let without = rig.decision();
    rig.done_with(1);
    assert!(rig.log(1)[0]["context"].get("like_asker").is_none());
    rig.voice(Voice::Resting);
    // Said for request 8, and for 9 before it: 8's own is the one taken.
    rig.voice(Voice::Listening);
    rig.voice(Voice::Thinking);
    for (request, like) in [(9, 0.21), (8, 0.86)] {
        let sent = addressed
            .publish_json(0, &said(request, Some(like)))
            .unwrap();
        assert_eq!(sent.delivered, 1);
    }
    thread::sleep(ms(100));
    rig.hear_as("And tomorrow?", true, Some(8));
    let with = rig.decision();
    rig.done_with(2);
    let like = rig.log(2)[1]["context"]["like_asker"].as_f64().unwrap();
    assert!((like - 0.86).abs() < 1e-6, "{like}");
    // The same answers, the same decision, to the bit.
    assert_eq!(with.probability.to_bits(), without.probability.to_bits());
    assert_eq!(with.choice, without.choice);
    // A request the voice said nothing for has none, though others had.
    rig.voice(Voice::Resting);
    rig.voice(Voice::Listening);
    rig.voice(Voice::Thinking);
    rig.hear_as("And tomorrow?", true, Some(10));
    rig.decision();
    rig.done_with(3);
    assert!(rig.log(3)[2]["context"].get("like_asker").is_none());
    // A message that does not add up is passed over.
    rig.voice(Voice::Resting);
    rig.voice(Voice::Listening);
    rig.voice(Voice::Thinking);
    let sent = addressed.publish_json(0, &said(11, Some(1.5))).unwrap();
    assert_eq!(sent.delivered, 1);
    thread::sleep(ms(100));
    rig.hear_as("And tomorrow?", true, Some(11));
    rig.decision();
    rig.done_with(4);
    assert!(rig.log(4)[3]["context"].get("like_asker").is_none());
    // The model was never shown it.
    let shown: Vec<Value> = rig.states.try_iter().collect();
    assert_eq!(shown.len(), 4);
    assert!(shown.iter().all(|state| state.get("like_asker").is_none()));
    assert!(shown.windows(2).all(|two| two[0] == two[1]));
    rig.stop();
}

/// The lamp keeps its own waits: only a test runs them short (#1806).
#[test]
fn the_loops_own_waits_are_the_lamps_unless_a_test_says_otherwise() {
    assert_eq!(Beside::default().timing, Timing::LAMP);
    assert_eq!(Timing::default(), Timing::LAMP);
    assert_eq!(
        Timing::LAMP,
        Timing {
            words_for: Duration::from_secs(1),
            seen_for: Duration::from_secs(2),
        }
    );
}

#[test]
fn what_the_lamp_did_after_each_decision_is_written_beside_it() {
    let mut rig = Rig::quick("outcome", |_| Ok(asked(SEVEN)));
    // Said to someone else: the lamp does nothing about it.
    rig.voice(Voice::Listening);
    rig.voice(Voice::Thinking);
    rig.hear("Did you send the report?");
    rig.decision();
    rig.done_with(1);
    rig.voice(Voice::Resting);
    thread::sleep(ms(150));
    // Said to the lamp: it answers, after the decision.
    rig.voice(Voice::Listening);
    rig.voice(Voice::Thinking);
    rig.hear("Hey Lamp, what is two plus two?");
    rig.decision();
    rig.done_with(2);
    let decided = Instant::now();
    thread::sleep(ms(200));
    rig.say("Four.", 31);
    rig.voice(Voice::Speaking);
    rig.say(" Easy one.", 31);
    let spoke = decided.elapsed();
    // The first is closed by the second, and the second its words' wait
    // after the lamp's first sound.
    let outcomes = rig.outcomes(2);
    let decisions = rig.log(2);
    let [silent, answered] = [&outcomes[0], &outcomes[1]];
    assert_eq!(silent["about"], "attention");
    assert_eq!(silent["decision_at_unix_ms"], decisions[0]["at_unix_ms"]);
    assert_eq!(silent["choice"], decisions[0]["judgement"]["choice"]);
    assert_eq!(silent["lamp"], "silent");
    assert_eq!(silent["overtaken"], true);
    for absent in [
        "spoke_after_ms",
        "said_after_ms",
        "heard_to_sound_ms",
        "request",
        "said",
    ] {
        assert!(silent.get(absent).is_none(), "{absent}: {silent}");
    }
    assert_eq!(answered["decision_at_unix_ms"], decisions[1]["at_unix_ms"]);
    assert_eq!(answered["lamp"], "spoke");
    assert_eq!(answered["overtaken"], false);
    assert_eq!(answered["request"], 31);
    assert_eq!(answered["said"], "Four. Easy one.");
    let after = answered["spoke_after_ms"].as_i64().unwrap();
    let said_after = answered["said_after_ms"].as_i64().unwrap();
    let from_heard = answered["heard_to_sound_ms"].as_i64().unwrap();
    println!(
        "spoke {after} ms after the decision ({spoke:?} by the test), said {said_after}, {from_heard} from the last piece"
    );
    assert!(
        (150..=200 + spoke.as_millis() as i64).contains(&after),
        "{after}"
    );
    assert!(said_after <= after && said_after >= 150, "{said_after}");
    assert!(from_heard > after, "{from_heard}");
    let (_, notes) = rig.stop();
    let outcomes: Vec<&Note> = notes
        .iter()
        .filter(|note| matches!(note, Note::Outcome { .. }))
        .collect();
    assert!(matches!(
        outcomes[..],
        [
            Note::Outcome {
                number: 1,
                spoke: false,
                overtaken: true,
                logged: true,
                ..
            },
            Note::Outcome {
                number: 2,
                spoke: true,
                overtaken: false,
                logged: true,
                ..
            }
        ]
    ));
}

#[test]
fn a_lamp_that_spoke_before_it_was_decided_is_written_with_a_time_less_than_nothing() {
    // A model that takes 600 ms: the lamp is quicker.
    let mut rig = Rig::quick("outcome-late", |_| {
        thread::sleep(ms(600));
        Ok(asked(SEVEN))
    });
    rig.voice(Voice::Listening);
    rig.voice(Voice::Thinking);
    rig.hear("What time is it?");
    // The lamp begins 150 ms after the model is asked, so the policy has
    // taken up what was heard by then. A time is when the loop takes a
    // message up, and each turn it takes what the lamp is doing before what
    // is heard: begun 150 ms after the piece was sent instead, a loop held
    // up that long took both in one turn, the lamp's sound first, and the
    // outcome said `silent` (the workspace sweep, 19 October 03:38; #1569
    // row 25).
    rig.shown();
    thread::sleep(ms(150));
    rig.say("It is noon.", 8);
    rig.voice(Voice::Speaking);
    rig.decision();
    let outcome = &rig.outcomes(1)[0];
    assert_eq!(outcome["lamp"], "spoke");
    assert_eq!(outcome["request"], 8);
    assert_eq!(outcome["said"], "It is noon.");
    let after = outcome["spoke_after_ms"].as_i64().unwrap();
    println!("spoke {after} ms after the decision");
    // Before it, by about the 450 ms the model took after the lamp began,
    // and by more on a machine that is slow to run this.
    assert!((-3_000..0).contains(&after), "{after}");
    // At least the 150 ms the lamp waited after the model was asked, which
    // was after what was heard was taken up.
    let from_heard = outcome["heard_to_sound_ms"].as_i64().unwrap();
    assert!(from_heard >= 100, "{from_heard} {outcome}");
    rig.stop();
}

/// #1652: each turn the policy's loop takes what the lamp is doing before
/// what is heard, so a turn held up past the gap between the two took the
/// lamp's sound first, and the outcome said `silent` (#1569 row 25). Here the
/// piece comes in after the lamp's sound, though stamped 150 ms before it, as
/// such a turn takes them: by the stamps, the lamp spoke, 150 ms after what
/// was heard.
#[test]
fn a_piece_taken_up_after_the_lamps_sound_but_sent_before_it_is_answered_by_it() {
    let mut rig = Rig::quick("outcome-held-up", |_| {
        thread::sleep(ms(300));
        Ok(asked(SEVEN))
    });
    rig.voice(Voice::Listening);
    rig.voice(Voice::Thinking);
    let heard_at = lamp_ipc::monotonic_us();
    thread::sleep(ms(150));
    rig.say("It is noon.", 8);
    rig.voice(Voice::Speaking);
    // Turns of the loop pass, each waiting at most 20 ms: the lamp's sound
    // is taken up before the piece is sent.
    thread::sleep(ms(100));
    rig.hear_sent("What time is it?", heard_at);
    rig.decision();
    let outcome = &rig.outcomes(1)[0];
    assert_eq!(outcome["lamp"], "spoke", "{outcome}");
    assert_eq!(outcome["request"], 8, "{outcome}");
    let from_heard = outcome["heard_to_sound_ms"].as_i64().unwrap();
    assert!((150..1_000).contains(&from_heard), "{from_heard} {outcome}");
    rig.stop();
}

/// #1652: the lamp's sound stamped before what was heard began is no answer
/// to it, though it is taken up after the decision: the open watch drops it,
/// as `Watch::new` drops one taken up before (the reviewer on #1657). The
/// outcome says `silent`. A second utterance's decision closes it, not the
/// window.
#[test]
fn a_sound_stamped_before_what_was_heard_is_no_answer_to_it() {
    let mut rig = Rig::new("outcome-old-sound", |_| Ok(asked(SEVEN)));
    rig.voice(Voice::Listening);
    rig.voice(Voice::Thinking);
    let heard_at = lamp_ipc::monotonic_us();
    rig.hear("What time is it?");
    rig.decision();
    // After the decision: a sound stamped 2 s before what was heard.
    rig.voice_sent(Voice::Speaking, heard_at - 2_000_000);
    rig.voice(Voice::Listening);
    rig.hear("Is it lunch yet?");
    rig.voice(Voice::Thinking);
    rig.decision();
    let outcome = &rig.outcomes(1)[0];
    assert_eq!(outcome["lamp"], "silent", "{outcome}");
    assert_eq!(outcome["overtaken"], true, "{outcome}");
    rig.stop();
}

/// The reviewer on #1657: a recorded session played again (`lamp-bus
/// replay`) sends every message with its recorded stamp, minutes old. Each
/// counts from its taking up, as before #1652, heard pieces, the lamp's words
/// and its state alike. Timed by only some of those stamps, the lamp's sound
/// would come before what it answered, and be `silent`; timed by all of them,
/// its time from the decision would be minutes off.
#[test]
fn a_session_stamped_minutes_ago_is_timed_by_its_taking_up() {
    let mut rig = Rig::quick("outcome-replayed", |_| {
        thread::sleep(ms(300));
        Ok(asked(SEVEN))
    });
    let recorded = || lamp_ipc::monotonic_us() - 180_000_000;
    rig.voice_sent(Voice::Listening, recorded());
    rig.voice_sent(Voice::Thinking, recorded());
    rig.hear_sent("What time is it?", recorded());
    thread::sleep(ms(150));
    rig.say_sent("It is noon.", 8, recorded());
    rig.voice_sent(Voice::Speaking, recorded());
    rig.decision();
    let outcome = &rig.outcomes(1)[0];
    assert_eq!(outcome["lamp"], "spoke", "{outcome}");
    let after = outcome["spoke_after_ms"].as_i64().unwrap();
    assert!((-3_000..0).contains(&after), "{after} {outcome}");
    let from_heard = outcome["heard_to_sound_ms"].as_i64().unwrap();
    assert!((100..1_000).contains(&from_heard), "{from_heard} {outcome}");
    rig.stop();
}

#[test]
fn what_the_lamp_last_said_is_in_the_context_as_it_was_when_the_utterance_began() {
    // A pause longer than this test's 160 ms between the two pieces by more
    // than a busy host's stall, which once closed the utterance after "And"
    // (#1060).
    let mut rig = Rig::paused("said", ms(2_000), |_| Ok(asked(SEVEN)));
    // The lamp answers something, and falls silent.
    rig.voice(Voice::Speaking);
    rig.say("It is", 5);
    rig.say(" ten past four.", 5);
    thread::sleep(ms(150));
    rig.voice(Voice::Listening);
    thread::sleep(ms(400));
    // Somebody speaks, and before they have finished the lamp has begun
    // on its answer to them.
    rig.hear("And");
    thread::sleep(ms(80));
    rig.say("Tomorrow it will rain.", 6);
    rig.voice(Voice::Speaking);
    thread::sleep(ms(80));
    rig.hear(" tomorrow?");
    let decision = rig.decision();
    let shown = rig.shown();
    assert_eq!(shown["heard"], "And tomorrow?");
    assert_eq!(shown["lamp_last_said"], "It is ten past four.");
    assert_eq!(shown["lamp_is_speaking_now"], false);
    // It stopped about 0.4 s before the first piece.
    let seconds = shown["seconds_since_lamp_spoke"].as_f64().unwrap();
    assert!((0.3..=0.9).contains(&seconds), "{seconds}");
    let line = &rig.log(1)[0];
    assert_eq!(line["context"]["lamp_last_said"], "It is ten past four.");
    assert_eq!(line["context"]["lamp_is_speaking"], false);
    assert_eq!(line["in_conversation"], true);
    assert_eq!(line["state"], shown);
    // In conversation takes 0.206 from the sum: 0.30958, which is 0.5768.
    assert!(
        (decision.probability - 0.5768).abs() < 0.0005,
        "{}",
        decision.probability
    );
    // Said while the lamp is speaking: it is, and what it is saying now
    // is what it last said.
    thread::sleep(ms(100));
    rig.hear_as("Stop.", true, None);
    rig.decision();
    let shown = rig.shown();
    assert_eq!(shown["lamp_is_speaking_now"], true);
    assert_eq!(shown["lamp_last_said"], "Tomorrow it will rain.");
    assert_eq!(shown["seconds_since_lamp_spoke"], 0.0);
    // Half a minute of nothing is not played here, so the other side of
    // the twenty seconds is shown by a lamp that only kept quiet aloud.
    rig.voice(Voice::Resting);
    rig.say("...", 7);
    thread::sleep(ms(100));
    rig.hear_as("Did the invoice go out?", true, None);
    rig.decision();
    let shown = rig.shown();
    assert_eq!(shown["lamp_last_said"], Value::Null);
    assert_eq!(shown["seconds_since_lamp_spoke"], Value::Null);
    let last = rig.log(3).pop().unwrap();
    assert_eq!(last["context"]["lamp_last_said"], "...");
    assert_eq!(last["in_conversation"], false);
    rig.stop();
}

/// #1040, in shadow: with `--attention-next` each decision's line carries a
/// second judgement, raised in conversation by `follows_lamp` above 0.55
/// (3 for each unit), and the diagnostics count the decisions where the two
/// choices differ. Nothing else changes: the same reply, judged with the
/// switch and without, publishes the same decision (apart from how long it
/// took) and writes the same line (apart from `next` and its times), and
/// without the switch the report has no `attention_next_differs` (the
/// reviewer on #1106).
#[test]
fn with_attention_next_a_second_judgement_is_written_beside_and_nothing_published_changes() {
    let follows = attention::questions()
        .iter()
        .position(|(name, _)| name == "follows_lamp")
        .unwrap();
    let mut seven = SEVEN;
    seven[follows] = 1.0;
    let logit = |p: f64| (p / (1.0 - p)).ln();
    let run = |on: bool| {
        let name = if on { "next-on" } else { "next-off" };
        let mut rig = if on {
            Rig::next(name, move |_| Ok(asked(seven)))
        } else {
            Rig::new(name, move |_| Ok(asked(seven)))
        };
        // The lamp says something; a reply to it, in conversation.
        rig.voice(Voice::Speaking);
        rig.say("It will rain tomorrow.", 5);
        thread::sleep(ms(150));
        rig.voice(Voice::Listening);
        thread::sleep(ms(200));
        rig.hear_as("I should bring an umbrella then.", true, None);
        let decision = rig.decision();
        let line = rig.log(1).remove(0);
        // The policy's next report after the decision.
        let mut reports = rig
            .bus
            .subscribe_typed(Diagnostic::TOPIC, "test_next", Diagnostic::TYPE)
            .unwrap();
        let started = Instant::now();
        let report = loop {
            let left = WAIT.saturating_sub(started.elapsed());
            let message = reports.recv(left).unwrap().expect("a report");
            let report: Diagnostic = message.parse().unwrap();
            if report.values.iter().any(|fact| fact.key == "decided") {
                break report;
            }
        };
        rig.stop();
        let differs = report
            .values
            .iter()
            .find(|fact| fact.key == "attention_next_differs")
            .map(|fact| fact.value.clone());
        (decision, line, differs)
    };
    let (decision, line, differs) = run(true);
    assert_eq!(line["in_conversation"], true);
    let today = line["judgement"]["probability"].as_f64().unwrap();
    assert!((f64::from(decision.probability) - today).abs() < 1e-6);
    assert_eq!(decision.choice, "look_up");
    // 3 × (1.0 − 0.55) on the sum: from look_up to answer.
    let next = &line["next"];
    let raised = logit(next["probability"].as_f64().unwrap()) - logit(today);
    assert!((raised - 1.35).abs() < 1e-3, "{raised}");
    assert_eq!(next["choice"], "answer");
    assert_eq!(next["from_rule"], false);
    assert_eq!(differs.as_deref(), Some("1"));
    // Without the switch: the same decision, the same line but `next`, and
    // nothing counted.
    let (off_decision, off_line, off_differs) = run(false);
    assert_eq!(
        Decision {
            took_ms: decision.took_ms,
            ..off_decision
        },
        decision
    );
    // What depends on the moment the test ran, not on the switch.
    let timeless = |line: &Value| {
        let mut line = line.clone();
        let object = line.as_object_mut().unwrap();
        for key in ["next", "at_unix_ms", "took_ms", "waited_ms"] {
            object.remove(key);
        }
        for part in ["context", "state"] {
            if let Some(part) = object.get_mut(part).and_then(Value::as_object_mut) {
                part.remove("seconds_since_lamp_spoke");
            }
        }
        line
    };
    assert!(off_line.get("next").is_none(), "{off_line}");
    assert_eq!(timeless(&off_line), timeless(&line));
    assert_eq!(off_differs, None);
}

#[test]
fn how_the_room_is_and_who_the_eyes_see_are_taken_from_the_bus() {
    use lamp_msgs::{Face, Faces, RoomState};
    let mut rig = Rig::quick("room", |_| Ok(asked(SEVEN)));
    // Before anything has been said of the room it is not known, and the
    // eyes have said nothing.
    rig.hear_as("Hello?", true, None);
    let first = rig.decision();
    assert_eq!(rig.shown()["room"], "unknown");
    assert!(rig.log(1)[0].get("seen").is_none());
    // The voice says the room is quiet. The eyes see three faces: one near
    // and turned to the lamp, one turned to it from across the room, one
    // near and turned away.
    let mut room = rig
        .bus
        .publisher_declared(RoomState::TOPIC, &RoomState::declaration(), "voice")
        .unwrap();
    let mut eyes = rig
        .bus
        .publisher_declared(Faces::TOPIC, &Faces::declaration(), "eyes")
        .unwrap();
    room.publish_json(
        0,
        &RoomState {
            state: lamp_msgs::Room::Quiet,
        },
    )
    .unwrap();
    let face = |id: &str, ymin: f32, ymax: f32, facing: bool| Face {
        speaking: None,
        id: id.into(),
        xmin: 0.4,
        ymin,
        xmax: 0.6,
        ymax,
        confidence: 0.9,
        yaw: None,
        facing: Some(facing),
    };
    eyes.publish_json(
        0,
        &Faces {
            faces: vec![
                face("f1", 0.30, 0.60, true),
                face("f2", 0.10, 0.20, true),
                face("f3", 0.20, 0.50, false),
            ],
        },
    )
    .unwrap();
    // The policy takes them in on its next turn of the loop.
    thread::sleep(ms(150));
    rig.hear_as("Hello?", true, None);
    let second = rig.decision();
    assert_eq!(rig.shown()["room"], "quiet: nobody else has been talking");
    let line = &rig.log(2)[1];
    assert_eq!(line["context"]["room"], "quiet");
    assert_eq!(line["seen"]["faces"], 3);
    assert_eq!(line["seen"]["near"], 2);
    assert_eq!(line["seen"]["facing"], 1);
    assert!(line["seen"]["age_ms"].as_u64().unwrap() < 1_000);
    // The same answers count for more in a quiet room. What the eyes saw
    // is written down and changes nothing yet.
    assert!(second.probability > first.probability);
    // A list older than its time (two seconds on the lamp, one here) says
    // nothing about who is there now.
    thread::sleep(ms(1_100));
    rig.hear_as("Hello?", true, None);
    rig.decision();
    assert!(rig.log(3)[2].get("seen").is_none());
    assert_eq!(rig.log(3)[2]["context"]["room"], "quiet");
    rig.stop();
}

#[test]
fn when_the_model_fails_the_rule_decides_and_the_failure_is_said_once() {
    let mut asked_times = 0;
    let mut rig = Rig::new("rule", move |_| {
        asked_times += 1;
        match asked_times {
            1 => Err(Failed::TimedOut),
            2 => Err(Failed::Refused(503)),
            3 => Ok(asked(SEVEN)),
            // An answer that leaves a question out, from a judge that
            // does not check its own.
            4 => {
                let mut part = asked(SEVEN);
                part.answers.remove("follows_lamp");
                Ok(part)
            }
            _ => Ok(asked(SEVEN)),
        }
    });
    // Nothing is known of the room and the lamp has not spoken: the rule
    // stays quiet.
    rig.hear_as("Is the printer working?", true, Some(3));
    let ruled = rig.decision();
    assert_eq!(
        ruled,
        Decision {
            about: "attention".into(),
            choice: "stay_quiet".into(),
            probability: 0.26,
            answers: Default::default(),
            by: "rule".into(),
            took_ms: ruled.took_ms,
            shadow: true,
            request: Some(3),
            task: None,
            arrival: None,
            ask: None,
            proposal: None,
        }
    );
    // In conversation, the rule answers.
    rig.say("Shall I go on?", 4);
    thread::sleep(ms(80));
    rig.hear_as("Yes please.", true, None);
    let ruled = rig.decision();
    assert_eq!((ruled.choice.as_str(), ruled.probability), ("answer", 0.74));
    assert_eq!(ruled.by, "rule");
    rig.hear_as("What is the capital of Peru?", true, None);
    assert_eq!(rig.decision().by, "stand-in-1.0-20261014");
    rig.hear_as("And of Chile?", true, None);
    let ruled = rig.decision();
    assert_eq!((ruled.by.as_str(), ruled.answers.len()), ("rule", 0));
    rig.hear_as("Thanks.", true, None);
    assert_eq!(rig.decision().by, "stand-in-1.0-20261014");

    let log = rig.log(5);
    let failed: Vec<&Value> = log.iter().map(|line| &line["failed"]).collect();
    assert_eq!(
        failed,
        [
            &json!("timed_out"),
            &json!("refused"),
            &Value::Null,
            &json!("missing"),
            &Value::Null
        ]
    );
    assert_eq!(
        log[0]["failed_why"],
        "the decision model did not answer before the deadline"
    );
    assert_eq!(
        log[1]["failed_why"],
        "the decision model's service refused with HTTP 503"
    );
    assert_eq!(
        log[3]["failed_why"],
        "the decision model's answer leaves out follows_lamp"
    );
    for (line, by_rule) in log.iter().zip([true, true, false, true, false]) {
        assert_eq!(line["judgement"]["from_rule"], by_rule);
        assert_eq!(line["asked"], "stand-in");
        assert_eq!(line["by"] == "rule", by_rule);
        assert_eq!(
            line["answers"].as_object().unwrap().len(),
            if by_rule { 0 } else { 7 }
        );
        // What was heard is in the log whoever decided.
        assert!(line["context"]["heard"].as_str().unwrap().len() > 5);
        assert_eq!(line["state"]["heard"], line["context"]["heard"]);
    }
    assert_eq!(log[1]["judgement"]["choice"], "answer");
    // What it cost is kept though the answers could not be used.
    assert_eq!(log[3]["usage"]["input_tokens"], 1066);
    assert_eq!(log[0].get("usage"), None);

    let (totals, notes) = rig.stop();
    assert_eq!((totals.decided, totals.by_rule), (5, 3));
    let about_the_model: Vec<&Note> = notes
        .iter()
        .filter(|note| {
            matches!(
                note,
                Note::ModelFailing { .. } | Note::ModelAnswering { .. }
            )
        })
        .collect();
    assert_eq!(
        about_the_model,
        [
            &Note::ModelFailing { kind: "timed_out" },
            &Note::ModelAnswering { failures: 2 },
            &Note::ModelFailing { kind: "missing" },
            &Note::ModelAnswering { failures: 1 },
        ]
    );
    assert_eq!(decided(&notes), 5);
}

#[test]
fn one_question_is_out_at_a_time_and_at_most_eight_utterances_wait() {
    // The model does not answer until the test lets it.
    let (release, held) = mpsc::channel::<()>();
    let mut rig = Rig::new("queue", move |_| {
        held.recv_timeout(WAIT).expect("released");
        Ok(asked(SEVEN))
    });
    rig.hear_as("kumquat 1", true, Some(1));
    assert_eq!(rig.shown()["heard"], "kumquat 1");
    // While that one is out the bus is still read: ten more are heard to
    // their end, and what the lamp does meanwhile is taken in.
    rig.say("Mango.", 30);
    for number in 2..=11 {
        thread::sleep(ms(15));
        rig.hear_as(&format!("kumquat {number}"), true, Some(number));
    }
    thread::sleep(ms(300));
    // Nothing else has been asked, and nothing decided.
    assert!(rig.states.try_recv().is_err());
    rig.no_decision_for(ms(50));
    for _ in 0..9 {
        release.send(()).unwrap();
    }
    // The first, and the eight that came last: the two that had waited
    // longest were let go.
    let kept = [1, 4, 5, 6, 7, 8, 9, 10, 11];
    for number in kept {
        assert_eq!(rig.decision().request, Some(number));
        if number > 1 {
            let shown = rig.shown();
            assert_eq!(shown["heard"], format!("kumquat {number}"));
            assert_eq!(shown["lamp_last_said"], "Mango.");
        }
    }
    rig.no_decision_for(ms(200));
    assert!(rig.states.try_recv().is_err());
    let log = rig.log(9);
    let logged: Vec<u64> = log
        .iter()
        .map(|line| line["request"].as_u64().unwrap())
        .collect();
    assert_eq!(logged, kept);
    // The ones that waited say how long.
    assert!(log[0]["waited_ms"].as_u64().unwrap() < 100);
    assert!(log[8]["waited_ms"].as_u64().unwrap() >= 300);
    let (totals, notes) = rig.stop();
    assert_eq!(
        totals,
        Totals {
            utterances: 11,
            decided: 9,
            dropped: 2,
            ..Totals::default()
        }
    );
    let about_the_queue: Vec<&Note> = notes
        .iter()
        .filter(|note| matches!(note, Note::Dropping | Note::CaughtUp { .. }))
        .collect();
    assert_eq!(
        about_the_queue,
        [&Note::Dropping, &Note::CaughtUp { dropped: 2 }]
    );
}

#[test]
fn a_voice_that_is_not_there_yet_is_said_once_and_waited_for() {
    // A lease of 600 ms, so that three seconds are not spent waiting for
    // one to run out.
    let mut rig = Rig::with("nobody", |_| Ok(asked(SEVEN)), Some(ms(600)), |_| {});
    thread::sleep(ms(1_000));
    rig.voice(Voice::Resting);
    rig.say("Good morning.", 1);
    rig.hear_as("Good morning, Lamp.", true, None);
    let decision = rig.decision();
    assert_eq!(decision.by, "stand-in-1.0-20261014");
    let (totals, notes) = rig.stop();
    assert_eq!(totals.decided, 1);
    for topic in ["voice/heard", "voice/said", "voice/state"] {
        let about: Vec<&Note> = notes
            .iter()
            .filter(|note| {
                matches!(note, Note::SourceGone { topic: of, .. } | Note::SourceBack { topic: of } if *of == topic)
            })
            .collect();
        assert_eq!(
            about,
            [
                &Note::SourceGone { topic, times: 1 },
                &Note::SourceBack { topic }
            ],
            "{topic}"
        );
    }
}

#[test]
fn a_log_that_cannot_be_written_is_said_once_and_decisions_go_on() {
    // Where the log's directory should be there is a file.
    let mut rig = Rig::with(
        "log",
        |_| Ok(asked(SEVEN)),
        None,
        |rig| {
            fs::write(&rig.log_dir, b"in the way").unwrap();
        },
    );
    for number in 1..=3 {
        rig.hear_as("Is it raining?", true, Some(number));
        assert_eq!(rig.decision().request, Some(number));
    }
    // The way is cleared once the third has been given up on, and the
    // next decision is written.
    rig.done_with(3);
    fs::remove_file(&rig.log_dir).unwrap();
    rig.hear_as("Is it raining now?", true, Some(4));
    rig.decision();
    rig.hear_as("And now?", true, Some(5));
    rig.decision();
    let logged: Vec<u64> = rig
        .log(2)
        .iter()
        .map(|line| line["request"].as_u64().unwrap())
        .collect();
    assert_eq!(logged, [4, 5]);
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &PathBuf| fs::metadata(path).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode(&rig.log_dir), 0o700);
    let file = fs::read_dir(&rig.log_dir).unwrap().next().unwrap().unwrap();
    assert_eq!(mode(&file.path()), 0o600);
    let (totals, notes) = rig.stop();
    assert_eq!(
        (totals.decided, totals.unlogged, totals.unpublished),
        (5, 3, 0)
    );
    let about_the_log: Vec<&Note> = notes
        .iter()
        .filter(|note| matches!(note, Note::LogFailing { .. } | Note::LogWriting { .. }))
        .collect();
    assert_eq!(
        about_the_log,
        [
            &Note::LogFailing {
                kind: io::ErrorKind::NotADirectory
            },
            &Note::LogWriting { missing: 3 },
        ]
    );
}

#[test]
fn a_bus_that_cannot_be_published_to_is_said_once_and_decisions_are_still_written() {
    // Where the policy's topic should be there is a file.
    let mut rig = Rig::with(
        "bus",
        |_| Ok(asked(SEVEN)),
        None,
        |rig| {
            fs::remove_dir_all(rig.root.join("policy")).unwrap();
            fs::write(rig.root.join("policy"), b"in the way").unwrap();
        },
    );
    rig.hear_as("Is it raining?", true, Some(1));
    rig.hear_as("Is it?", true, Some(2));
    // Written down, and not published.
    rig.log(2);
    // The way is cleared. Within a second or so the policy is publishing,
    // and the test, whose own socket went with the file, listens again.
    fs::remove_file(rig.root.join("policy")).unwrap();
    rig.decisions = rig
        .bus
        .subscribe_typed(Decision::TOPIC, "test", Decision::TYPE)
        .unwrap();
    let started = Instant::now();
    while rig.bus.publishers(Decision::TOPIC).unwrap().is_empty() {
        assert!(started.elapsed() < WAIT);
        thread::sleep(ms(20));
    }
    rig.hear_as("And now?", true, Some(3));
    assert_eq!(rig.decision().request, Some(3));
    rig.log(3);
    let (totals, notes) = rig.stop();
    assert_eq!(
        (totals.decided, totals.unpublished, totals.unlogged),
        (3, 2, 0)
    );
    let about_the_bus: Vec<&Note> = notes
        .iter()
        .filter(|note| matches!(note, Note::BusFailing { .. } | Note::BusWorking { .. }))
        .collect();
    assert_eq!(
        about_the_bus,
        [
            &Note::BusFailing {
                kind: io::ErrorKind::NotADirectory
            },
            &Note::BusWorking { unpublished: 2 },
        ]
    );
}

#[test]
fn messages_that_are_not_of_their_type_are_passed_over_and_nothing_of_them_is_said() {
    let mut rig = Rig::new("unreadable", |_| Ok(asked(SEVEN)));
    // A publisher that declared nothing can send anything.
    let secret = "zanzibar marmalade";
    for wrong in [
        json!({"text": secret}),
        json!({"text": 7, "finished": secret}),
        json!(secret),
    ] {
        rig.spoken.push(secret.to_owned());
        assert_eq!(rig.heard.publish_json(0, &wrong).unwrap().delivered, 1);
    }
    assert_eq!(
        rig.state
            .publish_json(0, &json!({"state": "dreaming"}))
            .unwrap()
            .delivered,
        1
    );
    rig.no_decision_for(PAUSE + ms(100));
    rig.hear_as("Lamp?", true, None);
    rig.decision();
    assert_eq!(rig.shown()["heard"], "Lamp?");
    let (totals, notes) = rig.stop();
    assert_eq!((totals.unreadable, totals.utterances), (4, 1));
    let about: Vec<&Note> = notes
        .iter()
        .filter(|note| matches!(note, Note::Unreadable { .. } | Note::Readable { .. }))
        .collect();
    assert_eq!(about.len(), 3, "{about:?}");
    assert!(about.contains(&&Note::Unreadable {
        topic: "voice/heard"
    }));
    assert!(about.contains(&&Note::Unreadable {
        topic: "voice/state"
    }));
    assert!(about.contains(&&Note::Readable {
        topic: "voice/heard",
        passed_over: 3
    }));
}

#[test]
fn a_judge_that_breaks_down_leaves_the_rule_to_decide() {
    let mut rig = Rig::new("broken", |state| {
        if state["heard"] == "Second." {
            // Not what a judge is to do; the policy outlives it.
            std::panic::resume_unwind(Box::new("the stand-in broke down"));
        }
        Ok(asked(SEVEN))
    });
    rig.hear_as("First.", true, Some(1));
    assert_eq!(rig.decision().by, "stand-in-1.0-20261014");
    rig.hear_as("Second.", true, Some(2));
    rig.hear_as("Third.", true, Some(3));
    for request in [2, 3] {
        let ruled = rig.decision();
        assert_eq!((ruled.by.as_str(), ruled.request), ("rule", Some(request)));
    }
    let log = rig.log(3);
    assert_eq!(log[1]["failed"], "unreachable");
    assert_eq!(log[2]["failed"], "unreachable");
    let (totals, notes) = rig.stop();
    assert_eq!((totals.decided, totals.by_rule), (3, 2));
    assert_eq!(
        notes
            .iter()
            .filter(|note| **note == Note::AskerGone)
            .count(),
        1
    );
}

#[test]
fn every_note_is_a_line_of_counts_and_kinds_and_can_hold_no_text() {
    // A note can be copied bit for bit, which nothing that owns text can.
    fn plain<T: Copy + Send + 'static>() {}
    plain::<Note>();
    let notes = [
        Note::Decided {
            number: 12,
            choice: attention::Choice::StayQuiet,
            probability: 0.3594,
            by_rule: false,
            took_ms: 655,
            waited_ms: 301,
            on: Some(Connection::Kept),
            pieces: 4,
            bytes: 27,
        },
        Note::ModelFailing { kind: "timed_out" },
        Note::ModelAnswering { failures: 3 },
        Note::SourceGone {
            topic: "voice/heard",
            times: 2,
        },
        Note::SourceBack {
            topic: "voice/heard",
        },
        Note::Unreadable {
            topic: "voice/said",
        },
        Note::Readable {
            topic: "voice/said",
            passed_over: 5,
        },
        Note::NotReceiving {
            topic: "voice/state",
            kind: io::ErrorKind::PermissionDenied,
        },
        Note::Receiving {
            topic: "voice/state",
        },
        Note::Dropping,
        Note::CaughtUp { dropped: 2 },
        Note::LogFailing {
            kind: io::ErrorKind::StorageFull,
        },
        Note::LogWriting { missing: 7 },
        Note::BusFailing {
            kind: io::ErrorKind::NotFound,
        },
        Note::BusWorking { unpublished: 1 },
        Note::AskerGone,
        Note::Outcome {
            number: 12,
            choice: attention::Choice::StayQuiet,
            spoke: true,
            spoke_after_ms: Some(-140),
            heard_to_sound_ms: Some(1_180),
            overtaken: false,
            logged: true,
        },
        Note::Outcome {
            number: 13,
            choice: attention::Choice::LookUp,
            spoke: false,
            spoke_after_ms: None,
            heard_to_sound_ms: None,
            overtaken: true,
            logged: false,
        },
    ];
    let lines: Vec<String> = notes.iter().map(Note::to_string).collect();
    assert_eq!(
        lines[0],
        "decision 12 about=attention choice=stay_quiet probability=0.359 by=model took_ms=655 waited_ms=301 on=kept pieces=4 bytes=27"
    );
    assert_eq!(
        lines[1],
        "the decision model is not answering (timed_out): the rule decides until it does"
    );
    assert_eq!(
        lines[11],
        "the decisions log cannot be written (no storage space): decisions go on without it"
    );
    assert_eq!(
        lines[16],
        "outcome of decision 12 choice=stay_quiet lamp=spoke spoke_after_ms=-140 heard_to_sound_ms=1180 overtaken=false logged=true"
    );
    assert_eq!(
        lines[17],
        "outcome of decision 13 choice=look_up lamp=silent spoke_after_ms=none heard_to_sound_ms=none overtaken=true logged=false"
    );
    for line in &lines {
        assert!(!line.is_empty() && !line.contains('\n'), "{line:?}");
    }
    // Each says something of its own.
    let mut distinct = lines.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), lines.len());
}

/// #705, the Policy's wait on a held call: a kind named in `--rule-decides`
/// is decided by the rule at once and the model is not asked; its line says
/// `certain` and no failure. A kind not named is asked as before.
#[test]
fn a_kind_the_rule_decides_is_not_asked_and_others_are() {
    // Nothing has said how the room is, the lamp has not spoken, and no name
    // was heard: `unknown false false none`.
    let mut rig = Rig::rule_decides(
        "rule-decides",
        |_| Ok(asked(SEVEN)),
        &["unknown false false none"],
    );
    let began = Instant::now();
    rig.hear_as("Turn it up a little.", true, None);
    let decision = rig.decision();
    assert_eq!(decision.by, "rule");
    assert_eq!(decision.took_ms, 0);
    // The rule in an unknown room, out of conversation, not called: quiet.
    assert_eq!(decision.choice, "stay_quiet");
    assert!(began.elapsed() < Duration::from_secs(5));
    let line = rig.log(1).remove(0);
    assert_eq!(line["certain"], true);
    assert!(line.get("failed").is_none(), "{line}");
    assert!(rig.states.try_recv().is_err(), "the model was asked");
    rig.stop();

    // Another kind named: this one is asked, as without the switch.
    let mut rig = Rig::rule_decides(
        "rule-asks",
        |_| Ok(asked(SEVEN)),
        &["busy false false >=0.99"],
    );
    rig.hear_as("Turn it up a little.", true, None);
    rig.shown();
    let decision = rig.decision();
    assert_ne!(decision.by, "rule");
    let line = rig.log(1).remove(0);
    assert!(line.get("certain").is_none(), "{line}");
    rig.stop();
}

/// The reviewer on #1524: a decision the rule makes for a named kind says
/// nothing of the model. After the model failed, a `certain` line leaves its
/// failing note standing: no "answering again".
#[test]
fn a_rule_decision_leaves_the_models_health_as_it_was() {
    let mut rig = Rig::rule_decides(
        "rule-health",
        |_| Err(Failed::TimedOut),
        &["unknown true false none"],
    );
    // Not in conversation: asked, and the model fails.
    rig.hear_as("Is it going to rain?", true, None);
    rig.shown();
    assert_eq!(rig.decision().by, "rule");
    // The lamp speaks; a reply to it is the named kind, decided by the rule.
    rig.voice(Voice::Speaking);
    rig.say("It will rain tomorrow.", 5);
    thread::sleep(ms(150));
    rig.voice(Voice::Listening);
    thread::sleep(ms(200));
    rig.hear_as("I should bring an umbrella then.", true, None);
    let decision = rig.decision();
    assert_eq!(decision.by, "rule");
    let log = rig.log(2);
    assert_eq!(log[0]["failed"], "timed_out");
    assert_eq!(log[1]["certain"], true);
    // Nothing was asked or shown for the certain line.
    assert!(
        log[1].get("asked").is_none() && log[1].get("state").is_none(),
        "{}",
        log[1]
    );
    assert!(log[0].get("asked").is_some() && log[0].get("state").is_some());
    let (_, notes) = rig.stop();
    let about_the_model: Vec<&Note> = notes
        .iter()
        .filter(|note| {
            matches!(
                note,
                Note::ModelFailing { .. } | Note::ModelAnswering { .. }
            )
        })
        .collect();
    assert_eq!(about_the_model, [&Note::ModelFailing { kind: "timed_out" }]);
}
