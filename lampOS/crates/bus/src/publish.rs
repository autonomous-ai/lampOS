//! The sending side: each message goes once to every subscriber's socket,
//! and the publisher never waits for any of them.
//!
//! A subscriber is somebody else's process. It may be slow, stopped in a
//! debugger or dead, and the publisher is the microphone or the servo loop,
//! which must not notice. So every send is non-blocking and has three ends:
//! taken, dropped because that subscriber's queue is full, or refused
//! because nobody holds the socket any more, in which case its file is
//! removed.
//!
//! Each subscriber gets a sending socket of its own. Linux charges a queued
//! datagram to the socket that sent it until the receiver reads it
//! (`unix(7)`, `SO_SNDBUF`), so with one sending socket for all, a single
//! subscriber that stopped reading would use the whole allowance up and
//! every send to the others would fail with it.
//!
//! A publisher also shows that it is there: an entry in the topic's `pubs/`
//! from its first message, or from the start if it declared the topic, given
//! a new time at most once a second while it publishes (`live.rs`). That is
//! one file touched a second and nothing a subscriber could hold up.
use crate::{
    Bus, FILE_MODE,
    declare::{self, Declaration},
    live, name, wire,
};
use serde::Serialize;
use std::{
    fs,
    io::{self, Write},
    os::unix::{
        fs::{FileTypeExt, OpenOptionsExt, PermissionsExt},
        net::UnixDatagram,
    },
    path::PathBuf,
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// The longest a publisher goes between two refreshes of its entry while it
/// publishes, whatever the lease: a third of the lease if that is less.
const SHOW_EVERY: Duration = Duration::from_secs(1);

/// The most subscribers one publisher sends to on a topic. A subscriber
/// that binds socket after socket would otherwise cost every publisher a
/// descriptor and a send for each.
pub const MOST_SUBSCRIBERS: usize = 64;

/// `EMSGSIZE`, which has no stable `ErrorKind`.
const MESSAGE_TOO_LONG: i32 = if cfg!(target_os = "linux") { 90 } else { 40 };

/// What became of one message.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Published {
    /// Subscribers whose queue took it.
    pub delivered: usize,
    /// Subscribers who did not get it: their queue was full, or the send
    /// failed in some other way that is theirs and not the publisher's.
    pub dropped: usize,
    /// Subscribers found gone on this send, and forgotten.
    pub removed: usize,
}

/// What a publisher has done since it was made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    /// Messages accepted, whether or not anyone was there to take them.
    pub published: u64,
    /// Sends taken, over all subscribers.
    pub delivered: u64,
    /// Sends not taken, over all subscribers.
    pub dropped: u64,
}

/// The same for one subscriber, while the publisher knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubscriberTotals {
    /// The subscriber's socket without `.sock`: its name, process and count.
    pub socket: String,
    pub delivered: u64,
    pub dropped: u64,
}

struct Link {
    name: String,
    path: PathBuf,
    socket: UnixDatagram,
    delivered: u64,
    dropped: u64,
    guarded_dead: bool,
}

enum Sent {
    Delivered,
    Dropped,
    Dead,
    TooLong,
}

impl Link {
    fn send(&self, datagram: &[u8]) -> Sent {
        loop {
            let error = match self.socket.send_to(datagram, &self.path) {
                Ok(_) => return Sent::Delivered,
                Err(error) => error,
            };
            return match error.kind() {
                io::ErrorKind::Interrupted => continue,
                // Nobody holds the socket, or its file has gone.
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound => Sent::Dead,
                _ if error.raw_os_error() == Some(MESSAGE_TOO_LONG) => Sent::TooLong,
                // A full queue is `WouldBlock` on Linux and `ENOBUFS` on
                // macOS. Whatever else it is, it is not ours to wait for.
                _ => Sent::Dropped,
            };
        }
    }
}

