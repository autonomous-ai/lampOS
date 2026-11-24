//! Parent receipt transport only. These trusted synthetic receipts do not
//! replace the separate real-child test's playback and origin proof.
use super::*;
use crate::say::FixedLine;

struct Fixture {
    worker: Worker,
    peer: Option<WorkerChannels>,
    owner: Controller,
    cloud: CloudAvailability,
    _directory: SessionDirectory,
}

impl Fixture {
    fn new(role: &str) -> Self {
        let directory = SessionDirectory::create().unwrap();
        let (mut worker, mut peer) = Worker::sleeping_fixture(&directory.path, role);
        worker.set_history_active(true).unwrap();
        if role == "second" {
            assert!(matches!(
                peer.control.receive::<Control>().unwrap(),
                Some(Control::HistoryRoute {
                    epoch: 1,
                    active: true
                })
            ));
        }
        let mut owner = Controller::new(new_boot().unwrap(), now());
        owner
            .set_microphone_permission(now(), Permission::Allowed)
            .unwrap();
        let until = MonoTime::from_micros(monotonic_us() + 1_000_000);
        owner
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        owner
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
        let source = owner.admit(now(), AdmittedInput::NewTurn).unwrap();
        assert_eq!(source.turn(), 1);
        owner.complete_turn(now(), source).unwrap();
        let mut cloud = CloudAvailability::default();
        cloud
            .observe(
                ProviderLink {
                    revision: 1,
                    session: 1,
                    observed_at_us: monotonic_us(),
                    source_clock: None,
                    state: ProviderLinkState::Ready {
                        setup_us: 1,
                        attempts: 1,
                        context: crate::provider_link::LinkContext::Initial,
                        outage_started_at_us: None,
                        after: None,
                    },
                },
                monotonic_us(),
            )
            .unwrap();
        Self {
            worker,
            peer: Some(peer),
            owner,
            cloud,
            _directory: directory,
        }
    }

    fn receipt(&mut self) -> Receipt {
        let at = monotonic_us();
        let until = MonoTime::from_micros(at + 1_000_000);
        self.owner
            .set_capture(now(), CaptureState::RetainingUntil(until))
            .unwrap();
        self.owner
            .set_admission(now(), AdmissionState::OpenUntil(until))
            .unwrap();
        let owner = self.owner.admit(now(), AdmittedInput::NewTurn).unwrap();
        self.owner.complete_turn(now(), owner).unwrap();
        let snapshot = self.owner.snapshot(now()).unwrap();
        Receipt {
            line: FixedLine {
                origin: lamp_msgs::FixedLineOrigin {
                    worker_boot: boot_text(self.worker.boot),
                    provider_session: 1,
                    request: 1,
                    call_id: format!("call-{}", owner.turn()),
                    route_epoch: 1,
                    microphone_generation: snapshot.microphone_generation(),
                    camera_generation: snapshot.camera_generation(),
                    issued_us: at,
                },
                canonical: crate::language::UNPLUG_STAY.into(),
                language: "en".into(),
                text: crate::language::UNPLUG_STAY.into(),
            },
            owner,
            played_at_us: at,
        }
    }

    fn flush(&mut self, pending: &mut Pending) -> io::Result<bool> {
        pending.flush(
            &mut self.worker,
            &self.cloud,
            self.owner.snapshot(now()).unwrap(),
        )
    }

    fn receive(&mut self) -> Option<ProviderInput> {
        self.peer.as_mut().unwrap().data.receive().unwrap()
    }
}

#[test]
fn fixed_line_pending_preserves_fifo_before_the_next_model_input_and_sends_once() {
    let mut fixture = Fixture::new("second");
    let mut pending = Pending::default();
    let first = fixture.receipt();
    let second = fixture.receipt();
    pending.push(first.clone());
    pending.push(second.clone());
    assert!(fixture.flush(&mut pending).unwrap());
    let snapshot = fixture.owner.snapshot(now()).unwrap();
    fixture
        .worker
        .channels
        .data
        .send(ProviderInput::Written {
            request: 4,
            session: 1,
            privacy_generation: snapshot.microphone_generation(),
            text: "The next natural input".into(),
            look: false,
            search: false,
            quoted: false,
        })
        .unwrap();
    for expected in [first, second] {
        let Some(ProviderInput::PlayedFixedLine { receipt }) = fixture.receive() else {
            panic!("receipt must precede ordinary input")
        };
        assert_eq!(receipt.line, expected.line);
        assert_eq!(receipt.owner, expected.owner);
        assert_eq!(receipt.played_at_us, expected.played_at_us);
    }
    assert!(matches!(
        fixture.receive(),
        Some(ProviderInput::Written { request: 4, .. })
    ));
    assert!(fixture.flush(&mut pending).unwrap());
    assert!(
        fixture.receive().is_none(),
        "a second flush must not duplicate receipt delivery"
    );
}

