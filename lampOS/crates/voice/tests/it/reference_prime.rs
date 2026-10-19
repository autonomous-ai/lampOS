use lamp_audio::{EchoProcessor, ownership::QueuedRange};
use lamp_ipc::monotonic_us;
use lamp_voice::{
    process::{SessionDirectory, new_boot},
    reference::{PRIME_FRAMES, PrimeBarrier, ReferenceAssembly},
    transport::ReferenceChannels,
    wire::{ClockPrimed, PreparedPrime, PrimeDisposition, ReferenceControl, ReferenceFaultKind},
};

struct Handshake {
    _directory: SessionDirectory,
    capture: ReferenceChannels,
    speaker: ReferenceChannels,
    assembly: ReferenceAssembly,
    echo: EchoProcessor,
    barrier: PrimeBarrier,
}
impl Handshake {
    fn new(capture_clock: bool) -> Self {
        let directory = SessionDirectory::create().unwrap();
        let capture_boot = new_boot().unwrap();
        let speaker_boot = new_boot().unwrap();
        let mut capture =
            ReferenceChannels::bind(&directory.path, "capture", capture_boot, speaker_boot)
                .unwrap();
        let mut speaker =
            ReferenceChannels::bind(&directory.path, "speaker", speaker_boot, capture_boot)
                .unwrap();
        assert!(speaker.connect_step().unwrap());
        assert!(capture.connect_step().unwrap());
        Self {
            _directory: directory,
            capture,
            speaker,
            assembly: if capture_clock {
                ReferenceAssembly::capture_clocked()
            } else {
                ReferenceAssembly::default()
            },
            echo: EchoProcessor::new(false),
            barrier: PrimeBarrier::default(),
        }
    }

    fn prepare(&mut self, epoch: u64, discarded: Option<QueuedRange>) -> PreparedPrime {
        let now = monotonic_us();
        let prime = ClockPrimed {
            playback_epoch: epoch,
            privacy_generation: 2,
            accepted_zero_frames: PRIME_FRAMES,
            accepted_at_us: now,
            queue_observed_at_us: now,
            queued_frames: PRIME_FRAMES,
            discarded,
        };
        self.speaker
            .control
            .send(ReferenceControl::ClockPrimed { prime })
            .unwrap();
        self.barrier.sent(prime, now).unwrap();
        let Some(ReferenceControl::ClockPrimed { prime }) = self.capture.control.receive().unwrap()
        else {
            panic!("missing speaker prime")
        };
        let prepared = self
            .assembly
            .prime(prime, monotonic_us(), 2, &mut self.echo)
            .unwrap();
        assert_eq!(self.assembly.timing().clock_started_at_us, None);
        self.capture
            .control
            .send(ReferenceControl::ReferencePrimed { prepared })
            .unwrap();
        let Some(ReferenceControl::ReferencePrimed { prepared }) =
            self.speaker.control.receive().unwrap()
        else {
            panic!("missing capture preparation")
        };
        prepared
    }

    fn start(&mut self, prepared: PreparedPrime) {
        let epoch = self
            .barrier
            .acknowledge(prepared, queued(prepared.playback_epoch), 2, monotonic_us())
            .unwrap();
        // These are the production worker's exact preparation/validation seams.
        // No real ALSA call or physical start is claimed by this test.
        let now = monotonic_us();
        self.speaker
            .control
            .send(ReferenceControl::ClockStarted {
                playback_epoch: epoch,
                start_requested_at_us: now,
                started_at_us: now,
            })
            .unwrap();
        let Some(ReferenceControl::ClockStarted {
            playback_epoch,
            start_requested_at_us,
            started_at_us,
        }) = self.capture.control.receive().unwrap()
        else {
            panic!("missing checked start")
        };
        self.assembly
            .started(
                playback_epoch,
                start_requested_at_us,
                started_at_us,
                monotonic_us(),
            )
            .unwrap();
        assert!(!self.barrier.is_waiting());
    }
}
fn queued(epoch: u64) -> QueuedRange {
    QueuedRange {
        epoch,
        retired_through: 0,
        accepted_through: PRIME_FRAMES as u64,
    }
}

