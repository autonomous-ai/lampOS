//! One revocable kernel witness per camera hold; no camera policy or server.
//!
//! Callers authenticate the connected grant peer and bind the token to their
//! current producer/source before transferring a ticket. An open ticket proves
//! only that this witness has not been shut down: source age, identity, capture
//! bounds, request ownership and expiry remain independent mandatory checks.
//! Grant steps never connect, wait or spin; the caller supplies the deadline and
//! readiness scheduling. No capture callback should create or transfer a pair.

use rustix::{
    io::{Errno, FdFlags, fcntl_getfd, fcntl_setfd},
    net::{
        AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags,
        SendAncillaryBuffer, SendAncillaryMessage, SendFlags, SocketType, getpeername, getsockname,
        recv, recvmsg, sendmsg, sockopt,
    },
};
use std::{
    io::{self, IoSlice, IoSliceMut},
    mem::MaybeUninit,
    net::Shutdown,
    num::NonZeroU64,
    os::fd::{AsFd, OwnedFd},
    os::unix::net::UnixStream,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

const SIZE: usize = 48;
const MAGIC: &[u8; 8] = b"LMPCTK03";

/// Ancillary storage only; the protocol still accepts EXACTLY one descriptor.
/// Darwin sockargs bounds one control mbuf by MCLBYTES=2048; AF_UNIX accepts
/// one exact SCM_RIGHTS cmsg, and soreceive stops before the next control record.
/// cmsg_space(512 ints) covers that bound plus header/alignment. A smaller
/// buffer triggers rustix1.1.5's Darwin truncated-cmsg drain/drop bug. Keep the
/// two-FD Linux buffer so its real CTRUNC refusal/cleanup remains exercised.
/// See the pinned Apple source proof in docs/reuse-decisions.md. Other request
/// receivers on this protocol must use the SAME capacity, never copy `2`.
#[cfg(target_os = "macos")]
pub const CONTROL_FD_CAPACITY: usize = 512;
#[cfg(not(target_os = "macos"))]
pub const CONTROL_FD_CAPACITY: usize = 2;

fn invalid(why: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why)
}
fn within(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "ticket grant expired",
        ))
    } else {
        Ok(())
    }
}
fn cloexec(fd: impl AsFd) -> io::Result<()> {
    let flags = fcntl_getfd(&fd)?;
    fcntl_setfd(fd, flags | FdFlags::CLOEXEC)?;
    Ok(())
}

/// Opaque producer incarnation and hold. Generate the publisher externally;
/// neither zero nor wraparound is an admissible authority identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    publisher: [u8; 16],
    hold: NonZeroU64,
}
impl Token {
    pub fn new(publisher: [u8; 16], hold: u64) -> Option<Self> {
        (publisher != [0; 16]).then_some(Self {
            publisher,
            hold: NonZeroU64::new(hold)?,
        })
    }

    /// A new hold, never the old one after overflow. Creation does not revoke
    /// its predecessor; the authority owner must explicitly do so first.
    pub fn next(self) -> Option<Self> {
        Self::new(self.publisher, self.hold.get().checked_add(1)?)
    }
}

/// Owns the only revoking endpoint and one observer endpoint. Every granted
/// descriptor observes the SAME kernel endpoint; one shutdown revokes all.
/// Before replacing/detaching an Arc-held witness, explicitly call `revoke`:
/// Drop alone may wait for an in-flight grant's last Arc to disappear.
#[derive(Debug)]
pub struct HoldWitness {
    token: Token,
    revoker: UnixStream,
    observer: UnixStream,
    revoked: AtomicBool,
}
impl HoldWitness {
    pub fn new(token: Token) -> io::Result<Self> {
        let (revoker, observer) = UnixStream::pair()?;
        cloexec(&revoker)?;
        cloexec(&observer)?;
        Ok(Self {
            token,
            revoker,
            observer,
            revoked: AtomicBool::new(false),
        })
    }

    /// Global revocation linearizes at successful shutdown, not when an Eyes
    /// datagram was received. It must precede any publication/inference lock.
    /// Failure still denies new grants, but is not proof of completed shutdown.
    pub fn revoke(&self) -> io::Result<()> {
        self.revoked.store(true, Ordering::Release);
        self.revoker.shutdown(Shutdown::Both)
    }

