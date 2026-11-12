//! Bounded camera frame delivery between processes.
//!
//! JPEG frames do not fit the runtime's small control datagrams, so they use a
//! separate private Unix stream socket that carries nothing else. The camera
//! worker is the [`FrameServer`]; each permitted consumer (perception, or the
//! provider for visual context) is a [`FrameClient`]. The parent supplies the
//! permitted consumer boots; a boot is never learned from the socket.
//!
//! Delivery is pull-based. A consumer has at most one outstanding request.
//! The freshness contract is: the producer copies the newest frame it holds
//! when it serves the request, and only if that frame's host dequeue
//! completion is no older than the request's `max_age_us` at copy time. The
//! frame may have been dequeued before the request was sent; nothing promises
//! a capture after the request. Frames arriving while no request is waiting
//! replace the camera library's pending slot rather than queueing.
//! Each consumer has one preallocated outgoing buffer, written nonblockingly
//! across loop iterations, so a slow consumer can never block capture or
//! privacy control. A frame left unsent for [`STALL_DEADLINE_US`] closes that
//! consumer. Privacy closure, stop and faults discard partly sent frames and
//! close every consumer: a cut-off frame is never completed.
//!
//! A frame keeps its original [`CameraGrant`]. Consumers must recheck it
//! against their own current authority before use; bytes already sent cannot be
//! retracted by this module. A producer whose source authority changes
//! abandons unsent tails with [`FrameServer::discard_sending`].
//!
//! Connection policy: one live connection per role; the same permitted
//! consumer may reconnect after any closure except its own protocol
//! violation; `close_all` is terminal for the producer.
//!
//! Peer policy: the socket's mode and directory let in only this user, and
//! the hello's boots name a consumer. A producer whose consumers are
//! another unit running as the same user also checks each connection's
//! peer as the kernel names it ([`FrameServer::with_peer_check`]), before a
//! byte of it is read: anything it refuses is closed and counted as
//! [`CloseReason::PeerRefused`]. A producer whose consumers are its own
//! process's workers sets none.
use crate::{DriverTimestamp, FrameObservation, TimestampDomain, TimestampPoint};
use lamp_turn_owner::{BootId, CameraGrant};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs,
    io::{self, Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
};

pub const HELLO_MAGIC: [u8; 4] = *b"LFH1";
pub const REQUEST_MAGIC: [u8; 4] = *b"LFR1";
pub const FRAME_MAGIC: [u8; 4] = *b"LFF1";
/// Magic, role, three zero bytes, consumer boot, controller boot.
pub const HELLO_BYTES: usize = 40;
/// Magic, maximum age (u32 microseconds), request id (u64).
pub const REQUEST_BYTES: usize = 16;
/// Magic, request id (u64), metadata length (u16), zero (u16), JPEG length (u32).
pub const FRAME_HEADER_BYTES: usize = 20;
pub const MAX_META_BYTES: usize = 2048;
/// The provider accepts context images up to 2 s old (`A1-006`).
pub const MAX_REQUEST_AGE_US: u32 = 2_000_000;
pub const HELLO_DEADLINE_US: u64 = 100_000;
pub const STALL_DEADLINE_US: u64 = 500_000;
pub const MAX_PENDING_CONNECTIONS: usize = 4;
/// Requested kernel send buffer for each consumer stream.
pub const SEND_BUFFER_BYTES: usize = 16 * 1024;
/// Bytes written to one consumer per service call, bounding one loop iteration.
pub const WRITE_SLICE_BYTES: usize = 256 * 1024;
/// Bytes read from one stream per poll call.
pub const READ_SLICE_BYTES: usize = 256 * 1024;
const MAX_EVENTS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The local perception worker.
    Perception,
    /// The provider worker, for bounded visual context.
    Context,
}

impl Role {
    pub const ALL: [Role; 2] = [Role::Perception, Role::Context];
    fn code(self) -> u8 {
        match self {
            Role::Perception => 1,
            Role::Context => 2,
        }
    }
    fn from_code(code: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.code() == code)
    }
    fn index(self) -> usize {
        usize::from(self.code() - 1)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Perception => "perception",
            Role::Context => "context",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.as_str() == text)
    }
}

