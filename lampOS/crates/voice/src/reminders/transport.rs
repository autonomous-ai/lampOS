//! Bounded anonymous IPC for the optional Schedule consumer child.
//!
//! The child opens/locks the fixed per-UID claim and transfers the same open
//! file description to the parent. Keep `Transport` alive for all of run_source,
//! including after child failure. Its destruction belongs after audio revocation
//! and verified worker retirement, not to a mute/unmute cycle. Acquiring this
//! lock is exclusion evidence, never proof that an earlier reminder was silent.
use rustix::{
    io::{Errno, FdFlags, fcntl_getfd, fcntl_setfd},
    net::{
        RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags, SendAncillaryBuffer,
        SendAncillaryMessage, SendFlags, SocketType, recvmsg, sendmsg, sockopt,
    },
    process::{Pid, Signal, geteuid, kill_process_group},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, TryLockError},
    io::{self, IoSlice, IoSliceMut, Write},
    mem::MaybeUninit,
    os::{
        fd::{AsFd, OwnedFd},
        unix::{fs::MetadataExt, net::UnixDatagram, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

pub const MAX_PACKET_BYTES: usize = 4096;
const POLL_PACKETS: usize = 4;
const SOCKET_BUFFER_BYTES: usize = 32 * 1024;
const CLAIM_PACKET: &[u8] = b"\0lamp-schedule-consumer-claim-v1";

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn not_ready() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "Schedule consumer transport is closed",
    )
}

pub(super) struct Transport {
    socket: UnixDatagram,
    child: Option<Child>,
    claim: Option<File>,
    pending: Option<Vec<u8>>,
    stopped: bool,
    exited: bool,
    failed: bool,
    retain_claim: bool,
}
impl Transport {
    /// Explicitly invoked only by the exact opt-in parent path. Process creation
    /// itself is an OS operation, not a claimed hard real-time boundary.
    pub fn spawn(executable: &Path) -> io::Result<Self> {
        if !executable.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "consumer executable must be absolute",
            ));
        }
        let mut command = Command::new(executable);
        command.arg(lamp_voice_processes::Command::ScheduleConsumer.as_str());
        let mut transport = Self::spawn_command(command)?;
        transport.retain_claim_until_process_exit();
        Ok(transport)
    }
    fn spawn_command(mut command: Command) -> io::Result<Self> {
        let (parent, child) = pair()?;
        let stdin: OwnedFd = child.into();
        command
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::null())
            // The consumer's journal lines go where the voice's go: they
            // name ids and reasons, never a reminder's words. With stderr
            // closed they reached no journal on lamp-4ace (2026-10-16).
            .stderr(Stdio::inherit())
            .process_group(0);
        let child = command.spawn()?;
        Ok(Self {
            socket: parent,
            child: Some(child),
            claim: None,
            pending: None,
            stopped: false,
            exited: false,
            failed: false,
            retain_claim: false,
        })
    }
    /// At most four nonblocking recvmsg calls and one retained ordinary packet.
    /// No paths, locks, filesystem metadata, sleeps, waits, or joins are used.
    pub fn poll(&mut self) -> io::Result<()> {
        if self.failed || self.stopped {
            return Err(not_ready());
        }
        let result = self.poll_inner();
        if result.is_err() {
            self.failed = true;
            self.pending = None;
        }
        result
    }
    fn poll_inner(&mut self) -> io::Result<()> {
        if self.pending.is_some() {
            return Ok(());
        }
        for _ in 0..POLL_PACKETS {
            let Some(mut packet) = receive_packet(&self.socket)? else {
                break;
            };
            if packet.bytes == CLAIM_PACKET {
                if self.claim.is_some() || packet.fds.len() != 1 {
                    return Err(invalid("consumer claim transfer is duplicate or malformed"));
                }
                // The private peer is the explicitly spawned, trusted local
                // child. File/path validation happened there, outside Voice's
                // urgent loop. Receiving a descriptor is not audio readiness.
                self.claim = Some(packet.fds.pop().expect("one claim descriptor").into());
            } else {
                if self.claim.is_none() || !packet.fds.is_empty() {
                    return Err(invalid("consumer message arrived without its claim"));
                }
                self.pending = Some(packet.bytes);
                break;
            }
        }
        Ok(())
    }
    pub fn claim_held(&self) -> bool {
        self.claim.is_some()
    }
    /// Production instances use this from creation. Even verified consumer
    /// exit does not prove that every old audio worker retired successfully.
    /// Keep the one shared claim open until this Voice process actually exits,
    /// including when run_source returns into an embedding/quota-wait loop.
    pub fn retain_claim_until_process_exit(&mut self) {
        self.retain_claim = true;
    }
    pub fn send<T: Serialize>(&mut self, message: &T) -> io::Result<bool> {
        if self.failed || self.stopped || self.exited {
            return Err(not_ready());
        }
        if !self.claim_held() {
            return Ok(false);
        }
        let bytes = encode(message)?;
        send_packet(&self.socket, &bytes, None)
    }
    pub fn receive<T: DeserializeOwned>(&mut self) -> io::Result<Option<T>> {
        self.poll()?;
        let Some(bytes) = self.pending.take() else {
            return Ok(None);
        };
        match serde_json::from_slice(&bytes) {
            Ok(value) => Ok(Some(value)),
            Err(_) => {
                self.failed = true;
                Err(invalid("consumer packet is not the expected JSON message"))
            }
        }
    }
    /// Nonblocking waitpid only. A child exit does not release the parent's
    /// claim or authorize replacement; the owning service retains this object.
    pub fn try_exit(&mut self) -> io::Result<bool> {
        if self.exited {
            return Ok(true);
        }
        if let Some(child) = &mut self.child
            && child.try_wait()?.is_some()
        {
            self.exited = true;
        }
        Ok(self.exited)
    }
    /// Signal the owned group at most once. Success/termination is established
    /// only by try_exit; kernel-stalled work has no userspace teardown guarantee.
    pub fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        self.pending = None;
        if self.exited {
            return;
        }
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(_)) => self.exited = true,
                Ok(None) => {
                    let _ = kill_process_group(Pid::from_child(child), Signal::TERM);
                }
                // Do not signal a possibly recycled ID after a wait ownership
                // error. Retain the claim and expose the error from try_exit.
                Err(_) => {}
            }
        }
    }
}
impl Drop for Transport {
    fn drop(&mut self) {
        self.stop();
        let exited = matches!(self.try_exit(), Ok(true));
        if self.retain_claim || (!exited && self.child.is_some()) {
            // Exactly one retained claim until the Voice process actually
            // exits. No replacement child is started, and Drop never blocks on
            // wait/fsync/join. Continue polling/reaping before normal teardown.
            if let Some(claim) = self.claim.take() {
                std::mem::forget(claim);
            }
        }
    }
}

