//! Real exec consumers, no camera/app/network fixture and no process-global env mutation.
use super::*;
use crate::{
    camera_authority::{Authority, Binding, Service, socket_path},
    who::KnownSpeakerPublisher,
};
use lamp_ipc::camera_authority::{ReceiveGrant, Ticket, Token};
use std::{
    fs::{self, DirBuilder},
    io::{BufRead, BufReader, Write},
    net::Shutdown,
    num::NonZeroU64,
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt, symlink},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        // Canonical short per-instance root, as the Voice session fixtures use:
        // macOS TMPDIR alone can exhaust the production AF_UNIX path budget.
        let path = PathBuf::from("/tmp").join(format!(
            "lamp-eyes-ticket-{}-{}",
            std::process::id(),
            lamp_ipc::monotonic_ns()
        ));
        DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn known(&self) -> PathBuf {
        self.0.join("known")
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait<T>(mut probe: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(value) = probe() {
            return value;
        }
        assert!(Instant::now() < until, "fixture observation deadline");
        thread::sleep(Duration::from_millis(2));
    }
}
fn binding(authority: &Authority, hold: u64) -> Binding {
    wait(|| authority.frame(Some(hold), lamp_ipc::monotonic_us()))
}
fn client(known: &Path, binding: Binding) -> Ticket {
    let mut stream = UnixStream::connect(socket_path(known).unwrap()).unwrap();
    let mut request = [0; 48];
    request[..8].copy_from_slice(b"LMPASK03");
    request[8..24].copy_from_slice(&binding.publisher);
    request[24..32].copy_from_slice(&binding.hold.to_be_bytes());
    request[32..40].copy_from_slice(&17u64.to_be_bytes());
    stream.write_all(&request).unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut receiver = ReceiveGrant::new(
        Token::new(binding.publisher, binding.hold).unwrap(),
        NonZeroU64::new(17).unwrap(),
        Instant::now() + Duration::from_secs(2),
    );
    wait(|| match receiver.try_receive(&stream) {
        Ok(ticket) => ticket,
        Err(error) => panic!("fixture grant: {error}"),
    })
}
fn publish(publisher: &KnownSpeakerPublisher, binding: Binding) {
    publisher.camera_ticket(Some(binding));
    publisher.permission_until(Some(binding.until_us));
    publisher.seen_frame(
        "fixture-person",
        None,
        "0101010101010101",
        "f1",
        lamp_ipc::monotonic_us(),
    );
}
struct Heartbeat {
    ended: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Heartbeat {
    fn start(socket: UnixDatagram) -> Self {
        let ended = Arc::new(AtomicBool::new(false));
        let stop = ended.clone();
        let worker = thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                if socket.send(br#"{"camera":true}"#).is_err() {
                    break;
                }
                thread::park_timeout(Duration::from_millis(100));
            }
        });
        Self {
            ended,
            worker: Some(worker),
        }
    }
}
impl Drop for Heartbeat {
    fn drop(&mut self) {
        self.ended.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            worker.join().unwrap();
        }
    }
}
struct Consumer {
    child: Child,
    lines: mpsc::Receiver<String>,
    reader: Option<thread::JoinHandle<()>>,
}
impl Consumer {
    fn start(known: &Path) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "permit::camera_authority_tests::fixture_process",
                "--nocapture",
            ])
            .env("LAMP_EYES_TICKET_FIXTURE_KNOWN", known)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let output = child.stdout.take().unwrap();
        let (send, lines) = mpsc::channel();
        let reader = thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                match line {
                    Ok(line) => {
                        if send.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        let consumer = Self {
            child,
            lines,
            reader: Some(reader),
        };
        consumer.expect("fixture:ready");
        consumer
    }
    fn expect(&self, message: &str) {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let line = self
                .lines
                .recv_timeout(until.saturating_duration_since(Instant::now()))
                .unwrap();
            if line.contains(message) {
                return;
            }
        }
    }
    fn closed(&mut self) {
        writeln!(self.child.stdin.as_mut().unwrap(), "closed").unwrap();
        self.expect("fixture:closed");
        let status = wait(|| self.child.try_wait().unwrap());
        assert!(status.success());
    }
}
impl Drop for Consumer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// Auxiliary exec entry, selected by the parent with a child-local environment.
#[test]
fn fixture_process() {
    let Some(known) = std::env::var_os("LAMP_EYES_TICKET_FIXTURE_KNOWN") else {
        return;
    };
    let known = PathBuf::from(known);
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(crate::who::known_speaker_of(&known)).unwrap()).unwrap();
    assert_eq!(value["version"], 3);
    let publisher = value["publisher"].as_str().unwrap();
    let mut bytes = [0; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&publisher[index * 2..index * 2 + 2], 16).unwrap();
    }
    let ticket = client(
        &known,
        Binding {
            publisher: bytes,
            hold: value["camera_hold"].as_u64().unwrap(),
            began_us: value["camera_began_us"].as_u64().unwrap(),
            until_us: value["permission_until_us"].as_u64().unwrap(),
        },
    );
    assert!(ticket.is_open());
    println!("fixture:ready");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "closed");
    assert!(!ticket.is_open());
    println!("fixture:closed");
}