/// Driver timestamp with its original meaning; never reinterpreted as host age.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireTimestamp {
    pub micros: u64,
    pub domain: WireDomain,
    pub point: WirePoint,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireDomain {
    HostMonotonic,
    Realtime,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WirePoint {
    StartOfExposure,
    EndOfFrame,
    Unknown,
}
impl From<DriverTimestamp> for WireTimestamp {
    fn from(t: DriverTimestamp) -> Self {
        Self {
            micros: t.micros,
            domain: match t.domain {
                TimestampDomain::HostMonotonic => WireDomain::HostMonotonic,
                TimestampDomain::Realtime => WireDomain::Realtime,
                TimestampDomain::Unknown => WireDomain::Unknown,
            },
            point: match t.point {
                TimestampPoint::StartOfExposure => WirePoint::StartOfExposure,
                TimestampPoint::EndOfFrame => WirePoint::EndOfFrame,
                TimestampPoint::Unknown => WirePoint::Unknown,
            },
        }
    }
}

/// Original identity, privacy lineage and host timing of one delivered frame.
/// Host dequeue times bound local retention; they are not exposure times.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameMeta {
    pub request: u64,
    pub camera_worker: BootId,
    pub capture_epoch: u64,
    pub frame_sequence: u64,
    pub grant: CameraGrant,
    pub source: String,
    pub format: [u8; 4],
    pub width: u32,
    pub height: u32,
    pub driver_sequence: u32,
    pub driver_timestamp: Option<WireTimestamp>,
    pub dequeue_started_at_us: u64,
    pub dequeue_completed_at_us: u64,
    /// When the producer copied the frame into this consumer's buffer.
    pub copied_at_us: u64,
    /// Present when the bytes are a re-encoded reduction of the original
    /// camera frame (identity, grant and times above stay the original's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived: Option<DerivedImage>,
}

/// A reduced copy of an original frame, for example a small context image.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedImage {
    pub source_width: u32,
    pub source_height: u32,
    pub quality: u8,
}

impl FrameMeta {
    pub fn new(observation: &FrameObservation, request: u64, copied_at_us: u64) -> Self {
        Self {
            request,
            camera_worker: observation.id.capture.worker,
            capture_epoch: observation.id.capture.epoch.get(),
            frame_sequence: observation.id.sequence.get(),
            grant: observation.grant,
            source: observation.source.as_str().to_owned(),
            format: observation.mode.format.0,
            width: observation.mode.width,
            height: observation.mode.height,
            driver_sequence: observation.driver_sequence,
            driver_timestamp: observation.driver_timestamp.map(Into::into),
            dequeue_started_at_us: observation.dequeue.started_at.as_micros(),
            dequeue_completed_at_us: observation.dequeue.completed_at.as_micros(),
            copied_at_us,
            derived: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseReason {
    /// Ordinary stop of the camera worker.
    Stopped,
    /// Camera privacy closed or its lineage changed.
    PrivacyClosed,
    /// The camera worker faulted.
    Fault,
    /// A frame stayed unsent past the stall deadline.
    ConsumerStalled,
    /// The consumer closed its end.
    ConsumerClosed,
    /// Malformed or out-of-order consumer input.
    ProtocolViolation,
    /// Unknown identity, a second live connection for a role, or a role
    /// banned after a protocol violation.
    Rejected,
    /// No complete hello within the deadline.
    HelloTimeout,
    /// More simultaneous unidentified connections than allowed.
    TooManyConnections,
    /// The peer, as the kernel named it, is not one the producer's check
    /// lets in ([`FrameServer::with_peer_check`]).
    PeerRefused,
    /// A socket error other than would-block.
    Io,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkEvent {
    Connected {
        role: Role,
        at_us: u64,
    },
    Closed {
        role: Option<Role>,
        reason: CloseReason,
        /// A frame was partly written and is now abandoned.
        partial_frame_discarded: bool,
        at_us: u64,
    },
}

/// Saturating diagnostic counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkCounts {
    pub accepted: u64,
    pub rejected: u64,
    pub requests: u64,
    pub frames_offered: u64,
    pub frames_sent: u64,
    pub bytes_sent: u64,
    pub too_old: u64,
    pub partial_discarded: u64,
    pub stalled: u64,
    pub protocol_violations: u64,
    pub events_dropped: u64,
}

fn bump(counter: &mut u64) {
    *counter = counter.saturating_add(1);
}

fn io_error(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

/// Check that `path`'s directory is private to this user and `path` is free.
fn private_socket_path(path: &Path) -> io::Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| io_error("frame socket needs a file name"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| io_error("frame socket needs a private directory"))?;
    let metadata = fs::symlink_metadata(parent)?;
    if !metadata.is_dir()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "frame socket directory must be private and owned by this user",
        ));
    }
    Ok(parent.canonicalize()?.join(name))
}

