//! #8546: parent settlement and model history must cross a real process boundary.
//! Speaker receipts and service streams are synthetic; no device audio is claimed.
use super::*;
use crate::provider_flow::OutputReceiver;
use crate::second_provider::{Engine, Listener, Services, net};
use futures_util::{future::BoxFuture, stream, stream::BoxStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};

const TEST: &str = "coordinator::tests::say::fixed_line_process::an_exact_skill_line_said_is_in_the_next_chat_requests_history";
const VI_TEST: &str = "coordinator::tests::say::fixed_line_process::played_vietnamese_fixed_line_reaches_real_worker_chat";
const SETTLEMENT_TEST: &str = "coordinator::tests::say::fixed_line_process::fixed_line_receipt_requires_complete_bound_playback";
const TIMING_TEST: &str =
    "coordinator::tests::say::fixed_line_process::played_fixed_line_real_worker_history_timing";
const TIMING_REPEATS: usize = 200;
const MODEL_ANSWER: &str = "I heard your next request.";
const MODE: &str = "SECOND_FIXED_LINE_TEST_MODE";
const ROOT: &str = "SECOND_FIXED_LINE_TEST_ROOT";
const FIXED: &str = "Okay, I'll stay.";
const CONTROL: &str = "The violet notebook is on the shelf.";
const NEXT: &str = "Please prepare to unplug again.";

/// Kills and reaps on assertion failure as well as on ordinary completion.
struct Reap(Option<Child>);
impl Drop for Reap {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn spawn(mode: &str, root: &Path, test: &str) -> Reap {
    let mut command = Command::new(std::env::current_exe().unwrap());
    // The fixture chooses its services and configuration, independent of a
    // builder's optional production switches. No environment is mutated in
    // the multithreaded parent test process.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("LAMP_") {
            command.env_remove(key);
        }
    }
    // An ignored exact-name child otherwise exits successfully after zero tests.
    // Ordinary functional fixtures must not inherit this timing-only selector.
    if test == TIMING_TEST {
        command.arg("--ignored");
    }
    Reap(Some(
        command
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(MODE, mode)
            .env(ROOT, root)
            .spawn()
            .unwrap(),
    ))
}

fn wait(ready: impl FnMut() -> bool, what: &str) {
    wait_within(ready, what, Duration::from_secs(10));
}

fn wait_within(mut ready: impl FnMut() -> bool, what: &str, within: Duration) {
    let deadline = Instant::now() + within;
    while !ready() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn reaped(child: &mut Reap, what: &str, within: Duration) {
    let mut status = None;
    wait_within(
        || {
            status = child.0.as_mut().unwrap().try_wait().unwrap();
            status.is_some()
        },
        what,
        within,
    );
    assert!(status.unwrap().success(), "{what}: {status:?}");
}

#[test]
fn an_exact_skill_line_said_is_in_the_next_chat_requests_history() {
    isolated("driver", TEST);
}

#[test]
fn played_vietnamese_fixed_line_reaches_real_worker_chat() {
    isolated("driver-vi", VI_TEST);
}

#[test]
fn fixed_line_receipt_requires_complete_bound_playback() {
    isolated("settlement", SETTLEMENT_TEST);
}

/// Host-only completion-to-next-chat measurement. No device audio or network.
#[test]
#[ignore = "#8546: isolated real-worker host timing; run after other build/test work"]
fn played_fixed_line_real_worker_history_timing() {
    isolated("timing", TIMING_TEST);
}

fn isolated(default_mode: &str, test: &str) {
    match std::env::var(MODE).ok().as_deref() {
        Some("worker") => worker(&PathBuf::from(std::env::var_os(ROOT).unwrap())),
        Some("driver") => driver(&PathBuf::from(std::env::var_os(ROOT).unwrap()), "en", test),
        Some("driver-vi") => driver(&PathBuf::from(std::env::var_os(ROOT).unwrap()), "vi", test),
        Some("settlement") => settlement_refusals(),
        Some("timing") => timing_driver(&PathBuf::from(std::env::var_os(ROOT).unwrap()), test),
        None => {
            let directory = SessionDirectory::create().unwrap();
            let mut driver = spawn(default_mode, &directory.path, test);
            reaped(
                &mut driver,
                "isolated parent playback driver",
                Duration::from_secs(if test == TIMING_TEST { 120 } else { 10 }),
            );
        }
        Some(other) => panic!("unknown fixture mode: {other}"),
    }
}

fn rendered_fixed(language: &str) -> String {
    crate::language::Language::parse(language)
        .unwrap()
        .and_then(|language| language.fixed(FIXED))
        .unwrap_or(FIXED)
        .to_owned()
}

struct CapturingServices {
    root: PathBuf,
    calls: Mutex<usize>,
}
struct SpokenStay;
impl Listener for SpokenStay {
    fn send(&self, samples: &[i16]) -> net::Result<()> {
        assert!(!samples.is_empty());
        Ok(())
    }
    fn finish(self: Box<Self>, _request: u64) -> BoxFuture<'static, net::Result<String>> {
        Box::pin(async { Ok("Never mind, please stay plugged in.".into()) })
    }
}

