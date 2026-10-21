//! MAIN-021: room evidence from a plain WAV and root's source recorder
//! receipt. The fixture is root's real, nonsecret example
//! (`lamp-v2-chat` `3998a8dc`, `coordination/recording-import-examples.json`);
//! only frame counts and digests are adapted to a generated WAV. File-only:
//! no audio device or transport is opened.
use crate::common;

use lamp_sound_fixtures::hash;
use lamp_voice_eval::{
    annotations::{Annotations, AttemptAnnotation, StepAnnotation},
    import::{ImportOptions, import_trace, receipt_room_evidence, verify_room_evidence},
    plan::LoadedPlan,
    record::{AttemptRecord, AttemptStatus, StimulusSource},
    stimulus::StimulusCatalog,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

const EXAMPLES: &str = include_str!("../fixtures/recording-import-examples.json");
const FRAMES: u32 = 32_000;

fn examples() -> Value {
    serde_json::from_str(EXAMPLES).unwrap()
}

/// A mono PCM16 WAV; returns (file SHA-256, PCM data SHA-256).
fn write_wav(path: &Path, rate: u32, value: i16) -> (String, String) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    let mut pcm = Vec::new();
    for index in 0..FRAMES {
        let sample = value.wrapping_add((index % 97) as i16);
        writer.write_sample(sample).unwrap();
        pcm.extend_from_slice(&sample.to_le_bytes());
    }
    writer.finalize().unwrap();
    (
        hash(&fs::read(path).unwrap()),
        format!("{:x}", Sha256::digest(&pcm)),
    )
}

/// Root's successful receipt with the voice input bound to a generated WAV.
fn receipt_for(wav_sha: &str, pcm_sha: &str) -> Value {
    let mut receipt = examples()["source_capture_receipt"].clone();
    let frames = u64::from(FRAMES);
    let inner = &mut receipt["receipt"]["captures"]["voice"];
    inner["frames"] = json!(frames);
    inner["pcm_bytes"] = json!(frames * 2);
    inner["pcm_sha256"] = json!(pcm_sha);
    let outer = &mut receipt["captures"]["voice"];
    outer["frames_received"] = json!(frames);
    outer["pcm_bytes_received"] = json!(frames * 2);
    outer["pcm_sha256_received"] = json!(pcm_sha);
    outer["wav_sha256"] = json!(wav_sha);
    assert_eq!(outer["wav"], "lamp-52e6-voice.wav");
    receipt
}

struct Fixture {
    dir: common::Private,
    receipt: std::path::PathBuf,
    wav_sha: String,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let dir = common::Private::new(label);
        let (wav_sha, pcm_sha) = write_wav(&dir.join("lamp-52e6-voice.wav"), 16_000, 300);
        let receipt = dir.join("lamp-52e6-capture-receipt.json");
        fs::write(&receipt, receipt_for(&wav_sha, &pcm_sha).to_string()).unwrap();
        Self {
            dir,
            receipt,
            wav_sha,
        }
    }
    fn evidence(&self) -> Value {
        receipt_room_evidence(&self.receipt, "voice", true)
    }
    fn rewrite(&self, change: impl FnOnce(&mut Value)) {
        let mut receipt: Value = serde_json::from_slice(&fs::read(&self.receipt).unwrap()).unwrap();
        change(&mut receipt);
        fs::write(&self.receipt, receipt.to_string()).unwrap();
    }
}

fn error(evidence: &Value) -> String {
    evidence["verification_error"]
        .as_str()
        .unwrap_or("")
        .to_owned()
}