fn would_block(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}

struct Pending {
    stream: UnixStream,
    hello: [u8; HELLO_BYTES],
    filled: usize,
    deadline_us: u64,
}

struct Request {
    id: u64,
    max_age_us: u32,
}

struct Slot {
    role: Role,
    stream: UnixStream,
    input: [u8; REQUEST_BYTES],
    input_filled: usize,
    request: Option<Request>,
    last_request_id: u64,
    out: Vec<u8>,
    sent: usize,
    out_since_us: u64,
}

impl Slot {
    fn sending(&self) -> bool {
        self.sent < self.out.len()
    }
}

/// Whether a connection's peer may go on to its hello, from the kernel's
/// naming of it (`SO_PEERCRED` and its cgroup), never its word.
pub type PeerCheck = Box<dyn Fn(&UnixStream) -> bool + Send + Sync>;

/// The camera worker's end. Every method is nonblocking and bounded.
pub struct FrameServer {
    listener: UnixListener,
    path: PathBuf,
    inode: (u64, u64),
    controller: BootId,
    expected: Vec<(Role, BootId)>,
    /// Roles closed for a protocol violation; they stay closed for this
    /// producer's lifetime.
    banned: [bool; 2],
    pending: Vec<Pending>,
    slots: [Option<Slot>; 2],
    capacity: usize,
    counts: LinkCounts,
    events: VecDeque<LinkEvent>,
    closed: bool,
    /// Who may connect, by the kernel's naming; `None` checks no peer.
    peer: Option<PeerCheck>,
}

impl FrameServer {
    /// Bind a mode-0600 listener in a private directory. Buffers for every
    /// permitted consumer are allocated here, not per frame.
    pub fn bind(
        path: &Path,
        controller: BootId,
        consumers: &[(Role, BootId)],
        max_frame_bytes: usize,
    ) -> io::Result<Self> {
        if consumers.is_empty()
            || consumers.len() > Role::ALL.len()
            || (consumers.len() == 2 && consumers[0].0 == consumers[1].0)
        {
            return Err(io_error(
                "frame consumers must be one or two distinct roles",
            ));
        }
        if max_frame_bytes == 0 || max_frame_bytes > crate::MAX_FRAME_BYTES {
            return Err(io_error("invalid frame byte bound"));
        }
        let path = private_socket_path(path)?;
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let metadata = fs::symlink_metadata(&path)?;
        Ok(Self {
            listener,
            inode: (metadata.dev(), metadata.ino()),
            path,
            controller,
            expected: consumers.to_vec(),
            banned: [false; 2],
            pending: Vec::with_capacity(MAX_PENDING_CONNECTIONS),
            slots: [None, None],
            capacity: FRAME_HEADER_BYTES + MAX_META_BYTES + max_frame_bytes,
            counts: LinkCounts::default(),
            events: VecDeque::with_capacity(MAX_EVENTS),
            closed: false,
            peer: None,
        })
    }

