use lamp_sensor5_recorder::{
    archive::{Archive, Packet, STOP_BUDGET},
    consent,
};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn directory(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "sensor5-recorder-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    consent::new_directory(&path).unwrap();
    path
}

fn start(output: &Path) -> Archive {
    let mut archive = Archive::spawn(
        Path::new(env!("CARGO_BIN_EXE_lamp-sensor5-recorder")),
        output,
    )
    .unwrap();
    let packet = Packet {
        header: json!({"kind":"start","session":{"schema_version":1,"synthetic":true,"clock_id":"synthetic-clock","physical_sync_validated":false},
        "consent_record":"{\"synthetic_test_only\":true}\n","consent_text":"Synthetic fixture; no person recorded.\n"}),
        bytes: vec![],
    };
    archive.enqueue(packet.encode().unwrap()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !archive.ready {
        archive.poll().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    archive
}

fn flush(archive: &mut Archive, ack: u64) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while archive.acknowledged < ack {
        archive.poll().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn packet() -> Packet {
    Packet {
        header: json!({"kind":"frame","frame":{"frame_sequence":7,"dequeue_completed_at_us":123_456}}),
        bytes: vec![0xff, 0xd8, 1, 2, 0xff, 0xd9],
    }
}

#[test]
fn exclusive_private_archive_keeps_original_bytes_metadata_and_hashes() {
    let parent = directory("retained");
    let output = parent.join("clip");
    let mut archive = start(&output);
    archive.enqueue(packet().encode().unwrap()).unwrap();
    flush(&mut archive, 1);
    archive
        .stop(json!({"reason":"duration_elapsed","frames_admitted":1,"audio_blocks_admitted":0}))
        .unwrap();
    let raw = fs::read(output.join("records/00000001.frame.jpg")).unwrap();
    assert_eq!(raw, packet().bytes);
    let sidecar: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("records/00000001.frame.jpg.json")).unwrap())
            .unwrap();
    assert_eq!(sidecar["sha256"], consent::sha256(&raw));
    assert_eq!(sidecar["frame"]["dequeue_completed_at_us"], 123_456);
    assert_eq!(fs::metadata(&output).unwrap().mode() & 0o777, 0o700);
    assert_eq!(
        fs::metadata(output.join("records/00000001.frame.jpg"))
            .unwrap()
            .mode()
            & 0o777,
        0o600
    );
    assert!(consent::new_directory(&output).is_err());
    eprintln!("retained synthetic evidence: {}", output.display());
}

#[test]
fn blocked_archive_queue_fails_bounded_and_reaps_child_without_deleting_evidence() {
    let parent = directory("blocked");
    let output = parent.join("clip");
    let mut archive = start(&output);
    archive.enqueue(packet().encode().unwrap()).unwrap();
    flush(&mut archive, 1);
    let pid = rustix::process::Pid::from_raw(archive.pid() as i32).unwrap();
    rustix::process::kill_process(pid, rustix::process::Signal::STOP).unwrap();
    let big = || {
        Packet {
            header: json!({"kind":"frame"}),
            bytes: vec![0; 2 * 1024 * 1024],
        }
        .encode()
        .unwrap()
    };
    archive.enqueue(big()).unwrap();
    archive.enqueue(big()).unwrap();
    assert!(archive.enqueue(big()).is_err());
    let now = Instant::now();
    assert!(archive.stop(json!({"reason":"privacy_revoked"})).is_err());
    assert!(now.elapsed() < STOP_BUDGET + Duration::from_millis(400));
    assert!(
        rustix::process::test_kill_process(pid).is_err(),
        "child must be reaped"
    );
    assert!(output.join("records/00000001.frame.jpg").is_file());
    assert!(!output.join("result.json").exists());
}

#[test]
fn stop_retains_only_already_admitted_packets_and_marks_failure() {
    let parent = directory("revoked");
    let output = parent.join("clip");
    let mut archive = start(&output);
    archive.enqueue(packet().encode().unwrap()).unwrap();
    flush(&mut archive, 1);
    archive.enqueue(packet().encode().unwrap()).unwrap();
    let stopped = archive.stop(json!({"reason":"privacy_revoked","frames_admitted":2}));
    if let Ok(bytes) = fs::read(output.join("result.json")) {
        if let Ok(result) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            assert_eq!(result["complete"], false);
        } else {
            assert!(stopped.is_err(), "partial terminal cannot claim success");
        }
    } else {
        assert!(stopped.is_err(), "missing terminal cannot claim success");
    }
    assert!(output.join("records/00000001.frame.jpg").is_file());
}

