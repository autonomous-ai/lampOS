//! The camera's private frame link, served by the eyes (`--frame-link`,
//! #6543 gap 1): the same link, the same server and the same rules as the
//! voice's camera worker (`lamp_camera::frames`), so `lamp-seen`, its
//! `perception` reader, gets frames where the eyes own the camera.
//!
//! - **Pull, one frame per request.** A frame is copied only for a reader
//!   that has asked and has nothing in transit (`FrameServer::wants_frame`);
//!   with no reader, or none asking, nothing is copied and the camera's
//!   bytes are never touched for this.
//! - **Each frame carries its `CameraGrant`.** The grant comes from a
//!   turn-owner `Controller` of the eyes' own, under the controller boot the
//!   launcher gives: every hold of the eyes' permission
//!   ([`crate::permit::Permit::held`]) is a new camera generation, so a
//!   close and a reopen, even coalesced between two turns, change it.
//! - **Nothing while privacy is closed.** The link is bound only while the
//!   permission holds. When it stops holding or a new hold begins, every
//!   reader is closed (`CloseReason::PrivacyClosed`), a frame in transit is
//!   abandoned, and the socket is removed; a new link is bound for the next
//!   hold. A frame read under an older hold is never offered.
//! - **A slow reader is dropped, never waited for.** Writes are
//!   nonblocking, in slices, from one preallocated buffer per reader; a
//!   frame left unsent for `STALL_DEADLINE_US` closes that reader. Nothing
//!   is queued.
//! - **Private.** `FrameServer::bind`: a mode-0600 socket in a directory
//!   owned by this user with no group or other access. The eyes run as
//!   `lampos`, as other parts do, so the socket's mode is not enough: each
//!   connection's peer is checked as the kernel names it, before a byte of
//!   it is read ([`reader_may`]: `lamp-seen.service` run as this user, as
//!   [`crate::tap`] checks lamp-control's), and any other is refused and
//!   counted (#6645's P2). The reader's boots come from a private file
//!   ([`boots_from`]), never the command line, where any process of the
//!   same user could read them in `/proc/<pid>/cmdline`, and are never
//!   learned from the socket.
use lamp_camera::{
    DriverTimestamp,
    frames::{CloseReason, FrameMeta, FrameServer, LinkCounts, LinkEvent, Role},
};
use lamp_ipc::caller::Caller;
use lamp_turn_owner::{BootId, CameraGrant, Controller, MonoTime, Permission};
use std::{
    collections::VecDeque,
    fs,
    io::{self, Read as _},
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

/// The camera source the eyes name in each frame's metadata.
pub const SOURCE: &str = "lamp-eyes";
/// How long after a bind that failed another is tried, within one hold.
pub const RETRY_BIND_US: u64 = 5_000_000;
/// The one unit whose peer may read the link.
pub const READER_UNIT: &str = "lamp-seen.service";
/// The boots file is read up to this many bytes (`lamp-seen frame-link`
/// writes about 170).
const BOOTS_MOST_BYTES: u64 = 1024;
/// The boots file's line naming the link, as `lamp-seen frame-link` keeps
/// it (#6693, `lamp_seen::link`).
const BOOTS_VARIABLE: &str = "LAMP_EYES_FRAME_LINK=";
/// The other line that file may hold, for the eyes' unit, not read here.
const VISITS_VARIABLE: &str = "LAMP_EYES_VISITS=";

/// Events of closed links kept for the journal at most.
pub const MOST_CLOSED_EVENTS: usize = 64;

/// Whether a peer the kernel named may read the link.
pub type May = Arc<dyn Fn(Option<&Caller>) -> bool + Send + Sync>;

/// Who may read the link: `lamp-seen.service` run as `uid`, the eyes' own
/// user, as the kernel names it (SO_PEERCRED and its pid's cgroup), and
/// nothing else: not another `lampos` part, not root's tools, not a caller
/// the kernel could not name (off Linux, every caller).
pub fn reader_may(caller: Option<&Caller>, uid: Option<u32>) -> bool {
    lamp_ipc::caller::is_unit_as(caller, uid, READER_UNIT)
}

/// A boot as 32 hex digits, not all zero.
fn boot_of(text: &str) -> Option<BootId> {
    if text.len() != 32 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    BootId::new(bytes).ok()
}

/// The two boots in the boots file's text, for the link at `socket`: the
/// file `lamp-seen frame-link` keeps (#6693), one line
/// `LAMP_EYES_FRAME_LINK=--frame-link SOCKET --frame-link-controller HEX32
/// --frame-link-reader HEX32`, and at most a `LAMP_EYES_VISITS=` line
/// beside it. Its socket must be this one, and the two boots differ.
pub fn boots_in(text: &str, socket: &Path) -> Result<(BootId, BootId), &'static str> {
    let mut found = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(BOOTS_VARIABLE) {
            if found.is_some() {
                return Err("the frame link is named twice");
            }
            let words: Vec<&str> = rest.split(' ').collect();
            let [
                "--frame-link",
                path,
                "--frame-link-controller",
                controller,
                "--frame-link-reader",
                reader,
            ] = words.as_slice()
            else {
                return Err("the frame link's line is not the one lamp-seen keeps");
            };
            if Path::new(path) != socket {
                return Err("the file names another frame link socket");
            }
            let (Some(controller), Some(reader)) = (boot_of(controller), boot_of(reader)) else {
                return Err("a boot is not 32 hex digits, not all zero");
            };
            if controller == reader {
                return Err("the controller and the reader are two boots");
            }
            found = Some((controller, reader));
        } else if !line.starts_with(VISITS_VARIABLE) {
            return Err("a line the boots file does not hold");
        }
    }
    found.ok_or("no frame link named")
}

