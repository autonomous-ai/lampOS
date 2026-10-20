use lamp_camera::*;
use lamp_turn_owner::{BootId, Controller, MonoTime, Permission, Snapshot};
use lamp_voice::{
    camera_config::{CameraConfig, UsbIdentity},
    camera_inspect::{MAX_FRAME_SAMPLES, MetadataReport, Observer},
    camera_worker::{
        CameraEvent, CameraLink, CameraLoop, Counts, FrameMetadata, IdentityMetadata, InspectPort,
        PortMetadata, Step, StopReason, Timing,
    },
    process::{SessionDirectory, new_boot},
    transport::WorkerChannels,
    wire::Control,
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    io,
    rc::Rc,
};

fn t(us: u64) -> MonoTime {
    MonoTime::from_micros(us)
}
fn boot(n: u8) -> BootId {
    BootId::new([n; 16]).unwrap()
}
fn config() -> CameraConfig {
    CameraConfig {
        device: "/dev/explicit-camera".into(),
        lock_directory: "/run/lamp-camera-lock".into(),
        source: "desk-camera".into(),
        expected_usb: UsbIdentity {
            vendor: 0x1234,
            product: 0x5678,
            topology: "1-2.3".into(),
            serial: None,
            interface_number: 0,
            capture_index: 0,
        },
        width: 640,
        height: 480,
        interval: None,
        max_frame_bytes: 256,
        buffers: 2,
    }
}
fn mode() -> NegotiatedMode {
    NegotiatedMode {
        source: SourceId::new("desk-camera").unwrap(),
        format: PixelFormat::MJPG,
        width: 640,
        height: 480,
        interval: None,
        size_image: 256,
        buffers: 2,
    }
}
fn port_metadata() -> PortMetadata {
    PortMetadata {
        identity: Some(IdentityMetadata {
            canonical_node: "/dev/video-test".into(),
            major: 81,
            minor: 0,
            inode: 10,
            usb: config().expected_usb,
        }),
        ..PortMetadata::default()
    }
}
#[derive(Clone)]
struct TestClock(Rc<Cell<u64>>);
impl Clock for TestClock {
    fn now(&mut self) -> Result<MonoTime, ClockError> {
        Ok(t(self.0.get()))
    }
}
#[derive(Default)]
struct PortState {
    active: Option<CaptureId>,
    starts: usize,
    stops: usize,
    reads: usize,
    frames: u32,
    start_delay: u64,
    read_delay: u64,
    stop_delay: u64,
    timestamp_override: Option<lamp_camera::DriverTimestamp>,
    payload_override: Option<Vec<u8>>,
    revoked_before_stop: bool,
    after_start: VecDeque<Control>,
    after_read: VecDeque<Control>,
}
struct FakePort {
    state: Rc<RefCell<PortState>>,
    controls: Rc<RefCell<VecDeque<Control>>>,
    clock: TestClock,
    revoked: Rc<Cell<bool>>,
}
impl PortIo for FakePort {
    fn start(
        &mut self,
        _: CaptureConfig,
        capture: CaptureId,
        _: OperationBudget,
    ) -> Result<NegotiatedMode, PortError> {
        let mut s = self.state.borrow_mut();
        s.starts += 1;
        s.active = Some(capture);
        self.clock.0.set(self.clock.0.get() + s.start_delay);
        self.controls.borrow_mut().append(&mut s.after_start);
        Ok(mode())
    }
    fn try_frame(
        &mut self,
        destination: &mut [u8],
        _: OperationBudget,
    ) -> Result<Option<PortFrame>, PortError> {
        let mut s = self.state.borrow_mut();
        s.reads += 1;
        self.clock.0.set(self.clock.0.get() + s.read_delay);
        self.controls.borrow_mut().append(&mut s.after_read);
        if s.frames == 0 {
            return Ok(None);
        }
        s.frames -= 1;
        let payload = s
            .payload_override
            .as_deref()
            .unwrap_or(&[0xff, 0xd8, 0x11, 0x22, 0xff, 0xd9]);
        destination[..payload.len()].copy_from_slice(payload);
        Ok(Some(PortFrame {
            capture: s.active.unwrap(),
            source: mode().source,
            bytes_used: payload.len(),
            driver_sequence: s.reads as u32,
            timestamp: Some(
                s.timestamp_override
                    .unwrap_or(lamp_camera::DriverTimestamp {
                        micros: 123,
                        domain: TimestampDomain::Unknown,
                        point: lamp_camera::TimestampPoint::Unknown,
                    }),
            ),
        }))
    }
    fn stop(&mut self, _: OperationBudget) -> Result<(), PortError> {
        let mut s = self.state.borrow_mut();
        s.stops += 1;
        s.revoked_before_stop = self.revoked.get();
        s.active = None;
        self.clock.0.set(self.clock.0.get() + s.stop_delay);
        Ok(())
    }
}
impl InspectPort for FakePort {
    fn metadata(&self) -> PortMetadata {
        port_metadata()
    }
}
#[derive(Default)]
struct Link {
    controls: Rc<RefCell<VecDeque<Control>>>,
    events: Vec<CameraEvent>,
    empty_reads: usize,
    after_empty: BTreeMap<usize, VecDeque<Control>>,
    fail_frame: bool,
    revoked: Rc<Cell<bool>>,
}
impl CameraLink for Link {
    fn receive_control(&mut self) -> io::Result<Option<Control>> {
        let result = self.controls.borrow_mut().pop_front();
        if result.is_none() {
            self.empty_reads += 1;
            if let Some(mut queued) = self.after_empty.remove(&self.empty_reads) {
                self.controls.borrow_mut().append(&mut queued);
            }
        }
        Ok(result)
    }
    fn publish(&mut self, event: CameraEvent) -> io::Result<()> {
        if self.fail_frame && matches!(event, CameraEvent::Frame { .. }) {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        if matches!(event, CameraEvent::Revoked { .. }) {
            self.revoked.set(true);
        }
        self.events.push(event);
        Ok(())
    }
}
struct Rig {
    runtime: CameraLoop<FakePort, TestClock>,
    clock: TestClock,
    state: Rc<RefCell<PortState>>,
    link: Link,
    owner: Controller,
}
impl Rig {
    fn new() -> Self {
        Self::build(None)
    }
    fn build(frames: Option<lamp_camera::frames::FrameServer>) -> Self {
        let clock = TestClock(Rc::new(Cell::new(1000)));
        let state = Rc::new(RefCell::new(PortState::default()));
        let link = Link::default();
        let capture = Capture::new(
            FakePort {
                state: state.clone(),
                controls: link.controls.clone(),
                clock: clock.clone(),
                revoked: link.revoked.clone(),
            },
            clock.clone(),
            boot(1),
            boot(2),
            config().capture().unwrap(),
        )
        .unwrap();
        let mut runtime = CameraLoop::new(capture, 1000);
        if let Some(frames) = frames {
            runtime = runtime.with_frames(frames);
        }
        let owner = Controller::new(boot(1), t(1000));
        Self {
            runtime,
            clock,
            state,
            link,
            owner,
        }
    }
    fn authority(&mut self, permission: Permission) -> Snapshot {
        let now = t(self.clock.0.get());
        self.owner.set_camera_permission(now, permission).unwrap();
        self.owner.snapshot(now).unwrap()
    }
    fn send(&mut self, control: Control) {
        self.link.controls.borrow_mut().push_back(control);
    }
    fn step(&mut self) -> io::Result<Step> {
        let clock = self.clock.clone();
        self.runtime.step(&mut self.link, &mut || clock.0.get())
    }
    fn start(&mut self) {
        let snapshot = self.authority(Permission::Allowed);
        self.send(Control::Authority { snapshot });
        self.send(Control::StartCapture);
        assert_eq!(self.step().unwrap(), Step::Continue);
    }
    fn frames(&self) -> Vec<&FrameMetadata> {
        self.link
            .events
            .iter()
            .filter_map(|e| {
                if let CameraEvent::Frame { frame } = e {
                    Some(frame)
                } else {
                    None
                }
            })
            .collect()
    }
}

#[test]
fn closed_and_permitted_but_not_started_never_open_or_claim_a_frame() {
    let mut r = Rig::new();
    r.step().unwrap();
    assert_eq!(r.state.borrow().starts, 0);
    let snapshot = r.authority(Permission::Allowed);
    r.send(Control::Authority { snapshot });
    r.step().unwrap();
    assert_eq!(r.state.borrow().starts, 0);
    assert!(r.frames().is_empty());
    r.send(Control::StartCapture);
    r.step().unwrap();
    assert_eq!(r.state.borrow().starts, 1);
    assert!(r.frames().is_empty());
    assert!(
        r.link
            .events
            .iter()
            .any(|e| matches!(e, CameraEvent::Started { .. }))
    );
}
#[test]
fn no_permission_or_unknown_permission_cannot_open_camera() {
    for permission in [None, Some(Permission::Denied), Some(Permission::Unknown)] {
        let mut r = Rig::new();
        if let Some(p) = permission {
            let snapshot = r.authority(p);
            r.send(Control::Authority { snapshot });
        }
        r.send(Control::StartCapture);
        assert!(r.step().is_err());
        assert_eq!(r.state.borrow().starts, 0);
    }
}
#[test]
fn original_grant_unknown_timestamp_and_metadata_only_survive_delivery() {
    let mut r = Rig::new();
    r.start();
    let grant = r.owner.camera_grant(t(r.clock.0.get())).unwrap();
    r.state.borrow_mut().frames = 1;
    r.step().unwrap();
    let frame = r.frames()[0];
    assert_eq!(frame.grant, grant);
    assert_eq!(frame.bytes_used, 6);
    assert_eq!(frame.capture_epoch, 1);
    assert_eq!(
        frame.driver_timestamp.unwrap().domain,
        lamp_voice::camera_worker::ClockDomain::Unknown
    );
    let json = serde_json::to_value(frame).unwrap();
    assert!(json.get("bytes").is_none());
    assert!(json.get("image").is_none());
}
#[test]
fn revoke_during_start_suppresses_started_readiness_and_stops_once() {
    let mut r = Rig::new();
    let allowed = r.authority(Permission::Allowed);
    let denied = r.authority(Permission::Denied);
    r.send(Control::Authority { snapshot: allowed });
    r.send(Control::StartCapture);
    r.state
        .borrow_mut()
        .after_start
        .push_back(Control::Authority { snapshot: denied });
    assert_eq!(r.step().unwrap(), Step::Stopped);
    assert_eq!(r.state.borrow().stops, 1);
    assert!(
        !r.link
            .events
            .iter()
            .any(|e| matches!(e, CameraEvent::Started { .. } | CameraEvent::Frame { .. }))
    );
}
#[test]
fn revoke_during_dequeue_discards_original_frame_before_handoff() {
    let mut r = Rig::new();
    r.start();
    let denied = r.authority(Permission::Denied);
    r.state.borrow_mut().frames = 1;
    r.state
        .borrow_mut()
        .after_read
        .push_back(Control::Authority { snapshot: denied });
    assert_eq!(r.step().unwrap(), Step::Stopped);
    assert!(r.frames().is_empty());
    assert_eq!(r.state.borrow().stops, 1);
}
#[test]
fn revoke_after_post_read_drain_is_seen_at_final_handoff_drain() {
    let mut r = Rig::new();
    r.start();
    r.state.borrow_mut().frames = 1;
    let denied = r.authority(Permission::Denied);
    let post_read_empty = r.link.empty_reads + 2;
    r.link.after_empty.insert(
        post_read_empty,
        VecDeque::from([Control::Authority { snapshot: denied }]),
    );
    assert_eq!(r.step().unwrap(), Step::Stopped);
    assert!(r.frames().is_empty());
    assert_eq!(r.state.borrow().stops, 1);
}
#[test]
fn stop_after_dequeue_and_before_delivery_wins() {
    let mut r = Rig::new();
    r.start();
    r.state.borrow_mut().frames = 1;
    r.state.borrow_mut().after_read.push_back(Control::Stop);
    assert_eq!(r.step().unwrap(), Step::Stopped);
    assert!(r.frames().is_empty());
}
#[test]
fn stale_duplicate_authority_does_not_close_newer_capture() {
    let mut r = Rig::new();
    let snapshot = r.authority(Permission::Allowed);
    r.send(Control::Authority { snapshot });
    r.send(Control::StartCapture);
    r.step().unwrap();
    r.send(Control::Authority { snapshot });
    r.state.borrow_mut().frames = 1;
    r.step().unwrap();
    assert_eq!(r.frames().len(), 1);
    assert_eq!(r.state.borrow().stops, 0);
}
#[test]
fn full_control_slice_defers_port_work_and_then_keeps_latest_delivery_bounded() {
    let mut r = Rig::new();
    r.start();
    r.state.borrow_mut().frames = 2;
    for _ in 0..16 {
        let snapshot = r.authority(Permission::Allowed);
        r.send(Control::Authority { snapshot });
    }
    r.step().unwrap();
    assert_eq!(r.state.borrow().reads, 0);
    assert!(r.frames().is_empty());
    r.step().unwrap();
    assert_eq!(r.frames().len(), 1);
    let mut commands = VecDeque::new();
    for _ in 0..16 {
        let snapshot = r.authority(Permission::Allowed);
        commands.push_back(Control::Authority { snapshot });
    }
    r.state.borrow_mut().after_read = commands;
    r.step().unwrap();
    assert_eq!(r.frames().len(), 1);
    r.step().unwrap();
    assert_eq!(r.frames().len(), 2);
    assert_eq!(r.frames()[1].sequence, 2);
}
#[test]
fn control_full_at_final_handoff_retains_only_original_token() {
    let mut r = Rig::new();
    r.start();
    r.state.borrow_mut().frames = 1;
    let mut commands = VecDeque::new();
    for _ in 0..16 {
        let snapshot = r.authority(Permission::Allowed);
        commands.push_back(Control::Authority { snapshot });
    }
    r.link.after_empty.insert(r.link.empty_reads + 2, commands);
    r.step().unwrap();
    assert!(r.frames().is_empty());
    r.step().unwrap();
    assert_eq!(r.frames().len(), 1);
    assert_eq!(r.frames()[0].sequence, 1);
}
#[test]
fn full_metadata_channel_closes_once_and_never_retries_frame() {
    let mut r = Rig::new();
    r.start();
    r.state.borrow_mut().frames = 1;
    r.link.fail_frame = true;
    assert!(r.step().is_err());
    assert_eq!(r.state.borrow().stops, 1);
    assert!(r.frames().is_empty());
    assert!(r.step().is_err());
    assert_eq!(r.state.borrow().reads, 1);
}
#[test]
fn late_control_cannot_revive_expired_capture() {
    let mut r = Rig::new();
    r.start();
    r.clock.0.set(251_000);
    let snapshot = r.authority(Permission::Allowed);
    r.send(Control::Authority { snapshot });
    assert!(r.step().is_err());
    assert_eq!(r.state.borrow().stops, 1);
    assert_eq!(r.state.borrow().reads, 0);
}
#[test]
fn read_and_start_overruns_stop_without_a_current_frame() {
    let mut r = Rig::new();
    r.state.borrow_mut().start_delay = START_BUDGET_US;
    let snapshot = r.authority(Permission::Allowed);
    r.send(Control::Authority { snapshot });
    r.send(Control::StartCapture);
    assert!(r.step().is_err());
    assert!(r.frames().is_empty());
    assert_eq!(r.state.borrow().stops, 1);
    let mut r = Rig::new();
    r.start();
    r.state.borrow_mut().read_delay = READ_BUDGET_US;
    r.state.borrow_mut().frames = 1;
    assert!(r.step().is_err());
    assert!(r.frames().is_empty());
    assert_eq!(r.state.borrow().stops, 1);
}
#[test]
fn no_first_frame_with_continuous_authority_is_explicit_failure() {
    let mut r = Rig::new();
    r.start();
    for tick in 1..=200 {
        r.clock.0.set(1000 + tick * 10_000);
        let snapshot = r.authority(Permission::Allowed);
        r.send(Control::Authority { snapshot });
        let result = r.step();
        if tick < 200 {
            assert!(result.is_ok());
        } else {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("FirstFrameTimeout")
            );
        }
    }
    assert_eq!(r.state.borrow().stops, 1);
    assert!(r.frames().is_empty());
}
#[test]
fn reported_stop_time_follows_cleanup_and_duplicate_start_is_failure() {
    let mut r = Rig::new();
    r.start();
    r.state.borrow_mut().stop_delay = 500;
    r.send(Control::Stop);
    assert_eq!(r.step().unwrap(), Step::Stopped);
    let CameraEvent::Stopped {
        at_us,
        stop: Some(timing),
        ..
    } = r.link.events.last().unwrap()
    else {
        panic!("stop evidence")
    };
    assert_eq!(*at_us, 1500);
    assert_eq!(timing.completed_at_us, 1500);
    let mut r = Rig::new();
    r.start();
    r.send(Control::StartCapture);
    assert!(r.step().is_err());
    assert_eq!(r.state.borrow().starts, 1);
}

