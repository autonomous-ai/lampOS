//! The Policy holding the lamp's actions on this route (#800, #1712,
//! `LAMP_POLICY_HOLDS_ACTIONS=on`), through the engine against scripted
//! services: first with a gate the test answers, then with the route's own
//! gate (`policy::Desk`) and runner (`worker::tool_runner`), an executive
//! that answers every call and a bus of the test's own, as `serve` builds
//! them.
use super::*;
use crate::second_provider::policy::{Desk, Turn};
use std::io::{BufRead, Write};

/// Each call put to the gate: its request and its name.
type Put = Arc<Mutex<Vec<(u64, String)>>>;

/// A gate that holds the calls named in `held` and runs every other, each
/// after 100 ms on the test's clock (the time a decision takes to come),
/// and tells the test each call put to it.
fn gated(engine: Engine, held: &'static [&'static str]) -> (Engine, Put) {
    let asked: Put = Arc::default();
    let seen = asked.clone();
    let engine = engine.holding_for_the_policy(Arc::new(move |request, name| {
        seen.lock().unwrap().push((request, name.clone()));
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if held.contains(&name.as_str()) {
                Policed::Held
            } else {
                Policed::Run
            }
        })
    }));
    (engine, asked)
}

fn spoken(engine: &mut Engine, request: u64) {
    engine.submit(start(request));
    engine.submit(audio(request, 1));
    engine.submit(end(request));
}

