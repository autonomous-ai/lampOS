//! The serve-session runner end to end without hardware: the runner starts
//! the real `lamp-session` relay in serve mode, which starts the fake
//! `lamp-voice serve` (cues through lamp-voice's own `CueSink`, serve-shaped
//! rolling metadata, SIGTERM stop). Every stimulus is prepared before its
//! trigger and started by one original expiring grant at the fake prepared
//! player's first "copy", a fake room datagram. Explicit clock; no audio.
//!
//! Attempts run one at a time with real child processes and prepared threads.
//! The coordinated path retains the unchanged 100 ms lateness bound, recorder
//! lifetimes and full delayed-transport wait before actual relay registration.
use crate::common;

use lamp_voice_eval::{
    evaluate,
    events::{EventKind, EventSource},
    physical::mac::{FakePlayerFault, LampMode, PhysicalBackend, PhysicalConfig},
    physical::serve_fixture_driver::ClockedServeFixture,
    plan::{LoadedPlan, SERVE_PLAN, TriggerEvent},
    record::{AttemptRecord, AttemptStatus, StepStatus, Stratum},
    runner::{SuiteOptions, run_suite},
    stimulus::{AssetIndex, StimulusCatalog},
};
use serde_json::Value;
use std::{fs, sync::Mutex};

const BIN: &str = env!("CARGO_BIN_EXE_lamp-voice-eval");
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

#[derive(Default)]
struct Control {
    /// Keep only this many steps of the example conversation.
    steps: Option<usize>,
    fake_args: Vec<&'static str>,
    player_faults: Vec<(usize, FakePlayerFault)>,
    recorder_fail_ms: Option<u32>,
    /// Extra fake recorder arguments (`--exit-code`, `--declare-end-after-ms`).
    recorder_args: Vec<&'static str>,
    /// Start the relay this late, like an SSH transport still connecting.
    transport_delay_ms: Option<u32>,
    runtime_flags: lamp_voice_eval::physical::session::RuntimeFlags,
}

fn plan_source(steps: Option<usize>) -> String {
    let mut plan: Value = serde_json::from_str(SERVE_PLAN).unwrap();
    if let Some(steps) = steps {
        plan["scenarios"][0]["steps"]
            .as_array_mut()
            .unwrap()
            .truncate(steps);
    }
    serde_json::to_string_pretty(&plan).unwrap()
}