#[test]
fn a_valid_receipt_and_its_wav_are_verified_as_room_evidence() {
    let f = Fixture::new("rcpt-ok");
    let evidence = f.evidence();
    assert_eq!(evidence["valid"], true, "{}", error(&evidence));
    assert_eq!(evidence["status"], "captured_unscored");
    assert_eq!(evidence["format"], "recorder_receipt");
    assert_eq!(evidence["sha256"], f.wav_sha);
    assert_eq!(evidence["verified_wav"]["frames"], FRAMES);
    assert_eq!(evidence["receipt"]["restoration_verified"], true);
    assert_eq!(
        evidence["receipt"]["boundary"],
        "host read/receipt timing, not ADC or acoustic timing"
    );
    // Bounded, nonsecret retention: no stderr text is copied.
    assert!(!evidence.to_string().contains("Interrupted system call"));
    assert_eq!(verify_room_evidence(&evidence).unwrap().sha256, f.wav_sha);
}

#[test]
fn roots_real_invalid_receipt_is_retained_and_refused() {
    let dir = common::Private::new("rcpt-invalid");
    let receipt = dir.join("receipt.json");
    let example = examples()["invalid_source_capture_receipt"].clone();
    fs::write(&receipt, example.to_string()).unwrap();
    let evidence = receipt_room_evidence(&receipt, "voice", true);
    assert_eq!(evidence["valid"], false);
    assert!(
        error(&evidence).contains("Microphone permission closed or unknown"),
        "{}",
        error(&evidence)
    );
    assert_eq!(evidence["receipt"]["privacy_revoked"], true);
    assert!(
        evidence["received"]["disposition"]
            .as_str()
            .unwrap()
            .contains("PCM discarded")
    );
    assert!(verify_room_evidence(&evidence).is_err());
}

#[test]
fn samples_that_differ_from_the_received_pcm_are_refused() {
    let f = Fixture::new("rcpt-pcm");
    // A different WAV whose file hash the receipt is updated to claim.
    let (wav_sha, _) = write_wav(&f.dir.join("lamp-52e6-voice.wav"), 16_000, 301);
    f.rewrite(|r| r["captures"]["voice"]["wav_sha256"] = json!(wav_sha));
    assert!(
        error(&f.evidence()).contains("samples differ"),
        "{}",
        error(&f.evidence())
    );
}

#[test]
fn a_wav_in_another_format_or_length_is_refused() {
    let f = Fixture::new("rcpt-format");
    let (wav_sha, pcm_sha) = write_wav(&f.dir.join("lamp-52e6-voice.wav"), 48_000, 300);
    f.rewrite(|r| {
        r["captures"]["voice"]["wav_sha256"] = json!(wav_sha);
        r["captures"]["voice"]["pcm_sha256_received"] = json!(pcm_sha);
        r["receipt"]["captures"]["voice"]["pcm_sha256"] = json!(pcm_sha);
    });
    assert!(
        error(&f.evidence()).contains("not mono 16 kHz"),
        "{}",
        error(&f.evidence())
    );
    let f = Fixture::new("rcpt-length");
    f.rewrite(|r| {
        r["receipt"]["captures"]["voice"]["frames"] = json!(FRAMES + 1);
        r["receipt"]["captures"]["voice"]["pcm_bytes"] = json!((FRAMES + 1) * 2);
        r["captures"]["voice"]["frames_received"] = json!(FRAMES + 1);
        r["captures"]["voice"]["pcm_bytes_received"] = json!((FRAMES + 1) * 2);
    });
    assert!(
        error(&f.evidence()).contains("frame count"),
        "{}",
        error(&f.evidence())
    );
}