/// #1712 item 6, its first check: the Policy's decision is about a turn
/// by its number, which the coordinator gives the turn's heard words on
/// `voice/heard` from the last `Started` before them
/// (`bus_out::input_started`). So each spoken turn's `Started` comes before
/// its heard words, and no other turn's between them.
#[tokio::test]
async fn each_spoken_turns_start_comes_before_its_heard_words() {
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Turn the light on.".to_owned()),
                Ok("And a bit warmer.".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![text("Done."), Ok(ChatEvent::Done)],
                vec![text("There."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script);
    let mut engine = engine.announcing(false);
    for (request, words) in [(1, "Turn the light on."), (2, "And a bit warmer.")] {
        spoken(&mut engine, request);
        let labels = turn(&mut engine).await;
        assert_eq!(
            labels[..2],
            [format!("started:{request}"), format!("heard:{words}")],
            "{labels:?}"
        );
        assert_eq!(
            labels
                .iter()
                .filter(|label| label.starts_with("started:"))
                .count(),
            1,
            "{labels:?}"
        );
    }
}

/// A turn begun before the last one's words were heard ends that one: its
/// words never come, so none can be told under the newer turn's number.
#[tokio::test(start_paused = true)]
async fn a_turn_begun_before_the_last_ones_words_were_heard_ends_that_one() {
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Turn the light on.".to_owned()),
                Ok("Never mind.".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new([vec![text("Sure."), Ok(ChatEvent::Done)]].into()),
        // The words come a second after the line ends, as `Finalize`'s do.
        finish_after: Some(Duration::from_secs(1)),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine.announcing(false);
    spoken(&mut engine, 1);
    tokio::time::sleep(Duration::from_millis(500)).await;
    spoken(&mut engine, 2);
    let labels = turn(&mut engine).await;
    let heard: Vec<&String> = labels
        .iter()
        .filter(|label| label.starts_with("heard:"))
        .collect();
    assert_eq!(heard, ["heard:Never mind."], "{labels:?}");
    let second = labels
        .iter()
        .position(|label| label == "started:2")
        .unwrap();
    let words = labels
        .iter()
        .position(|label| label == "heard:Never mind.")
        .unwrap();
    assert!(second < words, "{labels:?}");
    assert_eq!(script.asked.lock().unwrap().len(), 1);
}

/// #800 on this route: a call the Policy holds is not run, and the turn
/// ends there in silence, as on the Live route, where none of a held
/// turn's sound is let out. What the model said before the call has gone
/// out already (the trade the first builder took on #1712: no turn's words
/// are kept back for a call that may come). The round's other calls run;
/// the model is not asked again, so it is never told twice.
#[tokio::test(start_paused = true)]
async fn a_call_the_policy_holds_is_not_run_and_the_turn_ends_in_silence() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Yes, give me five minutes.".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![
                    text("Sure. "),
                    Ok(ChatEvent::ToolCalls(vec![
                        call("c1", "set_timer", json!({"minutes": 5})),
                        call("c2", "the_time", json!({})),
                    ])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Five minutes, starting now."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, calls, _) = scripted(script.clone());
    let (mut engine, asked) = gated(engine.announcing(false), &["set_timer"]);
    spoken(&mut engine, 1);
    let labels = turn(&mut engine).await;
    assert_eq!(
        labels,
        [
            "started:1",
            "heard:Yes, give me five minutes.",
            "said:Sure.",
            "audio:5",
            "complete"
        ]
    );
    assert_eq!(
        *asked.lock().unwrap(),
        [(1, "set_timer".to_owned()), (1, "the_time".to_owned())]
    );
    assert_eq!(
        *calls.lock().unwrap(),
        [("the_time".to_owned(), json!({}), true)],
        "the held call is not run; the round's other one is"
    );
    assert_eq!(script.asked.lock().unwrap().len(), 1, "never asked again");
}

/// A call the Policy lets run is run after its wait, and the turn goes on
/// as without the switch.
#[tokio::test(start_paused = true)]
async fn a_call_the_policy_lets_run_is_run_and_the_turn_goes_on() {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Lamp, turn the light on.".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        "light_on",
                        json!({}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("It's on."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, calls, _) = scripted(script.clone());
    let (mut engine, asked) = gated(engine.announcing(false), &["set_timer"]);
    spoken(&mut engine, 1);
    let labels = turn(&mut engine).await;
    assert_eq!(
        labels[labels.len() - 4..],
        ["said:It's on.", "audio:8", "generated", "complete"]
    );
    assert_eq!(*asked.lock().unwrap(), [(1, "light_on".to_owned())]);
    assert_eq!(
        *calls.lock().unwrap(),
        [("light_on".to_owned(), json!({}), true)]
    );
}

/// A turn the lamp wrote itself is nobody's to judge: its calls are never
/// put to the Policy, as on the Live route.
#[tokio::test(start_paused = true)]
async fn the_lamps_own_turn_is_never_put_to_the_policy() {
    let script = Arc::new(Script {
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![call(
                        "c1",
                        "light_on",
                        json!({}),
                    )])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Done."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, calls, _) = scripted(script);
    let (mut engine, asked) = gated(engine.announcing(false), &["light_on"]);
    engine.submit(written(1, "Show the light."));
    turn(&mut engine).await;
    assert!(asked.lock().unwrap().is_empty());
    assert_eq!(
        *calls.lock().unwrap(),
        [("light_on".to_owned(), json!({}), false)]
    );
}

/// An executive that answers every call done, telling the test each line
/// it was sent, and the Policy's place on a bus of the test's own, in one
/// short folder.
struct Bench {
    dir: std::path::PathBuf,
    decisions: lamp_bus::Publisher,
    asked: std::sync::mpsc::Receiver<Value>,
}

impl Bench {
    fn new(tag: &str) -> Self {
        // Short: a socket's whole path must fit in 103 bytes.
        let dir = std::path::PathBuf::from(format!("/tmp/lsq-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let listener =
            std::os::unix::net::UnixListener::bind(dir.join(crate::tools::CALL_SOCKET)).unwrap();
        let (told, asked) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut line = String::new();
                if std::io::BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .is_err()
                {
                    continue;
                }
                if told
                    .send(serde_json::from_str::<Value>(&line).unwrap())
                    .is_err()
                {
                    return;
                }
                let _ = stream.write_all(b"{\"ok\":true,\"result\":{}}\n");
            }
        });
        let decisions = lamp_bus::Bus::new(&dir)
            .unwrap()
            .publisher_declared(
                lamp_msgs::Decision::TOPIC,
                &lamp_msgs::Decision::declaration(),
                "policy",
            )
            .unwrap();
        Self {
            dir,
            decisions,
            asked,
        }
    }

    /// The Policy's decisions as `serve` reads them: through the rule's one
    /// reader (`Heeding::reading_the_policy`), which a line going on in a
    /// busy room reads too.
    fn decided(&self) -> crate::second_provider::policy::Decided {
        let heeding = Arc::new(Mutex::new(crate::second_provider::worker::Heeding::new(
            false, true, None, None,
        )));
        heeding
            .lock()
            .unwrap()
            .reading_the_policy(crate::policy_holds::Verdicts::on(&self.dir).unwrap());
        Arc::new(move |request| heeding.lock().ok()?.policy_verdict(request))
    }

    fn settings(&self) -> crate::tools::Settings {
        crate::tools::Settings {
            exec: Some(self.dir.clone()),
            schedule_tools: true,
            ..Default::default()
        }
    }

    /// The engine as `serve` builds it with the switch: the route's own
    /// runner, which leaves its battery line to the gate, and the gate.
    fn engine(&self, script: Arc<Script>) -> Engine {
        let desk = Desk::new(
            self.decided(),
            self.settings(),
            false,
            Arc::new(|_| Turn::default()),
            crate::second_provider::worker::answered_here(None),
        );
        Engine::with_clock(
            script,
            Arc::new(|| "SYSTEM".to_owned().into()),
            crate::second_provider::worker::tool_runner(self.settings(), None, None, true),
            Arc::new(Instant::now),
        )
        .announcing(false)
        .holding_for_the_policy(Arc::new(desk).gate())
    }

    fn decide(&mut self, request: u64, choice: &str) {
        let decision = json!({"about": "attention", "choice": choice, "probability": 0.235,
                              "answers": {}, "by": "typesafe/jev-1.13-20260917",
                              "took_ms": 540, "shadow": false, "request": request});
        self.decisions.publish_json(0, &decision).unwrap();
    }

    /// The names of the calls the executive was sent.
    fn ran(&self) -> Vec<String> {
        self.asked
            .try_iter()
            .map(|line| line["name"].as_str().unwrap_or_default().to_owned())
            .collect()
    }
}

impl Drop for Bench {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Turns whose call is `light_on`, each with whether it runs: one that
/// runs is said after it ("It's on."); a held one asks the chat once.
fn light_turns(lines: &[(&str, bool)]) -> Arc<Script> {
    let mut chats = VecDeque::new();
    for (_, runs) in lines {
        chats.push_back(vec![
            Ok(ChatEvent::ToolCalls(vec![call(
                "c1",
                "light_on",
                json!({}),
            )])),
            Ok(ChatEvent::Done),
        ]);
        if *runs {
            chats.push_back(vec![text("It's on."), Ok(ChatEvent::Done)]);
        }
    }
    Arc::new(Script {
        heard: Mutex::new(
            lines
                .iter()
                .map(|(line, _)| Ok((*line).to_owned()))
                .collect(),
        ),
        chats: Mutex::new(chats),
        ..Script::default()
    })
}

/// Every output of a turn through its last, as [`turn`] gives them, with
/// as long as `within` for each.
async fn turn_within(engine: &mut Engine, within: Duration) -> Vec<String> {
    let mut labels = Vec::new();
    loop {
        let event = tokio::time::timeout(within, engine.next())
            .await
            .expect("the turn ends");
        let last = matches!(
            event,
            ProviderOutput::TurnComplete { .. } | ProviderOutput::TurnFailed { .. }
        );
        labels.push(format!("{event:?}").chars().take(40).collect::<String>());
        if last {
            return labels;
        }
    }
}

/// #1712 item 6, as `serve` builds the route with the switch: in a turn
/// the Policy judged `stay_quiet` ("Yes, a bit brighter.", said to someone
/// else) the light is never asked of the executive, and the turn says
/// nothing; in one it took up, it is, and the lamp says so.
#[tokio::test]
async fn a_held_light_never_reaches_the_executive_and_a_taken_one_does() {
    let mut bench = Bench::new("exec");
    let mut engine = bench.engine(light_turns(&[
        ("Yes, a bit brighter.", false),
        ("Lamp, the light.", true),
    ]));
    bench.decide(1, "stay_quiet");
    bench.decide(2, "answer");
    spoken(&mut engine, 1);
    let held = turn(&mut engine).await;
    assert_eq!(
        held,
        ["started:1", "heard:Yes, a bit brighter.", "complete"]
    );
    assert!(bench.ran().is_empty(), "nothing reached the executive");
    spoken(&mut engine, 2);
    let taken = turn(&mut engine).await;
    assert_eq!(
        taken[taken.len() - 4..],
        ["said:It's on.", "audio:8", "generated", "complete"]
    );
    assert_eq!(bench.ran(), ["light_on"]);
}

/// The peer read's P2 on #1984: a round's held calls wait for the Policy
/// together, one `policy_holds::WAIT` for the round, as the Live route parks
/// them at once. "Light on, louder, and a timer." with the Policy down: all
/// three run once the one wait has passed, not one wait after another
/// (6.04 s before).
#[tokio::test]
async fn a_rounds_held_calls_wait_for_the_policy_together() {
    let bench = Bench::new("round");
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Light on, louder, and a timer.".to_owned())].into()),
        chats: Mutex::new(
            [
                vec![
                    Ok(ChatEvent::ToolCalls(vec![
                        call("c1", "light_on", json!({})),
                        call("c2", "volume_up", json!({})),
                        call("c3", "set_timer", json!({"minutes": 5})),
                    ])),
                    Ok(ChatEvent::Done),
                ],
                vec![text("Done."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let mut engine = bench.engine(script);
    let began = Instant::now();
    spoken(&mut engine, 1);
    let labels = turn_within(&mut engine, crate::policy_holds::WAIT * 2).await;
    let took = began.elapsed();
    assert!(
        labels
            .last()
            .is_some_and(|last| last.starts_with("TurnComplete")),
        "{labels:?}"
    );
    let mut ran = bench.ran();
    ran.sort();
    assert_eq!(ran, ["light_on", "set_timer", "volume_up"]);
    assert!(
        took >= crate::policy_holds::WAIT
            && took
                < crate::policy_holds::WAIT
                    + 2 * crate::second_provider::policy::LOOK_EVERY
                    + Duration::from_millis(300),
        "one wait for the round, not three: {took:?}"
    );
}

/// The peer read's P3 on #1984, after #1925: `reports_stop` beside a held
/// `light_on` in one round still runs. The Policy never holds it
/// (`Route::ReportsStop`), so a person stopping the reports is heard
/// whatever the Policy thinks of the turn, and the light isn't run.
#[tokio::test]
async fn a_reports_stop_beside_a_held_light_still_runs() {
    let mut bench = Bench::new("stop");
    let settings = crate::tools::Settings {
        reports_stop: true,
        ..bench.settings()
    };
    let desk = Desk::new(
        bench.decided(),
        settings,
        false,
        Arc::new(|_| Turn::default()),
        crate::second_provider::worker::answered_here(None),
    );
    let ran: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = ran.clone();
    let tools: ToolRunner = Arc::new(move |_, _, name, _| {
        seen.lock().unwrap().push(name);
        Box::pin(async { json!({"ok": true}) })
    });
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("Stop the reports, and the light.".to_owned())].into()),
        chats: Mutex::new(
            [vec![
                Ok(ChatEvent::ToolCalls(vec![
                    call("c1", "light_on", json!({})),
                    call("c2", crate::reports_stop::NAME, json!({})),
                ])),
                Ok(ChatEvent::Done),
            ]]
            .into(),
        ),
        ..Script::default()
    });
    let mut engine = Engine::with_clock(
        script,
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .holding_for_the_policy(Arc::new(desk).gate());
    bench.decide(1, "stay_quiet");
    spoken(&mut engine, 1);
    let labels = turn(&mut engine).await;
    assert_eq!(
        labels.last().map(String::as_str),
        Some("complete"),
        "{labels:?}"
    );
    assert_eq!(*ran.lock().unwrap(), [crate::reports_stop::NAME]);
}

/// The p-th share (0 to 1) of `waits`, sorted.
fn share(waits: &mut [Duration], p: f64) -> Duration {
    waits.sort();
    let at = ((waits.len() - 1) as f64 * p).round() as usize;
    waits[at]
}

/// How long the lamp's action waits from the model's call to its run, on
/// this route as `serve` builds it, with the turn's decision on the bus
/// before the call and with none (the wait's 2 s cap): `n` spoken turns,
/// each asking for the light, timed from the gate's call to the runner's.
async fn waits(decided: bool, n: u64) -> Vec<Duration> {
    let mut bench = Bench::new(if decided { "wd" } else { "wn" });
    let lines: Vec<String> = (0..n).map(|turn| format!("The light, {turn}.")).collect();
    let lines: Vec<(&str, bool)> = lines.iter().map(|line| (line.as_str(), true)).collect();
    let script = light_turns(&lines);
    let desk = Arc::new(Desk::new(
        bench.decided(),
        bench.settings(),
        false,
        Arc::new(|_| Turn::default()),
        crate::second_provider::worker::answered_here(None),
    ));
    let called: Arc<Mutex<Vec<Instant>>> = Arc::default();
    let ran: Arc<Mutex<Vec<Instant>>> = Arc::default();
    let gate = desk.gate();
    let at_call = called.clone();
    let timed: PolicyGate = Arc::new(move |request, name| {
        at_call.lock().unwrap().push(Instant::now());
        gate(request, name)
    });
    let runner = crate::second_provider::worker::tool_runner(bench.settings(), None, None, true);
    let at_run = ran.clone();
    let tools: ToolRunner = Arc::new(move |request, heard, name, args| {
        at_run.lock().unwrap().push(Instant::now());
        runner(request, heard, name, args)
    });
    let mut engine = Engine::with_clock(
        script,
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .holding_for_the_policy(timed);
    for request in 1..=n {
        if decided {
            bench.decide(request, "answer");
        }
        spoken(&mut engine, request);
        let labels = turn_within(&mut engine, crate::policy_holds::WAIT * 2).await;
        assert!(
            labels
                .last()
                .is_some_and(|last| last.starts_with("TurnComplete")),
            "{labels:?}"
        );
    }
    assert_eq!(bench.ran().len() as u64, n);
    let called = called.lock().unwrap();
    let ran = ran.lock().unwrap();
    called
        .iter()
        .zip(ran.iter())
        .map(|(call, run)| run.duration_since(*call))
        .collect()
}

/// The wait with the decision there: well under one look of the gate's
/// (20 ms) past the call, on this machine; with none, the 2 s cap and at
/// most one look more.
#[tokio::test]
async fn an_action_waits_no_longer_than_its_decision_takes_or_the_cap() {
    let mut decided = waits(true, 8).await;
    assert!(
        share(&mut decided, 1.0) < Duration::from_millis(100),
        "{decided:?}"
    );
    let mut none = waits(false, 2).await;
    assert!(
        share(&mut none, 0.0) >= crate::policy_holds::WAIT
            && share(&mut none, 1.0) < crate::policy_holds::WAIT + Duration::from_millis(100),
        "{none:?}"
    );
}

/// The figures for #1712 item 6's step 6: run with `--ignored
/// --nocapture`. Prints the median and the 90th percentile of the wait from
/// the call to its run, with the decision there and with none.
#[tokio::test]
#[ignore = "a measurement: 20 s of waits"]
async fn measure_the_wait_from_the_call_to_its_run() {
    for (decided, n) in [(true, 40), (false, 10)] {
        let mut waits = waits(decided, n).await;
        let ms = |wait: Duration| wait.as_secs_f64() * 1_000.0;
        let median = ms(share(&mut waits, 0.5));
        let p90 = ms(share(&mut waits, 0.9));
        let most = ms(share(&mut waits, 1.0));
        println!(
            "the decision {}: {n} calls, median {median:.1} ms, p90 {p90:.1} ms, most {most:.1} ms",
            if decided { "there" } else { "absent" }
        );
    }
}