impl Services for CapturingServices {
    fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
        Box::pin(async { Ok(Box::new(SpokenStay) as Box<dyn Listener>) })
    }

    fn chat(&self, messages: Vec<net::Message>) -> BoxStream<'static, net::Result<net::ChatEvent>> {
        let captured_at_us = monotonic_us();
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        let index = *calls;
        // Capture the real Engine's service boundary, never a manually
        // assembled expected history or a shared parent THREAD.
        let capture = serde_json::json!({
            "pid": std::process::id(),
            "captured_at_us": captured_at_us,
            "messages": messages,
        });
        let temporary = self.root.join(format!("chat-{index}.tmp"));
        std::fs::write(&temporary, serde_json::to_vec(&capture).unwrap()).unwrap();
        std::fs::rename(temporary, self.root.join(format!("chat-{index}.json"))).unwrap();
        let latest = self.root.join("latest.tmp");
        std::fs::write(&latest, serde_json::to_vec(&capture).unwrap()).unwrap();
        std::fs::rename(latest, self.root.join("latest.json")).unwrap();
        let events = match index {
            1 => vec![
                Ok(net::ChatEvent::Text(CONTROL.into())),
                Ok(net::ChatEvent::ToolCalls(vec![net::ToolCall {
                    id: "stay-origin".into(),
                    name: "stay_plugged_in".into(),
                    args: serde_json::json!({}),
                    extra: None,
                }])),
                Ok(net::ChatEvent::Done),
            ],
            // The tool speaks its exact line separately. This round adds no
            // model words to the worker-generated positive control above.
            2 => vec![Ok(net::ChatEvent::Done)],
            _ => vec![
                Ok(net::ChatEvent::Text(MODEL_ANSWER.into())),
                Ok(net::ChatEvent::Done),
            ],
        };
        Box::pin(stream::iter(events))
    }

    fn speak(&self, _: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
        Box::pin(stream::iter([Ok(vec![400; 480])]))
    }
}

fn boots() -> (BootId, BootId) {
    (
        BootId::new([61; 16]).unwrap(),
        BootId::new([62; 16]).unwrap(),
    )
}

