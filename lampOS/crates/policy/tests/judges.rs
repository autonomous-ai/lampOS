//! Real Unix bus and private log tests, with a controlled local stand-in for
//! the slow service. No provider, key, microphone, camera or lamp is used.
use lamp_bus::{Bus, Publisher, Subscriber};
use lamp_msgs::{Decision, Diagnostic, Room, RoomState, Speech, Voice, VoiceState};
use lamp_policy::{
    attention,
    decide::{Answer, Asked, Decider, Failed, Question},
    policy::{self, Note, Totals, judges},
};
use serde_json::Value;
use std::{
    fs, io,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(5);
fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn answer() -> Asked {
    Asked {
        answers: attention::questions()
            .into_iter()
            .zip([0.9, 0.01, 0.01, 0.9, 0.8, 0.01, 0.01])
            .map(|((name, _), p)| (name, Answer::Noul(p)))
            .collect(),
        model: "private-stand-in-response".into(),
        took: Duration::ZERO,
        usage: None,
        connection: None,
    }
}

struct Slow {
    asked: mpsc::Sender<Value>,
    responses: mpsc::Receiver<Reply>,
}
enum Reply {
    Answer(Result<Asked, Failed>),
    Exit,
}
impl Decider for Slow {
    fn ask(
        &mut self,
        state: &str,
        questions: &[(String, Question)],
        deadline: Duration,
    ) -> Result<Asked, Failed> {
        assert_eq!(questions, attention::questions());
        assert_eq!(deadline, WAIT);
        self.asked
            .send(serde_json::from_str(state).unwrap())
            .unwrap();
        match self.responses.recv_timeout(WAIT) {
            Ok(Reply::Answer(answer)) => answer,
            Ok(Reply::Exit) => panic!("injected slow worker failure"),
            Err(_) => Err(Failed::TimedOut),
        }
    }
    fn ready(&mut self, _: Duration) {
        panic!("opt-in mode must not delay its first ask with a warmup");
    }
    fn name(&self) -> &str {
        "controlled-slow"
    }
}

struct Rig {
    root: PathBuf,
    log: PathBuf,
    heard: Publisher,
    decisions: Subscriber,
    notes: mpsc::Receiver<Note>,
    taken: Vec<Note>,
    asked: mpsc::Receiver<Value>,
    responses: Option<mpsc::Sender<Reply>>,
    stop: Arc<AtomicBool>,
    done: mpsc::Receiver<io::Result<Totals>>,
    worker: Option<thread::JoinHandle<()>>,
    _room: Publisher,
    _voice: Publisher,
}

impl Rig {
    fn new(deadline: Duration) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = PathBuf::from(format!(
            "/tmp/lpj-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let log = root.join("logs");
        let bus = Bus::new(root.join("bus")).unwrap().rescan_every(ms(1));
        let heard = bus
            .publisher_declared(Speech::TOPICS[0], &Speech::declaration(), "voice")
            .unwrap();
        let mut room = bus
            .publisher_declared(RoomState::TOPIC, &RoomState::declaration(), "voice")
            .unwrap();
        let mut voice = bus
            .publisher_declared(VoiceState::TOPIC, &VoiceState::declaration(), "voice")
            .unwrap();
        let decisions = bus
            .subscribe_typed(Decision::TOPIC, "test", Decision::TYPE)
            .unwrap();
        let mut health = bus
            .subscribe_typed(Diagnostic::TOPIC, "test", Diagnostic::TYPE)
            .unwrap();
        let (ask, asked) = mpsc::channel();
        let (responses, response) = mpsc::channel();
        let (note, notes) = mpsc::channel();
        let (finish, done) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let run_stop = Arc::clone(&stop);
        let run_bus = bus.clone();
        let options = policy::Options {
            log_dir: log.clone(),
            deadline: WAIT,
            pause: ms(10),
            act_attention: false,
            // A report every 20 ms, not the lamp's second, so that a wait for
            // the next one takes no real time (#1128).
            report_every: ms(20),
        };
        let worker = thread::spawn(move || {
            let result = policy::run_with_local(
                &run_bus,
                Slow {
                    asked: ask,
                    responses: response,
                },
                &options,
                &run_stop,
                &mut |value| {
                    let _ = note.send(value);
                },
                judges::Options {
                    deadline,
                    ..judges::Options::default()
                },
            );
            let _ = finish.send(result);
        });
        health.recv(WAIT).unwrap().expect("startup report");
        room.publish_json(0, &RoomState { state: Room::Quiet })
            .unwrap();
        voice
            .publish_json(
                0,
                &VoiceState {
                    state: Voice::Listening,
                },
            )
            .unwrap();
        // A readiness handshake, not a timing measurement: a report from a
        // loop turn that began after these cues were published, so took
        // them. A report goes at most once a turn, at its end, so the first
        // one to a subscription made now was sent after the cues, and the
        // turn that ends with any later one began after it. The rig's 20 ms
        // reports make the old "next report" one that may be sent in the
        // turn the cues landed in (#1128).
        let mut after_cues = bus
            .subscribe_typed(Diagnostic::TOPIC, "test_after_cues", Diagnostic::TYPE)
            .unwrap();
        let until = Instant::now() + WAIT;
        let mut reports = 0;
        loop {
            let report: Diagnostic = after_cues
                .recv(until.saturating_duration_since(Instant::now()))
                .unwrap()
                .expect("cue receipt heartbeat")
                .parse()
                .unwrap();
            reports += 1;
            if reports >= 2
                && report
                    .values
                    .iter()
                    .any(|v| v.key == "log_written" && v.value.parse::<u64>().unwrap_or(0) >= 1)
            {
                break;
            }
        }
        Self {
            root,
            log,
            heard,
            decisions,
            notes,
            taken: Vec::new(),
            asked,
            responses: Some(responses),
            stop,
            done,
            worker: Some(worker),
            _room: room,
            _voice: voice,
        }
    }

    fn hear(&mut self, request: u64, text: &str) {
        let sent = self
            .heard
            .publish_json(
                0,
                &Speech {
                    text: text.into(),
                    finished: true,
                    locale: None,
                    request: Some(request),
                },
            )
            .unwrap();
        assert_eq!(sent.delivered, 1);
    }

    fn decision(&mut self) -> Decision {
        self.decisions
            .recv(WAIT)
            .unwrap()
            .expect("one final choice")
            .parse()
            .unwrap()
    }

    fn reply(&self, response: Result<Asked, Failed>) {
        self.responses
            .as_ref()
            .unwrap()
            .send(Reply::Answer(response))
            .unwrap();
    }

    fn note(&mut self, predicate: impl Fn(Note) -> bool) -> Note {
        let end = Instant::now() + WAIT;
        loop {
            let note = self
                .notes
                .recv_timeout(end.saturating_duration_since(Instant::now()))
                .unwrap();
            self.taken.push(note);
            if predicate(note) {
                return note;
            }
        }
    }

    fn stop(&mut self) -> Totals {
        self.stop.store(true, Ordering::Relaxed);
        let totals = self
            .done
            .recv_timeout(WAIT)
            .expect("shutdown must not join a held slow judge")
            .unwrap();
        self.worker.take().unwrap().join().unwrap();
        self.taken.extend(self.notes.try_iter());
        totals
    }

    fn rows(&self, prefix: &str) -> Vec<Value> {
        let mut paths: Vec<_> = fs::read_dir(&self.log)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(prefix)
            })
            .collect();
        paths.sort();
        paths
            .iter()
            .flat_map(|path| {
                assert_eq!(
                    fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
                fs::read_to_string(path)
                    .unwrap()
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.responses.take();
        if self.worker.is_some() {
            self.stop();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn timely_answer_logs_both_and_publishes_their_probability_combination_once() {
    let mut rig = Rig::new(ms(1000));
    rig.hear(71, "Lamp, secret authored question");
    let shown = rig.asked.recv_timeout(WAIT).unwrap();
    assert_eq!(shown["heard"], "Lamp, secret authored question");
    assert!(shown.get("name_score").is_none() && shown.get("facing").is_none());
    rig.reply(Ok(answer()));
    let decision = rig.decision();
    assert!(decision.shadow);
    assert_eq!(decision.by, "local+jev");
    assert_eq!(decision.request, Some(71));
    assert_eq!(decision.answers.len(), 7);
    assert_eq!(rig.stop().decided, 1);
    let rows = rig.rows("judges-");
    let final_row = rows.iter().find(|row| row["event"] == "choice").unwrap();
    assert_eq!(final_row["local"]["answers"].as_object().unwrap().len(), 7);
    assert_eq!(final_row["slow"]["answers"].as_object().unwrap().len(), 7);
    let local = final_row["local"]["judgement"]["probability"]
        .as_f64()
        .unwrap() as f32;
    let slow = final_row["slow"]["judgement"]["probability"]
        .as_f64()
        .unwrap() as f32;
    assert_eq!(decision.probability, 0.5 * local + 0.5 * slow);
    assert_eq!(final_row["baseline"]["kind"], "same_snapshot");
    assert_eq!(final_row["local_used"], true);
    assert_eq!(final_row["slow_used"], true);
    assert_eq!(rig.rows("models-").len(), 1);
    let printed = rig
        .taken
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    for secret in ["secret authored question", "private-stand-in-response"] {
        assert!(!printed.contains(secret));
    }
    assert!(rig.taken.iter().any(|note| matches!(
        note,
        Note::JudgesFinished {
            failed: 0,
            dropped: 0,
            pending: 0,
            drained: true,
            ..
        }
    )));
}

#[test]
fn late_slow_answer_is_audited_and_cannot_change_this_or_the_next_utterance() {
    let mut rig = Rig::new(ms(25));
    rig.hear(81, "okay");
    rig.asked.recv_timeout(WAIT).unwrap();
    let first = rig.decision();
    assert_eq!(first.choice, "look_up");
    assert_eq!(first.by, "local");
    rig.hear(82, "next authored scrap");
    let second = rig.decision();
    assert_eq!(second.choice, "look_up");
    assert_eq!(second.request, Some(82));
    assert!(matches!(
        rig.asked.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    rig.reply(Ok(answer()));
    rig.note(
        |note| matches!(note, Note::Judge(n) if n.judge == "slow" && n.number == 1 && !n.used),
    );
    assert_eq!(rig.stop().decided, 2);
    assert!(rig.decisions.recv(Duration::ZERO).unwrap().is_none());
    let rows = rig.rows("judges-");
    let choices: Vec<_> = rows.iter().filter(|r| r["event"] == "choice").collect();
    assert_eq!(choices.len(), 2);
    assert_eq!(choices[0]["result"]["guard"], "no_timely_slow_no_call");
    assert_eq!(choices[1]["reason"], "remote_busy");
    let late = rows.iter().find(|r| r["event"] == "slow").unwrap();
    assert_eq!(late["number"], 1);
    assert_eq!(late["used"], false);
    assert_eq!(late["after_final"], true);
    assert_eq!(late["answer"]["answers"].as_object().unwrap().len(), 7);
}

#[test]
fn newer_utterance_closes_the_older_choice_before_its_own() {
    let mut rig = Rig::new(ms(1000));
    rig.hear(91, "first authored scrap");
    rig.asked.recv_timeout(WAIT).unwrap();
    rig.hear(92, "second authored scrap");
    assert_eq!(rig.decision().request, Some(91));
    assert_eq!(rig.decision().request, Some(92));
    rig.reply(Ok(answer()));
    rig.note(|note| matches!(note, Note::Judge(n) if n.judge == "slow" && !n.used));
    rig.stop();
    let rows = rig.rows("judges-");
    let choices: Vec<_> = rows.iter().filter(|r| r["event"] == "choice").collect();
    assert_eq!(choices[0]["reason"], "superseded");
    assert_eq!(choices[1]["reason"], "remote_busy");
    let late = rows.iter().find(|r| r["event"] == "slow").unwrap();
    assert_eq!(late["closed"], "superseded");
}

#[test]
fn failed_slow_judge_leaves_local_probability_and_recovers_for_the_next() {
    let mut rig = Rig::new(ms(1000));
    rig.hear(101, "Lamp, answer this authored question");
    rig.asked.recv_timeout(WAIT).unwrap();
    rig.reply(Err(Failed::Refused(503)));
    let first = rig.decision();
    assert_eq!(first.by, "local");
    rig.hear(102, "Lamp, another authored question");
    rig.asked.recv_timeout(WAIT).unwrap();
    rig.reply(Ok(answer()));
    assert_eq!(rig.decision().by, "local+jev");
    rig.stop();
    let rows = rig.rows("judges-");
    let first_row = rows.iter().find(|r| r["event"] == "choice").unwrap();
    assert_eq!(first_row["slow"]["error"], "refused");
    assert_eq!(
        first_row["local"]["judgement"]["probability"]
            .as_f64()
            .unwrap() as f32,
        first.probability
    );
    assert_eq!(first_row["result"]["guard"], Value::Null);
}

#[test]
fn shutdown_finishes_a_pending_local_choice_without_waiting_for_the_remote() {
    let mut rig = Rig::new(ms(1000));
    rig.hear(111, "okay");
    rig.asked.recv_timeout(WAIT).unwrap();
    assert_eq!(rig.stop().decided, 1);
    assert_eq!(rig.decision().choice, "look_up");
    let rows = rig.rows("judges-");
    let shutdown = rows.iter().find(|r| r["event"] == "shutdown").unwrap();
    assert_eq!(shutdown["slow_unobserved"], 1);
    assert_eq!(shutdown["open_outcome"], 1);
    assert_eq!(
        rows.iter().find(|r| r["event"] == "choice").unwrap()["reason"],
        "shutdown"
    );
    // The deliberately held worker is still released before leaving the test.
    rig.reply(Err(Failed::TimedOut));
}

#[test]
fn a_dead_slow_worker_keeps_local_decisions_and_reports_the_actual_fallback() {
    let mut rig = Rig::new(ms(1000));
    rig.hear(131, "Lamp, authored question");
    rig.asked.recv_timeout(WAIT).unwrap();
    rig.responses.as_ref().unwrap().send(Reply::Exit).unwrap();
    rig.note(|note| matches!(note, Note::SlowGone));
    assert_eq!(rig.decision().by, "local");
    rig.hear(132, "okay");
    let next = rig.decision();
    assert_eq!(next.by, "local");
    assert_eq!(next.choice, "look_up");
    assert_eq!(rig.stop().decided, 2);
    assert!(!rig.taken.iter().any(|note| matches!(note, Note::AskerGone)));
    assert!(Note::SlowGone.to_string().contains("local judge continues"));
    let rows = rig.rows("judges-");
    assert_eq!(
        rows.iter().find(|r| r["event"] == "choice").unwrap()["reason"],
        "worker_gone"
    );
}

#[test]
fn log_write_failure_reports_incomplete_evidence_but_decisions_continue() {
    let mut rig = Rig::new(ms(1000));
    // Make the daily judge file a directory, causing a real append failure.
    let day = fs::read_dir(&rig.log)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("models-")
        });
    // Model evidence is queued before the startup report; obtain its name
    // from that record if present, otherwise use its recorded timestamp date.
    let model = day.expect("model written during cue heartbeat");
    let judge = rig.log.join(
        model
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace("models-", "judges-"),
    );
    fs::create_dir(judge).unwrap();
    rig.hear(121, "Lamp, authored question");
    rig.asked.recv_timeout(WAIT).unwrap();
    rig.reply(Ok(answer()));
    assert!(rig.decision().shadow);
    assert_eq!(rig.stop().decided, 1);
    assert!(
        rig.taken
            .iter()
            .any(|n| matches!(n, Note::JudgesFinished {failed, drained:false, ..} if *failed > 0))
    );
}

#[test]
#[ignore = "host measurement: run alone with --release --ignored --nocapture"]
fn measure_shadow_choices_while_slow_is_held() {
    let mut rig = Rig::new(ms(100));
    rig.hear(1, "authored short scrap");
    rig.asked.recv_timeout(WAIT).unwrap();
    assert_eq!(rig.decision().choice, "look_up");
    for request in 2..=101 {
        rig._voice.alive().unwrap();
        rig._room.alive().unwrap();
        rig.hear(request, "authored short scrap");
        assert_eq!(rig.decision().choice, "look_up");
    }
    // The held service has a five-second test deadline. The recorded reason
    // below verifies it stayed held throughout, rather than silently expiring.
    rig.stop();
    rig.reply(Err(Failed::TimedOut));
    let rows = rig.rows("judges-");
    let choices: Vec<_> = rows.iter().filter(|r| r["event"] == "choice").collect();
    assert_eq!(choices.len(), 101);
    assert_eq!(choices[0]["reason"], "deadline");
    assert!(choices[1..].iter().all(|r| r["reason"] == "remote_busy"));
    let first_us = choices[0]["eligible_to_choice_us"].as_u64().unwrap();
    let mut repeated: Vec<_> = choices[1..]
        .iter()
        .map(|r| r["eligible_to_choice_us"].as_u64().unwrap())
        .collect();
    repeated.sort_unstable();
    println!(
        "SHADOW_TIMING {}",
        serde_json::json!({
            "schema":1,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
            "profile":if cfg!(debug_assertions) {"debug"} else {"release"},
            "decision_budget_ms":100,"samples_repeated":100,"first_us":first_us,
            "median_us":repeated[49],"p95_us":repeated[94],"p99_us":repeated[98],"worst_us":repeated[99],
            "boundary":"eligible utterance to final internal choice immediately before bus publish; includes local answer, bounded log enqueue and callback; excludes gather, bus delivery and acoustics",
            "scenario":"first choice waits to its deadline; following 100 are local-only while the same slow request remains explicitly held",
        })
    );
    assert!(rig.taken.iter().any(|note| matches!(
        note,
        Note::JudgesFinished {
            failed: 0,
            dropped: 0,
            pending: 0,
            drained: true,
            ..
        }
    )));
}