#[test]
fn fixed_prime_handshake_and_reset_keep_analysis_truthful() {
    exercise_mode(false);
}
#[test]
fn capture_clock_prime_handshake_and_reset_keep_analysis_truthful() {
    exercise_mode(true);
}
fn exercise_mode(capture_clock: bool) {
    let mut path = Handshake::new(capture_clock);
    for epoch in 1..=2 {
        let prepared = path.prepare(epoch, (epoch > 1).then(|| queued(epoch - 1)));
        assert_eq!(prepared.prepared_through, 480);
        let expected_analysis = if capture_clock { 0 } else { 480 };
        assert_eq!(prepared.analysed_through, expected_analysis);
        assert_eq!(path.assembly.timing().analysed_through, expected_analysis);
        assert_eq!(path.assembly.timing().capture_blocks_processed, 0);
        path.start(prepared);
        assert_eq!(path.assembly.timing().analysed_through, expected_analysis);
        assert_eq!(
            path.barrier
                .acknowledge(prepared, queued(epoch), 2, monotonic_us()),
            Err(ReferenceFaultKind::InvalidStart)
        );
        path.barrier.reset();
    }
}

fn armed() -> (PrimeBarrier, PreparedPrime) {
    let mut barrier = PrimeBarrier::default();
    barrier
        .sent(
            ClockPrimed {
                playback_epoch: 1,
                privacy_generation: 2,
                accepted_zero_frames: 480,
                accepted_at_us: 10_000,
                queue_observed_at_us: 10_001,
                queued_frames: 480,
                discarded: None,
            },
            9_000,
        )
        .unwrap();
    (
        barrier,
        PreparedPrime {
            playback_epoch: 1,
            privacy_generation: 2,
            prepared_through: 480,
            analysed_through: 480,
            disposition: PrimeDisposition::Analysed,
        },
    )
}

#[test]
fn malformed_or_wrong_owner_ack_cannot_consume_pending_prime() {
    let (_, original) = armed();
    for bad in [
        PreparedPrime {
            playback_epoch: 0,
            ..original
        },
        PreparedPrime {
            playback_epoch: 2,
            ..original
        },
        PreparedPrime {
            privacy_generation: 0,
            ..original
        },
        PreparedPrime {
            privacy_generation: 3,
            ..original
        },
        PreparedPrime {
            prepared_through: 479,
            ..original
        },
        PreparedPrime {
            prepared_through: 481,
            ..original
        },
        PreparedPrime {
            analysed_through: 0,
            ..original
        },
        PreparedPrime {
            analysed_through: 481,
            ..original
        },
        PreparedPrime {
            disposition: PrimeDisposition::BufferedForCapture,
            ..original
        },
    ] {
        let (mut barrier, _) = armed();
        assert!(
            barrier.acknowledge(bad, queued(1), 2, 10_002).is_err(),
            "{bad:?}"
        );
        assert!(barrier.is_waiting());
    }
}

#[test]
fn changed_hardware_cursor_privacy_deadline_and_reset_reject_ack() {
    for current in [
        queued(0),
        queued(2),
        QueuedRange {
            accepted_through: 479,
            ..queued(1)
        },
        QueuedRange {
            accepted_through: 481,
            ..queued(1)
        },
        QueuedRange {
            retired_through: 1,
            ..queued(1)
        },
    ] {
        let (mut barrier, prepared) = armed();
        assert_eq!(
            barrier.acknowledge(prepared, current, 2, 10_002),
            Err(ReferenceFaultKind::InvalidStart)
        );
    }
    for privacy in [0, 3] {
        let (mut barrier, prepared) = armed();
        assert_eq!(
            barrier.acknowledge(prepared, queued(1), privacy, 10_002),
            Err(ReferenceFaultKind::PrivacyMismatch)
        );
    }
    for now in [8_999, 109_000] {
        let (mut barrier, prepared) = armed();
        assert_eq!(
            barrier.acknowledge(prepared, queued(1), 2, now),
            Err(ReferenceFaultKind::BarrierTimeout)
        );
    }
    let (mut barrier, prepared) = armed();
    barrier.reset();
    assert_eq!(
        barrier.acknowledge(prepared, queued(1), 2, 10_002),
        Err(ReferenceFaultKind::InvalidStart)
    );
}