/// The link's two boots (controller, reader) from `file`, read at start:
/// `None` if there is no file or no folder for it (the switch is off, or
/// lamp-seen has not started yet: no link). The file must
/// be a regular file of this user's, not a link, with no group write and
/// no other access (0600 or 0640), in a folder of this user's or root's
/// with no group write and no other access.
pub fn boots_from(file: &Path, socket: &Path) -> io::Result<Option<(BootId, BootId)>> {
    let denied = |what: &str| io::Error::new(io::ErrorKind::PermissionDenied, what.to_owned());
    let euid = rustix::process::geteuid().as_raw();
    let parent = file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| denied("the boots file needs a private folder"))?;
    // No folder is no file: lamp-seen makes it (its runtime folder), and an
    // update round can restart the eyes before a newly added lamp-seen starts.
    let folder = match fs::symlink_metadata(parent) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        folder => folder?,
    };
    if !folder.is_dir()
        || !(folder.uid() == euid || folder.uid() == 0)
        || folder.mode() & 0o027 != 0
    {
        return Err(denied(
            "the boots file's folder must be this user's or root's, with no group write or other access",
        ));
    }
    match fs::symlink_metadata(file) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(denied("the boots file must be a regular file"));
        }
        Ok(_) => {}
    }
    let opened = fs::File::open(file)?;
    // Checked again on what was opened, so a file swapped in between is not read.
    let metadata = opened.metadata()?;
    if !metadata.is_file() || metadata.uid() != euid || metadata.mode() & 0o037 != 0 {
        return Err(denied(
            "the boots file must be this user's, 0600 or 0640, with no other access",
        ));
    }
    let mut text = String::new();
    opened
        .take(BOOTS_MOST_BYTES + 1)
        .read_to_string(&mut text)?;
    if text.len() as u64 > BOOTS_MOST_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the boots file is too long",
        ));
    }
    boots_in(&text, socket)
        .map(Some)
        .map_err(|why| io::Error::new(io::ErrorKind::InvalidData, why))
}

/// What the launcher gives: the socket, the controller boot the reader
/// names in its hello, and the reader's own boot (`perception`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub path: PathBuf,
    pub controller: BootId,
    pub reader: BootId,
}

/// One frame as the eyes read it, for its metadata. Its bytes are lent to
/// [`FrameLink::offer`] for that call only.
#[derive(Clone, Copy, Debug)]
pub struct Read {
    /// The eyes' worker boot and the camera open's epoch.
    pub worker: BootId,
    pub epoch: u64,
    /// Every frame the eyes have read, this one included.
    pub sequence: u64,
    pub width: u32,
    pub height: u32,
    pub driver_sequence: u32,
    pub driver_timestamp: Option<DriverTimestamp>,
    /// Host monotonic time around the read, in microseconds.
    pub dequeue_started_us: u64,
    pub dequeue_completed_us: u64,
}

/// Saturating counts of the eyes' side, beside the link's own.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Counts {
    /// Links bound, one per hold of the permission.
    pub bound: u64,
    /// Binds that failed (the journal says why).
    pub bind_failed: u64,
    /// Links closed for a hold that ended or changed.
    pub privacy_closed: u64,
    /// Connections refused for their peer: not `lamp-seen.service` as
    /// this user (the journal says each).
    pub peer_refused: u64,
    /// Frames not offered: read under another hold.
    pub other_hold: u64,
    /// Frames not offered: not a whole JPEG (no SOI or EOI at the ends).
    pub not_jpeg: u64,
}