#[test]
fn explicit_close_revokes_two_exec_consumers_while_publication_lock_is_held() {
    let directory = Directory::new();
    let known = directory.known();
    let publisher = KnownSpeakerPublisher::new(known.clone());
    publisher.require_camera_ticket();
    let service = Service::start(&known, publisher.publisher_id().unwrap()).unwrap();
    let authority = service.authority();
    let (send, socket) = UnixDatagram::pair().unwrap();
    let permit = Permit::listen_with_authority(socket, Some(authority.clone())).unwrap();
    send.send(br#"{"camera":true}"#).unwrap();
    let hold = wait(|| permit.held());
    let initial = binding(&authority, hold);
    publish(&publisher, initial);
    let heartbeat = Heartbeat::start(send.try_clone().unwrap());
    let mut first = Consumer::start(&known);
    let mut second = Consumer::start(&known);
    drop(heartbeat);
    // A FIFO barrier proves no earlier true remains queued before we hold at.
    send.send(br#"{"fixture_barrier":true}"#).unwrap();
    wait(|| {
        permit
            .heard()
            .iter()
            .any(|packet| packet == br#"{"fixture_barrier":true}"#)
            .then_some(())
    });
    let current = binding(&authority, hold);
    publish(&publisher, current);
    let source = fs::read(crate::who::known_speaker_of(&known)).unwrap();
    let kernel_witness = client(&known, current);
    assert!(kernel_witness.is_open());
    let at = permit.at.lock().unwrap();
    assert!(lamp_ipc::monotonic_us() < current.until_us);
    send.send(br#"{"camera":false}"#).unwrap();
    // State denial precedes the syscall, so wait on the actual shared kernel
    // witness before asking the two exec consumers to make their final check.
    wait(|| (!kernel_witness.is_open()).then_some(()));
    assert!(
        authority
            .frame(Some(hold), lamp_ipc::monotonic_us())
            .is_none()
    );
    // Shutdown, not lease expiry or source rewrite, must explain the EOF.
    assert!(lamp_ipc::monotonic_us() < current.until_us);
    assert_eq!(
        fs::read(crate::who::known_speaker_of(&known)).unwrap(),
        source
    );
    first.closed();
    second.closed();
    drop(at);
    drop(service);
    assert!(!socket_path(&known).unwrap().exists());
}

#[test]
fn reopen_changes_hold_and_old_observer_never_revives() {
    let directory = Directory::new();
    let known = directory.known();
    let service = Service::start(&known, [7; 16]).unwrap();
    let authority = service.authority();
    let now = lamp_ipc::monotonic_us();
    authority.permit(1, now, now + 1_500_000);
    let old_binding = binding(&authority, 1);
    let old = client(&known, old_binding);
    assert!(old.is_open());
    authority.close().unwrap();
    assert!(!old.is_open());
    let now = lamp_ipc::monotonic_us();
    authority.permit(3, now, now + 1_500_000);
    let fresh_binding = binding(&authority, 3);
    let fresh = client(&known, fresh_binding);
    assert!(fresh.is_open());
    assert!(!old.is_open());
    assert!(authority.frame(Some(1), now).is_none());
    assert!(
        authority.frame(Some(3), now - 1).is_none(),
        "old captured frame cannot bind new hold"
    );
    drop(service);
    assert!(!fresh.is_open());
}

#[test]
fn v3_runtime_source_requires_ready_capture_binding_and_keeps_recognition_generation_separate() {
    let directory = Directory::new();
    let known = directory.known();
    let publisher = KnownSpeakerPublisher::new(known.clone());
    publisher.require_camera_ticket();
    publisher.seen_frame(
        "fixture-person",
        None,
        "0101010101010101",
        "f1",
        lamp_ipc::monotonic_us(),
    );
    assert!(
        !crate::who::known_speaker_of(&known).exists(),
        "no V2 fallback without server"
    );
    let service = Service::start(&known, publisher.publisher_id().unwrap()).unwrap();
    let authority = service.authority();
    let now = lamp_ipc::monotonic_us();
    authority.permit(9, now, now + 1_500_000);
    publish(&publisher, binding(&authority, 9));
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(crate::who::known_speaker_of(&known)).unwrap()).unwrap();
    assert_eq!(value["version"], 3);
    assert_eq!(value["camera_hold"], 9);
    assert_ne!(value["generation"], value["camera_hold"]);
    assert!(value["observed_us"].as_u64().unwrap() >= value["camera_began_us"].as_u64().unwrap());
    publisher.camera_ticket(None);
    assert_ne!(
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(crate::who::known_speaker_of(&known)).unwrap()
        )
        .unwrap()["state"],
        "seen"
    );
}

#[test]
fn expired_source_is_not_renewed_by_an_open_descriptor() {
    let directory = Directory::new();
    let known = directory.known();
    let service = Service::start(&known, [7; 16]).unwrap();
    let authority = service.authority();
    let now = lamp_ipc::monotonic_us();
    authority.permit(1, now, now + 1_500_000);
    let current = binding(&authority, 1);
    let ticket = client(&known, current);
    assert!(ticket.is_open());
    thread::sleep(Duration::from_micros(
        current.until_us.saturating_sub(lamp_ipc::monotonic_us()),
    ));
    assert!(authority.frame(Some(1), lamp_ipc::monotonic_us()).is_none());
    wait(|| (!ticket.is_open()).then_some(()));
}

#[test]
fn unsafe_path_and_partial_peer_cleanup_are_bounded() {
    let directory = Directory::new();
    assert!(socket_path(&directory.0.join("k".repeat(100))).is_err());
    let known = directory.known();
    let path = socket_path(&known).unwrap();
    let private = path.parent().unwrap();
    fs::create_dir(private).unwrap();
    fs::set_permissions(private, fs::Permissions::from_mode(0o700)).unwrap();
    let foreign = directory.0.join("foreign");
    fs::write(&foreign, b"keep").unwrap();
    symlink(&foreign, &path).unwrap();
    assert!(Service::start(&known, [7; 16]).is_err());
    assert_eq!(fs::read(&foreign).unwrap(), b"keep");
    fs::remove_file(&path).unwrap();
    let service = Service::start(&known, [7; 16]).unwrap();
    let mut partial = UnixStream::connect(&path).unwrap();
    partial.write_all(b"L").unwrap();
    thread::sleep(Duration::from_millis(400));
    let mut byte = [0u8; 1];
    wait(
        || match rustix::net::recv(&partial, &mut byte[..], rustix::net::RecvFlags::DONTWAIT) {
            Ok((_, 0)) | Err(rustix::io::Errno::CONNRESET) => Some(()),
            Err(rustix::io::Errno::AGAIN) => None,
            other => panic!("expired partial peer did not close: {other:?}"),
        },
    );
    drop(partial);
    drop(service);
    assert!(!path.exists());
}

fn refused(known: &Path, binding: Binding) {
    let mut stream = UnixStream::connect(socket_path(known).unwrap()).unwrap();
    let mut request = [0; 48];
    request[..8].copy_from_slice(b"LMPASK03");
    request[8..24].copy_from_slice(&binding.publisher);
    request[24..32].copy_from_slice(&binding.hold.to_be_bytes());
    request[32..40].copy_from_slice(&17u64.to_be_bytes());
    stream.write_all(&request).unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut receiver = ReceiveGrant::new(
        Token::new(binding.publisher, binding.hold).unwrap(),
        NonZeroU64::new(17).unwrap(),
        Instant::now() + Duration::from_secs(2),
    );
    wait(|| match receiver.try_receive(&stream) {
        Err(_) => Some(()),
        Ok(None) => None,
        Ok(Some(_)) => panic!("unbound grant was accepted"),
    });
}

#[test]
fn old_or_wrong_incarnation_requests_never_get_a_fresh_hold_ticket() {
    let directory = Directory::new();
    let known = directory.known();
    let service = Service::start(&known, [7; 16]).unwrap();
    let authority = service.authority();
    let now = lamp_ipc::monotonic_us();
    authority.permit(1, now, now + 1_500_000);
    let original = binding(&authority, 1);
    let ticket = client(&known, original);
    refused(
        &known,
        Binding {
            publisher: [8; 16],
            ..original
        },
    );
    authority.close().unwrap();
    let now = lamp_ipc::monotonic_us();
    authority.permit(3, now, now + 1_500_000);
    let current = binding(&authority, 3);
    refused(&known, original);
    assert!(!ticket.is_open());
    assert!(client(&known, current).is_open());
}

#[test]
fn renewals_never_retag_old_observations_under_a_new_camera_hold() {
    let directory = Directory::new();
    let known = directory.known();
    let publisher = KnownSpeakerPublisher::new(known.clone());
    publisher.require_camera_ticket();
    let service = Service::start(&known, publisher.publisher_id().unwrap()).unwrap();
    let authority = service.authority();
    let now = lamp_ipc::monotonic_us();
    authority.permit(1, now, now + 1_500_000);
    let first = binding(&authority, 1);
    publish(&publisher, first);
    let path = crate::who::known_speaker_of(&known);
    let before: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    authority.permit(1, now, lamp_ipc::monotonic_us() + 1_500_000);
    let renewed = binding(&authority, 1);
    publisher.camera_ticket(Some(renewed));
    publisher.permission_until(Some(renewed.until_us));
    assert!(publisher.refresh_authority(lamp_ipc::monotonic_us()));
    let after: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(before["observed_us"], after["observed_us"]);
    assert_eq!(before["camera_hold"], after["camera_hold"]);
    authority.close().unwrap();
    let now = lamp_ipc::monotonic_us();
    authority.permit(3, now, now + 1_500_000);
    let new_hold = binding(&authority, 3);
    publisher.camera_ticket(Some(new_hold));
    publisher.permission_until(Some(new_hold.until_us));
    assert_ne!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap()["state"],
        "seen"
    );
    publish(&publisher, new_hold);
    let fresh: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(fresh["state"], "seen");
    assert_eq!(fresh["camera_hold"], 3);
}

#[test]
fn private_layout_keeps_non_utf8_and_never_unlinks_existing_or_replaced_sockets() {
    use std::os::unix::{
        ffi::{OsStrExt, OsStringExt},
        net::UnixListener,
    };
    let directory = Directory::new();
    let known = directory
        .0
        .join(std::ffi::OsString::from_vec(b"known-\xff".to_vec()));
    let path = socket_path(&known).unwrap();
    assert!(
        path.as_os_str()
            .as_bytes()
            .ends_with(b"known-\xff.authority.d/hold.sock")
    );
    let (known, path, service) = match Service::start(&known, [7; 16]) {
        Ok(service) => (known, path, service),
        #[cfg(target_os = "macos")]
        Err(error) if error.raw_os_error() == Some(rustix::io::Errno::ILSEQ.raw_os_error()) => {
            // This filesystem rejects the opaque name before socket binding.
            // Keep that exact native result, then exercise every live check.
            assert_eq!(
                error.raw_os_error(),
                Some(rustix::io::Errno::ILSEQ.raw_os_error())
            );
            let known = directory.known();
            let path = socket_path(&known).unwrap();
            let service = Service::start(&known, [7; 16]).unwrap();
            (known, path, service)
        }
        Err(error) => panic!("opaque authority service failed: {error}"),
    };
    assert!(
        Service::start(&known, [8; 16]).is_err(),
        "existing same-UID server must not be unlinked"
    );
    assert!(path.exists());
    fs::remove_file(&path).unwrap();
    let replacement = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    drop(service);
    assert!(path.exists(), "teardown must preserve a replacement inode");
    drop(replacement);
    fs::remove_file(path).unwrap();
}

#[test]
fn retirement_between_at_renewal_and_private_handoff_rolls_capture_without_restamping_lease() {
    let directory = Directory::new();
    let known = directory.known();
    let service = Service::start(&known, [7; 16]).unwrap();
    let authority = service.authority();
    let permit = Permit::default();
    let first = Receipt {
        at: Instant::now(),
        us: lamp_ipc::monotonic_us(),
    };
    {
        let mut at = permit.at.lock().unwrap();
        received_permission(
            &mut at,
            &permit.hold,
            &permit.began_us,
            true,
            first.at,
            first.us,
        );
    }
    let old_hold = permit.held().unwrap();
    let old_began = permit.began_us.load(Ordering::Relaxed);
    update_disclosure_permission(
        &authority,
        &permit.at,
        &permit.hold,
        &permit.began_us,
        old_hold,
        old_began,
        first,
    );
    let old_binding = binding(&authority, old_hold);
    let old_ticket = client(&known, old_binding);
    // Deterministic handoff ordering: at is already renewed, but authority has
    // not received that callback. Real retirement/shutdown supplies the expiry
    // state without relying on a scheduler winning a narrow timed race.
    let renewed = Receipt {
        at: Instant::now(),
        us: lamp_ipc::monotonic_us(),
    };
    {
        let mut at = permit.at.lock().unwrap();
        received_permission(
            &mut at,
            &permit.hold,
            &permit.began_us,
            true,
            renewed.at,
            renewed.us,
        );
    }
    assert_eq!(permit.held(), Some(old_hold));
    let queued_frame = lamp_ipc::monotonic_us();
    wait(|| (lamp_ipc::monotonic_us() > queued_frame).then_some(()));
    authority.close().unwrap();
    assert!(!old_ticket.is_open());
    update_disclosure_permission(
        &authority,
        &permit.at,
        &permit.hold,
        &permit.began_us,
        old_hold,
        old_began,
        renewed,
    );
    let fresh_hold = permit.held().unwrap();
    assert!(fresh_hold > old_hold);
    assert_eq!(*permit.at.lock().unwrap(), Some(renewed.at));
    let floor = permit.began_us.load(Ordering::Relaxed);
    assert!(floor > queued_frame && floor >= renewed.us);
    assert_eq!(permit.frame_hold(queued_frame), None);
    assert!(
        !permit.with_frame_hold(Some(fresh_hold), queued_frame, |_, valid| valid),
        "new read-time hold tag cannot rebind a queued pre-roll image"
    );
    assert!(authority.frame(Some(fresh_hold), queued_frame).is_none());
    let post_roll = lamp_ipc::monotonic_us();
    assert_eq!(permit.frame_hold(post_roll), Some(fresh_hold));
    let fresh_binding = binding(&authority, fresh_hold);
    assert!(
        fresh_binding.until_us <= renewed.us + 1_500_000,
        "rollover must spend only the original receipt deadline"
    );
    assert!(authority.frame(Some(fresh_hold), post_roll).is_some());
    let fresh_ticket = client(&known, fresh_binding);
    assert!(fresh_ticket.is_open());
    assert!(!old_ticket.is_open());
    // Continuous true renewals keep the repaired logical hold live, rather
    // than requiring a physical false or repeatedly recreating the old token.
    let next = Receipt {
        at: Instant::now(),
        us: lamp_ipc::monotonic_us(),
    };
    {
        let mut at = permit.at.lock().unwrap();
        received_permission(
            &mut at,
            &permit.hold,
            &permit.began_us,
            true,
            next.at,
            next.us,
        );
    }
    update_disclosure_permission(
        &authority,
        &permit.at,
        &permit.hold,
        &permit.began_us,
        fresh_hold,
        floor,
        next,
    );
    assert_eq!(permit.held(), Some(fresh_hold));
    assert!(fresh_ticket.is_open());
}
