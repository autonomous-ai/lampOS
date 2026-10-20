//! MAIN-023: the physical backend's final dispatch check, driven through a
//! real local pipe transport. A shell script stands in for the Lamp relay and
//! writes scripted relay lines; a recording-only player shows whether audio
//! would have started. Nothing is played.
use crate::common;
#[path = "common/final_drain_clock.rs"]
mod final_drain_clock;

use lamp_voice_eval::{
    events::EventKind,
    physical::{
        mac::{LampMode, PhysicalBackend, PhysicalConfig, Player},
        protocol::SCHEMA,
    },
    plan::LoadedPlan,
    record::StimulusSource,
    runner::{AttemptContext, Backend, Begin, Injection},
    stimulus::{AssetIndex, SceneTiming, StimulusCatalog},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
};

const SCENARIO: &str = "fixed-reply-echo-only";

struct Recording(Arc<AtomicUsize>);

impl Player for Recording {
    fn start(
        &mut self,
        _wav: &Path,
        _timing: &SceneTiming,
        _report_directory: &Path,
        _cancel_file: &Path,
    ) -> lamp_voice_eval::Result<JoinHandle<Value>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(thread::spawn(
            || json!({"valid": true, "status": "recorded_only"}),
        ))
    }
    fn describe(&self) -> Value {
        json!({"player": "recording only"})
    }
}

fn start_line() -> Value {
    json!({"type":"session_start","schema":SCHEMA,"lamp_us":1,"mode":"fixture",
        "cue_socket":true,"runtime_argv":[]})
}

fn session_end() -> Value {
    json!({"type":"session_end","lamp_us":9,"exit_code":1,"killed":false,
        "events_sha256":null,"events_lines":0,"error":"runtime exited with status 1"})
}

fn cue_invalid() -> Value {
    json!({"type":"cue_invalid","lamp_received_us":5,"error":"cue boot changed"})
}

