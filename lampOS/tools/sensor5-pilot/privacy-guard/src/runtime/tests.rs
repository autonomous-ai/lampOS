use super::*;
use lamp_bus::{Header, Kind};
use lamp_camera::frames::{CloseReason, FrameServer};
use lamp_msgs::ClockStamp;
use lamp_turn_owner::{BootId, Controller, MonoTime, Permission};
use std::{cell::Cell, os::unix::fs::DirBuilderExt, sync::atomic::AtomicU64};

static NEXT: AtomicU64 = AtomicU64::new(0);
const CLOCK: &str = "synthetic-runtime-clock";
const JPEG: &[u8] = &[0xff, 0xd8, 1, 2, 3, 4, 0xff, 0xd9];

fn lease(sequence: u64, at_us: u64, permitted: bool) -> Message {
    let value = MicrophonePrivacy {
        stamp: ClockStamp {
            clock_id: CLOCK.into(),
            at_us,
            sequence,
        },
        session_id: "1111111111111111".into(),
        controller_boot: "22222222222222222222222222222222".into(),
        microphone_generation: 1,
        observed_us: at_us,
        expires_at_us: at_us + 50_000,
        permitted,
        physical_muted: Some(!permitted),
        no_slide: false,
    };
    Message {
        header: Header {
            topic: MicrophonePrivacy::TOPIC.into(),
            kind: Kind::Latest,
            enc: "json".into(),
            type_name: Some(MicrophonePrivacy::TYPE.into()),
            source: "voice".into(),
            boot: value.session_id.clone(),
            seq: sequence,
            at_us,
            at_unix_ms: 1_000,
            expires_ms: Some(50),
            clock: None,
        },
        payload: serde_json::to_vec(&value).unwrap(),
        gap: 0,
        publisher_restarted: false,
        expired: false,
    }
}

fn live() -> Admission {
    let mut admission = Admission::new(CLOCK.into(), 0);
    admission
        .observe(&lease(1, 1_000, true), 1_000, 1_000)
        .unwrap();
    admission
}

fn workers() -> (Workers, SyncSender<Message>, SyncSender<Message>) {
    let (privacy_tx, privacy) = mpsc::sync_channel(16);
    let (audio_tx, audio) = mpsc::sync_channel(32);
    (
        Workers {
            stop: Arc::new(AtomicBool::new(false)),
            failed: Arc::new(AtomicBool::new(false)),
            privacy,
            audio,
        },
        privacy_tx,
        audio_tx,
    )
}

struct Wire {
    server: FrameServer,
    camera: CameraInput,
    directory: PathBuf,
}

