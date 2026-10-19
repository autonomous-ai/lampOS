//! The cached serve-conversation example and the plan rules it relies on.
//! Existing plan bytes, and so their hashes, stay unchanged.
mod common;

use lamp_voice_eval::{
    plan::{DEFAULT_PLAN, LoadedPlan, MAX_STEPS, SERVE_PLAN, TriggerEvent},
    stimulus::StimulusCatalog,
};
use serde_json::Value;

#[test]
fn the_default_plan_hash_is_unchanged() {
    let catalog = StimulusCatalog::load().unwrap();
    let plan = LoadedPlan::parse(DEFAULT_PLAN, &catalog).unwrap();
    assert_eq!(
        plan.sha256,
        "e9dad734a420fc14c955e023bad49ea5f509809dd2c3079007ae89084a3023cc"
    );
}

#[test]
fn the_serve_example_is_eight_cached_context_linked_turns() {
    let catalog = StimulusCatalog::load().unwrap();
    let plan = LoadedPlan::parse(SERVE_PLAN, &catalog).unwrap();
    let scenario = &plan.plan.scenarios[0];
    assert_eq!(scenario.steps.len(), 8);
    // Every stimulus is a catalog scene with a cached asset identity; the
    // runner never renders speech.
    assert!(scenario.steps.iter().all(|s| {
        s.scene
            .as_deref()
            .is_some_and(|scene| catalog.scene(scene).is_ok())
    }));
    let triggers: Vec<_> = scenario.steps.iter().map(|s| s.trigger.after).collect();
    assert_eq!(
        triggers,
        [
            TriggerEvent::ListeningReady,
            TriggerEvent::NextTurnReady,
            TriggerEvent::NextTurnReady,
            TriggerEvent::SpeakerFirstWrite,
            TriggerEvent::NextTurnReady,
            TriggerEvent::LocalEndpoint,
            TriggerEvent::NextTurnReady,
            TriggerEvent::NextTurnReady,
        ]
    );
}

fn variant(edit: impl FnOnce(&mut Value)) -> String {
    let mut plan: Value = serde_json::from_str(SERVE_PLAN).unwrap();
    edit(&mut plan["scenarios"][0]["steps"]);
    plan.to_string()
}

#[test]
fn a_correction_must_follow_a_request_and_conversations_stop_at_twenty_turns() {
    let catalog = StimulusCatalog::load().unwrap();
    // The example's correction follows a request (a fragment); after a
    // step that expects no admitted request it is refused.
    let after_silence = variant(|steps| steps[4]["expect"] = "silence".into());
    let error = LoadedPlan::parse(&after_silence, &catalog).unwrap_err();
    assert!(error.to_string().contains("local_endpoint"), "{error}");

    let repeat = |count: usize| {
        variant(|steps| {
            let follow = steps[1].clone();
            let list = steps.as_array_mut().unwrap();
            list.truncate(2);
            for n in list.len()..count {
                let mut step = follow.clone();
                step["id"] = format!("follow-{n}").into();
                list.push(step);
            }
        })
    };
    let mut twenty: Value = serde_json::from_str(&repeat(MAX_STEPS)).unwrap();
    twenty["scenarios"][0]["session_seconds"] = 600.into();
    assert!(LoadedPlan::parse(&twenty.to_string(), &catalog).is_ok());
    let mut over: Value = serde_json::from_str(&repeat(MAX_STEPS + 1)).unwrap();
    over["scenarios"][0]["session_seconds"] = 600.into();
    let error = LoadedPlan::parse(&over.to_string(), &catalog).unwrap_err();
    assert!(error.to_string().contains("1..20 steps"), "{error}");
}

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

/// A dry `physical-run` of the serve example with `config`, against a
/// synthetic cache of its scenes. It starts no session.
fn dry_run(dir: &common::Private, config: &std::path::Path) -> std::process::Output {
    let catalog = StimulusCatalog::load().unwrap();
    let plan = LoadedPlan::parse(SERVE_PLAN, &catalog).unwrap();
    let tones: Vec<_> = plan.plan.scenarios[0]
        .steps
        .iter()
        .filter_map(|step| step.scene.as_deref())
        .map(|scene| (scene, common::tone_scene(&catalog, scene, 0.3)))
        .collect();
    let manifest = common::synthetic_cache(&dir.path, &catalog, &tones);
    std::process::Command::new(env!("CARGO_BIN_EXE_lamp-voice-eval"))
        .args([
            "physical-run",
            "--config",
            &config.display().to_string(),
            "--plan",
            &format!("{FIXTURES}/voice-eval-serve-v1.json"),
            "--cache",
            &dir.join("cache").display().to_string(),
            "--manifest",
            &manifest.display().to_string(),
            "--out",
            &dir.join("run").display().to_string(),
            "--scenario",
            "serve-conversation",
        ])
        .output()
        .unwrap()
}

