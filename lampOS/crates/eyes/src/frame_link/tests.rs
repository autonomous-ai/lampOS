//! The eyes' frame link over real Unix sockets, with `lamp-seen`'s client
//! (`lamp_camera::frames::FrameClient`, the `perception` role). Time is
//! passed explicitly, so the stall deadline is deterministic.
use super::*;
use lamp_camera::frames::{FrameClient, STALL_DEADLINE_US};
use std::{
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        let path = PathBuf::from(format!(
            "/tmp/lef-{}-{}",
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
const EYES: u8 = 2;
const READER: u8 = 3;
const MAX: usize = 2 * 1024 * 1024;

fn config(dir: &Dir) -> Config {
    Config {
        path: dir.socket(),
        controller: boot(CONTROLLER),
        reader: boot(READER),
    }
}

/// A link that lets any peer in: no test process is `lamp-seen.service`.
fn link(dir: &Dir) -> FrameLink {
    FrameLink::new_allowing(config(dir), MAX, 1, Arc::new(|_| true)).unwrap()
}

/// Turn the link until `client` connects, then a while longer, and say
/// whether the link closed it.
fn refused(link: &mut FrameLink, client: &mut FrameClient) -> bool {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !client.connect().unwrap() {
        assert!(Instant::now() < deadline);
        link.turn(Some(1), 10).unwrap();
    }
    let mut out = Vec::new();
    loop {
        link.turn(Some(1), 20).unwrap();
        match client.poll(&mut out) {
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(1)),
            Ok(None) => return false,
            Ok(Some(_)) => panic!("a frame no one asked for"),
            Err(_) => return true,
        }
    }
}

fn client(dir: &Dir) -> FrameClient {
    FrameClient::new(
        &dir.socket(),
        Role::Perception,
        boot(READER),
        boot(CONTROLLER),
        MAX,
    )
}

fn jpeg(len: usize, fill: u8) -> Vec<u8> {
    let mut bytes = vec![fill; len];
    bytes[..2].copy_from_slice(&[0xff, 0xd8]);
    bytes[len - 2..].copy_from_slice(&[0xff, 0xd9]);
    bytes
}

fn read(sequence: u64, at_us: u64) -> Read {
    Read {
        worker: boot(EYES),
        epoch: 1,
        sequence,
        width: 640,
        height: 480,
        driver_sequence: sequence as u32,
        driver_timestamp: None,
        dequeue_started_us: at_us.saturating_sub(100),
        dequeue_completed_us: at_us,
    }
}

/// Turn the link until the client is connected and its request is read.
fn connect_and_ask(link: &mut FrameLink, client: &mut FrameClient, hold: u64, now_us: u64) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !client.connect().unwrap() {
        assert!(Instant::now() < deadline, "the client did not connect");
        link.turn(Some(hold), now_us).unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    while !link.connected() {
        assert!(Instant::now() < deadline, "the reader was not accepted");
        link.turn(Some(hold), now_us).unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    client.request(500_000).unwrap();
    while !link.wants_frame() {
        assert!(Instant::now() < deadline, "the request was not read");
        link.turn(Some(hold), now_us).unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn pump(
    link: &mut FrameLink,
    client: &mut FrameClient,
    hold: Option<u64>,
    now_us: u64,
    out: &mut Vec<u8>,
) -> io::Result<Option<FrameMeta>> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        link.turn(hold, now_us).unwrap();
        if let Some(meta) = client.poll(out)? {
            return Ok(Some(meta));
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(None)
}

#[test]
fn nothing_is_bound_or_served_until_the_permission_holds() {
    let dir = Dir::new();
    let mut link = link(&dir);
    // The start's check left nothing behind.
    assert!(!dir.socket().exists());
    link.turn(None, 10).unwrap();
    assert!(!link.bound());
    assert!(!dir.socket().exists());
    let mut client = client(&dir);
    assert!(!client.connect().unwrap(), "nothing listens while closed");
    assert!(!link.wants_frame());
    let offered = link.offer(None, &read(1, 10), &jpeg(64, 1), 10).unwrap();
    assert!(!offered);
    link.turn(Some(1), 20).unwrap();
    assert!(link.bound());
    assert!(dir.socket().exists());
}

#[test]
fn a_reader_gets_each_frame_with_its_grant() {
    let dir = Dir::new();
    let mut link = link(&dir);
    let mut client = client(&dir);
    link.turn(Some(1), 10).unwrap();
    let grant = link.grant().expect("bound with a grant");
    connect_and_ask(&mut link, &mut client, 1, 10);
    let bytes = jpeg(48 * 1024, 7);
    assert!(link.offer(Some(1), &read(5, 100), &bytes, 120).unwrap());
    let mut out = Vec::new();
    let meta = pump(&mut link, &mut client, Some(1), 130, &mut out)
        .unwrap()
        .expect("a frame");
    assert_eq!(out, bytes);
    assert_eq!(meta.grant, grant);
    assert_eq!(meta.camera_worker, boot(EYES));
    assert_eq!(meta.frame_sequence, 5);
    assert_eq!(meta.source, SOURCE);
    assert_eq!(meta.format, *b"MJPG");
    assert_eq!((meta.width, meta.height), (640, 480));
    assert_eq!(meta.dequeue_completed_at_us, 100);
    assert_eq!(meta.copied_at_us, 120);
    // The next one, within the same hold, keeps the grant.
    client.request(500_000).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !link.wants_frame() {
        assert!(Instant::now() < deadline);
        link.turn(Some(1), 200).unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(link.offer(Some(1), &read(6, 200), &bytes, 210).unwrap());
    let meta = pump(&mut link, &mut client, Some(1), 220, &mut out)
        .unwrap()
        .expect("a second frame");
    assert_eq!(meta.grant, grant);
    assert_eq!(meta.frame_sequence, 6);
}

#[test]
fn no_frame_is_copied_for_a_reader_that_has_not_asked() {
    let dir = Dir::new();
    let mut link = link(&dir);
    link.turn(Some(1), 10).unwrap();
    assert!(!link.wants_frame());
    assert!(!link.offer(Some(1), &read(1, 10), &jpeg(64, 1), 10).unwrap());
    assert_eq!(link.link_counts().unwrap().frames_offered, 0);
}

#[test]
fn the_permission_ending_closes_the_reader_and_removes_the_socket() {
    let dir = Dir::new();
    let mut link = link(&dir);
    let mut client = client(&dir);
    link.turn(Some(1), 10).unwrap();
    connect_and_ask(&mut link, &mut client, 1, 10);
    link.turn(None, 50).unwrap();
    assert!(!link.bound());
    assert!(!link.wants_frame());
    assert!(!dir.socket().exists());
    assert_eq!(link.counts().privacy_closed, 1);
    // The reader learns at once: its link is closed.
    let mut out = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match client.poll(&mut out) {
            Err(_) => break,
            Ok(Some(_)) => panic!("a frame after the permission ended"),
            Ok(None) => {
                assert!(Instant::now() < deadline, "the reader was not closed");
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
    assert!(!link.offer(None, &read(2, 60), &jpeg(64, 1), 60).unwrap());
}

#[test]
fn a_frame_in_transit_is_abandoned_when_the_permission_ends() {
    let dir = Dir::new();
    let mut link = link(&dir);
    let mut client = client(&dir);
    link.turn(Some(1), 10).unwrap();
    connect_and_ask(&mut link, &mut client, 1, 10);
    // Far larger than the socket's buffers: most of it is still unsent.
    let bytes = jpeg(MAX - 4096, 9);
    assert!(link.offer(Some(1), &read(1, 100), &bytes, 100).unwrap());
    link.turn(None, 150).unwrap();
    let mut out = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match client.poll(&mut out) {
            Err(_) => break,
            Ok(Some(_)) => panic!("a cut-off frame was completed"),
            Ok(None) => {
                assert!(Instant::now() < deadline, "the reader was not closed");
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}

#[test]
fn each_hold_is_a_new_grant_and_a_frame_of_the_old_one_is_not_offered() {
    let dir = Dir::new();
    let mut link = link(&dir);
    let mut client = client(&dir);
    link.turn(Some(1), 10).unwrap();
    let first = link.grant().unwrap();
    // A close and a reopen between two turns: the hold changed.
    link.turn(Some(2), 20).unwrap();
    let second = link.grant().unwrap();
    assert_ne!(first, second);
    assert_eq!(link.counts().privacy_closed, 1);
    connect_and_ask(&mut link, &mut client, 2, 30);
    // Read under hold 1, offered after hold 2 began: not offered.
    assert!(!link.offer(Some(1), &read(1, 30), &jpeg(64, 1), 30).unwrap());
    assert_eq!(link.counts().other_hold, 1);
    assert!(link.wants_frame(), "the reader still waits");
    assert!(link.offer(Some(2), &read(2, 40), &jpeg(64, 1), 40).unwrap());
    let mut out = Vec::new();
    let meta = pump(&mut link, &mut client, Some(2), 50, &mut out)
        .unwrap()
        .unwrap();
    assert_eq!(meta.grant, second);
}

#[test]
fn a_frame_that_is_not_a_whole_jpeg_is_not_offered() {
    let dir = Dir::new();
    let mut link = link(&dir);
    let mut client = client(&dir);
    link.turn(Some(1), 10).unwrap();
    connect_and_ask(&mut link, &mut client, 1, 10);
    let mut padded = jpeg(64, 1);
    padded.extend_from_slice(&[0, 0]);
    assert!(!link.offer(Some(1), &read(1, 20), &padded, 20).unwrap());
    assert_eq!(link.counts().not_jpeg, 1);
    assert!(link.wants_frame());
}

#[test]
fn a_slow_reader_is_dropped_without_stalling_the_eyes() {
    let dir = Dir::new();
    let mut link = link(&dir);
    let mut client = client(&dir);
    link.turn(Some(1), 10).unwrap();
    connect_and_ask(&mut link, &mut client, 1, 10);
    // The reader asked and then reads nothing.
    let bytes = jpeg(MAX - 4096, 3);
    let started = Instant::now();
    assert!(link.offer(Some(1), &read(1, 100), &bytes, 100).unwrap());
    let mut longest = started.elapsed();
    for step in 1..50 {
        let turn = Instant::now();
        link.turn(Some(1), 100 + step * 1_000).unwrap();
        longest = longest.max(turn.elapsed());
    }
    assert!(link.connected(), "within the deadline it stays");
    let turn = Instant::now();
    link.turn(Some(1), 100 + STALL_DEADLINE_US).unwrap();
    longest = longest.max(turn.elapsed());
    assert!(!link.connected(), "the stalled reader was dropped");
    assert_eq!(link.link_counts().unwrap().stalled, 1);
    assert_eq!(link.link_counts().unwrap().frames_sent, 0);
    // No call waited on the reader: each returned well inside one look.
    assert!(
        longest < Duration::from_millis(50),
        "a turn took {longest:?}"
    );
    // The link stays for the next reader.
    assert!(link.bound());
    assert!(!link.wants_frame());
}

#[test]
fn a_stranger_is_refused() {
    let dir = Dir::new();
    let mut link = link(&dir);
    link.turn(Some(1), 10).unwrap();
    let mut stranger = FrameClient::new(
        &dir.socket(),
        Role::Perception,
        boot(9),
        boot(CONTROLLER),
        MAX,
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while !stranger.connect().unwrap() {
        assert!(Instant::now() < deadline);
        link.turn(Some(1), 10).unwrap();
    }
    for _ in 0..50 {
        link.turn(Some(1), 20).unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!link.connected());
    assert_eq!(link.link_counts().unwrap().rejected, 1);
}

#[test]
fn a_directory_that_is_not_private_is_refused_at_start() {
    let dir = Dir::new();
    fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o750)).unwrap();
    let error = FrameLink::new(
        Config {
            path: dir.socket(),
            controller: boot(CONTROLLER),
            reader: boot(READER),
        },
        MAX,
        1,
    )
    .err()
    .expect("refused");
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn a_socket_left_by_an_eyes_that_ended_is_replaced() {
    let dir = Dir::new();
    let _left = std::os::unix::net::UnixListener::bind(dir.socket()).unwrap();
    let mut link = link(&dir);
    link.turn(Some(1), 10).unwrap();
    assert!(link.bound());
}

#[test]
fn only_lamp_seen_as_this_user_may_read() {
    let caller = |uid, unit: Option<&str>| Caller {
        pid: 7,
        uid,
        unit: unit.map(str::to_owned),
    };
    let me = Some(990);
    assert!(reader_may(
        Some(&caller(990, Some("lamp-seen.service"))),
        me
    ));
    for (who, why) in [
        (
            caller(990, Some("lamp-skills.service")),
            "another part as lampos",
        ),
        (
            caller(990, Some("lamp-cloud.service")),
            "another part as lampos",
        ),
        (caller(990, None), "no unit"),
        (caller(0, Some("lamp-seen.service")), "root"),
        (caller(1000, Some("lamp-seen.service")), "another user"),
    ] {
        assert!(!reader_may(Some(&who), me), "{why}");
    }
    assert!(!reader_may(None, me), "a caller the kernel could not name");
}

#[test]
fn a_peer_with_the_wrong_cgroup_is_refused_and_counted_with_the_right_boots() {
    let dir = Dir::new();
    // The product's check: this test process is no lamp-seen.service (and
    // off Linux the kernel names no caller), so it is refused.
    let mut link = FrameLink::new(config(&dir), MAX, 1).unwrap();
    link.turn(Some(1), 10).unwrap();
    let mut reader = client(&dir);
    assert!(refused(&mut link, &mut reader), "the link closed it");
    assert!(!link.connected());
    assert_eq!(link.counts().peer_refused, 1);
    assert_eq!(link.link_counts().unwrap().rejected, 1);
    let event = link.event().expect("the refusal is journaled");
    assert!(matches!(
        event,
        LinkEvent::Closed {
            role: None,
            reason: CloseReason::PeerRefused,
            ..
        }
    ));
    assert!(said(&event).contains("not lamp-seen.service"));
}

#[test]
fn a_peer_the_check_takes_is_accepted() {
    let dir = Dir::new();
    let asked = Arc::new(AtomicU64::new(0));
    let counted = asked.clone();
    let mut link = FrameLink::new_allowing(
        config(&dir),
        MAX,
        1,
        Arc::new(move |caller: Option<&Caller>| {
            counted.fetch_add(1, Ordering::SeqCst);
            // On Linux the kernel names this process: its own uid.
            #[cfg(target_os = "linux")]
            assert_eq!(
                caller.map(|caller| caller.uid),
                Some(rustix::process::geteuid().as_raw())
            );
            let _ = caller;
            true
        }),
    )
    .unwrap();
    let mut reader = client(&dir);
    link.turn(Some(1), 10).unwrap();
    connect_and_ask(&mut link, &mut reader, 1, 10);
    assert!(link.connected());
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(link.counts().peer_refused, 0);
}

#[test]
fn a_reader_cut_for_privacy_is_journaled() {
    let dir = Dir::new();
    let mut link = link(&dir);
    let mut reader = client(&dir);
    link.turn(Some(1), 10).unwrap();
    connect_and_ask(&mut link, &mut reader, 1, 10);
    assert!(matches!(
        link.event(),
        Some(LinkEvent::Connected {
            role: Role::Perception,
            ..
        })
    ));
    // The hold ends: the server is dropped in the same turn, its events kept.
    link.turn(None, 50).unwrap();
    assert!(!link.bound());
    let event = link.event().expect("the cut is journaled");
    assert!(matches!(
        event,
        LinkEvent::Closed {
            role: Some(Role::Perception),
            reason: CloseReason::PrivacyClosed,
            ..
        }
    ));
    assert!(said(&event).contains("cut, the camera's permission ended or changed"));
    assert!(link.event().is_none());
    // A new hold's link starts with no old events.
    link.turn(Some(2), 60).unwrap();
    assert!(link.event().is_none());
}

/// The boots file as `lamp-seen frame-link` keeps it (#6693).
fn boots_text(socket: &Path) -> String {
    format!(
        "LAMP_EYES_FRAME_LINK=--frame-link {} --frame-link-controller \
         01010101010101010101010101010101 --frame-link-reader \
         03030303030303030303030303030303\nLAMP_EYES_VISITS=--visits on\n",
        socket.display()
    )
}

fn write_boots(dir: &Dir, text: &str, mode: u32) -> PathBuf {
    let file = dir.0.join("frame-link.env");
    let _ = fs::remove_file(&file);
    fs::write(&file, text).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(mode)).unwrap();
    file
}

#[test]
fn the_boots_come_from_the_file_lamp_seen_keeps() {
    let dir = Dir::new();
    let socket = dir.socket();
    for mode in [0o600, 0o640] {
        let file = write_boots(&dir, &boots_text(&socket), mode);
        assert_eq!(
            boots_from(&file, &socket).unwrap(),
            Some((boot(CONTROLLER), boot(READER))),
            "mode {mode:o}"
        );
    }
    // No file: the switch is off, no link.
    assert_eq!(boots_from(&dir.0.join("none.env"), &socket).unwrap(), None);
    // Open to others, or group-writable: refused.
    for mode in [0o644, 0o660, 0o604] {
        let file = write_boots(&dir, &boots_text(&socket), mode);
        assert_eq!(
            boots_from(&file, &socket).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied,
            "mode {mode:o}"
        );
    }
    // A link to the file is not followed.
    let file = write_boots(&dir, &boots_text(&socket), 0o600);
    let link = dir.0.join("linked.env");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    assert!(boots_from(&link, &socket).is_err());
    // A folder open to others: refused.
    fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        boots_from(&file, &socket).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}

#[test]
fn a_boots_file_not_as_lamp_seen_keeps_it_is_refused() {
    let socket = PathBuf::from("/run/lampos-eyes-frames/camera.frames");
    let good = boots_text(&socket);
    assert_eq!(
        boots_in(&good, &socket),
        Ok((boot(CONTROLLER), boot(READER)))
    );
    let one = "01010101010101010101010101010101";
    let three = "03030303030303030303030303030303";
    for text in [
        good.replace(&socket.display().to_string(), "/run/other.frames"),
        good.replace(three, one),
        good.replace(three, "00"),
        good.replace(three, &"0".repeat(32)),
        format!("{good}{good}"),
        format!("{good}OTHER=1\n"),
        "LAMP_EYES_VISITS=--visits on\n".to_owned(),
        String::new(),
        format!(
            "LAMP_EYES_FRAME_LINK=--frame-link {} --frame-link-controller {one}\n",
            socket.display()
        ),
    ] {
        assert!(boots_in(&text, &socket).is_err(), "{text:?}");
    }
}