impl Wire {
    fn new() -> Self {
        let directory = PathBuf::from(format!(
            "/private/tmp/s5f-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        consent::create_private(&directory.join("README.txt"),b"Retained synthetic actual-FrameClient regression. No devices or people. Source JPEG is a protocol-envelope fixture, not a decodable photograph.\n").unwrap();
        consent::create_private(&directory.join("source.jpg"), JPEG).unwrap();
        let controller = BootId::new([1; 16]).unwrap();
        let reader = BootId::new([3; 16]).unwrap();
        let socket = directory.join("frames");
        let mut server = FrameServer::bind(
            &socket,
            controller,
            &[(Role::Perception, reader)],
            MAX_FRAME_BYTES,
        )
        .unwrap();
        let mut camera = CameraInput::new(FrameClient::new(
            &socket,
            Role::Perception,
            reader,
            controller,
            MAX_FRAME_BYTES,
        ));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !camera.client.connect().unwrap() || !server.connected(Role::Perception) {
            assert!(Instant::now() < deadline);
            server.service(10_000);
            std::thread::sleep(Duration::from_millis(1));
        }
        Self {
            server,
            camera,
            directory,
        }
    }

    fn offer(&mut self) {
        self.camera.request_if_idle().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !self.server.wants_frame() {
            assert!(Instant::now() < deadline);
            self.server.service(10_000);
            std::thread::sleep(Duration::from_millis(1));
        }
        let mut owner = Controller::new(BootId::new([1; 16]).unwrap(), MonoTime::from_micros(1));
        owner
            .set_camera_permission(MonoTime::from_micros(1), Permission::Allowed)
            .unwrap();
        let meta = FrameMeta {
            request: 0,
            camera_worker: BootId::new([2; 16]).unwrap(),
            capture_epoch: 1,
            frame_sequence: 7,
            grant: owner.camera_grant(MonoTime::from_micros(1)).unwrap(),
            source: "lamp-eyes".into(),
            format: *b"MJPG",
            width: 16,
            height: 16,
            driver_sequence: 7,
            driver_timestamp: None,
            dequeue_started_at_us: 8_900,
            dequeue_completed_at_us: 9_000,
            copied_at_us: 10_000,
            derived: None,
        };
        consent::create_private(
            &self.directory.join("source-frame.json"),
            &serde_json::to_vec_pretty(&meta).unwrap(),
        )
        .unwrap();
        assert_eq!(self.server.offer_meta(&meta, JPEG, 10_000).unwrap(), 1);
        self.server.service(10_000);
    }

    fn outcome(&self, value: serde_json::Value) {
        consent::create_private(
            &self.directory.join("outcome.json"),
            &serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
        eprintln!(
            "retained frame-fence regression: {}",
            self.directory.display()
        );
    }
}

#[test]
fn camera_close_during_encoding_rejects_the_actual_archive_enqueue_path() {
    let mut wire = Wire::new();
    wire.offer();
    wire.camera.probe().unwrap();
    let (meta, bytes) = wire.camera.take().unwrap();
    let mut admission = live();
    let header = admission.frame(&meta, &bytes, 11_000).unwrap();
    let (workers, _privacy, _audio) = workers();
    let enqueued = Cell::new(false);
    let result = admit_packet_with(
        &mut admission,
        &workers,
        &mut wire.camera,
        PreparedMedia {
            packet: Packet { header, bytes },
            source_completed_us: meta.dequeue_completed_at_us,
            source_permit: None,
        },
        &|| Time {
            monotonic_us: 11_000,
            unix_ms: 1_000,
        },
        |packet| {
            wire.server.close_all(CloseReason::PrivacyClosed, 11_000);
            packet.encode()
        },
        |_| {
            enqueued.set(true);
            Ok(())
        },
    );
    assert!(result.is_err());
    assert!(!enqueued.get());
    assert!(!wire.camera.client.is_connected());
    wire.outcome(json!({"camera_closed_during_encode":true,"enqueued":false,"error":result.unwrap_err().to_string()}));
}

#[test]
fn status_probe_preserves_a_legitimate_requested_frame_for_the_normal_consumer() {
    let mut wire = Wire::new();
    wire.offer();
    let mut admission = live();
    let (workers, _privacy, _audio) = workers();
    live_fence(&mut admission, &workers, &mut wire.camera, &|| Time {
        monotonic_us: 11_000,
        unix_ms: 1_000,
    })
    .unwrap();
    wire.camera.request_if_idle().unwrap();
    assert!(wire.camera.client.outstanding().is_none());
    live_fence(&mut admission, &workers, &mut wire.camera, &|| Time {
        monotonic_us: 11_001,
        unix_ms: 1_000,
    })
    .unwrap();
    let (meta, bytes) = wire
        .camera
        .take()
        .expect("status probes retain pending frame");
    assert_eq!(meta.frame_sequence, 7);
    assert_eq!(bytes, JPEG);
    assert!(wire.camera.take().is_none());
    wire.outcome(json!({"pending_frame_preserved":true,"native_sequence":meta.frame_sequence,"exact_jpeg_bytes":bytes==JPEG}));
}

#[test]
fn observed_failures_at_the_deadline_win_over_duration_success() {
    for cause in [
        "observer",
        "privacy",
        "camera",
        "source_eof",
        "expired",
        "healthy",
    ] {
        let mut wire = Wire::new();
        let mut admission = live();
        let (workers, privacy, _audio) = workers();
        match cause {
            "observer" => workers.failed.store(true, Ordering::Release),
            "privacy" => privacy.send(lease(2, 10_000, false)).unwrap(),
            "camera" => wire.server.close_all(CloseReason::PrivacyClosed, 11_000),
            "source_eof" => drop(privacy),
            _ => {}
        }
        let now = if cause == "expired" { 51_000 } else { 11_000 };
        let result = duration_complete(
            &mut admission,
            &workers,
            &mut wire.camera,
            1_000,
            10_000,
            (1, 1),
            &|| Time {
                monotonic_us: now,
                unix_ms: 1_000,
            },
        );
        if cause == "healthy" {
            assert!(result.unwrap());
            wire.outcome(json!({"cause":cause,"duration_elapsed":true}));
        } else {
            assert!(result.is_err(), "{cause}");
            wire.outcome(json!({"cause":cause,"duration_elapsed":false,"error":result.unwrap_err().to_string()}));
        }
    }
}

#[test]
fn original_audio_ticket_survives_encoding_and_a_newer_voice_lease() {
    let mut wire = Wire::new();
    let mut admission = live();
    let (workers, privacy, _audio) = workers();
    let original = AudioCapturePermit {
        clock_id: CLOCK.into(),
        epoch: 42,
        until_us: 12_000,
    };
    let audio = Audio {
        sample_rate: 16_000,
        channels: 1,
        sample_format: Audio::S16LE.into(),
        sequence: 0,
    };
    let message = Message {
        header: Header {
            topic: "mic1/audio".into(),
            kind: Kind::Event,
            enc: Audio::ENCODING.into(),
            type_name: Some(Audio::TYPE.into()),
            source: "ears".into(),
            boot: "3333333333333333".into(),
            seq: 1,
            at_us: 9_000,
            at_unix_ms: 1_000,
            expires_ms: Some(50),
            clock: None,
        },
        payload: audio
            .block_with_capture_permit(&[0; 640], &original)
            .unwrap(),
        gap: 0,
        publisher_restarted: false,
        expired: false,
    };
    consent::create_private(
        &wire.directory.join("source-audio-payload.bin"),
        &message.payload,
    )
    .unwrap();
    let header = admission.audio(&message, CLOCK, 11_000).unwrap();
    let (_, permit, samples) = Audio::split_with_capture_permit(&message.payload).unwrap();
    let now = Cell::new(11_000);
    let enqueued = Cell::new(false);
    let result = admit_packet_with(
        &mut admission,
        &workers,
        &mut wire.camera,
        PreparedMedia {
            packet: Packet {
                header,
                bytes: samples.to_vec(),
            },
            source_completed_us: 9_000,
            source_permit: permit,
        },
        &|| Time {
            monotonic_us: now.get(),
            unix_ms: 1_000,
        },
        |packet| {
            privacy.send(lease(2, 12_000, true)).unwrap();
            now.set(13_000);
            packet.encode()
        },
        |_| {
            enqueued.set(true);
            Ok(())
        },
    );
    assert!(result.is_err());
    assert!(!enqueued.get());
    wire.outcome(
        json!({"source_original_until_us":12_000,"final_now_us":13_000,"new_voice_until_us":62_000,
        "enqueued":false,"error":result.unwrap_err().to_string()}),
    );
}