fn sample() -> std::path::PathBuf {
    format!("{FIXTURES}/voice-eval-serve-pilot-config.example.json").into()
}

/// The shipped pilot config parses, validates its runtime policy with
/// lamp-voice's own rules and forwards it, in a dry run that starts nothing.
#[test]
fn the_sample_pilot_config_forwards_the_installed_pilot_settings() {
    let dir = common::Private::new("cfg");
    let output = dry_run(&dir, &sample());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["dry_run"], true);
    assert!(!dir.join("run").exists(), "a dry run creates nothing");
    assert_eq!(
        report["backend"]["runtime_flags"],
        serde_json::json!([
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
            "--voice-route",
            "second",
            "--runtime-env",
            "LAMP_SECOND_CHAT=deepseek",
            "--runtime-env",
            "LAMP_SECOND_SEARCH=deepseek",
            "--runtime-env",
            "LAMP_SECOND_SPEECH=elevenlabs",
            "--runtime-env",
            "LAMP_VOICE_SECOND_CONFIG=/etc/lampOS/voice/second.json"
        ])
    );
    assert_eq!(report["backend"]["noise_suppression"], "off");
    assert_eq!(report["attempts"][0]["status"], "would run");
}

/// The opt-in endpoint trial: the same pilot settings plus `--endpoint
/// smart-turn`, so its turns compare with the existing cached-speech cohorts.
#[test]
fn the_endpoint_trial_config_adds_only_the_endpoint_flag() {
    let dir = common::Private::new("cfg");
    let trial: std::path::PathBuf =
        format!("{FIXTURES}/voice-eval-serve-endpoint-trial.example.json").into();
    let pilot: Value = serde_json::from_str(&std::fs::read_to_string(sample()).unwrap()).unwrap();
    let mut differs: Value =
        serde_json::from_str(&std::fs::read_to_string(&trial).unwrap()).unwrap();
    assert_eq!(differs["endpoint"], "smart-turn");
    differs.as_object_mut().unwrap().remove("endpoint");
    assert_eq!(differs, pilot, "the trial changes nothing else");
    let output = dry_run(&dir, &trial);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["backend"]["runtime_flags"],
        serde_json::json!([
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
            "--endpoint",
            "smart-turn",
            "--voice-route",
            "second",
            "--runtime-env",
            "LAMP_SECOND_CHAT=deepseek",
            "--runtime-env",
            "LAMP_SECOND_SEARCH=deepseek",
            "--runtime-env",
            "LAMP_SECOND_SPEECH=elevenlabs",
            "--runtime-env",
            "LAMP_VOICE_SECOND_CONFIG=/etc/lampOS/voice/second.json"
        ])
    );
}

/// The owner, 19 Oct 19:3x: a run is on the second route unless it names
/// live. One without the second route's configuration is refused on the Mac,
/// before anything starts on the lamp; one that names live (a comparison the
/// first builder names on #1712) runs, and its record says live.
#[test]
fn a_run_is_on_the_second_route_unless_it_names_live() {
    let dir = common::Private::new("cfg");
    let mut config: Value =
        serde_json::from_str(&std::fs::read_to_string(sample()).unwrap()).unwrap();
    let settings = config.as_object_mut().unwrap();
    settings.remove("voice_route");
    settings.remove("runtime_env");
    let path = dir.join("config.json");
    std::fs::write(&path, config.to_string()).unwrap();
    let output = dry_run(&dir, &path);
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("second route needs its configuration"),
        "{error}"
    );
    config["voice_route"] = "live".into();
    std::fs::write(&path, config.to_string()).unwrap();
    let output = dry_run(&dir, &path);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let flags = report["backend"]["runtime_flags"].as_array().unwrap();
    assert_eq!(flags[flags.len() - 2..], ["--voice-route", "live"]);
}

#[test]
fn a_config_policy_lamp_voice_would_refuse_is_refused_on_the_mac() {
    let dir = common::Private::new("cfg");
    let mut config: Value =
        serde_json::from_str(&std::fs::read_to_string(sample()).unwrap()).unwrap();
    // Bounded recovery without a policy that masks Lamp's own playback.
    config["turn_taking"] = "vad-only".into();
    config["reference_recovery"] = "bounded".into();
    let path = dir.join("config.json");
    std::fs::write(&path, config.to_string()).unwrap();
    let output = dry_run(&dir, &path);
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("lamp-voice would refuse runtime options"),
        "{error}"
    );
}
