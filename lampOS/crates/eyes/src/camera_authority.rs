//! Eyes-owned camera-hold witness and bounded off-camera grant service.
//!
//! Only successful witness shutdown is revocation's kernel linearization.
//! The authority mutex contains scalar/Arc work only; never socket I/O, pair
//! creation, publication or Permit's `at` lock. No observer is ever shut down.
use lamp_ipc::camera_authority::{Grant, HoldWitness, Token};
use rustix::{
    io::Errno,
    net::{RecvAncillaryBuffer, RecvFlags, ReturnFlags, recvmsg},
};
use std::{
    fs::{self, DirBuilder, Permissions},
    io::{self, IoSliceMut},
    mem::MaybeUninit,
    num::NonZeroU64,
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
const CONNECTIONS: usize = 4;
const TICK: Duration = Duration::from_millis(5);
const GRANT_FOR: Duration = Duration::from_millis(300);

/// Original permit/capture boundary, distinct from recognition generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub publisher: [u8; 16],
    pub hold: u64,
    pub began_us: u64,
    pub until_us: u64,
}
/// A retired private hold must not be recreated under the same token. The
/// permit owner can roll its logical capture hold under the original receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermitUpdate {
    Accepted,
    NeedNewHold,
    Unavailable,
}
struct SharedWitness {
    witness: HoldWitness,
    shutdown_complete: AtomicBool,
}
impl SharedWitness {
    fn revoke(&self) -> io::Result<()> {
        if self.shutdown_complete.load(Ordering::Acquire) {
            return Ok(());
        }
        match self.witness.revoke() {
            Ok(()) => {
                self.shutdown_complete.store(true, Ordering::Release);
                Ok(())
            }
            Err(_) if self.shutdown_complete.load(Ordering::Acquire) => Ok(()),
            Err(error) => Err(error),
        }
    }
    fn grant(&self, nonce: NonZeroU64, deadline: Instant) -> io::Result<Grant> {
        self.witness.grant(nonce, deadline)
    }
}
#[derive(Default)]
struct State {
    serial: u64,
    highest_hold: u64,
    desired: Option<Binding>,
    witness: Option<Arc<SharedWitness>>,
    closing: bool,
    failed: bool,
}
/// A clone does not own the service thread and never blocks on its shutdown.
pub struct Authority {
    publisher: [u8; 16],
    state: Mutex<State>,
}
impl Authority {
    fn lock(&self) -> MutexGuard<'_, State> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                state.failed = true;
                state
            }
        }
    }
    /// Close before touching the camera/publication lock. Retained grant Arcs
    /// cannot defer the explicit shutdown. Repeated closes remain denied.
    pub fn close(&self) -> io::Result<()> {
        self.close_matching(None).map(|_| ())
    }
    fn close_matching(&self, expected: Option<(u64, Option<Binding>)>) -> io::Result<bool> {
        let (serial, witness) = {
            let mut state = self.lock();
            if expected
                .is_some_and(|(serial, desired)| state.serial != serial || state.desired != desired)
            {
                // Stale work did nothing; it is NOT completed revocation.
                return Ok(false);
            }
            state.desired = None;
            state.closing = true;
            match state.serial.checked_add(1) {
                Some(next) => state.serial = next,
                None => state.failed = true,
            }
            // Keep the revoker visible until completion. A concurrent explicit
            // close must not return early while an expiry owns the only Arc.
            (state.serial, state.witness.clone())
        };
        let result = witness.as_ref().map_or(Ok(()), |witness| witness.revoke());
        let complete = self.finish_revoke(serial, witness.as_ref(), &result);
        drop(witness);
        if complete {
            Ok(true)
        } else {
            result.map(|()| false)
        }
    }
    fn finish_revoke(
        &self,
        serial: u64,
        witness: Option<&Arc<SharedWitness>>,
        result: &io::Result<()>,
    ) -> bool {
        let complete =
            witness.is_none_or(|witness| witness.shutdown_complete.load(Ordering::Acquire));
        let retired = {
            let mut state = self.lock();
            // NotConnected without a recorded successful syscall is not proof.
            // It also must not poison a concurrent successful close forever.
            if !complete
                && result
                    .as_ref()
                    .is_err_and(|error| error.kind() != io::ErrorKind::NotConnected)
            {
                state.failed = true;
            }
            let same = witness.is_some_and(|witness| {
                state
                    .witness
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, witness))
            });
            if complete
                && (same
                    || (witness.is_none() && state.serial == serial && state.witness.is_none()))
            {
                state.closing = false;
                state.witness.take()
            } else {
                None
            }
        };
        // Arc identity, not stale serial, lets an earlier actual success finish
        // the SAME closing record without retiring any newer hold/pair.
        drop(retired);
        complete
    }
    /// Only an actual completed retirement can authorize a new capture floor.
    /// A stale expiry may overlap another close before its kernel syscall.
    fn retire_for_rollover(&self, expected: (u64, Option<Binding>)) -> PermitUpdate {
        match self.close_matching(Some(expected)) {
            Ok(true) => PermitUpdate::NeedNewHold,
            Ok(false) | Err(_) => PermitUpdate::Unavailable,
        }
    }
    /// Called only after the existing permit timestamp/hold update. Original
    /// receipt times are passed through even if that lock was delayed.
    pub fn permit(&self, hold: u64, began_us: u64, until_us: u64) -> PermitUpdate {
        let now = lamp_ipc::monotonic_us();
        let (serial, old) = {
            let mut state = self.lock();
            if state.failed
                || state.closing
                || began_us == 0
                || began_us > now
                || until_us.checked_sub(now).is_none_or(|left| {
                    left == 0 || u128::from(left) > crate::permit::STALE.as_micros()
                })
            {
                return PermitUpdate::Unavailable;
            }
            if let Some(desired) = state.desired.as_mut()
                && desired.hold == hold
                && desired.until_us > now
            {
                desired.until_us = until_us;
                return PermitUpdate::Accepted;
            }
            if hold != 0 && hold == state.highest_hold {
                let expected = (state.serial, state.desired);
                drop(state);
                return self.retire_for_rollover(expected);
            }
            if hold != 0 && hold < state.highest_hold {
                // A delayed older callback cannot recreate its token or poison
                // the currently newer authority. Highest hold never decreases.
                return PermitUpdate::Unavailable;
            }
            if hold == 0 {
                state.failed = true;
                state.desired = None;
            } else if let Some(next) = state.serial.checked_add(1) {
                state.serial = next;
                state.highest_hold = hold;
                state.desired = Some(Binding {
                    publisher: self.publisher,
                    hold,
                    began_us,
                    until_us,
                });
            } else {
                state.failed = true;
                state.desired = None;
            }
            state.closing = state.witness.is_some();
            (state.serial, state.witness.clone())
        };
        let result = old.as_ref().map_or(Ok(()), |old| old.revoke());
        let complete = self.finish_revoke(serial, old.as_ref(), &result);
        drop(old);
        if complete && !self.lock().failed {
            PermitUpdate::Accepted
        } else {
            PermitUpdate::Unavailable
        }
    }
    /// Snapshot only; pair creation is the worker's work. A source cannot bind
    /// a queued frame from an earlier hold, nor publish before service ready.
    pub fn frame(&self, hold: Option<u64>, frame_us: u64) -> Option<Binding> {
        let state = self.lock();
        let desired = state.desired?;
        (!state.failed
            && !state.closing
            && state.witness.is_some()
            && hold == Some(desired.hold)
            && frame_us >= desired.began_us
            && frame_us > 0
            && frame_us <= lamp_ipc::monotonic_us()
            && lamp_ipc::monotonic_us() < desired.until_us)
            .then_some(desired)
    }
    fn witness(&self, token: Token) -> Option<Arc<SharedWitness>> {
        let state = self.lock();
        let desired = state.desired?;
        (!state.failed
            && !state.closing
            && lamp_ipc::monotonic_us() < desired.until_us
            && Token::new(self.publisher, desired.hold) == Some(token))
        .then(|| state.witness.clone())
        .flatten()
    }
    fn maintain(&self) {
        let job = {
            let state = self.lock();
            if state.failed || state.closing {
                None
            } else {
                state
                    .desired
                    .map(|desired| (state.serial, desired, state.witness.is_some()))
            }
        };
        let Some((serial, desired, ready)) = job else {
            return;
        };
        if lamp_ipc::monotonic_us() >= desired.until_us {
            let _ = self.close_matching(Some((serial, Some(desired))));
            return;
        }
        if ready {
            return;
        }
        let Some(token) = Token::new(self.publisher, desired.hold) else {
            return;
        };
        let witness = match HoldWitness::new(token).map(|witness| {
            Arc::new(SharedWitness {
                witness,
                shutdown_complete: AtomicBool::new(false),
            })
        }) {
            Ok(witness) => witness,
            Err(_) => {
                let mut state = self.lock();
                if state.serial == serial {
                    state.failed = true;
                }
                return;
            }
        };
        let mut candidate = Some(witness);
        {
            let mut state = self.lock();
            if !state.failed
                && !state.closing
                && state.serial == serial
                && state.desired.is_some_and(|current| {
                    current.hold == desired.hold && lamp_ipc::monotonic_us() < current.until_us
                })
                && state.witness.is_none()
            {
                state.witness = candidate.take();
            }
        }
        // Failed installation drops the last Arc (and its revoker) here.
        drop(candidate);
    }
}

