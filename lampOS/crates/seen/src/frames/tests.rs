use super::*;
use lamp_camera::{
    CallTiming, CaptureId, FrameId, FrameObservation, NegotiatedMode, PixelFormat, SourceId,
    frames::{CloseReason, FrameServer, Role},
};
use lamp_turn_owner::{BootId, Controller, MonoTime, Permission};
use std::collections::VecDeque;
use std::num::NonZeroU64;

fn boot(n: u8) -> BootId {
    BootId::new([n; 16]).unwrap()
}

/// Grants of one controller: `generation` 1 is the first open, each later
/// one another close and open of the privacy.
fn grant(generation: u64) -> CameraGrant {
    let mut controller = Controller::new(boot(1), MonoTime::from_micros(0));
    let mut grant = None;
    for step in 0..generation {
        let at = MonoTime::from_micros(10 + step * 10);
        if step > 0 {
            controller
                .set_camera_permission(at, Permission::Denied)
                .unwrap();
        }
        controller
            .set_camera_permission(MonoTime::from_micros(15 + step * 10), Permission::Allowed)
            .unwrap();
        grant = Some(
            controller
                .camera_grant(MonoTime::from_micros(15 + step * 10))
                .unwrap(),
        );
    }
    grant.unwrap()
}

fn observation(sequence: u64, grant: CameraGrant) -> FrameObservation {
    FrameObservation {
        id: FrameId {
            capture: CaptureId {
                worker: boot(2),
                epoch: NonZeroU64::new(1).unwrap(),
            },
            sequence: NonZeroU64::new(sequence).unwrap(),
        },
        source: SourceId::new("desk-camera").unwrap(),
        grant,
        mode: NegotiatedMode {
            source: SourceId::new("desk-camera").unwrap(),
            format: PixelFormat::MJPG,
            width: 640,
            height: 480,
            interval: None,
            size_image: 64 * 1024,
            buffers: 2,
        },
        driver_sequence: sequence as u32,
        driver_timestamp: None,
        dequeue: CallTiming {
            started_at: MonoTime::from_micros(900),
            completed_at: MonoTime::from_micros(1_000),
        },
    }
}

fn meta(grant: CameraGrant) -> FrameMeta {
    FrameMeta::new(&observation(1, grant), 1, 1_000)
}

fn jpeg(len: usize) -> Vec<u8> {
    let mut bytes = vec![7; len];
    bytes[..2].copy_from_slice(&[0xff, 0xd8]);
    bytes[len - 2..].copy_from_slice(&[0xff, 0xd9]);
    bytes
}

/// The camera's end, as told.
#[derive(Default)]
struct Fake {
    connected: bool,
    refuse: bool,
    outstanding: Option<u64>,
    requests: u64,
    frames: VecDeque<io::Result<FrameMeta>>,
    disconnects: u64,
}

impl Source for Fake {
    fn connected(&self) -> bool {
        self.connected
    }
    fn connect(&mut self) -> io::Result<bool> {
        self.connected = !self.refuse;
        Ok(self.connected)
    }
    fn outstanding(&self) -> Option<u64> {
        self.outstanding
    }
    fn request(&mut self, _: u32) -> io::Result<u64> {
        assert!(self.outstanding.is_none(), "one request at a time");
        assert!(self.connected);
        self.requests += 1;
        self.outstanding = Some(self.requests);
        Ok(self.requests)
    }
    fn poll(&mut self, jpeg_out: &mut Vec<u8>) -> io::Result<Option<FrameMeta>> {
        if self.outstanding.is_none() {
            return Ok(None);
        }
        match self.frames.pop_front() {
            None => Ok(None),
            Some(Ok(meta)) => {
                *jpeg_out = jpeg(64);
                self.outstanding = None;
                Ok(Some(meta))
            }
            Some(Err(error)) => {
                self.disconnect();
                Err(error)
            }
        }
    }
    fn disconnect(&mut self) {
        self.connected = false;
        self.outstanding = None;
        self.disconnects += 1;
    }
}

/// A sink that keeps how many frames and bytes it was lent, never the bytes.
#[derive(Default)]
struct Count {
    frames: usize,
    bytes: usize,
}

impl Sink for Count {
    fn frame(&mut self, _: &FrameMeta, jpeg: &[u8]) {
        self.frames += 1;
        self.bytes += jpeg.len();
    }
}

#[test]
fn a_closed_gate_asks_nothing_and_holds_nothing() {
    let mut taker = Taker::new(Fake::default());
    let mut sink = Count::default();
    for step in 0..5 {
        assert_eq!(
            taker.step(Err(Closed::NoOptIn), step * EVERY_US, &mut sink),
            Step::Idle
        );
    }
    assert!(!taker.holds_anything());
    assert_eq!(taker.source.requests, 0, "nothing asked of the camera");
    assert_eq!(sink.frames, 0);
}