struct Parent {
    owner: Controller,
    observer: Observer,
    report: MetadataReport,
    grant: lamp_turn_owner::CameraGrant,
}
impl Parent {
    fn new() -> Self {
        let mut owner = Controller::new(boot(1), t(1000));
        owner
            .set_camera_permission(t(1000), Permission::Allowed)
            .unwrap();
        let grant = owner.camera_grant(t(1000)).unwrap();
        let mut observer = Observer::new(boot(2), 1000);
        let mut report = MetadataReport::new(config(), "a".repeat(64), 1, 1000).unwrap();
        observer
            .accept(
                CameraEvent::Opening {
                    at_us: 1000,
                    deadline_us: 101000,
                },
                Some(grant),
                1000,
                &mut report,
            )
            .unwrap();
        observer
            .accept(
                CameraEvent::Started {
                    at_us: 1100,
                    worker: boot(2),
                    capture_epoch: 1,
                    mode: mode().into(),
                    timing: Timing {
                        started_at_us: 1000,
                        completed_at_us: 1100,
                    },
                    port: port_metadata(),
                },
                Some(grant),
                1100,
                &mut report,
            )
            .unwrap();
        Self {
            owner,
            observer,
            report,
            grant,
        }
    }
    fn frame(&self, sequence: u64) -> FrameMetadata {
        FrameMetadata {
            worker: boot(2),
            capture_epoch: 1,
            sequence,
            grant: self.grant,
            mode: mode().into(),
            bytes_used: 6,
            driver_sequence: sequence as u32,
            driver_timestamp: None,
            dequeue: Timing {
                started_at_us: 1200,
                completed_at_us: 1300,
            },
            published_at_us: 1400,
        }
    }
    fn accept(&mut self, frame: FrameMetadata) -> io::Result<()> {
        self.observer.accept(
            CameraEvent::Frame { frame },
            Some(self.grant),
            1500,
            &mut self.report,
        )
    }
}
#[test]
fn parent_rechecks_original_grant_and_cannot_relabel_after_reopen() {
    let mut p = Parent::new();
    let frame = p.frame(1);
    p.owner
        .set_camera_permission(t(1200), Permission::Denied)
        .unwrap();
    p.owner
        .set_camera_permission(t(1300), Permission::Allowed)
        .unwrap();
    let newer = p.owner.camera_grant(t(1400)).unwrap();
    assert_ne!(newer, p.grant);
    assert!(
        p.observer
            .accept(
                CameraEvent::Frame { frame },
                Some(newer),
                1500,
                &mut p.report
            )
            .is_err()
    );
    assert_eq!(p.report.frames_accepted, 0);
}
#[test]
fn parent_has_no_ready_frame_on_start_and_reports_bounded_samples_only() {
    let mut p = Parent::new();
    assert!(p.report.first_frame_at_us.is_none());
    for sequence in 1..=70 {
        p.accept(p.frame(sequence)).unwrap();
    }
    assert_eq!(p.report.frames_accepted, 70);
    assert_eq!(p.report.first_frame_samples.len(), MAX_FRAME_SAMPLES);
    assert_eq!(p.report.omitted_frame_samples, 6);
    assert_eq!(p.report.dequeue_to_parent_acceptance.count, 70);
    assert_eq!(p.report.dequeue_to_parent_acceptance.max_us, Some(200));
    assert!(
        !p.report.images_saved
            && !p.report.images_transmitted
            && !p.report.optical_visibility_qualified
            && !p.report.listening_readiness_claimed
    );
}
#[test]
fn parent_rejects_late_wrong_source_repeated_and_old_epoch_metadata() {
    let mut p = Parent::new();
    p.accept(p.frame(1)).unwrap();
    assert!(p.accept(p.frame(1)).is_err());
    let mut p = Parent::new();
    let mut frame = p.frame(1);
    frame.capture_epoch = 2;
    assert!(p.accept(frame).is_err());
    let mut p = Parent::new();
    let mut frame = p.frame(1);
    frame.mode.source = "other-camera".into();
    assert!(p.accept(frame).is_err());
    let mut p = Parent::new();
    let frame = p.frame(1);
    assert!(
        p.observer
            .accept(
                CameraEvent::Frame { frame },
                Some(p.grant),
                200_000,
                &mut p.report
            )
            .is_err()
    );
}
#[test]
fn an_opening_deadline_cannot_be_extended_by_progress_and_stop_is_not_success() {
    let mut owner = Controller::new(boot(1), t(1000));
    owner
        .set_camera_permission(t(1000), Permission::Allowed)
        .unwrap();
    let grant = owner.camera_grant(t(1000)).unwrap();
    let mut observer = Observer::new(boot(2), 1000);
    let mut report = MetadataReport::new(config(), "a".repeat(64), 1, 1000).unwrap();
    observer
        .accept(
            CameraEvent::Opening {
                at_us: 1000,
                deadline_us: 101000,
            },
            Some(grant),
            1000,
            &mut report,
        )
        .unwrap();
    assert!(
        observer
            .accept(
                CameraEvent::Progress {
                    at_us: 2000,
                    counts: Counts::default(),
                    last_read: None,
                    startup_discards: None,
                },
                Some(grant),
                2000,
                &mut report
            )
            .is_err()
    );
    assert!(observer.check_deadline(103_000).is_err());
    let mut p = Parent::new();
    assert!(
        p.observer
            .accept(
                CameraEvent::Stopped {
                    at_us: 1200,
                    reason: StopReason::Fault,
                    counts: Counts::default(),
                    stop: None,
                    port: PortMetadata::default(),
                    error: Some("fault".into())
                },
                Some(p.grant),
                1200,
                &mut p.report
            )
            .is_err()
    );
}
#[test]
fn camera_role_channels_are_separate_and_unknown_roles_still_rejected() {
    let directory = SessionDirectory::create().unwrap();
    let parent = new_boot().unwrap();
    let worker = new_boot().unwrap();
    let mut a = WorkerChannels::bind(&directory.path, "camera", parent, worker, false).unwrap();
    let mut b = WorkerChannels::bind(&directory.path, "camera", parent, worker, true).unwrap();
    a.connect(&directory.path, "camera", false).unwrap();
    b.connect(&directory.path, "camera", true).unwrap();
    a.control.send(Control::Stop).unwrap();
    assert!(matches!(
        b.control.receive::<Control>().unwrap(),
        Some(Control::Stop)
    ));
    assert!(b.data.receive::<CameraEvent>().unwrap().is_none());
    assert!(WorkerChannels::bind(&directory.path, "arbitrary", parent, worker, false).is_err());
}

