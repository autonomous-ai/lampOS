//! Frame link over real Unix stream sockets in one process. Time is passed
//! explicitly to the server, so stall and hello deadlines are deterministic.
use lamp_camera::{
    CallTiming, CaptureId, FrameId, FrameObservation, NegotiatedMode, PixelFormat, SourceId,
    frames::{
        CloseReason, FrameClient, FrameServer, LinkEvent, MAX_REQUEST_AGE_US, Role,
        STALL_DEADLINE_US,
    },
};
use lamp_turn_owner::{BootId, CameraGrant, Controller, MonoTime, Permission};
use std::{
    fs,
    io::{Read, Write},
    num::NonZeroU64,
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/tmp/lcf-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    fn socket(&self) -> PathBuf {
        self.0.join("camera.frames")
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn boot(n: u8) -> BootId {
    BootId::new([n; 16]).unwrap()
}
const CONTROLLER: u8 = 1;
const CAMERA: u8 = 2;
const PERCEPTION: u8 = 3;
const CONTEXT: u8 = 4;

fn grant(now: u64) -> CameraGrant {
    let mut controller = Controller::new(boot(CONTROLLER), MonoTime::from_micros(0));
    controller
        .set_camera_permission(MonoTime::from_micros(now), Permission::Allowed)
        .unwrap();
    controller.camera_grant(MonoTime::from_micros(now)).unwrap()
}

fn observation(sequence: u64, dequeued_at_us: u64) -> FrameObservation {
    FrameObservation {
        id: FrameId {
            capture: CaptureId {
                worker: boot(CAMERA),
                epoch: NonZeroU64::new(1).unwrap(),
            },
            sequence: NonZeroU64::new(sequence).unwrap(),
        },
        source: SourceId::new("desk-camera").unwrap(),
        grant: grant(1),
        mode: NegotiatedMode {
            source: SourceId::new("desk-camera").unwrap(),
            format: PixelFormat::MJPG,
            width: 1280,
            height: 720,
            interval: None,
            size_image: 2 * 1024 * 1024,
            buffers: 2,
        },
        driver_sequence: sequence as u32,
        driver_timestamp: None,
        dequeue: CallTiming {
            started_at: MonoTime::from_micros(dequeued_at_us - 100),
            completed_at: MonoTime::from_micros(dequeued_at_us),
        },
    }
}

fn jpeg(len: usize, fill: u8) -> Vec<u8> {
    let mut bytes = vec![fill; len];
    bytes[..2].copy_from_slice(&[0xff, 0xd8]);
    bytes[len - 2..].copy_from_slice(&[0xff, 0xd9]);
    bytes
}

fn server(dir: &Dir, consumers: &[(Role, u8)]) -> FrameServer {
    let consumers: Vec<_> = consumers.iter().map(|(r, b)| (*r, boot(*b))).collect();
    FrameServer::bind(&dir.socket(), boot(CONTROLLER), &consumers, 2 * 1024 * 1024).unwrap()
}

fn client(dir: &Dir, role: Role, own: u8) -> FrameClient {
    FrameClient::new(
        &dir.socket(),
        role,
        boot(own),
        boot(CONTROLLER),
        2 * 1024 * 1024,
    )
}

/// Run the server and poll the client until a frame or the deadline.
fn pump(
    server: &mut FrameServer,
    client: &mut FrameClient,
    now_us: u64,
    out: &mut Vec<u8>,
) -> std::io::Result<Option<lamp_camera::frames::FrameMeta>> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        server.service(now_us);
        if let Some(meta) = client.poll(out)? {
            return Ok(Some(meta));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(None)
}

fn connect(server: &mut FrameServer, client: &mut FrameClient, role: Role) {
    assert!(client.connect().unwrap());
    let deadline = Instant::now() + Duration::from_secs(3);
    while !server.connected(role) && Instant::now() < deadline {
        server.service(10);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(server.connected(role), "consumer was not accepted");
}

/// Wait until the server has read the client's request.
fn await_request(server: &mut FrameServer) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !server.wants_frame() && Instant::now() < deadline {
        server.service(10);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(server.wants_frame());
}

#[test]
fn a_requested_frame_arrives_whole_with_its_original_identity_and_grant() {
    let dir = Dir::new();
    let mut server = server(&dir, &[(Role::Perception, PERCEPTION)]);
    let mut client = client(&dir, Role::Perception, PERCEPTION);
    connect(&mut server, &mut client, Role::Perception);
    assert!(!server.wants_frame(), "no request yet");
    assert_eq!(
        server
            .offer(&observation(1, 1_000), &jpeg(64, 7), 1_010)
            .unwrap(),
        0
    );
    let id = client.request(250_000).unwrap();
    await_request(&mut server);
    let bytes = jpeg(300_000, 9);
    assert_eq!(
        server.offer(&observation(5, 2_000), &bytes, 2_020).unwrap(),
        1
    );
    let mut out = Vec::new();
    let meta = pump(&mut server, &mut client, 2_030, &mut out)
        .unwrap()
        .unwrap();
    assert_eq!(out, bytes);
    assert_eq!(meta.request, id);
    assert_eq!(meta.frame_sequence, 5);
    assert_eq!(meta.camera_worker, boot(CAMERA));
    assert_eq!(meta.grant, grant(1));
    assert_eq!((meta.width, meta.height), (1280, 720));
    assert_eq!(meta.dequeue_completed_at_us, 2_000);
    assert_eq!(meta.copied_at_us, 2_020);
    assert_eq!(client.outstanding(), None);
    // The next frame needs a new request.
    assert!(!server.wants_frame());
    assert_eq!(server.counts().frames_sent, 1);
}

#[test]
fn only_the_permitted_consumer_identity_is_accepted_once() {
    let dir = Dir::new();
    let mut server = server(&dir, &[(Role::Perception, PERCEPTION)]);
    for (role, own, controller) in [
        (Role::Perception, CONTEXT, CONTROLLER),
        (Role::Context, CONTEXT, CONTROLLER),
        (Role::Perception, PERCEPTION, 9),
    ] {
        let mut stranger = FrameClient::new(&dir.socket(), role, boot(own), boot(controller), 1024);
        assert!(stranger.connect().unwrap());
        let mut out = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(3);
        let result = loop {
            server.service(10);
            match stranger.poll(&mut out) {
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                other => break other,
            }
        };
        assert!(result.is_err(), "rejected consumer must see the link close");
    }
    assert!(!server.connected(Role::Perception));
    assert_eq!(server.counts().rejected, 3);
    let mut good = client(&dir, Role::Perception, PERCEPTION);
    connect(&mut server, &mut good, Role::Perception);
    // While it is connected, a second live connection for the role is refused.
    let mut second = client(&dir, Role::Perception, PERCEPTION);
    assert!(second.connect().unwrap());
    for _ in 0..50 {
        server.service(10);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(server.connected(Role::Perception));
    assert_eq!(server.counts().rejected, 4);
    // After the consumer itself closes, the same identity may reconnect.
    good.disconnect();
    for _ in 0..50 {
        server.service(10);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!server.connected(Role::Perception));
    let mut again = client(&dir, Role::Perception, PERCEPTION);
    connect(&mut server, &mut again, Role::Perception);
    let closes: Vec<_> = std::iter::from_fn(|| server.take_event()).collect();
    assert!(closes.iter().any(|e| matches!(
        e,
        LinkEvent::Closed {
            reason: CloseReason::Rejected,
            ..
        }
    )));
    assert!(closes.iter().any(|e| matches!(
        e,
        LinkEvent::Closed {
            role: Some(Role::Perception),
            reason: CloseReason::ConsumerClosed,
            ..
        }
    )));
}

#[test]
fn a_protocol_violation_bans_the_role_for_the_producer_lifetime() {
    let dir = Dir::new();
    let mut server = server(&dir, &[(Role::Perception, PERCEPTION)]);
    // A consumer with the permitted identity sends two requests at once.
    let mut violator = UnixStream::connect(dir.socket()).unwrap();
    let mut hello = [0_u8; 40];
    hello[..4].copy_from_slice(b"LFH1");
    hello[4] = 1;
    hello[8..24].copy_from_slice(&[PERCEPTION; 16]);
    hello[24..40].copy_from_slice(&[CONTROLLER; 16]);
    violator.write_all(&hello).unwrap();
    let mut request = [0_u8; 16];
    request[..4].copy_from_slice(b"LFR1");
    request[4..8].copy_from_slice(&100_000_u32.to_le_bytes());
    for id in [1_u64, 2] {
        request[8..16].copy_from_slice(&id.to_le_bytes());
        violator.write_all(&request).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while server.counts().protocol_violations == 0 && Instant::now() < deadline {
        server.service(10);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(server.counts().protocol_violations, 1);
    // The same identity can no longer connect to this producer.
    let mut after = client(&dir, Role::Perception, PERCEPTION);
    assert!(after.connect().unwrap());
    for _ in 0..50 {
        server.service(10);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!server.connected(Role::Perception));
    assert!(std::iter::from_fn(|| server.take_event()).any(|e| matches!(
        e,
        LinkEvent::Closed {
            reason: CloseReason::Rejected,
            ..
        }
    )));
}

/// A frame larger than the socket buffers is abandoned mid-write: after
/// `discard_sending`, no further byte of it is written even when the
/// consumer drains, and the same consumer can reconnect for a new frame.
#[test]
fn discarding_an_unsent_tail_stops_the_frame_and_allows_reconnection() {
    let dir = Dir::new();
    let mut server = server(&dir, &[(Role::Context, CONTEXT)]);
    let mut client = client(&dir, Role::Context, CONTEXT);
    connect(&mut server, &mut client, Role::Context);
    client.request(250_000).unwrap();
    await_request(&mut server);
    server
        .offer(
            &observation(1, 1_000),
            &jpeg(2 * 1024 * 1024 - 4096, 5),
            1_000,
        )
        .unwrap();
    server.service(1_100);
    assert!(
        server.sending(Role::Context),
        "a 2 MiB frame cannot fit the socket buffers"
    );
    let sent_before = server.counts().bytes_sent;
    assert_eq!(server.discard_sending(CloseReason::PrivacyClosed, 1_200), 1);
    assert!(!server.sending(Role::Context));
    // The consumer drains what the kernel already holds, then sees the end.
    let mut out = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    let result = loop {
        server.service(1_300);
        match client.poll(&mut out) {
            Ok(None) if Instant::now() < deadline => continue,
            other => break other,
        }
    };
    assert!(result.is_err(), "the cut-off frame is never completed");
    assert_eq!(
        server.counts().bytes_sent,
        sent_before,
        "no byte after the discard"
    );
    assert_eq!(server.counts().partial_discarded, 1);
    // Not terminal: the same consumer reconnects and receives a whole frame.
    let mut again = FrameClient::new(
        &dir.socket(),
        Role::Context,
        boot(CONTEXT),
        boot(CONTROLLER),
        2 * 1024 * 1024,
    );
    connect(&mut server, &mut again, Role::Context);
    again.request(250_000).unwrap();
    await_request(&mut server);
    let small = jpeg(1000, 6);
    server.offer(&observation(2, 2_000), &small, 2_000).unwrap();
    let meta = pump(&mut server, &mut again, 2_100, &mut out)
        .unwrap()
        .unwrap();
    assert_eq!((meta.frame_sequence, out.len()), (2, 1000));
}

#[test]
fn a_frame_older_than_the_request_bound_is_not_sent() {
    let dir = Dir::new();
    let mut server = server(&dir, &[(Role::Context, CONTEXT)]);
    let mut client = client(&dir, Role::Context, CONTEXT);
    connect(&mut server, &mut client, Role::Context);
    client.request(50_000).unwrap();
    await_request(&mut server);
    assert_eq!(
        server
            .offer(&observation(1, 1_000), &jpeg(64, 1), 60_000)
            .unwrap(),
        0
    );
    assert_eq!(server.counts().too_old, 1);
    assert!(
        server.wants_frame(),
        "the request stays open for a newer frame"
    );
    assert_eq!(
        server
            .offer(&observation(2, 59_000), &jpeg(64, 2), 60_000)
            .unwrap(),
        1
    );
    let mut out = Vec::new();
    let meta = pump(&mut server, &mut client, 60_010, &mut out)
        .unwrap()
        .unwrap();
    assert_eq!(meta.frame_sequence, 2);
}

#[test]
fn protocol_violations_close_only_that_consumer() {
    let dir = Dir::new();
    let mut server = server(
        &dir,
        &[(Role::Perception, PERCEPTION), (Role::Context, CONTEXT)],
    );
    let mut good = client(&dir, Role::Context, CONTEXT);
    connect(&mut server, &mut good, Role::Context);
    // A raw consumer sends two requests without waiting for a frame.
    let mut raw = UnixStream::connect(dir.socket()).unwrap();
    let mut hello = [0_u8; 40];
    hello[..4].copy_from_slice(b"LFH1");
    hello[4] = 1;
    hello[8..24].copy_from_slice(&[PERCEPTION; 16]);
    hello[24..40].copy_from_slice(&[CONTROLLER; 16]);
    raw.write_all(&hello).unwrap();
    let mut request = [0_u8; 16];
    request[..4].copy_from_slice(b"LFR1");
    request[4..8].copy_from_slice(&100_000_u32.to_le_bytes());
    for id in [1_u64, 2] {
        request[8..16].copy_from_slice(&id.to_le_bytes());
        raw.write_all(&request).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while server.counts().protocol_violations == 0 && Instant::now() < deadline {
        server.service(10);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(server.counts().protocol_violations, 1);
    assert!(!server.connected(Role::Perception));
    assert!(
        server.connected(Role::Context),
        "the other consumer is unaffected"
    );
    // The server shut the stream down; the raw consumer reads end of file.
    let mut rest = Vec::new();
    assert_eq!(raw.read_to_end(&mut rest).unwrap_or(0), 0);
    // Request ages outside the bound are rejected by the client itself.
    assert!(good.request(0).is_err());
    assert!(good.request(MAX_REQUEST_AGE_US + 1).is_err());
}

#[test]
fn a_stalled_consumer_is_closed_without_blocking_and_its_partial_frame_is_discarded() {
    let dir = Dir::new();
    let mut server = server(&dir, &[(Role::Perception, PERCEPTION)]);
    let mut client = client(&dir, Role::Perception, PERCEPTION);
    connect(&mut server, &mut client, Role::Perception);
    client.request(250_000).unwrap();
    await_request(&mut server);
    // Larger than any socket buffer; the client never reads.
    let started = Instant::now();
    assert_eq!(
        server
            .offer(
                &observation(1, 1_000),
                &jpeg(2 * 1024 * 1024 - 4096, 3),
                1_000
            )
            .unwrap(),
        1
    );
    for step in 0..10 {
        server.service(1_000 + step * 1_000);
    }
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "offer or service blocked"
    );
    assert!(server.connected(Role::Perception));
    server.service(1_000 + STALL_DEADLINE_US);
    assert!(!server.connected(Role::Perception));
    assert_eq!(server.counts().stalled, 1);
    assert_eq!(server.counts().partial_discarded, 1);
    assert_eq!(server.counts().frames_sent, 0);
    // The client sees a cut-off frame as an error, never as a frame.
    let mut out = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    let result = loop {
        match client.poll(&mut out) {
            Ok(None) if Instant::now() < deadline => continue,
            other => break other,
        }
    };
    assert!(result.is_err(), "{result:?}");
    assert!(out.is_empty());
    assert!(!client.is_connected());
}

#[test]
fn privacy_close_mid_frame_never_completes_the_frame() {
    let dir = Dir::new();
    let mut server = server(&dir, &[(Role::Perception, PERCEPTION)]);
    let mut client = client(&dir, Role::Perception, PERCEPTION);
    connect(&mut server, &mut client, Role::Perception);
    client.request(250_000).unwrap();
    await_request(&mut server);
    server
        .offer(&observation(1, 1_000), &jpeg(1_500_000, 4), 1_000)
        .unwrap();
    // Read part of the frame before privacy closes.
    let mut out = Vec::new();
    assert!(client.poll(&mut out).unwrap().is_none());
    server.close_all(CloseReason::PrivacyClosed, 2_000);
    let event = std::iter::from_fn(|| server.take_event()).last().unwrap();
    assert!(matches!(
        event,
        LinkEvent::Closed {
            reason: CloseReason::PrivacyClosed,
            partial_frame_discarded: true,
            ..
        }
    ));
    let deadline = Instant::now() + Duration::from_secs(3);
    let result = loop {
        match client.poll(&mut out) {
            Ok(None) if Instant::now() < deadline => continue,
            other => break other,
        }
    };
    assert!(result.is_err());
    assert!(out.is_empty());
    // A closed server accepts and offers nothing further.
    assert!(!server.wants_frame());
    assert_eq!(
        server
            .offer(&observation(2, 3_000), &jpeg(64, 1), 3_000)
            .unwrap(),
        0
    );
    let mut late = client_after_close(&dir);
    assert!(late.connect().unwrap_or(false));
    server.service(4_000);
    assert!(!server.connected(Role::Perception));
}

fn client_after_close(dir: &Dir) -> FrameClient {
    client(dir, Role::Perception, PERCEPTION)
}

#[test]
fn the_client_rejects_unrequested_or_malformed_frames() {
    let dir = Dir::new();
    let listener = std::os::unix::net::UnixListener::bind(dir.socket()).unwrap();
    let mut client = client(&dir, Role::Perception, PERCEPTION);
    assert!(client.connect().unwrap());
    let (mut fake, _) = listener.accept().unwrap();
    let mut hello = [0; 40];
    fake.read_exact(&mut hello).unwrap();
    let id = client.request(100_000).unwrap();
    let mut request = [0; 16];
    fake.read_exact(&mut request).unwrap();
    // A frame for a different request id.
    let mut header = Vec::new();
    header.extend_from_slice(b"LFF1");
    header.extend_from_slice(&(id + 1).to_le_bytes());
    header.extend_from_slice(&10_u16.to_le_bytes());
    header.extend_from_slice(&[0, 0]);
    header.extend_from_slice(&4_u32.to_le_bytes());
    fake.write_all(&header).unwrap();
    let mut out = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    let result = loop {
        match client.poll(&mut out) {
            Ok(None) if Instant::now() < deadline => continue,
            other => break other,
        }
    };
    assert!(result.unwrap_err().to_string().contains("unrequested"));
    assert!(!client.is_connected());
}

#[test]
fn binding_requires_a_private_directory_and_removes_only_its_own_socket() {
    let dir = Dir::new();
    fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o755)).unwrap();
    let error = FrameServer::bind(
        &dir.socket(),
        boot(CONTROLLER),
        &[(Role::Perception, boot(PERCEPTION))],
        1024,
    )
    .err()
    .unwrap();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o700)).unwrap();
    for bad in [
        vec![],
        vec![(Role::Perception, boot(3)), (Role::Perception, boot(4))],
    ] {
        assert!(FrameServer::bind(&dir.socket(), boot(CONTROLLER), &bad, 1024).is_err());
    }
    let server = server(&dir, &[(Role::Perception, PERCEPTION)]);
    let mode = fs::metadata(dir.socket()).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    drop(server);
    assert!(!dir.socket().exists());
}

/// A producer whose listen queue is full (it never accepts) must not block a
/// consumer's connection attempt; once it accepts, the consumer connects.
#[test]
fn connecting_never_blocks_on_a_full_listen_queue() {
    use rustix::net::{AddressFamily, SocketAddrUnix, SocketType};
    let dir = Dir::new();
    let address = SocketAddrUnix::new(dir.socket()).unwrap();
    let listener = rustix::net::socket(AddressFamily::UNIX, SocketType::STREAM, None).unwrap();
    rustix::net::bind(&listener, &address).unwrap();
    rustix::net::listen(&listener, 0).unwrap();
    // Fill the queue with nonblocking connections until the kernel refuses.
    let mut fillers = Vec::new();
    for _ in 0..256 {
        let socket = rustix::net::socket(AddressFamily::UNIX, SocketType::STREAM, None).unwrap();
        rustix::io::ioctl_fionbio(&socket, true).unwrap();
        match rustix::net::connect(&socket, &address) {
            Ok(()) | Err(rustix::io::Errno::INPROGRESS) => fillers.push(socket),
            Err(_) => break,
        }
    }
    let mut client = client(&dir, Role::Perception, PERCEPTION);
    let started = Instant::now();
    for _ in 0..20 {
        let _ = client.connect();
    }
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "connection attempts blocked for {:?}",
        started.elapsed()
    );
    // Accepting everything queued lets a later attempt through.
    rustix::io::ioctl_fionbio(&listener, true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut accepted = Vec::new();
    let connected = loop {
        while let Ok(socket) = rustix::net::accept(&listener) {
            accepted.push(socket);
        }
        match client.connect() {
            Ok(true) => break true,
            Ok(false) | Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(2))
            }
            _ => break false,
        }
    };
    assert!(connected);
    drop(fillers);
}

/// The freshness contract: a frame dequeued before the request is served if,
/// at copy time, it is within the request's age bound; one just past the
/// bound is not.
#[test]
fn freshness_is_bounded_age_at_copy_time_not_capture_after_request() {
    let dir = Dir::new();
    let mut server = server(&dir, &[(Role::Perception, PERCEPTION)]);
    let mut client = client(&dir, Role::Perception, PERCEPTION);
    connect(&mut server, &mut client, Role::Perception);
    // The frame was dequeued at t=10 ms; the request is sent "later".
    let frame = observation(1, 10_000);
    client.request(50_000).unwrap();
    await_request(&mut server);
    // Copy at t=60 ms: exactly 50 ms old, within the bound.
    assert_eq!(server.offer(&frame, &jpeg(64, 1), 60_000).unwrap(), 1);
    let mut out = Vec::new();
    let meta = pump(&mut server, &mut client, 60_000, &mut out)
        .unwrap()
        .unwrap();
    assert_eq!(meta.dequeue_completed_at_us, 10_000);
    assert_eq!(meta.copied_at_us - meta.dequeue_completed_at_us, 50_000);
    // One microsecond older is refused.
    client.request(50_000).unwrap();
    await_request(&mut server);
    assert_eq!(
        server
            .offer(&observation(2, 10_000), &jpeg(64, 1), 60_001)
            .unwrap(),
        0
    );
    assert_eq!(server.counts().too_old, 1);
}
