use super::*;
use lamp_camera::frames::FrameServer;
use lamp_turn_owner::{Controller, Permission};
use std::{
    cell::{Cell, RefCell},
    fs,
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicU64, Ordering},
};

// Process ownership: this test process writes and reads this fixture state; not IPC.
static NEXT: AtomicU64 = AtomicU64::new(0);
fn boot(n: u8) -> BootId {
    BootId::new([n; 16]).unwrap()
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let p = PathBuf::from(format!(
            "/tmp/lcv-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        Self(p)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[derive(Default)]
struct Sink {
    offers: RefCell<Vec<(u64, u64, u64, usize)>>,
    revocations: RefCell<Vec<u64>>,
    connected: Cell<bool>,
    inflight: Cell<bool>,
}
impl Sink {
    fn live() -> Self {
        Self {
            connected: Cell::new(true),
            ..Self::default()
        }
    }
}
impl ImageSink for Sink {
    fn connected_for(&self, _: RequestId) -> bool {
        self.connected.get()
    }
    fn offer(
        &self,
        r: RequestId,
        f: u64,
        g: u64,
        _: Instant,
        j: &[u8],
    ) -> Result<ImageOffer, ImageRefusal> {
        self.offers.borrow_mut().push((r.get(), f, g, j.len()));
        Ok(ImageOffer::Queued)
    }
    fn revoke(&self, g: u64) -> ImageRevocation {
        self.revocations.borrow_mut().push(g);
        if self.inflight.get() {
            ImageRevocation::AbandonedInFlight
        } else {
            ImageRevocation::DroppedUnsent
        }
    }
}
fn turn(r: u64, s: u64) -> Turn {
    Turn {
        request: RequestId::new(r).unwrap(),
        session: SessionId::new(s).unwrap(),
    }
}
fn source(dir: &Directory) -> Source {
    Source {
        socket: dir.0.join(FRAME_SOCKET),
        camera: boot(2),
        source: "desk".into(),
        width: 320,
        height: 240,
        max_frame_bytes: 128 * 1024,
    }
}
fn setup(dir: &Directory) -> (ContextDelivery, Controller, BoundaryGuard, Sink) {
    let mut controller = Controller::new(boot(1), MonoTime::from_micros(0));
    controller
        .set_camera_permission(MonoTime::from_micros(1), Permission::Allowed)
        .unwrap();
    let mut guard = BoundaryGuard::new(boot(1), MonoTime::from_micros(0));
    guard
        .install(
            MonoTime::from_micros(2),
            controller.snapshot(MonoTime::from_micros(2)).unwrap(),
        )
        .unwrap();
    let sink = Sink::live();
    let mut context = ContextDelivery::new(source(dir), boot(1), boot(3)).unwrap();
    context.authority(&mut guard, &sink, 3);
    (context, controller, guard, sink)
}
fn frame(c: &mut ContextDelivery, controller: &mut Controller, bytes: usize) {
    c.attempt = Some(Attempt {
        turn: turn(1, 7),
        started_at_us: 10,
        requested: true,
    });
    c.highest_request = 1;
    c.frame = Some(FrameMeta {
        request: 1,
        camera_worker: boot(2),
        capture_epoch: 1,
        frame_sequence: 9,
        grant: controller.camera_grant(MonoTime::from_micros(10)).unwrap(),
        source: "desk".into(),
        format: *b"MJPG",
        width: 320,
        height: 240,
        driver_sequence: 1,
        driver_timestamp: None,
        dequeue_started_at_us: 8,
        dequeue_completed_at_us: 9,
        copied_at_us: 10,
        derived: None,
    });
    c.jpeg = vec![0; bytes];
    c.jpeg[..2].copy_from_slice(&[255, 216]);
    c.jpeg[bytes - 2..].copy_from_slice(&[255, 217]);
}
#[test]
fn valid_frame_is_offered_once_under_original_session_not_replayed() {
    let dir = Directory::new();
    let (mut c, mut owner, mut guard, sink) = setup(&dir);
    frame(&mut c, &mut owner, 128);
    c.export_at(Some(turn(1, 7)), &mut guard, &sink, 12, 13, Instant::now());
    assert_eq!(*sink.offers.borrow(), vec![(1, 9, 1, 128)]);
    let receipt = c.receipt.as_ref().unwrap();
    assert_eq!(
        (receipt.request, receipt.session, receipt.outcome),
        (1, 7, Outcome::Queued)
    );
    c.poll(Some(turn(1, 8)), &sink, 14);
    assert!(c.attempt.is_none());
    assert_eq!(sink.offers.borrow().len(), 1);
}
#[test]
fn permission_changed_after_read_revokes_and_does_not_export() {
    let dir = Directory::new();
    let (mut c, mut owner, mut guard, sink) = setup(&dir);
    frame(&mut c, &mut owner, 128);
    owner
        .set_camera_permission(MonoTime::from_micros(11), Permission::Denied)
        .unwrap();
    guard
        .install(
            MonoTime::from_micros(12),
            owner.snapshot(MonoTime::from_micros(12)).unwrap(),
        )
        .unwrap();
    c.export_at(Some(turn(1, 7)), &mut guard, &sink, 13, 14, Instant::now());
    assert!(sink.offers.borrow().is_empty());
    assert_eq!(*sink.revocations.borrow(), vec![1]);
    assert_eq!(c.receipt.as_ref().unwrap().outcome, Outcome::PrivacyClosed);
}
#[test]
fn partial_cloud_write_revocation_reports_connection_abandonment() {
    let dir = Directory::new();
    let (mut c, mut owner, mut guard, sink) = setup(&dir);
    frame(&mut c, &mut owner, 128);
    c.export_at(Some(turn(1, 7)), &mut guard, &sink, 12, 13, Instant::now());
    sink.inflight.set(true);
    owner
        .set_camera_permission(MonoTime::from_micros(14), Permission::Denied)
        .unwrap();
    guard
        .install(
            MonoTime::from_micros(15),
            owner.snapshot(MonoTime::from_micros(15)).unwrap(),
        )
        .unwrap();
    c.authority(&mut guard, &sink, 16);
    assert_eq!(
        c.receipt.as_ref().unwrap().outcome,
        Outcome::RevokedInFlight
    );
}
#[test]
fn stale_wrong_camera_grant_size_future_and_session_fail_closed() {
    for case in 0..7 {
        let dir = Directory::new();
        let (mut c, mut owner, mut guard, sink) = setup(&dir);
        frame(&mut c, &mut owner, 128);
        let mut candidate = Some(turn(1, 7));
        match case {
            0 => c.frame.as_mut().unwrap().camera_worker = boot(9),
            1 => c.frame.as_mut().unwrap().copied_at_us = 99,
            2 => c.jpeg.resize(lamp_gemini::MAX_IMAGE_BYTES + 1, 0),
            3 => candidate = Some(turn(1, 8)),
            4 => sink.connected.set(false),
            5 => c.frame.as_mut().unwrap().width = 1080,
            _ => {
                owner
                    .set_camera_permission(MonoTime::from_micros(11), Permission::Denied)
                    .unwrap();
                owner
                    .set_camera_permission(MonoTime::from_micros(12), Permission::Allowed)
                    .unwrap();
                guard
                    .install(
                        MonoTime::from_micros(13),
                        owner.snapshot(MonoTime::from_micros(13)).unwrap(),
                    )
                    .unwrap();
            }
        }
        c.export_at(candidate, &mut guard, &sink, 14, 15, Instant::now());
        assert!(sink.offers.borrow().is_empty(), "case {case}");
        assert!(c.attempt.is_none());
    }
}
#[test]
fn expired_authority_and_input_end_do_not_wait_or_refresh_image() {
    for ended in [false, true] {
        let dir = Directory::new();
        let (mut c, mut owner, mut guard, sink) = setup(&dir);
        frame(&mut c, &mut owner, 128);
        if ended {
            c.export_at(None, &mut guard, &sink, 12, 13, Instant::now());
        } else {
            c.export_at(
                Some(turn(1, 7)),
                &mut guard,
                &sink,
                300_000,
                300_001,
                Instant::now(),
            );
        }
        assert!(sink.offers.borrow().is_empty());
        assert!(c.attempt.is_none());
    }
}
#[test]
fn conservative_clock_conversion_never_restamps_stale_frame() {
    let now = Instant::now();
    assert_eq!(
        now.duration_since(conservative_capture_time(200, 210, now, 100).unwrap()),
        Duration::from_micros(110)
    );
    assert!(conservative_capture_time(200, 199, now, 100).is_none());
    assert!(conservative_capture_time(200, 10201, now, 100).is_none());
    assert!(conservative_capture_time(200, 210, now, 201).is_none());
}
#[test]
fn camera_unavailable_has_one_bounded_attempt_and_receipt_slot() {
    let dir = Directory::new();
    let (mut c, _owner, mut guard, sink) = setup(&dir);
    c.poll(Some(turn(1, 7)), &sink, 10);
    c.poll(Some(turn(1, 7)), &sink, MAX_ATTEMPT_US + 10);
    assert_eq!(c.receipt.as_ref().unwrap().outcome, Outcome::Expired);
    c.poll(Some(turn(1, 7)), &sink, MAX_ATTEMPT_US + 11);
    assert!(c.attempt.is_none());
    c.send_receipt(|_| Err(io::ErrorKind::WouldBlock.into()))
        .unwrap();
    assert!(c.receipt.is_some());
    c.authority(&mut guard, &sink, MAX_ATTEMPT_US + 12);
    assert!(sink.offers.borrow().is_empty());
}
#[test]
fn real_frame_link_reads_then_rechecks_privacy_before_offer() {
    let dir = Directory::new();
    let (mut c, mut owner, mut guard, sink) = setup(&dir);
    let mut server = FrameServer::bind(
        &source(&dir).socket,
        boot(1),
        &[(Role::Context, boot(3))],
        128 * 1024,
    )
    .unwrap();
    c.poll(Some(turn(1, 7)), &sink, 10);
    for _ in 0..8 {
        server.service(11);
        c.poll(Some(turn(1, 7)), &sink, 11);
    }
    assert!(server.wants_frame());
    frame(&mut c, &mut owner, 128);
    let meta = c.frame.take().unwrap();
    let jpeg = c.jpeg.clone();
    c.jpeg.clear();
    server.offer_meta(&meta, &jpeg, 12).unwrap();
    for _ in 0..8 {
        server.service(13);
        c.poll(Some(turn(1, 7)), &sink, 13);
    }
    assert!(c.frame.is_some());
    owner
        .set_camera_permission(MonoTime::from_micros(14), Permission::Denied)
        .unwrap();
    guard
        .install(
            MonoTime::from_micros(15),
            owner.snapshot(MonoTime::from_micros(15)).unwrap(),
        )
        .unwrap();
    c.export_at(Some(turn(1, 7)), &mut guard, &sink, 16, 17, Instant::now());
    assert!(sink.offers.borrow().is_empty());
}

#[test]
fn a_new_question_cannot_reuse_an_already_offered_frame_or_change_camera_epoch() {
    for change_epoch in [false, true] {
        let dir = Directory::new();
        let (mut c, mut owner, mut guard, sink) = setup(&dir);
        frame(&mut c, &mut owner, 128);
        c.export_at(Some(turn(1, 7)), &mut guard, &sink, 12, 13, Instant::now());
        frame(&mut c, &mut owner, 128);
        c.attempt.as_mut().unwrap().turn = turn(2, 7);
        c.highest_request = 2;
        if change_epoch {
            c.frame.as_mut().unwrap().capture_epoch = 2;
            c.frame.as_mut().unwrap().frame_sequence = 10;
        }
        c.export_at(Some(turn(2, 7)), &mut guard, &sink, 14, 15, Instant::now());
        assert_eq!(sink.offers.borrow().len(), 1);
        assert_eq!(c.receipt.as_ref().unwrap().outcome, Outcome::InvalidFrame);
    }
}

#[test]
fn original_attempt_deadline_is_rechecked_after_frame_read_and_control_drain() {
    for age in [MAX_ATTEMPT_US - 1, MAX_ATTEMPT_US, MAX_ATTEMPT_US + 1] {
        let dir = Directory::new();
        let (mut context, mut owner, mut guard, sink) = setup(&dir);
        frame(&mut context, &mut owner, 128);
        let meta = context.frame.as_mut().unwrap();
        meta.dequeue_started_at_us = 249_900;
        meta.dequeue_completed_at_us = 249_901;
        meta.copied_at_us = 249_902;
        // Same grant, request and session; only its authority lease is fresh.
        guard
            .install(
                MonoTime::from_micros(249_950),
                owner.snapshot(MonoTime::from_micros(249_950)).unwrap(),
            )
            .unwrap();
        let polled_at = 10 + MAX_ATTEMPT_US - 1;
        context.poll(Some(turn(1, 7)), &sink, polled_at);
        assert!(context.attempt.is_some());
        assert!(context.frame.is_some());
        let exported_at = 10 + age;
        context.export_at(
            Some(turn(1, 7)),
            &mut guard,
            &sink,
            polled_at.max(exported_at - 1),
            exported_at,
            Instant::now(),
        );
        let expired = age >= MAX_ATTEMPT_US;
        assert_eq!(sink.offers.borrow().is_empty(), expired, "age {age}");
        assert_eq!(
            context.receipt.as_ref().unwrap().outcome,
            if expired {
                Outcome::Expired
            } else {
                Outcome::Queued
            },
        );
        assert!(context.attempt.is_none());
        assert!(context.frame.is_none());
        assert!(context.jpeg.is_empty());
        let offers = sink.offers.borrow().len();
        context.poll(Some(turn(1, 7)), &sink, exported_at + 1);
        assert!(
            context.attempt.is_none(),
            "completed attempt must not restart"
        );
        assert_eq!(sink.offers.borrow().len(), offers);
    }
}