/// Append without UTF-8 conversion. The private directory is a sibling of the
/// configured known file, not a new globally writable protocol location.
pub fn socket_path(known: &Path) -> io::Result<PathBuf> {
    let name = known
        .file_name()
        .ok_or_else(|| invalid("known file has no basename"))?;
    let mut directory = name.to_os_string();
    directory.push(".authority.d");
    let path = known.with_file_name(directory).join("hold.sock");
    if !path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
        || path.as_os_str().as_bytes().len() >= 100
    {
        return Err(invalid(
            "camera authority socket path is not bounded absolute",
        ));
    }
    Ok(path)
}
fn invalid(why: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why)
}
fn bind(known: &Path) -> io::Result<(UnixListener, PathBuf)> {
    let path = socket_path(known)?;
    let directory = path
        .parent()
        .ok_or_else(|| invalid("missing authority directory"))?;
    let parent = directory
        .parent()
        .ok_or_else(|| invalid("missing known parent"))?;
    let uid = rustix::process::geteuid().as_raw();
    for ancestor in parent.ancestors() {
        if fs::symlink_metadata(ancestor)?.file_type().is_symlink() {
            return Err(invalid("symlink in authority parent"));
        }
    }
    let metadata = fs::symlink_metadata(parent)?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o022 != 0 {
        return Err(invalid("known parent is not owned and protected"));
    }
    match DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(directory)?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o777 != 0o700 {
        return Err(invalid("authority directory is not private and owned"));
    }
    match fs::symlink_metadata(&path) {
        Ok(_) => return Err(invalid("authority path already exists")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(&path)?;
    let metadata = fs::symlink_metadata(&path)?;
    let identity = (metadata.dev(), metadata.ino());
    let setup = (|| {
        fs::set_permissions(&path, Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        rustix::net::listen(&listener, CONNECTIONS as i32)?;
        Ok::<_, io::Error>(())
    })();
    if let Err(error) = setup {
        remove_owned_socket(&path, identity);
        return Err(error);
    }
    Ok((listener, path))
}
fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    #[cfg(target_os = "linux")]
    {
        Ok(rustix::net::sockopt::socket_peercred(stream)?.uid.as_raw())
    }
    #[cfg(target_os = "macos")]
    {
        nix::unistd::getpeereid(stream)
            .map(|(uid, _)| uid.as_raw())
            .map_err(Into::into)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = stream;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "camera authority peer credentials",
        ))
    }
}
struct Connection {
    stream: UnixStream,
    frame: [u8; 49],
    used: usize,
    deadline: Instant,
    grant: Option<Grant>,
}
impl Connection {
    fn step(&mut self, authority: &Authority) -> io::Result<bool> {
        if Instant::now() >= self.deadline {
            return Err(invalid("grant request expired"));
        }
        if let Some(grant) = self.grant.as_mut() {
            return grant.try_send(&self.stream);
        }
        let mut storage = [MaybeUninit::uninit();
            rustix::cmsg_space!(ScmRights(lamp_ipc::camera_authority::CONTROL_FD_CAPACITY))];
        let mut ancillary = RecvAncillaryBuffer::new(&mut storage);
        let flags = RecvFlags::DONTWAIT;
        #[cfg(target_os = "linux")]
        let flags = flags | RecvFlags::CMSG_CLOEXEC;
        let received = match recvmsg(
            &self.stream,
            &mut [IoSliceMut::new(&mut self.frame[self.used..])],
            &mut ancillary,
            flags,
        ) {
            Ok(received) => received,
            Err(Errno::AGAIN | Errno::INTR) => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        // Requests carry no rights. Draining safely closes every unexpected FD.
        let mut rights = false;
        for message in ancillary.drain() {
            rights = true;
            drop(message);
        }
        if rights
            || received
                .flags
                .intersects(ReturnFlags::TRUNC | ReturnFlags::CTRUNC)
        {
            return Err(invalid("grant request has ancillary data"));
        }
        self.used += received.bytes;
        if self.used > 48 {
            return Err(invalid("oversized grant request"));
        }
        if received.bytes != 0 {
            return Ok(false);
        }
        if self.used != 48 || &self.frame[..8] != b"LMPASK03" || self.frame[40..48] != [0; 8] {
            return Err(invalid("bad grant request"));
        }
        let mut publisher = [0; 16];
        publisher.copy_from_slice(&self.frame[8..24]);
        let hold = u64::from_be_bytes(self.frame[24..32].try_into().map_err(|_| invalid("hold"))?);
        let nonce = NonZeroU64::new(u64::from_be_bytes(
            self.frame[32..40]
                .try_into()
                .map_err(|_| invalid("nonce"))?,
        ))
        .ok_or_else(|| invalid("zero grant nonce"))?;
        let token = Token::new(publisher, hold).ok_or_else(|| invalid("zero grant token"))?;
        let witness = authority
            .witness(token)
            .ok_or_else(|| invalid("grant hold unavailable"))?;
        let grant = witness.grant(nonce, self.deadline)?;
        // A close after this snapshot only yields an EOF observer; the client
        // must perform the primitive's post-handshake liveness check.
        if authority.witness(token).is_none() {
            return Err(invalid("grant hold retired"));
        }
        self.grant = Some(grant);
        Ok(false)
    }
}

/// Owns exactly one worker and four connection slots. Startup I/O is not on the
/// camera path; failures leave the ordinary camera independent and private
/// source unavailable. Stop revokes before joining; no detached grant worker.
pub struct Service {
    authority: Arc<Authority>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    path: PathBuf,
    socket_identity: (u64, u64),
}
impl Service {
    pub fn start(known: &Path, publisher: [u8; 16]) -> io::Result<Self> {
        if Token::new(publisher, 1).is_none() {
            return Err(invalid("zero publisher incarnation"));
        }
        let (listener, path) = bind(known)?;
        let metadata = fs::symlink_metadata(&path)?;
        let socket_identity = (metadata.dev(), metadata.ino());
        let authority = Arc::new(Authority {
            publisher,
            state: Mutex::new(State::default()),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let owner = authority.clone();
        let ended = stop.clone();
        let worker = thread::Builder::new()
            .name("camera-grants".into())
            .spawn(move || {
                let mut slots: [Option<Connection>; CONNECTIONS] = std::array::from_fn(|_| None);
                while !ended.load(Ordering::Acquire) {
                    owner.maintain();
                    if let Some(slot) = slots.iter_mut().find(|slot| slot.is_none()) {
                        match listener.accept() {
                            Ok((stream, _)) => {
                                // Accepted sockets need explicit control-only
                                // nonblocking mode on every platform.
                                let uid = stream
                                    .set_nonblocking(true)
                                    .and_then(|()| peer_uid(&stream))
                                    .ok();
                                if uid.is_some_and(|uid| {
                                    uid == 0 || uid == rustix::process::geteuid().as_raw()
                                }) {
                                    *slot = Some(Connection {
                                        stream,
                                        frame: [0; 49],
                                        used: 0,
                                        deadline: Instant::now() + GRANT_FOR,
                                        grant: None,
                                    });
                                }
                            }
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                            Err(_) => break,
                        }
                    }
                    for slot in &mut slots {
                        let done = slot.as_mut().is_some_and(|connection| {
                            !matches!(connection.step(&owner), Ok(false))
                        });
                        if done {
                            *slot = None;
                        }
                    }
                    thread::park_timeout(TICK);
                }
                let _ = owner.close();
            });
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                remove_owned_socket(&path, socket_identity);
                return Err(error);
            }
        };
        Ok(Self {
            authority,
            stop,
            worker: Some(worker),
            path,
            socket_identity,
        })
    }
    pub fn authority(&self) -> Arc<Authority> {
        self.authority.clone()
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.authority.close();
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
        remove_owned_socket(&self.path, self.socket_identity);
    }
}
fn remove_owned_socket(path: &Path, identity: (u64, u64)) {
    // Never unlink a foreign replacement, including another same-UID server.
    if fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.file_type().is_socket()
            && (metadata.dev(), metadata.ino()) == identity
            && metadata.uid() == rustix::process::geteuid().as_raw()
    }) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner() -> Authority {
        Authority {
            publisher: [7; 16],
            state: Mutex::new(State::default()),
        }
    }
    fn open(owner: &Authority, hold: u64) -> Arc<SharedWitness> {
        let now = lamp_ipc::monotonic_us();
        owner.permit(hold, now, now + 1_500_000);
        owner.maintain();
        owner.witness(Token::new([7; 16], hold).unwrap()).unwrap()
    }
    #[test]
    fn delayed_record_of_real_success_does_not_poison_a_concurrent_close() {
        let owner = owner();
        let old = open(&owner, 1);
        // The first closer actually completed the kernel syscall, but is
        // paused before recording it. A second call differs on Linux/Mac.
        let actual_success = old.witness.revoke();
        assert!(actual_success.is_ok());
        let second = owner.close();
        if let Err(error) = &second {
            assert_eq!(error.kind(), io::ErrorKind::NotConnected);
            let state = owner.lock();
            assert!(state.closing && !state.failed);
        }
        // Resume the first closer's completion record, based on its actual
        // saved syscall result, not the primitive's earlier revoked bit.
        old.shutdown_complete.store(true, Ordering::Release);
        let serial = owner.lock().serial;
        assert!(owner.finish_revoke(serial, Some(&old), &actual_success));
        let fresh = open(&owner, 3);
        assert!(!Arc::ptr_eq(&old, &fresh));
        assert!(!owner.lock().failed);
    }
    #[test]
    fn retired_equal_hold_requests_rollover_and_genuinely_larger_hold_recovers() {
        let owner = owner();
        let old = open(&owner, 1);
        owner.close().unwrap();
        let now = lamp_ipc::monotonic_us();
        assert_eq!(
            owner.permit(1, now, now + 1_500_000),
            PermitUpdate::NeedNewHold
        );
        assert!(owner.witness(Token::new([7; 16], 1).unwrap()).is_none());
        assert!(old.shutdown_complete.load(Ordering::Acquire));
        let fresh = open(&owner, 3);
        assert!(!Arc::ptr_eq(&old, &fresh));
        assert!(!owner.lock().failed);
        let now = lamp_ipc::monotonic_us();
        assert_eq!(
            owner.permit(1, now, now + 1_500_000),
            PermitUpdate::Unavailable
        );
        assert!(Arc::ptr_eq(
            &fresh,
            &owner.witness(Token::new([7; 16], 3).unwrap()).unwrap()
        ));
        owner.close().unwrap();
        let newest = open(&owner, 5);
        assert!(!Arc::ptr_eq(&fresh, &newest));
    }
    #[test]
    fn delayed_old_completion_cannot_retire_a_new_pair() {
        let owner = owner();
        let old = open(&owner, 1);
        owner.close().unwrap();
        let old_serial = owner.lock().serial;
        let fresh = open(&owner, 3);
        let actual_success = old.revoke();
        assert!(actual_success.is_ok());
        assert!(owner.finish_revoke(old_serial, Some(&old), &actual_success));
        let current = owner.witness(Token::new([7; 16], 3).unwrap()).unwrap();
        assert!(Arc::ptr_eq(&current, &fresh));
        assert!(owner.frame(Some(3), lamp_ipc::monotonic_us()).is_some());
    }

    /// The stale expiry snapshot is a no-op while another closer still owns
    /// the live old endpoint. It cannot authorize an early new capture floor.
    #[test]
    fn stale_expected_close_cannot_roll_capture_before_real_shutdown_completes() {
        use lamp_ipc::camera_authority::{ReceiveGrant, Ticket};

        fn transfer(witness: &SharedWitness, hold: u64) -> Ticket {
            let (send, receive) = UnixStream::pair().unwrap();
            receive.set_nonblocking(true).unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            let nonce = NonZeroU64::new(17).unwrap();
            let mut grant = witness.grant(nonce, deadline).unwrap();
            while !grant.try_send(&send).unwrap() {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
            let mut receiver =
                ReceiveGrant::new(Token::new([7; 16], hold).unwrap(), nonce, deadline);
            loop {
                if let Some(ticket) = receiver.try_receive(&receive).unwrap() {
                    return ticket;
                }
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
        }

        let owner = owner();
        let old = open(&owner, 1);
        let old_ticket = transfer(&old, 1);
        assert!(old_ticket.is_open());
        let renewed_receipt_us = lamp_ipc::monotonic_us();
        let original_until = renewed_receipt_us.checked_add(1_500_000).unwrap();
        let expected = {
            let mut state = owner.lock();
            // Private expiry can lag a newly received true heartbeat's at
            // update. No system clock or live kernel endpoint is replaced.
            state.desired.as_mut().unwrap().until_us = renewed_receipt_us - 1;
            (state.serial, state.desired)
        };
        assert!(expected.1.unwrap().until_us < renewed_receipt_us);
        {
            // Exact state left by a winning close's mutex phase, paused
            // BEFORE its real syscall. The witness remains in the owner.
            let mut state = owner.lock();
            state.desired = None;
            state.closing = true;
            state.serial = state.serial.checked_add(1).unwrap();
        }
        assert_eq!(
            owner.retire_for_rollover(expected),
            PermitUpdate::Unavailable
        );
        assert!(old_ticket.is_open(), "no shutdown has completed yet");
        {
            let state = owner.lock();
            assert!(state.closing && !state.failed);
            assert!(Arc::ptr_eq(state.witness.as_ref().unwrap(), &old));
            assert_eq!(state.highest_hold, 1);
        }
        let queued_before_shutdown = lamp_ipc::monotonic_us();
        // Finish using the real production explicit-close path and kernel FD.
        owner.close().unwrap();
        assert!(!old_ticket.is_open());
        let completed_us = lamp_ipc::monotonic_us();
        let retired = {
            let state = owner.lock();
            assert!(!state.closing && !state.failed && state.witness.is_none());
            (state.serial, state.desired)
        };
        assert_eq!(
            owner.retire_for_rollover(retired),
            PermitUpdate::NeedNewHold
        );
        let floor_deadline = Instant::now() + Duration::from_secs(3);
        let floor = loop {
            let now = lamp_ipc::monotonic_us();
            if now > queued_before_shutdown && now >= completed_us {
                break now;
            }
            assert!(Instant::now() < floor_deadline);
            thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(
            owner.permit(2, floor, original_until),
            PermitUpdate::Accepted
        );
        owner.maintain();
        let fresh = owner.witness(Token::new([7; 16], 2).unwrap()).unwrap();
        assert!(!Arc::ptr_eq(&old, &fresh));
        let fresh_ticket = transfer(&fresh, 2);
        assert!(fresh_ticket.is_open());
        assert!(!old_ticket.is_open());
        assert!(
            owner.frame(Some(2), queued_before_shutdown).is_none(),
            "a new read-time hold cannot rebind a pre-shutdown queued image"
        );
        let binding = owner.frame(Some(2), floor).unwrap();
        assert_eq!(binding.began_us, floor);
        assert_eq!(binding.until_us, original_until, "no receipt restamp");
        assert!(binding.began_us >= completed_us);
        assert!(owner.frame(Some(1), floor).is_none());
        owner.close().unwrap();
        assert!(!fresh_ticket.is_open());
    }
}