fn run_end_cue() -> Value {
    json!({"type":"cue","lamp_received_us":1002,"cue":{"schema":1,"sequence":1,
        "kind":"run_end","boot":[1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        "event_us":1000,"sent_us":1001,"expires_us":101000}})
}

fn lines(values: &[Value]) -> String {
    values.iter().map(|v| format!("{v}\n")).collect()
}

/// What the stand-in relay does after its delayed lines.
enum Then {
    /// Keep the transport open (no end of stream).
    StayOpen,
    /// Exit, closing the transport without a `session_end`.
    Close,
}

struct Rig {
    dir: common::Private,
    _catalog: StimulusCatalog,
    plan: LoadedPlan,
    assets: AssetIndex,
    /// Measured once, as the runner does before dispatching, so a target
    /// time is not consumed by measuring the scene.
    timing: SceneTiming,
    starts: Arc<AtomicUsize>,
}

impl Rig {
    fn new(label: &str) -> Self {
        let catalog = StimulusCatalog::load().unwrap();
        let plan = LoadedPlan::default_plan(&catalog).unwrap();
        let dir = common::Private::new(label);
        let scene = plan.scenario(SCENARIO).unwrap().steps[0]
            .scene
            .clone()
            .unwrap();
        let tones = vec![(scene.as_str(), common::tone_scene(&catalog, &scene, 0.3))];
        let manifest = common::synthetic_cache(&dir.path, &catalog, &tones);
        let assets = AssetIndex::load(&dir.join("cache"), &[manifest], &catalog).unwrap();
        let mut timing = catalog.estimate(&scene).unwrap();
        timing.measure(&assets.pcm(&scene).unwrap());
        Self {
            dir,
            _catalog: catalog,
            plan,
            assets,
            timing,
            starts: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// A backend whose transport writes `now`, then after `delay` writes
    /// `later` (raw text, so framing errors can be scripted) and then `then`.
    fn backend(&self, now: &[Value], delay: &str, later: &str, then: Then) -> PhysicalBackend<'_> {
        fs::write(self.dir.join("now.jsonl"), lines(now)).unwrap();
        fs::write(self.dir.join("later.jsonl"), later).unwrap();
        let end = match then {
            // Keep the transport open while owned, consume pings silently,
            // and exit when the backend's existing Drop closes stdin.
            Then::StayOpen => "while IFS= read -r runner_line; do :; done",
            Then::Close => "exit 0",
        };
        let script = format!(
            "cat '{now}'; sleep {delay}; cat '{later}'; {end}",
            now = self.dir.join("now.jsonl").display(),
            later = self.dir.join("later.jsonl").display(),
        );
        PhysicalBackend::new(
            self.config(script),
            &self.assets,
            Box::new(Recording(self.starts.clone())),
        )
    }

    fn config(&self, script: String) -> PhysicalConfig {
        let lamp_root = self.dir.join("lamp");
        fs::create_dir_all(&lamp_root).unwrap();
        PhysicalConfig {
            lamp_command: vec!["/bin/sh".into(), "-c".into(), script, "relay".into()],
            lamp_work_root: lamp_root.display().to_string(),
            mode: LampMode::Fixture {
                reply: "/nonexistent/reply.wav".into(),
                sha256: "0".repeat(64),
            },
            noise_suppression: "on".into(),
            diagnostics: false,
            room_recorder: None,
            room_independent: false,
            allowance_seconds: 10,
            ring_channel_ceiling: None,
            stimulus_source: StimulusSource::LoudspeakerSynthetic,
            runtime_flags: Default::default(),
        }
    }

    fn begin(&self, backend: &mut PhysicalBackend<'_>) {
        let run = self.dir.join("run");
        lamp_voice_eval::ledger::create_run_directory(&run).unwrap();
        let scenario = self.plan.scenario(SCENARIO).unwrap();
        let context = AttemptContext {
            attempt_id: "a0".into(),
            run_id: "dispatch".into(),
            scenario,
            repetition: 0,
            order: 0,
            seed: 1,
            run_directory: run,
        };
        assert!(matches!(backend.begin(&context).unwrap(), Begin::Ready));
    }

    /// Final dispatch of the scenario's first stimulus at `at_us`.
    fn inject(&self, backend: &mut PhysicalBackend<'_>, at_us: u64) -> Injection {
        let step = &self.plan.scenario(SCENARIO).unwrap().steps[0];
        let scene = step.scene.clone().unwrap();
        backend.inject(step, &scene, &self.timing, at_us).unwrap()
    }

    fn started(&self) -> usize {
        self.starts.load(Ordering::SeqCst)
    }
}

/// Read live events until a run end is seen, as a runner would.
fn until_run_end(backend: &mut PhysicalBackend<'_>) {
    let deadline = backend.now_us() + 5_000_000;
    while backend.now_us() < deadline {
        if let Some(event) = backend.next_event(deadline).unwrap()
            && matches!(event.kind, EventKind::RunEnd { .. })
        {
            return;
        }
    }
    panic!("no run end within 5 s");
}

fn assert_withheld(rig: &Rig, injection: &Injection, cause: &str) {
    assert_eq!(rig.started(), 0, "the player must not start");
    assert!(injection.withheld.is_some(), "dispatch must be withheld");
    assert_eq!(injection.receipt["dispatched"], false);
    assert_eq!(
        injection.receipt["withheld_cause"], cause,
        "{}",
        injection.receipt
    );
    assert!(
        injection.receipt["reason"]
            .as_str()
            .is_some_and(|r| !r.is_empty()),
        "a precise reason is required: {}",
        injection.receipt
    );
}

#[test]
fn a_known_session_end_withholds_dispatch() {
    let rig = Rig::new("disp-end");
    let mut backend = rig.backend(&[start_line(), session_end()], "0", "", Then::StayOpen);
    rig.begin(&mut backend);
    until_run_end(&mut backend);
    let now = backend.now_us();
    let injection = rig.inject(&mut backend, now);
    assert_withheld(&rig, &injection, "session_ended");
    assert!(
        injection.receipt["reason"]
            .as_str()
            .unwrap()
            .contains("status 1")
    );
}

#[test]
fn a_known_invalid_cue_withholds_dispatch_as_a_protocol_failure() {
    let rig = Rig::new("disp-cue");
    let mut backend = rig.backend(&[start_line(), cue_invalid()], "0", "", Then::StayOpen);
    rig.begin(&mut backend);
    until_run_end(&mut backend);
    let now = backend.now_us();
    let injection = rig.inject(&mut backend, now);
    assert_withheld(&rig, &injection, "protocol_failed");
    assert!(
        injection.receipt["reason"]
            .as_str()
            .unwrap()
            .contains("cue boot changed")
    );
}

#[test]
fn a_known_run_end_cue_withholds_dispatch() {
    let rig = Rig::new("disp-runend");
    // The live pong lets the runner decide the cue; without any live clock
    // map it would be refused, never published.
    let pong = json!({"type":"pong","seq":2,"lamp_us":1_000});
    let mut backend = rig.backend(
        &[start_line(), pong, run_end_cue()],
        "0",
        "",
        Then::StayOpen,
    );
    rig.begin(&mut backend);
    until_run_end(&mut backend);
    let now = backend.now_us();
    let injection = rig.inject(&mut backend, now);
    assert_withheld(&rig, &injection, "session_ended");
}

#[test]
fn a_session_end_first_seen_in_the_final_drain_withholds_and_is_kept() {
    let rig = Rig::new("disp-late-end");
    let mut backend = final_drain_clock::backend(
        &rig,
        &[start_line()],
        "0.3",
        &lines(&[session_end()]),
        Then::StayOpen,
    );
    rig.begin(&mut backend);
    let at = backend.now_us() + 800_000;
    let injection = rig.inject(&mut backend, at);
    assert_withheld(&rig, &injection, "session_ended");
    // The drained end is still delivered to the runner.
    let now = backend.now_us();
    let next = backend.next_event(now).unwrap();
    assert!(matches!(
        next.map(|e| e.kind),
        Some(EventKind::RunEnd { .. })
    ));
}

#[test]
fn an_invalid_cue_first_seen_in_the_final_drain_withholds() {
    let rig = Rig::new("disp-late-cue");
    let mut backend = rig.backend(
        &[start_line()],
        "0.3",
        &lines(&[cue_invalid()]),
        Then::StayOpen,
    );
    rig.begin(&mut backend);
    let at = backend.now_us() + 800_000;
    let injection = rig.inject(&mut backend, at);
    assert_withheld(&rig, &injection, "protocol_failed");
}

#[test]
fn a_relay_framing_error_first_seen_in_the_final_drain_withholds() {
    let rig = Rig::new("disp-late-frame");
    let mut backend = rig.backend(&[start_line()], "0.3", "not a relay line\n", Then::StayOpen);
    rig.begin(&mut backend);
    let at = backend.now_us() + 800_000;
    let injection = rig.inject(&mut backend, at);
    assert_withheld(&rig, &injection, "protocol_failed");
}

#[test]
fn a_transport_closed_during_the_final_drain_withholds() {
    let rig = Rig::new("disp-late-close");
    let mut backend = rig.backend(&[start_line()], "0.3", "", Then::Close);
    rig.begin(&mut backend);
    let at = backend.now_us() + 800_000;
    let injection = rig.inject(&mut backend, at);
    assert_withheld(&rig, &injection, "session_ended");
    assert!(
        injection.receipt["reason"]
            .as_str()
            .unwrap()
            .contains("transport closed")
    );
}

#[test]
fn a_healthy_session_still_dispatches_once() {
    let rig = Rig::new("disp-ok");
    let mut backend = rig.backend(&[start_line()], "0", "", Then::StayOpen);
    rig.begin(&mut backend);
    let at = backend.now_us() + 200_000;
    let injection = rig.inject(&mut backend, at);
    assert!(injection.withheld.is_none(), "{}", injection.receipt);
    assert_eq!(rig.started(), 1);
}

fn cue(sequence: u64, kind: &str, owner: Option<(u64, u64)>, extra: Value) -> Value {
    const LAMP: u64 = 50_000_000;
    let event = LAMP + sequence * 10;
    let mut cue = json!({"schema":1,"sequence":sequence,"kind":kind,
        "boot":[1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        "event_us":event,"sent_us":event + 1,"expires_us":event + 100_000});
    if let Some((turn, generation)) = owner {
        cue["turn"] = json!(turn);
        cue["generation"] = json!(generation);
    }
    if let (Some(cue), Some(extra)) = (cue.as_object_mut(), extra.as_object()) {
        cue.extend(extra.clone());
    }
    json!({"type":"cue","lamp_received_us":event + 2,"cue":cue})
}

#[test]
fn the_first_terminal_cause_wins_and_a_provider_failure_stays_provider_unavailable() {
    let rig = Rig::new("disp-provider");
    let now = [
        start_line(),
        // Answers the ping the backend writes once session_start is taken
        // (ping 1 precedes it and never defines the map), so owned cues have
        // a clock map.
        json!({"type":"pong","seq":2,"lamp_us":50_000_000}),
        cue(1, "listening_ready", None, json!({})),
        cue(2, "input_admitted", Some((1, 1)), json!({})),
        cue(
            3,
            "turn_completed",
            Some((1, 1)),
            json!({"outcome":"provider_failed","playback_gaps":0}),
        ),
        session_end(),
    ];
    let mut backend = rig.backend(&now, "0", "", Then::StayOpen);
    rig.begin(&mut backend);
    until_run_end(&mut backend);
    let now = backend.now_us();
    let injection = rig.inject(&mut backend, now);
    assert_withheld(&rig, &injection, "provider_failed");
    assert_eq!(
        injection.withheld.map(|w| w.status()),
        Some(lamp_voice_eval::record::StepStatus::ProviderUnavailable)
    );
}

/// MAIN-025: a non-terminal event first taken by the backend's final drain
/// starts nothing; the runner must observe it before a second dispatch.
#[test]
fn an_unobserved_event_in_the_final_drain_requires_revalidation() {
    let rig = Rig::new("disp-revalidate");
    let ready = json!({"type":"stdout","lamp_received_us":3,
        "line":"{\"status\":\"listening_ready\"}"});
    let mut backend = rig.backend(&[start_line()], "0.3", &lines(&[ready]), Then::StayOpen);
    rig.begin(&mut backend);
    let at = backend.now_us() + 800_000;
    let injection = rig.inject(&mut backend, at);
    assert_eq!(rig.started(), 0);
    assert_eq!(injection.receipt["withheld_cause"], "revalidate");
    assert_eq!(injection.receipt["unobserved_events"], 1);
    let now = backend.now_us();
    let event = backend.next_event(now).unwrap();
    assert!(matches!(
        event.map(|e| e.kind),
        Some(EventKind::ListeningReady)
    ));
    let now = backend.now_us();
    let injection = rig.inject(&mut backend, now);
    assert!(injection.withheld.is_none(), "{}", injection.receipt);
    assert_eq!(rig.started(), 1);
}

/// Independent audit: time may elapse after runner eligibility and before the
/// final player boundary. The physical backend has the original target and
/// must not request a now-invalid stimulus even when no new event is queued.
#[test]
fn audit_physical_boundary_rejects_a_target_already_over_100ms_late() {
    let rig = Rig::new("audit-late-boundary");
    let mut backend = rig.backend(&[start_line()], "0", "", Then::StayOpen);
    rig.begin(&mut backend);
    // Consume the initial non-event so the boundary has no event that can
    // incidentally force Withheld::Revalidate.
    let drain_until = backend.now_us() + 50_000;
    assert!(backend.next_event(drain_until).unwrap().is_none());
    let target = backend.now_us().checked_sub(150_000).unwrap();
    let injection = rig.inject(&mut backend, target);
    eprintln!(
        "audit boundary: starts={}, requested_late_us={}, withheld={:?}",
        rig.started(),
        injection.receipt["late_us"],
        injection.withheld
    );
    assert_eq!(
        rig.started(),
        0,
        "an expired target must not reach Player::start"
    );
    assert!(injection.withheld.is_some());
}

/// The production relay starts Trace only after runtime exit (session.rs
/// stream_trace call after try_wait / kill and the final stdout/cue drain).
/// SessionEnd follows the complete trace, so it need not have arrived yet.
#[test]
fn audit_physical_boundary_does_not_dispatch_during_post_exit_trace_relay() {
    let rig = Rig::new("audit-trace-phase");
    let trace = json!({"type":"trace","record":{"kind":"run_end","at_us":10,"status":"failed"}});
    let mut backend = rig.backend(&[start_line(), trace], "0", "", Then::StayOpen);
    rig.begin(&mut backend);
    // Consume the actual post-exit record. It currently yields no live event.
    let drain_until = backend.now_us() + 50_000;
    let observed = backend.next_event(drain_until).unwrap();
    let at = backend.now_us();
    let injection = rig.inject(&mut backend, at);
    eprintln!(
        "audit post-exit trace: event={:?}, starts={}, withheld={:?}",
        observed.map(|e| e.kind),
        rig.started(),
        injection.withheld
    );
    assert_eq!(
        rig.started(),
        0,
        "post-exit trace relay must close live dispatch"
    );
    assert!(injection.withheld.is_some());
}

/// MAIN-030: the final drain is bounded. A relay flooding harmless lines
/// cannot hold dispatch open; the backend starts nothing and asks the runner
/// to revalidate, and a later dispatch drains the rest and starts once.
#[test]
fn a_final_drain_that_exhausts_its_budget_requires_revalidation() {
    let rig = Rig::new("disp-budget");
    let mut flood = vec![start_line()];
    flood.extend((0..2000).map(|_| json!({"type":"pong","seq":999_999,"lamp_us":5})));
    let mut backend = rig.backend(&flood, "0", "", Then::StayOpen);
    rig.begin(&mut backend);
    let at = backend.now_us() + 300_000;
    let injection = rig.inject(&mut backend, at);
    assert_eq!(rig.started(), 0);
    assert_eq!(injection.receipt["withheld_cause"], "revalidate");
    assert_eq!(
        injection.receipt["drain_exhausted"], true,
        "{}",
        injection.receipt
    );
    let now = backend.now_us();
    let injection = rig.inject(&mut backend, now);
    assert!(injection.withheld.is_none(), "{}", injection.receipt);
    assert_eq!(rig.started(), 1);
}