/// The link while the eyes' permission holds, and the lineage its grants
/// come from.
pub struct FrameLink {
    config: Config,
    max_frame_bytes: usize,
    controller: Controller,
    server: Option<FrameServer>,
    /// The hold the bound link (or the next bind) belongs to.
    hold: Option<u64>,
    grant: Option<CameraGrant>,
    retry_at_us: u64,
    counts: Counts,
    /// Who may connect, checked on each server's accept.
    may: May,
    peer_refused: Arc<AtomicU64>,
    /// Events of a server already closed (its readers cut for privacy),
    /// kept for [`event`](Self::event) so the journal says them.
    closed_events: VecDeque<LinkEvent>,
}

fn private_directory(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| io::Error::other("the frame link needs a private directory"))?;
    let metadata = fs::symlink_metadata(parent)?;
    let euid = rustix::process::geteuid().as_raw();
    if !metadata.is_dir() || metadata.uid() != euid || metadata.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the frame link's directory must be private and owned by this user",
        ));
    }
    Ok(())
}

/// A socket left at `path` by an eyes that ended without removing it, in
/// the private directory only: a file of another kind is left alone and
/// the bind refused.
fn remove_stale(path: &Path) -> io::Result<()> {
    private_directory(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(path),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "something other than a socket is at the frame link's path",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

impl FrameLink {
    /// Checks the path by binding the link once and removing it: a
    /// directory that is not private fails here, at start, not at the first
    /// hold. Nothing is served until [`turn`](Self::turn) sees a hold. Only
    /// `lamp-seen.service` run as this user may connect ([`reader_may`]).
    pub fn new(config: Config, max_frame_bytes: usize, now_us: u64) -> io::Result<Self> {
        let uid = rustix::process::geteuid().as_raw();
        Self::new_allowing(
            config,
            max_frame_bytes,
            now_us,
            Arc::new(move |caller| reader_may(caller, Some(uid))),
        )
    }

    /// [`new`](Self::new) for the peers `may` lets in (its tests, where no
    /// peer is `lamp-seen.service`).
    pub fn new_allowing(
        config: Config,
        max_frame_bytes: usize,
        now_us: u64,
        may: May,
    ) -> io::Result<Self> {
        remove_stale(&config.path)?;
        drop(FrameServer::bind(
            &config.path,
            config.controller,
            &[(Role::Perception, config.reader)],
            max_frame_bytes,
        )?);
        Ok(Self {
            controller: Controller::new(config.controller, MonoTime::from_micros(now_us)),
            config,
            max_frame_bytes,
            server: None,
            hold: None,
            grant: None,
            retry_at_us: 0,
            counts: Counts::default(),
            may,
            peer_refused: Arc::new(AtomicU64::new(0)),
            closed_events: VecDeque::new(),
        })
    }

    pub fn counts(&self) -> Counts {
        Counts {
            peer_refused: self.peer_refused.load(Ordering::Relaxed),
            ..self.counts
        }
    }
    pub fn link_counts(&self) -> Option<LinkCounts> {
        self.server.as_ref().map(FrameServer::counts)
    }
    /// Whether the link is bound now (the permission holds).
    pub fn bound(&self) -> bool {
        self.server.is_some()
    }
    /// Whether the reader is connected now.
    pub fn connected(&self) -> bool {
        self.server
            .as_ref()
            .is_some_and(|server| server.connected(Role::Perception))
    }
    /// The grant frames are offered under now, while the link is bound.
    pub fn grant(&self) -> Option<CameraGrant> {
        self.server.as_ref().and(self.grant)
    }

    /// One turn of the eyes' loop, first thing, with the hold the camera
    /// may be open under now (`Permit::held`): the link follows it, then
    /// accepts, reads requests and continues writes. Bounded, nonblocking.
    /// A bind that fails is said once in `Err` and tried again later.
    pub fn turn(&mut self, hold: Option<u64>, now_us: u64) -> io::Result<()> {
        if hold != self.hold {
            self.close(now_us);
            self.hold = hold;
            self.retry_at_us = now_us;
        }
        if self.hold.is_some()
            && self.server.is_none()
            && now_us >= self.retry_at_us
            && let Err(error) = self.bind(now_us)
        {
            self.counts.bind_failed = self.counts.bind_failed.saturating_add(1);
            self.retry_at_us = now_us.saturating_add(RETRY_BIND_US);
            return Err(error);
        }
        if let Some(server) = self.server.as_mut() {
            server.service(now_us);
        }
        Ok(())
    }

    fn bind(&mut self, now_us: u64) -> io::Result<()> {
        let now = MonoTime::from_micros(now_us);
        // A new generation for each hold: closed first, then allowed.
        let grant = self
            .controller
            .set_camera_permission(now, Permission::Denied)
            .and_then(|()| {
                self.controller
                    .set_camera_permission(now, Permission::Allowed)
            })
            .and_then(|()| self.controller.camera_grant(now))
            .map_err(|error| io::Error::other(format!("camera grant: {error:?}")))?;
        remove_stale(&self.config.path)?;
        let (may, refused) = (self.may.clone(), self.peer_refused.clone());
        let server = FrameServer::bind(
            &self.config.path,
            self.config.controller,
            &[(Role::Perception, self.config.reader)],
            self.max_frame_bytes,
        )?
        .with_peer_check(Box::new(move |stream| {
            let allowed = may(lamp_ipc::caller::of_stream(stream).as_ref());
            if !allowed {
                refused.fetch_add(1, Ordering::Relaxed);
            }
            allowed
        }));
        self.server = Some(server);
        self.grant = Some(grant);
        self.counts.bound = self.counts.bound.saturating_add(1);
        Ok(())
    }

    /// Close every reader, abandon a frame in transit and remove the
    /// socket: the hold ended or changed. The closed server's events (each
    /// reader cut for privacy) are kept for [`event`](Self::event) before
    /// it is dropped, so the journal says them (#6645's P3). Bounded: at
    /// most [`MOST_CLOSED_EVENTS`] wait, the oldest let go first.
    fn close(&mut self, now_us: u64) {
        if let Some(mut server) = self.server.take() {
            server.close_all(CloseReason::PrivacyClosed, now_us);
            self.counts.privacy_closed = self.counts.privacy_closed.saturating_add(1);
            while let Some(event) = server.take_event() {
                if self.closed_events.len() >= MOST_CLOSED_EVENTS {
                    self.closed_events.pop_front();
                }
                self.closed_events.push_back(event);
            }
        }
        self.grant = None;
        let now = MonoTime::from_micros(now_us);
        let _ = self
            .controller
            .set_camera_permission(now, Permission::Denied);
    }

    /// Whether the reader is waiting for a frame and has none in transit:
    /// only then is a frame worth offering.
    pub fn wants_frame(&self) -> bool {
        self.server.as_ref().is_some_and(FrameServer::wants_frame)
    }

    /// Offer the newest frame, read under `hold`, if the reader is waiting.
    /// One copy into the reader's buffer; the bytes are not kept past this
    /// call. Returns whether the reader took it (it may be too old).
    pub fn offer(
        &mut self,
        hold: Option<u64>,
        read: &Read,
        jpeg: &[u8],
        now_us: u64,
    ) -> io::Result<bool> {
        if !self.wants_frame() {
            return Ok(false);
        }
        let Some(grant) = self.grant else {
            return Ok(false);
        };
        if hold.is_none() || hold != self.hold {
            self.counts.other_hold = self.counts.other_hold.saturating_add(1);
            return Ok(false);
        }
        if jpeg.len() < 4 || jpeg[..2] != [0xff, 0xd8] || jpeg[jpeg.len() - 2..] != [0xff, 0xd9] {
            self.counts.not_jpeg = self.counts.not_jpeg.saturating_add(1);
            return Ok(false);
        }
        let meta = FrameMeta {
            request: 0,
            camera_worker: read.worker,
            capture_epoch: read.epoch,
            frame_sequence: read.sequence,
            grant,
            source: SOURCE.to_owned(),
            format: *b"MJPG",
            width: read.width,
            height: read.height,
            driver_sequence: read.driver_sequence,
            driver_timestamp: read.driver_timestamp.map(Into::into),
            dequeue_started_at_us: read.dequeue_started_us,
            dequeue_completed_at_us: read.dequeue_completed_us,
            copied_at_us: now_us,
            derived: None,
        };
        let server = self.server.as_mut().expect("wants_frame checked it");
        Ok(server.offer_meta(&meta, jpeg, now_us)? > 0)
    }

    /// The link's next event (a reader come or gone, and why), for the
    /// journal: those of a link closed for privacy first, then the bound
    /// one's. Bounded by the link.
    pub fn event(&mut self) -> Option<LinkEvent> {
        self.closed_events
            .pop_front()
            .or_else(|| self.server.as_mut().and_then(FrameServer::take_event))
    }
}

/// One journal line for a link event.
pub fn said(event: &LinkEvent) -> String {
    match event {
        LinkEvent::Connected { role, .. } => {
            format!("frame link: the {} reader connected", role.as_str())
        }
        LinkEvent::Closed {
            role,
            reason,
            partial_frame_discarded,
            ..
        } => format!(
            "frame link: {} closed ({reason:?}{}){}",
            role.map_or("a connection", |role| role.as_str()),
            match reason {
                CloseReason::PrivacyClosed => ": cut, the camera's permission ended or changed",
                CloseReason::PeerRefused => ": not lamp-seen.service as this user",
                _ => "",
            },
            if *partial_frame_discarded {
                ", a frame in transit dropped"
            } else {
                ""
            }
        ),
    }
}

#[cfg(test)]
mod tests;