    /// Check each connection's peer with `check` as it is accepted, before
    /// a byte of it is read; one refused is closed and counted
    /// ([`CloseReason::PeerRefused`]).
    pub fn with_peer_check(mut self, check: PeerCheck) -> Self {
        self.peer = Some(check);
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn counts(&self) -> LinkCounts {
        self.counts
    }
    pub fn take_event(&mut self) -> Option<LinkEvent> {
        self.events.pop_front()
    }
    pub fn connected(&self, role: Role) -> bool {
        self.slots[role.index()].is_some()
    }

    fn event(&mut self, event: LinkEvent) {
        if self.events.len() >= MAX_EVENTS {
            bump(&mut self.counts.events_dropped);
            return;
        }
        self.events.push_back(event);
    }

    fn close_slot(&mut self, index: usize, reason: CloseReason, now_us: u64) {
        if let Some(slot) = self.slots[index].take() {
            let partial = slot.sending() && slot.sent > 0;
            if partial {
                bump(&mut self.counts.partial_discarded);
            }
            let _ = slot.stream.shutdown(std::net::Shutdown::Both);
            self.event(LinkEvent::Closed {
                role: Some(slot.role),
                reason,
                partial_frame_discarded: partial,
                at_us: now_us,
            });
        }
    }

    /// Whether a frame is still being written to this role's consumer.
    pub fn sending(&self, role: Role) -> bool {
        self.slots[role.index()].as_ref().is_some_and(Slot::sending)
    }

    /// Abandon every frame that is not completely written, closing those
    /// connections (a stream cannot resume mid-frame). Unlike `close_all`
    /// the producer keeps accepting, and the same consumer may reconnect.
    /// Returns how many frames were abandoned.
    pub fn discard_sending(&mut self, reason: CloseReason, now_us: u64) -> usize {
        let mut abandoned = 0;
        for index in 0..self.slots.len() {
            if self.slots[index].as_ref().is_some_and(Slot::sending) {
                self.close_slot(index, reason, now_us);
                abandoned += 1;
            }
        }
        abandoned
    }

    /// Discard every partly sent frame and close all consumers, then stop
    /// accepting. Used for privacy closure, stop and faults. Terminal for
    /// this producer: serving again needs a new producer incarnation.
    pub fn close_all(&mut self, reason: CloseReason, now_us: u64) {
        for index in 0..self.slots.len() {
            self.close_slot(index, reason, now_us);
        }
        for pending in self.pending.drain(..) {
            let _ = pending.stream.shutdown(std::net::Shutdown::Both);
        }
        self.closed = true;
    }

    /// Accept and identify consumers, read requests and continue writes.
    pub fn service(&mut self, now_us: u64) {
        if self.closed {
            return;
        }
        self.accept(now_us);
        self.identify(now_us);
        for index in 0..self.slots.len() {
            if let Err(reason) = self.service_slot(index, now_us) {
                if reason == CloseReason::ConsumerStalled {
                    bump(&mut self.counts.stalled);
                }
                if reason == CloseReason::ProtocolViolation {
                    bump(&mut self.counts.protocol_violations);
                    self.banned[index] = true;
                }
                self.close_slot(index, reason, now_us);
            }
        }
    }

    fn accept(&mut self, now_us: u64) {
        // Bounded: at most a few accepts per call.
        for _ in 0..MAX_PENDING_CONNECTIONS {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if self.peer.as_ref().is_some_and(|check| !check(&stream)) {
                        bump(&mut self.counts.rejected);
                        let _ = stream.shutdown(std::net::Shutdown::Both);
                        self.event(LinkEvent::Closed {
                            role: None,
                            reason: CloseReason::PeerRefused,
                            partial_frame_discarded: false,
                            at_us: now_us,
                        });
                        continue;
                    }
                    if self.pending.len() >= MAX_PENDING_CONNECTIONS
                        || stream.set_nonblocking(true).is_err()
                    {
                        bump(&mut self.counts.rejected);
                        let _ = stream.shutdown(std::net::Shutdown::Both);
                        self.event(LinkEvent::Closed {
                            role: None,
                            reason: CloseReason::TooManyConnections,
                            partial_frame_discarded: false,
                            at_us: now_us,
                        });
                        continue;
                    }
                    self.pending.push(Pending {
                        stream,
                        hello: [0; HELLO_BYTES],
                        filled: 0,
                        deadline_us: now_us.saturating_add(HELLO_DEADLINE_US),
                    });
                }
                Err(error) if would_block(&error) => return,
                Err(_) => return,
            }
        }
    }

    fn identify(&mut self, now_us: u64) {
        let mut index = 0;
        while index < self.pending.len() {
            let pending = &mut self.pending[index];
            let mut closed = None;
            match pending.stream.read(&mut pending.hello[pending.filled..]) {
                Ok(0) => closed = Some(CloseReason::ConsumerClosed),
                Ok(count) => pending.filled += count,
                Err(error) if would_block(&error) => {}
                Err(_) => closed = Some(CloseReason::Io),
            }
            if closed.is_none() && pending.filled < HELLO_BYTES && now_us >= pending.deadline_us {
                closed = Some(CloseReason::HelloTimeout);
            }
            if closed.is_none() && pending.filled < HELLO_BYTES {
                index += 1;
                continue;
            }
            let pending = self.pending.swap_remove(index);
            if let Some(reason) = closed {
                bump(&mut self.counts.rejected);
                self.event(LinkEvent::Closed {
                    role: None,
                    reason,
                    partial_frame_discarded: false,
                    at_us: now_us,
                });
                continue;
            }
            match self.check_hello(&pending.hello) {
                Some(role) => {
                    // Small kernel buffering: fewer exported-but-unread bytes
                    // after a revocation, and the same behavior on every OS.
                    let _ = rustix::net::sockopt::set_socket_send_buffer_size(
                        &pending.stream,
                        SEND_BUFFER_BYTES,
                    );
                    bump(&mut self.counts.accepted);
                    self.slots[role.index()] = Some(Slot {
                        role,
                        stream: pending.stream,
                        input: [0; REQUEST_BYTES],
                        input_filled: 0,
                        request: None,
                        last_request_id: 0,
                        out: Vec::with_capacity(self.capacity),
                        sent: 0,
                        out_since_us: now_us,
                    });
                    self.event(LinkEvent::Connected {
                        role,
                        at_us: now_us,
                    });
                }
                None => {
                    bump(&mut self.counts.rejected);
                    let _ = pending.stream.shutdown(std::net::Shutdown::Both);
                    self.event(LinkEvent::Closed {
                        role: None,
                        reason: CloseReason::Rejected,
                        partial_frame_discarded: false,
                        at_us: now_us,
                    });
                }
            }
        }
    }

    fn check_hello(&self, hello: &[u8; HELLO_BYTES]) -> Option<Role> {
        if hello[..4] != HELLO_MAGIC || hello[5..8] != [0; 3] {
            return None;
        }
        let role = Role::from_code(hello[4])?;
        let consumer: [u8; 16] = hello[8..24].try_into().ok()?;
        let controller: [u8; 16] = hello[24..40].try_into().ok()?;
        let permitted = self
            .expected
            .iter()
            .any(|(r, boot)| *r == role && boot.bytes() == consumer);
        // One live connection per role. The same permitted consumer may
        // reconnect after a closure that was not its own protocol violation;
        // a restarted consumer (new boot) needs a new producer incarnation.
        (permitted
            && controller == self.controller.bytes()
            && !self.banned[role.index()]
            && self.slots[role.index()].is_none())
        .then_some(role)
    }

    fn service_slot(&mut self, index: usize, now_us: u64) -> Result<(), CloseReason> {
        let Some(slot) = self.slots[index].as_mut() else {
            return Ok(());
        };
        // Requests: at most a few small reads; anything unexpected closes.
        for _ in 0..4 {
            match slot.stream.read(&mut slot.input[slot.input_filled..]) {
                Ok(0) => return Err(CloseReason::ConsumerClosed),
                Ok(count) => slot.input_filled += count,
                Err(error) if would_block(&error) => break,
                Err(_) => return Err(CloseReason::Io),
            }
            if slot.input_filled < REQUEST_BYTES {
                continue;
            }
            slot.input_filled = 0;
            let input = slot.input;
            let max_age_us = u32::from_le_bytes(input[4..8].try_into().expect("4 bytes"));
            let id = u64::from_le_bytes(input[8..16].try_into().expect("8 bytes"));
            if input[..4] != REQUEST_MAGIC
                || max_age_us == 0
                || max_age_us > MAX_REQUEST_AGE_US
                || id <= slot.last_request_id
                || slot.request.is_some()
                || slot.sending()
            {
                return Err(CloseReason::ProtocolViolation);
            }
            slot.last_request_id = id;
            slot.request = Some(Request { id, max_age_us });
            self.counts.requests = self.counts.requests.saturating_add(1);
        }
        // Continue an outgoing frame within this call's write slice.
        let mut budget = WRITE_SLICE_BYTES;
        while slot.sending() && budget > 0 {
            let end = slot.out.len().min(slot.sent + budget);
            match slot.stream.write(&slot.out[slot.sent..end]) {
                Ok(0) => return Err(CloseReason::Io),
                Ok(count) => {
                    slot.sent += count;
                    budget -= count;
                    self.counts.bytes_sent = self.counts.bytes_sent.saturating_add(count as u64);
                }
                Err(error) if would_block(&error) => break,
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {
                    return Err(CloseReason::ConsumerClosed);
                }
                Err(_) => return Err(CloseReason::Io),
            }
        }
        if slot.sending() {
            if now_us.saturating_sub(slot.out_since_us) >= STALL_DEADLINE_US {
                return Err(CloseReason::ConsumerStalled);
            }
        } else if !slot.out.is_empty() {
            slot.out.clear();
            slot.sent = 0;
            self.counts.frames_sent = self.counts.frames_sent.saturating_add(1);
        }
        Ok(())
    }

    /// Whether some connected consumer is waiting for a frame and has no
    /// frame in transit. Only then should the caller take a delivery slot.
    pub fn wants_frame(&self) -> bool {
        !self.closed
            && self
                .slots
                .iter()
                .flatten()
                .any(|slot| slot.request.is_some() && !slot.sending())
    }

    /// Copy one frame for every waiting consumer whose age bound admits it, and
    /// start writing. Returns how many consumers took it. The caller releases
    /// its library slot afterwards; the bytes are not borrowed past this call.
    pub fn offer(
        &mut self,
        observation: &FrameObservation,
        jpeg: &[u8],
        now_us: u64,
    ) -> io::Result<usize> {
        self.offer_meta(&FrameMeta::new(observation, 0, now_us), jpeg, now_us)
    }

    /// Like [`offer`](Self::offer) for a frame described by `meta` (for
    /// example a derived image that keeps the original identity and grant).
    /// The request id and copy time are filled in per consumer; age is
    /// measured from the original host dequeue completion.
    pub fn offer_meta(&mut self, meta: &FrameMeta, jpeg: &[u8], now_us: u64) -> io::Result<usize> {
        if self.closed {
            return Ok(0);
        }
        if jpeg.len() + FRAME_HEADER_BYTES + MAX_META_BYTES > self.capacity {
            return Err(io_error("frame exceeds the frame link bound"));
        }
        let age_us = now_us.saturating_sub(meta.dequeue_completed_at_us);
        let mut taken = 0;
        for index in 0..self.slots.len() {
            let Some(slot) = self.slots[index].as_mut() else {
                continue;
            };
            let Some(request) = slot.request.as_ref() else {
                continue;
            };
            if slot.sending() {
                continue;
            }
            if age_us > u64::from(request.max_age_us) {
                bump(&mut self.counts.too_old);
                continue;
            }
            let mut described = meta.clone();
            described.request = request.id;
            described.copied_at_us = now_us;
            let encoded = serde_json::to_vec(&described).map_err(io::Error::other)?;
            if encoded.len() > MAX_META_BYTES {
                return Err(io_error("frame metadata exceeds its bound"));
            }
            slot.out.clear();
            slot.out.extend_from_slice(&FRAME_MAGIC);
            slot.out.extend_from_slice(&request.id.to_le_bytes());
            slot.out
                .extend_from_slice(&u16::try_from(encoded.len()).expect("bounded").to_le_bytes());
            slot.out.extend_from_slice(&[0, 0]);
            slot.out
                .extend_from_slice(&u32::try_from(jpeg.len()).expect("bounded").to_le_bytes());
            slot.out.extend_from_slice(&encoded);
            slot.out.extend_from_slice(jpeg);
            slot.sent = 0;
            slot.out_since_us = now_us;
            slot.request = None;
            bump(&mut self.counts.frames_offered);
            taken += 1;
        }
        // Start writing now rather than waiting a loop iteration.
        self.service(now_us);
        Ok(taken)
    }
}