#[test]
fn inconsistent_or_unsafe_receipts_are_refused() {
    for (label, change, expected) in [
        (
            "rcpt-name",
            Box::new(|r: &mut Value| {
                r["captures"]["voice"]["wav"] = json!("../lamp-52e6-voice.wav")
            }) as Box<dyn Fn(&mut Value)>,
            "plain sibling",
        ),
        (
            "rcpt-digest",
            Box::new(|r: &mut Value| {
                r["captures"]["voice"]["pcm_sha256_received"] = json!("0".repeat(64))
            }),
            "digests disagree",
        ),
        (
            "rcpt-kill",
            Box::new(|r: &mut Value| {
                r["receipt"]["captures"]["voice"]["forced_kill"] = json!(true)
            }),
            "graceful stop",
        ),
        (
            "rcpt-rate",
            Box::new(|r: &mut Value| r["rate_hz"] = json!(48_000)),
            "mono 16 kHz S16_LE",
        ),
        (
            "rcpt-input-invalid",
            Box::new(|r: &mut Value| r["captures"]["voice"]["valid"] = json!(false)),
            "marked the voice capture invalid",
        ),
    ] {
        let f = Fixture::new(label);
        f.rewrite(change);
        let evidence = f.evidence();
        assert_eq!(evidence["valid"], false, "{label}");
        assert!(
            error(&evidence).contains(expected),
            "{label}: {}",
            error(&evidence)
        );
    }
    let f = Fixture::new("rcpt-missing-input");
    assert_eq!(
        receipt_room_evidence(&f.receipt, "camera", true)["valid"],
        false
    );
}

#[test]
fn a_receipt_changed_after_import_fails_review_time_verification() {
    let f = Fixture::new("rcpt-changed");
    let evidence = f.evidence();
    assert_eq!(evidence["valid"], true);
    f.rewrite(|r| r["receipt"]["privacy_checks"] = json!(352));
    let failure = verify_room_evidence(&evidence).unwrap_err().to_string();
    assert!(failure.contains("changed since import"), "{failure}");
}

fn import(f: &Fixture) -> AttemptRecord {
    let events = f.dir.join("events.jsonl");
    fs::write(
        &events,
        concat!(
            "{\"kind\":\"run_start\",\"at_us\":1,\"provider_kind\":\"one_cached_reply\"}\n",
            "{\"kind\":\"run_end\",\"at_us\":2,\"status\":\"completed_unscored\",\"error\":null}\n"
        ),
    )
    .unwrap();
    let catalog = StimulusCatalog::load().unwrap();
    let plan = LoadedPlan::default_plan(&catalog).unwrap();
    let out = f.dir.join("run");
    lamp_voice_eval::ledger::create_run_directory(&out).unwrap();
    import_trace(
        &plan,
        &catalog,
        &ImportOptions {
            run_id: "receipt-import".into(),
            events_path: &events,
            scenario: "fixed-reply-echo-only".into(),
            turn_map: Vec::new(),
            room_metadata: None,
            room_receipt: Some((&f.receipt, "voice")),
            room_independent: true,
            source: StimulusSource::LoudspeakerSynthetic,
        },
        &out,
    )
    .unwrap()
}

#[test]
fn an_imported_trace_with_a_receipt_is_annotated_and_scored_only_after_listening() {
    let f = Fixture::new("rcpt-import");
    let record = import(&f);
    assert_eq!(record.evidence["room_audio"]["valid"], true);
    // No diagnostics beside this trace: target audio is unverified, not adapted.
    assert_eq!(record.evidence["target_audio"]["render"]["verified"], false);
    let template =
        lamp_voice_eval::annotations::template(&record.run_id, std::slice::from_ref(&record));
    let entry = &template.attempts[&record.attempt_id];
    assert_eq!(
        entry.room_recording_sha256.as_deref(),
        Some(f.wav_sha.as_str())
    );
    assert!(!entry.listened);
    let mut annotations = Annotations {
        schema: 1,
        run_id: record.run_id.clone(),
        annotator: "synthetic test".into(),
        method: "synthetic times, not acoustic evidence".into(),
        attempts: BTreeMap::from([(
            record.attempt_id.clone(),
            AttemptAnnotation {
                room_recording_sha256: Some(f.wav_sha.clone()),
                listened: false,
                steps: BTreeMap::from([(
                    "greet".into(),
                    StepAnnotation {
                        user_speech_end_s: Some(0.25),
                        first_substantive_answer_word_s: Some(1.25),
                        ..StepAnnotation::default()
                    },
                )]),
                notes: None,
            },
        )]),
    };
    assert!(annotations.score(&record)[0].measurement().is_none());
    annotations
        .attempts
        .get_mut(&record.attempt_id)
        .unwrap()
        .listened = true;
    assert_eq!(annotations.score(&record)[0].value_ms, Some(1000.0));
    assert_eq!(record.status, AttemptStatus::Completed);
}