#[test]
#[ignore = "subprocess helper used only by orphan test"]
fn orphan_parent_helper() {
    let Some(parent) = std::env::var_os("SENSOR5_ORPHAN_TEST") else {
        return;
    };
    let parent = PathBuf::from(parent);
    let mut archive = start(&parent.join("clip"));
    archive.enqueue(packet().encode().unwrap()).unwrap();
    flush(&mut archive, 1);
    fs::write(parent.join("child.pid"), archive.pid().to_string()).unwrap();
    std::mem::forget(archive);
    std::process::exit(0);
}

#[test]
fn archive_child_dies_when_parent_exits_even_with_open_output() {
    let parent = directory("orphan");
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "orphan_parent_helper",
            "--nocapture",
        ])
        .env("SENSOR5_ORPHAN_TEST", &parent)
        .status()
        .unwrap();
    assert!(status.success());
    let pid = rustix::process::Pid::from_raw(
        fs::read_to_string(parent.join("child.pid"))
            .unwrap()
            .parse()
            .unwrap(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while rustix::process::test_kill_process(pid).is_ok() {
        assert!(Instant::now() < deadline, "orphan archive child survived");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(parent.join("clip/records/00000001.frame.jpg").exists());
}

#[test]
fn retained_synthetic_converter_fixture() {
    let parent = directory("fixture");
    let output = parent.join("clip");
    let mut archive = start(&output);
    let mut jpeg = vec![];
    jpeg_encoder::Encoder::new(&mut jpeg, 90)
        .encode(&[80; 16 * 16 * 3], 16, 16, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    let mut index = 0;
    for i in 0..10u64 {
        let end = 1_000_000 + i * 40_000;
        let meta = json!({"request":i+1,"camera_worker":[2,2,2,2,2,2,2,2,2,2,2,2,2,2,2,2],"capture_epoch":1,"frame_sequence":i+1,
            "grant":{"boot":[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1],"generation":1},"source":"lamp-eyes","format":[77,74,80,71],"width":16,"height":16,
            "driver_sequence":i,"driver_timestamp":{"micros":end-1000,"domain":"host_monotonic","point":"end_of_frame"},
            "dequeue_started_at_us":end-100,"dequeue_completed_at_us":end,"copied_at_us":end+10,"derived":null});
        archive.enqueue(Packet{header:json!({"kind":"frame","frame":meta,"received_us":end+100,"source_frames_between":0}),bytes:jpeg.clone()}.encode().unwrap()).unwrap();
        index += 1;
        for j in 0..2u64 {
            let sequence = i * 2 + j;
            let complete = end + j * 20_000;
            archive.enqueue(Packet{header:json!({"kind":"audio","header":{"topic":"mic1/audio","kind":"event","enc":"json+pcm","type":"lamp_msgs/Audio","source":"ears","boot":"3333333333333333","seq":sequence+1,"at_us":complete,"at_unix_ms":1_000,"expires_ms":50},
                "audio":{"sample_rate":16_000,"channels":1,"sample_format":"S16LE","sequence":sequence},"received_us":complete+100,
                "bus_gap":0,"audio_sequence_gap":0,"timestamp_boundary":"ears_capture_read_completed","adc_timestamp":false,"clock_provenance":"existing_ears_privacy_runtime_source"}),bytes:vec![0;640]}.encode().unwrap()).unwrap();
            index += 1;
        }
        flush(&mut archive, index);
    }
    archive.stop(json!({"reason":"duration_elapsed","frames_admitted":10,"audio_blocks_admitted":20,"physical_sync_validated":false})).unwrap();
    eprintln!("SYNTHETIC_CONVERTER_FIXTURE={}", output.display());
}