fn run(control: Control) -> AttemptRecord {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let catalog = StimulusCatalog::load().unwrap();
    let source = plan_source(control.steps);
    let plan = LoadedPlan::parse(&source, &catalog).unwrap();
    let scenario = plan.plan.scenarios[0].clone();
    let dir = common::Private::new("srv");
    let tones: Vec<_> = scenario
        .steps
        .iter()
        .filter_map(|step| step.scene.as_deref())
        .map(|scene| (scene, common::tone_scene(&catalog, scene, 0.3)))
        .collect();
    let manifest = common::synthetic_cache(&dir.path, &catalog, &tones);
    let assets = AssetIndex::load(&dir.join("cache"), &[manifest], &catalog).unwrap();
    let plan_copy = dir.join("plan.json");
    fs::write(&plan_copy, &source).unwrap();
    let lamp_root = dir.join("lamp");
    fs::create_dir(&lamp_root).unwrap();
    let room = dir.join("room.sock");
    let mut command = match control.transport_delay_ms {
        Some(ms) => vec![
            BIN.to_owned(),
            "fake-slow-transport".into(),
            "--delay-ms".into(),
            ms.to_string(),
            "--".into(),
        ],
        None => Vec::new(),
    };
    command.extend([
        BIN.to_owned(),
        "lamp-session".into(),
        "--runtime".into(),
        BIN.into(),
    ]);
    let mut prefix: Vec<String> = [
        "fake-lamp-voice",
        "--fake-plan",
        &plan_copy.display().to_string(),
        "--fake-scenario",
        &scenario.id,
        "--fake-profile",
        "v2-serve-v1-reopen",
        "--fake-room",
        &room.display().to_string(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    prefix.extend(control.fake_args.iter().map(|s| s.to_string()));
    for arg in prefix {
        command.extend(["--runtime-prefix-arg".to_owned(), arg]);
    }
    let room_recorder = control.recorder_fail_ms.map(|ms| {
        let mut argv = vec![
            BIN.to_owned(),
            "fake-room-recorder".into(),
            "--out".into(),
            "{out}".into(),
            "--seconds".into(),
            "{seconds}".into(),
            "--fail-after-ms".into(),
            ms.to_string(),
        ];
        argv.extend(control.recorder_args.iter().map(|s| s.to_string()));
        argv
    });
    let mut config = PhysicalConfig {
        lamp_command: command,
        lamp_work_root: lamp_root.display().to_string(),
        mode: LampMode::Serve {
            provider_config: "/private/unused-fake-provider.json".into(),
        },
        noise_suppression: "on".into(),
        diagnostics: false,
        room_recorder,
        room_independent: false,
        allowance_seconds: 10,
        ring_channel_ceiling: None,
        stimulus_source: lamp_voice_eval::record::StimulusSource::LoudspeakerSynthetic,
        runtime_flags: control.runtime_flags,
    };
    let mut fixture = ClockedServeFixture::new(&dir.path).unwrap();
    fixture.configure(&mut config).unwrap();
    let clock = fixture.clock();
    let player = fixture.player(room, control.player_faults);
    let mut backend =
        PhysicalBackend::new_with_clock(config, &assets, Box::new(player), move || clock.now())
            .with_fixture_driver(fixture)
            .unwrap();
    let run = dir.join("run");
    lamp_voice_eval::ledger::create_run_directory(&run).unwrap();
    let options = SuiteOptions {
        run_id: "serve".into(),
        scenarios: vec![scenario.id.clone()],
        repetitions: 1,
        seed: 5,
        shuffle: false,
        attempt_seed: None,
        self_test: Value::Null,
    };
    let mut records = run_suite(&plan, &catalog, &mut backend, &options, &run).unwrap();
    // The ledger retains the attempt exactly as returned.
    let ledger = fs::read_to_string(run.join("ledger.jsonl")).unwrap();
    assert!(ledger.contains("attempt_finished"));
    let record = records.remove(0);
    if let Some(directory) = std::env::var_os("SERVE_SESSION_DUMP") {
        let name = format!("{}.json", std::thread::current().name().unwrap_or("record"));
        fs::write(
            std::path::Path::new(&directory).join(name.replace("::", "-")),
            serde_json::to_vec_pretty(&record).unwrap(),
        )
        .unwrap();
    }
    // Scoring never fails on any control.
    let _ = evaluate::score(&record, plan.plan.answer_deadline_ms, None);
    record
}

fn statuses(record: &AttemptRecord) -> Vec<StepStatus> {
    record.steps.iter().map(|step| step.status).collect()
}

fn serve_end(record: &AttemptRecord) -> &Value {
    &record.evidence["session_end"]["serve"]
}

/// Every control ends through the relay's one stop path with its process
/// group confirmed gone, and its metadata is either complete or reported.
fn assert_reaped(record: &AttemptRecord) {
    let serve = serve_end(record);
    assert_eq!(serve["group_reaped"], true, "{serve}");
    assert!(
        serve["exited_us"].is_u64() || serve["forced_kill"] == true,
        "{serve}"
    );
    assert!(
        record.evidence["transport_exit"].is_i64(),
        "{}",
        record.evidence["transport_exit"]
    );
}

fn assert_clean_stop(record: &AttemptRecord) {
    assert_reaped(record);
    let serve = serve_end(record);
    assert_eq!(serve["stop_cause"], "runner_stop", "{serve}");
    assert_eq!(serve["forced_kill"], false);
    assert!(serve["signal_sent_us"].as_u64() >= serve["stop_requested_us"].as_u64());
    assert!(serve["exited_us"].as_u64() > serve["signal_sent_us"].as_u64());
    let metadata = &serve["metadata"];
    assert_eq!(metadata["sequence_gaps"], 0, "{metadata}");
    assert_eq!(metadata["history_truncated"], false);
    assert!(metadata["error"].is_null(), "{metadata}");
    assert_eq!(metadata["health"]["writer_finished"], true);
    assert_eq!(record.evidence["session_end"]["killed"], false);
}

/// The prepared receipt of one dispatched step: armed before its trigger,
/// granted once with the original deadline, first copy before expiry.
fn assert_granted(record: &AttemptRecord, step: &str) {
    let step_record = record.steps.iter().find(|s| s.step == step).unwrap();
    assert_eq!(
        step_record.status,
        StepStatus::Injected,
        "{step}: {:?}",
        step_record.detail
    );
    let prepared = &step_record.receipt["prepared"];
    let expires = prepared["expires_monotonic_ns"].as_u64().unwrap();
    let headroom = prepared["margin"]["headroom_at_grant_us"].as_i64().unwrap();
    assert!((1..=100_000).contains(&headroom), "{step}: {prepared}");
    assert_eq!(prepared["margin"]["expired_at_send"], false);
    assert!(
        prepared["armed_runner_us"].as_u64().unwrap()
            < step_record.trigger.as_ref().unwrap().runner_us,
        "{step} was armed only after its trigger"
    );
    let playback = record.evidence["playbacks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["step"] == step)
        .unwrap();
    assert_eq!(playback["prepared"], true);
    let copied = playback["report"]["counters"]["source_started_host_ns"]
        .as_u64()
        .unwrap();
    assert!(copied > 0 && copied < expires, "{step}: {playback}");
    // The corrected start is the first copy, not the grant.
    assert_eq!(step_record.started_us, Some(copied / 1000));
}

#[test]
fn normal_eight_turn_conversation_in_one_serve_session() {
    let record = run(Control::default());
    assert_eq!(
        record.status,
        AttemptStatus::Completed,
        "{:#}",
        record.evidence
    );
    assert_eq!(record.stratum, Stratum::PhysicalServe);
    assert_eq!(statuses(&record), vec![StepStatus::Injected; 8]);
    for step in [
        "sky", "why", "sunset", "coffee", "more", "correct", "change", "math",
    ] {
        assert_granted(&record, step);
    }
    // Each next turn waited for the readiness event after its turn finished.
    for step in record.steps.iter().filter(|s| {
        s.trigger
            .as_ref()
            .is_some_and(|t| t.event == TriggerEvent::NextTurnReady)
    }) {
        let trigger = step.trigger.as_ref().unwrap();
        let finished = record
            .live_events
            .iter()
            .filter(|e| e.turn == trigger.turn)
            .find(|e| {
                matches!(
                    e.kind,
                    EventKind::TurnCompleted { .. } | EventKind::TurnCancelled { .. }
                )
            })
            .and_then(|e| e.at_us)
            .unwrap();
        let ready = trigger.event_at_us.unwrap();
        assert!(
            ready >= finished + 90_000,
            "{}: ready {} us after finish",
            step.step,
            ready - finished
        );
    }
    // The correction bound to the superseded follow-up's own endpoint.
    let more = record.steps.iter().find(|s| s.step == "more").unwrap();
    let correct = record.steps.iter().find(|s| s.step == "correct").unwrap();
    let endpoint = correct.trigger.as_ref().unwrap();
    assert_eq!(endpoint.event, TriggerEvent::LocalEndpoint);
    assert!(endpoint.turn > more.trigger.as_ref().unwrap().turn);
    // One runtime cycle, one provider session, no continuity loss.
    let continuity = &record.evidence["serve_continuity"];
    assert_eq!(continuity["service_cycles"], 1, "{continuity}");
    assert_eq!(continuity["context_continuity_lost"], false);
    assert!(
        record.evidence["protocol_errors"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        record.evidence["unused_prepared_players"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        record
            .live_events
            .iter()
            .all(|e| e.source != EventSource::Runner)
    );
    assert_clean_stop(&record);
}

fn step<'a>(record: &'a AttemptRecord, id: &str) -> &'a lamp_voice_eval::record::StepRecord {
    record.steps.iter().find(|s| s.step == id).unwrap()
}

fn detail(record: &AttemptRecord, id: &str) -> String {
    step(record, id).detail.clone().unwrap_or_default()
}

#[test]
fn delayed_readiness_is_awaited_by_event_not_by_completion() {
    let record = run(Control {
        steps: Some(3),
        fake_args: vec!["--fake-reopen-ms", "700"],
        ..Control::default()
    });
    assert_eq!(
        record.status,
        AttemptStatus::Completed,
        "{:#}",
        record.evidence
    );
    assert_eq!(statuses(&record), vec![StepStatus::Injected; 3]);
    for id in ["why", "sunset"] {
        let trigger = step(&record, id).trigger.clone().unwrap();
        let finished = record
            .live_events
            .iter()
            .find(|e| e.turn == trigger.turn && matches!(e.kind, EventKind::TurnCompleted { .. }))
            .and_then(|e| e.at_us)
            .unwrap();
        let ready_at = trigger.event_at_us.unwrap();
        assert!(
            ready_at >= finished + 690_000,
            "{id}: {} us",
            ready_at - finished
        );
        // The readiness event is the reopened availability itself.
        assert!(record.live_events.iter().any(|e| e.at_us == Some(ready_at)
            && e.kind == EventKind::InputAvailability { available: true }));
        assert_granted(&record, id);
    }
    assert_clean_stop(&record);
}

#[test]
fn an_extra_candidate_during_the_wait_withholds_the_rest() {
    let record = run(Control {
        steps: Some(3),
        fake_args: vec!["--fake-extra-candidate", "1:30"],
        ..Control::default()
    });
    assert_eq!(
        record.status,
        AttemptStatus::Completed,
        "{:#}",
        record.evidence
    );
    assert_eq!(
        statuses(&record),
        [
            StepStatus::Injected,
            StepStatus::PreconditionLost,
            StepStatus::Withheld
        ]
    );
    assert!(
        detail(&record, "why").contains("unplanned input"),
        "{}",
        detail(&record, "why")
    );
    assert!(
        step(&record, "why").receipt.is_null(),
        "nothing was granted"
    );
    assert_clean_stop(&record);
}

#[test]
fn stale_duplicate_reordered_foreign_and_lost_cues_end_the_conversation() {
    for (fault, expected) in [
        ("stale-completion", "cue"),
        ("duplicate-completion", "order changed"),
        ("reorder-retirement", "order changed"),
        ("foreign-completion", "context continuity lost"),
        ("drop-completion", "order changed"),
    ] {
        let record = run(Control {
            steps: Some(3),
            fake_args: vec!["--fake-cue-fault", fault],
            ..Control::default()
        });
        let errors = record.evidence["protocol_errors"].to_string();
        assert!(errors.contains(expected), "{fault}: {errors}");
        assert!(
            matches!(&record.status, AttemptStatus::Aborted { reason } if reason.contains("invalid scheduling")),
            "{fault}: {:?}",
            record.status
        );
        assert_eq!(statuses(&record)[0], StepStatus::Injected, "{fault}");
        assert_eq!(statuses(&record)[1], StepStatus::SessionEnded, "{fault}");
        assert_eq!(statuses(&record)[2], StepStatus::Withheld, "{fault}");
        // The rejected cue is retained with its reason.
        assert!(
            record.evidence["cue_receipts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["accepted"] == false),
            "{fault}"
        );
        assert_clean_stop(&record);
    }
}

#[test]
fn a_provider_outage_is_waited_out_by_event_and_a_terminal_link_ends_it() {
    let record = run(Control {
        steps: Some(2),
        fake_args: vec!["--fake-outage", "1:50:800"],
        ..Control::default()
    });
    assert_eq!(
        record.status,
        AttemptStatus::Completed,
        "{:#}",
        record.evidence
    );
    assert_eq!(statuses(&record), vec![StepStatus::Injected; 2]);
    let link = |state: &str| {
        record
            .live_events
            .iter()
            .filter(|e| matches!(&e.kind, EventKind::ProviderLink { state: s } if s == state))
            .filter_map(|e| e.at_us)
            .collect::<Vec<_>>()
    };
    let down = link("unavailable")[0];
    let back = *link("ready").last().unwrap();
    assert!(back >= down + 790_000);
    let trigger = step(&record, "why").trigger.clone().unwrap();
    assert_eq!(
        trigger.event_at_us,
        Some(back),
        "dispatched on the recovery"
    );
    assert_granted(&record, "why");
    assert_clean_stop(&record);

    let record = run(Control {
        steps: Some(3),
        fake_args: vec!["--fake-terminal", "1:50"],
        ..Control::default()
    });
    assert_eq!(
        statuses(&record),
        [
            StepStatus::Injected,
            StepStatus::ProviderUnavailable,
            StepStatus::Withheld
        ]
    );
    let serve = serve_end(&record);
    assert_eq!(serve["stop_cause"], "runtime_exited", "{serve}");
    assert_eq!(record.evidence["session_end"]["exit_code"], 1);
    assert!(
        matches!(&record.status, AttemptStatus::Aborted { reason } if reason.contains("exited with status 1")),
        "{:?}",
        record.status
    );
    assert_reaped(&record);
}

#[test]
fn a_late_first_copy_or_failed_player_withholds_every_later_turn() {
    let record = run(Control {
        steps: Some(3),
        player_faults: vec![(2, FakePlayerFault::LateFirstCopy { delay_ms: 150 })],
        ..Control::default()
    });
    assert_eq!(
        statuses(&record),
        [
            StepStatus::Injected,
            StepStatus::DeliveryFailed,
            StepStatus::PreconditionLost
        ]
    );
    let playback = record.evidence["playbacks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["step"] == "why")
        .unwrap();
    assert_eq!(playback["report"]["status"], "start_expired", "{playback}");
    assert_eq!(playback["report"]["counters"]["source_started_host_ns"], 0);
    assert!(
        detail(&record, "sunset").contains("player:why"),
        "{}",
        detail(&record, "sunset")
    );
    // Nothing reached the room: no second turn was ever admitted.
    assert!(!record.live_events.iter().any(|e| e.turn == Some(2)));
    assert_clean_stop(&record);

    let record = run(Control {
        steps: Some(3),
        player_faults: vec![(2, FakePlayerFault::PrepareFails)],
        ..Control::default()
    });
    assert_eq!(
        statuses(&record),
        [
            StepStatus::Injected,
            StepStatus::DeliveryFailed,
            StepStatus::Withheld
        ]
    );
    assert!(
        detail(&record, "why").contains("player_failed"),
        "{}",
        detail(&record, "why")
    );
    assert_clean_stop(&record);
}

#[test]
fn a_runtime_stop_mute_or_ignored_sigterm_is_retained_and_reaped() {
    let record = run(Control {
        steps: Some(3),
        fake_args: vec!["--fake-stop", "1:50"],
        ..Control::default()
    });
    assert_eq!(statuses(&record)[1], StepStatus::SessionEnded);
    let serve = serve_end(&record);
    assert_eq!(serve["stop_cause"], "runtime_exited", "{serve}");
    assert_eq!(serve["metadata"]["sequence_gaps"], 0);
    assert!(record.events.iter().any(|e| matches!(&e.kind,
        EventKind::RunEnd { status: Some(s), .. } if s == "stopped_unscored")));
    assert_reaped(&record);

    let record = run(Control {
        steps: Some(3),
        fake_args: vec!["--fake-mute", "1:50:300"],
        ..Control::default()
    });
    assert_eq!(
        statuses(&record)[1],
        StepStatus::SessionEnded,
        "{:?}",
        record.steps[1].detail
    );
    let continuity = &record.evidence["serve_continuity"];
    assert_eq!(continuity["service_cycles"], 2, "{continuity}");
    assert_eq!(continuity["context_continuity_lost"], true);
    assert_eq!(continuity["verified_service_restart"], true);
    assert!(
        !continuity["boot_changes_observed"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        record.evidence["protocol_errors"]
            .to_string()
            .contains("context continuity lost")
    );
    // Only the first cycle is scored.
    assert!(
        continuity["unscored_records_after_first_cycle"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_clean_stop(&record);

    let record = run(Control {
        steps: Some(1),
        fake_args: vec!["--fake-ignore-sigterm", "yes"],
        ..Control::default()
    });
    let serve = serve_end(&record);
    assert_eq!(serve["forced_kill"], true, "{serve}");
    assert_eq!(record.evidence["session_end"]["killed"], true);
    assert!(
        matches!(&record.status, AttemptStatus::Aborted { reason } if reason.contains("stop grace")),
        "{:?}",
        record.status
    );
    assert_reaped(&record);
}

#[test]
fn a_room_recording_that_ends_early_withholds_the_rest() {
    let record = run(Control {
        steps: Some(3),
        recorder_fail_ms: Some(6_000),
        ..Control::default()
    });
    assert_eq!(statuses(&record)[0], StepStatus::Injected);
    assert_eq!(statuses(&record)[1], StepStatus::PreconditionLost);
    assert!(
        detail(&record, "why").contains("room_recorder"),
        "{}",
        detail(&record, "why")
    );
    assert_ne!(record.evidence["room_audio"]["valid"], true);
    assert_clean_stop(&record);
}

/// The 4ace startup refusal (wdqcviuk): the first ping was written while the
/// transport was still connecting, the first player's preparation then held
/// the runner, and the first cue was judged against that setup-inflated
/// clock map. Only pings written after the relay's session start now define
/// the map, and the session is consumed while a player prepares; the 100 ms
/// cue bound is unchanged.
#[test]
fn startup_cues_stay_fresh_through_a_slow_transport_and_a_slow_first_preparer() {
    let record = run(Control {
        steps: Some(2),
        transport_delay_ms: Some(400),
        // Ready 2.3 s after the fake runtime starts: its startup cues come at
        // ~2.0 s, inside the preparation, and the first prompt is due at
        // readiness + 500 ms, after it.
        player_faults: vec![(1, FakePlayerFault::SlowPrepare { delay_ms: 2_300 })],
        ..Control::default()
    });
    assert_eq!(
        record.status,
        AttemptStatus::Completed,
        "{}",
        record.evidence["protocol_errors"]
    );
    assert_eq!(
        statuses(&record)[0],
        StepStatus::Injected,
        "{}",
        detail(&record, "sky")
    );
    let receipts = record.evidence["cue_receipts"].as_array().unwrap();
    assert!(
        receipts.iter().all(|r| r["accepted"] == true),
        "{receipts:?}"
    );
    // The startup cues were taken while the first player was preparing.
    let sky_ready_us =
        record.evidence["playbacks"][0]["report"]["prepared_start"]["ready"]["ready_monotonic_ns"]
            .as_u64()
            .unwrap()
            / 1000;
    let startup: Vec<&Value> = receipts
        .iter()
        .take_while(|r| r["cue"]["kind"] != "input_candidate")
        .collect();
    assert!(
        startup.iter().any(|r| r["runner_received_us"]
            .as_u64()
            .is_some_and(|at| at < sky_ready_us)),
        "no startup cue arrived during preparation: {startup:?} ready {sky_ready_us}"
    );
    // Decision-time clock evidence: each startup cue was judged with a
    // mapping that could prove it fresh.
    for receipt in &startup {
        let freshness = &receipt["freshness"];
        assert!(
            freshness["uncertainty_us"]
                .as_u64()
                .is_some_and(|u| u < 50_000),
            "{receipt}"
        );
        assert!(
            freshness["margin_us"].as_i64().is_some_and(|m| m > 0),
            "{receipt}"
        );
    }
    // Ping 1 waited while the transport connected: retained, never used.
    let samples = record.evidence["clock_samples"].as_array().unwrap();
    assert_eq!(samples[0]["seq"], 1);
    assert_eq!(samples[0]["usable"], false, "{}", samples[0]);
    assert_eq!(samples[0]["became_best"], false);
    assert!(
        samples[0]["rtt_us"]
            .as_u64()
            .is_some_and(|rtt| rtt >= 400_000),
        "{}",
        samples[0]
    );
    let first_usable = samples.iter().find(|s| s["usable"] == true).unwrap();
    assert!(
        first_usable["rtt_us"]
            .as_u64()
            .is_some_and(|rtt| rtt < 100_000),
        "{first_usable}"
    );
    assert_clean_stop(&record);
}

#[test]
fn a_recorder_that_exits_zero_mid_conversation_withholds_the_rest_and_certifies_nothing() {
    // Like a shortened capture behind `open -W`: exit 0, no declared end.
    let record = run(Control {
        steps: Some(3),
        recorder_fail_ms: Some(6_000),
        recorder_args: vec!["--exit-code", "0"],
        ..Control::default()
    });
    assert_eq!(statuses(&record)[0], StepStatus::Injected);
    assert_eq!(statuses(&record)[1], StepStatus::PreconditionLost);
    assert!(detail(&record, "why").contains("room_recorder"));
    let room = &record.evidence["room_audio"];
    assert_eq!(room["recorder_exit"], 0);
    assert_eq!(
        room["coverage_lost"]["kind"], "recorder_exited_early",
        "{room}"
    );
    let score = evaluate::score(&record, 15_000, None);
    let state = serde_json::to_value(&score.room_recording).unwrap();
    assert_eq!(state["state"], "invalid", "{state}");
    assert_eq!(state["status"], "ended_during_session", "{state}");
    assert!(score.acoustic.iter().all(|a| a.measurement().is_none()));
    assert_clean_stop(&record);
}

#[test]
fn a_declared_capture_end_before_the_next_prompt_and_its_window_withholds_it() {
    // The recording declares an end that covers the first prompt and its 8 s
    // observation window but not the second; the recorder is still running.
    let record = run(Control {
        steps: Some(3),
        // From recorder start: the first prompt needs ~16 s (start ~4.2 s,
        // speech to 3.9 s, 8 s window), the second ~24 s.
        recorder_fail_ms: Some(23_000),
        recorder_args: vec!["--exit-code", "0", "--declare-end-after-ms", "20000"],
        ..Control::default()
    });
    assert_eq!(statuses(&record)[0], StepStatus::Injected);
    assert_eq!(statuses(&record)[1], StepStatus::PreconditionLost);
    assert!(
        detail(&record, "why").contains("declared capture end"),
        "{}",
        detail(&record, "why")
    );
    // Normally refused before the grant; a slow run may reach the declared
    // end first. Either way the recorder did not exit early.
    let room = &record.evidence["room_audio"];
    assert!(
        matches!(
            room["coverage_lost"]["kind"].as_str(),
            Some("prompt_not_covered" | "declared_end_reached")
        ),
        "{room}"
    );
    assert!(room["declared_capture_end_runner_us"].as_u64().is_some());
    // Only the first prompt was granted.
    assert_eq!(
        record.evidence["playbacks"].as_array().map(Vec::len),
        Some(1)
    );
    assert_clean_stop(&record);
}

#[test]
fn the_runner_forwards_pilot_runtime_settings_through_the_relay() {
    let mut runtime_flags = lamp_voice_eval::physical::session::RuntimeFlags::default();
    for (flag, value) in [
        ("--turn-taking", "duplex"),
        ("--reference-clock", "capture"),
        ("--reference-recovery", "off"),
        ("--playout-start-ms", "200"),
        ("--provider-continuation", "kept-start"),
    ] {
        assert!(runtime_flags.take(flag, value).unwrap());
    }
    let record = run(Control {
        steps: Some(1),
        runtime_flags,
        ..Control::default()
    });
    assert_eq!(
        record.status,
        AttemptStatus::Completed,
        "{:#}",
        record.evidence
    );
    assert_eq!(statuses(&record), [StepStatus::Injected]);
    let argv: Vec<String> =
        serde_json::from_value(record.evidence["session_start"]["runtime_argv"].clone()).unwrap();
    let from = argv
        .iter()
        .position(|a| a == "--noise-suppression")
        .unwrap();
    let to = argv.iter().position(|a| a == "--cue-socket").unwrap();
    assert_eq!(
        argv[from..to],
        [
            "--noise-suppression",
            "on",
            "--turn-taking",
            "duplex",
            "--reference-clock",
            "capture",
            "--reference-recovery",
            "off",
            "--playout-start-ms",
            "200",
            "--provider-continuation",
            "kept-start",
        ]
    );
    assert_eq!(
        record.evidence["session_start"]["runtime_argv"][from - 3],
        "serve"
    );
    assert_clean_stop(&record);
}

#[test]
fn serve_readiness_comes_only_from_the_fresh_cue_never_from_stdout() {
    let record = run(Control {
        steps: Some(1),
        fake_args: vec!["--fake-early-ready-stdout", "yes"],
        ..Control::default()
    });
    // The runtime's own readiness record, which its cue carries.
    let ready = record
        .events
        .iter()
        .find(|e| e.kind == EventKind::ListeningReady)
        .and_then(|e| e.at_us)
        .unwrap();
    let trigger = step(&record, "sky").trigger.clone().unwrap();
    assert_eq!(
        trigger.event_at_us,
        Some(ready),
        "triggered on stale stdout"
    );
    assert!(
        record
            .live_events
            .iter()
            .all(|e| !(e.kind == EventKind::ListeningReady && e.source == EventSource::Stdout))
    );
    assert_eq!(statuses(&record), [StepStatus::Injected]);
    assert_granted(&record, "sky");
    assert_clean_stop(&record);
}

#[test]
fn a_nonzero_runtime_exit_never_completes_a_serve_attempt() {
    let record = run(Control {
        steps: Some(1),
        fake_args: vec!["--fake-exit-code", "2"],
        ..Control::default()
    });
    assert_eq!(statuses(&record), [StepStatus::Injected]);
    assert_eq!(record.evidence["session_end"]["exit_code"], 2);
    assert!(
        matches!(&record.status, AttemptStatus::Aborted { reason } if reason.contains("exited with status 2")),
        "{:?}",
        record.status
    );
}