fn worker(root: &Path) {
    let (boot, worker_boot) = boots();
    let mut channels = WorkerChannels::bind(root, "second", boot, worker_boot, true).unwrap();
    channels.connect(root, "second", true).unwrap();
    std::fs::write(root.join("bound"), std::process::id().to_string()).unwrap();
    // A fresh exec owns a fresh THREAD. Never insert FIXED or CONTROL into it.
    let origin_path = root.join("origin.json");
    let engine = Engine::new(
        Arc::new(CapturingServices {
            root: root.to_owned(),
            calls: Mutex::new(0),
        }),
        || crate::second_provider::worker::system_message("Test lamp.", None, "", None, None),
        Arc::new(|_, _, _, _| panic!("this fixture does not ask tools")),
    )
    .with_memory_scope(
        worker_boot,
        Arc::default(),
        Arc::new(|_, _, _, _, _| panic!("the fixed-line runner owns this spoken call")),
        None,
    )
    .keeping_fixed_lines(
        worker_boot,
        Arc::new(move |origin, context, call, lease| {
            let origin = origin.expect("the real admitted spoken call must mint its origin");
            assert_eq!(call.name, "stay_plugged_in");
            assert_eq!(origin.call_id, call.id);
            assert_eq!(origin.request, context.request);
            assert!(lease.is_current());
            assert_eq!(lease.scope().unwrap().person(), None, "room owner fixture");
            std::fs::write(&origin_path, serde_json::to_vec(&origin).unwrap()).unwrap();
            Box::pin(async {
                crate::tools::ToolAnswer::plain(serde_json::json!({
                    "next":"Say nothing. The lamp says it will stay, then gets back up by itself."
                }))
            })
        }),
    )
    .announcing(false);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime
        .block_on(crate::second_provider::worker::run(
            channels, boot, engine, None, None,
        ))
        .unwrap();
    runtime.shutdown_timeout(Duration::from_secs(1));
}

fn renew(owner: &mut Controller, peer: &mut WorkerChannels) {
    let at = now();
    let until = MonoTime::from_micros(at.as_micros() + 100_000);
    owner
        .set_capture(at, CaptureState::RetainingUntil(until))
        .unwrap();
    owner
        .set_admission(at, AdmissionState::OpenUntil(until))
        .unwrap();
    peer.control
        .send(Control::Authority {
            snapshot: owner.snapshot(at).unwrap(),
        })
        .unwrap();
}

fn spoken(owner: &mut Controller, peer: &mut WorkerChannels) -> TurnOwner {
    renew(owner, peer);
    let turn = owner.admit(now(), AdmittedInput::NewTurn).unwrap();
    renew(owner, peer);
    let privacy_generation = owner.snapshot(now()).unwrap().microphone_generation();
    let request = turn.turn();
    for input in [
        ProviderInput::Start {
            request,
            session: 1,
            privacy_generation,
            continues: false,
            continues_cut_off: false,
            over_the_lamp: false,
            after_stop: false,
            follows_on: false,
            over_the_lamps_voice: false,
            heard_of_cut: None,
            soft_lead_us: 0,
            language: None,
        },
        ProviderInput::Audio {
            request,
            privacy_generation,
            sequence: 1,
            read_completed_at_us: monotonic_us(),
            samples: vec![1; 160],
        },
        ProviderInput::End {
            request,
            privacy_generation,
            sound: false,
            cut_in: false,
        },
    ] {
        peer.data.send(input).unwrap();
    }
    turn
}

fn written(owner: &mut Controller, peer: &mut WorkerChannels, text: &str) -> TurnOwner {
    renew(owner, peer);
    let turn = owner.admit(now(), AdmittedInput::NewTurn).unwrap();
    renew(owner, peer);
    let snapshot = owner.snapshot(now()).unwrap();
    peer.data
        .send(ProviderInput::Written {
            request: turn.turn(),
            session: 1,
            privacy_generation: snapshot.microphone_generation(),
            text: text.into(),
            look: false,
            search: false,
            quoted: false,
        })
        .unwrap();
    turn
}

fn completed(
    owner: &mut Controller,
    peer: &mut WorkerChannels,
    flow: &mut OutputReceiver,
    cloud: &mut CloudAvailability,
    turn: TurnOwner,
) {
    wait(
        || {
            renew(owner, peer);
            while let Some(event) = peer.control.receive::<WorkerEvent>().unwrap() {
                match event {
                    WorkerEvent::Ready => {}
                    WorkerEvent::ProviderLink { link } => {
                        cloud.observe(link, monotonic_us()).unwrap()
                    }
                    other => panic!("unexpected worker control: {other:?}"),
                }
            }
            let mut complete = false;
            while let Some(event) = peer.data.receive::<ProviderOutput>().unwrap() {
                flow.received().unwrap();
                assert!(
                    !matches!(event, ProviderOutput::TurnFailed { .. }),
                    "{event:?}"
                );
                complete |= matches!(event, ProviderOutput::TurnComplete { request, .. } if request == turn.turn());
            }
            flow.replenish(0, &mut peer.control).unwrap();
            complete
        },
        "real worker turn completion",
    );
    owner.complete_turn(now(), turn).unwrap();
    renew(owner, peer);
}