#[test]
fn reset_requires_discard_evidence_and_old_ack_never_starts_new_epoch() {
    for capture_clock in [false, true] {
        let mut path = Handshake::new(capture_clock);
        let old = path.prepare(1, None);
        path.start(old);
        let now = monotonic_us();
        let reset = ClockPrimed {
            playback_epoch: 2,
            privacy_generation: 2,
            accepted_zero_frames: 480,
            accepted_at_us: now,
            queue_observed_at_us: now,
            queued_frames: 480,
            discarded: None,
        };
        assert!(
            path.assembly
                .prime(reset, monotonic_us(), 2, &mut path.echo)
                .is_err()
        );
        assert_eq!(path.assembly.timing().playback_epoch, 1);
        path.barrier.reset();
        let new = path.prepare(2, Some(queued(1)));
        assert_eq!(
            path.barrier.acknowledge(old, queued(2), 2, monotonic_us()),
            Err(ReferenceFaultKind::InvalidStart)
        );
        assert!(path.barrier.is_waiting());
        path.start(new);
    }
}

#[test]
fn buffered_ack_cannot_claim_analysed_or_extra_held_samples() {
    for (prepared_through, analysed_through) in [(479, 0), (481, 0), (480, 1), (480, 480)] {
        let (mut barrier, mut prepared) = armed();
        prepared.disposition = PrimeDisposition::BufferedForCapture;
        prepared.prepared_through = prepared_through;
        prepared.analysed_through = analysed_through;
        assert_eq!(
            barrier.acknowledge(prepared, queued(1), 2, 10_002),
            Err(ReferenceFaultKind::InvalidStart)
        );
    }
}

#[test]
fn duplicate_prime_or_bad_ack_does_not_refresh_original_deadline() {
    let (mut barrier, prepared) = armed();
    let later = ClockPrimed {
        playback_epoch: 2,
        privacy_generation: 2,
        accepted_zero_frames: 480,
        accepted_at_us: 108_000,
        queue_observed_at_us: 108_000,
        queued_frames: 480,
        discarded: Some(queued(1)),
    };
    assert_eq!(
        barrier.sent(later, 108_000),
        Err(ReferenceFaultKind::InvalidPrime)
    );
    assert_eq!(
        barrier.acknowledge(
            PreparedPrime {
                prepared_through: 479,
                ..prepared
            },
            queued(1),
            2,
            108_999
        ),
        Err(ReferenceFaultKind::InvalidStart)
    );
    assert_eq!(
        barrier.acknowledge(prepared, queued(1), 2, 109_000),
        Err(ReferenceFaultKind::BarrierTimeout)
    );
    let (mut barrier, prepared) = armed();
    assert_eq!(barrier.acknowledge(prepared, queued(1), 2, 108_999), Ok(1));
}

#[test]
fn malformed_and_legacy_wire_ack_are_not_implicitly_upgraded() {
    for raw in [
        r#"{"kind":"reference_primed","playback_epoch":1,"analysed_through":480}"#,
        r#"{"kind":"reference_primed","prepared":{"playback_epoch":1,"privacy_generation":2,"prepared_through":480,"analysed_through":0,"disposition":"unknown"}}"#,
        r#"{"kind":"reference_primed","prepared":{"playback_epoch":1,"privacy_generation":2,"prepared_through":480,"analysed_through":0,"disposition":"buffered_for_capture","extra":true}}"#,
    ] {
        assert!(serde_json::from_str::<ReferenceControl>(raw).is_err());
    }
}