    /// Prepare one bounded reply on an already authenticated control stream.
    /// A grant racing revoke may transfer a terminal observer; the receiver
    /// checks it before exposing a ticket. The caller still checks its own
    /// current hold and original source/deadline before and after this step.
    pub fn grant(&self, nonce: NonZeroU64, deadline: Instant) -> io::Result<Grant> {
        within(deadline)?;
        if self.revoked.load(Ordering::Acquire) {
            return Err(invalid("camera witness revoked"));
        }
        let observer = self.observer.try_clone()?;
        cloexec(&observer)?;
        if self.revoked.load(Ordering::Acquire) {
            return Err(invalid("camera witness revoked"));
        }
        within(deadline)?;
        Ok(Grant {
            frame: frame(self.token, nonce),
            sent: 0,
            observer,
            deadline,
            terminal: false,
        })
    }
}
impl Drop for HoldWitness {
    fn drop(&mut self) {
        let _ = self.revoke();
    }
}
fn frame(token: Token, nonce: NonZeroU64) -> [u8; SIZE] {
    let mut bytes = [0; SIZE];
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..24].copy_from_slice(&token.publisher);
    bytes[24..32].copy_from_slice(&token.hold.get().to_be_bytes());
    bytes[32..40].copy_from_slice(&nonce.get().to_be_bytes());
    // Final eight bytes are reserved and must remain zero.
    bytes
}