fn settle_exact(
    owner: &mut Controller,
    origin: lamp_msgs::FixedLineOrigin,
    language: &str,
) -> Reply {
    let (mut says, answered) = crate::say::Says::words_for_test(
        "prepare-unplug",
        crate::say::Words::ExactSound {
            samples: vec![400; 480],
            text: Some(rendered_fixed(language)),
            pieces: None,
        },
        crate::say::NORMAL_PRIORITY,
    );
    says.set_fixed_origin_for_test(origin, FIXED, language);
    let (mut reply, mut pending, mut trace) = (None, None, SessionState::default());
    let admission = InputAdmission::default();
    let now_is = crate::say::SoundNow {
        conversing: false,
        ..sound_now(owner, &reply, &admission, &says)
    };
    assert!(take_exact(
        &mut says, &now_is, owner, &mut reply, &mut trace
    ));
    assert!(matches!(answered.try_recv(), Ok(Answer::Begun(_))));
    let mut played = play_sound(owner, &mut reply, &mut pending, &mut trace, 2);
    played
        .peer
        .control
        .send(WorkerEvent::SpeechRetired {
            owner: played.last.permit.owner(),
            sequence: played.last.chunk_sequence,
            final_sample: PlaybackCursor {
                epoch: 1,
                frame: played.cursor,
            },
            observed_at_us: monotonic_us(),
        })
        .unwrap();
    service_speaker_control(
        &mut played.speaker,
        owner,
        &mut reply,
        &mut pending,
        &mut played.mask,
        &mut played.snapshot,
        &mut 0,
        &mut 16,
        &mut trace,
        &mut None,
    )
    .unwrap();
    let current = reply.as_mut().unwrap();
    assert!(current.settled(&pending));
    owner.complete_turn(now(), current.owner).unwrap();
    current.answer_say(&pending, owner.snapshot(now()).unwrap());
    assert_eq!(answered.try_recv(), Ok(Answer::Done(Outcome::Spoken)));
    reply.unwrap()
}