#[test]
fn an_open_gate_asks_once_copies_hands_and_wipes() {
    let mut taker = Taker::new(Fake::default());
    let mut sink = Count::default();
    assert_eq!(taker.step(Ok(()), 0, &mut sink), Step::Requested);
    // One request at a time: no second while one is out.
    assert_eq!(taker.step(Ok(()), 1, &mut sink), Step::Idle);
    assert_eq!(taker.source.requests, 1);
    taker.source.frames.push_back(Ok(meta(grant(1))));
    assert_eq!(taker.step(Ok(()), 2, &mut sink), Step::Handed);
    assert_eq!((sink.frames, sink.bytes), (1, 64));
    assert!(taker.jpeg.is_empty(), "the copy is wiped once handed");
    // The next request waits its time.
    assert_eq!(taker.step(Ok(()), 3, &mut sink), Step::Idle);
    assert_eq!(taker.step(Ok(()), 2 + EVERY_US, &mut sink), Step::Requested);
    assert_eq!(taker.counts().handed, 1);
    assert_eq!(taker.counts().requested, 2);
}

#[test]
fn the_gate_closing_with_a_request_out_drops_everything() {
    for closed in [
        Closed::NoOptIn,
        Closed::PrivacyClosed,
        Closed::PrivacyUnknown,
        Closed::Nobody,
        Closed::Stranger,
        Closed::Asleep,
    ] {
        let mut taker = Taker::new(Fake::default());
        let mut sink = Count::default();
        assert_eq!(taker.step(Ok(()), 0, &mut sink), Step::Requested);
        taker.source.frames.push_back(Ok(meta(grant(1))));
        // The frame is never read: the gate closed first.
        assert_eq!(taker.step(Err(closed), 1, &mut sink), Step::Dropped(closed));
        assert!(!taker.holds_anything(), "{closed:?}");
        assert_eq!(sink.frames, 0, "{closed:?}");
        assert_eq!(taker.source.disconnects, 1);
    }
}

#[test]
fn a_frame_under_another_grant_is_dropped_with_everything() {
    let mut taker = Taker::new(Fake::default());
    let mut sink = Count::default();
    taker.step(Ok(()), 0, &mut sink);
    taker.source.frames.push_back(Ok(meta(grant(1))));
    assert_eq!(taker.step(Ok(()), 1, &mut sink), Step::Handed);
    taker.step(Ok(()), 1 + EVERY_US, &mut sink);
    // The privacy closed and opened again between: a new grant.
    taker.source.frames.push_back(Ok(meta(grant(2))));
    assert_ne!(grant(1), grant(2));
    assert_eq!(
        taker.step(Ok(()), 2 + EVERY_US, &mut sink),
        Step::GrantChanged
    );
    assert_eq!(sink.frames, 1, "the second frame never reaches the sink");
    assert!(!taker.holds_anything());
    assert!(taker.jpeg.is_empty());
}

#[test]
fn a_link_that_fails_drops_everything() {
    let mut taker = Taker::new(Fake::default());
    let mut sink = Count::default();
    taker.step(Ok(()), 0, &mut sink);
    taker.source.frames.push_back(Ok(meta(grant(1))));
    taker.step(Ok(()), 1, &mut sink);
    taker.step(Ok(()), 1 + EVERY_US, &mut sink);
    taker.source.frames.push_back(Err(io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "camera closed the frame link",
    )));
    assert_eq!(taker.step(Ok(()), 2 + EVERY_US, &mut sink), Step::Lost);
    assert!(!taker.holds_anything());
    assert_eq!(taker.counts().lost, 1);
}

#[test]
fn no_camera_listening_asks_nothing() {
    let mut taker = Taker::new(Fake {
        refuse: true,
        ..Fake::default()
    });
    let mut sink = Count::default();
    assert_eq!(taker.step(Ok(()), 0, &mut sink), Step::Idle);
    assert_eq!(taker.source.requests, 0);
}

/// Through the camera's own frame link: a frame copied and dropped, and
/// the camera's privacy close dropping everything here.
#[test]
fn through_the_cameras_frame_link() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::path::PathBuf::from(format!("/tmp/lsf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = dir.join("camera.frames");
    let mut server =
        FrameServer::bind(&socket, boot(1), &[(Role::Perception, boot(3))], 64 * 1024).unwrap();
    let client = FrameClient::new(&socket, Role::Perception, boot(3), boot(1), 64 * 1024);
    let mut taker = Taker::new(client);
    let mut sink = Count::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut handed = false;
    let mut now = 1_000;
    while std::time::Instant::now() < deadline && !handed {
        server.service(now);
        if server.wants_frame() {
            server
                .offer(&observation(1, grant(1)), &jpeg(4096), now)
                .unwrap();
        }
        handed = taker.step(Ok(()), now, &mut sink) == Step::Handed;
        now += 1_000;
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(handed, "a frame came through");
    assert_eq!((sink.frames, sink.bytes), (1, 4096));
    assert!(taker.jpeg.is_empty());
    // The camera's privacy closes: every consumer is closed, and here
    // everything is let go.
    server.close_all(CloseReason::PrivacyClosed, now);
    let mut lost = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline && !lost {
        now += 1_000;
        lost = taker.step(Ok(()), now + EVERY_US, &mut sink) == Step::Lost;
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(lost, "the close reached the client");
    assert_eq!(sink.frames, 1);
    drop(server);
    let _ = std::fs::remove_dir_all(dir);
}