impl Drop for FrameServer {
    fn drop(&mut self) {
        // Remove only our own socket, never a replacement at the same path.
        if let Ok(metadata) = fs::symlink_metadata(&self.path)
            && metadata.file_type().is_socket()
            && (metadata.dev(), metadata.ino()) == self.inode
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// A received frame header, before its bytes are complete.
struct Incoming {
    request: u64,
    meta_len: usize,
    jpeg_len: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientCounts {
    pub connects: u64,
    pub requests: u64,
    pub frames: u64,
    pub disconnects: u64,
}

/// A nonblocking connection attempt: connected (or in progress) but its hello
/// not yet fully written.
struct Connecting {
    stream: UnixStream,
    hello: [u8; HELLO_BYTES],
    sent: usize,
    deadline: std::time::Instant,
}

/// A connection attempt that sends its hello within this bound or is
/// abandoned (and may be retried later).
pub const CONNECT_DEADLINE: std::time::Duration = std::time::Duration::from_millis(100);

/// A consumer's end. Every call is nonblocking, including connection and the
/// hello; one outstanding request.
pub struct FrameClient {
    path: PathBuf,
    role: Role,
    own: BootId,
    controller: BootId,
    max_frame_bytes: usize,
    stream: Option<UnixStream>,
    /// A nonblocking connection still sending its hello, with its deadline.
    pending: Option<Connecting>,
    header: [u8; FRAME_HEADER_BYTES],
    header_filled: usize,
    incoming: Option<Incoming>,
    body: Vec<u8>,
    outstanding: Option<u64>,
    next_request: u64,
    counts: ClientCounts,
}

impl FrameClient {
    pub fn new(
        path: &Path,
        role: Role,
        own: BootId,
        controller: BootId,
        max_frame_bytes: usize,
    ) -> Self {
        Self {
            path: path.to_owned(),
            role,
            own,
            controller,
            max_frame_bytes: max_frame_bytes.min(crate::MAX_FRAME_BYTES),
            stream: None,
            pending: None,
            header: [0; FRAME_HEADER_BYTES],
            header_filled: 0,
            incoming: None,
            body: Vec::new(),
            outstanding: None,
            next_request: 1,
            counts: ClientCounts::default(),
        }
    }

    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }
    pub fn outstanding(&self) -> Option<u64> {
        self.outstanding
    }
    pub fn counts(&self) -> ClientCounts {
        self.counts
    }

    /// Whether a nonblocking connection attempt is still in progress.
    pub fn connecting(&self) -> bool {
        self.pending.is_some()
    }

    /// Advance a nonblocking connection by at most one step. `Ok(true)` once
    /// connected and identified; `Ok(false)` while the producer is not
    /// listening, its queue is full, or the hello is still being written
    /// (see [`connecting`](Self::connecting)). An attempt that cannot send
    /// its hello within [`CONNECT_DEADLINE`] is abandoned with `TimedOut`.
    pub fn connect(&mut self) -> io::Result<bool> {
        if self.stream.is_some() {
            return Ok(true);
        }
        if self.pending.is_none() {
            let Some(stream) = self.open()? else {
                return Ok(false);
            };
            let mut hello = [0; HELLO_BYTES];
            hello[..4].copy_from_slice(&HELLO_MAGIC);
            hello[4] = self.role.code();
            hello[8..24].copy_from_slice(&self.own.bytes());
            hello[24..40].copy_from_slice(&self.controller.bytes());
            self.pending = Some(Connecting {
                stream,
                hello,
                sent: 0,
                deadline: std::time::Instant::now() + CONNECT_DEADLINE,
            });
        }
        let attempt = self.pending.as_mut().expect("set above");
        match attempt.stream.write(&attempt.hello[attempt.sent..]) {
            Ok(count) => attempt.sent += count,
            Err(error)
                if would_block(&error)
                    || matches!(error.raw_os_error(), Some(code) if code == rustix::io::Errno::NOTCONN.raw_os_error()) =>
                {}
            Err(error) => {
                self.pending = None;
                return Err(error);
            }
        }
        if attempt.sent < HELLO_BYTES {
            if std::time::Instant::now() >= attempt.deadline {
                self.pending = None;
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "frame link connection did not complete in time",
                ));
            }
            return Ok(false);
        }
        let attempt = self.pending.take().expect("checked");
        self.stream = Some(attempt.stream);
        self.header_filled = 0;
        self.incoming = None;
        self.outstanding = None;
        self.counts.connects = self.counts.connects.saturating_add(1);
        Ok(true)
    }