fn driver(root: &Path, language: &str, test: &str) {
    let (boot, worker_boot) = boots();
    let peer = WorkerChannels::bind(root, "second", boot, worker_boot, false).unwrap();
    let mut child = spawn("worker", root, test);
    let mut worker =
        Worker::fixture_with_child(peer, child.0.take().unwrap(), worker_boot, "second");
    wait(|| root.join("bound").exists(), "real worker socket bind");
    worker.channels.connect(root, "second", false).unwrap();
    let child_pid: u32 = std::fs::read_to_string(root.join("bound"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(child_pid, worker.process_id());
    assert_ne!(child_pid, std::process::id());
    let mut owner = Controller::new(boot, now());
    owner
        .set_microphone_permission(now(), Permission::Allowed)
        .unwrap();
    let mut flow = OutputReceiver::default();
    let mut cloud = CloudAvailability::default();
    worker.set_history_active(true).unwrap();
    let first = spoken(&mut owner, &mut worker.channels);
    completed(
        &mut owner,
        &mut worker.channels,
        &mut flow,
        &mut cloud,
        first,
    );
    let origin: lamp_msgs::FixedLineOrigin =
        serde_json::from_slice(&std::fs::read(root.join("origin.json")).unwrap()).unwrap();
    assert_eq!(origin.request, first.turn());
    assert_eq!(origin.worker_boot, crate::process::boot_text(worker_boot));
    assert_eq!(cloud.ready_session(), Some(origin.provider_session));
    let mut settled = settle_exact(&mut owner, origin, language);
    let receipt = settled
        .fixed_line_receipt
        .take()
        .expect("Spoken has a bound receipt");
    let mut pending = crate::coordinator::fixed_lines::Pending::default();
    pending.push(receipt);
    renew(&mut owner, &mut worker.channels);
    assert!(
        pending
            .flush(&mut worker, &cloud, owner.snapshot(now()).unwrap())
            .unwrap()
    );
    let next = written(&mut owner, &mut worker.channels, NEXT);
    completed(
        &mut owner,
        &mut worker.channels,
        &mut flow,
        &mut cloud,
        next,
    );
    worker.channels.control.send(Control::Stop).unwrap();
    let mut status = None;
    wait(
        || {
            status = worker.try_exit().unwrap();
            status.is_some()
        },
        "real worker stopped",
    );
    assert!(status.unwrap().success());
    let capture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("latest.json")).unwrap()).unwrap();
    assert_eq!(capture["pid"].as_u64(), Some(u64::from(child_pid)));
    let messages = capture["messages"].as_array().unwrap();
    assert!(
        capture["messages"].to_string().contains(NEXT),
        "captured the next Written request, not the tool's follow-up round"
    );
    let assistant_has = |text: &str| {
        messages.iter().any(|message| {
            message["role"] == "assistant"
                && message["content"]
                    .as_str()
                    .is_some_and(|said| said.trim() == text)
        })
    };
    assert!(
        assistant_has(CONTROL),
        "positive control: worker-generated history missing: {capture}"
    );
    assert!(
        assistant_has(&rendered_fixed(language)),
        "played parent fixed line missing from real worker Chat: {capture}"
    );
}