/// What a test opens right after dropping the lock's holder: a claim, or a
/// Store's directory. A child another test forks at that moment shares the
/// lock until its exec (CLOEXEC), so `WouldBlock` is tried again for up to
/// 5 s (#1569). A lock that is really kept still fails then.
#[cfg(test)]
pub(super) fn when_free<T>(mut open: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match open() {
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    && std::time::Instant::now() < until =>
            {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            opened => return opened,
        }
    }
}

/// In-process test peer with the real claim descriptor handoff. The returned
/// parent has no child to stop/reap and releases its descriptor on ordinary
/// drop; callers still poll to accept the queued claim before protocol data.
#[cfg(test)]
pub(super) fn test_pair(path: &Path) -> io::Result<(Transport, WorkerTransport)> {
    let (parent, child) = pair()?;
    let worker = WorkerTransport::from_socket_at(child, path)?;
    Ok((
        Transport {
            socket: parent,
            child: None,
            claim: None,
            pending: None,
            stopped: false,
            exited: false,
            failed: false,
            retain_claim: false,
        },
        worker,
    ))
}

pub(super) struct WorkerTransport {
    socket: UnixDatagram,
    claim: File,
    sent_claim: bool,
    failed: bool,
}
impl WorkerTransport {
    /// Only the child calls this. All claim-file access and lock acquisition
    /// happen here, before any protocol packet can be sent to the parent.
    pub fn from_stdin() -> io::Result<Self> {
        Self::from_stdin_at(&claim_path())
    }
    fn from_stdin_at(path: &Path) -> io::Result<Self> {
        let stdin = std::io::stdin();
        if sockopt::socket_type(&stdin)? != SocketType::DGRAM {
            return Err(invalid("consumer stdin is not a datagram socket"));
        }
        let fd = rustix::io::dup(&stdin)?;
        cloexec(&fd)?;
        let socket = UnixDatagram::from(fd);
        // UnixDatagram's address access also rejects a non-Unix descriptor.
        socket.local_addr()?;
        socket.peer_addr()?;
        configure(&socket)?;
        Self::from_socket_at(socket, path)
    }
    fn from_socket_at(socket: UnixDatagram, path: &Path) -> io::Result<Self> {
        let mut transport = Self {
            socket,
            claim: acquire_claim(path)?,
            sent_claim: false,
            failed: false,
        };
        transport.flush_claim()?;
        Ok(transport)
    }
    fn flush_claim(&mut self) -> io::Result<bool> {
        if self.failed {
            return Err(not_ready());
        }
        if !self.sent_claim {
            self.sent_claim = send_packet(&self.socket, CLAIM_PACKET, Some(&self.claim))?;
        }
        Ok(self.sent_claim)
    }
    pub fn send<T: Serialize>(&mut self, message: &T) -> io::Result<bool> {
        if !self.flush_claim()? {
            return Ok(false);
        }
        send_packet(&self.socket, &encode(message)?, None)
    }
    pub fn receive<T: DeserializeOwned>(&mut self) -> io::Result<Option<T>> {
        if !self.flush_claim()? {
            return Ok(None);
        }
        let Some(packet) = receive_packet(&self.socket)? else {
            return Ok(None);
        };
        if !packet.fds.is_empty() || packet.bytes == CLAIM_PACKET {
            self.failed = true;
            return Err(invalid("parent must not transfer consumer claims"));
        }
        match serde_json::from_slice(&packet.bytes) {
            Ok(value) => Ok(Some(value)),
            Err(_) => {
                self.failed = true;
                Err(invalid("parent packet is not the expected JSON message"))
            }
        }
    }
}

