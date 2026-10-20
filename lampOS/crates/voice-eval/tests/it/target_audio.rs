//! MAIN-021: offline verification of real lamp-voice diagnostics directories,
//! written by lamp-voice's own `Recorder`, against a runtime trace.
use crate::common;

use lamp_turn_owner::BootId;
use lamp_voice::{
    diagnostic_control::{DiagnosticFinished, DiagnosticStarted, DiagnosticStream},
    diagnostics::{
        CaptureMeta, Config, DEFAULT_FINISH_WAIT, EndMeta, EndReason, PrivacyMeta, Recorder,
        RenderMeta, SilenceKind, StreamKind,
    },
};
use lamp_voice_eval::target_audio::{self, TargetAudio};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

const BOOT: [u8; 16] = [7; 16];

fn render_stream(runtime: &Path, reply: &[i16]) -> (DiagnosticStarted, DiagnosticFinished) {
    let boot = BootId::new(BOOT).unwrap();
    let mut recorder = Recorder::start(Config::new(
        runtime.join("render-audio"),
        boot,
        StreamKind::Render,
    ))
    .unwrap();
    let started = DiagnosticStarted {
        boot,
        stream: DiagnosticStream::Render,
        started_at_us: recorder.started_at_us(),
    };
    let at = recorder.started_at_us();
    recorder.try_privacy(PrivacyMeta {
        at_us: at,
        generation: 1,
        open: true,
    });
    let meta = |first: u64, count: u64, at_us: u64| RenderMeta {
        playback_epoch: 1,
        privacy_generation: 1,
        first_sample: first,
        end_sample: first + count,
        accepted_at_us: at_us,
        queue_observed_at_us: at_us,
        queued_frames: count as i64,
        owner: None,
        chunk_sequence: None,
    };
    recorder.try_silence(meta(0, 240, at + 1), SilenceKind::Prime, 240);
    let mut first = 240;
    for (index, chunk) in reply.chunks(240).enumerate() {
        let count = chunk.len() as u64;
        recorder.try_render(meta(first, count, at + 2 + index as u64 * 10_000), chunk);
        first += count;
    }
    let report = recorder.finish(
        EndMeta {
            at_us: lamp_ipc::monotonic_us().max(at + 1_000_000),
            reason: EndReason::Stopped,
        },
        DEFAULT_FINISH_WAIT,
    );
    assert!(report.valid, "{report:?}");
    (started, DiagnosticFinished::from_report(started, report))
}

fn capture_stream(runtime: &Path) -> (DiagnosticStarted, DiagnosticFinished) {
    let boot = BootId::new(BOOT).unwrap();
    let mut recorder = Recorder::start(Config::new(
        runtime.join("capture-audio"),
        boot,
        StreamKind::Capture,
    ))
    .unwrap();
    let started = DiagnosticStarted {
        boot,
        stream: DiagnosticStream::Capture,
        started_at_us: recorder.started_at_us(),
    };
    let at = recorder.started_at_us();
    recorder.try_privacy(PrivacyMeta {
        at_us: at,
        generation: 2,
        open: true,
    });
    for sequence in 1..=2 {
        let meta = CaptureMeta {
            epoch: 1,
            dsp_epoch: 1,
            privacy_generation: 2,
            frame_sequence: sequence,
            first_read_started_at_us: at + sequence,
            last_read_started_at_us: at + sequence,
            read_completed_at_us: at + sequence,
            successful_reads: 1,
            processing_completed_at_us: at + sequence,
            ..CaptureMeta::default()
        };
        recorder.try_capture(meta, &[300; 160], &[200; 160]);
    }
    let report = recorder.finish(
        EndMeta {
            at_us: lamp_ipc::monotonic_us().max(at + 3),
            reason: EndReason::Completed,
        },
        DEFAULT_FINISH_WAIT,
    );
    assert!(report.valid, "{report:?}");
    (started, DiagnosticFinished::from_report(started, report))
}

fn trace_entries(
    worker: &str,
    started: DiagnosticStarted,
    receipt: &DiagnosticFinished,
) -> [Value; 2] {
    [
        json!({"kind":"audio_diagnostics_started","worker":worker,"worker_boot":BOOT,"details":started}),
        json!({"kind":"audio_diagnostics_certified","worker":worker,"worker_boot":BOOT,
            "valid":true,"receipt":receipt,"error":null}),
    ]
}

/// A runtime directory with both streams, and its trace.
fn runtime(dir: &common::Private, reply: &[i16]) -> (std::path::PathBuf, Vec<Value>) {
    let runtime = dir.join("runtime");
    fs::create_dir(&runtime).unwrap();
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
    let (render_start, render_receipt) = render_stream(&runtime, reply);
    let (capture_start, capture_receipt) = capture_stream(&runtime);
    let mut trace = vec![json!({"kind":"run_start","at_us":1})];
    trace.extend(trace_entries("speaker", render_start, &render_receipt));
    trace.extend(trace_entries("capture", capture_start, &capture_receipt));
    (runtime, trace)
}