    /// One nonblocking `connect`. `None` when the producer is absent or its
    /// listen queue is full; the caller retries later.
    fn open(&self) -> io::Result<Option<UnixStream>> {
        use rustix::net::{AddressFamily, SocketAddrUnix, SocketType};
        let socket = rustix::net::socket(AddressFamily::UNIX, SocketType::STREAM, None)?;
        rustix::io::fcntl_setfd(&socket, rustix::io::FdFlags::CLOEXEC)?;
        rustix::io::ioctl_fionbio(&socket, true)?;
        let address = SocketAddrUnix::new(&self.path)?;
        match rustix::net::connect(&socket, &address) {
            Ok(()) => {}
            // The connection completes asynchronously; the hello write waits.
            Err(rustix::io::Errno::INPROGRESS) => {}
            Err(
                rustix::io::Errno::NOENT
                | rustix::io::Errno::CONNREFUSED
                | rustix::io::Errno::AGAIN,
            ) => {
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        }
        Ok(Some(UnixStream::from(socket)))
    }

    /// Drop the connection and any partial frame.
    pub fn disconnect(&mut self) {
        self.pending = None;
        if let Some(stream) = self.stream.take() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            self.counts.disconnects = self.counts.disconnects.saturating_add(1);
        }
        self.header_filled = 0;
        self.incoming = None;
        self.body.clear();
        self.outstanding = None;
    }