/// Incremental single reply. Sends rights exactly once, with the first accepted
/// bytes. Completion half-closes only the CONTROL stream's write side, so the
/// receiver can reject trailing bytes before accepting the exact fixed frame.
/// The dedicated control stream is configured nonblocking before each send;
/// callers must not share it with blocking operations. Observer flags are never
/// changed. After any error drop this transfer and its control stream.
#[derive(Debug)]
pub struct Grant {
    frame: [u8; SIZE],
    sent: usize,
    observer: UnixStream,
    deadline: Instant,
    terminal: bool,
}
impl Grant {
    /// At most one nonblocking sendmsg and, at completion, control shutdown.
    /// False means schedule another step on readiness, never busy-spin.
    pub fn try_send(&mut self, control: &UnixStream) -> io::Result<bool> {
        if self.terminal {
            return Err(invalid("ticket transfer already ended"));
        }
        let result = self.send_step(control);
        if !matches!(result, Ok(false)) {
            self.terminal = true;
        }
        result
    }
    fn send_step(&mut self, control: &UnixStream) -> io::Result<bool> {
        within(self.deadline)?;
        validate_unix_stream_kind(control)?;
        // Darwin MSG_DONTWAIT alone does not bound the send-buffer space wait.
        // This endpoint is dedicated to control, never the shared observer.
        control.set_nonblocking(true)?;
        let rights = [self.observer.as_fd()];
        let mut storage = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut ancillary = SendAncillaryBuffer::new(&mut storage);
        if self.sent == 0 && !ancillary.push(SendAncillaryMessage::ScmRights(&rights)) {
            return Err(invalid("ticket ancillary capacity"));
        }
        let flags = SendFlags::DONTWAIT;
        #[cfg(target_os = "linux")]
        let flags = flags | SendFlags::NOSIGNAL;
        // Darwin's per-socket SIGPIPE policy is set on the control connection,
        // never on the shared observer endpoint.
        #[cfg(target_os = "macos")]
        sockopt::set_socket_nosigpipe(control, true)?;
        match sendmsg(
            control,
            &[IoSlice::new(&self.frame[self.sent..])],
            &mut ancillary,
            flags,
        ) {
            Ok(0) => return Err(invalid("ticket control accepted no bytes")),
            Ok(count) => self.sent += count,
            Err(Errno::AGAIN | Errno::INTR) => return Ok(false),
            // Darwin AF_UNIX reduces the send high-water mark while data
            // waits in its peer. Its sosendcheck reports EMSGSIZE when this
            // fixed, one-FD control exceeds that remaining mark, before the
            // normal would-block branch. No bytes/rights were accepted. This
            // is Pending only for the initial fixed control; never Ready or
            // a renewed deadline. SO_SNDBUF reports the configured total and
            // cannot preflight this transient remaining capacity.
            #[cfg(target_os = "macos")]
            Err(Errno::MSGSIZE) if self.sent == 0 => return Ok(false),
            Err(error) => return Err(error.into()),
        }
        within(self.deadline)?;
        if self.sent == SIZE {
            control.shutdown(Shutdown::Write)?;
            within(self.deadline)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// Fixed-size incremental receiver, retaining owned ancillary descriptors until
/// exact framing, control EOF and witness validation succeed. No heap buffer.
#[derive(Debug)]
pub struct ReceiveGrant {
    expected: [u8; SIZE],
    bytes: [u8; SIZE + 1],
    used: usize,
    observer: Option<OwnedFd>,
    deadline: Instant,
    terminal: bool,
}
impl ReceiveGrant {
    pub fn new(token: Token, nonce: NonZeroU64, deadline: Instant) -> Self {
        Self {
            expected: frame(token, nonce),
            bytes: [0; SIZE + 1],
            used: 0,
            observer: None,
            deadline,
            terminal: false,
        }
    }

    /// One nonblocking recvmsg. None means await readiness under the original
    /// deadline. Any error is terminal and immediately drops retained rights.
    pub fn try_receive(&mut self, control: &UnixStream) -> io::Result<Option<Ticket>> {
        if self.terminal {
            return Err(invalid("ticket receiver already ended"));
        }
        let result = self.receive_step(control);
        if !matches!(result, Ok(None)) {
            self.terminal = true;
            drop(self.observer.take());
        }
        result
    }
    fn receive_step(&mut self, control: &UnixStream) -> io::Result<Option<Ticket>> {
        within(self.deadline)?;
        // The Darwin upper bound below is for native AF_UNIX stream control,
        // not arbitrary descriptors safely wrapped in a UnixStream Rust type.
        validate_unix_stream_kind(control)?;
        let mut storage =
            [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(CONTROL_FD_CAPACITY))];
        let mut ancillary = RecvAncillaryBuffer::new(&mut storage);
        let flags = RecvFlags::DONTWAIT;
        #[cfg(target_os = "linux")]
        let flags = flags | RecvFlags::CMSG_CLOEXEC;
        let received = match recvmsg(
            control,
            &mut [IoSliceMut::new(&mut self.bytes[self.used..])],
            &mut ancillary,
            flags,
        ) {
            Ok(received) => received,
            Err(Errno::AGAIN | Errno::INTR) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut malformed = false;
        for message in ancillary.drain() {
            match message {
                RecvAncillaryMessage::ScmRights(rights) => {
                    for fd in rights {
                        if self.observer.is_some() {
                            malformed = true;
                        } else {
                            cloexec(&fd)?;
                            self.observer = Some(fd);
                        }
                    }
                }
                _ => malformed = true,
            }
        }
        if malformed
            || received
                .flags
                .intersects(ReturnFlags::TRUNC | ReturnFlags::CTRUNC)
        {
            return Err(invalid("unexpected or truncated ticket rights"));
        }
        self.used += received.bytes;
        if self.used > SIZE {
            return Err(invalid("oversized ticket frame"));
        }
        within(self.deadline)?;
        if received.bytes != 0 {
            return Ok(None);
        }
        if self.used != SIZE || self.bytes[..SIZE] != self.expected {
            return Err(invalid("ticket frame or source binding mismatch"));
        }
        let observer = self
            .observer
            .take()
            .ok_or_else(|| invalid("missing ticket FD"))?;
        validate_observer(&observer)?;
        let ticket = Ticket {
            observer,
            lost: AtomicBool::new(false),
        };
        if !ticket.is_open() {
            return Err(invalid("ticket already revoked or not silent"));
        }
        within(self.deadline)?;
        Ok(Some(ticket))
    }
}
// CONTROL deliberately ends with EOF: both write halves may already be
// closed when queued bytes/rights are read. Its caller authenticated the peer
// before the challenge. Native family/type must still protect the UNIX parser.
fn validate_unix_stream_kind(fd: &impl AsFd) -> io::Result<()> {
    if sockopt::socket_type(fd)? != SocketType::STREAM
        || getsockname(fd)?.address_family() != AddressFamily::UNIX
    {
        return Err(invalid("ticket FD is not a Unix stream"));
    }
    Ok(())
}
fn validate_unix_stream(fd: &impl AsFd) -> io::Result<()> {
    validate_unix_stream_kind(fd)?;
    // OBSERVER connection is authority, unlike completed CONTROL framing.
    // A successful getpeername on Darwin's unnamed socketpair is normalized
    // to None by rustix. Unconnected sockets still return ENOTCONN, propagated
    // by `?`; None after a successful syscall is not that error.
    let peer = getpeername(fd)?;
    let connected = match peer {
        Some(peer) => peer.address_family() == AddressFamily::UNIX,
        None => cfg!(target_os = "macos"),
    };
    if !connected {
        return Err(invalid("ticket FD is not connected"));
    }
    Ok(())
}
fn validate_observer(fd: &OwnedFd) -> io::Result<()> {
    validate_unix_stream(fd)?;
    if !fcntl_getfd(fd)?.contains(FdFlags::CLOEXEC) {
        return Err(invalid("ticket FD lacks CLOEXEC"));
    }
    Ok(())
}

/// An observer only: deliberately exposes neither its FD nor shutdown/flag
/// mutation. Dropping it closes only this descriptor, never other observers.
/// Source expiry/identity/request checks are REQUIRED even while this is open.
#[derive(Debug)]
pub struct Ticket {
    observer: OwnedFd,
    lost: AtomicBool,
}
impl Ticket {
    /// One non-consuming, nonblocking recv syscall. Only WouldBlock means open;
    /// EOF, data and every other error latch Lost. PEEK avoids one reader hiding
    /// unexpected bytes from another. A later close cannot recall sent bytes.
    pub fn is_open(&self) -> bool {
        if self.lost.load(Ordering::Acquire) {
            return false;
        }
        let mut byte = [0u8; 1];
        if !matches!(
            recv(
                &self.observer,
                &mut byte[..],
                RecvFlags::DONTWAIT | RecvFlags::PEEK
            ),
            Err(Errno::AGAIN)
        ) {
            self.lost.store(true, Ordering::Release);
            return false;
        }
        !self.lost.load(Ordering::Acquire)
    }
}