fn reply() -> Vec<i16> {
    (0..600).map(|i| ((i % 50) * 100) as i16).collect()
}

fn render_error(audio: &TargetAudio) -> String {
    audio.render.as_ref().err().cloned().unwrap_or_default()
}

#[test]
fn real_render_and_capture_streams_verify_with_contiguous_spans() {
    let dir = common::Private::new("ta-ok");
    let (runtime, trace) = runtime(&dir, &reply());
    let audio = target_audio::verify(&runtime, &trace);
    let (render, spans) = audio.render.as_ref().unwrap();
    assert_eq!(render.frames, 240 + 600);
    assert_eq!(render.rate, 24_000);
    assert_eq!(spans.len(), 1 + 3);
    assert_eq!(spans[0].silence.as_deref(), Some("prime"));
    assert_eq!(
        spans
            .iter()
            .map(|s| (s.offset_frames, s.frames))
            .collect::<Vec<_>>(),
        vec![(0, 240), (240, 240), (480, 240), (720, 120)]
    );
    let pcm = target_audio::read_pcm(render, "render_accepted.pcm16le").unwrap();
    assert_eq!(&pcm[..240], &[0; 240]);
    assert_eq!(&pcm[240..], reply().as_slice());
    let capture = audio.capture.as_ref().unwrap();
    assert_eq!((capture.frames, capture.rate), (320, 16_000));
    let pre = target_audio::read_pcm(capture, "pre_aec.pcm16le").unwrap();
    assert_eq!(pre, vec![300; 320]);
    assert_eq!(audio.evidence()["render"]["verified"], true);
}

#[test]
fn a_changed_pcm_byte_leaves_only_that_stream_unverified() {
    let dir = common::Private::new("ta-tamper");
    let (runtime, trace) = runtime(&dir, &reply());
    let path = runtime.join("render-audio/render_accepted.pcm16le");
    let mut bytes = fs::read(&path).unwrap();
    bytes[600] ^= 1;
    fs::write(&path, bytes).unwrap();
    let audio = target_audio::verify(&runtime, &trace);
    assert!(
        render_error(&audio).contains("certified hash"),
        "{}",
        render_error(&audio)
    );
    assert!(audio.capture.is_ok());
}

#[test]
fn an_uncertified_or_missing_receipt_is_unverified_not_adapted() {
    let dir = common::Private::new("ta-uncert");
    let (runtime, mut trace) = runtime(&dir, &reply());
    trace[2]["valid"] = json!(false);
    trace[2]["error"] = json!("diagnostic finish was not valid and acknowledged");
    let audio = target_audio::verify(&runtime, &trace);
    assert!(
        render_error(&audio).contains("did not certify"),
        "{}",
        render_error(&audio)
    );
    trace.remove(2);
    let audio = target_audio::verify(&runtime, &trace);
    assert!(render_error(&audio).contains("no audio_diagnostics_certified"));
}

#[test]
fn a_replaced_marker_or_a_duplicate_identity_fails() {
    let dir = common::Private::new("ta-marker");
    let (runtime, mut trace) = runtime(&dir, &reply());
    let duplicate = trace[1].clone();
    trace.push(duplicate);
    let audio = target_audio::verify(&runtime, &trace);
    assert!(render_error(&audio).contains("more than one"));
    trace.pop();
    fs::write(runtime.join("render-audio/complete.json"), b"{}").unwrap();
    let audio = target_audio::verify(&runtime, &trace);
    assert!(audio.render.is_err());
}

#[test]
fn pcm_changed_after_verification_is_refused_on_read() {
    let dir = common::Private::new("ta-race");
    let (runtime, trace) = runtime(&dir, &reply());
    let audio = target_audio::verify(&runtime, &trace);
    let (render, _) = audio.render.as_ref().unwrap();
    let path = runtime.join("render-audio/render_accepted.pcm16le");
    let mut bytes = fs::read(&path).unwrap();
    bytes[0] ^= 1;
    fs::write(&path, bytes).unwrap();
    assert!(target_audio::read_pcm(render, "render_accepted.pcm16le").is_err());
}

/// MAIN-034: a correctly hashed but absurd completion marker frame count is
/// unverified evidence, never an arithmetic panic.
#[test]
fn an_absurd_marker_frame_count_is_unverified_not_a_panic() {
    let dir = common::Private::new("ta-overflow");
    let (runtime, mut trace) = runtime(&dir, &reply());
    let path = runtime.join("render-audio/complete.json");
    let mut marker: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    marker["render_frames"] = json!(u64::MAX);
    let bytes = serde_json::to_vec(&marker).unwrap();
    fs::write(&path, &bytes).unwrap();
    let digest = sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(&bytes));
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    trace[2]["receipt"]["completion_sha256"] = json!(hex);
    let audio = target_audio::verify(&runtime, &trace);
    assert!(
        render_error(&audio).contains("frame count"),
        "{}",
        render_error(&audio)
    );
}
