#![cfg(unix)]
use lamp_audio::{
    EchoProcessor, RENDER_SAMPLES,
    drift::{Controller, Resampler, TARGET_FRAMES},
    replay::{self, HistoricalProcessing, ReplayOptions},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const BOOT: [u8; 16] = [3; 16];
struct Fixture {
    root: PathBuf,
    run: PathBuf,
    capture: Vec<Value>,
    render: Vec<Value>,
    pre: Vec<i16>,
    post: Vec<i16>,
    accepted: Vec<i16>,
    valid: bool,
    processing: Option<Value>,
    /// The run's start, first in the session's events, when given.
    session: Option<Value>,
    max_seconds: u64,
}
fn directories() -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "lamp-replay-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    fs::create_dir(root.join("artifacts")).unwrap();
    let run = root.join("run");
    fs::create_dir(&run).unwrap();
    for leaf in ["capture-audio", "render-audio"] {
        fs::create_dir(run.join(leaf)).unwrap();
    }
    (root, run)
}
impl Fixture {
    fn new() -> Self {
        let (root, run) = directories();
        let mut capture = vec![
            row(
                "privacy",
                json!({"at_us":1,"generation":2,"open":true}),
                0,
                0,
                0,
                None,
            ),
            row("reset", reset(20, "start", 1, 1, None), 0, 0, 0, None),
        ];
        let mut render = vec![
            row(
                "privacy",
                json!({"at_us":1,"generation":2,"open":true}),
                0,
                0,
                0,
                None,
            ),
            row("reset", reset(2, "start", 1, 1, None), 0, 0, 0, None),
        ];
        let mut accepted = vec![0; 480];
        let first: [i16; 240] = std::array::from_fn(|i| ((i as i32 * 317) % 16000 - 8000) as i16);
        let second: [i16; 240] = std::array::from_fn(|i| ((i as i32 * 263) % 12000 - 6000) as i16);
        accepted.extend(first);
        accepted.extend(second);
        for (begin, count, at, prime) in [
            (0, 120, 3, true),
            (120, 120, 4, true),
            (240, 240, 5, true),
            (480, 120, 21, false),
            (600, 120, 22, false),
            (720, 240, 41, false),
        ] {
            render.push(row("render_accepted",json!({"playback_epoch":1,"privacy_generation":2,"first_sample":begin,"end_sample":begin+count,"accepted_at_us":at,"queue_observed_at_us":at,"queued_frames":480,"owner":null,"chunk_sequence":null}),count,0,begin*2,prime.then_some("prime")));
        }
        let mut processor = EchoProcessor::new(true);
        processor.render(&[0; 240]).unwrap();
        processor.render(&[0; 240]).unwrap();
        let mut pre = Vec::new();
        let mut post = Vec::new();
        for index in 0..3 {
            if index == 0 {
                processor.render(&first).unwrap();
            } else if index == 1 {
                processor.render(&second).unwrap();
            }
            let mut samples: [i16; 160] =
                std::array::from_fn(|i| ((i as i32 * 191 + index * 337) % 12000 - 6000) as i16);
            if index == 0 {
                samples[0] = i16::MAX;
            }
            post.extend(processor.capture(&samples, 18).unwrap());
            pre.extend(samples);
            let at = 30 + index as u64 * 20;
            let analysed = if index == 0 { 720 } else { 960 };
            let reference_at = if index == 0 { 22 } else { 41 };
            capture.push(row("capture",json!({"epoch":1,"dsp_epoch":1,"privacy_generation":2,"frame_sequence":index+1,"first_read_started_at_us":at,"last_read_started_at_us":at,"read_completed_at_us":at+2,"successful_reads":1,"alsa_status_monotonic_us":at+2,"status_observed_at_us":at+3,"available_frames":0,"delayed_frames":32,"processing_completed_at_us":at+5,"aec_queue_delay_ms":18,"reference":{"playback_epoch":1,"accepted_through":analysed,"analysed_through":analysed,"capture_blocks_processed":index+1,"accepted_at_us":reference_at,"analysed_at_us":at-2,"queue_observed_at_us":reference_at,"queued_frames":480,"clock_started_at_us":21,"ignored_old_epoch_packets":0},"vad_score":0.4}),160,index as u64*320,0,None));
        }
        capture.push(row(
            "end",
            json!({"at_us":90,"reason":"stopped"}),
            0,
            960,
            0,
            None,
        ));
        render.push(row(
            "end",
            json!({"at_us":90,"reason":"stopped"}),
            0,
            0,
            1920,
            None,
        ));
        let fixture = Self {
            root,
            run,
            capture,
            render,
            pre,
            post,
            accepted,
            valid: true,
            processing: Some(json!({"aec":"sonora_aec3","noise_suppression":true})),
            session: None,
            max_seconds: 1,
        };
        fixture.save();
        fixture
    }