    /// Ask for the newest frame no older than `max_age_us` at copy time.
    pub fn request(&mut self, max_age_us: u32) -> io::Result<u64> {
        if self.outstanding.is_some() {
            return Err(io_error("a frame request is already outstanding"));
        }
        if max_age_us == 0 || max_age_us > MAX_REQUEST_AGE_US {
            return Err(io_error("invalid frame age bound"));
        }
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "frame link closed"))?;
        let id = self.next_request;
        let mut request = [0; REQUEST_BYTES];
        request[..4].copy_from_slice(&REQUEST_MAGIC);
        request[4..8].copy_from_slice(&max_age_us.to_le_bytes());
        request[8..16].copy_from_slice(&id.to_le_bytes());
        // Sixteen bytes on an otherwise idle direction; a short write would
        // mean the link is unusable rather than slow.
        match stream.write(&request) {
            Ok(REQUEST_BYTES) => {}
            Ok(_) => {
                self.disconnect();
                return Err(io_error("short frame request write"));
            }
            Err(error) => {
                self.disconnect();
                return Err(error);
            }
        }
        self.next_request += 1;
        self.outstanding = Some(id);
        self.counts.requests = self.counts.requests.saturating_add(1);
        Ok(id)
    }

    /// Read without blocking. On a complete frame, copy its JPEG bytes into
    /// `jpeg` (replacing its contents) and return its metadata. Malformed input or a closed camera
    /// disconnects and returns an error; the caller may reconnect.
    pub fn poll(&mut self, jpeg: &mut Vec<u8>) -> io::Result<Option<FrameMeta>> {
        match self.poll_inner(jpeg) {
            Ok(frame) => Ok(frame),
            Err(error) => {
                self.disconnect();
                Err(error)
            }
        }
    }

    fn poll_inner(&mut self, jpeg: &mut Vec<u8>) -> io::Result<Option<FrameMeta>> {
        let Some(stream) = self.stream.as_mut() else {
            return Ok(None);
        };
        let mut budget = READ_SLICE_BYTES;
        loop {
            if self.incoming.is_none() {
                let result = stream.read(&mut self.header[self.header_filled..]);
                match result {
                    Ok(0) => {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "camera closed the frame link",
                        ));
                    }
                    Ok(count) => self.header_filled += count,
                    Err(error) if would_block(&error) => return Ok(None),
                    Err(error) => return Err(error),
                }
                if self.header_filled < FRAME_HEADER_BYTES {
                    continue;
                }
                let h = self.header;
                let request = u64::from_le_bytes(h[4..12].try_into().expect("8 bytes"));
                let meta_len = usize::from(u16::from_le_bytes(h[12..14].try_into().expect("2")));
                let jpeg_len = u32::from_le_bytes(h[16..20].try_into().expect("4 bytes")) as usize;
                if h[..4] != FRAME_MAGIC
                    || h[14..16] != [0, 0]
                    || Some(request) != self.outstanding
                    || meta_len == 0
                    || meta_len > MAX_META_BYTES
                    || jpeg_len < 4
                    || jpeg_len > self.max_frame_bytes
                {
                    return Err(io_error("malformed or unrequested frame header"));
                }
                self.body.clear();
                self.body.reserve_exact(meta_len + jpeg_len);
                self.incoming = Some(Incoming {
                    request,
                    meta_len,
                    jpeg_len,
                });
            }
            let incoming = self.incoming.as_ref().expect("checked");
            let total = incoming.meta_len + incoming.jpeg_len;
            let mut chunk = [0_u8; 16 * 1024];
            while self.body.len() < total && budget > 0 {
                let want = (total - self.body.len()).min(chunk.len()).min(budget);
                match stream.read(&mut chunk[..want]) {
                    Ok(0) => {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "camera closed the frame link inside a frame",
                        ));
                    }
                    Ok(count) => {
                        self.body.extend_from_slice(&chunk[..count]);
                        budget -= count;
                    }
                    Err(error) if would_block(&error) => return Ok(None),
                    Err(error) => return Err(error),
                }
            }
            if self.body.len() < total {
                return Ok(None);
            }
            let incoming = self.incoming.take().expect("checked");
            self.header_filled = 0;
            let meta: FrameMeta = serde_json::from_slice(&self.body[..incoming.meta_len])
                .map_err(|_| io_error("malformed frame metadata"))?;
            let bytes = &self.body[incoming.meta_len..];
            if meta.request != incoming.request
                || bytes[..2] != [0xff, 0xd8]
                || bytes[bytes.len() - 2..] != [0xff, 0xd9]
            {
                return Err(io_error("frame metadata or JPEG envelope mismatch"));
            }
            jpeg.clear();
            jpeg.extend_from_slice(bytes);
            self.body.clear();
            self.outstanding = None;
            self.counts.frames = self.counts.frames.saturating_add(1);
            return Ok(Some(meta));
        }
    }
}