fn timing_driver(root: &Path, test: &str) {
    let (boot, worker_boot) = boots();
    let peer = WorkerChannels::bind(root, "second", boot, worker_boot, false).unwrap();
    let mut child = spawn("worker", root, test);
    let mut worker =
        Worker::fixture_with_child(peer, child.0.take().unwrap(), worker_boot, "second");
    wait(|| root.join("bound").exists(), "timing worker socket bind");
    worker.channels.connect(root, "second", false).unwrap();
    let child_pid: u32 = std::fs::read_to_string(root.join("bound"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(child_pid, worker.process_id());
    assert_ne!(child_pid, std::process::id());
    let mut owner = Controller::new(boot, now());
    owner
        .set_microphone_permission(now(), Permission::Allowed)
        .unwrap();
    let mut flow = OutputReceiver::default();
    let mut cloud = CloudAvailability::default();
    worker.set_history_active(true).unwrap();
    let first = spoken(&mut owner, &mut worker.channels);
    completed(
        &mut owner,
        &mut worker.channels,
        &mut flow,
        &mut cloud,
        first,
    );
    let origin: lamp_msgs::FixedLineOrigin =
        serde_json::from_slice(&std::fs::read(root.join("origin.json")).unwrap()).unwrap();
    assert_eq!(origin.request, first.turn());
    assert_eq!(origin.worker_boot, crate::process::boot_text(worker_boot));
    assert_eq!(cloud.ready_session(), Some(origin.provider_session));

    let mut elapsed_us = Vec::with_capacity(TIMING_REPEATS + 1);
    for sample in 0..=TIMING_REPEATS {
        // The original spoken call may produce several distinct exact lines.
        // Each iteration must actually settle a new playback owner and use the
        // production history queue, with no deliberate post-retirement delay.
        let mut settled = settle_exact(&mut owner, origin.clone(), "en");
        let receipt = settled
            .fixed_line_receipt
            .take()
            .expect("timed Spoken receipt");
        let played_at_us = receipt.played_at_us;
        let mut pending = crate::coordinator::fixed_lines::Pending::default();
        pending.push(receipt);
        renew(&mut owner, &mut worker.channels);
        assert!(
            pending
                .flush(&mut worker, &cloud, owner.snapshot(now()).unwrap())
                .unwrap()
        );
        let next_text = format!("Timing follow-up {sample}: please prepare to unplug again.");
        let next = written(&mut owner, &mut worker.channels, &next_text);
        completed(
            &mut owner,
            &mut worker.channels,
            &mut flow,
            &mut cloud,
            next,
        );
        // Completion follows the atomic capture write. Read the capture only
        // afterward, so filesystem polling is outside the measured interval.
        let capture: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("latest.json")).unwrap()).unwrap();
        assert_eq!(capture["pid"].as_u64(), Some(u64::from(child_pid)));
        let messages = capture["messages"].as_array().unwrap();
        let ask = messages
            .iter()
            .rposition(|message| {
                message["role"] == "user" && message["content"].as_str() == Some(next_text.as_str())
            })
            .expect("capture must be this unique follow-up's actual chat call");
        let last_assistant = messages[..ask]
            .iter()
            .rposition(|message| message["role"] == "assistant")
            .expect("a completed fixed line precedes the current user ask");
        assert_eq!(
            messages[last_assistant]["content"].as_str(),
            Some(FIXED),
            "sample {sample}: a previous fixed line cannot substitute for this receipt: {capture}"
        );
        let previous_assistant = messages[..last_assistant]
            .iter()
            .rev()
            .find(|message| message["role"] == "assistant")
            .expect("worker-generated ordinary answer separates repeated fixed lines");
        assert_eq!(
            previous_assistant["content"].as_str(),
            Some(if sample == 0 { CONTROL } else { MODEL_ANSWER }),
            "sample {sample}: the positive control or prior ordinary answer must precede the new fixed line"
        );
        let captured_at_us = capture["captured_at_us"].as_u64().unwrap();
        elapsed_us.push(
            captured_at_us
                .checked_sub(played_at_us)
                .expect("chat capture must follow final speaker retirement"),
        );
    }
    worker.channels.control.send(Control::Stop).unwrap();
    let mut status = None;
    wait(
        || {
            status = worker.try_exit().unwrap();
            status.is_some()
        },
        "timing worker stopped",
    );
    assert!(status.unwrap().success());

    let first_us = elapsed_us[0];
    let mut repeated = elapsed_us[1..].to_vec();
    repeated.sort_unstable();
    // Nearest-rank percentiles across exactly 200 repeated propagations.
    let p50_us = repeated[(TIMING_REPEATS * 50).div_ceil(100) - 1];
    let p95_us = repeated[(TIMING_REPEATS * 95).div_ceil(100) - 1];
    let max_us = *elapsed_us.iter().max().unwrap();
    eprintln!(
        "#8546 host history timing: first={:.3}ms repeated_n={} p50={:.3}ms p95={:.3}ms max_all={:.3}ms; boundary=final speaker retirement to actual worker Chat call with newest fixed line before unique user ask; includes immediate follow-up admission/setup; synthetic PCM/services, no network/acoustics or deliberate between-turn wait",
        first_us as f64 / 1_000.0,
        TIMING_REPEATS,
        p50_us as f64 / 1_000.0,
        p95_us as f64 / 1_000.0,
        max_us as f64 / 1_000.0,
    );
    assert!(
        p95_us <= 50_000,
        "repeated propagation p95 exceeds 50 ms: {p95_us} us"
    );
    assert!(
        max_us <= 100_000,
        "propagation maximum exceeds 100 ms: {max_us} us"
    );
}