#[test]
fn fixed_line_pending_drops_each_changed_binding_or_invalid_line() {
    let mut fixture = Fixture::new("second");
    for changed in [
        "boot",
        "session",
        "epoch",
        "microphone",
        "camera",
        "expired",
        "future",
        "text",
        "canonical",
        "owner",
    ] {
        let mut receipt = fixture.receipt();
        match changed {
            "boot" => receipt.line.origin.worker_boot = "different-worker".into(),
            "session" => receipt.line.origin.provider_session += 1,
            "epoch" => receipt.line.origin.route_epoch += 1,
            "microphone" => receipt.line.origin.microphone_generation += 1,
            "camera" => receipt.line.origin.camera_generation += 1,
            "expired" => {
                receipt.played_at_us = monotonic_us()
                    .checked_sub(crate::fixed_line_history::DELIVERY_US + 1)
                    .expect("host monotonic uptime exceeds delivery bound");
                receipt.line.origin.issued_us = receipt.played_at_us;
            }
            "future" => receipt.played_at_us += 1_000_000,
            "text" => receipt.line.text = "an arbitrary private exact line".into(),
            "canonical" => receipt.line.canonical = "an arbitrary private exact line".into(),
            "owner" => receipt.line.origin.request = receipt.owner.turn(),
            _ => unreachable!(),
        }
        let mut pending = Pending::default();
        pending.push(receipt);
        assert!(fixture.flush(&mut pending).unwrap(), "{changed}");
        assert!(pending.0.is_empty(), "{changed}");
        assert!(fixture.receive().is_none(), "{changed}");
    }
}

#[test]
fn fixed_line_pending_drops_on_privacy_close_route_return_and_unready_session() {
    for change in [
        "microphone_close",
        "microphone_reopen",
        "camera_close",
        "route_return",
        "unready",
        "ended",
    ] {
        let mut fixture = Fixture::new("second");
        let mut pending = Pending::default();
        pending.push(fixture.receipt());
        match change {
            "microphone_close" | "microphone_reopen" => {
                fixture
                    .owner
                    .set_microphone_permission(now(), Permission::Denied)
                    .unwrap();
                if change == "microphone_reopen" {
                    fixture
                        .owner
                        .set_microphone_permission(now(), Permission::Allowed)
                        .unwrap();
                }
            }
            "camera_close" => {
                fixture
                    .owner
                    .set_camera_permission(now(), Permission::Allowed)
                    .unwrap();
                fixture
                    .owner
                    .set_camera_permission(now(), Permission::Denied)
                    .unwrap();
            }
            "route_return" => {
                fixture.worker.set_history_active(false).unwrap();
                fixture.worker.set_history_active(true).unwrap();
                assert_eq!(fixture.worker.history_epoch(), Some(2));
            }
            "unready" => fixture.cloud = CloudAvailability::default(),
            "ended" => fixture.cloud.end_output().unwrap(),
            _ => unreachable!(),
        }
        assert!(fixture.flush(&mut pending).unwrap(), "{change}");
        assert!(fixture.receive().is_none(), "{change}");
    }
    let mut live = Fixture::new("provider");
    let mut pending = Pending::default();
    pending.push(live.receipt());
    assert!(live.flush(&mut pending).unwrap());
    assert!(live.receive().is_none(), "Live has no history receipt path");
}

#[test]
fn fixed_line_pending_is_bounded_and_clear_cancels_unsent_receipts() {
    let mut fixture = Fixture::new("second");
    let mut pending = Pending::default();
    for _ in 0..5 {
        pending.push(fixture.receipt());
    }
    assert_eq!(pending.0.len(), 4);
    assert!(fixture.flush(&mut pending).unwrap());
    for _ in 0..4 {
        assert!(matches!(
            fixture.receive(),
            Some(ProviderInput::PlayedFixedLine { .. })
        ));
    }
    assert!(fixture.receive().is_none());
    pending.push(fixture.receipt());
    pending.clear();
    assert!(fixture.flush(&mut pending).unwrap());
    assert!(fixture.receive().is_none());
}

#[test]
fn fixed_line_pending_backpressure_blocks_later_input_without_losing_the_receipt() {
    let mut fixture = Fixture::new("second");
    let mut filled = false;
    for _ in 0..4096 {
        match fixture.worker.channels.data.send(ProviderInput::Called {
            request: 1,
            privacy_generation: 0,
        }) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                filled = true;
                break;
            }
            Err(error) => panic!("unexpected fill failure: {error}"),
        }
    }
    assert!(filled, "the bounded fixture fills the actual data socket");
    // Construct only after filling: slow setup cannot expire the receipt first.
    let receipt = fixture.receipt();
    let mut pending = Pending::default();
    pending.push(receipt.clone());
    assert!(
        !fixture.flush(&mut pending).unwrap(),
        "ordinary input must wait"
    );
    assert_eq!(pending.0.len(), 1);
    assert_eq!(
        pending.0.front().unwrap().played_at_us,
        receipt.played_at_us
    );
    while let Some(input) = fixture.receive() {
        assert!(matches!(input, ProviderInput::Called { .. }));
    }
    assert!(
        receipt.current(monotonic_us()),
        "the retry stays inside the original delivery bound"
    );
    assert!(fixture.flush(&mut pending).unwrap());
    let Some(ProviderInput::PlayedFixedLine { receipt: delivered }) = fixture.receive() else {
        panic!("the retained receipt must be delivered")
    };
    assert_eq!(
        delivered.played_at_us, receipt.played_at_us,
        "retry cannot renew its deadline"
    );
    assert!(fixture.receive().is_none());
}

#[test]
fn fixed_line_pending_propagates_a_closed_transport_error() {
    let mut fixture = Fixture::new("second");
    drop(fixture.peer.take());
    let mut pending = Pending::default();
    pending.push(fixture.receipt());
    assert!(fixture.flush(&mut pending).is_err());
    assert_eq!(pending.0.len(), 1);
}