    /// A capture-clocked recording made as the voice makes one
    /// (`ReferenceAssembly::capture_clocked`, `prime`, `accept` and
    /// `render_for_capture` in `crates/voice/src/reference.rs`): the ring
    /// starts with its lead and the prime, every packet the speaker accepts
    /// waits in it, and each capture block takes one block from it at the
    /// rate the controller asks. The speaker runs 1,000 ppm fast and every
    /// seventh packet comes late with the next, so the rate keeps moving.
    /// `reset_after` puts a DSP reset with a new playback epoch there.
    fn capture_clocked(blocks: usize, reset_after: Option<usize>) -> Self {
        const PRIME: usize = 480;
        const LEAD: usize = TARGET_FRAMES - PRIME;
        let (root, run) = directories();
        let privacy = json!({"at_us":1,"generation":2,"open":true});
        let mut capture = vec![row("privacy", privacy.clone(), 0, 0, 0, None)];
        let mut render = vec![row("privacy", privacy, 0, 0, 0, None)];
        let (mut pre, mut post, mut accepted) = (Vec::new(), Vec::new(), Vec::new());
        let ranges: Vec<std::ops::Range<usize>> = match reset_after {
            Some(at) => vec![0..at, at..blocks],
            None => std::iter::once(0..blocks).collect(),
        };
        let mut sequence = 0u64;
        let mut previous_accepted = 0u64;
        for (index, range) in ranges.into_iter().enumerate() {
            let epoch = index as u64 + 1;
            // After the last block before it was processed.
            let start = 1_000 + range.start as u64 * 10_000 + 7;
            if index == 0 {
                render.push(row(
                    "reset",
                    reset(start - 2, "start", 1, 1, None),
                    0,
                    0,
                    0,
                    None,
                ));
                capture.push(row(
                    "reset",
                    reset(start, "start", 1, 1, None),
                    0,
                    0,
                    0,
                    None,
                ));
            } else {
                let discard = json!({"playback_epoch":epoch-1,"retired_through":previous_accepted-240,"accepted_through":previous_accepted});
                render.push(row(
                    "reset",
                    reset(
                        start - 2,
                        "playback_reset",
                        epoch,
                        epoch,
                        Some(discard.clone()),
                    ),
                    0,
                    0,
                    accepted.len() as u64 * 2,
                    None,
                ));
                capture.push(row(
                    "reset",
                    reset(start, "dsp_reset", epoch, epoch, Some(discard)),
                    0,
                    pre.len() as u64 * 2,
                    0,
                    None,
                ));
            }
            for first in [0, 240] {
                render.push(row("render_accepted",json!({"playback_epoch":epoch,"privacy_generation":2,"first_sample":first,"end_sample":first+240,"accepted_at_us":start-1,"queue_observed_at_us":start-1,"queued_frames":PRIME,"owner":null,"chunk_sequence":null}),240,0,accepted.len() as u64*2,Some("prime")));
                accepted.extend([0i16; 240]);
            }
            let mut ring = Resampler::new();
            ring.push(&[0; LEAD + PRIME]).unwrap();
            let mut controller = Controller::default();
            let mut processor = EchoProcessor::new(false);
            let (mut pushed, mut packets, mut last_at) = (PRIME as u64, 0u64, start - 1);
            for (count, block) in range.enumerate() {
                let at = 1_000 + block as u64 * 10_000 + 10_000;
                loop {
                    // The speaker's clock is 1,000 ppm fast; every seventh
                    // packet waits for the next.
                    let mut due = start + (packets as f64 * 10_000.0 / 1.001) as u64;
                    if packets % 7 == 6 {
                        due += 6_000;
                    }
                    if due > at {
                        break;
                    }
                    let samples: Vec<i16> = (0..RENDER_SAMPLES as u64)
                        .map(|i| {
                            let n = (pushed + i) as f64;
                            let envelope = 0.5 + 0.5 * (n / 3_000.0).sin();
                            (envelope * (6_000.0 * (n * 0.071).sin() + 2_500.0 * (n * 0.233).sin()))
                                as i16
                        })
                        .collect();
                    ring.push(&samples).unwrap();
                    render.push(row("render_accepted",json!({"playback_epoch":epoch,"privacy_generation":2,"first_sample":pushed,"end_sample":pushed+240,"accepted_at_us":due,"queue_observed_at_us":due,"queued_frames":480,"owner":null,"chunk_sequence":null}),240,0,accepted.len() as u64*2,None));
                    accepted.extend(&samples);
                    pushed += 240;
                    packets += 1;
                    last_at = due;
                }
                let now = at + 3;
                let mut taken = [0; RENDER_SAMPLES];
                ring.take(&mut taken).unwrap();
                processor.render(&taken).unwrap();
                let since = now.saturating_sub(last_at).min(20_000);
                let owed = since as f64 * 24_000.0 / 1e6;
                let ppm = controller.observe(ring.unread() as f64 + owed);
                ring.set_ppm(ppm);
                let analysed = ring.consumed().saturating_sub(LEAD as u64);
                let samples: [i16; 160] = std::array::from_fn(|i| {
                    let echo = i32::from(taken[i * 3 / 2]) / 3;
                    (echo + ((i as i32 * 191 + block as i32 * 337) % 600 - 300)) as i16
                });
                post.extend(processor.capture(&samples, 5).unwrap());
                sequence += 1;
                capture.push(row("capture",json!({"epoch":1,"dsp_epoch":epoch,"privacy_generation":2,"frame_sequence":sequence,"first_read_started_at_us":at-8,"last_read_started_at_us":at-8,"read_completed_at_us":at+1,"successful_reads":1,"alsa_status_monotonic_us":at+1,"status_observed_at_us":at+2,"available_frames":0,"delayed_frames":32,"processing_completed_at_us":at+5,"aec_queue_delay_ms":5,"reference":{"playback_epoch":epoch,"accepted_through":pushed,"analysed_through":analysed,"capture_blocks_processed":count+1,"capture_blocks_before_clock_start":0,"accepted_at_us":last_at,"analysed_at_us":now,"queue_observed_at_us":last_at,"queued_frames":480,"clock_started_at_us":start,"ignored_old_epoch_packets":0},"vad_score":0.4}),160,pre.len() as u64*2,0,None));
                pre.extend(samples);
            }
            previous_accepted = pushed;
        }
        let end = 1_000 + blocks as u64 * 10_000 + 20_000;
        capture.push(row(
            "end",
            json!({"at_us":end,"reason":"stopped"}),
            0,
            pre.len() as u64 * 2,
            0,
            None,
        ));
        render.push(row(
            "end",
            json!({"at_us":end,"reason":"stopped"}),
            0,
            0,
            accepted.len() as u64 * 2,
            None,
        ));
        let fixture = Self {
            root,
            run,
            capture,
            render,
            pre,
            post,
            accepted,
            valid: true,
            processing: Some(json!({"aec":"sonora_aec3","noise_suppression":false})),
            session: Some(
                json!({"kind":"run_start","at_us":1,"reference_clock":"capture","echo_reference":"as-given"}),
            ),
            max_seconds: 10,
        };
        fixture.save();
        fixture
    }
    fn output(&self, name: &str) -> PathBuf {
        self.root.join("artifacts").join(name)
    }
    fn save(&self) {
        let mut trace: Vec<Value> = self.session.iter().cloned().collect();
        for (kind, leaf, rows, pre, post, accepted, worker_boot) in [
            (
                "capture",
                "capture-audio",
                &self.capture,
                self.pre.as_slice(),
                self.post.as_slice(),
                &[][..],
                [4u8; 16],
            ),
            (
                "render",
                "render-audio",
                &self.render,
                &[][..],
                &[][..],
                self.accepted.as_slice(),
                [5u8; 16],
            ),
        ] {
            let directory = self.run.join(leaf);
            let mut events = Vec::new();
            for (index, row) in rows.iter().enumerate() {
                let mut row = row.clone();
                row["sequence"] = json!(index + 1);
                if row["kind"] == "end" && !self.valid {
                    row["meta"]["reason"] = json!("fault");
                }
                events.extend(serde_json::to_vec(&row).unwrap());
                events.push(b'\n');
            }
            let values = [
                ("events.jsonl", events),
                ("pre_aec.pcm16le", pcm(pre)),
                ("post_aec.pcm16le", pcm(post)),
                ("render_accepted.pcm16le", pcm(accepted)),
            ];
            for (name, bytes) in &values {
                write(&directory.join(name), bytes);
            }
            let mut pending = json!({"schema_version":1,"boot":BOOT,"stream":kind,"started_at_us":0,"max_seconds":self.max_seconds,"max_bytes":16*1024*1024});
            if kind == "capture"
                && let Some(processing) = &self.processing
            {
                pending["software_processing"] = processing.clone();
            }
            write(
                &directory.join("manifest.pending.json"),
                &serde_json::to_vec(&pending).unwrap(),
            );
            let ended_at = rows.last().unwrap()["meta"]["at_us"].as_u64().unwrap();
            let finalized_at = ended_at + 10;
            let zeros = rows
                .iter()
                .filter(|r| !r["accepted_silence_kind"].is_null())
                .map(|r| r["frames"].as_u64().unwrap())
                .sum::<u64>();
            let mut result = json!({"boot":BOOT,"stream":kind,"started_at_us":0,"finalized_at_us":finalized_at,"valid":self.valid,"faults":if self.valid{0}else{2048},"records_queued":rows.len(),"records_written":rows.len(),"records_rejected":0,"capture_frames":pre.len(),"render_frames":accepted.len(),"accepted_zero_frames":zeros,"pre_aec_sha256":hash(&values[1].1),"post_aec_sha256":hash(&values[2].1),"render_accepted_sha256":hash(&values[3].1),"events_sha256":hash(&values[0].1),"end":{"at_us":ended_at,"reason":if self.valid{"stopped"}else{"fault"}}});
            if kind == "capture"
                && let Some(processing) = &self.processing
            {
                result["software_processing"] = processing.clone();
            }
            let bytes = serde_json::to_vec(&result).unwrap();
            write(&directory.join("result.json"), &bytes);
            let complete = directory.join("complete.json");
            if complete.exists() {
                fs::remove_file(&complete).unwrap();
            }
            if self.valid {
                write(&complete, &bytes);
            }
            let identity = json!({"boot":BOOT,"stream":kind,"started_at_us":0});
            let worker = if kind == "capture" {
                "capture"
            } else {
                "speaker"
            };
            trace.push(json!({"kind":"audio_diagnostics_started","worker":worker,"worker_boot":worker_boot,"details":identity}));
            let mut receipt = json!({"started":identity,"valid":self.valid,"acknowledged":true,"outcome":if self.valid{"complete"}else{"invalid"},"faults":if self.valid{0}else{2048},"completion_marker_published":self.valid,"completion_sha256":hash(&bytes),"finalized_at_us":finalized_at});
            for field in [
                "records_written",
                "records_rejected",
                "capture_frames",
                "render_frames",
                "accepted_zero_frames",
            ] {
                receipt[field] = result[field].clone();
            }
            trace.push(json!({"kind":"audio_diagnostics_certified","worker":worker,"worker_boot":worker_boot,"valid":self.valid,"receipt":receipt}));
        }
        write(&self.run.join("events.jsonl"), &jsonl(&trace));
    }
    fn replay(
        &self,
        name: &str,
        options: ReplayOptions,
    ) -> lamp_audio::Result<replay::ReplayReport> {
        replay::replay(&self.root, &self.run, &self.output(name), options)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn write(path: &Path, bytes: &[u8]) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
}
fn row(kind: &str, meta: Value, frames: u64, pre: u64, render: u64, zero: Option<&str>) -> Value {
    json!({"kind":kind,"sequence":0,"meta":meta,"frames":frames,"accepted_silence_kind":zero,"pre_aec_byte_offset":pre,"post_aec_byte_offset":pre,"render_accepted_byte_offset":render})
}
fn reset(at: u64, reason: &str, dsp: u64, playback: u64, discard: Option<Value>) -> Value {
    json!({"at_us":at,"reason":reason,"capture_epoch":1,"dsp_epoch":dsp,"playback_epoch":playback,"privacy_generation":2,"discarded":discard})
}
fn pcm(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn jsonl(rows: &[Value]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for row in rows {
        bytes.extend(serde_json::to_vec(row).unwrap());
        bytes.push(b'\n');
    }
    bytes
}

#[test]
fn certified_partial_writes_reproduce_recorded_call_order_and_exact_ns_pcm() {
    let fixture = Fixture::new();
    let report = fixture.replay("paired", ReplayOptions::default()).unwrap();
    assert!(report.source_certified);
    assert!(report.replay_complete_for_recorded_capture);
    assert_eq!(report.replayed_capture_frames, 480);
    assert_eq!(report.segments.len(), 1);
    assert_eq!(report.segments[0].aec_ns_mismatched_samples_vs_recorded, 0);
    assert_eq!(report.segments[0].render_blocks, 4);
    assert_eq!(report.segments[0].pre_aec.saturated_samples, 1);
    assert_eq!(report.word_error_rate, None);
    assert_eq!(report.acoustic_quality_verdict, None);
    let wav =
        hound::WavReader::open(fixture.output("paired").join("segment-001-aec_ns.wav")).unwrap();
    assert_eq!(wav.spec().sample_rate, 16000);
    assert_eq!(
        wav.into_samples::<i16>()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap(),
        fixture.post
    );
    let operations = fs::read_to_string(fixture.output("paired").join("operations.jsonl")).unwrap();
    let rows: Vec<Value> = operations
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows[0]["source_meta"]["generation"], 2);
    assert_eq!(rows[0]["source_meta"]["open"], true);
    let order: Vec<_> = rows
        .iter()
        .filter_map(|v| match v["kind"].as_str() {
            Some("render_before_capture") => Some("render"),
            Some("capture_pair") => Some("capture"),
            _ => None,
        })
        .collect();
    assert_eq!(
        order,
        vec![
            "render", "render", "render", "capture", "render", "capture", "capture"
        ]
    );
}
/// A scene stopped while its recording was being written leaves an empty
/// result (lamp-4ace, 2026-10-14, the third of five cut-in scenes): the
/// replay says the recording was not finished.
#[test]
fn an_unfinished_recording_is_named_as_one() {
    let fixture = Fixture::new();
    write(&fixture.run.join("capture-audio").join("result.json"), b"");
    let Err(error) = fixture.replay("unfinished", ReplayOptions::default()) else {
        panic!("an unfinished recording was replayed");
    };
    let error = error.to_string();
    assert!(error.contains("not finished"), "{error}");
}

#[test]
fn the_steady_reference_is_replayed_only_when_asked_for_and_says_so() {
    let fixture = Fixture::new();
    let given = fixture.replay("given", ReplayOptions::default()).unwrap();
    let steady = fixture
        .replay(
            "steady",
            ReplayOptions {
                reference: Some(lamp_audio::Reference::Steady),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(given.segments[0].reference, "as_given");
    assert_eq!(given.segments[0].steady_delay_ms, None);
    assert_eq!(steady.segments[0].reference, "steady");
    assert!(steady.source_certified && steady.replay_complete_for_recorded_capture);
    assert_eq!(
        steady.replayed_capture_frames,
        given.replayed_capture_frames
    );
    let rows = |name: &str| -> Vec<Value> {
        fs::read_to_string(fixture.output(name).join("operations.jsonl"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .filter(|row: &Value| row["kind"] == "capture_pair")
            .collect()
    };
    for row in rows("given") {
        assert!(row["steady_reference"].is_null());
    }
    // Nothing is matched in so short a recording: the reference passes
    // half a millisecond late, and says that it does.
    for row in rows("steady") {
        assert_eq!(row["steady_reference"]["delay_ms"], 0.5);
        assert!(row["steady_reference"]["echo_ms"].is_null());
    }
}

#[test]
fn replay_preserves_alignment_observations_without_using_them_as_delay_hints() {
    let mut fixture = Fixture::new();
    let mut baseline = Vec::new();
    for recorded in [None, Some(112)] {
        if let Some(value) = recorded {
            for row in &mut fixture.capture {
                if row["kind"] == "capture" {
                    row["meta"]["aec_internal_alignment_ms"] = json!(value);
                }
            }
            fixture.save();
        }
        let name = if recorded.is_none() {
            "legacy"
        } else {
            "observed"
        };
        let report = fixture.replay(name, ReplayOptions::default()).unwrap();
        assert!(report.source_certified && report.replay_complete_for_recorded_capture);
        assert_eq!(report.segments[0].aec_ns_mismatched_samples_vs_recorded, 0);
        let operations = fs::read_to_string(fixture.output(name).join("operations.jsonl")).unwrap();
        let rows: Vec<Value> = operations
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|row| row["kind"] == "capture_pair")
            .collect();
        assert_eq!(rows.len(), 3);
        for row in &rows {
            assert_eq!(row["meta"]["aec_queue_delay_ms"], 18);
            assert_eq!(
                row["meta"].get("aec_internal_alignment_ms").is_some(),
                recorded.is_some()
            );
            assert_eq!(
                row["meta"]["aec_internal_alignment_ms"].as_i64(),
                recorded.map(i64::from)
            );
        }
        let observed: Vec<_> = rows
            .iter()
            .map(|row| row["replayed_internal_alignment_ms"].clone())
            .collect();
        assert!(observed.iter().all(|v| v["aec_ns"].is_number()));
        assert!(observed.iter().all(|v| v["aec_only"].is_number()));
        if recorded.is_none() {
            baseline = observed;
        } else {
            assert_eq!(observed, baseline);
        }
    }
}

#[test]
fn replay_preserves_start_request_bounds_without_inventing_legacy_evidence() {
    for requested in [None, Some(20)] {
        let mut fixture = Fixture::new();
        if let Some(requested) = requested {
            for row in &mut fixture.capture {
                if row["kind"] == "capture" {
                    row["meta"]["reference"]["clock_start_requested_at_us"] = json!(requested);
                }
            }
            fixture.save();
        }
        let report = fixture
            .replay("clock-bounds", ReplayOptions::default())
            .unwrap();
        assert!(report.replay_complete_for_recorded_capture);
        assert_eq!(report.segments[0].aec_ns_mismatched_samples_vs_recorded, 0);
        let operations =
            fs::read_to_string(fixture.output("clock-bounds").join("operations.jsonl")).unwrap();
        let captures: Vec<Value> = operations
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|row| row["kind"] == "capture_pair")
            .collect();
        assert_eq!(captures.len(), 3);
        for row in captures {
            let reference = &row["meta"]["reference"];
            assert_eq!(reference["clock_started_at_us"], 21);
            assert_eq!(
                reference
                    .get("clock_start_requested_at_us")
                    .and_then(Value::as_u64),
                requested
            );
            assert_eq!(
                reference.get("clock_start_requested_at_us").is_some(),
                requested.is_some()
            );
        }
    }
}

#[test]
fn fault_prefix_requires_explicit_opt_in_and_missing_mode_is_not_inferred() {
    let mut fixture = Fixture::new();
    fixture.valid = false;
    fixture.processing = None;
    fixture.save();
    assert!(fixture.replay("refused", ReplayOptions::default()).is_err());
    assert!(!fixture.output("refused").exists());
    let report = fixture
        .replay(
            "exploratory",
            ReplayOptions {
                exploratory_prefix: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!report.source_certified);
    assert_eq!(
        report.processing_provenance["kind"],
        "unspecified_in_recording"
    );
    assert_eq!(
        report.source_classification,
        "exploratory_fault_prefix_not_benchmark_evidence"
    );
    let evidence = fixture.root.join("source.json");
    write(
        &evidence,
        &serde_json::to_vec(&json!({"source_id":"a".repeat(64)})).unwrap(),
    );
    let report = fixture
        .replay(
            "historical",
            ReplayOptions {
                exploratory_prefix: true,
                historical_processing: Some(HistoricalProcessing {
                    noise_suppression: true,
                    source_evidence: evidence,
                }),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        report.processing_provenance["kind"],
        "operator_asserted_historical_source_not_runtime_mode_receipt"
    );
    assert!(!report.source_certified);
}
#[test]
fn marker_without_timely_receipt_or_with_timeout_cannot_certify() {
    for timeout in [false, true] {
        let fixture = Fixture::new();
        let path = fixture.run.join("events.jsonl");
        let mut rows: Vec<Value> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        if timeout {
            rows[1]["receipt"]["outcome"] = json!("timed_out");
            rows[1]["receipt"]["acknowledged"] = json!(false);
        } else {
            rows.retain(|v| v["kind"] != "audio_diagnostics_certified");
        }
        write(&path, &jsonl(&rows));
        assert!(fixture.replay("refused", ReplayOptions::default()).is_err());
    }
}
#[test]
fn missing_prime_and_analysis_gaps_stop_without_padding_or_relabeling() {
    for prime in [false, true] {
        let mut fixture = Fixture::new();
        if prime {
            for row in &mut fixture.render {
                if row["accepted_silence_kind"] == "prime" {
                    row["accepted_silence_kind"] = Value::Null;
                }
            }
        } else {
            fixture.capture[3]["meta"]["reference"]["analysed_through"] = json!(1200);
            fixture.capture[3]["meta"]["reference"]["accepted_through"] = json!(1200);
        }
        fixture.save();
        let report = fixture.replay("stopped", ReplayOptions::default()).unwrap();
        assert!(!report.replay_complete_for_recorded_capture);
        assert_eq!(report.replayed_capture_frames, if prime { 0 } else { 160 });
        assert!(
            report
                .limitations
                .iter()
                .any(|s| s.contains("replay stopped"))
        );
    }
}
#[test]
fn changed_hash_privacy_sequence_and_byte_offsets_are_not_exploratory_repairs() {
    for mutation in 0..4 {
        let mut fixture = Fixture::new();
        match mutation {
            0 => {
                fixture.pre[0] = 0;
                write(
                    &fixture.run.join("capture-audio/pre_aec.pcm16le"),
                    &pcm(&fixture.pre),
                );
            }
            1 => {
                fixture.capture[0]["meta"]["open"] = json!(false);
                fixture.save();
            }
            2 => {
                fixture.capture[3]["meta"]["frame_sequence"] = json!(9);
                fixture.save();
            }
            _ => {
                fixture.capture[3]["pre_aec_byte_offset"] = json!(0);
                fixture.save();
            }
        }
        let result = fixture.replay(
            "invalid",
            ReplayOptions {
                exploratory_prefix: true,
                ..Default::default()
            },
        );
        if mutation == 2 {
            let report = result.unwrap();
            assert!(!report.replay_complete_for_recorded_capture);
            assert_eq!(report.replayed_capture_frames, 160);
        } else {
            assert!(result.is_err());
        }
    }
}
#[test]
fn output_is_exclusive_private_and_limited_to_workspace_artifact_roots() {
    let fixture = Fixture::new();
    fixture.replay("one", ReplayOptions::default()).unwrap();
    assert!(fixture.replay("one", ReplayOptions::default()).is_err());
    assert!(
        replay::replay(
            &fixture.root,
            &fixture.run,
            &fixture.root.join("outside"),
            ReplayOptions::default()
        )
        .is_err()
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(fixture.output("one"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(fixture.output("one").join("segment-001-aec_ns.wav"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let input = fixture.run.join("capture-audio/pre_aec.pcm16le");
    let original = fixture.root.join("original");
    fs::rename(&input, &original).unwrap();
    std::os::unix::fs::symlink(&original, &input).unwrap();
    assert!(fixture.replay("symlink", ReplayOptions::default()).is_err());
}

#[test]
fn dsp_reset_preserves_mic_sequence_splits_wavs_and_uses_new_actual_prime() {
    for wrong_sequence in [false, true] {
        let mut fixture = Fixture::new();
        fixture.capture.pop();
        fixture.render.pop();
        let discard = json!({"playback_epoch":1,"retired_through":950,"accepted_through":960});
        fixture.render.push(row(
            "reset",
            reset(95, "playback_reset", 2, 2, Some(discard.clone())),
            0,
            0,
            1920,
            None,
        ));
        for (first, at) in [(0, 96), (240, 97)] {
            fixture.render.push(row("render_accepted",json!({"playback_epoch":2,"privacy_generation":2,"first_sample":first,"end_sample":first+240,"accepted_at_us":at,"queue_observed_at_us":at,"queued_frames":480,"owner":null,"chunk_sequence":null}),240,0,1920+first*2,Some("prime")));
        }
        fixture.accepted.extend([0; 480]);
        fixture.capture.push(row(
            "reset",
            reset(100, "dsp_reset", 2, 2, Some(discard)),
            0,
            960,
            0,
            None,
        ));
        let mut meta = fixture.capture[2]["meta"].clone();
        meta["dsp_epoch"] = json!(2);
        meta["frame_sequence"] = json!(if wrong_sequence { 1 } else { 4 });
        for field in [
            "first_read_started_at_us",
            "last_read_started_at_us",
            "read_completed_at_us",
            "status_observed_at_us",
            "processing_completed_at_us",
            "alsa_status_monotonic_us",
        ] {
            meta[field] = json!(110);
        }
        meta["reference"] = json!({"playback_epoch":2,"accepted_through":480,"analysed_through":480,"capture_blocks_processed":1,"capture_blocks_before_clock_start":1,"accepted_at_us":97,"analysed_at_us":100,"queue_observed_at_us":97,"queued_frames":480,"clock_started_at_us":null,"ignored_old_epoch_packets":0});
        fixture
            .capture
            .push(row("capture", meta, 160, 960, 0, None));
        let samples = [300i16; 160];
        fixture.pre.extend(samples);
        let mut processor = EchoProcessor::new(true);
        processor.render(&[0; 240]).unwrap();
        processor.render(&[0; 240]).unwrap();
        fixture
            .post
            .extend(processor.capture(&samples, 18).unwrap());
        fixture.capture.push(row(
            "end",
            json!({"at_us":120,"reason":"stopped"}),
            0,
            1280,
            0,
            None,
        ));
        fixture.render.push(row(
            "end",
            json!({"at_us":120,"reason":"stopped"}),
            0,
            0,
            2880,
            None,
        ));
        fixture.save();
        let report = fixture.replay("reset", ReplayOptions::default()).unwrap();
        assert_eq!(report.segments.len(), if wrong_sequence { 1 } else { 2 });
        assert_eq!(report.replay_complete_for_recorded_capture, !wrong_sequence);
        if !wrong_sequence {
            assert_eq!(report.segments[1].aec_ns_mismatched_samples_vs_recorded, 0);
            assert_eq!(report.segments[1].capture_blocks, 1);
            assert_eq!(report.segments[1].render_blocks, 2);
            assert!(
                fixture
                    .output("reset")
                    .join("segment-002-aec_only.wav")
                    .is_file()
            );
        }
    }
}

#[test]
fn oversized_input_metadata_and_inconsistent_processing_provenance_are_refused() {
    for mutation in 0..4 {
        let mut fixture = Fixture::new();
        match mutation {
            0 => {
                let file = fs::OpenOptions::new()
                    .write(true)
                    .open(fixture.run.join("capture-audio/pre_aec.pcm16le"))
                    .unwrap();
                file.set_len(160 * 1024 * 1024 + 1).unwrap();
            }
            1 => {
                fixture.capture[2]["meta"]["oversized"] = json!("x".repeat(4096));
                fixture.save();
            }
            2 => {
                let path = fixture.run.join("capture-audio/manifest.pending.json");
                let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                value["max_seconds"] = json!(601);
                write(&path, &serde_json::to_vec(&value).unwrap());
            }
            _ => {
                let path = fixture.run.join("capture-audio/manifest.pending.json");
                let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                value["software_processing"]["noise_suppression"] = json!(false);
                write(&path, &serde_json::to_vec(&value).unwrap());
            }
        }
        assert!(
            fixture
                .replay(
                    "refused",
                    ReplayOptions {
                        exploratory_prefix: true,
                        ..Default::default()
                    }
                )
                .is_err()
        );
    }
}

#[test]
fn missing_or_zero_worker_identity_cannot_certify_matching_markers() {
    for missing in [false, true] {
        let fixture = Fixture::new();
        let path = fixture.run.join("events.jsonl");
        let mut rows: Vec<Value> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        for row in &mut rows {
            if missing {
                row.as_object_mut().unwrap().remove("worker_boot");
            } else {
                row["worker_boot"] = json!(vec![0u8; 16]);
            }
        }
        write(&path, &jsonl(&rows));
        assert!(fixture.replay("refused", ReplayOptions::default()).is_err());
    }
}

#[test]
fn a_capture_clocked_recording_replays_whole_and_bit_for_bit() {
    let fixture = Fixture::capture_clocked(300, None);
    let report = fixture.replay("clocked", ReplayOptions::default()).unwrap();
    assert!(report.source_certified, "{:?}", report.limitations);
    assert!(
        report.replay_complete_for_recorded_capture,
        "{:?}",
        report.limitations
    );
    assert_eq!(report.reference_clock, "capture");
    assert_eq!(report.reference_clock_from, "events.jsonl run_start");
    assert_eq!(report.echo_reference_recorded.as_deref(), Some("as-given"));
    assert_eq!(report.replayed_capture_frames, 300 * 160);
    assert_eq!(report.segments.len(), 1);
    // The canceller's own output, sample for sample.
    assert_eq!(
        report.segments[0].aec_only_mismatched_samples_vs_recorded,
        0
    );
    assert_eq!(report.segments[0].reference, "as_given");
    // The rate moved: the ring was read off the sample grid.
    let operations =
        fs::read_to_string(fixture.output("clocked").join("operations.jsonl")).unwrap();
    let rates: Vec<f64> = operations
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|row| row["kind"] == "render_for_capture")
        .map(|row| row["rate_ppm"].as_f64().unwrap())
        .collect();
    assert_eq!(rates.len(), 300);
    assert!(rates.iter().any(|&rate| rate != 0.0));
}

#[test]
fn an_older_capture_clocked_session_is_known_by_its_first_cursor() {
    let mut fixture = Fixture::capture_clocked(60, None);
    fixture.session = None;
    fixture.save();
    let report = fixture.replay("older", ReplayOptions::default()).unwrap();
    assert_eq!(report.reference_clock, "capture");
    assert_eq!(
        report.reference_clock_from,
        "the first capture record's cursor"
    );
    assert_eq!(report.echo_reference_recorded, None);
    assert!(report.replay_complete_for_recorded_capture);
    assert_eq!(
        report.segments[0].aec_only_mismatched_samples_vs_recorded,
        0
    );
}

#[test]
fn a_reset_in_the_middle_of_a_capture_clocked_recording_starts_a_new_ring() {
    let fixture = Fixture::capture_clocked(200, Some(120));
    let report = fixture.replay("reset", ReplayOptions::default()).unwrap();
    assert!(
        report.replay_complete_for_recorded_capture,
        "{:?}",
        report.limitations
    );
    assert_eq!(report.segments.len(), 2);
    assert_eq!(
        (
            report.segments[0].capture_blocks,
            report.segments[1].capture_blocks
        ),
        (120, 80)
    );
    for segment in &report.segments {
        assert_eq!(segment.aec_only_mismatched_samples_vs_recorded, 0);
    }
}

#[test]
fn events_cut_off_in_their_last_line_are_read_up_to_it() {
    let fixture = Fixture::capture_clocked(40, None);
    let path = fixture.run.join("events.jsonl");
    let mut events = fs::read(&path).unwrap();
    events.extend_from_slice(br#"{"kind":"provider_event","at_us":99,"det"#);
    write(&path, &events);
    let report = fixture.replay("cut", ReplayOptions::default()).unwrap();
    assert!(report.events_cut_off);
    assert!(report.source_certified);
    assert!(report.replay_complete_for_recorded_capture);
    assert!(report.limitations.iter().any(|l| l.contains("cut off")));
    // A line that is not JSON anywhere else is still refused, by number.
    let mut broken = fs::read(&path).unwrap();
    broken.splice(0..0, b"not json\n".iter().copied());
    write(&path, &broken);
    let Err(error) = fixture.replay("broken", ReplayOptions::default()) else {
        panic!("a line that is not JSON was read");
    };
    let error = error.to_string();
    assert!(error.contains("events.jsonl line 1"), "{error}");
}

#[test]
fn where_a_recording_cannot_be_followed_the_replay_names_the_record_and_keeps_what_came_before() {
    let mut fixture = Fixture::capture_clocked(100, None);
    // The lamp's cursor after the 61st block, one frame on: no ring reads
    // that from what was accepted.
    let index = fixture
        .capture
        .iter()
        .position(|row| row["kind"] == "capture" && row["meta"]["frame_sequence"] == 61)
        .unwrap();
    let analysed = fixture.capture[index]["meta"]["reference"]["analysed_through"]
        .as_u64()
        .unwrap();
    fixture.capture[index]["meta"]["reference"]["analysed_through"] = json!(analysed + 1);
    fixture.save();
    let report = fixture
        .replay(
            "parted",
            ReplayOptions {
                exploratory_prefix: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!report.replay_complete_for_recorded_capture);
    assert_eq!(report.replayed_capture_frames, 60 * 160);
    assert_eq!(
        report.segments[0].aec_only_mismatched_samples_vs_recorded,
        0
    );
    let record = index + 1;
    assert!(
        report
            .limitations
            .iter()
            .any(|l| l.contains(&format!("capture record {record}")) && l.contains("read to frame")),
        "{:?}",
        report.limitations
    );
}

#[test]
fn the_linear_output_is_written_only_when_asked_for_and_changes_no_other_output() {
    let fixture = Fixture::new();
    let plain = fixture.replay("plain", ReplayOptions::default()).unwrap();
    let linear = fixture
        .replay(
            "linear",
            ReplayOptions {
                linear_output: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(linear.source_certified && linear.replay_complete_for_recorded_capture);
    let name = "segment-001-aec_linear.wav";
    assert!(!fixture.output("plain").join(name).exists());
    assert!(plain.segments[0].aec_linear.is_none());
    let report: Value =
        serde_json::from_slice(&fs::read(fixture.output("plain").join("report.json")).unwrap())
            .unwrap();
    assert!(report["segments"][0].get("aec_linear").is_none());
    let mut files = linear.segments[0].wav_pcm_sha256.clone();
    assert!(files.remove(name).is_some());
    assert_eq!(files, plain.segments[0].wav_pcm_sha256);
    let wav = hound::WavReader::open(fixture.output("linear").join(name)).unwrap();
    assert_eq!(wav.spec().sample_rate, 16000);
    assert_eq!(u64::from(wav.len()), linear.replayed_capture_frames);
    assert_eq!(
        linear.segments[0].aec_linear.as_ref().map(|s| s.frames),
        Some(480)
    );
}