/// Separate process to isolate the parent THREAD and playback test helpers.
/// The origin here is a trusted fixture; origin minting is exercised above.
fn settlement_refusals() {
    for case in ["partial", "unrendered", "cancelled", "unbound"] {
        let mut owner = resting_owner();
        let source = owner.admit(now(), AdmittedInput::NewTurn).unwrap();
        owner.complete_turn(now(), source).unwrap();
        let snapshot = owner.snapshot(now()).unwrap();
        let origin = lamp_msgs::FixedLineOrigin {
            worker_boot: crate::process::boot_text(boots().1),
            provider_session: 1,
            request: source.turn(),
            call_id: "stay-fixture".into(),
            route_epoch: 1,
            microphone_generation: snapshot.microphone_generation(),
            camera_generation: snapshot.camera_generation(),
            issued_us: monotonic_us(),
        };
        let text = if case == "unbound" {
            "Private arbitrary computer words"
        } else {
            FIXED
        };
        let (mut says, answered) = crate::say::Says::words_for_test(
            "prepare-unplug",
            crate::say::Words::ExactSound {
                samples: vec![400; 480],
                text: Some(text.into()),
                pieces: None,
            },
            crate::say::NORMAL_PRIORITY,
        );
        if case != "unbound" {
            says.set_fixed_origin_for_test(origin, FIXED, "en");
        }
        let (mut reply, mut pending, mut trace) = (None, None, SessionState::default());
        let admission = InputAdmission::default();
        let sound = crate::say::SoundNow {
            conversing: false,
            ..sound_now(&mut owner, &reply, &admission, &says)
        };
        assert!(take_exact(
            &mut says, &sound, &mut owner, &mut reply, &mut trace
        ));
        assert!(matches!(answered.try_recv(), Ok(Answer::Begun(_))));
        if case == "unrendered" {
            // A renderer delivered a piece and closed short, as a failed
            // later TTS piece does. Its delivered PCM really traverses the
            // speaker helper and final retirement below.
            let feed = crate::speaker_stream::Feed::for_line(480);
            feed.push(&[400; 480]);
            feed.close_short();
            reply.as_mut().unwrap().notice = NoticeDelivery::fed(SpeechOrigin::ExactLine, feed);
        }
        let whole_audio = matches!(case, "unrendered" | "unbound");
        let mut played = play_sound(
            &mut owner,
            &mut reply,
            &mut pending,
            &mut trace,
            if whole_audio { 2 } else { 1 },
        );
        if case == "cancelled" {
            assert!(reply.as_ref().unwrap().fixed_line_receipt.is_none());
            cancel_reply(&mut owner, &mut reply, &mut trace, "user_interrupted").unwrap();
            assert!(
                reply.is_none(),
                "cancelled reply cannot later settle a receipt"
            );
            assert!(matches!(
                answered.try_recv(),
                Ok(Answer::Done(Outcome::Dropped(_)))
            ));
            continue;
        }
        if whole_audio {
            assert!(played.last.end_of_speech);
            played
                .peer
                .control
                .send(WorkerEvent::SpeechRetired {
                    owner: played.last.permit.owner(),
                    sequence: played.last.chunk_sequence,
                    final_sample: PlaybackCursor {
                        epoch: 1,
                        frame: played.cursor,
                    },
                    observed_at_us: monotonic_us(),
                })
                .unwrap();
            service_speaker_control(
                &mut played.speaker,
                &mut owner,
                &mut reply,
                &mut pending,
                &mut played.mask,
                &mut played.snapshot,
                &mut 0,
                &mut 16,
                &mut trace,
                &mut None,
            )
            .unwrap();
        } else {
            assert!(
                !played.last.end_of_speech,
                "only half of the exact line played"
            );
        }
        let current = reply.as_mut().unwrap();
        if case == "unrendered" {
            assert!(current.notice.retired(), "all delivered PCM retired");
            assert_eq!(
                current.notice.fed_whole(),
                Some(false),
                "the renderer owed more"
            );
        }
        owner.complete_turn(now(), current.owner).unwrap();
        current.answer_say(&pending, owner.snapshot(now()).unwrap());
        assert!(
            current.fixed_line_receipt.is_none(),
            "{case} cannot produce a history receipt"
        );
        let outcome = answered.try_recv().unwrap();
        if case == "unbound" {
            assert_eq!(
                outcome,
                Answer::Done(Outcome::Spoken),
                "arbitrary text can play without receiving history provenance"
            );
        } else {
            assert!(
                matches!(outcome, Answer::Done(Outcome::Dropped(_))),
                "{case}: {outcome:?}"
            );
        }
    }
}
