//! Audit regression: actual PhysicalBackend dispatch with a recording Player; no audio.
use crate::common;
use lamp_voice_eval::{
    Result,
    events::EventKind,
    physical::{
        mac::{LampMode, PhysicalBackend, PhysicalConfig, Player},
        protocol::SessionLine,
    },
    plan::LoadedPlan,
    record::StimulusSource,
    runner::{AttemptContext, Backend, Begin},
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

struct RecordingPlayer(Arc<AtomicUsize>);
impl Player for RecordingPlayer {
    fn start(
        &mut self,
        _: &Path,
        _: &SceneTiming,
        _: &Path,
        _: &Path,
    ) -> Result<JoinHandle<Value>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(thread::spawn(|| json!({"audit_no_audio":true})))
    }
    fn describe(&self) -> Value {
        json!({"kind":"audit_no_audio"})
    }
}

fn final_dispatch(kind: &str) -> usize {
    let dir = common::Private::new("dispatch");
    let catalog = StimulusCatalog::load().unwrap();
    let plan = LoadedPlan::default_plan(&catalog).unwrap();
    let scenario = plan.scenario("quick-chat").unwrap();
    let samples = common::tone_scene(&catalog, "quick-chat", 0.2);
    let manifest = common::synthetic_cache(&dir.path, &catalog, &[("quick-chat", samples)]);
    let assets = AssetIndex::load(&dir.join("cache"), &[manifest], &catalog).unwrap();
    let base = lamp_ipc::monotonic_us();
    let terminal = match kind {
        "session_end" => SessionLine::SessionEnd {
            lamp_us: base,
            exit_code: Some(0),
            killed: false,
            events_sha256: None,
            events_lines: 0,
            error: None,
            stray_processes_stopped: false,
            serve: None,
        },
        "cue_invalid" => SessionLine::CueInvalid {
            lamp_received_us: base,
            error: "unknown terminal outcome".into(),
        },
        _ => unreachable!(),
    };
    let lines = [
        SessionLine::SessionStart {
            schema: 1,
            lamp_us: base,
            mode: "directed".into(),
            cue_socket: true,
            runtime_argv: vec![],
        },
        SessionLine::Stdout {
            lamp_received_us: base,
            line: r#"{"status":"listening_ready"}"#.into(),
        },
        terminal,
    ];
    let relay = lines
        .iter()
        .map(|v| serde_json::to_string(v).unwrap() + "\n")
        .collect::<String>();
    fs::write(dir.join("relay.jsonl"), relay).unwrap();
    // The first read is the actual initial ping handshake. The second keeps
    // the inert transport alive until the backend is dropped (bounded there).
    fs::write(
        dir.join("relay.sh"),
        format!(
            "IFS= read -r first || exit 2\ncat '{}'\nIFS= read -r last || exit 0\n",
            dir.join("relay.jsonl").display()
        ),
    )
    .unwrap();
    let config = PhysicalConfig {
        lamp_command: vec!["/bin/sh".into(), dir.join("relay.sh").display().to_string()],
        lamp_work_root: dir.path.display().to_string(),
        mode: LampMode::Directed {
            provider_config: "/unused/audit-provider.json".into(),
        },
        noise_suppression: "off".into(),
        diagnostics: false,
        room_recorder: None,
        room_independent: false,
        allowance_seconds: 5,
        ring_channel_ceiling: None,
        stimulus_source: StimulusSource::LoudspeakerSynthetic,
        runtime_flags: Default::default(),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let mut backend =
        PhysicalBackend::new(config, &assets, Box::new(RecordingPlayer(calls.clone())));
    let run = dir.join("run");
    fs::create_dir(&run).unwrap();
    let context = AttemptContext {
        attempt_id: "dispatch".into(),
        run_id: "audit".into(),
        scenario,
        repetition: 0,
        order: 0,
        seed: 1,
        run_directory: run,
    };
    assert!(matches!(backend.begin(&context).unwrap(), Begin::Ready));
    let until = lamp_ipc::monotonic_us() + 1_000_000;
    let ready = backend.next_event(until).unwrap().unwrap();
    assert!(matches!(ready.kind, EventKind::ListeningReady));
    // Consume the terminal explicitly, proving this is current, known session
    // state, not speculative external timing. inject's new final guard must
    // also preserve that terminal state before it can call the Player.
    let ended = backend.next_event(until).unwrap().unwrap();
    assert!(matches!(ended.kind, EventKind::RunEnd { .. }));
    let timing = catalog.estimate("quick-chat").unwrap();
    let result = backend.inject(
        &scenario.steps[0],
        "quick-chat",
        &timing,
        lamp_ipc::monotonic_us(),
    );
    let count = calls.load(Ordering::SeqCst);
    eprintln!(
        "audit terminal={kind}, player_starts={count}, inject_ok={}, withheld={:?}",
        result.is_ok(),
        result
            .as_ref()
            .ok()
            .and_then(|i| i.withheld.as_ref().map(|w| w.reason()))
    );
    drop(backend);
    count
}

#[test]
fn final_dispatch_must_not_start_after_session_end() {
    assert_eq!(
        final_dispatch("session_end"),
        0,
        "Player started after known SessionEnd"
    );
}
#[test]
fn final_dispatch_must_not_start_after_cue_protocol_failure() {
    assert_eq!(
        final_dispatch("cue_invalid"),
        0,
        "Player started after known cue protocol failure"
    );
}