#[test]
fn an_invalid_receipt_keeps_the_attempt_with_unmeasured_acoustics() {
    let f = Fixture::new("rcpt-import-invalid");
    f.rewrite(|r| {
        r["receipt"]["valid"] = json!(false);
        r["receipt"]["failure"] = json!("RuntimeError: Microphone permission closed or unknown");
    });
    let record = import(&f);
    assert_eq!(record.status, AttemptStatus::Completed);
    assert_eq!(record.evidence["room_audio"]["valid"], false);
    let template =
        lamp_voice_eval::annotations::template(&record.run_id, std::slice::from_ref(&record));
    let annotations = Annotations {
        schema: 1,
        run_id: record.run_id.clone(),
        annotator: "synthetic test".into(),
        method: "synthetic".into(),
        attempts: template.attempts,
    };
    let scored = annotations.score(&record);
    assert!(scored.iter().all(|s| s.measurement().is_none()));
}

/// MAIN-034 (1): an absurd frame count is invalid evidence, never a panic,
/// and the attempt is still imported with unmeasured acoustics.
#[test]
fn an_absurd_receipt_frame_count_is_invalid_evidence_not_a_panic() {
    let f = Fixture::new("rcpt-overflow");
    f.rewrite(|r| {
        r["receipt"]["captures"]["voice"]["frames"] = json!(u64::MAX);
        r["receipt"]["captures"]["voice"]["pcm_bytes"] = json!(u64::MAX);
        r["captures"]["voice"]["frames_received"] = json!(u64::MAX);
        r["captures"]["voice"]["pcm_bytes_received"] = json!(u64::MAX);
    });
    let evidence = f.evidence();
    assert_eq!(evidence["valid"], false);
    assert!(
        error(&evidence).contains("frame count"),
        "{}",
        error(&evidence)
    );
    let record = import(&f);
    assert_eq!(record.status, AttemptStatus::Completed);
    assert_eq!(record.evidence["room_audio"]["valid"], false);
}

/// MAIN-034 (2): review rebinds the WAV to the receipt's own selected record.
/// Cached evidence that names a different, internally valid WAV is refused
/// even though the receipt bytes are unchanged.
#[test]
fn review_rebinds_the_wav_to_the_receipt_not_to_cached_claims() {
    let f = Fixture::new("rcpt-rebind");
    let mut record = import(&f);
    assert_eq!(record.evidence["room_audio"]["valid"], true);
    let other = f.dir.join("other.wav");
    let (other_sha, other_pcm) = write_wav(&other, 16_000, 999);
    let room = &mut record.evidence["room_audio"];
    room["wav_path"] = json!(other.to_str().unwrap());
    room["sha256"] = json!(other_sha);
    room["expected_pcm_sha256"] = json!(other_pcm);
    let failure = verify_room_evidence(&record.evidence["room_audio"])
        .unwrap_err()
        .to_string();
    assert!(failure.contains("receipt"), "{failure}");
    let annotations = Annotations {
        schema: 1,
        run_id: record.run_id.clone(),
        annotator: "synthetic test".into(),
        method: "synthetic".into(),
        attempts: BTreeMap::from([(
            record.attempt_id.clone(),
            AttemptAnnotation {
                room_recording_sha256: Some(other_sha),
                listened: true,
                steps: BTreeMap::from([(
                    "greet".into(),
                    StepAnnotation {
                        user_speech_end_s: Some(0.25),
                        first_substantive_answer_word_s: Some(1.25),
                        ..StepAnnotation::default()
                    },
                )]),
                notes: None,
            },
        )]),
    };
    assert!(annotations.score(&record)[0].measurement().is_none());
}