/// Sensitive events use separate, bounded storage, never Publisher.datagram.
/// Clearing is best effort in ordinary process memory, not a hard retention
/// deadline and not revocation of bytes copied by the kernel or a receiver.
struct SensitiveDatagram {
    bytes: Box<[u8]>,
    len: usize,
    #[cfg(test)]
    scrubbed: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl SensitiveDatagram {
    fn new() -> Self {
        Self {
            bytes: vec![0; wire::MOST_BYTES].into_boxed_slice(),
            len: 0,
            #[cfg(test)]
            scrubbed: None,
        }
    }

    fn encode(&mut self, header: &wire::Header, payload: &[u8]) -> io::Result<()> {
        let mut output = io::Cursor::new(self.bytes.as_mut());
        serde_json::to_writer(&mut output, header)?;
        output.write_all(b"\n")?;
        output.write_all(payload)?;
        self.len = output.position() as usize;
        Ok(())
    }

    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    fn scrub(&mut self) {
        // Clear the whole allocation, including partially encoded errors.
        self.bytes.fill(0);
        std::hint::black_box(self.bytes.as_mut());
        self.len = 0;
    }
}

impl Drop for SensitiveDatagram {
    fn drop(&mut self) {
        self.scrub();
        #[cfg(test)]
        if let Some(observed) = &self.scrubbed {
            observed.store(self.bytes.iter().all(|byte| *byte == 0), Ordering::SeqCst);
        }
    }
}

fn authority_lost() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "sensitive event authority revoked",
    )
}

/// At most two syscall attempts per destination, each with a fresh check.
/// An arbitrary callback can itself block: callers must provide a pure,
/// bounded check. A syscall/preemption after the check is not revocable.
fn guarded_attempt(
    authorized: &mut impl FnMut() -> bool,
    mut send: impl FnMut() -> io::Result<usize>,
) -> io::Result<Sent> {
    for _ in 0..2 {
        if !authorized() {
            return Err(authority_lost());
        }
        match send() {
            Ok(_) => return Ok(Sent::Delivered),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Ok(match error.kind() {
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound => Sent::Dead,
                    _ if error.raw_os_error() == Some(MESSAGE_TOO_LONG) => Sent::TooLong,
                    _ => Sent::Dropped,
                });
            }
        }
    }
    Ok(Sent::Dropped)
}

/// Shared by the real nonblocking sockets and deterministic adversarial tests.
fn guarded_fanout(
    destinations: usize,
    authorized: &mut impl FnMut() -> bool,
    mut send: impl FnMut(usize) -> io::Result<usize>,
) -> (Published, Option<io::Error>) {
    let mut outcome = Published::default();
    for index in 0..destinations {
        let sent = match guarded_attempt(authorized, || send(index)) {
            Ok(sent) => sent,
            Err(error) => return (outcome, Some(error)),
        };
        match sent {
            Sent::Delivered => outcome.delivered += 1,
            Sent::Dropped => outcome.dropped += 1,
            Sent::Dead => outcome.removed += 1,
            Sent::TooLong => {
                return (
                    outcome,
                    Some(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "sensitive event exceeds socket datagram limit",
                    )),
                );
            }
        }
    }
    (outcome, None)
}

pub struct Publisher {
    header: wire::Header,
    seq: u64,
    directory: PathBuf,
    subs: PathBuf,
    latest: PathBuf,
    writing: PathBuf,
    directory_mode: u32,
    rescan_every: Duration,
    scanned: Option<Instant>,
    links: Vec<Link>,
    datagram: Vec<u8>,
    totals: Totals,
    /// The first thing that went wrong with the root during this publish.
    trouble: Option<io::Error>,
    /// The last publish ended in such trouble, and it has been said.
    failing: bool,
    /// What this publisher declared the topic to be, if it did.
    declaration: Option<Declaration>,
    pubs: PathBuf,
    /// Where this publisher shows that it is alive.
    entry: PathBuf,
    show_every: Duration,
    /// When the entry was last given a new time, or that was last tried.
    shown: Option<Instant>,
    /// The entry has been there, so there may be one to remove at the end.
    listed: bool,
    /// The last try to show it failed.
    unlisted: bool,
    /// Pure cadence reads; the caller owns any fixture-time advancement.
    cadence_now: Box<dyn Fn() -> Instant + Send + Sync>,
    /// The bus's clocks: the wall clock for `at_unix_ms`, and the boot a
    /// monotonic stamp names.
    clocks: crate::clock::Clocks,
}