#[test]
fn queued_old_progress_cannot_hide_a_current_worker_stall() {
    let mut p = Parent::new();
    p.observer
        .accept(
            CameraEvent::Progress {
                at_us: 30_000,
                counts: Counts::default(),
                last_read: None,
                startup_discards: None,
            },
            Some(p.grant),
            30_000,
            &mut p.report,
        )
        .unwrap();
    let event = CameraEvent::Progress {
        at_us: 1200,
        counts: Counts::default(),
        last_read: None,
        startup_discards: None,
    };
    assert!(
        p.observer
            .accept(event, Some(p.grant), 32_000, &mut p.report)
            .is_err()
    );
    assert_eq!(p.report.frames_accepted, 0);
}

mod frame_link {
    use super::*;
    use lamp_camera::frames::{CloseReason, FrameClient, FrameServer, LinkEvent, Role};
    use std::{os::unix::fs::PermissionsExt, path::PathBuf};

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn dir(label: &str) -> Dir {
        let path = PathBuf::from(format!("/tmp/lcw-{label}-{}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        Dir(path)
    }
    const CONSUMER: u8 = 7;

    fn rig(dir: &Dir) -> (Rig, FrameClient) {
        let socket = dir.0.join(lamp_voice::camera_worker::FRAME_SOCKET);
        let server =
            FrameServer::bind(&socket, boot(1), &[(Role::Perception, boot(CONSUMER))], 256)
                .unwrap();
        let rig = Rig::build(Some(server));
        let client = FrameClient::new(&socket, Role::Perception, boot(CONSUMER), boot(1), 256);
        (rig, client)
    }
    fn connect(r: &mut Rig, client: &mut FrameClient) {
        assert!(client.connect().unwrap());
        for _ in 0..200 {
            r.step().unwrap();
            if r.runtime.frame_link().unwrap().connected(Role::Perception) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("consumer not accepted");
    }
    fn receive(r: &mut Rig, client: &mut FrameClient) -> (lamp_camera::frames::FrameMeta, Vec<u8>) {
        let mut out = Vec::new();
        for _ in 0..200 {
            r.step().unwrap();
            if let Some(meta) = client.poll(&mut out).unwrap() {
                return (meta, out);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("no frame");
    }

    #[test]
    fn frames_leave_the_library_only_for_a_waiting_consumer() {
        let d = dir("wait");
        let (mut r, mut client) = rig(&d);
        r.start();
        connect(&mut r, &mut client);
        assert!(r.link.events.iter().any(|e| matches!(
            e,
            CameraEvent::FrameLink {
                event: LinkEvent::Connected {
                    role: Role::Perception,
                    ..
                },
                ..
            }
        )));
        // Captured frames replace the pending slot; none is handed off.
        r.state.borrow_mut().frames = 3;
        for _ in 0..3 {
            r.step().unwrap();
        }
        assert!(r.frames().is_empty());
        let grant = r.owner.camera_grant(t(r.clock.0.get())).unwrap();
        client.request(200_000).unwrap();
        r.state.borrow_mut().frames = 1;
        let (meta, bytes) = receive(&mut r, &mut client);
        assert_eq!(bytes, [0xff, 0xd8, 0x11, 0x22, 0xff, 0xd9]);
        assert_eq!(meta.grant, grant);
        let published = r.frames();
        assert_eq!(
            published.len(),
            1,
            "metadata is published for the delivered frame"
        );
        assert_eq!(published[0].sequence, meta.frame_sequence);
        assert_eq!(published[0].capture_epoch, meta.capture_epoch);
        // Without a new request nothing more leaves the library.
        r.state.borrow_mut().frames = 2;
        r.step().unwrap();
        r.step().unwrap();
        assert_eq!(r.frames().len(), 1);
    }

    #[test]
    fn privacy_revoke_closes_the_frame_link_before_the_camera_stops() {
        let d = dir("privacy");
        let (mut r, mut client) = rig(&d);
        r.start();
        connect(&mut r, &mut client);
        client.request(200_000).unwrap();
        r.step().unwrap();
        let snapshot = r.authority(Permission::Denied);
        r.send(Control::Authority { snapshot });
        assert_eq!(r.step().unwrap(), Step::Stopped);
        assert!(r.state.borrow().revoked_before_stop);
        let closed = r.link.events.iter().position(|e| {
            matches!(
                e,
                CameraEvent::FrameLink {
                    event: LinkEvent::Closed {
                        reason: CloseReason::PrivacyClosed,
                        ..
                    },
                    ..
                }
            )
        });
        let stopped = r
            .link
            .events
            .iter()
            .position(|e| matches!(e, CameraEvent::Stopped { .. }));
        assert!(closed.unwrap() < stopped.unwrap());
        let mut out = Vec::new();
        let mut result = Ok(None);
        for _ in 0..200 {
            result = client.poll(&mut out);
            if !matches!(result, Ok(None)) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            result.is_err(),
            "the consumer sees the link close, not a frame"
        );
    }

    #[test]
    fn a_worker_fault_closes_consumers() {
        let d = dir("fault");
        let (mut r, mut client) = rig(&d);
        r.start();
        connect(&mut r, &mut client);
        // A duplicate start is a worker fault.
        r.send(Control::StartCapture);
        assert!(r.step().is_err());
        assert!(!r.runtime.frame_link().unwrap().connected(Role::Perception));
    }

    #[test]
    fn consumer_flags_are_strict() {
        use lamp_voice::camera_worker::parse_frame_consumers;
        let hex = "07".repeat(16);
        let ok = parse_frame_consumers(&["--frame-consumer".into(), format!("perception={hex}")])
            .unwrap();
        assert_eq!(ok, vec![(Role::Perception, boot(7))]);
        for bad in [
            vec!["--frame-consumer".to_owned()],
            vec!["--frame-consumer".into(), format!("vision={hex}")],
            vec!["--frame-consumer".into(), "perception=00".into()],
            vec![
                "--frame-consumer".into(),
                format!("context={hex}"),
                "--frame-consumer".into(),
                format!("context={hex}"),
            ],
            vec!["--other".into(), "x".into()],
        ] {
            assert!(parse_frame_consumers(&bad).is_err(), "{bad:?}");
        }
    }
}

#[test]
fn driver_teardown_after_prompt_revoke_has_its_own_bound() {
    let mut r = Rig::new();
    r.start();
    r.state.borrow_mut().stop_delay = 40_000;
    r.send(Control::Stop);
    assert_eq!(
        r.step()
            .expect("sealed worker may finish bounded driver teardown"),
        Step::Stopped
    );
    assert_eq!(r.state.borrow().stops, 1);
    assert_eq!(r.clock.0.get(), 41_000);
    assert!(r.state.borrow().revoked_before_stop);
    let revoked = r
        .link
        .events
        .iter()
        .find_map(|e| match e {
            CameraEvent::Revoked {
                at_us,
                teardown_deadline_us,
                error,
                ..
            } => Some((*at_us, *teardown_deadline_us, error)),
            _ => None,
        })
        .unwrap();
    assert_eq!((revoked.0, revoked.1), (1000, 251_000));
    assert!(revoked.2.is_none());
    assert!(r.step().is_err(), "no read or restart after revocation");
    assert_eq!(r.state.borrow().stops, 1);
}

#[test]
fn teardown_overrun_keeps_prompt_revocation_but_never_reports_success() {
    let mut r = Rig::new();
    r.start();
    r.state.borrow_mut().stop_delay = DRIVER_TEARDOWN_BUDGET_US;
    r.send(Control::Stop);
    assert!(
        r.step()
            .unwrap_err()
            .to_string()
            .contains("StopBudgetExceeded")
    );
    assert!(r.state.borrow().revoked_before_stop);
    assert!(matches!(
        r.link.events.last().unwrap(),
        CameraEvent::Stopped { error: Some(_), .. }
    ));
    assert!(r.step().is_err());
    assert_eq!(r.state.borrow().stops, 1);
}

#[test]
fn read_failure_is_reported_before_driver_teardown_and_stays_failed() {
    let mut r = Rig::new();
    r.start();
    r.state.borrow_mut().read_delay = READ_BUDGET_US;
    r.state.borrow_mut().stop_delay = 40_000;
    let error = r.step().unwrap_err().to_string();
    assert!(error.contains("ReadBudgetExceeded"));
    assert!(r.state.borrow().revoked_before_stop);
    assert!(r.link.events.iter().any(|e| matches!(e, CameraEvent::Revoked { at_us: 11_000, error: Some(message), .. } if message.contains("ReadBudgetExceeded"))));
    assert_eq!(r.clock.0.get(), 51_000);
    assert!(matches!(
        r.link.events.last().unwrap(),
        CameraEvent::Stopped { error: Some(_), .. }
    ));
    assert!(r.frames().is_empty());
}

#[test]
fn timestamp_fault_receipt_retains_exact_comparison_without_frame_bytes() {
    const PIXELS: &[u8] = b"private-pixel-marker-pretend-secret";
    for (predicate, timestamp, previous) in [
        ("future", 251_038, None),
        ("before_capture_start", 999, None),
        ("too_old", 1037, Some(200_000)),
        ("regressed", 250_994, Some(250_995)),
    ] {
        let mut r = Rig::new();
        r.start();
        // Refresh ordinary authority while the original lease is still live;
        // timestamp rejection must not be confused with a privacy timeout.
        r.clock.0.set(200_000);
        let authority = r.authority(Permission::Allowed);
        r.send(Control::Authority {
            snapshot: authority,
        });
        r.step().unwrap();
        if let Some(prior) = previous {
            r.clock.0.set(prior);
            let mut state = r.state.borrow_mut();
            state.frames = 1;
            state.timestamp_override = Some(lamp_camera::DriverTimestamp {
                micros: prior,
                domain: TimestampDomain::HostMonotonic,
                point: lamp_camera::TimestampPoint::StartOfExposure,
            });
            drop(state);
            r.step().unwrap();
        }
        r.clock.0.set(251_000);
        {
            let mut state = r.state.borrow_mut();
            state.frames = 1;
            state.read_delay = 37;
            state.timestamp_override = Some(lamp_camera::DriverTimestamp {
                micros: timestamp,
                domain: TimestampDomain::HostMonotonic,
                point: lamp_camera::TimestampPoint::StartOfExposure,
            });
            state.payload_override = Some([&[0xff, 0xd8], PIXELS, &[0xff, 0xd9]].concat());
        }
        let before_frames = r.frames().len();
        assert!(
            r.step()
                .unwrap_err()
                .to_string()
                .contains("InvalidDriverTimestamp")
        );
        assert_eq!(
            r.frames().len(),
            before_frames,
            "rejected bytes must never be delivered"
        );
        let revoked: Vec<_> = r
            .link
            .events
            .iter()
            .filter(|e| matches!(e, CameraEvent::Revoked { .. }))
            .collect();
        assert_eq!(revoked.len(), 1);
        let json = serde_json::to_value(revoked[0]).unwrap();
        let record = json
            .get("timestamp_rejection")
            .expect("fault receipt must retain rejected timestamp evidence");
        assert_eq!(record["predicate"], predicate);
        assert_eq!(record["timestamp"]["micros"], timestamp);
        assert_eq!(record["timestamp"]["domain"], "host_monotonic");
        assert_eq!(record["timestamp"]["point"], "start_of_exposure");
        assert_eq!(record["capture_epoch"], 1);
        assert_eq!(record["driver_sequence"], r.state.borrow().reads as u32);
        assert_eq!(record["capture_started_at_us"], 1000);
        assert_eq!(record["dequeue_started_at_us"], 251_000);
        assert_eq!(record["comparison_at_us"], 251_037);
        assert_eq!(record["maximum_age_us"], 250_000);
        assert_eq!(record["previous_timestamp"]["micros"].as_u64(), previous);
        let mut report = MetadataReport::new(config(), "a".repeat(64), 1, 251_037).unwrap();
        let mut observer = Observer::new(boot(2), 251_037);
        assert!(
            observer
                .accept(revoked[0].clone(), None, 251_037, &mut report)
                .unwrap_err()
                .to_string()
                .contains("revoked before inspection end")
        );
        assert_eq!(
            serde_json::to_value(report).unwrap()["revoked_worker_event"]["timestamp_rejection"],
            *record,
            "native inspection report must preserve the original fault evidence"
        );
        let encoded = serde_json::to_vec(revoked[0]).unwrap();
        assert!(
            encoded.len() <= 1500,
            "fixed record must remain bounded metadata"
        );
        assert!(
            !String::from_utf8(encoded.clone())
                .unwrap()
                .contains(std::str::from_utf8(PIXELS).unwrap())
        );
        assert!(r.state.borrow().revoked_before_stop);
        assert_eq!(r.state.borrow().stops, 1);
        assert!(r.step().is_err(), "a fault remains terminal");
        let again = r
            .link
            .events
            .iter()
            .find(|e| matches!(e, CameraEvent::Revoked { .. }))
            .unwrap();
        assert_eq!(
            serde_json::to_vec(again).unwrap(),
            encoded,
            "the single record must not be overwritten"
        );
    }
}

fn refresh_camera_at(r: &mut Rig, at_us: u64) {
    r.clock.0.set(at_us);
    let snapshot = r.authority(Permission::Allowed);
    r.send(Control::Authority { snapshot });
}
fn queue_monotonic_frame(r: &mut Rig, exposure_us: u64) {
    let mut state = r.state.borrow_mut();
    state.frames = 1;
    state.timestamp_override = Some(lamp_camera::DriverTimestamp {
        micros: exposure_us,
        domain: TimestampDomain::HostMonotonic,
        point: lamp_camera::TimestampPoint::StartOfExposure,
    });
}

#[test]
fn startup_discard_keeps_camera_loop_live_until_a_fresh_original_grant_frame() {
    let mut r = Rig::new();
    r.start();
    let grant = r.owner.camera_grant(t(1000)).unwrap();
    for at_us in [200_000, 400_000] {
        refresh_camera_at(&mut r, at_us);
        r.step().unwrap();
    }
    r.clock.0.set(455_929);
    queue_monotonic_frame(&mut r, 55_000);
    let reads = r.state.borrow().reads;
    assert_eq!(
        r.step().expect("old startup frame is not terminal"),
        Step::Continue
    );
    assert_eq!(r.state.borrow().reads, reads + 1);
    assert!(r.frames().is_empty());
    assert_eq!(r.state.borrow().stops, 0);
    r.clock.0.set(457_000);
    queue_monotonic_frame(&mut r, 450_000);
    assert_eq!(r.step().unwrap(), Step::Continue);
    assert_eq!(r.frames().len(), 1);
    assert_eq!(r.frames()[0].grant, grant);
    assert_eq!(r.frames()[0].sequence, 1);
    assert_eq!(r.frames()[0].driver_timestamp.unwrap().micros, 450_000);
}

#[test]
fn startup_forever_old_frames_finish_as_unavailable_with_first_latest_age_evidence() {
    let mut r = Rig::new();
    r.start();
    for n in 1..20 {
        let at = 1_000 + n * 100_000;
        refresh_camera_at(&mut r, at);
        if n >= 4 {
            queue_monotonic_frame(&mut r, at - 300_000);
        }
        let reads = r.state.borrow().reads;
        assert_eq!(r.step().unwrap(), Step::Continue);
        assert_eq!(r.state.borrow().reads, reads + 1);
        assert!(r.frames().is_empty());
    }
    refresh_camera_at(&mut r, 1_000 + FIRST_FRAME_DEADLINE_US);
    let reads = r.state.borrow().reads;
    assert!(
        r.step()
            .unwrap_err()
            .to_string()
            .contains("FirstFrameTimeout")
    );
    assert_eq!(
        r.state.borrow().reads,
        reads,
        "deadline wins before another dequeue"
    );
    assert_eq!(r.state.borrow().stops, 1);
    assert!(r.state.borrow().revoked_before_stop);
    let revoked = r
        .link
        .events
        .iter()
        .find(|e| matches!(e, CameraEvent::Revoked { .. }))
        .unwrap();
    let value = serde_json::to_value(revoked).unwrap();
    assert_eq!(value["startup_discards"]["count"], 16);
    assert_eq!(value["startup_discards"]["first"]["age_us"], 300_000);
    assert_eq!(value["startup_discards"]["latest"]["age_us"], 300_000);
    assert_eq!(
        value["startup_discards"]["first"]["comparison"]["comparison_at_us"],
        401_000
    );
    assert_eq!(
        value["startup_discards"]["latest"]["comparison"]["comparison_at_us"],
        1_901_000
    );
    assert!(
        value.get("timestamp_rejection").is_none(),
        "startup discards are not a fatal timestamp record"
    );
    let mut report = MetadataReport::new(config(), "a".repeat(64), 5, 2_001_000).unwrap();
    let mut observer = Observer::new(boot(2), 2_001_000);
    assert!(
        observer
            .accept(revoked.clone(), None, 2_001_000, &mut report)
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(report).unwrap()["revoked_worker_event"]["startup_discards"],
        value["startup_discards"]
    );
    assert!(
        r.step().is_err(),
        "unavailable is terminal, never auto-reopened"
    );
}

#[test]
fn startup_draining_serves_privacy_and_stop_before_any_export_or_next_read() {
    for privacy in [false, true] {
        let mut r = Rig::new();
        r.start();
        for at in [200_000, 400_000] {
            refresh_camera_at(&mut r, at);
            r.step().unwrap();
        }
        r.clock.0.set(455_929);
        queue_monotonic_frame(&mut r, 55_000);
        let command = if privacy {
            Control::Authority {
                snapshot: r.authority(Permission::Denied),
            }
        } else {
            Control::Stop
        };
        r.state.borrow_mut().after_read.push_back(command);
        r.state.borrow_mut().payload_override = Some(
            [
                &[0xff, 0xd8],
                b"private-startup-pixels".as_slice(),
                &[0xff, 0xd9],
            ]
            .concat(),
        );
        let reads = r.state.borrow().reads;
        assert_eq!(r.step().unwrap(), Step::Stopped);
        assert_eq!(r.state.borrow().reads, reads + 1);
        assert!(r.frames().is_empty());
        assert_eq!(r.state.borrow().stops, 1);
        assert!(r.state.borrow().revoked_before_stop);
        let revoked = r
            .link
            .events
            .iter()
            .find(|e| matches!(e, CameraEvent::Revoked { .. }))
            .unwrap();
        let value = serde_json::to_value(revoked).unwrap();
        assert_eq!(
            value["reason"],
            if privacy {
                "privacy_changed"
            } else {
                "requested"
            }
        );
        assert!(value["error"].is_null());
        assert_eq!(value["startup_discards"]["count"], 1);
        assert_eq!(value["startup_discards"]["first"]["age_us"], 400_929);
        assert!(
            !serde_json::to_string(&r.link.events)
                .unwrap()
                .contains("private-startup-pixels")
        );
        assert!(r.step().is_err());
        assert_eq!(r.state.borrow().reads, reads + 1);
    }
}

#[test]
fn startup_discard_summary_reaches_progress_without_claiming_a_frame() {
    let mut r = Rig::new();
    r.start();
    for at in [200_000, 400_000] {
        refresh_camera_at(&mut r, at);
        r.step().unwrap();
    }
    r.clock.0.set(455_929);
    queue_monotonic_frame(&mut r, 55_000);
    r.step().unwrap();
    let progress = r
        .link
        .events
        .iter()
        .rev()
        .find(|e| matches!(e, CameraEvent::Progress { .. }))
        .unwrap();
    let value = serde_json::to_value(progress).unwrap();
    assert_eq!(value["counts"]["received"], 0);
    assert_eq!(value["startup_discards"]["count"], 1);
    assert_eq!(value["startup_discards"]["first"]["age_us"], 400_929);
    assert!(r.frames().is_empty());
    assert!(serde_json::to_vec(progress).unwrap().len() < 2500);
    // Old metadata without this optional diagnostic still decodes.
    let mut legacy = value.clone();
    legacy.as_object_mut().unwrap().remove("startup_discards");
    assert!(serde_json::from_value::<CameraEvent>(legacy).is_ok());
}