fn claim_path() -> PathBuf {
    #[cfg(target_os = "linux")]
    let root = "/run/lock";
    #[cfg(not(target_os = "linux"))]
    let root = "/private/tmp";
    Path::new(root).join(format!(
        "lampos-schedule-consumer-{}.lock",
        geteuid().as_raw()
    ))
}
fn acquire_claim(path: &Path) -> io::Result<File> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "consumer claim needs an absolute file path",
        ));
    }
    use rustix::fs::{Mode, OFlags, open};
    // NONBLOCK prevents a substituted FIFO from stalling before fstat rejects
    // it. Never unlink or replace an existing claim inode, including on exit.
    let file: File = open(
        path,
        OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::from_bits_retain(0o600),
    )?
    .into();
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != geteuid().as_raw()
        || metadata.mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(invalid(
            "consumer claim is not a private owned regular file",
        ));
    }
    file.try_lock().map_err(|error| match error {
        TryLockError::WouldBlock => io::Error::new(
            io::ErrorKind::WouldBlock,
            "another Schedule consumer owns the claim",
        ),
        TryLockError::Error(error) => error,
    })?;
    let named = fs::symlink_metadata(path)?;
    if named.file_type().is_symlink()
        || named.dev() != metadata.dev()
        || named.ino() != metadata.ino()
        || named.nlink() != 1
    {
        return Err(invalid("consumer claim path changed while acquiring it"));
    }
    Ok(file)
}
fn pair() -> io::Result<(UnixDatagram, UnixDatagram)> {
    let (parent, child) = UnixDatagram::pair()?;
    configure(&parent)?;
    configure(&child)?;
    Ok((parent, child))
}
fn configure(socket: &UnixDatagram) -> io::Result<()> {
    socket.set_nonblocking(true)?;
    cloexec(socket)?;
    sockopt::set_socket_recv_buffer_size(socket, SOCKET_BUFFER_BYTES)?;
    sockopt::set_socket_send_buffer_size(socket, SOCKET_BUFFER_BYTES)?;
    #[cfg(target_os = "macos")]
    sockopt::set_socket_nosigpipe(socket, true)?;
    Ok(())
}
fn cloexec(fd: impl AsFd) -> io::Result<()> {
    let flags = fcntl_getfd(&fd)?;
    fcntl_setfd(fd, flags | FdFlags::CLOEXEC)?;
    Ok(())
}
struct Packet {
    bytes: Vec<u8>,
    fds: Vec<OwnedFd>,
}
fn receive_packet(socket: &UnixDatagram) -> io::Result<Option<Packet>> {
    let mut bytes = [0; MAX_PACKET_BYTES + 1];
    let mut ancillary = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(2))];
    let mut control = RecvAncillaryBuffer::new(&mut ancillary);
    let flags = RecvFlags::DONTWAIT;
    #[cfg(target_os = "linux")]
    let flags = flags | RecvFlags::CMSG_CLOEXEC;
    let received = match recvmsg(
        socket,
        &mut [IoSliceMut::new(&mut bytes)],
        &mut control,
        flags,
    ) {
        Ok(received) => received,
        Err(Errno::AGAIN | Errno::INTR) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut fds = Vec::with_capacity(2);
    let mut unexpected = false;
    for ancillary in control.drain() {
        match ancillary {
            RecvAncillaryMessage::ScmRights(rights) => {
                for fd in rights {
                    if fds.len() == 2 {
                        unexpected = true;
                        continue;
                    }
                    cloexec(&fd)?;
                    fds.push(fd);
                }
            }
            _ => unexpected = true,
        }
    }
    if unexpected
        || received
            .flags
            .intersects(ReturnFlags::TRUNC | ReturnFlags::CTRUNC)
        || received.bytes == 0
        || received.bytes > MAX_PACKET_BYTES
    {
        return Err(invalid(
            "consumer datagram is empty, oversized, or has excess ancillary data",
        ));
    }
    Ok(Some(Packet {
        bytes: bytes[..received.bytes].to_vec(),
        fds,
    }))
}
fn send_packet(socket: &UnixDatagram, bytes: &[u8], claim: Option<&File>) -> io::Result<bool> {
    if bytes.is_empty() || bytes.len() > MAX_PACKET_BYTES {
        return Err(invalid("consumer datagram exceeds its byte limit"));
    }
    let mut ancillary = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = SendAncillaryBuffer::new(&mut ancillary);
    let rights = claim.map(|file| [file.as_fd()]);
    if let Some(rights) = &rights
        && !control.push(SendAncillaryMessage::ScmRights(rights))
    {
        return Err(invalid("consumer claim ancillary buffer is too small"));
    }
    let flags = SendFlags::DONTWAIT;
    #[cfg(target_os = "linux")]
    let flags = flags | SendFlags::NOSIGNAL;
    match sendmsg(socket, &[IoSlice::new(bytes)], &mut control, flags) {
        Ok(written) if written == bytes.len() => Ok(true),
        Ok(_) => Err(invalid("consumer datagram was partially sent")),
        // Darwin also reports ENOBUFS when this datagram queue cannot accept
        // another whole packet. Nothing was accepted; preserve caller state
        // so the same bounded packet can be retried after the peer drains.
        Err(Errno::AGAIN | Errno::INTR | Errno::NOBUFS) => Ok(false),
        Err(error) => Err(error.into()),
    }
}
fn encode(message: &impl Serialize) -> io::Result<Vec<u8>> {
    struct Bounded {
        bytes: Vec<u8>,
    }
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > MAX_PACKET_BYTES - self.bytes.len() {
                return Err(invalid("consumer JSON exceeds its byte limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut bounded = Bounded { bytes: Vec::new() };
    serde_json::to_writer(&mut bounded, message)
        .map_err(|_| invalid("consumer message is not bounded JSON"))?;
    Ok(bounded.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::process::{WaitOptions, WaitStatus, kill_process, waitpid};
    use serde::Deserialize;
    use serde_json::{Value, json};
    use std::{
        fs::{DirBuilder, OpenOptions},
        os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink},
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64, Ordering},
        },
        time::{Duration, Instant},
    };

    static NEXT: AtomicU64 = AtomicU64::new(1);
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "lamp-schedule-transport-{}-{}-{}",
                geteuid().as_raw(),
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            DirBuilder::new().mode(0o700).create(&path).unwrap();
            Self(path)
        }
        fn claim(&self) -> PathBuf {
            self.0.join("claim")
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn parent(socket: UnixDatagram) -> Transport {
        Transport {
            socket,
            child: None,
            claim: None,
            pending: None,
            stopped: false,
            exited: false,
            failed: false,
            retain_claim: false,
        }
    }
    struct Peers {
        parent: Transport,
        worker: Option<WorkerTransport>,
        scratch: Scratch,
    }
    impl Peers {
        fn new() -> Self {
            let scratch = Scratch::new();
            let (a, b) = pair().unwrap();
            let worker = WorkerTransport::from_socket_at(b, &scratch.claim()).unwrap();
            Self {
                parent: parent(a),
                worker: Some(worker),
                scratch,
            }
        }
        fn worker(&mut self) -> &mut WorkerTransport {
            self.worker.as_mut().unwrap()
        }
    }
    impl Drop for Peers {
        fn drop(&mut self) {
            // This fixture has no child process; ordinary field drop closes
            // both endpoints and descriptors without the production leak path.
            self.parent.exited = true;
        }
    }
    fn wait(mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !ready() {
            assert!(
                Instant::now() < deadline,
                "authored subprocess did not progress within 2 seconds"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn in_process_test_helper_transfers_and_releases_the_real_claim() {
        let scratch = Scratch::new();
        let (mut parent, mut worker) = test_pair(&scratch.claim()).unwrap();
        assert!(!parent.claim_held());
        parent.poll().unwrap();
        assert!(parent.claim_held());
        assert!(parent.send(&json!({"request":1})).unwrap());
        assert_eq!(
            worker.receive::<Value>().unwrap(),
            Some(json!({"request":1}))
        );
        drop(worker);
        assert_eq!(
            acquire_claim(&scratch.claim()).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(parent);
        assert!(when_free(|| acquire_claim(&scratch.claim())).is_ok());
    }

    #[test]
    fn transferred_claim_survives_child_side_close_and_is_cloexec() {
        let mut peers = Peers::new();
        assert!(!peers.parent.claim_held());
        assert!(!peers.parent.send(&json!({"before_claim":true})).unwrap());
        peers.parent.poll().unwrap();
        assert!(peers.parent.claim_held());
        assert!(
            fcntl_getfd(peers.parent.claim.as_ref().unwrap())
                .unwrap()
                .contains(FdFlags::CLOEXEC)
        );
        drop(peers.worker.take());
        assert_eq!(
            acquire_claim(&peers.scratch.claim()).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(peers.parent.claim.take());
        assert!(when_free(|| acquire_claim(&peers.scratch.claim())).is_ok());
    }

    #[test]
    fn exact_packet_bound_and_bidirectional_json_roundtrip() {
        let mut peers = Peers::new();
        peers.parent.poll().unwrap();
        let payload = "x".repeat(MAX_PACKET_BYTES - 2);
        assert!(peers.worker().send(&payload).unwrap());
        assert_eq!(peers.parent.receive::<String>().unwrap(), Some(payload));
        let command = json!({"kind":"complete","counter":17});
        assert!(peers.parent.send(&command).unwrap());
        assert_eq!(peers.worker().receive::<Value>().unwrap(), Some(command));
        assert!(peers.worker().receive::<Value>().unwrap().is_none());
    }

    #[test]
    fn oversize_preflight_sends_nothing_and_leaves_valid_sender_usable() {
        let mut peers = Peers::new();
        peers.parent.poll().unwrap();
        assert!(peers.worker().send(&"x".repeat(MAX_PACKET_BYTES)).is_err());
        assert!(peers.parent.receive::<Value>().unwrap().is_none());
        assert!(peers.worker().send(&json!({"still_valid":true})).unwrap());
        assert_eq!(
            peers.parent.receive::<Value>().unwrap(),
            Some(json!({"still_valid":true}))
        );
    }

    #[test]
    fn oversized_raw_datagram_is_rejected_instead_of_truncated_json() {
        let mut peers = Peers::new();
        peers.parent.poll().unwrap();
        peers
            .worker()
            .socket
            .send(&vec![b' '; MAX_PACKET_BYTES + 1])
            .unwrap();
        assert_eq!(
            peers.parent.receive::<Value>().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(peers.parent.failed);
        assert!(peers.parent.claim_held());
        assert!(peers.parent.poll().is_err());
    }

    #[test]
    fn malformed_or_unknown_json_shape_faults_the_receiver() {
        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Reply {
            _counter: u64,
        }
        for bytes in [
            b"{".as_slice(),
            br#"{"_counter":1,"extra":2}"#.as_slice(),
            b"".as_slice(),
        ] {
            let mut peers = Peers::new();
            peers.parent.poll().unwrap();
            peers.worker().socket.send(bytes).unwrap();
            assert!(peers.parent.receive::<Reply>().is_err());
            assert!(peers.parent.failed);
        }
    }

    #[test]
    fn messages_before_claim_are_never_delivered() {
        let (a, b) = pair().unwrap();
        let mut transport = parent(a);
        b.send(br#"{"offer":true}"#).unwrap();
        assert!(transport.receive::<Value>().is_err());
        assert!(!transport.claim_held());
        transport.exited = true;
    }

    #[test]
    fn duplicate_claim_does_not_replace_or_release_existing_lock() {
        let mut peers = Peers::new();
        peers.parent.poll().unwrap();
        let worker = peers.worker();
        assert!(send_packet(&worker.socket, CLAIM_PACKET, Some(&worker.claim)).unwrap());
        assert!(peers.parent.poll().is_err());
        drop(peers.worker.take());
        assert!(peers.parent.claim_held());
        assert_eq!(
            acquire_claim(&peers.scratch.claim()).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn claim_without_descriptor_and_rights_with_ordinary_json_are_rejected() {
        let (a, b) = pair().unwrap();
        let mut transport = parent(a);
        assert!(send_packet(&b, CLAIM_PACKET, None).unwrap());
        assert!(transport.poll().is_err());
        assert!(!transport.claim_held());
        transport.exited = true;
        let mut peers = Peers::new();
        peers.parent.poll().unwrap();
        let worker = peers.worker();
        assert!(send_packet(&worker.socket, b"{}", Some(&worker.claim)).unwrap());
        assert!(peers.parent.receive::<Value>().is_err());
    }

    #[test]
    fn excess_ancillary_descriptors_fault_without_installing_a_claim() {
        let scratch = Scratch::new();
        let claim = acquire_claim(&scratch.claim()).unwrap();
        let (a, b) = pair().unwrap();
        let mut transport = parent(a);
        let rights = [claim.as_fd(), claim.as_fd(), claim.as_fd()];
        let mut bytes = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(3))];
        let mut control = SendAncillaryBuffer::new(&mut bytes);
        assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
        sendmsg(
            &b,
            &[IoSlice::new(CLAIM_PACKET)],
            &mut control,
            SendFlags::DONTWAIT,
        )
        .unwrap();
        assert!(transport.poll().is_err());
        assert!(!transport.claim_held());
        transport.exited = true;
        drop(claim);
        assert!(
            when_free(|| acquire_claim(&scratch.claim())).is_ok(),
            "rejected ancillary rights leaked their lock"
        );
    }

    #[test]
    fn flood_is_backpressured_and_poll_retains_only_one_ordered_packet() {
        let mut peers = Peers::new();
        let mut sent = 0;
        for counter in 0..1024 {
            if !peers.worker().send(&json!({"counter":counter})).unwrap() {
                break;
            }
            sent += 1;
        }
        assert!(
            sent > 1 && sent < 1024,
            "fixed kernel queue did not exert bounded backpressure"
        );
        peers.parent.poll().unwrap();
        let first = peers.parent.pending.clone().unwrap();
        for _ in 0..4 {
            peers.parent.poll().unwrap();
        }
        assert_eq!(peers.parent.pending.as_ref().unwrap(), &first);
        for counter in 0..sent {
            assert_eq!(
                peers.parent.receive::<Value>().unwrap(),
                Some(json!({"counter":counter}))
            );
        }
        assert!(peers.parent.receive::<Value>().unwrap().is_none());
        // The full-queue attempt was not accepted or duplicated. Retrying
        // that exact next logical packet succeeds once space is available.
        assert!(peers.worker().send(&json!({"counter":sent})).unwrap());
        assert_eq!(
            peers.parent.receive::<Value>().unwrap(),
            Some(json!({"counter":sent}))
        );
        assert!(peers.parent.receive::<Value>().unwrap().is_none());
    }

    #[test]
    fn claim_path_traps_are_refused_without_replacing_existing_data() {
        let scratch = Scratch::new();
        let real = scratch.0.join("real");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&real)
            .unwrap();
        file.write_all(b"existing data").unwrap();
        let link = scratch.0.join("symlink");
        symlink(&real, &link).unwrap();
        assert!(acquire_claim(&link).is_err());
        let hard = scratch.0.join("hardlink");
        fs::hard_link(&real, &hard).unwrap();
        assert!(acquire_claim(&hard).is_err());
        assert!(acquire_claim(&real).is_err());
        fs::remove_file(hard).unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(acquire_claim(&real).is_err());
        assert!(acquire_claim(&scratch.0).is_err());
        assert!(acquire_claim(Path::new("relative-claim")).is_err());
        // rustix has no mkfifo API on Darwin. The Linux target additionally
        // proves that NONBLOCK prevents an attacker-supplied FIFO open hanging.
        #[cfg(target_os = "linux")]
        {
            let fifo = scratch.0.join("fifo");
            rustix::fs::mkfifoat(
                rustix::fs::CWD,
                &fifo,
                rustix::fs::Mode::from_bits_retain(0o600),
            )
            .unwrap();
            assert!(acquire_claim(&fifo).is_err());
        }
        assert_eq!(fs::read(real).unwrap(), b"existing data");
    }

    #[test]
    fn releasing_a_claim_preserves_its_inode_and_private_mode() {
        let scratch = Scratch::new();
        let first = acquire_claim(&scratch.claim()).unwrap();
        let metadata = first.metadata().unwrap();
        assert_eq!(metadata.mode() & 0o7777, 0o600);
        assert_eq!(metadata.uid(), geteuid().as_raw());
        assert_eq!(metadata.nlink(), 1);
        assert_eq!(
            acquire_claim(&scratch.claim()).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(first);
        let second = when_free(|| acquire_claim(&scratch.claim())).unwrap();
        assert_eq!(second.metadata().unwrap().ino(), metadata.ino());
    }

    fn subprocess(scratch: &Scratch, mode: &str) -> Transport {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .arg("--exact")
            .arg("reminders::transport::tests::child_fixture")
            .arg("--nocapture")
            .env("LAMP_SCHEDULE_TRANSPORT_TEST_MODE", mode)
            .env("LAMP_SCHEDULE_TRANSPORT_TEST_CLAIM", scratch.claim());
        Transport::spawn_command(command).unwrap()
    }
    fn ready(transport: &mut Transport) {
        let mut received = None;
        wait(|| {
            received = transport.receive::<Value>().unwrap();
            received.is_some()
        });
        assert!(transport.claim_held());
        assert_eq!(received, Some(json!({"ready":true})));
    }
    #[test]
    fn real_child_exit_does_not_drop_the_parent_claim_or_start_replacement() {
        let scratch = Scratch::new();
        let mut transport = subprocess(&scratch, "exit");
        ready(&mut transport);
        wait(|| transport.try_exit().unwrap());
        assert!(
            transport
                .child
                .as_mut()
                .unwrap()
                .try_wait()
                .unwrap()
                .unwrap()
                .success()
        );
        assert!(transport.claim_held());
        assert_eq!(
            acquire_claim(&scratch.claim()).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(transport);
        assert!(when_free(|| acquire_claim(&scratch.claim())).is_ok());
    }
    #[test]
    fn stop_signals_once_and_actual_child_exit_can_be_polled_without_waiting() {
        let scratch = Scratch::new();
        let mut transport = subprocess(&scratch, "hold");
        wait(|| {
            transport.poll().unwrap();
            transport.claim_held()
        });
        transport.stop();
        transport.stop();
        assert!(transport.stopped);
        wait(|| transport.try_exit().unwrap());
        assert!(transport.claim_held());
        assert!(transport.send(&json!({"late":true})).is_err());
    }
    #[test]
    fn production_retention_survives_drop_until_the_parent_process_exits() {
        let scratch = Scratch::new();
        let mut transport = subprocess(&scratch, "retain");
        ready(&mut transport);
        wait(|| transport.try_exit().unwrap());
        assert!(
            transport
                .child
                .as_mut()
                .unwrap()
                .try_wait()
                .unwrap()
                .unwrap()
                .success()
        );
        // The child asserts retention before actual process exit. The kernel,
        // rather than Transport Drop, releases the intentionally retained FD.
        assert!(when_free(|| acquire_claim(&scratch.0.join("retained-inner"))).is_ok());
    }
    #[test]
    fn child_fixture() {
        let Ok(mode) = std::env::var("LAMP_SCHEDULE_TRANSPORT_TEST_MODE") else {
            return;
        };
        let path = PathBuf::from(std::env::var_os("LAMP_SCHEDULE_TRANSPORT_TEST_CLAIM").unwrap());
        let mut worker = WorkerTransport::from_stdin_at(&path).unwrap();
        if mode == "retain" {
            let inner_path = path.with_file_name("retained-inner");
            let (a, b) = pair().unwrap();
            let child_side = WorkerTransport::from_socket_at(b, &inner_path).unwrap();
            let mut parent_side = parent(a);
            parent_side.poll().unwrap();
            parent_side.exited = true;
            parent_side.retain_claim_until_process_exit();
            drop(child_side);
            drop(parent_side);
            assert_eq!(
                acquire_claim(&inner_path).unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
        }
        wait(|| worker.send(&json!({"ready":true})).unwrap());
        if mode == "hold" {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    // Each nested fixture shares a ten-second budget across its startup and
    // protocol handshakes. This is a test timeout, not a latency claim.
    // Keep the existing two-second helper and its callers unchanged.
    fn boundary_wait(deadline: Instant, mut ready: impl FnMut() -> bool) {
        while !ready() {
            assert!(
                Instant::now() < deadline,
                "claim-retention fixture did not complete its bounded handshake"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn boundary_expect(
        deadline: Instant,
        expected: &str,
        mut receive: impl FnMut() -> io::Result<Option<String>>,
    ) {
        boundary_wait(deadline, || match receive().unwrap() {
            Some(actual) => {
                assert_eq!(actual, expected);
                true
            }
            None => false,
        });
    }
    fn boundary_message(socket: &UnixDatagram) -> io::Result<Option<String>> {
        let Some(packet) = receive_packet(socket)? else {
            return Ok(None);
        };
        assert!(packet.fds.is_empty());
        Ok(Some(serde_json::from_slice(&packet.bytes).unwrap()))
    }
    fn boundary_command(mode: &str, path: &Path) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "reminders::transport::tests::claim_retention_boundary_fixture",
                "--nocapture",
            ])
            .env("LAMP_SCHEDULE_RETENTION_TEST_MODE", mode)
            .env("LAMP_SCHEDULE_RETENTION_TEST_CLAIM", path);
        command
    }

    // Only the isolated fixture uses this guard. No other thread reaps its
    // child. Check wait ownership before every signal, including after a
    // Transport method might already have reaped it. Never signal on ECHILD.
    struct ExactChildCleanup {
        pid: Pid,
        owned: bool,
    }
    impl ExactChildCleanup {
        fn new(child: &Child) -> Self {
            Self {
                pid: Pid::from_child(child),
                owned: true,
            }
        }
        fn reap(&mut self, options: WaitOptions) -> io::Result<Option<WaitStatus>> {
            if !self.owned {
                return Err(Errno::CHILD.into());
            }
            loop {
                match waitpid(Some(self.pid), options) {
                    Ok(Some((pid, status))) => {
                        self.owned = false;
                        assert_eq!(pid, self.pid);
                        return Ok(Some(status));
                    }
                    Ok(None) => return Ok(None),
                    Err(Errno::INTR) => continue,
                    Err(error) => {
                        if error == Errno::CHILD {
                            self.owned = false;
                        }
                        return Err(error.into());
                    }
                }
            }
        }
        fn kill_and_reap(&mut self) -> io::Result<WaitStatus> {
            if let Some(status) = self.reap(WaitOptions::NOHANG)? {
                return Ok(status);
            }
            // An unreaped exact child reserves this PID until this guard's
            // wait. A concurrent exit is harmless; ESRCH still needs a reap.
            match kill_process(self.pid, Signal::KILL) {
                Ok(()) | Err(Errno::SRCH) => {}
                Err(error) => return Err(error.into()),
            }
            self.reap(WaitOptions::empty())?
                .ok_or_else(|| io::Error::other("blocking exact-child wait returned no status"))
        }
    }
    impl Drop for ExactChildCleanup {
        fn drop(&mut self) {
            if self.owned {
                // Also runs when a fixture assertion unwinds. This guard is
                // declared after Transport, so cleanup precedes its Drop.
                let _ = self.kill_and_reap();
            }
        }
    }

    fn claim_retention_boundary(mode: &str) {
        let scratch = Scratch::new();
        // Wait synchronously for the containing fixture. Outer assertions run
        // only after its bounded protocol and exact-child cleanup have ended;
        // killing this fixture early could orphan the nested consumer.
        let output = boundary_command(mode, &scratch.claim()).output().unwrap();
        assert!(
            output.status.success(),
            "{mode}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("claim-retention boundary completed")
        );
        // Both consumer and containing fixture are gone. Only actual fixture
        // exit releases the intentionally retained parent descriptor.
        assert!(when_free(|| acquire_claim(&scratch.claim())).is_ok());
    }
    #[test]
    fn term_resistant_child_drop_retains_claim_until_fixture_exit() {
        claim_retention_boundary("term-parent");
    }
    #[test]
    fn lost_wait_ownership_drop_retains_claim_until_fixture_exit() {
        claim_retention_boundary("wait-parent");
    }
    #[test]
    fn claim_retention_boundary_fixture() {
        let Ok(mode) = std::env::var("LAMP_SCHEDULE_RETENTION_TEST_MODE") else {
            return;
        };
        let path = PathBuf::from(std::env::var_os("LAMP_SCHEDULE_RETENTION_TEST_CLAIM").unwrap());
        let deadline = Instant::now() + Duration::from_secs(10);
        match mode.as_str() {
            "term-child" | "exit-child" => {
                let mut worker = WorkerTransport::from_stdin_at(&path).unwrap();
                let term = Arc::new(AtomicBool::new(false));
                // Signal installation is confined to this dedicated process,
                // and readiness is sent only after the handler is installed.
                let _registration = (mode == "term-child").then(|| {
                    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term))
                        .unwrap()
                });
                boundary_wait(deadline, || worker.send(&"ready").unwrap());
                if mode == "term-child" {
                    boundary_wait(deadline, || term.load(Ordering::SeqCst));
                    boundary_wait(deadline, || worker.send(&"term-received").unwrap());
                    boundary_expect(deadline, "after-drop", || worker.receive());
                    assert!(term.load(Ordering::SeqCst));
                    boundary_wait(deadline, || worker.send(&"alive-after-drop").unwrap());
                    // Remain alive until the exact owning fixture kills and
                    // reaps us. Missing cleanup fails within this budget.
                    boundary_wait(deadline, || false);
                } else {
                    boundary_expect(deadline, "exit", || worker.receive());
                }
            }
            "term-parent" | "wait-parent" => {
                let child_mode = if mode == "term-parent" {
                    "term-child"
                } else {
                    "exit-child"
                };
                let mut transport =
                    Transport::spawn_command(boundary_command(child_mode, &path)).unwrap();
                let mut cleanup = ExactChildCleanup::new(transport.child.as_ref().unwrap());
                // Exercise the ambiguous-child fallback itself. The separate
                // production-retention test covers retain_claim=true; this
                // fixture does not execute Transport::spawn's full entry.
                assert!(!transport.retain_claim);
                boundary_expect(deadline, "ready", || transport.receive());
                assert!(transport.claim_held());
                if mode == "term-parent" {
                    // Keep only a socket duplicate for positive evidence after
                    // stop/Drop; it does not duplicate the claim descriptor.
                    let evidence = transport.socket.try_clone().unwrap();
                    transport.stop();
                    boundary_expect(deadline, "term-received", || boundary_message(&evidence));
                    assert!(!transport.try_exit().unwrap());
                    drop(transport);
                    let probe = encode(&"after-drop").unwrap();
                    boundary_wait(deadline, || send_packet(&evidence, &probe, None).unwrap());
                    boundary_expect(deadline, "alive-after-drop", || boundary_message(&evidence));
                    assert!(cleanup.reap(WaitOptions::NOHANG).unwrap().is_none());
                    let status = cleanup.kill_and_reap().unwrap();
                    assert_eq!(
                        status.terminating_signal(),
                        Some(signal_hook::consts::SIGKILL)
                    );
                } else {
                    boundary_wait(deadline, || transport.send(&"exit").unwrap());
                    let mut status = None;
                    boundary_wait(deadline, || {
                        status = cleanup.reap(WaitOptions::NOHANG).unwrap();
                        status.is_some()
                    });
                    assert_eq!(status.unwrap().exit_status(), Some(0));
                    assert!(!cleanup.owned);
                    // Reaping through rustix leaves std::process::Child's
                    // cached status unset: this is a real ownership error.
                    assert_eq!(
                        transport
                            .child
                            .as_mut()
                            .unwrap()
                            .try_wait()
                            .unwrap_err()
                            .raw_os_error(),
                        Some(Errno::CHILD.raw_os_error())
                    );
                    assert_eq!(
                        transport.try_exit().unwrap_err().raw_os_error(),
                        Some(Errno::CHILD.raw_os_error())
                    );
                    assert!(!transport.exited);
                    drop(transport);
                }
                assert!(!cleanup.owned);
                assert_eq!(
                    acquire_claim(&path).unwrap_err().kind(),
                    io::ErrorKind::WouldBlock
                );
                println!("claim-retention boundary completed");
            }
            other => panic!("unknown claim-retention fixture mode: {other}"),
        }
    }

    /// Run alone after builds/tests are idle; this is host IPC timing, not a
    /// lamp/device measurement or a hard scheduler guarantee. There is no
    /// threshold assertion. Initial descriptor transfer is inside first_poll.
    #[test]
    #[ignore = "host-only timing probe; run alone with --ignored --exact --nocapture"]
    fn poll_cost() {
        fn measured(parent: &mut Transport) -> f64 {
            let started = Instant::now();
            parent.poll().unwrap();
            started.elapsed().as_secs_f64() * 1000.0
        }
        fn summary(first: f64, samples: Vec<f64>) -> Value {
            let mut sorted = samples.clone();
            sorted.sort_by(f64::total_cmp);
            json!({
                "first_poll_ms":first,
                "warm_count":samples.len(),
                "warm_p50_ms":(sorted[49] + sorted[50]) / 2.0,
                "warm_p95_ms":sorted[94],
                "warm_max_ms":sorted[99],
                "all_max_ms":first.max(sorted[99]),
                "warm_samples_ms":samples,
            })
        }
        let mut idle = Peers::new();
        let idle_first = measured(&mut idle.parent);
        let idle_warm = (0..100).map(|_| measured(&mut idle.parent)).collect();

        let mut incoming = Peers::new();
        let message = json!({"bounded_fixture":"x".repeat(300)});
        let fill_incoming = |peers: &mut Peers| {
            for _ in 0..1024 {
                if !peers.worker().send(&message).unwrap() {
                    return;
                }
            }
            panic!("fixture failed to reach its bounded socket capacity");
        };
        fill_incoming(&mut incoming);
        let incoming_first = measured(&mut incoming.parent);
        let incoming_warm = (0..100)
            .map(|_| {
                assert!(incoming.parent.pending.take().is_some());
                fill_incoming(&mut incoming);
                measured(&mut incoming.parent)
            })
            .collect();

        let mut withheld = Peers::new();
        let bytes = encode(&message).unwrap();
        let mut blocked = false;
        for _ in 0..1024 {
            // Authored queue setup before timing. The peer deliberately never
            // receives these parent-to-child packets during this condition.
            if !send_packet(&withheld.parent.socket, &bytes, None).unwrap() {
                blocked = true;
                break;
            }
        }
        assert!(blocked);
        let withheld_first = measured(&mut withheld.parent);
        let withheld_warm = (0..100).map(|_| measured(&mut withheld.parent)).collect();
        println!(
            "{}",
            json!({
                "kind":"schedule_consumer_transport_poll_cost",
                "machine":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},
                "boundary":"Instant immediately before Transport::poll through return; anonymous in-process Unix datagram peer; first poll includes claim descriptor receive/CLOEXEC; excludes spawn, file/lock acquisition, serialization/deserialization, coordinator work, storage, network, providers and hardware",
                "basis":"real elapsed time on the executing host; fixture queue conditions are authored",
                "reference_poll_target_ms":2,
                "target_is_not_asserted_or_a_hard_os_guarantee":true,
                "percentiles":"100 warm samples; p50 averages middle two; p95 nearest rank",
                "idle_peer":summary(idle_first,idle_warm),
                "saturated_incoming_one_packet_per_poll":summary(incoming_first,incoming_warm),
                "outgoing_full_peer_withholds_reads":summary(withheld_first,withheld_warm),
            })
        );
    }
}