/// Sixteen hex characters no other publisher instance is likely to have:
/// the process, the time and a count, stirred. Not a secret.
fn boot() -> String {
    static MADE: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64);
    let mut stirred = nanos
        ^ u64::from(process::id()).rotate_left(40)
        ^ MADE
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_mul(0x9e37_79b9_7f4a_7c15);
    stirred = (stirred ^ (stirred >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    stirred = (stirred ^ (stirred >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    format!("{:016x}", stirred ^ (stirred >> 31))
}

impl Publisher {
    pub(crate) fn new(bus: &Bus, topic: &str, kind: wire::Kind, source: &str) -> io::Result<Self> {
        Self::new_with_cadence_clock(bus, topic, kind, source, Instant::now)
    }

    pub(crate) fn new_with_cadence_clock(
        bus: &Bus,
        topic: &str,
        kind: wire::Kind,
        source: &str,
        now: impl Fn() -> Instant + Send + Sync + 'static,
    ) -> io::Result<Self> {
        name::topic(topic)?;
        name::source(source)?;
        let directory = bus.root.join(topic);
        let boot = boot();
        let pubs = directory.join("pubs");
        Ok(Self {
            subs: directory.join("subs"),
            latest: directory.join("latest"),
            // No topic's directory begins with a dot, and no two publishers
            // have one `boot`.
            writing: directory.join(format!(".latest.{boot}.new")),
            entry: live::entry(&pubs, source, &boot),
            pubs,
            directory,
            header: wire::Header {
                topic: topic.into(),
                kind,
                enc: String::new(),
                type_name: None,
                source: source.into(),
                boot,
                seq: 0,
                at_us: 0,
                at_unix_ms: 0,
                expires_ms: None,
                clock: None,
            },
            seq: 0,
            directory_mode: bus.directory_mode,
            rescan_every: bus.rescan_every,
            scanned: None,
            links: Vec::new(),
            datagram: Vec::new(),
            totals: Totals::default(),
            trouble: None,
            failing: false,
            declaration: None,
            show_every: SHOW_EVERY.min(bus.lease / 3),
            shown: None,
            listed: false,
            unlisted: false,
            cadence_now: Box::new(now),
            clocks: bus.clocks.clone(),
        })
    }

    /// A publisher that says what its topic carries. The declaration is
    /// written and the publisher listed before this returns, so a topic can
    /// be seen to have a source that has not spoken yet. It fails if the
    /// topic is already declared as another type or kind, and, unlike a
    /// publisher that declares nothing, if the root cannot be used.
    pub(crate) fn declared(
        bus: &Bus,
        topic: &str,
        declaration: &Declaration,
        source: &str,
    ) -> io::Result<Self> {
        Self::declared_with_cadence_clock(bus, topic, declaration, source, Instant::now)
    }

    pub(crate) fn declared_with_cadence_clock(
        bus: &Bus,
        topic: &str,
        declaration: &Declaration,
        source: &str,
        now: impl Fn() -> Instant + Send + Sync + 'static,
    ) -> io::Result<Self> {
        declaration.check()?;
        let mut publisher =
            Self::new_with_cadence_clock(bus, topic, declaration.kind, source, now)?;
        let place = publisher.directory.display().to_string();
        let named = |error: io::Error| match error.kind() {
            // A disagreement names the topic itself.
            io::ErrorKind::InvalidData => error,
            kind => io::Error::new(kind, format!("bus topic {place}: {error}")),
        };
        declare::write(
            &publisher.directory,
            topic,
            declaration,
            &publisher.header.boot,
            bus.directory_mode,
        )
        .map_err(named)?;
        publisher.header.type_name = Some(declaration.type_name.clone());
        publisher.header.expires_ms = declaration.lifespan_ms;
        publisher.declaration = Some(declaration.clone());
        publisher.show((publisher.cadence_now)()).map_err(named)?;
        Ok(publisher)
    }

    pub fn topic(&self) -> &str {
        &self.header.topic
    }

    /// What this publisher declared the topic to be, if it declared it.
    pub fn declaration(&self) -> Option<&Declaration> {
        self.declaration.as_ref()
    }

    pub fn kind(&self) -> wire::Kind {
        self.header.kind
    }

    /// What marks this instance in every header it sends.
    pub fn boot(&self) -> &str {
        &self.header.boot
    }

    /// Say with every message from now on how long it stays meaningful, or
    /// with `None` say nothing. A publisher that declared a lifespan starts
    /// with that one. A subscriber's `recv` passes over a message that
    /// arrives later than this.
    pub fn expires_after(&mut self, lasting: Option<Duration>) {
        self.header.expires_ms = lasting.map(|lasting| lasting.as_millis() as u64);
    }

    /// Say that every `at_us` this publisher is given from now on is the
    /// kernel's CLOCK_MONOTONIC ([`crate::monotonic_us`], as
    /// `lamp_ipc::monotonic_us` reads it), by stamping each header's `clock`
    /// with this boot's id. A subscriber of the same boot then judges the
    /// message's age and silence by `at_us`, which a time server setting the
    /// wall clock does not move. Only the caller knows what its `at_us` is,
    /// so nothing is stamped unless it says so. Returns whether it is
    /// stamped: not where the bus knows no boot id (macOS), and then the
    /// wall clock judges as before.
    pub fn stamp_monotonic_clock(&mut self) -> bool {
        self.header.clock = self.clocks.boot_id().map(str::to_owned);
        self.header.clock.is_some()
    }

    /// Show that this publisher is alive though it has nothing to publish.
    /// Publishing does the same, so only a publisher of a topic that can be
    /// silent for longer than the lease needs it: without it, its
    /// subscribers are told after a lease of silence that the source has
    /// gone. Costs nothing when the entry was refreshed a moment ago.
    pub fn alive(&mut self) -> io::Result<()> {
        let now = (self.cadence_now)();
        if !self.unlisted && !self.due(now) {
            return Ok(());
        }
        self.show(now).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("bus topic {}: {error}", self.directory.display()),
            )
        })
    }

    /// Whether the entry is old enough to be given a new time.
    fn due(&self, now: Instant) -> bool {
        self.shown
            .is_none_or(|at| now.saturating_duration_since(at) >= self.show_every)
    }

    /// Give the entry a new time, creating it and `pubs/` if need be.
    fn show(&mut self, now: Instant) -> io::Result<()> {
        self.shown = Some(now);
        let done = match live::touch(&self.entry) {
            // The first publisher of the topic.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                crate::make_directory(&self.pubs, self.directory_mode)
                    .and_then(|()| live::touch(&self.entry))
            }
            done => done,
        };
        self.unlisted = done.is_err();
        self.listed |= done.is_ok();
        done
    }

    pub fn totals(&self) -> Totals {
        self.totals
    }

    /// The subscribers this publisher knows of, and how each has fared: the
    /// one that drops is the one that is slow.
    pub fn subscribers(&self) -> Vec<SubscriberTotals> {
        self.links
            .iter()
            .map(|link| SubscriberTotals {
                socket: link.name.trim_end_matches(".sock").into(),
                delivered: link.delivered,
                dropped: link.dropped,
            })
            .collect()
    }

    /// Publish `value` as JSON. `at_us` is the caller's monotonic clock.
    pub fn publish_json<T: Serialize + ?Sized>(
        &mut self,
        at_us: u64,
        value: &T,
    ) -> io::Result<Published> {
        self.publish(at_us, wire::JSON, |datagram| {
            serde_json::to_writer(datagram, value).map_err(io::Error::from)
        })
    }

    /// Publish bytes that are read the way `enc` says. With `json` the
    /// caller answers for their being JSON; with `none` there are none.
    pub fn publish_bytes(&mut self, at_us: u64, enc: &str, bytes: &[u8]) -> io::Result<Published> {
        if enc == wire::NONE && !bytes.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a message with enc none has no payload",
            ));
        }
        self.publish(at_us, enc, |datagram| {
            datagram.extend_from_slice(bytes);
            Ok(())
        })
    }

    /// Discover subscribers and refresh presence for guarded event publication.
    /// This can perform blocking filesystem work. Call it before obtaining a
    /// sensitive payload; it grants no authority and handles no payload bytes.
    /// Guarded publication itself never scans or writes presence/latest files.
    pub fn prepare_guarded_event(&mut self) -> io::Result<()> {
        if self.header.kind != wire::Kind::Event {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "guarded publication requires an event topic",
            ));
        }
        self.trouble = None;
        for link in &self.links {
            if link.guarded_dead {
                let _ = fs::remove_file(&link.path);
            }
        }
        self.links.retain(|link| !link.guarded_dead);
        let now = (self.cadence_now)();
        if self
            .scanned
            .is_none_or(|last| now.saturating_duration_since(last) >= self.rescan_every)
        {
            self.scan(now);
        }
        if self.due(now) || self.unlisted {
            self.show(now)?;
        }
        self.trouble.take().map_or(Ok(()), Err)
    }

    /// Publish sensitive bytes to the event subscribers already discovered.
    /// Call `prepare_guarded_event` separately, before borrowing the payload.
    /// `authorized` must be a pure bounded check of the original permit and
    /// current revocation fence, not a renewed lease. It is checked before and
    /// after encoding and immediately before each send and bounded retry.
    ///
    /// PermissionDenied may follow partial delivery; totals include sends that
    /// already succeeded. No filesystem work occurs after encoding, no latest
    /// value is written, and encoded bytes never enter the reusable datagram.
    /// Temporary encoded storage is scrubbed on return or unwinding. This is
    /// not hard-deadline erasure: thread suspension, syscall/kernel buffering,
    /// receiver copies and the caller's original buffer are outside this API.
    pub fn publish_bytes_guarded(
        &mut self,
        at_us: u64,
        enc: &str,
        bytes: &[u8],
        mut authorized: impl FnMut() -> bool,
    ) -> io::Result<Published> {
        if self.header.kind != wire::Kind::Event {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "guarded publication requires an event topic",
            ));
        }
        name::encoding(enc)?;
        if bytes.len() > wire::MOST_BYTES || enc == wire::NONE && !bytes.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid sensitive event payload length or encoding",
            ));
        }
        if !authorized() {
            return Err(authority_lost());
        }
        let sequence = self
            .seq
            .checked_add(1)
            .ok_or_else(|| io::Error::other("publisher sequence exhausted"))?;
        let mut header = self.header.clone();
        header.enc = enc.into();
        header.seq = sequence;
        header.at_us = at_us;
        header.at_unix_ms = self.clocks.wall_ms();
        let mut datagram = SensitiveDatagram::new();
        datagram.encode(&header, bytes)?;
        if !authorized() {
            return Err(authority_lost());
        }
        // Accepted before fanout: a later revocation must never reuse a
        // sequence already observed by an earlier destination.
        self.seq = sequence;
        self.totals.published += 1;
        let mut destinations = [0usize; MOST_SUBSCRIBERS];
        let mut count = 0;
        for (index, link) in self.links.iter().enumerate() {
            if !link.guarded_dead {
                destinations[count] = index;
                count += 1;
            }
        }
        let mut attempts = [0u8; MOST_SUBSCRIBERS];
        let (outcome, error) = guarded_fanout(count, &mut authorized, |destination| {
            let index = destinations[destination];
            attempts[index] += 1;
            let link = &mut self.links[index];
            let result = link.socket.send_to(datagram.as_bytes(), &link.path);
            match &result {
                Ok(_) => link.delivered += 1,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                    ) =>
                {
                    link.guarded_dead = true
                }
                Err(error)
                    if error.kind() != io::ErrorKind::Interrupted || attempts[index] == 2 =>
                {
                    link.dropped += 1
                }
                _ => {}
            }
            result
        });
        self.totals.delivered += outcome.delivered as u64;
        self.totals.dropped += outcome.dropped as u64;
        match error {
            Some(error) => Err(error),
            None => Ok(outcome),
        }
    }

    fn publish(
        &mut self,
        at_us: u64,
        enc: &str,
        payload: impl FnOnce(&mut Vec<u8>) -> io::Result<()>,
    ) -> io::Result<Published> {
        name::encoding(enc)?;
        self.header.enc.clear();
        self.header.enc.push_str(enc);
        // The count moves on only once the message has been accepted: a
        // refused one must not look like a lost one.
        self.header.seq = self.seq + 1;
        self.header.at_us = at_us;
        self.header.at_unix_ms = self.clocks.wall_ms();
        let mut datagram = std::mem::take(&mut self.datagram);
        datagram.clear();
        let done = serde_json::to_writer(&mut datagram, &self.header)
            .map_err(io::Error::from)
            .and_then(|()| {
                datagram.push(b'\n');
                payload(&mut datagram)
            })
            .and_then(|()| self.deliver(&datagram));
        self.datagram = datagram;
        done
    }

    fn deliver(&mut self, datagram: &[u8]) -> io::Result<Published> {
        if datagram.len() > wire::MOST_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "a message of {} bytes with its header is over the bus's {}: \
                     send media in smaller pieces, or keep it in a file and publish where",
                    datagram.len(),
                    wire::MOST_BYTES
                ),
            ));
        }
        self.trouble = None;
        let now = (self.cadence_now)();
        let looked = self
            .scanned
            .is_none_or(|at| now.saturating_duration_since(at) >= self.rescan_every);
        if looked {
            self.scan(now);
        }
        // Once a second, and after a failure with every look at the root, so
        // that a root which stays unusable is said once like any other
        // trouble with it.
        if (self.due(now) || (self.unlisted && looked))
            && let Err(error) = self.show(now)
        {
            self.trouble.get_or_insert(error);
        }
        let mut outcome = Published::default();
        self.send(datagram, 0, &mut outcome)?;
        if outcome.removed > 0 {
            // Whatever replaced the dead one may already be there, and
            // should not wait for the next look to hear this message.
            let found = self.scan(now);
            self.send(datagram, self.links.len() - found, &mut outcome)?;
        }
        self.seq += 1;
        self.totals.published += 1;
        self.totals.delivered += outcome.delivered as u64;
        self.totals.dropped += outcome.dropped as u64;
        if self.header.kind == wire::Kind::Latest
            && let Err(error) = self.keep(datagram)
        {
            self.trouble.get_or_insert(error);
        }
        // A root that cannot be used is said once, not with every one of
        // fifty messages a second, and again only after a look at it found
        // nothing wrong.
        match self.trouble.take() {
            None => {
                self.failing &= !looked;
                Ok(outcome)
            }
            Some(error) if !self.failing => {
                self.failing = true;
                Err(io::Error::new(
                    error.kind(),
                    format!("bus topic {}: {error}", self.directory.display()),
                ))
            }
            Some(_) => Ok(outcome),
        }
    }

    /// Read `subs/` again: forget whoever's socket file has gone and give
    /// each newcomer a socket to be sent on. Says how many are new; they
    /// are the last ones in `links`.
    fn scan(&mut self, now: Instant) -> usize {
        self.scanned = Some(now);
        let mut present = Vec::new();
        match fs::read_dir(&self.subs) {
            Ok(entries) => {
                for entry in entries {
                    let entry = match entry {
                        Ok(entry) => entry,
                        Err(error) => {
                            // Half a listing is no reason to forget anyone.
                            self.trouble.get_or_insert(error);
                            return 0;
                        }
                    };
                    if let Ok(name) = entry.file_name().into_string()
                        && name.ends_with(".sock")
                        && entry.file_type().is_ok_and(|kind| kind.is_socket())
                    {
                        present.push(name);
                    }
                }
            }
            // Nobody has subscribed yet.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                self.trouble.get_or_insert(error);
                return 0;
            }
        }
        present.sort_unstable();
        self.links
            .retain(|link| present.binary_search(&link.name).is_ok());
        let mut found = 0;
        for name in present {
            if self.links.iter().any(|link| link.name == name) {
                continue;
            }
            if self.links.len() == MOST_SUBSCRIBERS {
                self.trouble.get_or_insert(io::Error::other(format!(
                    "more than {MOST_SUBSCRIBERS} subscribers: the rest get nothing"
                )));
                break;
            }
            let socket = UnixDatagram::unbound().and_then(|socket| {
                socket.set_nonblocking(true)?;
                Ok(socket)
            });
            match socket {
                Ok(socket) => {
                    self.links.push(Link {
                        path: self.subs.join(&name),
                        name,
                        socket,
                        delivered: 0,
                        dropped: 0,
                        guarded_dead: false,
                    });
                    found += 1;
                }
                Err(error) => {
                    self.trouble.get_or_insert(error);
                    break;
                }
            }
        }
        found
    }

    /// Send to every subscriber from `from` on. An error is the message's
    /// own: the operating system takes no datagram this long.
    fn send(&mut self, datagram: &[u8], from: usize, outcome: &mut Published) -> io::Result<()> {
        let mut at = from;
        while let Some(link) = self.links.get_mut(at) {
            match link.send(datagram) {
                Sent::Delivered => {
                    link.delivered += 1;
                    outcome.delivered += 1;
                }
                Sent::Dead => {
                    let _ = fs::remove_file(&link.path);
                    self.links.remove(at);
                    outcome.removed += 1;
                    continue;
                }
                // Every sending socket here is alike, so what is too long
                // for the first is too long for all and nobody has had it.
                Sent::TooLong if outcome.delivered + outcome.dropped == 0 => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "the operating system takes no datagram of {} bytes on a local \
                             socket here, less than the bus's {} (macOS: 2048 unless \
                             net.local.dgram.maxdgram is raised)",
                            datagram.len(),
                            wire::MOST_BYTES
                        ),
                    ));
                }
                Sent::Dropped | Sent::TooLong => {
                    link.dropped += 1;
                    outcome.dropped += 1;
                }
            }
            at += 1;
        }
        Ok(())
    }

    /// Put the message in the topic's `latest`. A reader sees the value
    /// before or this one, never part of either.
    fn keep(&self, datagram: &[u8]) -> io::Result<()> {
        match self.replace(datagram) {
            // The first value of a topic nobody has subscribed to.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                crate::make_directory(&self.directory, self.directory_mode)?;
                self.replace(datagram)
            }
            done => done,
        }
    }

    fn replace(&self, datagram: &[u8]) -> io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(FILE_MODE)
            .open(&self.writing)?;
        file.write_all(datagram)?;
        // The mode asked for, whatever the process's umask took from it.
        file.set_permissions(fs::Permissions::from_mode(FILE_MODE))?;
        drop(file);
        fs::rename(&self.writing, &self.latest)
    }
}

impl Drop for Publisher {
    /// The value and the declaration stay for whoever subscribes next: only
    /// a half-written value is cleared away, and this publisher's entry, so
    /// that it is seen to have gone at once and not a lease later.
    fn drop(&mut self) {
        if self.header.kind == wire::Kind::Latest && self.seq > 0 {
            let _ = fs::remove_file(&self.writing);
        }
        if self.listed {
            let _ = fs::remove_file(&self.entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_publisher_instance_gets_its_own_sixteen_hex_characters() {
        let made: HashSet<String> = (0..2_000).map(|_| boot()).collect();
        assert_eq!(made.len(), 2_000);
        for boot in made {
            assert_eq!(boot.len(), 16);
            assert!(
                boot.bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            );
        }
    }
}

#[cfg(test)]
#[path = "publisher_clock_tests.rs"]
mod clock_tests;

#[cfg(test)]
#[path = "guarded_publish_tests.rs"]
mod guarded_tests;
